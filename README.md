# Electric Studio

Electrical CAD for control panels and machine wiring. One electrical model is the source of
truth; schematics, panel layouts and manufacturing reports are generated from it.

Drawing, DXF/DWG/PDF output and rendering come from the Rust crates of
[CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0), used as libraries.

> Status: **M4 complete**: live checks, drawing export (one PDF for the project; DXF, DWG and
> PNG per page) and the `ecad` command for scripts and CI, on top of multi-page schematics,
> parts, BOM, terminal strips and PLC I/O. MVP gate: a real 10-page diagram redrawn as the reference project ([results](docs/MVP-GATE.md)). Early development: a starter IEC library of about 40 symbols.

## Try it

```sh
cargo run -p ecad-cli -- demo out
```

This writes `out/drawings.pdf`, `out/page-1.dxf`, `.dwg`, `.png` and the reports for a small sample
circuit (L+ → pushbutton -S1 → coil -K1 → M). The first build takes several minutes.

Projects are folders of JSON files:

```sh
cargo run -p ecad-cli -- new-sample my-project     # write the sample as a project folder
cargo run -p ecad-cli -- new-sample my-project power  # or: start, contacts, potentials, terminals, plc, library
cargo run -p ecad-cli -- check my-project          # summarise and list errors and warnings (exit 1 on errors)
cargo run -p ecad-cli -- export my-project out     # drawings.pdf, DXF/DWG/PNG per page, and the reports
```

## Use it in scripts and CI

The `ecad` command does everything the editor's checks and exports do, without a window:

```sh
ecad check my-project                      # exit 1 on errors
ecad check my-project --strict             # exit 1 on warnings too
ecad check my-project --json               # the findings as JSON, each with its page, location and target
ecad export my-project out --only pdf,reports   # any of pdf, dxf, dwg, png, reports
ecad import-dxf my-project 3 old-page-3.dwg     # an old drawing behind page 3, to redraw over
ecad import-tia my-project tags.xlsx       # PLC symbols and comments renamed in TIA Portal, by address
ecad report my-project bom > bom.csv       # one report on stdout: wire-list, bom, terminal-diagram,
                                           # plc-io-list, cable-list, table-of-contents,
                                           # devices-without-parts, panel-cut-list, wire-totals
ecad --help
```

Exit codes: 0 done, 1 the check failed, 2 the command couldn't run (bad arguments, a project
that doesn't load, a file that can't be written). The project folder diffs cleanly in git, so a
CI job can run `ecad check --strict` and `ecad export` on every push.

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
| `ecad-exchange` | Tag exchange with PLC tools: the TIA Portal PLC tag table (.xlsx), out and back |
| `ecad-mcp` | MCP server for one project: AI assistants read it and, with `--write`, edit it with undo (docs/MCP.md) |
| `ecad-cli` | The `ecad` command: check, export and reports for scripts and CI |
| `apps/ecad-app` | The editor application (`electric-studio`): a thin egui shell over `ecad_editor::Session` |

Planned: `ecad-symbols`, `ecad-parts`, `ecad-mcp`. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Milestones

| Milestone | Scope |
| --- | --- |
| M0 Foundation | Workspace, crates, CI, golden-file tests, feasibility test |
| M1 One-page editor | M1a editing core · M1b UI chosen: egui · M1c Open/New, unsaved-changes prompt, readable labels, side-panel edits |
| M2 Multi-page projects | M2a pages, sheet, title block · M2b cross-references · M2c potentials, junctions, interruption points |
| **M3 Parts and reports** | M3a parts catalogue, BOM, report export · M3b terminal strips, terminal diagram · M3c PLC channels, PLC I/O list |
| **M4 Checks and output** | M4a live checks, checks panel, `ecad check` · M4b PDF/DXF/DWG export · M4c `ecad` for scripts and CI |
| *MVP release gate* | *G1a sheet grid with rows, references like `A3/2F`, full title block · G1b tag format and wire numbers per potential · G2 multi-pole devices and contact tables · G3 symbol library and black boxes · G4a wire details, cables, cable list, BOM code and pages, table of contents · G4b connectors and location boxes · reference project redrawn end to end (`ecad new-sample <dir> reference`)* |
| *Gate check* | *One real panel drawn end to end; BOM and wire list match it; checks clean. Done: 0 errors, 28 devices match the BOM exactly; gaps listed in [docs/MVP-GATE.md](docs/MVP-GATE.md)* |
| *G5 Gate gaps* | *G5a wire tidying: drag a wire's middle run, Straighten, Tidy wires, new wires avoid clashes, wire-overlap checks · G5b changeover contacts, contact numbers from the parts' contact blocks · G5c parts per terminal type, black-box bridges, PLC wire numbers · G6 mirrored symbols, dashed PE wires* |
| **M5 Panel layout** | M5a layout pages: mounting plate at scale, DIN rails, ducts, device footprints from part sizes, checks, cut list, reference layout L7 · M5b editing the layout: place unplaced devices, snap to rails, draw rails and ducts, layout pages in the app · M5c wire lengths through the ducts in the wire list, wire totals, cut list in the exports |
| M6 Integrations | M6a TIA Portal PLC tag table: export, symbols and comments back by address · M6b MCP server · M6c DXF/DWG underlay to redraw over. No HMI tag exchange: HMI tags come in from TIA Portal |
| *Proposed next* | *M7 components library with manufacturer import (CSV/Excel, BMEcat/ETIM, ECLASS Advanced) · M8 annotations and device data · M9 generated pages (cover, contents, BOM, terminal strips, legend) · M10 symbols and black-box sections · M11 front and machine layouts · M12 SPAC migration. See [docs/GAPS.md](docs/GAPS.md)* |

## Contributing

See [AGENTS.md](AGENTS.md).
