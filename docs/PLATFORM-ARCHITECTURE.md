# Machine-centric platform: architecture proposal

**Status: revised after the owner's reviews of D1–D29 and D30–D38 (27 Nov 2026), for the next
independent review.** Nothing here is implemented. No implementation work (including the component library
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
  operations as every workspace. Each of its change sets needs a person's approval in the app
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
- **Node names are scoped explicitly** (D33):
  - a named potential's name is unique in the project, among potentials;
  - an interruption's name is unique in the project, among interruptions, and all its
    drawings are one node;
  - a junction has no name, only its id.
  Lookups by name say which kind they look for.
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
  .ecad/state.json    hashes of the files as last saved, to detect outside changes (11.8)          new
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

### 4.7 Archives, restore and objects (D38)

**No Git, Git LFS or other version control is required** for anything: saving, revisions,
baselines, recovery or portability are all native. Companies that keep projects in Git may
use Git LFS for `objects/`. Excluding `objects/` from version control is safe only when a
separate, verified backup keeps them, and the documentation says so.

- **Project archive and restore:**
  - `ecad archive <project> <file>` writes one file (a zip) with every project file, every
    object reachable from it (baselines, issued documents, library revisions, assets), and
    a manifest of SHA-256 hashes;
  - `ecad restore <file> <dir>` unpacks it into a new folder and verifies completeness (every
    referenced object present) and integrity (every hash matches) before reporting success;
  - the app offers both;
  - an archive is also how a project travels with its libraries' revisions to another
    machine.
- **Missing objects.** An object referenced but not present is reported with what needs it
  (a baseline, an issued document, a library revision). The referencing baseline or document
  is marked incomplete, and nothing that depends on it is guessed. It can be restored from an
  archive or backup that has it; restore checks the hash.
- **Damaged objects.** An object whose content doesn't match its hash is moved to
  `objects/damaged/`, never overwritten or deleted, and reported like a missing one.
- **Unreferenced objects** (left by an interrupted import, or by a library revision a person
  deleted) are listed, not deleted.
  - Cleanup is an explicit operation with a preview. It marks everything reachable from the
    roots, then offers to delete only objects reached by none of them and older than a grace
    period.
  - The roots: the current project files, every baseline manifest, every change record, every
    issued document, every pinned library copy and every library revision the project
    references.
  - **Content referenced by project snapshots, library revisions, baselines or issued
    documentation is never deleted.**
- **Libraries** follow the same rules for their own `objects/`.

## 5. Libraries

All four kinds are folders saved by the same store, with immutable revisions in `objects/`.
The project keeps copies of what it uses (section 4.2).

| Library | Holds | Status |
| --- | --- | --- |
| **Components** | Products, electrical and mechanical, by canonical id: manufacturer, order number, class, technical data, data sheet; then terminal pattern, symbol mapping, 2D geometry, accessories | Core in draft #32 (import from CSV/Excel). Then BMEcat with ETIM, ETIM xChange, ECLASS Advanced and AASX |
| **Symbols** | IEC symbols, black boxes, CAD blocks | Built-in today; library folders with the symbol editor (section 8) |
| **Circuit templates** | Reusable circuits: devices with roles, terminals, nodes, internal wires, ports, drawing | New |
| **Functional modules** | Machine units: sections, requirements, chosen templates, parameters | New, after templates are proven |

### 5.1 Components (the agreed component library, D37)

What #32 built stays the base: a shared library, technical data, imports that never wipe
data, and take/refresh into projects. Reconciled with D7 and D37:

**Product model**

1. **One canonical product definition, independent of any manufacturer or source format.** Every
   product has the same schema, whoever made it and whichever file it came from. A source
   format maps into the schema and never shapes it.
   - **Generic products** (no manufacturer: "contactor 9 A, 24 V DC coil") are allowed as
     placeholders before a selection, and as user-created components (D39).
2. **Stable identities** (D7):
   - a UUID per product, given when it is first created;
   - a UUID per manufacturer, with name aliases (`ABB`, `ABB S.p.A.`);
   - the content hash of each revision (4.4);
   - installed devices keep their own ids (4.1).
   Order numbers, old numbers and source keys are aliases, matched on import, never the
   identity. Two makers' identical numbers are two products, and a renumbered product keeps
   its id.
3. **Electrical and purely mechanical components.** Mechanical items (enclosures, plates, DIN
   rails, ducts, glands, brackets, end stops, labels, fixings) are products with a kind,
   dimensions and no terminals. They appear in the BOM and the layouts.
4. **Typed technical specifications** with value, unit and provenance per line (11.4). A value
   that can't be parsed stays as the source wrote it and is flagged. Nothing is defaulted or
   invented.
5. **Electrical functions, terminals and symbol mappings.**
   - *Functions:* what each part of the product does (coil, NO or NC contact, main contact
     pole, protection function, PLC DI or DO channel, supply input). They map to the symbol
     roles the drawings use.
   - *Terminal pattern:* the physical terminals of each function (A1/A2, 13-14, 1-2...), from
     which a device's terminals are created (4.5).
   - *Symbol mapping:* which symbols draw which functions.
6. **Mechanical geometry, dimensions, mounting and clearances:**
   - footprint, front view, cut-out and drilling outlines as CAD geometry (section 8);
   - width, height and depth;
   - mounting (DIN rail 35 mm, screw, door cut-out, its orientation);
   - minimum clearances on each side (for heat, arcing, wiring).
   Layout checks use the clearances as well as the outline.
7. **Relations:**
   - accessories (an auxiliary block for a contactor, a marker for a terminal);
   - compatible products (fits with, replaces);
   - variants of one family (coil voltages, sizes), each variant its own product.
   Relations are links by product id. Replacement (12.6) offers compatible products first.
8. **User-created components and custom libraries.** A person creates and edits products in
   the app, with the same schema and the geometry editors (C2).
   - Several libraries can be open, each with its own id, in an order of preference: company
     library, then manufacturer libraries.
   - Edits are revisions like imports.
9. **Self-contained project snapshots** with source and revision (4.4), **immutable revision
   contents and dependent assets** (4.4), **controlled three-way updates** (4.4).
10. **Offline:** libraries are folders on disk or a share. Nothing in the library needs a
    network, and nothing is fetched during import.

**Importers**

| Priority | Formats |
| --- | --- |
| First | CSV and Excel with **reusable column mappings** (saved import profiles, per source); BMEcat with ETIM; **ETIM xChange** (JSON); DXF and SVG for product geometry |
| Later, on demonstrated need and licensing | ECLASS Advanced, AASX |
| Later, on feasibility | DWG product geometry (the DWG reader already exists for underlays); STEP (3D; would need a reader and a 2D projection, outside the initial scope) |
| Only after verifying usage and licensing rights | EDZ |

Every importer must provide:

- **Defensive parsing and bounded resources:**
  - limits on file size, unpacked size, rows, columns, nesting and entity count;
  - XML with external entities and DTD fetching disabled;
  - complex binary parsers in the isolated worker process.
- **Archive safety:** path traversal, absolute paths, links and zip bombs refused; nothing
  extracted outside a temporary folder.
- **Transactional application and rollback.** An import is a change set (11.3). It is
  previewed, applied all or nothing as one undo step, and recorded as a change record. A
  failure part way changes nothing.
- **Duplicate detection:**
  - within the file (the same product twice);
  - against the library (same manufacturer through its aliases and the same number or
    alias);
  - likely duplicates (the same number after normalising spaces, dashes and case) are listed
    for a person to decide, never merged silently.
- **Conflict resolution.** Each source's last imported revision is the base of a three-way
  comparison:
  - fields the new file changed are updated;
  - fields a person edited in the library since are kept;
  - fields both changed are conflicts, decided by a person.
  This replaces #32's simpler rule, under which a non-empty incoming value always replaces
  the library's (section 14, item 11).
- **Import reports:**
  - rows read, products added, updated, unchanged and rejected (with why);
  - duplicates, conflicts, unparsed values;
  - the file's name and hash.
  Each report is kept with the change record.
- **Unknown technical properties are kept** under their source names (as #32 does for unmapped
  columns), never dropped.
- **Missing manufacturer data is never fabricated.**
- **Automatic assignments are suggestions.** Symbols, functions and terminal mappings assigned
  by class defaults or by matching are **Suggested and unverified** unless the source file
  carries them as authoritative manufacturer data (ECLASS Advanced or EDZ connection points).
  A person verifies them (11.4).

**Testing.**
- Synthetic fixtures (in the repository) for deterministic CI, including hostile files.
- Representative real manufacturer files for interoperability acceptance, subject to their
  licences. Where a licence forbids redistribution, they are kept outside the public
  repository and run locally, as the SPAC and LibreDWG samples are today.

**Portable library packages** are Electric Studio's native exchange format.
- A package is one file (a zip): a manifest with library id, the content list and SHA-256
  hashes, plus the products and all their objects (geometry, symbols, data sheets kept).
- It holds a whole library or a selection.
- Importing one is a library import like any other: validated, hash-checked, creating
  revisions, never overwriting.
- Signing packages is left for later.
- A project can be exported the same way, with the library items it uses (section 4.7).

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

CAD is **a shared capability across Electric Studio, not a separate general-purpose CAD
application** (D37). Approved scope (D11, D37):

| Capability | Used by |
| --- | --- |
| Lines, polylines, arcs, circles, rectangles, text, dimensions (linear, aligned, radius, angle); hatches and images | Annotations on pages, symbols, product geometry, layouts |
| Layers (visibility, colour, linetype, print) | Every drawing |
| Snapping (grid, end, mid, centre, intersection, perpendicular, connection points), alignment and distribution, rotation, scaling | Every editor |
| Reusable graphical blocks with attributes | Title blocks, logos, door devices, repeated details |
| Electrical symbol and connection-point editing | Symbol library |
| Component geometry creation and modification; importing and adapting manufacturer geometry | Component library |
| Cabinet mounting plates, doors and cut-outs | Cabinet layout |
| Machine layout creation and editing; equipment positioning; cable routing | Machine layout |
| Import: DXF, DWG (as today, in the worker), SVG. Export: PDF, DXF, DWG (as today), SVG | All |

**Ownership (D30):**

- **The engineering model owns geometry and electrical semantics.** CAD geometry is model
  data in `ecad-model`, edited through commands with undo, like everything else.
  - Today's `Graphic` (line, dashed, circle, text) in symbols is the start of this type. It
    gains the primitives, layers and blocks above.
  - `ecad-model` doesn't depend on rendering or UI crates.
- **The editor owns the tools.** Drawing tools, grips, snapping, alignment, rotation and
  scaling are `ecad-editor` behaviour, testable without a UI. Today's select, move, rotate and
  grid snap are the start.
- **CADCraft is kept behind an adapter** (`ecad-view`), as today:
  - rendering, PDF, DXF and DWG output, and DXF/DWG reading;
  - CADCraft's editing engine is not used, because it treats the drawing as the source of
    truth.
- **Geometry algorithms.** If native CAD operations need algorithms independent of rendering
  (intersections and offsets for snapping, hatch filling, polygon clearance tests), the first
  choice is small functions in the model on its own geometry types. A **dedicated geometry
  adapter** (for example over CADCraft's geometry crate) is evaluated only when
  implementation shows those functions aren't enough. No abstraction is introduced ahead of
  that evidence.

**Electrical semantics stay independent of geometry** (D37):

- connectivity lives in terminals, nodes and wires (4.5), never in where lines touch;
- moving, rotating or scaling a symbol, or drawing a line over a connection point, never
  connects or disconnects anything;
- a wire is made only by the wire operation, and a terminal only by the device, product or
  symbol operations;
- editing a symbol's connection points, when the symbol is in use, is an operation of its
  own. Its preview lists every drawing and terminal mapping affected. It is never a side
  effect of editing the symbol's lines.
- deleting geometry never deletes a terminal, node or wire.

**SVG (D35).** The library is chosen during C0 against:

- maintenance, licence and the geometry it supports;
- numerical correctness, tested against reference files;
- resource limits.

On import:

- scripts, external resources (images, fonts, stylesheets by URL), network access and file
  access outside the imported file are all disabled;
- paths become polylines and arcs within the same bounds and limits as DXF import;
- unsupported entities (filters, masks, embedded fonts...) and lossy conversions (curves
  approximated, gradients flattened) are listed in the import report.

**Layout editing follows section 12.** New devices arrive in the "not placed yet" list, and are
placed by a person or into free space on request. Existing positions and routes never move to
make room; lack of space or clearance is a check, not a rearrangement.

A machine drawing from mechanical CAD is an underlay on a machine-layout page, with field
devices and cable routes drawn over it.

**Full 3D modelling is outside the initial scope.**

## 9. Migration

| Change | Migration | Breaking? |
| --- | --- | --- |
| Terminals as entities (`terminals.json`) | Format 1 reader creates one terminal per physical drawing point, with a UUID v5 from (function, point), so converting twice gives the same ids; designations move from drawings to terminals | File format yes, behaviour no |
| Nodes (`nodes.json`) | Junction, potential and interruption devices become nodes; their tags become node names in their kind's scope; their drawings become node drawings; electrical connectivity, names, drawings and cross-references are preserved and checked equal after conversion (D33). **Ambiguous mappings are detected, never merged or reconnected silently:** an interruption whose drawings join differently named potentials, a potential and an interruption sharing a name, a junction device drawn more than once. The conversion then keeps the nodes apart, reports each case with its pages, and nothing is saved until a person resolves them or saves knowingly | File format yes; scripts that treat `L+` as a device find it as a node by name |
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
| MCP `--write` | Removed; MCP clients query, prepare and preview; a person approves the exact transaction in the app (D32) | **Behaviour yes,** on purpose |
| CLI editing commands | Dry run unless `--apply` and the project allows automation; recorded as CLI automation (11.8) | **Behaviour yes,** on purpose |
| Library imports | Three-way against each source's last import instead of "a non-empty value replaces" (5.1) | Behaviour yes, for #32's draft only |
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
| AI approval can't be enforced against an agent with the user's own permissions | Enforcement inside the trust boundary (built-in assistant, MCP); outside changes detected, not claimed attributable; documented need for OS-level isolation (D31) |
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
| `objects/` grows Git repositories | Git never required; native archive and restore; Git LFS may be recommended; `objects/` excluded only with a verified backup (D38) |
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
   - **Approval happens only in a trusted, application-controlled interface: the app's
     review panel** (D32). A terminal prompt isn't one, because an agent may control the
     terminal. Approval is never an argument, a flag or a message an assistant can send.
   - **A person approves the exact transaction** they reviewed: the change set's content hash
     and the project state it was previewed against.
     - A substantial proposal (a whole new conveyor section) is approved as one transaction.
       Its operations are reviewed together, not command by command.
   - This is enforced within Electric Studio's trust boundary (section 11.8).
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
- **Approve and apply.** One `Batch`, so one undo step, labelled with the change set's title
  and actor.
  - **Approval binds to the exact proposal:** the SHA-256 of the change set's canonical
    content, and the base fingerprint it was previewed against.
  - Applying checks both. A proposal modified after approval, or a project changed since the
    preview, is refused, and the person previews and approves it again (D32).
  - An assistant's change set is applied only after a person approves it in the app.
  - Applying writes an engineering change record (section 11.4), with the approval.
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
  - a person (the user of the app);
  - **CLI automation:** the operating-system user, host, command line and time. The CLI acts
    only with explicit authorisation (section 11.8);
  - a generator (template instance, regeneration);
  - an import (file, format and hash);
  - an assistant (provider, model, session; the MCP client as it identified itself, recorded
    as a claim).
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

### 11.8 Trust boundary

Approved distinction (D31): AI that the application runs, versus agents that the application
doesn't control.

| Inside the boundary: enforced | Outside the boundary: detected where possible |
| --- | --- |
| The built-in assistant (AI1 onwards) | An agent with the user's own account privileges: a coding agent with a shell, a desktop-control tool, a script |
| MCP clients, through `ecad-mcp` | Direct edits of project files by any program |
| The app's review and approval panel | A terminal that an agent may be controlling |
| Trusted CLI automation, authorised explicitly (below) | — |

- **Inside:** actors are assigned by Electric Studio. Assistants and MCP clients can only
  query, prepare and preview. Approval is a person's act in the app, bound to the exact
  transaction (11.3).
- **Outside:** an agent running with the user's own privileges can do whatever the user can:
  run the CLI, edit files, operate the app's window.
  - **Electric Studio doesn't claim to attribute those changes to an actor.**
  - Stronger enforcement needs OS-level isolation: the agent runs under a separate account
    with no write access to the project folder, and a person approves in the app under
    their own account.
  - This is stated in the documentation and in the MCP setup guide.
- **Trusted CLI automation stays supported** (scripts, CI). An editing CLI command applies only
  with explicit authorisation:
  - the project allows automation (a setting a person turns on in the app);
  - the command is given `--apply`; without it, every editing command is a dry run.
  - The change record names the actor `CLI automation` with OS user, host and command line.
    The data it writes has provenance `Imported` or `Entered` by automation, and stays
    unverified until a person verifies it.
  - The setting is a policy for trusted pipelines, not protection against an agent that can
    change it. That agent is outside the boundary.
- **Detecting outside changes.** Every save writes a manifest of each file's SHA-256 to
  `.ecad/state.json`.
  - On load, files that differ from it, or that are missing or new, are reported as **modified
    outside Electric Studio**, with an impact report against the last saved state.
  - The report lists what changed, not who changed it. A person can accept the changes (a
    change record with actor *external, unknown*) or restore from the last baseline.
  - This detects changes. It can't prevent them, and a tool that rewrites the manifest too
    goes undetected.

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
- **Replacing equipment** keeps the device and its identity: same id, tag, drawings, wires
  and layout position. Its product changes. **Terminal reconciliation is explicit:**
  - the operation proposes a mapping from each existing terminal to a terminal of the new
    product's pattern, matched by function and designation. The mapping is a suggestion;
  - the preview shows it, with every terminal that has no match: its wires, drawings and PLC
    assignment, and new terminals the drawings don't show yet;
  - a person confirms or edits the mapping. Nothing is dropped or reconnected without that.
    Mapped terminals keep their `TerminalId`, and so their wires.
  - Unmatched terminals with wires can't be deleted by the replacement. They stay, flagged,
    until a person rewires or removes them explicitly.
  - The impact analysis also reports a larger footprint or clearance than the space, a
    different rating against the requirement, and compatible alternatives (5.1).
  - The change report lists *replace*, with the terminal mapping, not *remove* and *add*.
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
- **Working on an as-built machine.** Once an as-built baseline exists, every change belongs
  to the open modification package. A first change with none open asks a person to open one,
  and nothing is changed until they do (D40).
- **One open package per project** (D36). There is no branching, no merging, and no
  concurrent packages.
  - *Cancel:* a person discards the package's changes. It is an explicit global operation with
    a preview: the working project returns to the package's as-built baseline. The package and
    its change records are kept, marked cancelled.
  - *Reopen:* a cancelled package, or a released one not yet installed, can be reopened. Its
    changes are reapplied as one change set against the current as-built baseline; the
    preview reports any conflict with what happened since.
  - *Recovery:* the package's state and records are saved through the store's journal, so a
    crash leaves the package as last saved. On reopening the project an open package is
    restored as open.
  - *Comparison:* at any time against the package's original as-built baseline (12.5).

## 13. Performance requirements

Storage stays transactional JSON (D5). The targets below are **initial engineering objectives,
not verified capabilities** (D34). Bottlenecks are profiled before any index, cache, database
or other complexity is introduced.

**Representative projects,** generated or kept in the repository:
- the conveyor;
- the reference project;
- a large synthetic project (2 000 devices, 20 000 wires, 300 pages) that carries real
  graphical assets: underlays from the DWG test set, product geometry, a full documentation
  set;
- a library of 200 000 products from 50 manufacturers, with geometry for a part of them.

| Operation | Project | Time target | Peak memory target |
| --- | --- | --- | --- |
| Open a project, cold (first open after boot or cache drop) / warm | large | < 4 s / < 2 s | < 1.5 GB |
| Save after a one-page change | large | < 1 s | — |
| Apply an interactive edit, with its checks | large | < 50 ms | — |
| Run all checks | large | < 1 s | — |
| Render a page | large | < 50 ms | — |
| Preview a change set of up to 200 operations | large | < 1 s | < +300 MB |
| Incremental generation of one template instance | large | < 1 s | — |
| Make a baseline / compare a baseline with now | large | < 5 s / < 2 s | < +500 MB |
| Archive / restore with verification | large | < 30 s each | streaming, < 500 MB |
| Open a component library, cold / warm | library | < 6 s / < 3 s | < 2 GB |
| Search the library | library | < 100 ms (95th percentile) | — |
| Import a manufacturer list | 50 000 rows | < 30 s | < 1 GB |

- **Long-running operations** report progress and can be cancelled: import, generation,
  baseline, compare, archive, restore, export, checks on large projects.
  - Cancelling leaves the project as it was. These operations prepare a change set or staged
    files and commit only at the end.
- **Repeatable regression measurements, under controlled conditions:**
  - **Deterministic counts on every PR:** instructions executed and bytes allocated for each
    benchmark, measured under an instruction-counting harness (Valgrind-based). They don't
    depend on the runner's load, so a change beyond a set percentage is a real regression,
    reported on the PR.
  - **Wall-clock and peak memory on a controlled machine:** a dedicated machine (or a
    dedicated self-hosted runner), with fixed power settings, nothing else running, several
    repetitions, and median and spread recorded against the previous result. Cold
    measurements drop the file cache first.
  - **Shared CI runners report wall-clock only as information, never as an unconditional
    merge blocker** (D34). A deterministic-count regression beyond threshold, or a confirmed
    regression on the controlled machine, needs an explanation in the PR before merging.

## 14. Where the approved decisions meet the current code

The review asked for decisions that conflict with implementation evidence to be challenged.
None needs reversing. These need the stated adjustment, or a decision of their own:

1. **D18 versus MCP `--write` and the CLI** (resolved by D31 and D32).
   - **What exists:**
     - `ecad-mcp --write` is a session-wide write permission for whatever client connects.
     - The `ecad` commands (`import-tia`, `import-dxf`, `library add`...) edit projects
       without confirmation, by design, for scripts and CI.
   - **What follows:**
     - `--write` is removed. MCP clients query, prepare and preview; a person approves the
       exact transaction in the app. This breaks today's MCP workflow (docs/MCP.md) on
       purpose.
     - CLI editing commands become dry runs unless given `--apply` in a project that allows
       automation, and are recorded as CLI automation (11.8).
     - No terminal prompt is offered as human approval.
   - **The limit** (11.8): an agent with the user's own privileges is outside the trust
     boundary. Its changes are detected where possible, not attributed or prevented.
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
   - **What follows:** this is a substantial new track (C0–C2 in the roadmap), not a detail
     of the layout milestone. Geometry algorithms start as small model functions; a dedicated
     geometry adapter comes only on evidence (D30).
8. **D26 baselines inside the project, and git** (resolved by D38). Underlays, product
   geometry and issued PDFs in `objects/` are binary and grow a Git repository with every
   baseline.
   - Git is never required; native archive and restore (4.7) carry everything.
   - Git LFS may be recommended to companies that use Git.
   - Excluding `objects/` from Git is documented as safe only with a separate, verified
     backup.
9. **D10 stored generated values.** Keeping the last generated values (not just hashes) roughly
   doubles the size of generated content in page files. That is accepted, because it is what
   makes field-level merging possible.
10. **D6 immutable revisions in libraries.** A library can't know which projects use which
    revision, so it can't safely drop old revisions for them. Because projects keep the
    revisions they use in their own `objects/`, a library may prune old revisions without
    harming any project.
11. **#32's import rule versus D37's conflict resolution.**
    - **What exists:** #32's library import lays a list over the library: a non-empty incoming
      value always replaces the library's. A field a person corrected in the library is
      overwritten silently by the next import of the same price list.
    - **What follows:** imports compare three ways against that source's last import (5.1),
      and the import is a change set with a report. #32's merge is reused only for the
      "unchanged since last import" case.
12. **#32's column mapping versus D37's reusable mappings.** #32 takes `--map` per run. Saved,
    named import profiles per source are added in M9, so a monthly price list imports the same
    way each time.
13. **Class-default symbol mapping versus D37's "assignments are suggestions".** Section 5.1
    originally proposed a default symbol per class to draw a selected product. That stays, but
    what it produces is marked Suggested and unverified unless the product's data is
    authoritative.
14. **Interactive terminal confirmation** (proposed in the first revision of 11.1) is
    withdrawn by D32. Approval is only in the app.
15. **The GUI is also reachable by desktop-control agents.** An agent driving the screen can
    click Approve. That is the same boundary as item 1: the app's panel is the trusted
    interface within Electric Studio's control, not a proof of a human. The documentation
    says so (11.8).

## 15. Reconciliation of the original component-library and CAD requirements (D37)

Each restated requirement, where this architecture covers it, and what was missing or
conflicting before this revision.

**Component library**

| Requirement | Covered in | Before this revision |
| --- | --- | --- |
| Manufacturer-independent canonical product definitions | 5.1 (1) | **Ambiguous:** read as one neutral schema for all sources, with generic products allowed; to confirm (D39) |
| Stable identities for manufacturers, products, revisions, installed devices | 4.1, 5.1 (2) | Covered (manufacturer ids added in the last revision) |
| Electrical and purely mechanical components | 5.1 (3) | Covered |
| Typed specifications with units and provenance | 5.1 (4), 11.4 | Covered |
| Electrical functions, terminals, symbol mappings | 5.1 (5) | **Functions were implicit** in the terminal pattern; now explicit |
| Mechanical geometry, dimensions, mounting, clearances | 5.1 (6), 8 | **Mounting information and clearances were missing;** added, and used by layout checks |
| Accessories, compatible products, variants | 5.1 (7) | **Compatible products and variants were missing;** added as relations |
| User-created components and custom libraries | 5.1 (8) | **Implicit;** now explicit, with several libraries in order of preference |
| Native portable exchange packages | 5.1 | Covered |
| Self-contained project snapshots | 4.2, 4.4 | Covered |
| Immutable revision contents and dependent assets | 4.4 | Covered |
| Controlled three-way updates and conflict resolution | 4.4, 5.1 | Covered for library to project; **conflicts on import into the library were not** (item 11 of section 14); now covered |
| Offline operation | 5.1 (10), 11.1 | Covered |

**Manufacturer imports**

| Requirement | Covered in | Before this revision |
| --- | --- | --- |
| CSV and Excel with reusable column mappings | 5.1, M9 | **Mappings were per run** (#32); import profiles added |
| BMEcat with ETIM; ETIM xChange; DXF and SVG assets | 5.1, C2, manufacturer-data track | **DXF and SVG product geometry** were in C2 but not listed as first-priority imports; now listed |
| ECLASS Advanced, AASX on need and licensing; STEP and DWG on feasibility; EDZ after rights are verified | 5.1 | **STEP and DWG product geometry weren't mentioned;** added (STEP deferred with 3D) |
| Defensive parsing, bounded resources, archive safety | 5.1 | Covered |
| Transactional application and rollback | 5.1, 11.3 | **Not stated for imports;** now every import is a change set |
| Duplicate detection | 5.1 | Within-file only (#32); **against the library and likely duplicates added** |
| Conflict resolution; import reports | 5.1 | **Missing** (#32 had a summary only); added |
| Preserve unknown properties; never fabricate missing data | 5.1 (4) | Unknown columns kept (#32); **"never fabricate" now explicit** |
| Automatic symbols, functions and terminal mappings stay unverified unless authoritative | 5.1, 11.4 | **Conflicted** with the class-default mapping (section 14, item 13); resolved |
| Synthetic fixtures in CI; real manufacturer files for interoperability, subject to licence | 5.1, ROADMAP | **Real-file acceptance was missing;** added |

**Native shared 2D CAD**

| Requirement | Covered in | Before this revision |
| --- | --- | --- |
| Component geometry creation and modification | 8, C2 | Covered |
| Symbol and connection-point editing | 8, C2 | Covered; **in-use connection-point edits now an explicit operation** |
| Plates, doors, cut-outs | 8, M15 | Covered |
| Machine layout creation and editing; equipment positioning; cable routing | 8, M15 | Covered |
| Lines, polylines, arcs, circles, rectangles, text, dimensions | 8, C0 | Covered |
| Layers, snapping, alignment, **rotation and scaling** | 8, C1 | **Rotation and scaling of geometry weren't listed;** added |
| Reusable blocks | 8, C0 | Covered |
| Importing and adapting manufacturer geometry | 8, C2 | Covered; adaptation (editing an imported outline) made explicit |
| A shared capability, not a general-purpose CAD application | 8 | **Not stated;** now stated |
| Connectivity independent of geometry; graphics never silently alter semantics | 4.5, 8 | **Implied;** now explicit rules |
| Full 3D outside initial scope | 8 | Covered |

No conflict remains between the restated requirements and D1–D36, apart from D39's reading of
"manufacturer-independent".

## Decision record

Reviewed by the owner with an independent technical reviewer on 27 Nov 2026: D1–D28 at
`ce38394` (D29 added then), D30–D38 at `867d607`.

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
| D30 | CADCraft | Kept behind an adapter; **the model owns geometry and semantics,** with no dependency on rendering or UI crates; a dedicated geometry adapter only on implementation evidence; no premature abstraction | 8 |
| D31 | AI enforcement boundary | Application-controlled AI versus unrestricted agents; **enforced in the built-in assistant and MCP;** agents with user-account privileges are outside the boundary (OS isolation for more); **outside changes detected and reported where possible, not claimed attributable;** trusted CLI automation with explicit authorisation and provenance | 11.8 |
| D32 | MCP write | `--write` removed; MCP clients query, prepare, preview; **a person approves the exact transaction in a trusted application interface,** a substantial proposal as one transaction; **no terminal confirmation as human authorisation;** stale or modified proposals refused | 11.1, 11.3 |
| D33 | Logical nodes | Junctions, potentials, interruptions become nodes; connectivity, names, drawings and cross-references preserved; stable ids and explicit name scoping; **ambiguous mappings detected, never merged or reconnected silently** | 4.5, 9 |
| D34 | Performance | Targets are **initial objectives, not verified capabilities;** peak memory, cold and warm, representative projects with real assets, cancellation and progress, repeatable measurements under controlled conditions; shared-runner wall clock never an unconditional blocker; profile first | 13 |
| D35 | SVG | Library chosen in C0 on maintenance, licence, geometry, numerical correctness and limits; scripts, external resources, network and outside file access disabled; unsupported and lossy conversions reported | 8 |
| D36 | Modification packages | One open per project; **cancel, reopen, recovery, comparison** with the original as-built baseline; no branching, merging or concurrent packages | 12.7 |
| D37 | Original requirements | Restated and reconciled | 5.1, 8, 15 |
| D38 | Storage and Git | **No Git, LFS or external VCS required;** native archive and restore with completeness and integrity checks; LFS may be recommended; `objects/` excluded only with a verified backup; safe handling of missing, damaged and unreferenced objects; cleanup never deletes referenced content | 4.7 |

Also confirmed as foundational: **minimal-impact changes** across all workspaces, generators,
library updates and AI operations, for unreleased projects too (section 12).

The approved conditions of D1–D29 stand, in particular:
- stable identities;
- minimal-impact changes;
- retrofit preservation;
- explicit terminal reconciliation on replacement (12.6).

### Open, raised by this revision

| # | Decision | Proposed |
| --- | --- | --- |
| D39 | What "manufacturer-independent canonical product definitions" means | One neutral product schema for every source, and generic products (no manufacturer) allowed as placeholders and user components |
| D40 | Changes after an as-built baseline | Every change belongs to the open modification package; with none open, a person is asked to open one first |
| D41 | The controlled benchmark machine | A dedicated self-hosted runner, or the owner's machine run by hand before releases; deterministic counts on every PR either way |
| D42 | CLI automation authorisation | A project setting a person turns on in the app, plus `--apply` per run; recorded as CLI automation |
| D43 | Outside-change detection | A saved manifest of file hashes (`.ecad/state.json`); differences reported on load, accepted or restored by a person |
