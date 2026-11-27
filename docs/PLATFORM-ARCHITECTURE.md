# Machine-centric platform: architecture proposal

**Status: proposal for review.** Nothing here is implemented. No breaking model change and
no implementation work (including the component library in draft PR #32) starts until
this is approved. Open points are listed under
[Decisions to make](#decisions-to-make), each with a proposed default.

Companion documents: [GAPS.md](GAPS.md) (what the product is missing) and
[ROADMAP.md](ROADMAP.md) (milestones, acceptance criteria, the conveyor acceptance project).

## 1. Direction

Electric Studio becomes a machine-centric electrical engineering platform. The main
workflow:

1. Define the machine: its functional sections and installation locations.
2. Define equipment requirements (motors, sensors, actuators...) without choosing a
   manufacturer.
3. Select physical products from the shared, manufacturer-independent component library.
4. Develop the electrical system from devices, connectivity and reusable circuit templates.
5. Generate the schematics and refine them by hand.
6. Develop cabinet and machine layouts with the shared CAD capabilities.
7. Generate synchronised documentation and validate consistency.

Schematic-first engineering, as it works today, stays fully supported. Both workflows edit
the same model. A project drawn by hand is a machine-centric project with no structure, no
requirements and no templates yet, and can gain them later.

Principles:

- Evolve the existing implementation; no rewrite and no parallel model.
- One source of truth per fact; everything else is derived and regenerated.
- No speculative abstraction: each concept enters when a milestone needs it, and the
  conveyor acceptance project exercises it.

## 2. Audit of the current implementation

At `main` (06b34fb, after M6) the workspace is about 22 000 lines of Rust in ten crates.

### What exists

| Area | Current state | Verdict |
| --- | --- | --- |
| Command layer | One `Command` enum; `Project::apply` validates and returns the inverse; `Batch` is all or nothing; `History` for undo | **Retain.** It is the backbone every workspace, the CLI and MCP already share |
| Identity | UUID v4 newtypes for pages, devices, functions, connections, cables, boxes, panel items. Symbols by string id, parts by order number | **Retain and extend** with the new entities |
| Device | Tag (IEC 81346 text, may embed `=` and `+`), description, parts | **Extend:** section, location, requirement, product |
| Function | One drawing of a device: symbol, page, position, rotation, mirror, designations, free `properties` (PLC address, symbol, comment) | **Retain** as the graphical representation. PLC data later moves to typed fields |
| Connection | Wire between two *drawn* connection points (`FunctionId` + point), with number, cross-section, colour, cable core and the bend of its drawn route | **Migrate.** Connectivity is bound to drawings, so it can't exist before a schematic. This is the central change (section 4.5) |
| Potentials, nets | Derived from drawn connections, junctions, potential and interruption points | **Retain** as derived, computed from model connectivity |
| Location | Only in tag text and in location-box labels (graphics); panel items on layout pages | **Migrate:** locations become model entities; boxes refer to them |
| Parts | Per-project catalogue by number. M7a (draft #32) adds class, technical data, data sheet, and a shared library | **Retain #32's core** as the product-definition layer, after review against this proposal |
| Symbols | Built-in IEC library plus project symbols (black boxes) | **Retain;** library symbols come later |
| Layout | Mounting plate per layout page: rails, ducts, device footprints; wire lengths through ducts | **Retain;** extend to door and machine layouts |
| Checks | `ecad-rules` over derived connectivity | **Extend** with requirement, product and structure checks |
| Reports | CSV only, derived | **Extend** to generated pages |
| Storage | Folder of JSON; transactional save with journal, lock, fingerprint; loads replay through commands | **Retain;** format 2 adds files (section 4.6) |
| Views | `ecad-view`, the only CADCraft user: render, PDF, DXF, DWG, underlays | **Retain** as the shared CAD layer |
| Editing | `ecad-editor` (Editor, Session, Frame); egui shell; CLI; MCP | **Retain;** new workspaces are new Frames over the same Session |

### Technical debt this direction exposes

1. **Connectivity lives in the drawing.** `ConnectionRef` names a function and a symbol
   point, so a wire can't exist until both ends are drawn. That blocks steps 3 and 4.
2. **Location is graphical.** A location box's label decides where devices are. That breaks
   "the model is the source of truth" (AGENTS.md rule 1).
3. **Structure is in tag text.** `=A1+B2-K1` is a string, so nothing can be grouped, filtered
   or validated by section or location.
4. **PLC channel data is stringly typed** (`Function.properties`).
5. **Terminal identity is implicit.** A strip terminal is a function whose number is the
   designation of its first point; its two sides are the symbol's points `1` and `2`.
6. **Library items are copied without provenance.** Nothing records where a project's part or
   symbol came from, or at which version.
7. **No stable content hash.** The fingerprint uses `DefaultHasher`, which is fine within a
   run but not stable across Rust versions. Revisions and generation need a stable hash.

## 3. Canonical engineering model

Six layers, each with one owner of its facts. Arrows mean "refers to".

```text
 Machine structure      Section (=)  ─┐        Location (+)
                                      │             ▲
 Requirements           Requirement ──┼─────────────┤   what is needed, maker-independent
                             ▲        │             │
 Products (libraries)   Part ◄───────────── selected for
                             ▲        │             │
 Device instances       Device ──────┴─────────────┘   tag, section, location, requirement, parts
                             ▲
 Connectivity           Terminal (device + designation) ◄── Wire, Cable core
                             ▲
 Representations        Function (symbol on a page) · wire graphics · layout items · generated pages
```

| Layer | Entity | Owns | Refers to |
| --- | --- | --- | --- |
| Machine structure | **Section**: a functional unit, IEC 81346 function aspect (`=CONV.DRV`) | Name, description, parent, order | Parent section |
| | **Location**: where things are installed, IEC 81346 location aspect (`+CAB.P`, `+FLD`) | Name, description, parent, kind (cabinet, plate, door, field, junction box) | Parent location |
| Requirements | **Requirement**: what a section needs, independent of maker | Equipment kind, parameters (0.55 kW, 400 V 3~, reversing, PNP NO...), quantity | Section, location |
| Products | **Part**: an orderable product (today's `Part`, extended by #32) | Number, maker, class, technical data, terminal pattern, symbol mapping, footprint | — (library item) |
| Device instances | **Device**: one installed thing with a tag | Tag, description, assignments | Section, location, requirement, parts |
| Connectivity | **Terminal**: a connection point of a device, identified by (device, designation) | — (identified, not stored separately) | Device |
| | **Wire**: a conductor between two terminals | Number, cross-section, colour, cable core | Two terminals, cable |
| | **Cable** (exists) | Tag, type, cores, shield | Part (later) |
| | **Potential** (derived) | — | Computed from wires |
| Representations | **Function**: a symbol on a page showing some of a device's terminals | Position, rotation, mirror, displayed designations | Device, page, symbol |
| | **Wire graphics**: how a wire is drawn on a page | Bend, which drawings it joins | Wire, functions |
| | **Layout item** (exists): rail, duct, device footprint | Position, length | Device, page |
| | **Generated page** | Nothing stored but its settings: content is regenerated | Report kind |

Rules:

- **Every layer except structure is optional.** A schematic-first project has devices,
  wires and drawings only. Sections, locations, requirements and templates add to it, and
  never become a precondition.
- **Requirement versus product.** A requirement is intent and a part is a selection. They
  are never synchronised automatically; validation reports where a selection doesn't meet
  its requirement.
- **Tags stay human-facing.** `Device.tag` is the product-aspect designation (`-K1`). The
  full designation (`=CONV.DRV+CAB.P-K1`) is computed from section and location when the
  naming setting asks for it. Existing tags that embed `=` or `+` keep working as text. A
  command can propose sections and locations from them, but nothing changes without
  confirmation.

## 4. Identity, ownership, relationships, versioning, persistence

### 4.1 Identities

| Entity | Identity | Stable across |
| --- | --- | --- |
| Section, location, requirement, device, wire, cable, page, function, box, layout item, template instance | UUID (as today) | Renames, moves, regeneration |
| Generated entity | UUID v5 from (template instance or generator id, local key), so regenerating gives the same ids | Regeneration |
| Terminal | (device id, designation), plus a side for two-sided terminals and the drawing for virtual points (section 4.5) | Moves; a rename of the designation updates every wire in the same command batch |
| Part | (library id, manufacturer, order number), compared without case. In a project's catalogue, the order number as today | Library moves (the library id is a UUID in `library.json`) |
| Symbol, template, module | Namespaced string: `iec.coil`, `<library>.<name>` | Library moves |
| Library | UUID in its `library.json` | Folder renames, network paths |

### 4.2 Ownership

- **The project owns** its structure, requirements, devices, connectivity, drawings, layouts
  and settings. It also owns a **copy** of every library item it uses: parts, symbols,
  templates. A project opens, checks and exports without any library, as today.
- **Libraries own** the masters: components, symbols, circuit templates, functional
  modules. A library is a folder like a project, saved by the same store (all or nothing,
  locked, refused over someone else's save; implemented for components in #32).
- **Generators own** nothing persistent. Generated content belongs to the project and
  records where it came from (section 6).

### 4.3 Relationships and integrity

- References are by id and checked on every command (as today: a device can't be removed
  while drawn; a part can't be removed while used). New rules follow the same pattern: a
  section in use can't be removed, and moving a section updates its children.
- Deleting a requirement doesn't delete its devices; they lose the reference and a check
  reports them.
- Derived data (potentials, full designations, cross-references, BOM, wire lengths, report
  pages) is never stored. It is computed and, where costly, cached in memory.

### 4.4 Versioning and provenance

- Each library item has a **revision**: a stable hash (SHA-256 of its canonical JSON)
  computed on save.
- A project copy records its **source**: library id, item key and the revision it was taken
  at.
- "Update from library" is a three-way comparison per field. The base is the copy's source
  revision, "theirs" is the library's current revision and "ours" is the project copy.
  - Fields only the library changed are updated.
  - Fields only the project changed are kept.
  - Fields both changed are listed as conflicts and decided by the user.
- This replaces #32's simpler "newer fills in" rule for library-to-project updates. The fill-in
  rule stays for importing manufacturer lists into a library.
- Project files themselves are versioned by the user's tools (git, backups). The project
  keeps no revision history beyond undo, and the title block's revision list stays
  documentation.

### 4.5 Connectivity (the central change)

Today: `Connection { from: ConnectionRef { function, point }, to: ... }`.

Proposed:

```text
Wire {
  id, number, cross_section, colour, cable_core,          // as today
  from: End, to: End,
}
End {
  device: DeviceId,
  terminal: String,           // the designation: A1, 13, 3 (terminal number), L+
  side: Option<Side>,         // the two sides of a strip terminal, when it matters
  drawing: Option<FunctionId> // which drawing the end is drawn at; required for virtual points
}
```

- **A wire exists without a drawing.** Ends with `drawing: None` are undrawn. The wire
  appears in the wire list, netlist and checks ("not drawn yet"), and schematic generation
  draws it.
- **A drawn wire is an ordinary wire** whose ends name their drawings. Drawing a wire by
  hand creates the same record as today, so schematic-first editing behaves exactly as now.
- **Physical devices:** a designation is unique on its device across all its drawings (a
  new invariant, checked). The drawing can then always be found from the terminal, and a
  wire to a terminal drawn later attaches automatically.
- **Virtual points** (junctions, potential points, interruption points) have no physical
  terminal. Their ends keep the drawing, and the netlist resolves through them as today.
- **On disk,** connectivity leaves the page files for a project-level `wires.json`. Page
  files keep only how each wire is drawn there (`bend`). Format 1 projects are read and
  converted; section 9 covers the migration.

### 4.6 Persistence (format 2)

```text
my-machine/
  project.json        format 2, name, pages, settings
  structure.json      sections and locations                  new
  requirements.json   requirements                            new
  devices.json        devices (+ section, location, requirement)
  wires.json          wires with terminal ends                moved out of pages
  cables.json, parts.json, symbols/   as today (+ source/revision of copies)
  templates/<id>.json copies of templates used                new
  instances.json      template and module instances, parameters, generation state   new
  pages/<id>.json     drawings, wire graphics, boxes, layout items, annotations
  underlays/<id>.json as today
```

The same transactional store, lock and fingerprint apply. Everything stays pretty-printed
and sorted, so it diffs in git. Files appear only when used, so a schematic-first project
looks almost as it does today.

## 5. Libraries

All four kinds are folders saved by the same store. The project keeps copies of what it
uses (section 4.2).

| Library | Holds | Status |
| --- | --- | --- |
| **Components** | Parts by maker and number: class, technical data, data sheet; then terminal pattern, symbol mapping, footprint, accessories | Core in draft #32 (import from CSV/Excel). BMEcat/ETIM next; ECLASS and AASX later |
| **Symbols** | IEC symbols and black boxes | Built-in today; library folders when users need their own |
| **Circuit templates** | Reusable circuits: devices with roles, internal wires, ports, drawing | New |
| **Functional modules** | Machine units: sections, requirements, chosen templates, parameters | New, after templates are proven |

### 5.1 Components (integrating the agreed component library)

What #32 built stays the base: a shared library, technical data, imports that never wipe
data, and take/refresh into projects. This proposal adds, in order of need:

1. **Revision and source** (section 4.4).
2. **Terminal pattern:** a product's physical terminals and what each is (coil A1/A2, NO
   13-14, main 1-2...). Today this is partly `contacts`. It lets the model check that a
   device's drawn terminals exist on its product.
3. **Symbol mapping:** which symbols represent the product (a contactor is a coil, three
   main contacts and its auxiliaries), with a default per class, so a selected product can
   be drawn.
4. **Footprint and front view:** size exists; 2D outlines come from manufacturer DXF through
   the shared CAD layer.
5. **Numeric technical data:** value and unit parsed where possible, so selection can
   compare a requirement's 0.55 kW with a part's data.

### 5.2 Circuit templates

A template is data, not code:

- **Devices:** a local name (`motor`, `contactor_fwd`), role, tag prefix, required product
  class, and the symbols each is drawn with.
- **Internal wires** between template terminals.
- **Ports:** named connections to the outside (`L1 L2 L3 PE`, `24V 0V`, `DI`, `DO`) bound
  when instantiated.
- **Drawing:** the template's symbols and wire routes, drawn once by a person on a template
  page, relative to an anchor.
- **Options:** booleans that include or leave out blocks (`reversing`, `thermal_contact`).
  Parameters substitute into texts. No loops and no scripting language; anything more
  waits for a real need.

Instantiating a template creates devices, terminals and wires in the model (no drawing),
recorded as a **template instance** with its parameters and port bindings. Drawing them is
generation's job (section 6).

### 5.3 Functional modules

A module is a machine unit: sections, requirements and the templates that realise them, for
example "conveyor zone with drive and two photoelectric sensors". It is built only after
the conveyor acceptance project has been done with templates alone.

## 6. Schematic generation

### 6.1 What it does

It draws what the model has and the pages don't yet show:

- template instances (their template drawing placed on pages);
- devices without a template (by their product's or class's default symbols);
- undrawn wires;
- interruption points and cross-references where a potential continues on another page.

It doesn't invent topology and doesn't auto-route. The layout comes from the template
drawings, arranged in a deterministic order on the sheet's grid: by section, then template
instance, then tag. A new page starts when a template doesn't fit. This keeps generation
explainable, testable and stable.

### 6.2 Determinism

Generation is a pure function `(model, generation settings) -> commands`:

- no randomness, no clock, no hash-map order;
- ids from UUID v5 (section 4.1);
- all ordering by stable keys.

Generating twice gives byte-identical project files, and that is an acceptance test. It
lives in `ecad-model` (it needs symbol geometry, not CADCraft), so the editor, CLI and MCP
run the same code.

### 6.3 Non-destructive regeneration

Each generated item (function, wire graphics, interruption point, generated page) carries:

```text
origin: { generator or template instance, local key }
generated: { placement_hash, content_hash }   // what the generator last produced
```

On regeneration each item is compared three ways: what was generated last time, what is
there now, and what would be generated now.

| Now versus last generated | Generator now | Result |
| --- | --- | --- |
| Unchanged | Produces it | Replaced by the new output |
| User changed placement (moved, rotated) | Produces it | Placement kept; content updated; recorded as *pinned* |
| User changed content (designations, wire number) | Produces it differently | User's version kept; listed as a conflict with the generator's proposal |
| Unchanged | No longer produces it | Removed |
| User changed it | No longer produces it | Kept, marked orphaned, listed |
| — | New item | Added |

- Regeneration is one command batch, so one undo step. It shows a preview (a dry run listing
  additions, updates, kept changes and conflicts) before it applies.
- **Detach** turns a generated item into a manual one, so the generator never touches it
  again.
- Manual items (no origin) are never touched. A hand-drawn schematic is all manual, so
  generation adds only what is missing.

## 7. Workspaces and shared data

| Workspace | Shows and edits |
| --- | --- |
| Machine structure | Sections and locations; device assignment |
| Requirements | Requirements per section; selection status |
| Devices and products | Devices, parts, library search and selection |
| Schematic | Pages (today's editor) |
| Cabinet layout | Plates and doors (today's layout pages, extended) |
| Machine layout | Field locations on a machine drawing; cable lengths |
| Documentation | Generated pages, exports, validation |

- One `Session` per open project holds the project, history and selection. Every workspace
  is a `Frame` over it and edits only through commands.
- Selection is shared: picking a device in the structure tree shows it in the schematic and
  the layout.
- Undo is one history across workspaces.
- CLI and MCP reach the same commands; each milestone adds its CLI commands and MCP tools
  with its model work.
- Libraries are opened alongside the project. Edits to a library are commands on the
  library, saved by its own store. The project only takes copies.

## 8. Shared CAD

The shared CAD layer is `ecad-view` over CADCraft: geometry, rendering, PDF, DXF and DWG
export, and DXF/DWG import (underlays). The rules stay as they are:

- only `ecad-view` depends on CADCraft;
- the model, not the drawing, is the source of truth;
- CADCraft's own editing engine isn't used.

Schematic pages, cabinet layouts, machine layouts, template drawings and generated pages all
render through it. Product footprints and front views come from manufacturer DXF through
the same reader as underlays. A machine drawing from mechanical CAD is an underlay on a
machine-layout page.

*To confirm:* this repo has no record of the "shared CAD requirements" agreed earlier beyond
the CADCraft decision. If they cover more (3D, a common CAD kernel with another product, or
editing geometry in CAD), they need adding here (decision D11).

## 9. Migration

| Change | Migration | Breaking? |
| --- | --- | --- |
| Wires with terminal ends; `wires.json` | Format 2 reader converts format 1: each end's function and point become (device, designation of that point, drawing) | File format yes, behaviour no |
| Designation unique per physical device | Checked on load; a format 1 project that breaks it loads, and a check reports it with a fix command | No |
| Sections and locations | New, empty. Optional command proposes them from tag prefixes and location-box labels | No |
| Location boxes | Gain an optional location reference; the label stays for boxes that aren't locations (connector housings, `EXT`) | No |
| Device assignments | New optional fields | No |
| PLC channel data | `properties` keys become typed fields, read from the old keys | File format yes |
| Parts | #32's fields plus source/revision; old files read unchanged | No |
| Stable hash | New, for revisions and generation; the folder fingerprint stays as it is | No |

- Format 2 is written only after a format 1 project is saved. The first save tells the user
  the project will no longer open in older versions.
- The samples and the reference project are converted and their golden drawings must stay
  identical: the same drawing from the new model.

## 10. Risks and dependencies

| Risk | Mitigation |
| --- | --- |
| The connectivity change touches routing, naming, checks, reports, view, editor and MCP | First a terminal-resolver layer with no format change (M7). Then the storage switch (M10) behind the existing tests and golden files |
| Terminal identity by designation: renumbering must carry the wires | Rename is one command that rewrites every end in the batch; property test over random renames |
| Two truths (requirement versus selection, template versus edited drawing) | Never synchronised silently: validation and regeneration report differences |
| Generation scope creep (auto-placement, auto-routing) | Template-based placement only; no auto-router in the roadmap |
| Template format too weak or too clever | Options and substitution only; the conveyor must be built with it; extend on evidence |
| Technical data as text can't be compared | Numeric parsing with units in M9; selection warns, never blocks, when data is missing |
| UI workload: seven workspaces in egui | Each milestone ships model, CLI and MCP first, with a minimal panel; UI polish is its own track |
| CADCraft 0.x changes | Pinned commit and golden files (as today) |
| PR #32 was built before this proposal | Reconcile in M9: keep its core, add revision/source, review its merge rule (section 4.4) |
| Performance on large projects | Derived data cached per Session; measured on the reference and conveyor projects |
| Functional safety is out of scope | Safety circuits are documented, not validated; stated in the docs and the UI |

Dependencies: M7 comes before M10; M8 before M11; M9 before M11 and M12; M10 before M12; M12 before
M13; M13 before M16. Details are in [ROADMAP.md](ROADMAP.md).

## Decisions to make

Each has a proposed default, used unless the review decides otherwise.

| # | Decision | Proposed |
| --- | --- | --- |
| D1 | Canonical connectivity: terminal-to-terminal wires (drawing optional) or drawn wires plus a separate logical overlay | **Terminal-to-terminal wires.** One truth; an overlay would need constant reconciliation |
| D2 | Terminal identity: (device, designation) with rename cascade, or a stored id per terminal | **(device, designation)**: readable files, no extra entity |
| D3 | Full designations: computed from section and location, or stored in the tag | **Computed;** tags stay `-K1` |
| D4 | IEC 81346-2 letter codes: 2009 (`-K`, `-S`) or 2019 (`-KF`, `-SF`) | **Project setting,** default as today |
| D5 | Library storage: folders of JSON or a database (SQLite) | **Folders of JSON,** as projects; revisit above ~100 000 parts per maker |
| D6 | Library versions: content-hash revisions or semantic versions | **Content hash;** a human label optional |
| D7 | Projects copy library items or only reference them | **Copy,** with source and revision |
| D8 | Template expressiveness | **Options and text substitution only** |
| D9 | Generation: template placement only, or automatic placement and routing | **Template placement only** |
| D10 | Regeneration granularity | **Per item, placement and content separately** |
| D11 | What the agreed "shared CAD requirements" include beyond CADCraft for 2D | **To confirm by the owner** |
| D12 | First requirement kinds | **Motor, sensor, valve or actuator, operator device (button, lamp, switch), power supply, generic** |
| D13 | PR #32 | **Keep as draft;** after approval rebase it as M9's base, adding revision/source |
| D14 | Requirement parameters: free or aligned with ETIM features | **A small fixed set per kind, mapped to ETIM features where one exists** |
| D15 | Concurrency | **One writer per folder (the lock), as today;** no live multi-user editing |
