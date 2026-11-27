# What is missing for a complete diagram

A gap analysis of Electric Studio after M6, against a real 89-sheet project
drawn in SPAC Automazione (an S7-1200 machine panel; see
[SPAC-NOTES.md](SPAC-NOTES.md)). The question is what that project contains
that Electric Studio cannot draw yet. Nothing from that project is copied
here; the categories are generic.

The gate project ([MVP-GATE.md](MVP-GATE.md)) proved the engine: devices,
wires, potentials, cross-references, terminals, PLC channels, cables, parts,
checks and exports. What is missing is mostly what turns a correct netlist
into a finished document set.

## How the sample project is built

| Sheets | Content | Electric Studio today |
| --- | --- | --- |
| 1–2 | Cover with project data form; sheet index | Title block on every page; table of contents as CSV only |
| 3–8 | Enclosure outline with dimensions, door drilling with button graphics, inner layout with device graphics, machine layout with cable trays | Mounting-plate layout with rails, ducts and footprint rectangles |
| 9–24 | Supply and distribution, busbar, inverters as black boxes with labelled sections and network ports, shielded motor cables with screens drawn, motor nameplate data, fuse ratings and wire specs written on the drawing | Power circuits, black boxes with terminals, cables; no text on the drawing beyond tags |
| 25–37 | 24 V supply with notes, safety relay, sensors with connector pinouts, Ethernet switch, HMI, PLC hardware layout | Partly: no notes, connector pinouts or module graphics |
| 38–57 | PLC I/O with descriptions in two languages, lamp pushbuttons with colour codes, contact mirrors, exchange signals | PLC channels, contact tables, cross-references; one language |
| 58–77 | Terminal-strip drawings (generated), cables between strips and junction boxes, connector pinouts | Terminal diagram as CSV only |
| 78–89 | BOM over many pages, symbol legend, enclosure temperature-rise calculation | BOM as CSV only |

## Gaps, by priority

### 1. Generated document pages (high)

Reports exist only as CSV files. A delivered project prints them as sheets in
the same frame and title block as the schematic.

- Cover page with a project data form (customer, machine, voltages, standards), from `ProjectInfo`.
- Table of contents pages.
- BOM pages, split over as many sheets as needed.
- Terminal-strip drawings: each terminal with the wires and cables on both sides, PE and shield terminals, bridges.
- Cable list and PLC I/O list pages.
- Symbol legend generated from the symbols the project uses.
- Generated pages are derived and regenerated (architecture rule 1), never edited by hand, and appear in the page order, the PDF and the DWG export.

### 2. Annotations on pages (high)

There is no way to put anything on a page that is not a device, wire or box.

- Free text and multi-line notes, with size, alignment and rotation.
- Lines, rectangles and circles for graphics that carry no electrical meaning.
- Images (product pictures, logos).
- Dimensions on layout pages.
- Text attached to a wire: cross-section, colour and type (for example `1x2.5 BK`), filled from the wire's own data, not typed.

### 3. Device data shown on the drawing (high)

- Technical data fields per device (rating, power, voltage, current, setting range), shown next to the symbol and filled from the part where possible: breaker ratings, fuse sizes, motor nameplate data.
- A second-language description for devices, PLC channels and terminals. Pages already have a second-language subtitle; the rest of the text does not.
- Pushbutton and lamp colours as device data, shown and used in the BOM.

### 4. Black boxes and connectors (medium)

- Sections inside a black box, each with a caption (power supply, digital inputs, relay outputs, analogue, brake).
- Two-line terminal labels: terminal name and function.
- Network ports (RJ45, Profinet) as connection points with their own symbol.
- Multi-pin connectors (M12, Harting-style) with a pinout table: pin number, core colour, signal.

### 5. Symbol library breadth (medium)

The library has about 46 IEC symbols. The sample also needs:

- Inverter or variable-speed drive, soft starter.
- Transformer, heater or resistor load, brake resistor.
- Earth, chassis and functional earth; cable screen and its termination to PE.
- Busbar.
- Photoelectric, inductive and magnetic sensors with their connector; RTD and thermocouple inputs.
- Safety relay, Ethernet switch, HMI panel.
- Panel fan with filter.
- PLC rack and module graphics (CPU, I/O modules) for a hardware overview sheet.

### 6. Front and machine layouts (medium)

- Door and front layout: cut-outs and the devices seen from the front (buttons, lamps, HMI), with dimensions.
- Device outlines from the part's real graphic instead of a rectangle.
- Machine layout: positions of motors and sensors on the machine, cable trays.

### 7. Calculations (low)

- Enclosure temperature rise (IEC/TR 60890), from the dissipated power of the parts in a panel. Needs a power-loss field on parts.

### 8. SPAC migration (after the above)

Planned for when Electric Studio can draw a complete project. First form: data
plus underlay (see [SPAC-NOTES.md](SPAC-NOTES.md)).

- Read `dbSPAC.db` and `MCables-Data.db`: devices, parts, terminals, connections, wire numbers and types.
- Split multi-sheet DWGs by layer into one underlay per page.
- Map SPAC block names to Electric Studio symbols.
- Open question: older projects may have Access files only.

## Proposed milestones

| Milestone | Scope |
| --- | --- |
| M7 Annotations and device data | Free text and notes, graphics, images, dimensions; wire annotations from wire data; technical data fields shown on the drawing; second-language descriptions; button and lamp colours |
| M8 Generated pages | Cover, table of contents, BOM, terminal-strip drawings, cable and PLC I/O lists, symbol legend as regenerated pages in the frame, in PDF and DWG export |
| M9 Symbols and black boxes | The missing symbols above; black-box sections, captions, two-line labels, network ports; connectors with pinouts |
| M10 Front and machine layouts | Door layout with cut-outs and front graphics, device outlines, machine layout |
| M11 SPAC migration | Data import from the SPAC databases plus per-sheet underlays |
| Later | Temperature-rise calculation |

M7 and M8 together are what makes the output look like a delivered project;
M9 widens what can be drawn. A sample project redrawn end to end with M7–M9
would be the gate before M11.
