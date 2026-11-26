# Electric Studio

Electrical CAD for control panels and machine wiring. One electrical model is the source of
truth; schematics, panel layouts and manufacturing reports are generated from it.

Drawing, DXF/DWG/PDF output and rendering come from the Rust crates of
[CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0), used as libraries.

> Status: **M4b**: drawing export (one PDF for the project; DXF, DWG and PNG per page), live
> checks, multi-page schematics, parts, BOM, terminal strips and PLC I/O. Early development: an eleven-symbol placeholder library.

## Try it

```sh
cargo run -p ecad-cli -- demo out
```

This writes `out/page-1.dxf`, `out/page-1.png` and `out/wire-list.csv` for a small sample
circuit (L+ → pushbutton -S1 → coil -K1 → M). The first build takes several minutes.

Projects are folders of JSON files:

```sh
cargo run -p ecad-cli -- new-sample my-project     # write the sample as a project folder
cargo run -p ecad-cli -- new-sample my-project plc    # or: start, contacts, potentials, terminals
cargo run -p ecad-cli -- check my-project          # summarise and list errors and warnings (exit 1 on errors)
cargo run -p ecad-cli -- export my-project out     # drawings.pdf, DXF/DWG/PNG per page, and the reports
```

## Try the editor

```sh
cargo run -p ecad-app            # or: cargo run -p ecad-app -- path/to/project
```

It opens `~/ElectricStudio/Sample` (created with the sample circuit if missing), or the project
folder given as an argument. Keys and tools are listed in docs/UI-DECISION.md. Windows builds
are attached to each CI run of the "Windows build" workflow (unsigned for now).

## Crates

| Crate | Responsibility |
| --- | --- |
| `ecad-model` | Project, pages, symbols, devices, functions, connections, routing, undoable commands and history |
| `ecad-project` | Loading and saving a project folder |
| `ecad-view` | Model pages to CADCraft drawings; project PDF, DXF, DWG, PNG and plain display primitives. The only CADCraft adapter |
| `ecad-editor` | UI-independent editing controller and session: tools, hit-testing, snapping, undo, overlays, pan/zoom, saving |
| `ecad-rules` | Checks on the model (unconnected points, potential clashes, missing parts…), live in the editor and in `ecad check` |
| `ecad-reports` | Reports as queries on the model: wire list, BOM, terminal diagram, PLC I/O list |
| `ecad-cli` | The `ecad` command |
| `apps/ecad-app` | The editor application (`electric-studio`): a thin egui shell over `ecad_editor::Session` |

Planned: `ecad-symbols`, `ecad-parts`, `ecad-mcp`. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Milestones

| Milestone | Scope |
| --- | --- |
| M0 Foundation | Workspace, crates, CI, golden-file tests, feasibility test |
| M1 One-page editor | M1a editing core · M1b UI chosen: egui · M1c Open/New, unsaved-changes prompt, readable labels, side-panel edits |
| M2 Multi-page projects | M2a pages, sheet, title block · M2b cross-references · M2c potentials, junctions, interruption points |
| **M3 Parts and reports** | M3a parts catalogue, BOM, report export · M3b terminal strips, terminal diagram · M3c PLC channels, PLC I/O list |
| **M4 Checks and output** | M4a live checks, checks panel, `ecad check` · M4b PDF/DXF/DWG export (this) · M4c headless CLI |
| *MVP release gate* | *One real panel drawn end to end; BOM and wire list match it; checks clean* |
| M5 Panel layout | 2D mounting plate, DIN rails, ducts, parts at scale, cables |
| M6 Integrations | HMI Studio tag exchange, TIA Portal tag table, MCP server, DXF import |

## Contributing

See [AGENTS.md](AGENTS.md).
