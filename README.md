# Electric Studio

Electrical CAD for control panels and machine wiring. One electrical model is the source of
truth; schematics, panel layouts and manufacturing reports are generated from it.

Drawing, DXF/DWG/PDF output and rendering come from the Rust crates of
[CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0), used as libraries.

> Status: **M1a (editing core)**. Early development; there is no editor window yet.

## Try it

```sh
cargo run -p ecad-cli -- demo out
```

This writes `out/page-1.dxf`, `out/page-1.png` and `out/wire-list.csv` for a small sample
circuit (L+ → pushbutton -S1 → coil -K1 → M). The first build takes several minutes.

Projects are folders of JSON files:

```sh
cargo run -p ecad-cli -- new-sample my-project     # write the sample as a project folder
cargo run -p ecad-cli -- check my-project          # load it and summarise
cargo run -p ecad-cli -- export my-project out     # DXF, PNG and wire list for every page
```

## Crates

| Crate | Responsibility |
| --- | --- |
| `ecad-model` | Project, pages, symbols, devices, functions, connections, routing, undoable commands and history |
| `ecad-project` | Loading and saving a project folder |
| `ecad-view` | Model pages to CADCraft drawings; DXF, PNG and plain display primitives. The only CADCraft adapter |
| `ecad-editor` | UI-independent editing controller: tools, hit-testing, snapping, undo, overlays |
| `ecad-reports` | Reports as queries on the model (wire list so far) |
| `ecad-cli` | The `ecad` command |

Planned: `ecad-rules`, `ecad-symbols`, `ecad-parts`, the editor shell (`ecad-app`), `ecad-mcp`. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Milestones

| Milestone | Scope |
| --- | --- |
| M0 Foundation | Workspace, crates, CI, golden-file tests, feasibility test |
| **M1 One-page editor** | M1a editing core (this) · M1b egui and Tauri test shells, UI decided · M1c finished editor |
| M2 Multi-page projects | Pages, title blocks, cross-references, potentials, interruption points |
| M3 Parts and reports | Parts catalogue, BOM, wire list, terminal diagram, PLC I/O list |
| M4 Checks and output | Live checks, PDF/DXF/DWG export, headless CLI |
| *MVP release gate* | *One real panel drawn end to end; BOM and wire list match it; checks clean* |
| M5 Panel layout | 2D mounting plate, DIN rails, ducts, parts at scale, cables |
| M6 Integrations | HMI Studio tag exchange, TIA Portal tag table, MCP server, DXF import |

## Contributing

See [AGENTS.md](AGENTS.md).
