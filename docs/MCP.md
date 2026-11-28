# The MCP server

`ecad-mcp` lets an AI assistant work with one Electric Studio project through the
[Model Context Protocol](https://modelcontextprotocol.io). The assistant can read the
schematic, run the checks, the release check and the reports, and **prepare proposals**:
change sets with a preview of what they would do. It cannot change the project. A person
reviews each proposal in the desktop app, approves the exact proposal there, and the app
applies it as one transaction, one undo step (AI0, D18, D32).

```sh
cargo build --release -p ecad-mcp
ecad-mcp my-project
```

`--write` no longer exists: the server refuses it (exit status 2). There is no setting that
lets an assistant apply changes.

The server talks JSON-RPC over stdin and stdout (MCP's stdio transport). The client starts
it. Log lines go to stderr.

### Explicit construction records (AI0)

The registry also exposes named `prepare_*_exact` and other construction operations for
building projects with stable UUID references in a multi-operation proposal. Use
`tools/list` for their schemas, and `prepare_change` to submit the ordered operations;
each structured operation has a typed `record` argument. These operations cover
pages, devices, electrical nodes, drawings with explicit bindings, wire specifications,
custom symbols, catalogue parts, cables, location boxes, mounting plates, panel items
and sheet/title definitions. They do not accept raw model commands or an automation-policy
override. Unknown fields (including nested fields) are refused; the project model checks
the engineering references and identifier rules. An empty page removal retires its name.
The CLI accepts the same structured record as JSON in `record=<json>`.

The construction proposal is reviewed and approved in the app, not by MCP. Global
changes and any detected safety impact require their separate confirmations. The
full-conveyor conformance test builds and undoes an entire reference project through
this path; it does not confer electrical or functional safety certification.

## Connecting a client

Claude Code:

```sh
claude mcp add electric-studio -- /path/to/ecad-mcp /path/to/my-project
```

Claude Desktop, in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "electric-studio": {
      "command": "/path/to/ecad-mcp",
      "args": ["/path/to/my-project"]
    }
  }
}
```

## Tools

The tools are generated from the engineering registry (`ecad_editor::registry`), which the
editor and the CLI share. `ecad operations` lists the same registry.

Queries:

| Tool | What it gives |
|---|---|
| `project_overview` | Name, pages, counts, errors and warnings |
| `list_devices` | Every device with description, products and drawings; optionally one page |
| `device_details` | One device's drawings, properties, and each point's wire |
| `list_wires` | The wire list, optionally for one device |
| `run_checks` | Errors and warnings with their locations |
| `release_check` | The shared release-validation policy: whether the project can be released, and why not |
| `report` | A report as CSV |
| `list_symbols` | Symbols that can be placed, with their points |
| `numbering` | Numbering policies, capacities, reserved and retired identifiers |

Proposals. Each `prepare_<operation>` tool prepares a change set of one operation; it also
takes an optional `title` and `rationale`. The operations are:

- `renumber_devices`, `renumber_wires`, `renumber_pages` and `renumber_terminals` (global);
- `place_symbol`, `connect`, `add_wire`, `delete_wire`, `delete_device`, `edit_device`,
  `set_plc_channel` and `add_page`;
- `reserve`, `unreserve`, `retire` and `reclaim`;
- `mark_safety_related`. An assistant can mark a device safety-related, but only a person can
  unmark one.

| Tool | What it does |
|---|---|
| `prepare_change` | One change set of several operations, approved and applied as one transaction |
| `preview_change` | A stored proposal's preview against the project now |
| `list_proposals` | Proposals with their status: pending, approved, applied or rejected |
| `reload` | Load the folder again, as last committed |

A preview lists:

- the affected devices, terminals and wires, with their old and new identifiers;
- the pages and other changes;
- the check result before and after;
- the direct and indirect safety impact.

Imports (`import_tia`, `import_dxf`, `remove_underlay`) are not offered to assistants.

Names, not ids: a page is its name (or `#2`), a device its tag, a point `TAG:POINT`.
Arguments are checked against each tool's schema. Every tool is marked non-destructive,
because none changes the project. A proposal is stored under `proposals/` in the project
folder, committed in the folder's manifest.

## Who the assistant is

The server records the client as an *assistant* actor. It uses the `clientInfo` name and
version from `initialize` and a session id. Those are what the client claims, not a proof of
identity (D31). Only trusted host code assigns actors: the app records a person, and the CLI
records the OS user, host, command line and version. A proposal cannot claim to be from a
person.

## Approval and applying

In the desktop app, **Proposals** lists the stored proposals. Opening one shows the affected
objects with their old and new identifiers, the pages, the check changes and the safety
impact. Changes with safety impact, and global operations, need an explicit confirmation.

- **Approve and apply** checks the proposal against the folder, then applies it and commits it
  in one transaction. The checks are:
  - its hash is the one reviewed;
  - its base is the committed revision;
  - the folder has no outside changes;
  - it still previews as reviewed;
  - an assistant's proposal adds no check errors.

  The approval is recorded as used, and a change record names the assistant, the person and
  the result. One undo restores the state before.
- **Reject** records the rejection.
- **Approve for the CLI** is only for proposals prepared from the command line (`ecad apply`).
  An assistant's proposal is applied in the app only.

A proposal that no longer matches is refused, nothing changes, and the refusal is recorded.
That covers a stale proposal (the project moved on, even if its content returned to an earlier
value), a modified one, and a replayed one.

## The folder

`.ecad/state.json` is the committed manifest. It lists every engineering and workflow file
with its SHA-256, and `.ecad/objects/` keeps the last committed contents. The manifest is
checked at open, preview, apply and save:

- A file changed outside Electric Studio blocks applying and saving until a person reconciles
  it in the app. They either restore the committed files, with the changed ones moved to
  `.ecad/quarantine/` and never deleted, or accept the changes as a new revision, which
  revokes outstanding approvals.
- The same applies to a missing, damaged or added file.
- Unexpected files are listed and ignored.
- A project saved before AI0 gets its first manifest when it is opened. No engineering file is
  rewritten.

`crates/ecad-mcp/tests/client/check.py` drives the server with the official MCP Python SDK. CI
runs it, and then approves the client's proposal in a desktop session and undoes it
(`tests/pilot.rs`).
