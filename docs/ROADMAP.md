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
- **Stability:** every new operation declares its scope and passes the stability test
  (PLATFORM-ARCHITECTURE.md section 12.1). Nothing renumbers, reorders or rearranges as a side
  effect.
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
| **M13** | Schematic generation | Deterministic, **incremental** generation: only missing or named content, on new inserted pages, existing pages byte-identical; template drawings and default symbols; interruption points and cross-references; origin and generated hashes; explicit, scoped regeneration of existing content with preview, pinned placement, conflicts, orphans, detach; resource and space problems reported | No (additive fields) | M12, S0, S1 |
| **M14** | Documentation set | Annotations (text, graphics, images, dimensions); device and wire data on drawings; second-language texts; generated pages (cover, contents, BOM, terminal strips, cable list, PLC I/O, legend) in the frame, in PDF and DWG; consistency validation report | No | M13 for generated content; annotations can start after M10 |
| **M15** | Layouts on shared CAD | Cabinet layout: footprints and front views from library DXF, door layout with cut-outs; machine layout: field locations over a machine drawing, cable routes and lengths; layout checks against locations. New devices go to "not placed yet" or into free rail space on request; existing positions never move; `plate-full` and `rail-full` checks; layout optimisation only as an explicit global operation | No | M8, M9, S0 |
| **M16** | Functional modules | Module library (sections, requirements, templates, parameters); instantiate; the conveyor zone as the first module | No | M13 |

### Stability track (foundational)

Incremental engineering and as-built preservation, PLATFORM-ARCHITECTURE.md section 12. Both
milestones come early, because every later milestone must already obey them.

| # | Milestone | Scope | Breaking? | Needs |
| --- | --- | --- | --- | --- |
| **S0** | Stable numbering | Numbering policy per kind (strategy, format, range); reserved and retired identifiers in `numbering.json`. Joining potentials no longer renumbers wires. Lowest-free reuse governed by the policy. Sparse page order and inserted page names (`3a`); `ReorderPages` and "renumber all" as explicit global operations. Removal without renumbering. Stability test harness | **Behaviour yes** (wire numbers on joining), on purpose | M7 |
| **S1** | Baselines and change impact | Baselines (draft, released, as-built, revision) in `baselines/`, content-addressed and integrity-checked; open read-only; title-block link. Change-impact engine (equipment, connections, terminals, cables, PLC, pages, layouts, BOM, physical work, compatibility) used by change-set preview and `ecad change-report --since`. Replacement keeping identity | No (additive) | S0, AI0 |

M10, M13 and M15 take on the incremental rules:

- M10: undrawn wires never move drawn ones.
- M13: incremental generation onto inserted pages; regenerating existing content only as an
  explicit, scoped operation.
- M15: new devices go to "not placed yet"; space is checked, not rearranged.

### AI track

The prerequisites are built into the engineering milestones; inference comes after them. See
PLATFORM-ARCHITECTURE.md section 11.

| # | Milestone | Scope | Inference? | Needs |
| --- | --- | --- | --- | --- |
| **AI0** | Engineering interface | One registry of typed queries and operations shared by editor, CLI and MCP. MCP tools and schemas generated from it. Actor recorded on every history entry. Change sets: propose, preview (diff, check delta, pages affected, safety items), apply as one undo step, base check. MCP gains `propose`, `preview` and `apply` | No | M7 |
| *(in M8, M9, M11, M12, M14)* | Provenance as each layer arrives | Suggested or Verified on sections and locations (M8), technical data lines and selections (M9, M11), requirement parameters (M11), template choices (M12), descriptions and translations (M14); the `verify` operation (people only); the `unverified-data` check | No | AI0 |
| *(in M8, M11)* | Safety-related items | Safety marking on sections (M8) and product classes (M11); separate confirmation for change sets that touch them; release export refuses unverified safety data | No | AI0 |
| **AI1** | Local assistant, read-only | `ecad-assist` adapter crate with the provider interface and an Ollama provider; assistant off by default; per-project data policy; queries only: explain checks, find devices, summarise the BOM; works offline | Yes, local | AI0, M11 |
| **AI2** | Proposals | The assistant produces change sets: machine structure and requirements from a description, component ranking with explanations; assistant panel with preview and accept or reject | Yes, local | AI1, M11 |
| **AI3** | Datasheet extraction | PDF data sheet text into suggested technical data, terminal patterns and sizes, with page citations | Yes, local | AI2, M9 |
| **AI4** | Templates and documentation | Template and option proposals for requirements; descriptions, notes and second-language texts | Yes, local | AI2, M12, M14 |
| **AI5** | External providers | Opt-in per project, excluded fields, audit log of what was sent | Yes, external | AI1 |

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
M7 ──► AI0 ──► AI1 (needs M11) ──► AI2 ──► AI3, AI4;  AI1 ──► AI5
M7 ──► S0 ──► S1 (needs AI0) ──► M13
```

M7 and M8 can run in parallel, and M9 follows M7. AI0 follows M7 and comes before M9, so the
library update preview (M9) and regeneration preview (M13) are built as change sets. S0 follows
M7; S1 follows S0 and AI0, since its impact engine is the change-set preview. Both come before
M13, so generation is incremental from the start. M10 is the one format change and comes
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
| S0 | Adding a device, a wire, a terminal and a page to the M7 conveyor changes only those objects (stability test). Joining two numbered potentials keeps both numbers and reports `wire-numbers-differ`. Removing `-B2` leaves every other tag, wire number and terminal as it was and retires `-B2`; with the policy set to no reuse, the next sensor is `-B3`, not `-B2`. A page inserted after 3 is `3a` and no other page file changes |
| S1 | The retrofit scenario's steps 1–3, 6, 9 and 10 below, with the second section added by commands rather than generated. Making a released baseline switches the policy to no reuse. Replacement of `-Q1` keeps its id, tag, drawings, wires and position, and the change report lists "replace" with any terminal or footprint differences |
| AI0 | Every step of the M7 conveyor script runs through the operation registry; the same build run as a change set by an "assistant" actor previews the expected additions with no check errors, applies as one undo step, and undo restores the project byte for byte; a change set made against an older fingerprint is refused; MCP's tool list matches the registry |
| M8 | Sections and locations as above; every device assigned; full designations (`=CONV.DRV+CAB.P-K1`) in reports; location boxes linked; proposing structure from tag prefixes on the schematic-first version gives the same tree |
| M9 | Technical data lines entered by the assistant actor are Suggested, the `unverified-data` check lists them, and a person's `verify` clears them. The test library imports from CSV and Excel; every device's part is taken from it; changing a price in the library and updating shows one field changed; a local contact-block edit survives; a conflicting edit is reported, not overwritten |
| M10 | The project saves as format 2 and reads back identical; the M7 schematic-first conveyor, saved as format 1 and read again, gives the same netlist and byte-identical drawings (golden); a wire added without drawing appears in the wire list as not drawn |
| M11 | `=SAF` and the safety relay are safety-related; an assistant change set touching `-K3` is flagged and can't be applied without the separate confirmation; the assistant actor can't verify it; a release export with unverified safety data fails. R1–R10 entered; products selected for each; R5's selection fails the check when the motor becomes 1.1 kW (2.6 A outside the protector range) and passes after reselecting |
| M12 | The electrical design above built from templates (supply, reversing DOL, 24 V, PLC channels, sensors, buttons, lamps, safety relay) with ports bound; the netlist equals the schematic-first conveyor's netlist (same terminals, same potentials) |
| M13 | The full retrofit scenario below, with step 5 by incremental generation. Pages generated from the model; **generating twice gives byte-identical files;** after moving `-K1` and renumbering one wire by hand, then changing R4 to 0.75 kW and reselecting the motor and protector, regenerating keeps the move and the number, updates the device data shown, and lists exactly the expected conflicts; check: 0 errors |
| M14 | Cover, contents, BOM, terminal strips, cable list and PLC I/O pages generated in the frame; BOM quantities equal the devices' parts; TIA tag table matches the PLC channels; PDF and DWG export with golden files |
| M15 | Retrofit step 8: the second section's devices don't fit the plate and give a `plate-full` warning while no existing item moves. Every cabinet device on the plate with its library footprint; the door with the operator devices; motor and sensors placed on the machine layout; cable lengths for `-W1`–`-W3` within the expected range |
| M16 | The conveyor rebuilt from one "conveyor zone" module plus supply and control; same netlist as M12 |

Always, from M7 on:

- the conveyor builds and checks with no AI provider present (AI1 onwards: and with the
  assistant switched off);

- the reference project and the samples still load;
- golden drawings unchanged unless a PR says why;
- `ecad check` on the conveyor gives 0 errors.

### The retrofit scenario

Permanent, in CI, from S1 on. It completes at M13 and M15.

1. Build the original conveyor completely (sections, requirements, products, generated and
   hand-refined pages, cabinet layout).
2. Make an **as-built** baseline.
3. Save, close and reopen the project; the baseline verifies its hashes.
4. Add a second conveyor section `=CONV2`:
   - its motor (requirement R11, like R4), its motor protector and contactors;
   - two photoelectric sensors;
   - their PLC channels on spare inputs and outputs;
   - their terminals and cables.
5. Generate only the new and affected content: new pages inserted after the conveyor's
   pages (`7a`, `7b`...), new terminals appended to `-X2` and `-X3`.
6. Every unaffected device tag, wire number, terminal designation, PLC address, page name and
   cabinet position is identical to the baseline (compared by id).
7. Every unaffected page file is byte-identical to the baseline's.
8. The plate has room for the new protector and contactors only if the original layout left
   it. The test plate doesn't, so a `plate-full` warning is reported and nothing existing
   moves.
9. The change report against the as-built baseline lists exactly the expected changes:
   - added equipment, wires, terminals, channels, cables and pages;
   - BOM additions;
   - physical work: new wiring only, no relabelling, no relocation;
   - the space warning.
10. The whole addition is one change set, applied as one step. Then:
    - undo restores the project byte for byte, and redo reapplies it;
    - saved, reopened and identical;
    - a crash injected mid-save leaves either the old or the new state.

Also tested:

- **Removal:** removing sensor `-B2` takes its drawings, wires, terminal use and layout item
  away. Nothing is renumbered, `-B2` is retired, and the report lists the rewiring.
- **Replacement:** `-Q1` gets a protector from another range. Its id, tag, drawings, wires and
  position are kept, and the report lists the replacement and any terminal or footprint
  differences.

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

See [Decisions to make](PLATFORM-ARCHITECTURE.md#decisions-to-make) (D1–D28). The ones that
block M7 and M10 are **D1, D2 and D13**; D11 (shared CAD scope) blocks M15; **D16 and D18**
block AI0; D20 and D21 block AI1 and AI5; **D22, D23 and D25** block S0, D26 and D27 block
S1, and D24 and D28 block M13.
