# Ambiguous format 1 fixture (M10)

`format1/` is M7's `potentials` sample, written by `ecad new-sample <dir> potentials` at the
M7 acceptance commit (e258a59). `make.py` then added three ambiguous mappings:

- **I9**, an interruption device whose page 1 drawing is wired to an L+ point and whose page 2
  drawing to an M point. Format 1 joined L+ and M through it.
- **M** also drawn as an interruption point (a potential and an interruption sharing a name).
- **J1**, a junction device, drawn a second time.

`tests/acceptance/tests/migration.rs` checks that converting it reports each with its pages,
keeps the nodes apart (I9's drawings become two interruptions, M a potential and an
interruption, J1 two junctions), joins nothing and reconnects nothing, and is deterministic.
The app saves it only on a second save; the CLI only with `--accept-conversion`.

To rebuild: `ecad new-sample format1 potentials` with the M7 binary, then
`python3 make.py format1`.
