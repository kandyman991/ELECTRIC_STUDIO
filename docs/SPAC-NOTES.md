# SPAC Automazione projects: what is inside

Notes from reading one complete SPAC Automazione project (89 sheets, an
S7-1200 machine panel). They describe where SPAC keeps its data. They record
structure only: no project, customer or device data from that project is kept
in this repository.

Migration from SPAC is planned for later, once Electric Studio can draw a
complete diagram by itself (see [GAPS.md](GAPS.md)). The planned form is
"data plus underlay first": devices, wires and terminals come in as data, and
each sheet comes in as an underlay to check and redraw over.

## The drawing files

- SPAC runs on an AutoCAD OEM engine, so its sheets are ordinary DWG files.
  They import as underlays today (all 18 DWGs of the sample import; 15 sit
  1:1 on the sheet). See [UNDERLAY-INTEROP.md](UNDERLAY-INTEROP.md).
- A *multifoglio* ("multi-sheet") DWG holds several sheets in one file, one
  sheet per layer. Which layer is which sheet is not in the DWG itself; it is
  in the `PlotSequence` table of the project database (below), with the
  columns `riga`, `multifoglio`, `foglio`, `descrizione` and `layer`.
  Migration must split such a DWG by layer into one underlay per page.
- Symbols are AutoCAD blocks with attributes. The sample uses 84 distinct
  blocks. Their names follow families: `_MORS*` terminals, `_ALIM_*` and
  `_SIGN_*` potential and signal arrows, `INP*`/`OUT*` PLC channels,
  `NA`/`NC`/`SC` contacts, coils, motors, motor protectors, breakers,
  `BLK*` black boxes, and whole-device graphics for CPUs, HMIs and switches.
  A block-to-symbol map is the core of a migration.

## The databases

| File | Format | What it holds |
| --- | --- | --- |
| `dbSPAC.db` | SQLite, text in UTF-16 | The project: components, BOM, terminals, cross-references, layouts, panel structure, the DWG list |
| `MCables-Data.db` | SQLite | The netlist: every connection with wire number, both ends and wire type |
| `*.mdb`, `._mdb` | Microsoft Access | Older copies and link tables. Older projects may have only these |

### `dbSPAC.db`

| Table | Holds |
| --- | --- |
| `Elementi` | Every component placed on a sheet: tag, block, sheet, position |
| `Distinta` | The bill of materials: part codes, manufacturer, quantity |
| `Morsettiere`, `MorsConn`, `Ordinamento_Morsettiere` | Terminal strips, what connects to each terminal, the order of terminals in a strip |
| `SegnaliniDatiCavo` | Wire markers and cable data |
| `BIT_AGG` | Extra PLC bit data (addresses and comments) |
| `Rimandi` | Cross-references between sheets (interruption points, contact mirrors) |
| `Layout` | Devices placed on layout sheets |
| `Struttura_Quadri` | Panel and location structure |
| `Dwg`, `PlotSequence` | The DWG files and which sheet is which layer |
| `XDict` | Extended attributes |

### `MCables-Data.db`

| Table | Holds |
| --- | --- |
| `Collegamenti` | One row per connection: `NumeroFilo` (wire number), `IDComp_P`/`IDComp_A` and `PIN_P`/`PIN_A` (from and to device and pin), `TAG_PIN`, `Mors` (terminal), `IDFilo`, `ColoreFilo` (colour), `NomeCavo`/`NomeVena` (cable and core) |
| `Componenti` | Components as the netlist sees them |
| `AcDbLine` | The wire segments as drawn |
| `Fili` | Wire types: designation, cross-section, colour |

`Collegamenti` maps almost directly onto Electric Studio's wire list. Together
with `Elementi`, `Distinta` and the terminal tables it is enough to rebuild
devices, parts, terminals and connections as data, before any sheet is
redrawn.

## Open questions for migration

- Whether older projects have `dbSPAC.db` or only Access files. Reading
  Access would need an MDB reader.
- How SPAC numbers wires (per potential or per connection) across the years.
- Whether the block names stayed stable across SPAC versions.
