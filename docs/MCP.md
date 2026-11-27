# The MCP server

`ecad-mcp` lets an AI assistant work on one Electric Studio project through the
[Model Context Protocol](https://modelcontextprotocol.io): it reads the schematic, runs the
checks and reports, and, if you allow it, edits the project with the same commands and undo
history as the editor.

```sh
cargo build --release -p ecad-mcp
ecad-mcp my-project           # read only
ecad-mcp my-project --write   # edits allowed too
```

It talks JSON-RPC over stdin and stdout (MCP's stdio transport); the client starts it. Log
lines go to stderr.

## Connecting a client

Claude Code:

```sh
claude mcp add electric-studio -- /path/to/ecad-mcp /path/to/my-project --write
```

Claude Desktop, in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "electric-studio": {
      "command": "/path/to/ecad-mcp",
      "args": ["/path/to/my-project", "--write"]
    }
  }
}
```

Leave out `--write` to let the assistant look but not touch.

## Tools

Read (always offered):

| Tool | What it gives |
|---|---|
| `project_overview` | Name, pages, counts, errors and warnings, unsaved changes |
| `list_devices` | Every device with description, parts and where it's drawn; optionally one page |
| `device_details` | One device's drawings, properties and each point's wire and far end |
| `list_wires` | The wire list, optionally for one device |
| `run_checks` | Errors and warnings with their page locations |
| `report` | Any report as CSV (wire list, BOM, terminal diagram, PLC I/O list, …) |
| `list_symbols` | Symbols that can be placed, with their points |

Edit (only with `--write`), one undo step each:

| Tool | What it does |
|---|---|
| `place_symbol` | Place a symbol at x, y mm on a page: a new device, or another drawing of one |
| `connect` | Draw a wire between two free points, `-S1:14` to `-K1:A1` |
| `delete_wire` | Delete the wire at a point |
| `delete_device` | Delete a device with its drawings, wires and panel footprint |
| `edit_device` | New tag and/or description |
| `set_plc_channel` | A PLC channel's address, symbol and comment |
| `undo`, `redo` | As in the editor |
| `save` | Write the project folder |

Names, not ids: a page is its name (or `#2` for the second page), a device its tag, a point
`TAG:POINT` as drawn. Positions are mm on the page and snap to the 4 mm grid. The model's
rules hold as in the editor: a point takes one wire, tags and PLC addresses are unique, and a
refused edit comes back as a tool error saying why.

## Saving, and the editor

Edits stay in the server until `save`; closing it without saving leaves the folder as it was.
The server and the editor each hold their own copy of the project, so don't edit the same
project in both at once: whichever saves last wins.
