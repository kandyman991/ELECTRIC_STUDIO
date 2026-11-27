# Machine-centric platform: architecture proposal

**Status: revised after the owner's review of D1–D28 (27 Nov 2026), for a second independent
review.** Nothing here is implemented. No implementation work (including the component library
in draft PR #32) starts until this revision is approved. The decision record, with each
decision's approved conditions, is at the end: [Decision record](#decision-record). Where an
approved decision conflicts with what the code does today, section 14 says so.

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
- **Stable across the machine's life.** Initial engineering, manufacturing, commissioning,
  maintenance, extensions and retrofits happen in the same project. Adding or changing
  equipment never changes unrelated objects or drawings (section 12, a foundational
  invariant).
- **AI-ready, AI-independent.** All engineering works without AI. An optional assistant
  (local first, external providers later) uses the same typed queries and validated
  operations as every workspace. Each of its change sets needs a person's acceptance
  (section 11).
- **Designed is not installed.** The approved design, a released design and the machine as
  built are distinct states (section 12.7).

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
| Parts | Per-project catalogue by order number. M7a (draft #32) adds class, technical data, data sheet, and a shared library keyed by (manufacturer, number) | **Retain #32's core** as the product layer; **migrate** to canonical product ids (D7) |
| Symbols | Built-in IEC library plus project symbols (black boxes); `Graphic` is line, dashed, circle, text | **Retain;** `Graphic` becomes the native CAD geometry (section 8) |
| Layout | Mounting plate per layout page: rails, ducts, device footprints; wire lengths through ducts | **Retain;** extend to door and machine layouts |
| Checks | `ecad-rules` over derived connectivity | **Extend** with requirement, product and structure checks |
| Reports | CSV only, derived | **Extend** to generated pages |
| Storage | Folder of JSON; transactional save with journal, lock, fingerprint; loads replay through commands | **Retain;** format 2 adds files (section 4.6) |
| Views | `ecad-view`, the only CADCraft user: render, PDF, DXF, DWG, underlays | **Retain** as the shared CAD layer |
| Editing | `ecad-editor` (Editor, Session, Frame); egui shell; CLI; MCP | **Retain;** new workspaces are new Frames over the same Session |
| AI access | MCP server: typed read tools, editing tools through the editor's script API and undo history, read-only unless `--write`, schema-checked arguments | **Retain** as the first AI interface. Missing: proposals with preview, a record of who made a change, provenance of AI-suggested data, safety approval rules (section 11) |

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
7. **Operations are defined three times.** The editor (UI events), the script API (CLI, MCP)
   and the MCP tool table each describe editing operations their own way. An assistant needs
   one typed catalogue (section 11.2).
8. **History doesn't record who made a change.** An edit by an MCP client and an edit by hand
   look the same afterwards.
9. **Edits change unrelated objects.** Four places break the stability invariant of section
   12:
   - with one number per potential, connecting two potentials renumbers the existing wires
     of one of them (`number_for_new_wire`);
   - new tags, terminal numbers and PLC channels take the lowest free number, so a deleted
     `-K2` comes back as the next new device;
   - inserting or moving a page rewrites the `order` of every page (`ReorderPages`);
   - a page inserted between 3 and 4 can't be called `3a`; it takes the next free number.
10. **No baselines.** A released or as-built state exists only as an export or a git commit.
11. **No stable content hash.** The fingerprint uses `DefaultHasher`, which is fine within a
   run but not stable across Rust versions. Revisions and generation need a stable hash.

## 3. Canonical engineering model

Six layers, each with one owner of its facts. Arrows mean "refers to".

```text
 Machine structure      Section (=)  ─┐        Location (+)
                                      │             ▲
 Requirements           Requirement ──┼─────────────┤   what is needed, maker-independent
                             ▲        │             │
 Products (libraries)   Product ◄────────── selected for
                             ▲        │             │
 Device instances       Device ──────┴─────────────┘   tag, section, location, requirement, products
                             ▲
 Connectivity           Terminal (TerminalId) · Node (NodeId) ◄── Wire, Cable core
                             ▲
 Representations        Function (symbol on a page) · node drawings · wire graphics · layout items · CAD geometry · generated pages
```

| Layer | Entity | Owns | Refers to |
| --- | --- | --- | --- |
| Machine structure | **Section**: a functional unit, IEC 81346 function aspect (`=CONV.DRV`) | Name, description, parent, order, safety marking | Parent section |
| | **Location**: where things are installed, IEC 81346 location aspect (`+CAB.P`, `+CONV`) | Name, description, parent, kind (cabinet, plate, door, field, junction box) | Parent location |
| Requirements | **Requirement**: what a section needs, independent of maker | Kind, typed parameters with units, quantity, optional ETIM mapping | Section, location |
| Products | **Product**: an orderable item, electrical or mechanical (today's `Part`, extended by #32) | Canonical product id, manufacturer id, order number and aliases, class, technical data, terminal pattern, symbol mapping, 2D geometry | Manufacturer (library items) |
| Device instances | **Device**: one installed thing with a tag | Tag, description, assignments, safety marking | Section, location, requirement, products |
| Connectivity | **Terminal**: a physical connection point of a device, with a persistent `TerminalId` | Designation (editable), side, kind (coil, contact pole, strip side, PLC channel, port) | Device |
| | **Node**: a logical connection point with no physical terminal, `NodeId`: a junction, a named potential (`L+`, `M`), an interruption pair | Kind, name | — |
| | **Wire**: a conductor between two ends, each a terminal or a node | Number, cross-section, colour, cable core | Two ends, cable |
| | **Cable** (exists) | Tag, type, cores, shield | Product (later) |
| | **Potential** (derived) | — | Computed from wires, terminals' internal bridges and nodes |
| Representations | **Function**: a symbol on a page showing some of a device's terminals | Position, rotation, mirror; which symbol point shows which `TerminalId` | Device, page, symbol, terminals |
| | **Node drawing**: a junction dot, potential arrow or interruption point showing a node | Position, rotation | Node, page |
| | **Wire graphics**: how a wire is drawn on a page | Bend, which drawings it joins | Wire, functions, node drawings |
| | **Layout item** (exists): rail, duct, device footprint, cut-out | Position, length | Device, product, page |
| | **CAD geometry**: lines, arcs, polylines, text, dimensions, hatches, blocks, on layers (section 8) | The geometry | Page, symbol, product, or block |
| | **Generated page** | Nothing stored but its settings: content is regenerated | Report kind |

Rules:

- **Every layer except structure is optional.** A schematic-first project has devices,
  terminals, nodes, wires and drawings only. Sections, locations, requirements and templates
  add to it, and never become a precondition.
- **Requirement versus product.** A requirement is intent and a product is a selection. They
  are never synchronised automatically; validation reports where a selection doesn't meet
  its requirement.
- **Tags stay human-facing.** `Device.tag` is the product-aspect designation (`-K1`). The
  full designation (`=CONV.DRV+CAB.P-K1`) is computed from section, location and the
  project's designation convention (D3, D4).
  - **It never relabels installed equipment silently.** Any operation that would change a
    device's full designation (renaming or moving a section or location, reassigning a
    device, changing the convention) is a relabelling operation. It is previewed, it lists
    every device whose printed designation changes, and it needs explicit confirmation;
    after a released baseline, a separate approval too.
  - Existing tags that embed `=` or `+` keep working as text. A command can propose sections
    and locations from them, but nothing changes without confirmation.

## 4. Identity, ownership, relationships, versioning, persistence

### 4.1 Identities

| Entity | Identity | Stable across |
| --- | --- | --- |
| Section, location, requirement, device, wire, cable, page, function, box, layout item, template instance, baseline, modification, change record | UUID (as today) | Renames, moves, regeneration |
| **Terminal** | **`TerminalId`**, a persistent UUID. The designation (`A1`, `13`, `3`) is an editable property, not the identity (D2) | Renames, renumbering, redrawing, replacing the device's product |
| **Node** (junction, named potential, interruption) | `NodeId`, a persistent UUID | Renames, redrawing |
| Generated entity | UUID v5 from (template instance or generator id, local key), so regenerating gives the same ids | Regeneration |
| **Product** | **Canonical product id**: a UUID given by the library when a product is first created. Manufacturer and order number are properties with aliases, matched on import, never the identity (D7) | Order-number changes, manufacturer renames, the same number from two makers |
| **Manufacturer** | UUID in the library, with name aliases (`ABB`, `ABB S.p.A.`) | Renames, spelling variants |
| Symbol, template, module, CAD block | Namespaced string: `iec.coil`, `<library>.<name>` | Library moves |
| Library | UUID in its `library.json` | Folder renames, network paths |
| Library item revision | SHA-256 of its canonical content | Never changes; the content is immutable |

Ids (UUIDs) are never reused. Human-facing identifiers (tags, wire and cable numbers, terminal
designations, PLC addresses, page names) follow the numbering policy of section 12.2: never
changed as a side effect, never reused automatically after release.

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

- **Revisions.** Each library item has a revision: the SHA-256 of its canonical content,
  computed on save.
- **Immutable revision contents** (D6). Every revision a project has taken, and its
  dependent assets (symbols, CAD blocks, geometry, data-sheet files when kept), is stored
  content-addressed in the library's `objects/` and, for what a project uses, in the
  project's `objects/`. Old revisions stay readable, so three-way comparison always has its
  base.
- **Source.** A project copy records its source: library id, canonical product id (or item
  key) and the revision it was taken at.
- **Pinned.** A project copy never changes because the library changed. Updating it is an
  explicit operation with a preview (section 12.5), so a released machine keeps the component
  revisions it was built with.
- **"Update from library"** is a three-way comparison per field. The base is the copy's
  source revision (read from `objects/`), "theirs" is the library's current revision and
  "ours" is the project copy.
  - Fields only the library changed are updated.
  - Fields only the project changed are kept.
  - Fields both changed are listed as conflicts and decided by a person.
- **Imports into a library** keep #32's fill-in rule (an import never wipes data), and each
  import creates new revisions, never rewrites old ones.
- **Provenance and verification** are tracked separately (section 11.4, D17).
- **Project revisions** are baselines (section 12.4): immutable snapshots inside the project
  folder, with no git needed. Git or backups still work on the folder as before.

### 4.5 Connectivity (the central change)

Today: `Connection { from: ConnectionRef { function, point }, to: ... }`. Terminals and
junctions are implicit: a terminal is a symbol point of a drawing, and a junction is a
device drawn with `iec.junction`.

Approved (D1, D2):

```text
Terminal { id: TerminalId, device: DeviceId, designation: String, side: Option<Side>, kind }
Node     { id: NodeId, kind: Junction | Potential { name } | Interruption { name } }
Wire     { id, number, cross_section, colour, cable_core, from: End, to: End }
End      = Terminal(TerminalId) | Node(NodeId)
Function { ..., shows: { symbol point -> TerminalId } }      // the drawing of a device
NodeDrawing { id, node: NodeId, page, position, symbol }       // a dot, an arrow, an interruption point
WireGraphics { wire, page, bend, from_drawing, to_drawing }    // how one wire is drawn on one page
```

- **A wire exists without a drawing.** An end whose terminal or node isn't drawn on any page
  is undrawn. The wire appears in the wire list, netlist and checks ("not drawn yet"), and
  schematic generation draws it.
- **A drawn wire is an ordinary wire** with graphics on a page. Drawing a wire by hand
  creates the wire and its graphics in one command, so schematic-first editing behaves
  exactly as now.
- **Terminals are entities.** A device's terminals come from its product's terminal pattern,
  from the symbols it is drawn with, or both, and are checked against each other. A terminal
  strip's terminal is one terminal per side, bridged internally. A PLC channel is a
  terminal of the card, with its address as a typed property.
- **Designations are properties.** Renumbering a terminal changes one field. No wire is
  rewritten, because wires point at the `TerminalId`. Designations stay unique per device:
  a checked invariant.
- **Nodes are explicit logical connectivity:**
  - a junction, where three or more wires meet with no physical terminal;
  - a named potential (`L+`, `M`, `L1`), drawn by potential points on any number of pages;
  - an interruption pair, continuing a potential on another page.
  Their drawings are representations only, and the netlist resolves through nodes.
- **On disk,** connectivity leaves the page files: `terminals.json`, `nodes.json` and
  `wires.json` at project level. Page files keep functions (with their point-to-terminal
  map), node drawings and wire graphics. Format 1 projects are read and converted
  (section 9).

### 4.6 Persistence (format 2)

```text
my-machine/
  project.json        format 2, name, pages, settings, designation convention
  structure.json      sections and locations                                    new
  requirements.json   requirements                                              new
  devices.json        devices (+ section, location, requirement, safety marking)
  terminals.json      terminals: id, device, designation, side, kind            new
  nodes.json          junctions, named potentials, interruptions                new
  wires.json          wires with terminal or node ends                          moved out of pages
  cables.json, symbols/   as today (+ source and revision of copies)
  products.json       the project's product copies (today's parts.json, read as such)
  templates/<id>.json copies of templates used                                  new
  instances.json      template and module instances, parameters, generation state, deletion overrides   new
  numbering.json      numbering policy, reserved and retired identifiers        new
  changes/<id>.json   engineering change and approval records (append-only)     new
  modifications.json  modification packages and their states (section 12.7)     new
  baselines/<id>.json baseline manifests: name, kind, state, format, file hashes   new
  objects/<sha256>    immutable content: baseline files, library revisions, assets, issued exports   new
  pages/<id>.json     functions, node drawings, wire graphics, boxes, layout items, CAD geometry
  underlays/<id>.json as today
```

- The same transactional store, lock and fingerprint apply to every file, including baseline
  creation (D26). A baseline's objects are staged, then its manifest is committed through the
  journal, so a crash leaves either no baseline or a complete one.
- **Concurrency** (D15): one writer per folder, through the lock. Readers never block and
  never see a half-written save (`load_checked` reloads if a save ran meanwhile). A writer
  whose project changed on disk since it loaded is refused, as today.
- Everything stays pretty-printed and sorted, so it diffs in git. Files appear only when
  used, so a schematic-first project looks almost as it does today.

## 5. Libraries

All four kinds are folders saved by the same store, with immutable revisions in `objects/`.
The project keeps copies of what it uses (section 4.2).

| Library | Holds | Status |
| --- | --- | --- |
| **Components** | Products, electrical and mechanical, by canonical id: manufacturer, order number, class, technical data, data sheet; then terminal pattern, symbol mapping, 2D geometry, accessories | Core in draft #32 (import from CSV/Excel). Then BMEcat with ETIM, ETIM xChange, ECLASS Advanced and AASX |
| **Symbols** | IEC symbols, black boxes, CAD blocks | Built-in today; library folders with the symbol editor (section 8) |
| **Circuit templates** | Reusable circuits: devices with roles, terminals, nodes, internal wires, ports, drawing | New |
| **Functional modules** | Machine units: sections, requirements, chosen templates, parameters | New, after templates are proven |

### 5.1 Components (integrating the agreed component library)

What #32 built stays the base: a shared library, technical data, imports that never wipe
data, and take/refresh into projects. Reconciled with the approved decisions:

1. **Canonical product ids and manufacturers** (D7). #32 keys products by (manufacturer,
   number) in lowercase. That changes to a UUID per product and a manufacturer entity with
   aliases. Imports match an incoming row to an existing product by manufacturer (through
   its aliases) and order number, or by a recorded alias of the number. A match keeps the id;
   no match creates one. Two makers' identical numbers are two products. A renumbered
   product keeps its id, with the old number recorded as an alias.
2. **Revision and source,** with immutable contents (section 4.4).
3. **Mechanical components:** enclosures, mounting plates, DIN rails, ducts, cable glands,
   brackets, end stops, labels, fixings. They are products with a kind and dimensions, used in
   the BOM and by the layouts (rails and ducts already carry a part number), with no electrical
   terminals.
4. **Terminal pattern:** a product's physical terminals and what each is (coil A1/A2, NO
   13-14, main 1-2...), from which a device's terminals are created (section 4.5).
5. **Symbol mapping:** which symbols represent the product (a contactor is a coil, three main
   contacts and its auxiliaries), with a default per class.
6. **2D geometry:** footprint, front view, cut-out and drilling outlines as CAD geometry
   (section 8). It is imported from manufacturer DXF or SVG, or drawn in the native editor.
7. **Typed technical data with units,** so selection can compare a requirement's 0.55 kW with
   a product's data. Values that can't be parsed stay text and are flagged.

**Formats, by track:**
- CSV and Excel (in #32);
- BMEcat with ETIM features;
- **ETIM xChange** (ETIM's JSON format);
- ECLASS Advanced (licence checked first) and AASX;
- DXF, DWG and SVG for geometry.

**Import security.** A manufacturer file is untrusted input:
- reads are bounded in file size, unpacked size, rows and columns, as #32 already does;
- XML is parsed with external entities and DTD fetching disabled;
- archive entries are checked against path traversal and zip bombs;
- nothing is downloaded, and no macro or script runs;
- complex binary parsers run in the isolated worker process used for DWG;
- every imported item passes the model's validation, and its provenance records the file
  and its hash.

**Portable library packages.** A package is one file (a zip): a manifest with library id,
content list and SHA-256 hashes, plus the items and their objects, for a whole library or a
selection. Importing one is a library import like any other: validated, hash-checked,
creating revisions, never overwriting. Signing packages is left for later. A project can be
exported the same way, with the library items it uses.

### 5.2 Circuit templates

A template is data, not code (D8):

- **Devices:** a local name (`motor`, `contactor_fwd`), role, tag prefix, required product
  class, terminals, and the symbols each is drawn with.
- **Nodes and internal wires** between template terminals and nodes.
- **Ports:** named connections to the outside (`L1 L2 L3 PE`, `24V 0V`, `DI`, `DO`) bound
  when instantiated.
- **Drawing:** the template's symbols, node drawings and wire routes, drawn once by a person
  in the native editor, relative to an anchor.
- **Typed parameters** (number with unit, choice, text, boolean) and **options** that
  include or leave out blocks (`reversing`, `thermal_contact`). Parameters substitute into
  texts and requirement values. There are no loops and no scripting language.

Instantiating a template creates devices, terminals, nodes and wires in the model (no
drawing). It is recorded as a **template instance** with its parameters and port bindings.
Drawing them is generation's job (section 6).

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

Generation is **incremental by default** (section 12.3). It draws only what is missing or
explicitly asked for, and puts new content on new pages inserted after the section's pages.
It never moves or renumbers what is already drawn. Regenerating existing content is a
separate operation with its own scope and preview.

### 6.2 Determinism

Generation is a pure function `(model, generation settings) -> commands`:

- no randomness, no clock, no hash-map order;
- ids from UUID v5 (section 4.1);
- all ordering by stable keys.

Generating twice gives byte-identical project files, and that is an acceptance test. It
lives in `ecad-model` (it needs symbol geometry, not CADCraft), so the editor, CLI and MCP
run the same code.

### 6.3 Non-destructive regeneration

Each generated item (function, node drawing, wire graphics, generated page) carries:

```text
origin:    { generator or template instance, local key }
generated: { placement, content }   // the full values the generator last produced, not only hashes
```

The last-generated values are stored, not just their hashes (D10), so regeneration can
compare and merge per field.

On regeneration each item is compared three ways: what was generated last time (base), what
is there now (ours) and what would be generated now (theirs). Placement (position, rotation,
mirror, bend) and content (symbol, terminals shown, texts) are handled separately.

| Ours versus base | Theirs | Result |
| --- | --- | --- |
| Unchanged | Produced | Replaced by the new output |
| Placement changed (moved, rotated) | Produced | Placement kept (*pinned*); content updated |
| Content changed (a designation, a text) | Produced differently | Ours kept; listed as a conflict with the generator's proposal |
| Unchanged | No longer produced | Removed |
| Changed | No longer produced | Kept, marked orphaned, listed |
| **Deleted by a person** | Produced | **Not recreated:** the deletion is a persistent override (a tombstone in `instances.json`), listed; a person can lift it |
| — | New item | Added |

- **Dependency-aware conflicts.** Items are compared in dependency order: a function before
  the wire graphics that end on it. A conflict on one item marks what depends on it.
  - If a person keeps their own version of a function, its wires' graphics are updated
    against that version, not against the generator's.
  - Kept or orphaned items whose terminals or nodes no longer exist are reported, not
    silently dropped.
- **Scope.** This table applies when a person regenerates existing content: an explicit
  operation with a scope (a page, a section, a template instance). Ordinary generation only
  adds what is missing (section 12.3).
- **One step.** Regeneration is one command batch, so one undo step. It shows a preview (a
  change set, section 11.3) before it applies.
- **Detach** turns a generated item into a manual one, so the generator never touches it
  again.
- **Manual items** (no origin) are never touched. A hand-drawn schematic is all manual, so
  generation adds only what is missing.

## 7. Workspaces and shared data

| Workspace | Shows and edits |
| --- | --- |
| Machine structure | Sections and locations; device assignment |
| Requirements | Requirements per section; selection status |
| Devices and products | Devices, terminals, products, library search and selection |
| Schematic | Pages (today's editor) |
| Cabinet layout | Plates and doors (today's layout pages, extended) |
| Machine layout | Field locations on a machine drawing; cable lengths |
| Documentation | Generated pages, exports, validation |
| Symbol and component geometry editors | Symbols, blocks, footprints, front views, cut-outs (section 8) |
| Revisions | Baselines, modification packages and their states, change reports (section 12) |
| Assistant (optional, later) | Conversation, and the change sets it proposes, for review and acceptance |

- One `Session` per open project holds the project, history and selection. Every workspace
  is a `Frame` over it and edits only through commands.
- Selection is shared: picking a device in the structure tree shows it in the schematic and
  the layout.
- Undo is one history across workspaces.
- CLI and MCP reach the same operations through one registry (section 11.2). Each milestone
  adds its operations there, and with them its CLI commands and MCP tools.
- Libraries are opened alongside the project. Edits to a library are commands on the
  library, saved by its own store. The project only takes copies.

## 8. Shared native 2D CAD

Approved scope (D11), shared by every graphical workspace:

| Capability | Used by |
| --- | --- |
| Drawing primitives: line, polyline, arc, circle, rectangle, text, hatch, image | Annotations on pages, symbols, product geometry, layouts |
| Dimensions (linear, aligned, radius, angle) | Cabinet and door layouts, cut-outs, machine layout |
| Layers (visibility, colour, linetype, print) | Every drawing |
| Snapping (grid, end, mid, centre, intersection, perpendicular, connection points) and alignment (align, distribute, offset) | Every editor |
| Reusable graphical blocks with attributes | Title blocks, logos, door devices, repeated details |
| Electrical symbol editing: geometry, connection points, text fields, role | Symbol library |
| Component geometry creation and editing: footprint, front view, cut-out, drilling | Component library |
| Cabinet mounting plates and doors, cut-outs | Cabinet layout |
| Machine layouts and cable routes | Machine layout |
| Import: DXF, DWG (as today, in the worker), SVG. Export: PDF, DXF, DWG (as today), SVG | All |

How it is built, from what exists:

- **The model owns the geometry.** CAD geometry is model data (`ecad-model`), edited through
  commands with undo, like everything else. Today's `Graphic` (line, dashed, circle, text) in
  symbols is the start of this type; it gains the primitives, layers and blocks above.
  Geometry stays model data because CADCraft's document model treats the drawing as the source
  of truth, which this design rejects (ARCHITECTURE.md, *Editing*).
- **The editor owns the tools.** Snapping, alignment, the drawing tools and grips are
  `ecad-editor` behaviour, testable without a UI. Today's select, move, rotate and grid snap
  are the start.
- **CADCraft stays where it fits:**
  - rendering, PDF, DXF and DWG output, and DXF/DWG reading, all in `ecad-view`;
  - its geometry algorithms where they help (intersections and offsets for snapping, hatch
    filling), behind `ecad-view`'s API or a thin geometry adapter (D30).
  - The rule "only `ecad-view` depends on CADCraft" stays unless D30 decides otherwise.
- **SVG:** export of any page or drawing; import into CAD geometry (paths become polylines
  and arcs, within the same bounds and limits as DXF import). The crate is chosen for licence
  and safety (D35).
- **Layout editing follows section 12.** New devices arrive in the "not placed yet" list, and
  are placed by a person or into free space on request. Existing positions and routes never
  move to make room; lack of space is a check, not a rearrangement.
- **A machine drawing** from mechanical CAD is an underlay on a machine-layout page; field
  devices and cable routes are drawn over it.
- **3D modelling is outside the initial scope.**

## 9. Migration

| Change | Migration | Breaking? |
| --- | --- | --- |
| Terminals as entities (`terminals.json`) | Format 1 reader creates one terminal per physical drawing point, with a UUID v5 from (function, point), so converting twice gives the same ids; designations move from drawings to terminals | File format yes, behaviour no |
| Nodes (`nodes.json`) | Junction, potential and interruption devices become nodes; their tags become node names; their drawings become node drawings (D33) | File format yes; scripts that treat `L+` as a device find it as a node by name |
| Wires (`wires.json`) | Each end's function and point become its `TerminalId` or `NodeId`; bends become wire graphics in the page file | File format yes, behaviour no |
| `Reassign` | Becomes "show another device's terminals"; wires move to that device's terminals and it is reported as rewiring (section 14, item 2) | Behaviour yes, reported |
| Designations unique per device | Checked on load; a format 1 project that breaks it loads, and a check reports it with a fix command | No |
| Products by canonical id | `parts.json` read as products with new ids; `PartRef` by id, with the number kept beside it for reading | File format yes |
| Sections and locations | New, empty. Optional command proposes them from tag prefixes and location-box labels | No |
| Location boxes | Gain an optional location reference; the label stays for boxes that aren't locations (connector housings, `EXT`) | No |
| Device assignments, safety marking | New optional fields | No |
| PLC channel data | `properties` keys become typed terminal properties, read from the old keys | File format yes |
| Symbol `Graphic` | Extended to the CAD primitives of section 8; old symbols read unchanged | No |
| Stable hash | New, for revisions and generation; the folder fingerprint stays as it is | No |
| Page order | Sparse keys; format 1's dense order is converted on read | File format yes |
| Numbering | New `numbering.json`; default **sequential** (D22): after deleting `-K2`, the next device is `-K4`, not `-K2` | **Behaviour yes,** on purpose |
| Per-potential wire numbers on joining | No longer renumbers existing wires; reports `wire-numbers-differ` instead | **Behaviour yes,** on purpose (section 12.2) |
| MCP `--write` | Removed; MCP becomes propose-and-preview, acceptance in the app or an interactive terminal (D32) | **Behaviour yes,** on purpose |
| Verification | Existing data is unverified (D17); only the configured kinds must be verified before release (section 14, item 5) | No |
| Baselines, change records, modifications, `objects/` | New; a project starts with none | No |

- Format 2 is written only after a format 1 project is saved. The first save tells the user
  the project will no longer open in older versions.
- The samples and the reference project are converted, and their golden drawings must stay
  identical: the same drawing from the new model.

## 10. Risks and dependencies

| Risk | Mitigation |
| --- | --- |
| The connectivity change touches routing, naming, checks, reports, view, editor and MCP | First terminals and nodes derived in memory with no format change (M7). Then the one storage switch (M10) behind the existing tests and golden files |
| Terminal identity: drawings, contact numbering and reassignment all touch terminals | Persistent `TerminalId` (D2) means a renumber writes one field; deterministic ids on conversion; property tests over random renames and reassignments |
| Native CAD is a large new scope | Its own track (C0–C3), model-owned geometry, editor-owned tools; CADCraft for rendering, IO and algorithms; no 3D |
| AI approval can't be enforced against an agent with the user's own permissions | Full enforcement for actors Electric Studio runs; CLI and file edits recorded as such; documented need for OS-level separation (D31) |
| Manufacturer files are hostile input | Bounded reads, no external entities, archive checks, worker isolation for complex parsers, validation, provenance with file hash |
| Verification noise makes releases impractical | Verification required only for configured kinds (default: safety-related, suggested, imported) |
| Two truths (requirement versus selection, template versus edited drawing) | Never synchronised silently: validation and regeneration report differences |
| Generation scope creep (auto-placement, auto-routing) | Template-based placement only; no auto-router in the roadmap |
| Template format too weak or too clever | Options and substitution only; the conveyor must be built with it; extend on evidence |
| Technical data as text can't be compared | Numeric parsing with units in M9; selection warns, never blocks, when data is missing |
| UI workload: seven workspaces in egui | Each milestone ships model, CLI and MCP first, with a minimal panel; UI polish is its own track |
| CADCraft 0.x changes | Pinned commit and golden files (as today) |
| PR #32 was built before this proposal | Reconcile in M9: keep its core, add revision/source, review its merge rule (section 4.4) |
| Performance on large projects | Derived data cached per Session; measured on the reference and conveyor projects |
| Functional safety is out of scope | Safety circuits are documented, not validated; stated in the docs and the UI |
| An assistant edits around the rules | No AI-only path; every operation goes through `apply` and the checks; proposals with new errors can't be applied; safety-related changes need a person |
| AI-suggested data passes as verified | Provenance on facts; the `unverified-data` check; release export refuses unverified safety-related data |
| Confidential data leaves the machine | Local provider by default; external only by project setting; excluded fields; audit log |
| An operation changes more than it should | Declared scope plus the stability test on every operation; the conveyor retrofit in CI |
| Baselines grow the project folder | Content-addressed and shared between baselines; underlays by hash; a baseline only adds changed files |
| Sparse page order and page-name insertion confuse old tools | Format 2 only; the format 1 reader converts dense order |
| Retirement makes numbering feel stuck | Policy per kind; explicit reuse (approved after release); explicit renumbering always available with a preview |
| `objects/` grows git repositories | Git LFS or `objects/` outside git (D38); baselines don't need git |
| AI infrastructure grows ahead of need | Only the registry, change sets and provenance are built before inference. Each also serves people, CLI or generation |

Dependencies: M7, then M10 (the one format break), then S0, AI0 and C0; M8 and M9 after
AI0; S1 after S0 and AI0; M11 after M8 and M9; M12 after M9 and M10; M13 after M12, S0 and
S1; C2 after C1 and M9; M14 after M13, C0 and S1; M15 after M8, M9, S0, C1 and C2; M16 after
M13. Details are in [ROADMAP.md](ROADMAP.md).

## 11. AI readiness

An optional AI engineering assistant is a foundational requirement, but **inference is not
built yet**. What is built now are the prerequisites that the model and command interfaces
need anyway. They also serve people, the CLI and generation, so none of it is AI-only
infrastructure.

### 11.1 Principles

1. **The core never depends on AI.** No AI crate is a dependency of `ecad-model`,
   `ecad-rules`, `ecad-reports`, `ecad-project`, `ecad-view` or `ecad-editor`. Every
   engineering function, including generation, works with AI absent, disabled or offline.
2. **An assistant is a client like any other.** It uses the same queries and operations as
   the graphical workspaces, the CLI and MCP. There is no AI-only path into the model.
3. **Validation can't be bypassed.**
   - Model invariants are enforced by `Project::apply` for every actor, as today.
   - Checks run on every proposal before it can be applied.
   - An assistant's proposal that adds check errors can't be applied as it is.
4. **AI proposes, people decide** (D18). An assistant reads and prepares change sets.
   - **Each change set needs a person's explicit acceptance.** There is no session-wide AI
     write permission, and no other interface (MCP, CLI, scripts) that applies an assistant's
     change without that acceptance.
   - **Trusted application code assigns actor identities and permissions,** never the
     client: the MCP server labels everything it receives as the connected assistant's, and
     the in-app assistant runs as an assistant actor.
   - Acceptance happens in a trusted interface: the app's review panel, or an interactive
     confirmation in a terminal that shows the change set's hash. It is never an argument an
     assistant can pass. Section 14 states the limit of this.
5. **No AI approval of safety-related design** (D19). No operation lets an assistant:
   - verify, approve or release anything safety-related;
   - remove or weaken a safety classification.
   Changes that touch safety-related items, directly or indirectly (section 11.5), always
   need a person's separate confirmation.
6. **Offline and confidential by default.** The default provider is local. Nothing leaves
   the machine unless the project allows an external provider, and what was sent is logged.

### 11.2 Typed engineering interface

One catalogue of **queries** (read) and **operations** (change), each with a name, typed
arguments and result, a JSON schema, and an effect class (read, change, safety-related
change). It grows from what exists:

- **Operations lower to `Command`s.** `Command` stays the low-level, invertible unit. An
  operation is a named, validated engineering step built from commands, like today's
  `script_place` or `import_parts`. Examples: "assign a device to a section", "select a
  product for a requirement", "instantiate a template", "regenerate a page".
- **One registry.** The editor's UI events, the CLI and MCP call the same operation
  functions. MCP's tool list and argument schemas are generated from the registry, not
  written by hand. Each milestone adds its operations to the registry and nowhere else.
- **Queries are bounded and paged**: overview, find by tag, section or class, netlist of a
  device, check results, BOM rows... An assistant gets what it asks for, never the whole
  project, which matters for context size and confidentiality.
- **Ids in, ids out.** Results carry stable ids (section 4.1) alongside tags, so a proposal
  built from a query still points at the same things after a rename.

### 11.3 Change sets: propose, preview, apply, undo

A **change set** is a proposed modification that isn't applied yet:

```text
ChangeSet {
  id, actor, title, rationale,          // why, in the proposer's words
  base: Fingerprint,                    // the project it was made against
  operations: [Operation],              // or the commands they lower to
  sources: [Source],                    // datasheet pages, library items, requirements used
}
```

- **Preview.** Apply it to a copy of the project and report:
  - entities added, changed and removed, by tag and id;
  - check results before and after: new errors and warnings, ones that went away;
  - pages whose drawing changes, with a rendering;
  - safety-related items touched.
- **Accept and apply.** One `Batch`, so one undo step, labelled with the change set's title and
  actor.
  - It is refused if the base fingerprint no longer matches; it is then previewed again
    against the current project.
  - An assistant's change set is applied only after a person accepts that change set
    (principle 4).
  - Applying writes an engineering change record (section 11.4).
- **Reject** or **edit** before applying. A person can drop single operations.
- **Not only for AI.** The same mechanism is the dry run of a library update (M9), the
  preview of regeneration (M13), an import's `--dry-run`, and every global operation of
  section 12. Its report is the change-impact analysis of section 12.5, built once for all of
  them.
- **Scope.** An assistant's change set is scoped like any operation: it can't contain a global
  operation (renumbering, reordering, regeneration of existing content, layout optimisation)
  unless a person authorises that operation separately.

### 11.4 Actors, provenance, verification and change records

Provenance and verification are tracked separately (D17).

- **Actors.** Every history entry and every applied change set records who made it. The
  application assigns the actor; a client never names itself.
  - a person (the signed-in user of the app or terminal);
  - a generator (template instance, regeneration);
  - an import (file, format and hash);
  - an assistant (provider, model, session).
- **Provenance: where a value came from.** It is kept for values that come from anywhere but
  a person's own entry, and for values a person entered where the source matters:
  requirement parameters, sections and locations, product selections, technical data (per
  line), descriptions and translations.

  ```text
  Provenance   { by: Actor, kind: Entered | Imported | Suggested | Derived, sources, at }
  Verification { by: person, on: date, note }        // separate, optional
  ```

- **Verification: whether a person has checked it.**
  - **Manual entry doesn't imply verification.** A value typed by hand has provenance
    `Entered` and is unverified until verified, like an imported or suggested one.
  - Derived values (computed by the model) carry `Derived` and the inputs they came from; they
    aren't verified on their own, their inputs are.
  - Verifying is an explicit operation only a person can carry out, singly or for a reviewed
    set.
  - Which kinds of data need verification before a release is a project setting. The default
    is safety-related items and everything suggested or imported.
- **Engineering change records** are persisted in `changes/<id>.json`, append-only:
  - the change set or operation group, its actor and time;
  - its impact summary (section 12.5);
  - acceptances, verifications and approvals, with who and when;
  - the modification it belongs to (section 12.7).
  They are kept with the project and included in baselines. Undo of an applied change writes a
  record of the undo; records aren't deleted. The full undo history stays in memory only.
- **Checks.**
  - `unverified-data` (warning) lists values that need verification and don't have it.
  - In a release export (`ecad export --release`), unverified safety-related data is an
    error.
- **Generated is not suggested.** Deterministic generation output (section 6) has an
  *origin*. It is derived from its inputs and regenerated, so its trustworthiness is its
  inputs' verification.

### 11.5 Safety-related items

An item is safety-related when it is in a section marked safety-related (`=SAF` in the
conveyor), when its product's class is a safety component (safety relay, emergency stop,
safety sensor), or when a person marks it.

- **Indirect impact** (D19). The preview and the change report also list items a change
  reaches through dependencies:
  - devices sharing a potential or a node with a safety-related terminal;
  - contacts in a chain that ends at a safety-related coil or input;
  - the supply and protection feeding a safety-related device;
  - PLC channels and cables that carry its signals;
  - products and templates it was built from.
  A change that touches only those is flagged as safety-relevant too.
- Change sets with a direct or indirect safety impact need a separate confirmation by a
  person.
- An assistant can't verify, approve or release safety-related items, and can't remove or
  weaken a safety classification. A person can, explicitly, with a change record.
- Electric Studio documents safety circuits; it doesn't validate them (ISO 13849 and IEC
  62061 evaluation is out of scope). The UI and reports say so.

### 11.6 Providers (later)

- **An adapter crate** (`ecad-assist`, not built yet) holds a small provider interface:
  - send a conversation with the tool catalogue;
  - receive text and tool calls.
- **Providers.** Ollama on localhost comes first; external providers are added later behind
  the same interface. The assistant's tools are the registry's queries and the change-set
  operations: query, propose, preview, never apply directly.
- **Data policy, per project, in `project.json`.**
  - Assistant on or off (default off); provider local only (default) or external allowed.
  - Fields never sent: customer, plant and confidential notes are excluded by default.
  - Content an assistant reads that comes from licensed sources (ECLASS data) follows the
    licence.
- **External providers** (D20): disabled by default, enabled per project by a person, with
  confidentiality enforced in the adapter:
  - excluded fields are never placed in a request;
  - queries limit what can be read;
  - a request is refused, not trimmed, when it would need excluded content.
- **Audit log:** a controlled local log of what was sent to an external provider and what came
  back, outside the project folder, with configurable retention.
- **Conversations** (D21) are kept outside the project folder by default, with configurable
  retention, and are exported into the project only on purpose.
- **Offline.** With no provider, everything else works; with a local provider, nothing needs
  a network.

### 11.7 Future assistant capabilities

They use the layers above and need nothing new in the core:

| Capability | What the assistant does | Deterministic part that stays in the core | Needs |
| --- | --- | --- | --- |
| Machine architecture | Proposes sections, locations and requirements from a description | Structure validation, naming | M8, M11 |
| Component selection | Ranks and explains library parts for a requirement | Filtering by class and data constraints; the selection check | M9, M11 |
| Datasheet extraction | Reads a PDF data sheet and proposes technical data, terminal patterns and sizes, citing the page | Data validation (units, names, limits) | M9; PDF text extraction |
| Template selection | Proposes templates and options for requirements | Instantiation, port binding, connectivity checks | M12 |
| Documentation | Drafts descriptions, notes and second-language texts | Generated pages, layout, BOM | M14 |
| Review | Explains check results and suggests fixes as change sets | The checks themselves | Any |

All of them produce change sets with suggested provenance. None writes to the model
directly.

## 12. Incremental engineering and as-built preservation

**Foundational invariant: an operation changes the objects it names, and data derived from
them, and nothing else.** Adding or modifying equipment in an existing project must not change
unrelated engineering objects or their drawings. This holds for the whole life of a machine
(initial engineering, release for manufacturing, commissioning, as-built, maintenance,
extensions and retrofits) and **for every project, released or not.** It binds every
workspace, generator, library update, import and AI operation.

### 12.1 What is preserved

Unless a person runs an explicit global operation, these never change as a side effect:

- device identities and reference designations;
- wire and cable numbers;
- terminal designations;
- PLC addresses and network assignments;
- page identities and page names (numbers);
- symbol positions and orientations;
- cabinet and machine layout positions;
- wire and cable routes (bends, duct paths);
- selected component revisions (pinned project copies, section 4.4);
- manual modifications, including those to generated content (section 6.3).

**Derived data does change, as it must:** cross-references, contact mirrors, potentials, the
BOM, reports and generated documentation pages. It is regenerated from the model and never
stored as truth.

How it is enforced:

- **Operations declare their scope:** the entities they may change.
- **A stability test** applies an operation and compares the project before and after by id.
  Anything changed outside the scope and outside derived data fails. It runs in the
  conveyor acceptance test and on every new operation.
- **Global operations are separate and explicit.** Each is its own operation in the registry
  (section 11.2), never part of another, always previewed (section 12.5), and authorised by a
  person:
  - renumbering (wires, devices, terminals, PLC channels, pages);
  - reordering pages;
  - regenerating existing content;
  - optimising a layout;
  - updating library revisions;
  - relabelling through structure, location or convention changes (section 3).

### 12.2 Numbering

- **New objects get an available identifier without touching existing ones. Nothing is
  renumbered automatically, ever** (D22).
- **A numbering policy per kind**: device tags per prefix, wire numbers (per wire or per
  potential), cable numbers, terminal numbers per strip, page names.
  - *Strategy:* **sequential by default**, next after the highest ever allocated, including
    retired values, for equipment and document identifiers. Other policies are configurable
    per kind (lowest free, ranges per section).
  - *Format:* today's tag and wire formats, with `{page}` and so on.
  - *Range:* where numbering starts.
- **Bounded physical resources** need resource-aware allocation: PLC channels on a card,
  terminals on a strip of fixed size, cores in a cable, ports on a switch.
  - Allocation takes the first free slot that isn't reserved or retired, in the resource's own
    order.
  - When none is left it reports exhaustion as a check in the preview. It never moves an
    existing assignment and never adds a card on its own.
- **Reserved identifiers and resources:** single values or ranges kept free (spare terminals,
  PLC channels for a planned extension, `-K10…-K19` for a future section). Allocation skips
  them, and using one is an explicit choice recorded in the change record.
- **Retired identifiers** (D23): when an object is removed, its identifier is recorded as
  retired, with the change record and the last baseline it was in.
  - After a released baseline, a retired identifier is **never reused automatically.**
  - Explicit reuse needs a person's approval and appears in the change report.
  - Before any release, sequential allocation doesn't reuse retired identifiers either;
    explicit reuse needs no separate approval then.
- **Per-potential wire numbers:** joining two numbered potentials no longer renumbers either.
  The wire takes the number of the side it starts from, and the existing
  `wire-numbers-differ` check reports the mismatch. Making them agree is the explicit
  renumbering operation.
- **Pages** (D25):
  - Inserting a page never renames others. The new page is named by the configurable suffix
    policy: after `3` it becomes `3a` (or `3.1`), and after `3a` it becomes `3b`.
  - The page order is a sparse key, so an insertion writes only the new page. Today's
    `ReorderPages`, which writes every page, becomes the explicit reorder operation.
- **Stored in `numbering.json`:** the policies, reservations and the retired list.

### 12.3 Incremental generation

Generation (section 6) inserts and updates locally:

- **Only what is missing or explicitly affected:**
  - template instances and devices not drawn yet;
  - undrawn wires;
  - items the user names.
- **Unaffected pages are untouched:** byte-identical page files. Their derived cross-reference
  texts may change on screen and in exports, since they are computed, not stored.
- **New content goes on new pages,** inserted after the last page of its section, named by the
  page policy, never renumbering existing pages. Placing it into free space on an existing
  page is an option a person chooses for that run.
- **Existing circuit instances are preserved** when machine functions are added: a new
  template instance is new, and an existing one is regenerated only on request.
- **Connections to existing potentials** use the new page's own potential or interruption
  points, so no existing page gains symbols. Cross-references on existing pages are derived.
- **Problems are reported, not redesigned:**
  - no free terminal, PLC channel or address in range;
  - no room on a plate or rail;
  - a potential conflict;
  - an electrical inconsistency.
  Each is a check result in the preview. Nothing is rearranged to make it fit.

### 12.4 Baselines and revisions

A **baseline** is an immutable snapshot of the project's complete engineering state, made by a
person at a stage of the machine's life:

| Kind | When |
| --- | --- |
| Draft | Any time, for comparison |
| Released | Release for manufacturing |
| As-built | After commissioning, as the machine really is |
| Revision | Maintenance or retrofit releases after that |

- **Contents** (D27):
  - every engineering file at that moment: structure, requirements, devices, terminals, nodes,
    wires, pages, layouts, numbering, change records;
  - required assets: underlays, product geometry, the symbols and templates used;
  - settings;
  - the pinned library revisions with their content;
  - the project format version and the program version that made it.
  - **Excluded:** the baseline store itself (no recursion), temporary and staged files,
    caches, locks.
- **Issued documents.** When exact historical reproduction is needed, the exports issued with
  a baseline (PDF, DWG, DXF, the TIA tag table) are stored with it as objects. A later
  program version that draws differently can still hand out exactly what was issued.
- **Storage, inside the project folder** (D26):
  - `baselines/<id>.json` holds name, kind, index (matching the title block revision), date,
    by, note, format and program versions, and the SHA-256 of every file;
  - `objects/` holds each content once, by hash, shared between baselines and with library
    revisions, written once and never rewritten.
- **Creation is transactional:** objects are staged, and the manifest is committed through
  the store's journal. A crash leaves no baseline or a complete one.
- **Integrity is checked** on load and on demand: a file that doesn't match its hash marks the
  baseline damaged, and nothing is silently repaired. A damaged baseline can be re-verified
  against a backup copy of `objects/`.
- **Without git** or any other tool. Git still works on the folder as before, and baselines
  diff cleanly too.
- **Read-only use:** a baseline opens as a read-only project (the same loader, the format
  readers of its version) for viewing, re-exporting, handing out its issued documents, and
  comparing.
- **The title block's revision list** gains an optional link to its baseline.
- **Not PLM:** no approval workflows, branches, merges, check-in or check-out. One project line
  with snapshots and modification states along it (section 12.7).

### 12.5 Change impact analysis

One engine compares two states of a project by stable id: before and after an operation or
change set, or a baseline and now. It reports by engineering meaning, not by file:

| Category | Examples |
| --- | --- |
| Equipment | Devices added, removed, modified (tag, part, location, section, requirement) |
| Circuits and connections | Wires added or removed, ends moved, potentials merged or split |
| Terminal strips, cables, PLC | Terminals added or removed, wires moved between terminals, cable cores reassigned, PLC channels and addresses changed |
| Pages and layouts | Pages added; pages whose drawing changed; layout items added or moved |
| BOM | Quantities and parts added, removed, changed |
| Physical work | Relabel (an existing object's tag, wire number or terminal designation changed), rewire (an existing wire's ends changed), relocate (an existing device moved to another location or plate position), replace (part changed) |
| Compatibility | New check errors and warnings: requirement not met, no space, potential conflict, resource exhausted |
| Safety | Safety-related items reached directly or indirectly (section 11.5) |
| Issued documentation | Documents issued with a baseline whose content would now differ: pages, terminal diagrams, BOM, cable list, tag table (D28) |

- **Uses:** the preview of every change set (11.3) and every global operation, and
  `ecad change-report <project> --since <baseline>` as a document (CSV, then a generated
  page in M14).
- **Proof of the invariant:** unrelated canonical objects and stored drawings appear in no
  category. That is what the stability test (12.1) checks. Derived cross-references and
  documentation may change where the new content requires it, and the issued documents they
  affect are listed (D28).

### 12.6 Removal and replacement

Removal and replacement are different operations (D23) and are reported differently.

- **Removing equipment** removes its device, terminals, drawings, wires to it and layout item,
  in one operation and one undo step.
  - Nothing else is renumbered.
  - Its identifiers are retired, and the gaps it leaves stay.
  - Bounded resources it used (PLC channels, strip terminals) become free slots for
    resource-aware allocation, unless reserved.
  - The change report lists removal and rewiring.
- **Replacing equipment** keeps the device and its identity: same id, tag, terminals (matched by
  designation to the new product's terminal pattern), drawings, wires and layout position.
  - Its product changes.
  - The impact analysis reports what physically changes: terminals the new product doesn't
    have (with the wires on them), new terminals, a larger footprint than the space, a
    different rating against its requirement.
  - The change report lists *replace*, not *remove* and *add*.
- **Remove, then add a new device** is a removal followed by a new device with a new
  identifier. Doing that instead of a replacement is a choice the person makes, and the
  report shows it as two changes.

### 12.7 Engineering states: designed, released, as-built, proposed modification

Two different questions are kept apart (D29):

- *What is approved?* — an engineering state;
- *what is physically installed?* — the as-built state.

An approved modification does not mean the machine has been modified.

| State | What it is | Where it lives |
| --- | --- | --- |
| **Designed** | The working project, as being engineered | The project files |
| **Released** | A design issued for manufacturing or for a modification to be carried out | A Released or Revision baseline |
| **As-built** | What is physically installed, confirmed after commissioning or after a modification was carried out | An As-built baseline (the latest is the machine's current physical state) |
| **Proposed modification** | A change to an as-built machine that is being designed, approved or carried out | A modification package over the working project |

- **A modification package** is a record in `modifications.json`: id, title, reason, the
  as-built baseline it starts from, its change records, and its status. The status moves
  through:
  - *proposed*, then *approved* (by a person, recorded);
  - *released* (a Revision baseline is made);
  - *installed*: a person confirms the work was carried out on the machine, and a new
    As-built baseline is made.
- **No parallel models.** The working project is the designed state of the modification
  being worked on, and baselines hold the others.
  - What is installed but changed by the modification is found by comparing with the
    as-built baseline (section 12.5).
  - The change report of a package is that comparison, scoped to its change records.
- **Physical status per object follows from it.** An object absent from the latest as-built
  baseline is *not installed yet*. One changed since it is *installed, modification pending*.
  The UI and reports can mark both without storing anything new.
- **One open modification at a time** initially, as a single project line. Several
  concurrent modifications on one machine would need branches, which is out of scope (D36).

## 13. Performance requirements

Storage stays transactional JSON (D5). Rather than a catalogue size that triggers a database,
these are the targets. A benchmark in CI measures them on generated data:

- reports every PR, so trends show;
- fails at twice the target, so CI noise doesn't block.

The reference machine is a mid-range laptop (4 cores, 16 GB, SSD), recorded with each result.

| Operation | Data | Target |
| --- | --- | --- |
| Open a project | 2 000 devices, 20 000 wires, 300 pages | < 2 s |
| Save after a one-page change | same | < 1 s |
| Apply an interactive edit, with its checks | same | < 50 ms |
| Run all checks | same | < 1 s |
| Render a page | same | < 50 ms |
| Preview a change set of up to 200 operations | same | < 1 s |
| Incremental generation of one template instance | same | < 1 s |
| Make a baseline / compare a baseline with now | same | < 5 s / < 2 s |
| Open a component library | 200 000 products, 50 manufacturers | < 3 s |
| Search the library | same | < 100 ms (95th percentile) |
| Import a manufacturer list | 50 000 rows | < 30 s |

When a target fails on the benchmark and profiling shows that loading or searching JSON is
the cause, the first remedy is a **derived index**: in memory, or as a cache file that can be
rebuilt and is never the source of truth. A database (SQLite) is considered only if that isn't
enough, and is itself a decision.

## 14. Where the approved decisions meet the current code

The review asked for decisions that conflict with implementation evidence to be challenged.
None needs reversing. These need the stated adjustment, or a decision of their own:

1. **D18 versus MCP `--write` and the CLI.**
   - **What exists:**
     - `ecad-mcp --write` is a session-wide write permission for whatever client connects.
     - The `ecad` commands (`import-tia`, `import-dxf`, `library add`...) edit projects
       without confirmation, by design, for scripts and CI.
   - **What follows:**
     - `--write` must go. The MCP server becomes propose-and-preview only, with acceptance in
       the app or an interactive terminal confirmation. This breaks today's MCP workflow
       (docs/MCP.md) on purpose (D32).
     - CLI edits are recorded with the actor `command line`, and their data is unverified.
   - **The limit:**
     - An AI agent that operates the user's own account (a coding agent with shell access, a
       desktop-control tool) can run the CLI, edit the files, or answer a terminal prompt.
       No software check can prove a person is at the keyboard.
     - Enforcement is complete only for actors Electric Studio itself runs (the in-app
       assistant and the MCP server). For an agent with the user's own permissions it is
       recorded, not prevented.
     - Real separation needs the agent to run without write access to the project folder.
       The documentation should say so plainly (D31).
2. **D2 TerminalId.**
   - **What exists:** terminals are implicit symbol points.
     - Contact numbers from the part's contact blocks (G5b) and terminal renumbering
       (`SetDesignations`) write designations on drawings.
     - `Reassign` moves a drawing to another device with new designations, and its wires
       follow.
   - **What follows:**
     - designations move to terminals;
     - `Reassign` becomes "show the other device's terminals here", which moves the wires to
       that device's terminals and is reported as rewiring;
     - format 1 conversion gives each terminal a UUID v5 from its drawing and point, so
       converting the same project twice gives the same ids.
3. **D1 explicit nodes.**
   - **What exists:** junctions, potential points and interruption points are devices with
     tags (`J1`, `L+`, `I1`), stored in `devices.json` and found by tag in scripts and MCP.
   - **What follows:**
     - they become nodes, with the tag as the node's name;
     - lookups by name keep working;
     - their drawings become node drawings (D33).
4. **D7 canonical product ids versus #32 and the project catalogue.**
   - **What exists:**
     - #32 keys products by lowercased (manufacturer, number);
     - the project catalogue is keyed by order number (`Project.parts`), and devices refer to
       parts by number (`PartRef.number`).
   - **What follows:**
     - products get ids, and `PartRef` refers by id (files keep the number beside it, for
       reading);
     - a project can then hold the same number from two makers;
     - #32's matching becomes the import alias rule (section 5.1).
5. **D17 "manual entry doesn't imply verification".**
   - **What exists:** every value in every existing project was entered by hand and is
     unverified.
   - **What follows:** to keep releases usable, which data must be verified before release is
     a project setting. The default is safety-related, suggested and imported data, not all
     data. Existing projects start with that default and show what is unverified.
6. **D22 sequential allocation.** Today new tags, terminals and channels take the lowest free
   value. Switching the default changes what schematic-first users see after deleting
   something. That is intended, and noted in the migration.
7. **D11 native CAD.**
   - **What exists:** the editor has grid snapping, select, move, rotate, and rail and duct
     tools. There are no general primitives, dimensions, layers or blocks, and no symbol or
     geometry editor.
   - **The constraint:** CADCraft's editing engine can't be used, because it makes the
     drawing the source of truth.
   - **What follows:** this is a substantial new track (C0–C3 in the roadmap), not a detail
     of the layout milestone. Using CADCraft's geometry algorithms outside `ecad-view` is a
     decision (D30).
8. **D26 baselines inside the project, and git.** Underlays, product geometry and issued PDFs
   in `objects/` are binary and grow a git repository with every baseline. The recommendation
   is to keep `objects/` in git LFS or outside git, since baselines need no git (D38).
9. **D10 stored generated values.** Keeping the last generated values (not just hashes) roughly
   doubles the size of generated content in page files. That is accepted, because it is what
   makes field-level merging possible.
10. **D6 immutable revisions in libraries.** A library can't know which projects use which
    revision, so it can't safely drop old revisions for them. Because projects keep the
    revisions they use in their own `objects/`, a library may prune old revisions without
    harming any project.

## Decision record

Reviewed by the owner with an independent technical reviewer on 27 Nov 2026.

### Approved, with conditions

| # | Decision | Approved, with conditions | Where |
| --- | --- | --- | --- |
| D1 | Canonical connectivity | Independent of drawings, **including explicit logical junctions and virtual connectivity** (nodes) | 3, 4.5 |
| D2 | Terminal identity | **Persistent, immutable `TerminalId`**; designations are editable properties | 4.1, 4.5 |
| D3 | Full designations | Computed; **changes to structure or locations never silently relabel installed equipment** | 3 |
| D4 | Designation conventions | Configurable per project; the existing default kept | 3 |
| D5 | Storage | Transactional JSON initially; **measurable performance requirements** decide on an index or SQLite, not a size threshold | 13 |
| D6 | Library revisions | Content hashes; **immutable revision contents and dependent assets available** for real three-way comparison | 4.4 |
| D7 | Project copies | Self-contained, with source and revision; **stable canonical product ids** against order-number collisions | 4.1, 5.1 |
| D8 | Templates | Simple typed parameters, options and substitutions; no scripting language initially | 5.2 |
| D9 | Generation | Deterministic, template-based, incremental placement; no automatic global optimisation or rearrangement | 6, 12.3 |
| D10 | Regeneration | Per item, placement and content separately; **persistent deletion overrides, sufficient stored content, dependency-aware conflicts** | 6.3 |
| D11 | Shared CAD | **Native 2D CAD:** component geometry, symbol editing, plates and doors, cut-outs, machine layouts, cable routes, primitives, dimensions, layers, snapping, alignment, blocks; CADCraft where appropriate; no 3D initially | 8 |
| D12 | Requirement kinds | Motors, sensors, valves, actuators, operator devices, power supplies, PLCs, remote I/O, drives, protection devices, safety equipment, generic; implemented incrementally | ROADMAP M11 |
| D13 | PR #32 | On hold; its compatible implementation reused after approval and reconciliation | 5.1, 14 (item 4) |
| D14 | Requirement parameters | Typed, unit-aware, extensible properties, optional ETIM mappings | 3 |
| D15 | Concurrency | Single writer, with safe concurrent reading, locking and conflict detection | 4.6 |
| D16 | Interface | One authoritative typed registry of operations and queries for GUI, CLI, MCP, generators and assistants | 11.2 |
| D17 | Provenance | **Provenance and verification tracked separately; manual entry doesn't imply verification;** imported, suggested and derived data keep provenance; **change and approval records persisted** | 11.4 |
| D18 | AI apply policy | **Explicit human acceptance of each AI change set; trusted code assigns actors and permissions; no session-wide AI write permission or bypassing interface** | 11.1, 11.3, 14 (item 1) |
| D19 | Safety | Impact analysis **includes indirectly affected devices, connectivity and dependencies;** AI can't remove safety classifications or approve safety-related designs | 11.5 |
| D20 | External providers | Disabled by default; explicit project authorisation; confidentiality enforced; controlled audit logging | 11.6 |
| D21 | Conversations | Outside the project by default; configurable retention; explicit export | 11.6 |
| D22 | Allocation | **Sequential** for equipment and document identifiers; configurable policies; **resource-aware allocation for bounded resources;** never renumber automatically | 12.2 |
| D23 | Retired identifiers | No automatic reuse after release; explicit reuse approved and reported; **replacement distinct from removal plus a new device** | 12.2, 12.6 |
| D24 | New generated content | New pages by default; existing free space only by explicit choice; never rearrange a cabinet | 12.3 |
| D25 | Inserted pages | Configurable suffixes, no renumbering | 12.2 |
| D26 | Baseline storage | Self-contained, content-addressed in the project; **transactional creation, integrity checking, recovery** | 12.4 |
| D27 | Baseline content | Complete: assets, settings, pinned library revisions, format and version; **excludes** baseline storage, temporary files and caches; **issued exports preserved** where exact reproduction is needed | 12.4 |
| D28 | Derived updates | Derived cross-references and documents may update; unrelated canonical objects and stored drawings unchanged; **affected issued documentation listed** | 12.5 |
| D29 | Engineering states | **Designed, released, as-built and proposed modification kept distinct;** an approved modification isn't an installed one; built on baselines, no parallel models or PLM | 12.7 |

Also confirmed as foundational: **minimal-impact changes** across all workspaces, generators,
library updates and AI operations, for unreleased projects too (section 12).

### Open, raised by this revision

| # | Decision | Proposed |
| --- | --- | --- |
| D30 | CADCraft geometry algorithms outside `ecad-view` | Keep the rule; reach them through a thin adapter in `ecad-view` until a measured need says otherwise |
| D31 | The limit of AI approval enforcement (section 14, item 1) | Enforce fully for the in-app assistant and MCP; record CLI and file edits as such; document that an agent with the user's own permissions needs OS-level separation |
| D32 | MCP `--write` | Remove it; MCP is propose-and-preview only, with acceptance in the app or an interactive terminal |
| D33 | Junction, potential and interruption devices | Convert to nodes on reading format 1; tags become node names; lookups by name keep working |
| D34 | Performance targets and reference machine | As in section 13 |
| D35 | SVG library | Chosen at C0 for licence (MIT or Apache) and bounded parsing |
| D36 | Concurrent modification packages | One open at a time initially |
| D37 | The earlier review requirements (manufacturer import security, portable library packages, mechanical components, ETIM xChange, SVG support, native CAD editing) | Covered in sections 5.1 and 8 from their names only. **The original review document isn't in the repository or this session; please attach it** so its details can be checked against this text |
| D38 | `objects/` and git | Recommend git LFS or keeping `objects/` out of git |
