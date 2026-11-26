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
| Page | One sheet: type, title block, column grid |
| Device | A real component with an IEC 81346 tag (`-K1`, `=A1+B2-K1`) and a part number |
| Function | One drawing of a device on a page (a coil here, its contacts elsewhere) |
| Connection point | A designated terminal (`A1`, `13`) with the direction a wire leaves in |
| Connection | A wire between two connection points, with wire number, cross-section, colour |
| Potential | Calculated from the wiring (M2), named at potential-definition points |
| Interruption point | A potential continuing on another page (M2) |
| Cable, terminal strip, PLC card | M2 to M3 |

Every object has a stable UUID; tags and names are attributes. Implemented so far: project,
page, symbol, device, function, connection point, connection, routing, commands and undo.

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
