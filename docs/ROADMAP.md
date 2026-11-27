# Roadmap: machine-centric platform

**Status: revised after the owner's review of D1–D28 (27 Nov 2026), for a second independent
review.** Pairs with [PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md). Nothing starts until
this revision is approved.

## Where things stand

| Milestone | State |
| --- | --- |
| M0–M6 (foundation, editor, multi-page, parts and reports, checks and output, MVP gate, panel layout, integrations) | Merged |
| M7a components library (PR #32) | Draft, **on hold** (D13). Its compatible parts are reused in M9 |

The earlier proposals in GAPS.md (library, annotations, generated pages, symbols, layouts,
SPAC) are all folded into the plan below. Nothing from them is dropped.

## Principles for every milestone

- One branch and one PR, reviewable on its own, mergeable on its own, leaving `main`
  releasable.
- Model and operations first (in the one registry, D16), then CLI and MCP, then a minimal app
  panel. UI polish is a separate track.
- **Minimal impact:** every new operation declares its scope and passes the stability test
  (PLATFORM-ARCHITECTURE.md 12.1). Nothing renumbers, reorders, relabels or rearranges as a
  side effect, in released and unreleased projects alike.
- No breaking change without a reader for the previous format, and golden drawings unchanged.
- **Performance:** the benchmark of PLATFORM-ARCHITECTURE.md section 13 runs on every PR from
  M7. A milestone may not regress a target.
- Each milestone extends the [conveyor acceptance project](#acceptance-project-the-conveyor)
  and its CI test. A milestone is done when its acceptance criteria pass in CI.

## Milestones, in order

| # | Milestone | Scope | Breaking? | Needs |
| --- | --- | --- | --- | --- |
| **M7** | Model foundations | Terminals and nodes derived in memory from today's model, with deterministic ids (UUID v5 from drawing and point), used by netlist, wire list and checks. Designations unique per device (check). Stable content hash. Netlist API. Typed PLC channel data in memory. Performance benchmark (section 13) | No | — |
| **M10** | Format 2: canonical connectivity | Persisted `terminals.json` (`TerminalId`, D2), `nodes.json` (junctions, potentials, interruptions, D1, D33), `wires.json` with terminal or node ends, wire graphics and node drawings in page files; undrawn wires; products in the project by id (`PartRef` by id, D7); sparse page order; typed PLC data; format 1 reader; golden drawings identical | **Format yes;** behaviour of `Reassign` reported as rewiring | M7 |
| **S0** | Stable numbering | Numbering policies per kind, **sequential by default** (D22); resource-aware allocation for PLC channels, strip terminals, cable cores; reserved identifiers and resources; retired identifiers (D23); joining potentials keeps numbers; inserted page names (D25); renumber and reorder as explicit global operations; removal without renumbering; the stability test harness | **Behaviour yes,** on purpose | M10 |
| **AI0** | Engineering interface | One registry of typed queries and operations (D16) shared by editor, CLI and MCP, MCP's tools generated from it. Actors assigned by trusted code. Change sets: propose, preview (impact, checks, pages, direct and indirect safety, D19), **accept each one,** apply as one undo step, base check. **MCP `--write` removed;** MCP is propose-and-preview (D18, D32). Persistent change records (D17) | **Behaviour yes** (MCP) | M10 |
| **M8** | Machine structure | Sections (=) and locations (+); device section and location; safety marking on sections; computed full designations with the project convention (D3, D4); **relabelling operations previewed and confirmed**; location boxes refer to locations; proposal from tag prefixes; `structure.json` | No (additive file) | M10, AI0 |
| **M9** | Component library | #32 reconciled: canonical product ids and manufacturers with aliases (D7); immutable revisions and assets in `objects/` (D6); three-way update; CSV and Excel import; **import security** (section 5.1); **portable library packages;** **mechanical components;** typed technical data with units; terminal patterns; symbol mapping; provenance and verification of imported data (D17); library panel | No (additive) | M10, AI0 |
| **S1** | Baselines, states and change impact | Baselines (draft, released, as-built, revision) with complete content, issued exports, transactional creation, integrity checks (D26, D27); **engineering states and modification packages** (D29); the change-impact engine with physical work, safety and issued documentation (D28); `ecad change-report --since`; replacement distinct from removal (D23) | No (additive) | S0, AI0 |
| **M11** | Equipment requirements | Requirement kinds per D12, added incrementally: motor, sensor, valve, actuator, operator device, power supply, PLC, remote I/O, drive, protection device, safety equipment, generic. Typed, unit-aware parameters with optional ETIM mapping (D14); selection from the library with constraint checks; safety marking by product class | No (additive file) | M8, M9 |
| **M12** | Circuit templates | Template format with typed parameters and options (D8); template library with revisions; instantiation into devices, terminals, nodes and undrawn wires; first templates: supply, DOL and reversing starters, 24 V supply, PLC DI and DO, sensor, pushbutton, lamp, safety relay | No (additive) | M9, M10 |
| **C0** | CAD geometry model | `Graphic` extended into native 2D geometry: primitives (line, polyline, arc, circle, rectangle, text, hatch, image), layers, blocks with attributes, dimensions; on pages as annotations; rendering, PDF, DXF and DWG export; **SVG export** | No (additive) | M10 |
| **C1** | CAD tools | Drawing tools, grips, snapping (end, mid, centre, intersection, perpendicular, connection points), alignment and distribution, dimension tools, layer control; all in `ecad-editor` | No | C0 |
| **M13** | Schematic generation | Deterministic, **incremental** generation onto inserted pages (D9, D24); template drawings and default symbols; node drawings and cross-references; stored generated values, persistent deletion overrides, dependency-aware regeneration (D10) as an explicit, scoped operation; resource and space problems reported | No (additive fields) | M12, S0, S1 |
| **C2** | Symbol and component geometry editors | Electrical symbol editor (geometry, connection points, fields, role); component geometry editor (footprint, front view, cut-out, drilling); **DXF and SVG import into geometry**, through the worker for untrusted files | No | C1, M9 |
| **M14** | Documentation set | Device and wire data on drawings; second-language texts; generated pages (cover, contents, BOM, terminal strips, cable list, PLC I/O, legend, change report) in the frame; issued exports stored with baselines; consistency validation report | No | M13, C0, S1 |
| **M15** | Layouts | Cabinet plates and **doors with cut-outs and dimensions,** footprints and front views from product geometry; machine layout over a machine drawing, **cable routes** and lengths; new devices go to "not placed yet"; `plate-full` and `rail-full` checks; layout optimisation only as an explicit global operation | No | M8, M9, S0, C1, C2 |
| **M16** | Functional modules | Module library (sections, requirements, templates, parameters); instantiation; the conveyor zone as the first module | No | M13 |

### Order and dependencies

Each milestone needs only what is listed:

- **M7** first, then **M10**.
- After M10: **S0**, **AI0**, **C0**.
- After AI0: **M8**, **M9**. After S0 and AI0: **S1**.
- After M8 and M9: **M11**. After M9 and M10: **M12**.
- After M12, S0 and S1: **M13**.
- After C0: **C1**; after C1 and M9: **C2**.
- After M13, C0 and S1: **M14**. After M8, M9, S0, C1 and C2: **M15**.
- After M13: **M16**.

- M10 now comes right after M7. Persistent terminal ids, nodes, product ids, sparse page order
  and typed PLC data are all format changes, so they arrive together in the one format break,
  before anything builds on them.
- S0 and AI0 follow M10, so every later milestone allocates identifiers safely and runs
  through the registry and change sets.
- S1 comes before M13, so generation is incremental and comparable against baselines from the
  start.
- The CAD track (C0–C2) can run in parallel with M8–M12.

### AI track (inference)

AI0 above builds the prerequisites. Inference comes later, each step independently reviewable.

| # | Milestone | Scope | Needs |
| --- | --- | --- | --- |
| **AI1** | Local assistant, read-only | `ecad-assist` adapter crate, provider interface, Ollama provider; off by default; per-project data policy; queries only; offline | AI0, M11 |
| **AI2** | Proposals | Change sets for machine structure and requirements from a description; component ranking with explanations; assistant review panel; each change set accepted individually | AI1 |
| **AI3** | Datasheet extraction | PDF text into suggested technical data, terminal patterns and sizes, with page citations; through import security | AI2, M9 |
| **AI4** | Templates and documentation | Template and option proposals; descriptions, notes and second-language texts | AI2, M12, M14 |
| **AI5** | External providers | Disabled by default, explicit project authorisation, confidentiality enforced, controlled audit log with retention (D20, D21) | AI1 |

### Parallel tracks

| Track | Scope | Can start after |
| --- | --- | --- |
| Symbols | Missing IEC symbols (inverter, transformer, heater, earth and screen, busbar, safety relay, switch, HMI, fan, PLC modules); black-box sections, captions, two-line labels, network ports; connectors with pinouts | M10 (drawn with C2's editor once it exists) |
| Manufacturer data | BMEcat with ETIM; **ETIM xChange;** ECLASS Advanced (licence checked first) and AASX; all through import security | M9 |
| SPAC migration | Data from the SPAC databases plus per-sheet underlays (SPAC-NOTES.md) | M14 |
| UI | Workspace navigation, cross-probing, panels polished | Any milestone, after its model work |
| Calculations | Enclosure temperature rise from product power losses | M9, M15 |

## Acceptance project: the conveyor

A small industrial belt conveyor, kept permanently in the repository, built **through the
machine-centric workflow by operations** (not hand-drawn pages). It is checked in CI on every
PR, and grows with each milestone without ever shrinking.

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
| `=SAF` | Emergency stop (safety-related) |

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
| R3 | `=CTRL` | `+CAB.P` | PLC with at least 8 DI and 6 DO, 24 V DC; **4 DI and 2 DO reserved** for a future extension |
| R4 | `=CONV.DRV` | `+CONV.DRV` | Motor 0.55 kW, 400 V 3~, 1.5 A, 4 pole, reversing, direct on line |
| R5 | `=CONV.DRV` | `+CAB.P` | Motor protection with adjustable thermal trip covering R4's current, auxiliary contact to the PLC |
| R6 | `=CONV.SEN` | `+CONV.IN` | Photoelectric sensor, PNP NO, 24 V DC, M12 connector |
| R7 | `=CONV.SEN` | `+CONV.OUT` | As R6 |
| R8 | `=CONV.OP` | `+CAB.D` | Start pushbutton green NO; stop pushbutton red NC; reverse jog pushbutton black NO |
| R9 | `=CONV.OP` | `+CAB.D` | Running lamp green, fault lamp red, 24 V DC |
| R10 | `=SAF` | `+CAB.D` | Emergency stop, two NC, with a safety relay dropping the motor contactors |

**Expected electrical design**

- Supply: `-Q0` main switch, `-F1` 400 V distribution MCB, `-F2` MCB for the 24 V supply,
  `-T1` 24 V supply, `-F3` 24 V circuit protection.
- Drive: `-Q1` motor protector, `-K1` forward and `-K2` reverse contactors, interlocked
  electrically by NC auxiliaries, feedback to the PLC.
- `-K3` safety relay: emergency stop `-S0` with two channels, dropping `-K1`/`-K2`, its
  auxiliary contact to the PLC.
- PLC `-A1`:
  - DI: start, stop, jog, infeed sensor, outfeed sensor, motor protector OK, safety relay OK,
    contactor feedback; four further DI reserved;
  - DO: forward, reverse, running lamp, fault lamp; two further DO reserved.
- Terminals: `-X1` supply, `-X2` motor, `-X3` field 24 V.
- Cables: `-W1` motor cable 4G1,5; `-W2`, `-W3` sensor cables 3 × 0,34 with M12.
- Mechanical: enclosure, mounting plate, DIN rails, ducts, cable glands, as library products.
- Products come from a **test component library committed in the repository,** with the
  fictional maker `Example`. It is imported from a CSV list, an Excel list and a portable
  package, as a user would.

### Acceptance criteria by milestone

| After | The CI test proves |
| --- | --- |
| M7 | The conveyor drawn schematic-first (a script of today's operations) resolves every wire end to a terminal or node, with the same ids on every load; designations unique; netlist stable across save and load; benchmark baseline recorded |
| M10 | Format 2 saves and reads back identical; the M7 conveyor saved as format 1 converts to the same terminals (same ids twice), nodes and netlist, with byte-identical golden drawings; renumbering a terminal changes one field and no wire; a wire added without drawing appears as not drawn; two products with the same number from two makers coexist |
| S0 | Stability test: adding a device, a wire, a terminal and a page changes only those. Joining two numbered potentials keeps both numbers and reports `wire-numbers-differ`. Removing `-B2` changes nothing else and retires it; the next sensor is `-B3` (sequential). A new DI for an unrelated device skips the reserved four. Allocating past the last free channel reports exhaustion and adds nothing. A page inserted after 3 is `3a`, and no other page file changes |
| AI0 | Every step of the conveyor script runs through the registry; MCP's tool list matches it. The same build as an assistant change set previews the expected additions with no new errors, applies only after acceptance, as one undo step; undo restores it byte for byte; a change set against an older fingerprint is refused. **No MCP or CLI call applies an assistant change set without acceptance;** an MCP client can't name itself a person. A change to `-F3`, which feeds the safety relay, is flagged as indirectly safety-related. Every applied change set leaves a change record |
| M8 | Sections and locations as above; every device assigned; full designations (`=CONV.DRV+CAB.P-K1`) in reports. **Renaming `=CONV.DRV` previews the relabelling of its devices and changes nothing until confirmed.** Location boxes linked; proposing structure from tag prefixes on the schematic-first version gives the same tree |
| M9 | The test library imports from CSV, Excel and a package; each product keeps its id across re-imports and number changes; a hostile-file set (XXE, zip bomb, path traversal, oversize) is refused with nothing written. Imported data carries provenance and is unverified; `verify` by a person clears it; manual entries are unverified too. Changing a price and updating shows one field changed; a local edit survives; a conflicting edit is reported; the base revision is read from `objects/` |
| S1 | The retrofit scenario's steps 1–4, 6, 9 and 10 below, with the second section added by operations, not yet generated. The modification package goes proposed → approved → released, and the as-built baseline stays unchanged until it is marked installed. Objects added by it show *not installed yet*. Replacing `-Q1` keeps its id, tag, terminals, drawings, wires and position and is reported as *replace*. Removing and re-adding is reported as removal plus addition with a new tag. A crash during baseline creation leaves no baseline or a complete one |
| M11 | `=SAF` and the safety relay are safety-related; an assistant change set touching `-K3` needs the separate confirmation; the assistant can't verify it or remove its classification; a release export with unverified safety data fails. R1–R10 entered with typed units; products selected; R5's selection fails the check when the motor becomes 1.1 kW (2.6 A, outside the protector range) and passes after reselecting |
| M12 | The electrical design above built from templates with ports bound; the netlist equals the schematic-first conveyor's (same potentials, terminals matched by device and designation) |
| C0 | Annotations, layers and a block (the company logo) on the conveyor pages; PDF, DXF, DWG and SVG export with golden files |
| C1 | Scripted CAD edits (draw, snap to end and intersection, align, dimension) give exact coordinates; undo restores each |
| M13 | The full retrofit scenario, with step 5 by incremental generation. **Generating twice gives byte-identical files.** After moving `-K1` and renumbering one wire by hand, then changing R4 to 0.75 kW and reselecting the motor and protector, scoped regeneration keeps the move and the number, updates the device data shown, and lists exactly the expected conflicts. A generated symbol deleted by hand is not recreated. Check: 0 errors |
| C2 | The safety relay's black-box symbol and the motor protector's footprint drawn in the editors; a manufacturer DXF and an SVG imported into product geometry; a hostile SVG refused |
| M14 | Cover, contents, BOM, terminal strips, cable list, PLC I/O and change-report pages generated in the frame; BOM quantities equal the devices' products, mechanical items included; TIA tag table matches the PLC channels; issued exports stored with the released baseline and re-handed byte for byte |
| M15 | Retrofit step 8: the second section's devices don't fit the plate, giving `plate-full` with no existing item moved. Every cabinet device on the plate with its product footprint; the door with cut-outs and dimensions; motor and sensors on the machine layout; cable routes and lengths for `-W1`–`-W3` within the expected range |
| M16 | The conveyor rebuilt from one "conveyor zone" module plus supply and control; same netlist as M12 |

Always, from M7 on:

- the conveyor builds and checks with no AI provider present (from AI1: also with the
  assistant switched off);
- the reference project and the samples still load;
- golden drawings unchanged unless a PR says why;
- benchmark targets met;
- `ecad check` on the conveyor gives 0 errors.

### The retrofit scenario

Permanent, in CI, from S1 on. It completes at M13 and M15.

1. Build the original conveyor completely (sections, requirements, products, generated and
   hand-refined pages, cabinet layout).
2. Make an **as-built** baseline.
3. Save, close and reopen the project; the baseline verifies its hashes.
4. Open a **modification package** against the as-built baseline and add a second conveyor
   section `=CONV2`:
   - its motor (requirement R11, like R4), motor protector and contactors;
   - two photoelectric sensors;
   - their PLC channels on the **reserved** inputs and outputs, chosen explicitly;
   - their terminals and cables.
5. Generate only the new and affected content:
   - new pages inserted after the conveyor's pages (`7a`, `7b`...);
   - new terminals allocated sequentially on `-X2` and `-X3`.
6. Every unaffected device tag, wire number, terminal designation, PLC address, page name and
   cabinet position is identical to the baseline (compared by id).
7. Every unaffected page file is byte-identical to the baseline's.
8. The plate has room for the new protector and contactors only if the original layout left
   it. The test plate doesn't, so a `plate-full` warning is reported and nothing existing
   moves.
9. The change report against the as-built baseline lists exactly the expected changes:
   - added equipment, wires, terminals, channels, cables and pages;
   - the reserved channels now used;
   - BOM additions;
   - physical work: new wiring only, no relabelling, no rewiring of existing wires, no
     relocation;
   - the issued documents that need reissuing (the PLC I/O list, `-X2` and `-X3`
     terminal diagrams, the BOM);
   - the space warning.
10. The whole addition is one accepted change set, applied as one step:
    - undo restores the project byte for byte, and redo reapplies it;
    - it is saved, reopened and identical;
    - a crash injected mid-save leaves either the old or the new state.

Also tested:

- **Removal:** removing sensor `-B2` takes its terminals, drawings, wires, channel use and
  layout item away. Nothing is renumbered, `-B2` is retired, the freed DI becomes free (not
  reserved), and the report lists the rewiring.
- **Replacement:** `-Q1` gets a protector from another range. Its identity and everything
  attached is kept, and the report lists the replacement and any terminal or footprint
  differences.
- **Retired identifiers:** after release, the next new sensor never gets `-B2`. Reusing `-B2`
  explicitly needs approval and appears in the change report.
- **States:** until the package is marked installed, the as-built baseline and the as-built
  documents are unchanged.

### Where it lives

- `crates/ecad-model/src/conveyor.rs`: the build script as operations, one function per
  milestone stage.
- `tests/acceptance/conveyor/`:
  - the test component library (CSV, Excel, package);
  - hostile-file fixtures;
  - expected netlist, BOM, reports and change reports;
  - golden pages.
- `ecad new-sample <dir> conveyor` writes it.
- CI runs it in the existing test job; no new infrastructure.

## Decisions

All of D1–D29 are approved with conditions; the record is in
[PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md#decision-record).

The open decisions raised by this revision, and what they block:

| Decision | Blocks |
| --- | --- |
| D33 (nodes from today's junction, potential and interruption devices) | M10 |
| D31, D32 (limit of AI enforcement, removing MCP `--write`) | AI0 |
| D34 (performance targets) | M7's benchmark |
| D30, D35 (CADCraft geometry outside `ecad-view`, SVG library) | C0 |
| D36 (one open modification) | S1 |
| D37 (the earlier review document) | Confirmation of M9, C0–C2 and the manufacturer-data track |
| D38 (`objects/` and git) | S1 |
