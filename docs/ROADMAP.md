# Roadmap: machine-centric platform

**Status: architecture approved and merged (PR #31, 27 Nov 2026), all decisions D1–D43
resolved. M7 accepted with owner-approved deferred Legion validation (27 Nov 2026); M10 authorized.** Pairs with
[PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md).

## Where things stand

| Milestone | State |
| --- | --- |
| M0–M6 (foundation, editor, multi-page, parts and reports, checks and output, MVP gate, panel layout, integrations) | Merged |
| M7 model foundations | M7a (PR #33, derived connectivity, netlist, content hash) merged; M7b (conveyor acceptance project, benchmark harness) complete; PR #35 merged after all six exact-head CI jobs passed; **accepted with R5 deferred to the first production release** |
| M10 format 2 | In progress: M10a (#37) and M10b (#38) merged; M10c next |
| Components library (PR #32) | Draft, **on hold** (D13). Its compatible parts are reused in M9 |

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
- **Performance** (PLATFORM-ARCHITECTURE.md section 13):
  - deterministic counts (instructions, allocations) are measured on every PR from M7;
  - wall-clock and peak memory are measured on the Legion development machine under
    controlled conditions (D41);
  - CI keeps correctness tests and performance smoke tests;
  - a regression needs an explanation in the PR;
  - shared-runner wall clock never blocks a merge on its own.
- Each milestone extends the [conveyor acceptance project](#acceptance-project-the-conveyor)
  and its CI test. A milestone is done when its acceptance criteria pass in CI.

## Milestones, in order

| # | Milestone | Scope | Breaking? | Needs |
| --- | --- | --- | --- | --- |
| **M7** | Model foundations | Terminals and nodes derived in memory from today's model, with deterministic endpoint ids (UUID v5 from canonical legacy endpoint) and physical groups, used by netlist, wire list and checks. Scoped group/point uniqueness and explicit bridges (check). Stable content hash. Netlist API. Typed PLC channel data in memory. Benchmark harness: representative projects (conveyor, reference, large synthetic with real assets), deterministic counts and performance smoke tests on every PR; wall clock, peak memory and graphical workloads, cold and warm, on the Legion under recorded idle conditions (section 13, D41) | No | — |
| **M10** | Format 2: canonical connectivity | Persisted `terminals.json` (`TerminalId`, D2), `nodes.json` (junctions, potentials, interruptions, with explicit identity-based bindings, scoped display names and **ambiguous mappings reported, never merged**, D1, D33), `wires.json` with terminal or node ends, wire graphics and node drawings in page files; undrawn wires; products in the project by id (`PartRef` by id, D7); sparse page order; typed PLC data; format 1 reader; golden drawings identical | **Format yes;** behaviour of `Reassign` reported as rewiring | M7 |
| **S0** | Stable numbering | Numbering policies per kind, **sequential by default** (D22); resource-aware allocation for PLC channels, strip terminals, cable cores; reserved identifiers and resources; retired identifiers (D23); joining potentials keeps numbers; inserted page names (D25); renumber and reorder as explicit global operations; removal without renumbering; the stability test harness | **Behaviour yes,** on purpose | M10 |
| **AI0** | Engineering interface | One registry of typed queries and operations (D16) shared by editor, CLI and MCP, MCP's tools generated from it. Actors assigned by trusted code. Change sets: prepare, preview (impact, checks, pages, direct and indirect safety, D19), **approval in the app only, bound to the change set's hash and base fingerprint;** a substantial proposal approved as one transaction; stale or modified proposals refused; one undo step. **MCP `--write` removed;** MCP queries, prepares and previews (D18, D32). Basic engineering impact comparison and retained last committed contents in the object store; separate engineering content hash and committed revision token. CLI classes (D42): read-only, scoped trusted automation (`--apply` within the project's automation scope, `ecad apply <change-set> --expect <fingerprint>` for consequential changes, app-issued approval record bound to exact hash, revision and scope, atomically consumed; actor, scope and results recorded), AI and MCP proposal-only; no bypass of change-set approval through any interface. Content manifest committed with the files and verified at open, preview, apply and save; reconciliation of outside changes; missing, damaged and unexpected files; path safety (D43). Trust boundary documented (D31). Persistent change records (D17). Shared release-validation policy with available checks and extension points for later milestones. Progress and cancellation for long operations (D34) | **Behaviour yes** (MCP) | M10 |
| **M8** | Machine structure | Sections (=) and locations (+); device section and location; safety marking on sections; computed full designations with the project convention (D3, D4); **relabelling operations previewed and confirmed**; location boxes refer to locations; proposal from tag prefixes; `structure.json` | No (additive file) | M10, AI0, S0 |
| **M9** | Component library | #32 reconciled against D37 (PLATFORM-ARCHITECTURE.md sections 5.1 and 15): product model: one neutral schema with generic, commercial and custom kinds and placeholders (D39); unresolved selections in the BOM ("to be selected"), a check and release validation; canonical product and manufacturer ids with aliases (D7); immutable revisions and assets in `objects/` (D6); mechanical components; typed technical data with units and provenance; functions, terminal patterns and symbol mappings; mounting and clearances; accessories, compatible products and variants; user-created products; several libraries in order of preference; library-to-project three-way update; imports: CSV and Excel with **saved import profiles**; imports as change sets with rollback, **import reports**, duplicate detection against the library, **three-way conflicts against the source's last import**; unknown properties kept, nothing fabricated; automatic assignments Suggested; **import security**; **portable library packages**; explicit compatibility-checked product replacement and terminal reconciliation; library panel | No (additive) | M10, AI0, S0 |
| **S1** | Baselines, states and change impact | Baselines (draft, released, as-built, revision) with complete content, issued exports, transactional creation, integrity checks (D26, D27); **engineering states and modification packages** (D29) with **cancel, reopen, recovery and comparison,** one open at a time (D36); every engineering modification after as-built inside the open package, opened in one simple step, with reading, viewing, printing and preferences exempt; approvals bound to the exact reviewed revision and invalidated by later changes to what they cover; installation confirmed against exact released baseline plus reviewed commissioning deviations (D40); the manifest verified at baseline verification and restoration and at archive and restore (D43); **project archive and restore** with completeness and integrity checks; missing, damaged and unreferenced objects handled; cleanup never deletes referenced content (D38); extend AI0's change-impact engine with baseline comparison, physical work and issued documentation (D28); `ecad change-report --since`; replacement with **explicit terminal reconciliation,** distinct from removal (D23) | No (additive) | S0, AI0, M9 |
| **M11** | Equipment requirements | Requirement kinds per D12, added incrementally: motor, sensor, valve, actuator, operator device, power supply, PLC, remote I/O, drive, protection device, safety equipment, generic. Typed, unit-aware parameters with optional ETIM mapping (D14); design with generic products, then select commercial or custom products later as compatibility-checked replacements with explicit terminal reconciliation, keeping device identities, connectivity and drawings (D39); selection from the library with constraint checks; safety marking by product class | No (additive file) | M8, M9, S1 |
| **M12** | Circuit templates | Template format with typed parameters and options (D8); template library with revisions; instantiation into devices, terminals, nodes and undrawn wires; first templates: supply, DOL and reversing starters, 24 V supply, PLC DI and DO, sensor, pushbutton, lamp, safety relay | No (additive) | M9, M10, S0 |
| **C0** | CAD geometry model | `Graphic` extended into native 2D geometry: primitives (line, polyline, arc, circle, rectangle, text, hatch, image), layers, blocks with attributes, dimensions; on pages as annotations; rendering, PDF, DXF and DWG export; **SVG export;** the SVG library chosen against D35's criteria; geometry owned by the model with no rendering or UI dependency, CADCraft behind the adapter (D30) | No (additive) | M10 |
| **C1** | CAD tools | Drawing tools, grips, snapping (end, mid, centre, intersection, perpendicular, connection points), alignment and distribution, **rotation and scaling,** dimension tools, layer control; all in `ecad-editor`; geometry algorithms as small model functions, a dedicated adapter only on evidence (D30); graphics never change connectivity | No | C0 |
| **M13** | Schematic generation | Deterministic, **incremental** generation onto inserted pages (D9, D24); template drawings and default symbols; node drawings and cross-references; stored generated values, persistent deletion overrides, dependency-aware regeneration (D10) as an explicit, scoped operation; resource and space problems reported | No (additive fields) | M11, M12, S0, S1 |
| **C2** | Symbol and component geometry editors | Electrical symbol editor (geometry, connection points, fields, role), with **edits to an in-use symbol's connection points as an explicit operation** with impact preview; component geometry editor (footprint, front view, cut-out, drilling, mounting, clearances); **DXF and SVG import into geometry and adaptation of manufacturer geometry,** through the worker for untrusted files, with scripts, external resources, network and outside file access disabled and **unsupported or lossy conversions reported** (D35) | No | C1, M9 |
| **M14** | Documentation set | Unresolved selections marked "to be selected" in BOM pages and blocking release; device and wire data on drawings; second-language texts; generated pages (cover, contents, BOM, terminal strips, cable list, PLC I/O, legend, change report) in the frame; issued exports stored with baselines; consistency validation report | No | M13, C0, S1 |
| **M15** | Layouts | Cabinet plates and **doors with cut-outs and dimensions,** footprints, front views and **clearances** from product geometry; machine layout over a machine drawing, **cable routes** and lengths; new devices go to "not placed yet"; `plate-full` and `rail-full` checks; layout optimisation only as an explicit global operation | No | M8, M9, S0, C1, C2 |
| **M16** | Functional modules | Module library (sections, requirements, templates, parameters); instantiation; the conveyor zone as the first module | No | M13 |

### Order and dependencies

Each milestone needs only what is listed:

- **M7** is accepted with the documented R5 exception; **M10** is next. The conveyor is
  `crates/ecad-model/src/conveyor.rs`, its acceptance tests `tests/acceptance/`, the benchmark
  harness `crates/ecad-bench` with `bench/` (baselines, the controlled-run script).
- After M10: **S0**, **AI0**, **C0**.
- After AI0 and S0: **M8**, **M9**. After S0, AI0 and M9: **S1**.
- After M8, M9 and S1: **M11**. After M9, M10 and S0: **M12**.
- After M11, M12, S0 and S1: **M13**.
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
| **AI2** | Proposals | Change sets for machine structure and requirements from a description; component ranking with explanations; assistant review panel; each proposal approved in the app as one exact transaction | AI1 |
| **AI3** | Datasheet extraction | PDF text into suggested technical data, terminal patterns and sizes, with page citations; through import security | AI2, M9 |
| **AI4** | Templates and documentation | Template and option proposals; descriptions, notes and second-language texts | AI2, M12, M14 |
| **AI5** | External providers | Disabled by default, explicit project authorisation, confidentiality enforced, controlled audit log with retention (D20, D21) | AI1 |

### Parallel tracks

| Track | Scope | Can start after |
| --- | --- | --- |
| Symbols | Missing IEC symbols (inverter, transformer, heater, earth and screen, busbar, safety relay, switch, HMI, fan, PLC modules); black-box sections, captions, two-line labels, network ports; connectors with pinouts | M10 (drawn with C2's editor once it exists) |
| Manufacturer data | First: BMEcat with ETIM; **ETIM xChange**. Later, on need and licensing: ECLASS Advanced, AASX. On feasibility: DWG product geometry; STEP (with 3D, outside the initial scope). EDZ only after usage and licensing rights are verified. All through import security, import reports and conflicts; interoperability acceptance with real manufacturer files kept locally where licences forbid redistribution | M9 |
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
| M7 | The conveyor drawn schematic-first (a script of today's operations) resolves every wire end to a terminal or node, with the same ids on every load; physical group and point uniqueness; shared designation on both strip sides, explicit bridge and isolated levels; repeated representations do not create extra endpoints; netlist stable across save and load; deterministic smoke, instruction and allocation reference recorded; controlled Legion baseline deferred by owner decision (mandatory before first production release) |
| M10 | Format 2 saves and reads back identical; the M7 conveyor saved as format 1 converts to the same terminals (same ids twice), nodes and netlist, with byte-identical golden drawings; renumbering a physical terminal group changes its displayed designation without changing endpoint ids or wires; independent networks with identical L+/M labels stay separate; a wire added without drawing appears as not drawn; two products with the same number from two makers coexist. **An ambiguous fixture** (an interruption joining `L+` and `M`, a potential and an interruption named alike) converts with the nodes kept apart and each case reported; nothing is merged or reconnected |
| S0 | Stability test: adding a device, a wire, a terminal and a page changes only those. Joining two numbered potentials keeps both numbers and reports `wire-numbers-differ`. Removing `-B2` changes nothing else and retires it; the next sensor is `-B3` (sequential). A new DI for an unrelated device skips the reserved four. Allocating past the last free channel reports exhaustion and adds nothing. A page inserted after 3 is `3a`, and no other page file changes |
| AI0 | Every step of the conveyor script runs through the registry; MCP's tool list matches it. The same build as an assistant change set previews the expected additions with no new errors, applies only after acceptance, as one undo step; undo restores canonical engineering and graphical files byte for byte and appends an undo record; committed revision changes so stale approvals cannot revive; a change set against an older fingerprint is refused. **No MCP or CLI call applies an assistant change set; approval exists only in the app.** An approved change set modified afterwards, or approved against an older project state, is refused. A forty-operation proposal is approved as one transaction. An MCP client can't name itself a person. CLI editing commands without `--apply`, or in a project not allowing automation, change nothing. `ecad apply` of a reviewed change set succeeds at the expected fingerprint and is refused at any other; an assistant's change set handed to `ecad apply` is refused. Missing, forged caller-supplied, changed, revoked, out-of-scope, stale and consumed approval records are refused; consumption is atomic. The shared release gate rejects available blocking errors consistently. A file edited while Electric Studio is closed is compared against retained committed content, without a baseline; missing prior content yields an explicit incomplete comparison. External edits are reported at open, preview and apply, with their impact where recoverable; saving over it is refused until reconciled. Accepting it records an *external, unknown* change and invalidates the approvals it touches. A deleted file, a corrupted object and an unexpected file are reported as missing, damaged (quarantined) and ignored. A fixture with `..`, an absolute path and a symlink out of the folder is refused. A crash injected between files and manifest leaves them consistent. A long generation can be cancelled, leaving the project unchanged. A change to `-F3`, which feeds the safety relay, is flagged as indirectly safety-related. Every applied change set leaves a change record |
| M8 | Sections and locations as above; every device assigned; full designations (`=CONV.DRV+CAB.P-K1`) in reports. **Renaming `=CONV.DRV` previews the relabelling of its devices and changes nothing until confirmed.** Location boxes linked; proposing structure from tag prefixes on the schematic-first version gives the same tree |
| M9 | The test library imports from CSV, Excel and a package; a saved import profile imports next month's list the same way; each product keeps its id across re-imports and number changes. A hostile-file set (XXE, zip bomb, path traversal, oversize) is refused with nothing written; a fault injected mid-import leaves the library unchanged. Every import leaves a report. A likely duplicate (`EX-CT 9` and `EX-CT-9`) is listed, not merged. A field corrected in the library and changed by the next import of the same list is a conflict; a field only the list changed updates. Unknown columns survive. Class-default symbol mappings are Suggested. Mechanical products, an accessory, a variant and a user-created generic product appear in the BOM. Imported and manual data are unverified until a person verifies them. A generic "contactor 9 A" device is designed, drawn and wired, then a commercial product selected: same device id, terminals reconciled by a person, wires and drawings untouched; until then the BOM shows it "to be selected" and release validation fails. A custom component is a placeholder until marked complete. Library-to-project updates are three-way, with the base revision read from `objects/` |
| S1 | A staged retrofit fixture using existing schematic-first devices, products and explicit operations (no M11 requirements or M13 generation yet): baseline, reopen, add devices, compare, report and undo; run the applicable stability checks. The modification package goes proposed → approved → released, and the as-built baseline stays unchanged until it is marked installed. Objects added by it show *not installed yet*. Replacing `-Q1` keeps its id, tag, terminals, drawings, wires and position and is reported as *replace*. Replacing `-Q1` with a product missing one auxiliary terminal shows that terminal and its wire unmatched, and nothing is dropped until a person decides. Removing and re-adding is reported as removal plus addition with a new tag. A crash during baseline creation leaves no baseline or a complete one. The package can be cancelled (the project returns to the as-built baseline, the package is kept as cancelled), reopened (its changes reapplied), and survives a crash as open. A change made with no package open after as-built waits for a person to open one, in one step; viewing, searching, printing documents and changing panel layout need no package. Approving the package and then adding one more wire invalidates the approval and returns it to proposed. Release needs a valid approval and the common release gate; release doesn't mark anything installed. Release A, edit working B, confirm installation of A plus explicitly confirmed deviations: the as-built snapshot excludes uninstalled B, which remains pending. Crash recovery preserves the released reference and confirmation. `ecad archive` then `ecad restore` gives an identical project with every object verified. A removed object is reported missing with what needs it; a corrupted one is quarantined; cleanup removes an orphan and never a referenced object |
| M11 | `=SAF` and the safety relay are safety-related; an assistant change set touching `-K3` needs the separate confirmation; the assistant can't verify it or remove its classification; release through available paths fails for any required unverified imported, suggested or safety-related data, derived data with unresolved required inputs, incompatible selections, unmatched terminals and unresolved placeholders; draft export remains available. R1–R10 entered with typed units; products selected; R5's selection fails the check when the motor becomes 1.1 kW (2.6 A, outside the protector range) and passes after reselecting |
| M12 | The electrical design above built from templates with ports bound; the netlist equals the schematic-first conveyor's (same potentials, terminals matched by device and designation) |
| C0 | Annotations, layers and a block (the company logo) on the conveyor pages; PDF, DXF, DWG and SVG export with golden files; moving a block over a connection point connects nothing |
| C1 | Scripted CAD edits (draw, snap to end and intersection, align, rotate, scale, dimension) give exact coordinates; undo restores each; rotating or scaling a symbol never changes a wire |
| M13 | The full retrofit scenario, with step 5 by incremental generation. **Generating twice gives byte-identical canonical engineering and graphical files; the second run is a no-op.** After moving `-K1` and renumbering one wire by hand, then changing R4 to 0.75 kW and reselecting the motor and protector, scoped regeneration keeps the move and the number, updates the device data shown, and lists exactly the expected conflicts. A generated symbol deleted by hand is not recreated. Check: 0 errors |
| C2 | The safety relay's black-box symbol and the motor protector's footprint, mounting and clearances drawn in the editors. A manufacturer DXF and an SVG imported into product geometry and adapted. An SVG with a script, an external image and a `file:` reference imports with all three disabled and reported, and with its gradient and text-on-path listed as lossy. Moving an in-use symbol's connection point previews every affected drawing and terminal mapping and changes nothing until confirmed |
| M14 | Cover, contents, BOM, terminal strips, cable list, PLC I/O and change-report pages generated in the frame; BOM quantities equal the devices' products, mechanical items included; TIA tag table matches the PLC channels; issued exports stored with the released baseline and re-handed byte for byte; GUI, CLI and issued-document generation use the same release gate |
| M15 | Retrofit step 8: the second section's devices don't fit the plate, giving `plate-full` with no existing item moved; a contactor placed inside another's clearance gives a clearance warning. Every cabinet device on the plate with its product footprint; the door with cut-outs and dimensions; motor and sensors on the machine layout; cable routes and lengths for `-W1`–`-W3` within the expected range |
| M16 | The conveyor rebuilt from one "conveyor zone" module plus supply and control; same netlist as M12 |

Always, from M7 on:

- the conveyor builds and checks with no AI provider present (from AI1: also with the
  assistant switched off);
- the reference project and the samples still load;
- golden drawings unchanged unless a PR says why;
- deterministic benchmark counts within threshold, or explained; performance smoke tests
  pass;
- before a release: controlled benchmarks on the Legion, and the native Windows
  responsiveness check (separate acceptance, D41);
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
    - undo restores canonical engineering and graphical content byte for byte, retains append-only history and advances the committed revision; redo reapplies it;
    - it is saved, reopened and identical;
    - a crash injected mid-save leaves either the old or the new state.

Also tested:

- **Removal:** removing sensor `-B2` takes its terminals, drawings, wires, channel use and
  layout item away. Nothing is renumbered, `-B2` is retired, the freed DI becomes free (not
  reserved), and the report lists the rewiring.
- **Replacement:** `-Q1` gets a protector from another range. Its identity and everything
  attached is kept. A person confirms the proposed terminal mapping. The report lists the
  replacement, the mapping, and any terminal, footprint or clearance differences.
- **Retired identifiers:** after release, the next new sensor never gets `-B2`. Reusing `-B2`
  explicitly needs approval and appears in the change report.
- **States:** until the package is marked installed, the as-built baseline and the as-built
  documents are unchanged.

### Where it lives

- `crates/ecad-model/src/conveyor.rs`: the build script as operations, one function per
  milestone stage (registry-driven integration fixtures live in the appropriate higher-level
  crate, avoiding a model-to-editor dependency).
- `tests/acceptance/conveyor/`:
  - the test component library (CSV, Excel, package);
  - hostile-file fixtures (XML, archives, SVG, DXF);
  - ambiguous format 1 node fixtures;
  - expected netlist, BOM, reports and change reports;
  - golden pages.
- `ecad new-sample <dir> conveyor` writes it.
- CI runs it in the existing test job; no new infrastructure.
- Real manufacturer files for interoperability acceptance are run locally where their licences
  forbid adding them to the public repository.

## Decisions

**All architectural decisions D1–D43 are resolved with conditions.** The record is in
[PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md#decision-record). None blocks a
milestone.

Details that are settled within their milestones, not now, are listed in
[PLATFORM-ARCHITECTURE.md section 16](PLATFORM-ARCHITECTURE.md#16-decided-now-and-left-to-the-milestones).
A milestone that finds one of them needs an architectural change brings it back as a new
decision.

### M7 closure and performance follow-up

**Decision (owner approved, 27 Nov 2026): M7 accepted; R5 deferred, not passed.** PR #35 merged at `e258a59ed2d437691497d653c6247e2d41af7168` after exact-head `90d6dfbf4b707aeba99256d727c8da26a8c09f58` passed all six hosted jobs ([CI](https://github.com/kandyman991/ELECTRIC_STUDIO/actions/runs/46037120317), [build](https://github.com/kandyman991/ELECTRIC_STUDIO/actions/runs/46037120356)). R1–R4 correction evidence and allocation reference are [retained on the PR](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7226987361). [Owner decision](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7228169838): temporary lack of Legion access does not block M10. No controlled wall-clock or peak-RSS baseline has been measured. Run the qualified D41 benchmark on the Legion when access returns, and retain evidence before the first production release ([#39](https://github.com/kandyman991/ELECTRIC_STUDIO/issues/39)); the pre-release gate and native Windows responsiveness requirement remain mandatory. No relaxation of ongoing CI checks or engineering acceptance criteria. The observed superlinear checks and render costs remain a follow-up: establish the baseline, profile multiple sizes and remove repeated work before synchronous interactive workflows depend on it. Sandbox wall-clock figures are not a merge gate.
