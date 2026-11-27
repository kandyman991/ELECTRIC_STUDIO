# Roadmap: machine-centric platform

**Status: proposal for review,** with [PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md).
Nothing starts until the architecture is approved.

## Where things stand

| Milestone | State |
| --- | --- |
| M0–M6 (foundation, editor, multi-page, parts and reports, checks and output, MVP gate, panel layout, integrations) | Merged |
| M7a components library (PR #32) | Draft, **on hold.** Built before this direction; reconciled in M9 |

The earlier proposals in GAPS.md (M7 library, M8 annotations, M9 generated pages, M10 symbols,
M11 layouts, M12 SPAC) are folded into the sequence below. Nothing from them is dropped.

## Principles for every milestone

- One branch and one PR, reviewable on its own, mergeable on its own, leaving `main`
  releasable.
- Model and commands first, then CLI and MCP, then a minimal app panel. UI polish is a
  separate track.
- No breaking change without a reader for the previous format and the golden files
  unchanged.
- Each milestone extends the [conveyor acceptance project](#acceptance-project-the-conveyor)
  and its CI test; a milestone is done when its acceptance criteria pass in CI.

## Milestones

| # | Milestone | Scope | Breaking? | Needs |
| --- | --- | --- | --- | --- |
| **M7** | Model foundations | Terminal resolver: every connection end as (device, designation, side, drawing), used by netlist, wire list and checks. Invariant check: designations unique per physical device. Rename cascade for designations. Stable content hash. Netlist API (`Project::netlist`). Typed PLC channel fields read from `properties`. | No | — |
| **M8** | Machine structure | Sections (=) and locations (+) as trees; device section and location; computed full designation (naming setting); location boxes refer to locations; proposal command from tag prefixes; `structure.json`; CLI and MCP; structure panel | No (additive file) | — |
| **M9** | Component library | PR #32 reconciled: shared library, CSV/Excel import, take/refresh with source and revision, three-way update; numeric technical data with units; terminal pattern and symbol mapping on parts; library panel (search, add, update) | No (additive fields) | M7 (stable hash) |
| **M10** | Connectivity in the model | Format 2: `wires.json` with terminal ends, undrawn wires, wire graphics in page files, format 1 reader; checks and reports for undrawn wires; schematic-first editing unchanged | **Format yes, behaviour no** | M7 |
| **M11** | Equipment requirements | Requirement kinds (motor, sensor, valve or actuator, operator device, power supply, generic) with parameters; assignment to sections and locations; product selection from the library with constraint checks; requirements report; requirements panel | No (additive file) | M8, M9 |
| **M12** | Circuit templates | Template format (devices, ports, internal wires, drawing, options); template library; instantiate into the model (devices and undrawn wires); project copies with revision; first templates: supply, DOL and reversing starters, 24 V supply, PLC DI and DO channel, sensor, pushbutton, lamp | No (additive) | M9, M10 |
| **M13** | Schematic generation | Deterministic placement of template drawings and default symbols; interruption points and cross-references; origin and generated hashes; regeneration with preview, pinned placement, conflicts, orphans, detach | No (additive fields) | M12 |
| **M14** | Documentation set | Annotations (text, graphics, images, dimensions); device and wire data on drawings; second-language texts; generated pages (cover, contents, BOM, terminal strips, cable list, PLC I/O, legend) in the frame, in PDF and DWG; consistency validation report | No | M13 for generated content; annotations can start after M10 |
| **M15** | Layouts on shared CAD | Cabinet layout: footprints and front views from library DXF, door layout with cut-outs; machine layout: field locations over a machine drawing, cable routes and lengths; layout checks against locations | No | M8, M9 |
| **M16** | Functional modules | Module library (sections, requirements, templates, parameters); instantiate; the conveyor zone as the first module | No | M13 |

### Parallel tracks (independent of the sequence above)

| Track | Scope | Can start after |
| --- | --- | --- |
| Symbols | Missing IEC symbols (inverter, transformer, heater, earth and screen, busbar, safety relay, switch, HMI, fan, PLC modules); black-box sections, captions, two-line labels, network ports; connectors with pinouts | M7 |
| Manufacturer data | BMEcat with ETIM; then ECLASS Advanced (licence checked first) and AASX | M9 |
| SPAC migration | Data from the SPAC databases plus per-sheet underlays (see SPAC-NOTES.md) | M14 |
| UI | Workspace navigation, cross-probing, panels polished | Any milestone, after its model work |
| Calculations | Enclosure temperature rise from part power losses | M9, M15 |

### Order and dependencies

```text
M7 ──► M10 ──► M12 ──► M13 ──► M14
 │              ▲        │
 └──► M9 ───────┤        └──► M16
M8 ──► M11 ◄── M9
M8, M9 ──► M15
```

M7 and M8 can run in parallel, and M9 follows M7. M10 is the one format change and comes
early, so later milestones build on the final connectivity.

## Acceptance project: the conveyor

A small industrial belt conveyor, kept permanently in the repository, built **through the
machine-centric workflow by commands** (not hand-drawn pages). It is checked in CI on every
PR. It grows with each milestone and never shrinks.

### The machine

A 3 m belt conveyor carrying boxes from an infeed to an outfeed, with one reversible drive.
It is started and stopped from the cabinet door and stopped by an emergency stop.

**Sections (function aspect)**

| Section | Purpose |
| --- | --- |
| `=PWR` | Supply and distribution |
| `=CTRL` | 24 V supply and PLC |
| `=CONV.DRV` | Conveyor drive |
| `=CONV.SEN` | Box detection |
| `=CONV.OP` | Operator controls |
| `=SAF` | Emergency stop |

**Locations (location aspect)**

| Location | What |
| --- | --- |
| `+CAB` | Control cabinet; `+CAB.P` mounting plate, `+CAB.D` door |
| `+CONV` | On the conveyor: `+CONV.DRV` drive end, `+CONV.IN` infeed, `+CONV.OUT` outfeed |

**Requirements (maker-independent)**

| Req. | Section | Location | Requirement |
| --- | --- | --- | --- |
| R1 | `=PWR` | `+CAB` | Supply 3/N/PE AC 400/230 V 50 Hz, main switch lockable, 16 A |
| R2 | `=CTRL` | `+CAB.P` | 24 V DC power supply, 5 A, from 230 V AC, protected on both sides |
| R3 | `=CTRL` | `+CAB.P` | PLC with at least 8 DI and 6 DO, 24 V DC |
| R4 | `=CONV.DRV` | `+CONV.DRV` | Motor 0.55 kW, 400 V 3~, 1.5 A, 4 pole, reversing, direct on line |
| R5 | `=CONV.DRV` | `+CAB.P` | Motor protection with adjustable thermal trip covering R4's current, auxiliary contact to the PLC |
| R6 | `=CONV.SEN` | `+CONV.IN` | Photoelectric sensor, PNP NO, 24 V DC, M12 connector |
| R7 | `=CONV.SEN` | `+CONV.OUT` | As R6 |
| R8 | `=CONV.OP` | `+CAB.D` | Start pushbutton green NO; stop pushbutton red NC; reverse jog pushbutton black NO |
| R9 | `=CONV.OP` | `+CAB.D` | Running lamp green, fault lamp red, 24 V DC |
| R10 | `=SAF` | `+CAB.D` | Emergency stop, two NC, with a safety relay dropping the motor contactors |

**Expected electrical design**

- Supply `-Q0` main switch, `-F1` 400 V distribution MCB, `-F2` MCB for the 24 V supply, `-T1`
  24 V supply, `-F3` 24 V circuit protection.
- Drive: `-Q1` motor protector, `-K1` forward and `-K2` reverse contactors, interlocked
  electrically by NC auxiliaries, feedback to the PLC.
- `-K3` safety relay: emergency stop `-S0` with two channels, dropping `-K1`/`-K2`, its
  auxiliary contact to the PLC.
- PLC `-A1`:
  - DI: start, stop, jog, infeed sensor, outfeed sensor, motor protector OK, safety relay OK,
    contactor feedback;
  - DO: forward, reverse, running lamp, fault lamp.
- Terminals: `-X1` supply, `-X2` motor, `-X3` field 24 V.
- Cables: `-W1` motor cable 4G1,5; `-W2`, `-W3` sensor cables 3 × 0,34 with M12.
- Products from a **test component library committed in the repository,** with the fictional
  maker `Example`, imported from a CSV and an Excel list as a user would.

### Acceptance criteria by milestone

| After | The CI test proves |
| --- | --- |
| M7 | The conveyor drawn schematic-first (a script of today's commands) resolves every wire end to (device, designation); designations unique; renaming a terminal keeps every wire; netlist stable across save and load |
| M8 | Sections and locations as above; every device assigned; full designations (`=CONV.DRV+CAB.P-K1`) in reports; location boxes linked; proposing structure from tag prefixes on the schematic-first version gives the same tree |
| M9 | The test library imports from CSV and Excel; every device's part is taken from it; changing a price in the library and updating shows one field changed; a local contact-block edit survives; a conflicting edit is reported, not overwritten |
| M10 | The project saves as format 2 and reads back identical; the M7 schematic-first conveyor, saved as format 1 and read again, gives the same netlist and byte-identical drawings (golden); a wire added without drawing appears in the wire list as not drawn |
| M11 | R1–R10 entered; products selected for each; R5's selection fails the check when the motor becomes 1.1 kW (2.6 A outside the protector range) and passes after reselecting |
| M12 | The electrical design above built from templates (supply, reversing DOL, 24 V, PLC channels, sensors, buttons, lamps, safety relay) with ports bound; the netlist equals the schematic-first conveyor's netlist (same terminals, same potentials) |
| M13 | Pages generated from the model; **generating twice gives byte-identical files;** after moving `-K1` and renumbering one wire by hand, then changing R4 to 0.75 kW and reselecting the motor and protector, regenerating keeps the move and the number, updates the device data shown, and lists exactly the expected conflicts; check: 0 errors |
| M14 | Cover, contents, BOM, terminal strips, cable list and PLC I/O pages generated in the frame; BOM quantities equal the devices' parts; TIA tag table matches the PLC channels; PDF and DWG export with golden files |
| M15 | Every cabinet device on the plate with its library footprint; the door with the operator devices; motor and sensors placed on the machine layout; cable lengths for `-W1`–`-W3` within the expected range |
| M16 | The conveyor rebuilt from one "conveyor zone" module plus supply and control; same netlist as M12 |

Always, from M7 on:

- the reference project and the samples still load;
- golden drawings unchanged unless a PR says why;
- `ecad check` on the conveyor gives 0 errors.

### Where it lives

- `crates/ecad-model/src/conveyor.rs`: the build script, as commands, one function per
  milestone stage.
- `tests/acceptance/conveyor/`:
  - the test component library (CSV and Excel);
  - expected netlist, BOM and reports;
  - golden pages.
- `ecad new-sample <dir> conveyor` writes it.
- CI runs it in the existing test job; no new infrastructure.

## Outstanding architectural decisions

See [Decisions to make](PLATFORM-ARCHITECTURE.md#decisions-to-make) (D1–D15). The ones that
block M7 and M10 are **D1, D2 and D13**; D11 (shared CAD scope) blocks M15.
