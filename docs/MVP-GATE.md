# MVP gate: redrawing a real diagram

The MVP gate (after M4) asks: can Electric Studio redraw a real machine diagram well enough to be
used for one? The owner supplied an 81-page production diagram (a cooling-spiral machine with a
PLC). Ten representative pages were redrawn with neutral names as the **reference project**:

```sh
ecad new-sample my-folder reference
ecad check my-folder
ecad export my-folder out
```

The code is `ecad_model::reference::reference_project()`. Customer names, internal article codes
and the original PDF are deliberately not in the repository.

| Page | Content |
|------|---------|
| P2   | Supply, main switch with fuses, distribution block, RCBOs |
| P3   | Cabinet auxiliaries: socket, thermostat, fan, lamp, 24 V supply |
| M3   | Spiral motor on an inverter, through terminal strip -X0 |
| M10A | Fan motors with motor-protection breakers |
| A1   | 24 V distribution with electronic protection |
| A2   | Emergency-stop chain through connectors to three remote boxes |
| A3   | Safety module, reset, safety contactors |
| I1   | CPU inputs: push-buttons, door sensors |
| O1   | CPU outputs: lamps, relays, contactors |
| O3   | Output card driving relays and contactors |

## Result

- `ecad check`: **0 errors**, 58 warnings. The warnings are expected for a partial redraw:
  devices on the field side with no part, spare points, interruption points whose partner page
  wasn't redrawn, and junctions that only join two wires at the end of a bus.
- Bill of materials, compared device by device against the original's BOM pages: **28 devices
  match exactly** (part numbers and quantities), including accessories (light blocks, collars,
  marker holders, the main switch's neutral bar, guide cone and shaft extension).
- Terminal strips: end brackets and end plates now count once per strip (`PartRef::per_strip`,
  the "per strip" checkbox in the app), terminals once per terminal. Totals match the original
  for the strips drawn; the feed-through terminal count follows the terminals actually drawn.
- Layout: no clashes on any page (a test keeps it so). Labels keep 0.5 mm clear of other
  texts, wires, symbols and location-box edges; wires may not share a stretch, run closer than
  3 mm side by side, or line up end to end with another potential closer than 16.5 mm.
  Tags, cross-references and wire numbers are placed where they meet these rules; a short wire
  whose number another wire of the page already shows goes unlabelled when there's no room.
- Wire numbers follow the original's per-potential scheme (`{page}{n:02}`), tags follow
  `{page}{letters}{n}`; both are project settings.

## Gaps found

In rough order of how much they matter for drawing real machines:

1. ~~**Parts per terminal type.**~~ — done in G5c: a part on a strip can count per terminal of
   one kind (new PE terminal symbol); -X0's two ZPE now match the original.
2. ~~**Changeover contact symbol** (11-12-14) is missing~~ — done in G5b (`iec.contact_co`).
3. ~~**Contact numbering by block position.**~~ — done in G5b: parts list their contact blocks
   (`13-14`, `21-22`, `11-12-14`) and contacts take their numbers from them.
4. ~~**Black boxes with internal bridges.**~~ — done in G5c; P2WB1 is now a black box with each
   pole joined to its outlet.
5. ~~**Contact tables in dense columns**~~ — done with the label placer: a table that finds no
   room beside its coil goes under the rail, as the original does.
6. ~~**PLC wire numbers from the address**~~ — done in G5c: naming setting `{io}{byte}{bit:02}`.
7. ~~**Vertical labels**~~ — done with the label placer: tags of arrows close together are written
   upwards.
8. ~~**Mirrored symbols**~~ — done in G6: any drawing can be mirrored (M); P2's main switch now
   runs horizontally as on the original.
9. ~~**Dashed PE wires**~~ — done in G6: protective earth conductors (GNYE, PE/PEN, PE points)
   are drawn dashed.

All nine are closed (G5, G6).
