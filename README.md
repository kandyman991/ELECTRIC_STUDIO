# Electric Studio

Electrical CAD for control panels and machine wiring. One electrical model is the source of
truth; schematics, panel layouts and manufacturing reports are generated from it.

Drawing, DXF/DWG/PDF output and rendering come from the Rust crates of
[CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0), used as libraries.

> Status: **M0 (foundation)**. Early development; nothing is usable yet.

## Try it

```sh
cargo run -p ecad-cli -- demo out
```

This writes `out/page-1.dxf`, `out/page-1.png` and `out/wire-list.csv` for a small sample
circuit (L+ → pushbutton -S1 → coil -K1 → M). The first build takes several minutes.

## Crates

| Crate | Responsibility |
| --- | --- |
| `ecad-model` | Project, pages, symbols, devices, functions, connections, routing |
| `ecad-view` | Model pages to CADCraft drawings; DXF and PNG export. The only CADCraft adapter |
| `ecad-reports` | Reports as queries on the model (wire list so far) |
| `ecad-cli` | The `ecad` command |

Planned: `ecad-rules`, `ecad-project` (storage), `ecad-symbols`, `ecad-parts`, `ecad-app`
(editor), `ecad-mcp`. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Milestones

| Milestone | Scope |
| --- | --- |
| **M0 Foundation** | Workspace, crates, CI, golden-file tests, feasibility test |
| M1 One-page editor | Place symbols, draw wires, device tags, save and load; UI technology decided |
| M2 Multi-page projects | Pages, title blocks, cross-references, potentials, interruption points |
| M3 Parts and reports | Parts catalogue, BOM, wire list, terminal diagram, PLC I/O list |
| M4 Checks and output | Live checks, PDF/DXF/DWG export, headless CLI |
| *MVP release gate* | *One real panel drawn end to end; BOM and wire list match it; checks clean* |
| M5 Panel layout | 2D mounting plate, DIN rails, ducts, parts at scale, cables |
| M6 Integrations | HMI Studio tag exchange, TIA Portal tag table, MCP server, DXF import |

## Contributing

See [AGENTS.md](AGENTS.md).
