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
| Sheet | Size (A3 landscape by default), border, 10 numbered columns, title block; gives addresses like `2.4` (page 2, column 4) |
| Device | A real component with an IEC 81346 tag (`-K1`, `=A1+B2-K1`) and a part number |
| Function | One drawing of a device on a page (a coil here, its contacts elsewhere), with its own terminal designations (`23`/`24` for a second NO contact) |
| Symbol role | Main function (coil: lists its contacts), contact (NO/NC: refers back to the coil), or single (terminal, supply) |
| Connection point | A designated terminal (`A1`, `13`) with the direction a wire leaves in |
| Connection | A wire between two connection points, with wire number, cross-section, colour |
| Potential | Calculated from the wiring: points joined by wires, inside junctions, and across all drawings of a potential or interruption device. Named by its potential points (device tag `L+`, `M`); two names on one potential is flagged |
| Interruption point | An arrow; all arrows of one device (same tag) are connected, so a potential continues on another page. Each shows where its partners are |
| Junction | A dot whose four connection points are one potential, so wiring can branch |
| Part | Catalogue entry: part number (unique), manufacturer, description; kept in the project (`parts.json`), importable from CSV |
| Device parts | The parts a device is built from, with quantities; the BOM adds them up |
| Terminal strip | A device (`-X2`) whose drawings are its terminals, each with a number unique on the strip (`-X2:3`); both sides of a terminal are one potential; its parts count once per terminal |
| PLC card | A device (`-KF1`) whose drawings are its channels: digital or analog input or output (DI, DO, AI, AO), numbered per card from 0 (`-KF1:3`). Each channel has an address (`I0.0`, `Q1.3`, `IW256`; new channels get the next free one), a PLC symbol and a comment. Addresses and symbols are unique in the project. The PLC I/O list reports them with what each channel is wired to |
| Cable | Later |

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
| `unconnected-point` | warning | A connection point without a wire |
| `junction-unused` | warning | A junction joining fewer than three wires |
| `lonely-interruption` | warning | An interruption point without a partner of the same name |
| `wire-unnumbered` | warning | A wire without a number |
| `missing-part` | warning | A device without a catalogue part (missing from the BOM) |
| `plc-no-address` | warning | A PLC channel without an address |
| `overlap` | warning | Two symbols overlapping on a page |

Some mistakes can't be made at all, because the model refuses the command: duplicate device
tags, wire numbers, terminal or channel numbers, PLC addresses or symbols, and a second wire on
one connection point.

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
