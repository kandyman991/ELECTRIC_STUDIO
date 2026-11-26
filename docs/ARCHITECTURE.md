# Architecture

Summary of the approved architecture (owner's design doc, 25 Nov 2026). Decisions marked
*open* have a proposed default but aren't settled yet.

## Layers

```
 Editor app        CLI            MCP server            entry points
      \             |              /
       ---- command layer (undo, redo, scripting, agents) ----
                    |
   ecad-model  ecad-rules  ecad-reports                  electrical core: source of truth
        |                         |
 ecad-project / symbols / parts   ecad-view              storage and libraries | CADCraft adapter
                                    |
             CADCraft crates (geom, doc, render, io), pinned commit
```

Dependencies point downward only. The core never depends on the UI or on CADCraft.

## Data model

| Object | What it is |
| --- | --- |
| Project | The whole job: standards, numbering rules, structure, revisions |
| Page | One sheet: number (`1`, `3a`), content description, position in the project |
| Sheet | Size (A3 landscape by default), border, numbered columns (from 1 or 0) and optional lettered rows, title block; gives addresses like `2.4` (page 2, column 4) or, with rows, `A3/2F` (page A3, column 2, row F) |
| Naming | A project setting. Device tags from a format: `-{letters}{n}` (`-K1`, the default) or `{page}{letters}{n}` (`A2SB1`: page A2, letter code SB). Wire numbers one per wire (`W1`…) or one per potential: every wire of a potential carries its name (`+A100`, `L1`) or a number from a format such as `{page}{n:02}` (`P201`). A wire that joins two potentials brings them to one number; renumbering a wire renumbers its potential; "Renumber all wires" numbers everything again page by page |
| Title block | Along the bottom of every page: the last three revisions, diagram code, project, customer, designer, plant, date, the page title in two lines (e.g. two languages), the page number and the next page. Set in File > Project settings |
| Device | A real component with an IEC 81346 tag (`-K1`, `=A1+B2-K1`) and a part number |
| Function | One drawing of a device on a page (a coil here, its contacts elsewhere), with its own terminal designations (`23`/`24` for a second NO contact) |
| Symbol role | Main function (a coil or a breaker: shows a contact table of its contacts), contact (NO/NC auxiliary contacts numbered 13-14, 21-22…, or main contacts 1-2 3-4 5-6; each refers back to its main function), or single (terminal, supply) |
| Symbol library | `ecad-model::library`: about 40 IEC 60617 symbols in categories (coils and valves, contacts, operator devices, sensors and switches, protection, signalling, loads and supplies, PLC, wiring), with IEC 81346 letter codes (`-KM`, `-KA`, `-QF`, `-SB`, `-SH`, `-HL`, `-FU`, `-M`…). New projects get the library; opening an older project adds the symbols it lacks without touching its own. `ecad new-sample <dir> library` draws them all |
| Black box | A device drawn as a box with its own named terminals on any side (an inverter, a safety module, a power supply): a symbol made for it (`box.1`…), added to the project with an undoable command |
| Multi-pole device | One symbol for all poles, 8 mm apart, with the dashed mechanical link: contactor main contacts, 3-pole breaker, motor-protection breaker, 3P+N residual-current breaker, switch-disconnector with fuses |
| Contact table | Under a coil or breaker, one row per pole of each contact: `14 ⌐ 13  3/6C`, the designations around a small NO or NC contact and where it is drawn |
| Connection point | A designated terminal (`A1`, `13`) with the direction a wire leaves in |
| Connection | A wire between two connection points, with wire number, cross-section, colour |
| Potential | Calculated from the wiring: points joined by wires, inside junctions, and across all drawings of a potential or interruption device. Named by its potential points (device tag `L+`, `M`); two names on one potential is flagged |
| Interruption point | An arrow; all arrows of one device (same tag) are connected, so a potential continues on another page. Each shows where its partners are |
| Junction | A dot whose four connection points are one potential, so wiring can branch |
| Part | Catalogue entry: part number (unique), manufacturer, description; kept in the project (`parts.json`), importable from CSV |
| Device parts | The parts a device is built from, with quantities; the BOM adds them up |
| Terminal strip | A device (`-X2`) whose drawings are its terminals, each with a number unique on the strip (`-X2:3`); both sides of a terminal are one potential; its parts count once per terminal |
| PLC card | A device (`-KF1`) whose drawings are its channels: digital or analog input or output (DI, DO, AI, AO), numbered per card from 0 (`-KF1:3`). Each channel has an address (`I0.0`, `Q1.3`, `IW256`; new channels get the next free one), a PLC symbol and a comment. Addresses and symbols are unique in the project. The PLC I/O list reports them with what each channel is wired to |
| Wire details | Each wire can carry its cross-section (`2,5`) and colour (`BK`, upper case), shown under its number, and the cable core it is |
| Location box | A dashed rectangle on a page with a label: a location (`+A101`), a connector housing (`-XCPR1`) or a field device (`EXT`). Drawn with the Box tool, moved by its edge; devices inside are in that place (shown in the device panel, outermost box first). Saved with the page |
| Plug connector | A device (`-XD1`) whose drawings are its pins, numbered like terminals on a strip (`-XD1:3`), both halves one potential; listed in the terminal diagram |
| Cable | Tag (`-WM1`), type (`4G2,5`), number of cores and shield. Its wires are marked with a slash and joined by a dashed line, with the tag and type beside them. A core is used once per cable; a cable with wires can't be removed. Saved in `cables.json` |

Every object has a stable UUID; tags and names are attributes. Implemented so far: project,
page, symbol, device, function, connection point, connection, routing, commands and undo.

## Checks

`ecad-rules` reads the model and lists what looks wrong; it never changes anything. The editor
reruns it after every change (cached per revision) and shows the result in the checks panel,
where clicking a finding opens its page and selects the symbol or wire. `ecad check` prints the
same list and exits with 1 when there are errors, for use in CI.

| Rule | Severity | Finds |
| --- | --- | --- |
| `potential-clash` | error | Differently named potentials (L+ and M) connected |
| `wire-number-shared` | error | One wire number on two different potentials |
| `wire-numbers-differ` | warning | With one number per potential: a potential whose wires carry different numbers |
| `unconnected-point` | warning | A connection point without a wire |
| `junction-unused` | warning | A junction joining fewer than three wires |
| `lonely-interruption` | warning | An interruption point without a partner of the same name |
| `wire-unnumbered` | warning | A wire without a number |
| `missing-part` | warning | A device without a catalogue part (missing from the BOM) |
| `plc-no-address` | warning | A PLC channel without an address |
| `cable-cores` | warning | A cable carrying more wires than it has cores |
| `overlap` | warning | Two symbols overlapping on a page |

Some mistakes can't be made at all, because the model refuses the command: duplicate device
tags, wire numbers, terminal or channel numbers, PLC addresses or symbols, and a second wire on
one connection point.

## Drawing output

`ecad-view` turns each page into a CADCraft drawing and writes it with `cadcraft-io`:

- **PDF**: the whole project in one file (`drawings.pdf`), one A3 landscape page per page, in
  page order, at 1:1. CADCraft plots sheets of one drawing, so the pages are laid side by side
  in a working drawing and each PDF page plots its sheet's window. Text is CADCraft's stroke
  font drawn as outlines: it prints like the screen but isn't searchable yet.
- **DXF** and **DWG** per page, with symbols as blocks and the device tag as a `TAG`
  attribute. DWG goes through CADCraft's DWG bridge (the `acadrust` crate, MPL-2.0, used
  unmodified). That bridge is meant to run untrusted input in a child process; we only write
  drawings this process just built, so it runs in process.
- **PNG** per page, as a preview.

File > Export drawings… and `ecad export` write all of them.

## Editing

`ecad-editor` holds all editing behaviour without any UI toolkit: Select (click, drag with
grid snapping, R to rotate, Delete), Place (new device with the next free tag) and Wire (click
two connection points; the next free wire number). A shell feeds it pointer events in page
millimetres and keys, and draws `Editor::base` (the page as plain polylines and triangles from
`ecad-view`) plus `Editor::overlays` (markers, outlines, rubber band, ghost symbol).

CADCraft's own editing engine is not used: it treats the drawing as the source of truth, which
is the opposite of this design. Only its drawing, rendering and file output are used.

## Project files

A project is a folder of pretty-printed JSON: `project.json`, `devices.json`,
`symbols/<id>.json`, `pages/<page-id>.json`. Output is stable (sorted, no timestamps) so it
diffs cleanly in git. Loading replays everything through the model's commands, so a broken
hand edit is rejected with the file named.

## Standards

IEC 60617 symbols, IEC 81346 reference designations, IEC 61082 documentation, IEC 60204-1
machine wiring rules. ANSI/NFPA 79 later.

## Decisions

| Decision | Status |
| --- | --- |
| Separate product from HMI Studio; they share only a tag-exchange format | Decided |
| CADCraft crates as libraries, not a fork | Decided |
| Milestones M0 to M6 with an MVP gate after M4 | Decided |
| UI technology: egui, a thin shell (`apps/ecad-app`) over `ecad-editor` | Decided in M1b (docs/UI-DECISION.md) |
| Project files: a folder of JSON files | Implemented in M1a |
| Licence and business model | *Open*, no licence until the owner decides |
| IEC first; Windows and Linux first | *Open*, proposed |
