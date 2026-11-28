"""Drive ecad-mcp with the official MCP Python SDK, as a real client does (AI0).

usage: python check.py <path to ecad-mcp> <path to ecad> <scratch dir>

The client queries, prepares the pilot proposal (S0 device renumbering on the conveyor) and
previews it; nothing it does changes the project. It writes <scratch>/pilot.json for the next
CI step, which approves that exact proposal in the desktop session
(`cargo test -p ecad-mcp --test pilot -- --ignored`). Exits non-zero on the first thing that
isn't as expected.
"""

import asyncio
import hashlib
import json
import os
import subprocess
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


def field(obj, camel, snake):
    """An SDK field: camelCase in SDK 1.x, snake_case in 2.x."""
    return getattr(obj, snake) if hasattr(obj, snake) else getattr(obj, camel)


def failed(result):
    return field(result, "isError", "is_error")


def structured(result):
    assert not failed(result), result.content[0].text
    return field(result, "structuredContent", "structured_content") or json.loads(result.content[0].text)


def folder_hash(project):
    """The engineering files, to show the client changed none of them."""
    paths = []
    for root, dirs, files in os.walk(project):
        dirs[:] = [d for d in dirs if d not in ("proposals", "approvals", "changes", ".ecad")]
        paths += [os.path.join(root, name) for name in files if name.endswith(".json")]
    h = hashlib.sha256()
    for path in sorted(paths):
        h.update(path.encode())
        h.update(open(path, "rb").read())
    return h.hexdigest()


async def run(server, ecad, scratch):
    project = os.path.join(scratch, "pilot")
    os.makedirs(project, exist_ok=True)
    # The conveyor with two tags out of drawing order (written by the pilot test helper).
    env = dict(os.environ, ECAD_PILOT_PROJECT=project)
    subprocess.run(
        ["cargo", "test", "-q", "-p", "ecad-mcp", "--test", "pilot", "--", "--ignored", "write_the_pilot_project"],
        check=True, env=env, capture_output=True,
    )
    before = folder_hash(project)

    # --write is gone.
    gone = subprocess.run([server, project, "--write"], capture_output=True, text=True)
    assert gone.returncode == 2 and "removed" in gone.stderr, gone.stderr

    params = StdioServerParameters(command=server, args=[project])
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            init = await session.initialize()
            assert field(init, "serverInfo", "server_info").name == "electric-studio", init
            print("protocol", field(init, "protocolVersion", "protocol_version"))

            tools = {t.name: t for t in (await session.list_tools()).tools}
            assert {"run_checks", "prepare_renumber_devices", "prepare_change", "preview_change"} <= set(tools), sorted(tools)
            assert not {"save", "undo", "connect"} & set(tools), sorted(tools)
            assert field(tools["list_wires"].annotations, "readOnlyHint", "read_only_hint") is True
            for t in tools.values():
                assert field(t.annotations, "destructiveHint", "destructive_hint") is False, t.name

            overview = structured(await session.call_tool("project_overview", {}))
            assert overview["errors"] == 0 and overview["can_edit"] is False, overview

            bad = await session.call_tool("prepare_place_symbol", {"symbol": "iec.coil", "page": "#1", "x": 1, "y": 1, "device": 5})
            assert failed(bad) and "must be a string" in bad.content[0].text, bad

            prepared = structured(await session.call_tool(
                "prepare_renumber_devices", {"title": "Renumber devices", "rationale": "tags in drawing order"}))
            proposal = prepared["proposal"]
            renamed = {(d["before"], d["after"]) for d in prepared["impact"]["devices"]}
            assert ("-K7", "-K2") in renamed, renamed

            preview = structured(await session.call_tool("preview_change", {"id": proposal}))
            assert preview["status"] == "pending" and preview["stale"] is False, preview
            assert preview["impact"]["safety_direct"], preview

            listed = structured(await session.call_tool("list_proposals", {}))
            assert any(p["id"] == proposal for p in listed["proposals"]), listed

    # Nothing applied: the engineering files are as they were; the proposal is pending.
    assert folder_hash(project) == before
    out = subprocess.run([ecad, "proposals", project, "--json"], check=True, capture_output=True, text=True).stdout
    assert any(p["id"] == proposal and p["status"] == "pending" for p in json.loads(out)["proposals"]), out
    with open(os.path.join(scratch, "pilot.json"), "w") as f:
        json.dump({"project": project, "proposal": proposal, "ecad": os.path.abspath(ecad)}, f)
    print("ok", proposal)


if __name__ == "__main__":
    asyncio.run(run(*sys.argv[1:4]))
