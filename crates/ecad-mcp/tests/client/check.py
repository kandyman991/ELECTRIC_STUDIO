"""Drive ecad-mcp with the official MCP Python SDK, as a real client does.

usage: python check.py <path to ecad-mcp> <path to ecad> <scratch dir>

Exits non-zero on the first thing that isn't as expected.
"""

import asyncio
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


async def run(server, ecad, scratch):
    project = os.path.join(scratch, "plc")
    subprocess.run([ecad, "new-sample", project, "plc"], check=True, capture_output=True)

    params = StdioServerParameters(command=server, args=[project, "--write"])
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            init = await session.initialize()
            assert field(init, "serverInfo", "server_info").name == "electric-studio", init
            print("protocol", field(init, "protocolVersion", "protocol_version"))

            tools = {t.name: t for t in (await session.list_tools()).tools}
            assert {"run_checks", "connect", "save", "reload"} <= set(tools), sorted(tools)
            assert field(tools["save"].annotations, "destructiveHint", "destructive_hint") is True
            assert field(tools["list_wires"].annotations, "readOnlyHint", "read_only_hint") is True

            overview = structured(await session.call_tool("project_overview", {}))
            assert overview["errors"] == 0 and overview["can_edit"] is True, overview

            csv = (await session.call_tool("report", {"name": "plc-io-list"})).content[0].text
            assert csv.startswith("card,channel,kind,address"), csv

            bad = await session.call_tool("place_symbol", {"symbol": "iec.coil", "page": "#1", "x": 1, "y": 1, "device": 5})
            assert failed(bad) and "must be a string" in bad.content[0].text, bad

            placed = structured(await session.call_tool("place_symbol", {"symbol": "iec.coil", "page": "#1", "x": 300, "y": 120}))
            other = structured(await session.call_tool("place_symbol", {"symbol": "iec.coil", "page": "#1", "x": 340, "y": 120}))
            wire = structured(await session.call_tool("connect", {"from": placed["tag"] + ":A2", "to": other["tag"] + ":A1"}))
            assert wire["unsaved_changes"] is True, wire
            await session.call_tool("set_plc_channel", {"card": "-KF1", "channel": "0", "symbol": "Door_Shut"})
            saved = structured(await session.call_tool("save", {}))
            assert saved["saved"] is True, saved

    # What the server saved is what the command line sees.
    io = subprocess.run([ecad, "report", project, "plc-io-list"], check=True, capture_output=True, text=True).stdout
    assert "Door_Shut" in io, io
    check = subprocess.run([ecad, "check", project, "--json"], capture_output=True, text=True)
    assert json.loads(check.stdout)["devices"] == overview["devices"] + 2, check.stdout
    print("ok")


if __name__ == "__main__":
    asyncio.run(run(*sys.argv[1:4]))
