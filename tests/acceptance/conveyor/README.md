# Conveyor acceptance project (M7)

A belt conveyor with a reversing three-phase motor, a 24 V supply, a safety relay with an
emergency stop, a PLC with digital inputs and outputs, field sensors through a two-level
terminal strip, and a cabinet layout. It is built in code with today's commands, schematic-first
(`crates/ecad-model/src/conveyor.rs`), and written to a folder with:

    ecad new-sample <dir> conveyor

Everything in it is fictitious: the maker is `Example`, part numbers start with `EX-`.

## What the tests prove (`tests/acceptance/tests/conveyor.rs`)

- The checks find no errors and no warnings.
- Every wire end resolves to a terminal or a node, with the same ids on every build and after a
  save and load; the netlist and the content hash survive save and load.
- Physical groups and points are unique; no ambiguity is reported.
- A strip terminal's two sides share one designation in one group, joined by an explicit bridge.
- The two-level terminals keep their levels apart (+24 V and 0 V on different nets).
- Potentials (`L+`, `M`) and interruptions (`Q-FWD`, `SAFE-OK`, ...) drawn on several pages are
  one node each; repeated drawings never add endpoints.
- PLC channels carry their typed, canonical addresses.
- The reports (`expected/*.csv`), the counts and content hash (`expected/summary.json`) and every
  page (`pages/*.dxf`, `pages/*.png`) match the committed results.

## Hostile cases

Built in the test from the conveyor, the ways format 1 can be damaged:

- a contactor coil drawn a second time: kept apart, reported as a shared designation, no net
  changes;
- a symbol change that removes a wired point: a `wire-end-missing` error per end, and the netlist
  report refuses to run;
- a junction drawn twice: reported;
- a non-finite coordinate: refused, the project unchanged.

## Updating the expected results

Only on purpose, and say why in the PR:

    ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-acceptance --test conveyor

Upgrading CADCraft changes `pages/` here as it does `crates/ecad-view/tests/golden/`.
