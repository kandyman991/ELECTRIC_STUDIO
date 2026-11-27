# Product gap analysis

What Electric Studio is missing, measured two ways:

- **Part A:** against the machine-centric workflow in
  [PLATFORM-ARCHITECTURE.md](PLATFORM-ARCHITECTURE.md).
- **Part B:** against a real 89-sheet project drawn in SPAC Automazione (an S7-1200 machine
  panel; see [SPAC-NOTES.md](SPAC-NOTES.md)). Nothing from that project is copied here; the
  categories are generic.

Each gap names the milestone in [ROADMAP.md](ROADMAP.md) that closes it.

The gate project ([MVP-GATE.md](MVP-GATE.md)) proved the schematic engine: devices, wires,
potentials, cross-references, terminals, PLC channels, cables, parts, checks and exports.

## Part A: the machine-centric workflow

| Step | Needs | Today | Gap | Closed by |
| --- | --- | --- | --- | --- |
| 1. Machine architecture | Sections (=) and locations (+) as model entities; devices assigned to them | Structure only as text in tags; locations only as box labels on pages | No structure model at all | M8 |
| 2. Equipment requirements | Maker-independent requirements per section, with parameters | Nothing | Whole layer missing | M11 |
| 3. Product selection | Shared library; selection against requirements; provenance and revisions | Per-project catalogue; shared library and import in draft #32, without revisions or matching | Library not merged; no revisions, numeric data, terminal patterns or symbol mapping; no matching | M9, M11 |
| 4. Electrical system | Devices, persistent terminals, logical nodes and wires without drawings; circuit templates | Wires exist only between drawn symbols; terminals implicit; junctions and potentials are devices; no templates | Connectivity bound to drawings; no terminal or node entities; no template library | M7, M10, M12 |
| 5. Schematics | Deterministic generation; manual refinement kept on regeneration | Manual drawing only (strong editor) | No generator, no origin tracking | M13 |
| 6. Layouts and CAD | Native 2D CAD (primitives, dimensions, layers, snapping, alignment, blocks; symbol and component geometry editing; SVG); cabinet plates and doors with cut-outs; machine layout with cable routes | Plate layout with rectangles, rails, ducts, wire lengths through ducts | No CAD geometry model or tools beyond grid snap, move and rotate; no symbol or geometry editor; no SVG; no door or machine layout, no real outlines, no cable routes | C0–C2, M15 |
| 7. Documentation and validation | Synchronised document set; consistency across layers | CSV reports; drawing-level checks | No generated pages; no requirement, structure or product checks | M14, with checks added in each milestone |
| Reuse | Functional modules (machine units) | Nothing | Missing | M16 |
| Product data | The D37 component library (PLATFORM-ARCHITECTURE.md sections 5.1 and 15): neutral product schema, canonical ids, mechanical components, functions and terminals, mounting and clearances, relations, user components, packages, immutable revisions; imports with profiles, reports, duplicates, three-way conflicts and import security; ETIM xChange | Per-project catalogue by order number; #32 keyed by (maker, number), per-run column maps, overwriting merge, size limits on import | Almost all of it beyond #32's CSV/Excel reading | M9, C2, manufacturer-data track |
| Engineering states and storage | Designed, released, as-built and proposed modification kept apart; one open package with cancel, reopen, recovery; change and approval records; native archive and restore without Git; safe object handling | Title-block revision list; transactional saves | No states, packages, records, archives or object store | AI0 (records), S1 |
| Lifecycle stability | Edits that change only what they name; numbering policy with reserved and retired identifiers; page insertion; baselines (released, as-built); change impact reports; incremental generation | Explicit "renumber all"; undo; saves all or nothing | Joining potentials renumbers wires; lowest-free reuse; page insert rewrites every page's order; no baselines or change reports | S0, S1, then the rules in M10, M13 and M15 |
| AI readiness | One typed query and operation interface; change sets approved in the app as exact transactions; a documented trust boundary with outside-change detection; trusted actor assignment; provenance and verification; direct and indirect safety rules; a replaceable local provider | MCP server with typed tools over the editor's operations, session-wide write with `--write`, with undo | No proposals, previews or acceptance; session-wide AI write; no actors, provenance or verification; no safety rules; operations defined in three places | AI0, provenance in M8–M14, AI1–AI5 |

Also needed by the workflow (from the audit in PLATFORM-ARCHITECTURE.md section 2): stable
terminal identity, a stable content hash, and typed PLC channel data (M7).

## Part B: what a delivered project contains

### How the sample project is built

| Sheets | Content | Electric Studio today |
| --- | --- | --- |
| 1–2 | Cover with project data form; sheet index | Title block on every page; table of contents as CSV only |
| 3–8 | Enclosure outline with dimensions, door drilling with button graphics, inner layout with device graphics, machine layout with cable trays | Mounting-plate layout with rails, ducts and footprint rectangles |
| 9–24 | Supply and distribution, busbar, inverters as black boxes with labelled sections and network ports, shielded motor cables with screens drawn, motor nameplate data, fuse ratings and wire specs written on the drawing | Power circuits, black boxes with terminals, cables; no text on the drawing beyond tags |
| 25–37 | 24 V supply with notes, safety relay, sensors with connector pinouts, Ethernet switch, HMI, PLC hardware layout | Partly: no notes, connector pinouts or module graphics |
| 38–57 | PLC I/O with descriptions in two languages, lamp pushbuttons with colour codes, contact mirrors, exchange signals | PLC channels, contact tables, cross-references; one language |
| 58–77 | Terminal-strip drawings (generated), cables between strips and junction boxes, connector pinouts | Terminal diagram as CSV only |
| 78–89 | BOM over many pages, symbol legend, enclosure temperature-rise calculation | BOM as CSV only |

### Gaps, by priority

#### 0. Components library with manufacturer import (high, asked for; M9 and the manufacturer-data track)

Today each project has its own parts catalogue (`parts.json`), typed in by
hand: order number, manufacturer, description, company code, contact blocks
and size. There is no library shared between projects and no import.

- A library outside the projects (a folder per company, kept under version
  control or on a share), searchable by manufacturer, order number, class and
  technical data. A project keeps a copy of every part it uses, so a project
  still opens on its own; "update from library" shows what changed.
- A richer entry: technical data as properties with units (rated current,
  voltage, power, setting range, cross-section), classification (ETIM or
  ECLASS class), the symbol or symbols it is drawn with, its connection points
  (terminal names), contact blocks, size and front view for the layout, power
  loss, pictures and data-sheet links, accessories.
- A default symbol per class (a contactor class brings a coil, three main
  contacts and its auxiliary contacts), so an imported part can be placed
  without a hand-made mapping.

Formats manufacturers publish, in the order to support them:

| Format | Who publishes it | What it brings | Notes |
| --- | --- | --- | --- |
| CSV or Excel with a column map | Everyone (price lists, own lists) | Order number, description, prices, any column | Works with any manufacturer from day one |
| BMEcat with ETIM features (XML) | ABB, Phoenix Contact, Weidmüller, Rittal and many more, free downloads | Commercial data, ETIM class and technical features, pictures, documents | No symbols or connection points; free to use |
| ECLASS Advanced (XML) | Phoenix Contact, Siemens, WAGO, Eaton and others, often through CADENAS | Technical data plus CAE data: connection points, functions, 2D drawings | Check the ECLASS licence terms before shipping a reader |
| AASX (Asset Administration Shell, IDTA nameplate and technical data) | Growing (Siemens and others) | Nameplate and technical data, open specification | Newer; worth reading once BMEcat works |
| DXF/DWG front views and footprints | Most manufacturers | Layout graphics | The underlay reader already reads these |
| EDZ (EPLAN Data Portal) | Almost every manufacturer | Parts, macros, connection points | EPLAN's own format; read the portal and manufacturer terms before any support |

#### 1. Generated document pages (high; M14)

Reports exist only as CSV files. A delivered project prints them as sheets in
the same frame and title block as the schematic.

- Cover page with a project data form (customer, machine, voltages, standards), from `ProjectInfo`.
- Table of contents pages.
- BOM pages, split over as many sheets as needed.
- Terminal-strip drawings: each terminal with the wires and cables on both sides, PE and shield terminals, bridges.
- Cable list and PLC I/O list pages.
- Symbol legend generated from the symbols the project uses.
- Generated pages are derived and regenerated (architecture rule 1), never edited by hand, and appear in the page order, the PDF and the DWG export.

#### 2. Annotations on pages (high; M14)

There is no way to put anything on a page that is not a device, wire or box.

- Free text and multi-line notes, with size, alignment and rotation.
- Lines, rectangles and circles for graphics that carry no electrical meaning.
- Images (product pictures, logos).
- Dimensions on layout pages.
- Text attached to a wire: cross-section, colour and type (for example `1x2.5 BK`), filled from the wire's own data, not typed.

#### 3. Device data shown on the drawing (high; M14, with data from M9)

- Technical data fields per device (rating, power, voltage, current, setting range), shown next to the symbol and filled from the part where possible: breaker ratings, fuse sizes, motor nameplate data.
- A second-language description for devices, PLC channels and terminals. Pages already have a second-language subtitle; the rest of the text does not.
- Pushbutton and lamp colours as device data, shown and used in the BOM.

#### 4. Black boxes and connectors (medium; symbols track)

- Sections inside a black box, each with a caption (power supply, digital inputs, relay outputs, analogue, brake).
- Two-line terminal labels: terminal name and function.
- Network ports (RJ45, Profinet) as connection points with their own symbol.
- Multi-pin connectors (M12, Harting-style) with a pinout table: pin number, core colour, signal.

#### 5. Symbol library breadth (medium; symbols track)

The library has about 46 IEC symbols. The sample also needs:

- Inverter or variable-speed drive, soft starter.
- Transformer, heater or resistor load, brake resistor.
- Earth, chassis and functional earth; cable screen and its termination to PE.
- Busbar.
- Photoelectric, inductive and magnetic sensors with their connector; RTD and thermocouple inputs.
- Safety relay, Ethernet switch, HMI panel.
- Panel fan with filter.
- PLC rack and module graphics (CPU, I/O modules) for a hardware overview sheet.

#### 6. Front and machine layouts (medium; M15)

- Door and front layout: cut-outs and the devices seen from the front (buttons, lamps, HMI), with dimensions.
- Device outlines from the part's real graphic instead of a rectangle.
- Machine layout: positions of motors and sensors on the machine, cable trays.

#### 7. Calculations (low; calculations track)

- Enclosure temperature rise (IEC/TR 60890), from the dissipated power of the parts in a panel. Needs a power-loss field on parts.

#### 8. SPAC migration (after M14; SPAC track)

Planned for when Electric Studio can draw a complete project. First form: data
plus underlay (see [SPAC-NOTES.md](SPAC-NOTES.md)).

- Read `dbSPAC.db` and `MCables-Data.db`: devices, parts, terminals, connections, wire numbers and types.
- Split multi-sheet DWGs by layer into one underlay per page.
- Map SPAC block names to Electric Studio symbols.
- Open question: older projects may have Access files only.

## Milestones

The milestones that close these gaps, their order and acceptance criteria are in
[ROADMAP.md](ROADMAP.md). That replaces the M7–M12 proposal first made in this document.
Nothing from it is dropped: the library is M9 plus the manufacturer-data track, annotations
and generated pages are M14, symbols and black boxes are a track, layouts are M15, and SPAC
migration is a track after M14.
