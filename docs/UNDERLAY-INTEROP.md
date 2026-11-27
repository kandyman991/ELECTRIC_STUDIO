# Underlay import: interoperability record

What has been checked with drawings made by other programs, and what hasn't. Kept apart from the
tests because the files aren't ours to commit.

## 2026-11-27: a real SPAC Automazione project

A complete project made with SPAC Automazione (an Italian electrical CAD on AutoCAD's DWG
engine), supplied by the owner; not committed. 18 DWG files: power, auxiliaries, CPU, inputs,
outputs, signals, motors, terminal strips, tables, panel layout, and SPAC's own backup copies.

All 18 import, in 0.1–3 s each. SPAC sheets are A3 frames (405 × 275 drawn from (10, 15) on
paper from (0, 0)) and land at 1:1 on Electric Studio's sheet; a sheet taller than A3 and a
1.4 m wide panel layout are shrunk to fit. The files give no `$INSUNITS`: millimetres are
assumed and the import says so. A sheet drawn from its underlay alone matched the PDF of the
same sheet: frame, grid, symbols, wire numbers, cross-references, title block text.

Two problems this project found, fixed before this record:

- The estimate of flattening (which refuses drawings before they are drawn) was 100–1500 ×
  what is drawn, refusing the layout and terminal-strip sheets. It counted hidden attributes
  (SPAC symbols carry many), entities on layers switched off or frozen, every arc and spline at
  its worst case, and hatch patterns across the whole drawing. It now skips what the renderer
  skips and counts curves as the renderer cuts them; on these drawings it is 1.6–6 × what is
  drawn, and a test holds it at or above what is drawn.
- Sheets were "fitted" (3.7 % enlarged) because Electric Studio's sheet doesn't start at
  (0, 0). Placement now never enlarges: real size where drawn, or from the sheet's corner, or
  centred; shrunk only if too big.

## 2026-11-27: LibreDWG test corpus

The test data of [LibreDWG](https://github.com/LibreDWG/libredwg) (`test/test-data`, GPL; not
vendored here): DWG and DXF files made by AutoCAD and other programs, from R1.4 to AutoCAD 2018,
including binary DXF. Each was imported with `ecad import-dxf` (debug build, through the worker
process) into a fresh project.

| Result | Files |
| --- | --- |
| Imported | 183 |
| Refused: no drawable entities (object-only test files, a single viewport) | 65 |
| Refused: only a point (points aren't drawn) | 7 |
| Refused: DWG older than R13 (AC1003, AC1004, AC1006), which CADCraft doesn't read | 8 |
| Refused: a deliberately malformed DXF | 1 |
| Refused: an empty-looking `PolyLine2D.dxf` holding only a viewport | 1 |

Every refusal is one of the above; no import crashed, hung or produced shapes the model refused.
The slowest was a hatch-heavy DWG: 654 646 points in 2.5 s. Units came from `$INSUNITS` where
set (inch, mm, metre seen) and were reported as assumed millimetres where not.

Two problems this run found, both fixed before this record:

- Text-only drawings were refused ("shapes out of bounds"): drawn glyphs reach past CADCraft's
  extents. Bounds are now taken from what is drawn.
- AutoCAD's own `example_*` drawings were refused as "over 1 000 000 units across": they hold an
  attribute 3.5 km from the rest (AutoCAD's header says the same). The source size limit is now
  10⁹ units; such a drawing comes in fitted to the sheet, as AutoCAD's zoom extents would show
  it, and its position and scale can be changed after.

To repeat it:

```sh
git clone --depth 1 --filter=blob:none --sparse https://github.com/LibreDWG/libredwg.git
git -C libredwg sparse-checkout set test/test-data
ecad new-sample proj start
for f in $(find libredwg/test/test-data -maxdepth 2 -name '*.dwg' -o -maxdepth 2 -name '*.dxf'); do
  ecad import-dxf proj '#1' "$f" | tail -1
done
```

## Not yet checked

- Electrical CAD programs other than SPAC (EPLAN and the like): none at hand.
- How an underlay looks against its source in the CAD program it came from (only import, sizes
  and units were checked, not a visual comparison).
