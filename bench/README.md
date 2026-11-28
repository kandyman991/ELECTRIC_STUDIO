# Benchmarks

`crates/ecad-bench` builds representative projects and runs workloads on them, one process per
workload.

## Projects and workloads

| Project | What it is |
|---|---|
| `conveyor` | The M7 acceptance project (`ecad new-sample <dir> conveyor`): 50 devices, 6 pages. |
| `reference` | The reference project (`ecad new-sample <dir> reference`). |
| `synthetic` | The conveyor replicated as a line of machines, with an underlay behind page 1. `smoke`: 4 copies (200 devices, 24 pages), 20 000 underlay points. `full`: 40 copies (2 000 devices, 240 pages), 400 000 points. `--underlay <file>` uses a real DXF or DWG instead (local only, never committed). |

Workloads: `open` (load from disk), `save`, `edit` (move a drawing, then run the checks, as the
editor does), `check`, `netlist`, `hash`, `render` (every page's primitives), `pdf` (the whole
project).

## What a run reports

- **Counts** that are deterministic on every machine: the project's pages, devices, drawings,
  wires, terminals, nodes, nets, underlay points and content hash, and what the workload
  produced (findings, nets, primitives...).
- **Times** of every iteration, ns. In a normal suite the first is process-first, after fixture construction and setup; the rest are warm. A separate prepared-fixture run measures actual filesystem-cold open after eviction.
- **Peak memory** (Linux): during setup, and during the iterations after the peak is reset
  (`/proc/self/clear_refs`), so it includes the project already in memory.

## In CI (every PR)

The `bench` job builds in release and runs:

1. `ecad-bench suite --size smoke`, then `ecad-bench compare` against `baseline-smoke.json`: any
   difference in a **count fails** the job. Times are printed but not compared (another machine).
2. `ecad-bench instructions instructions-conveyor.json`: each workload's instruction count under
   Valgrind (cachegrind), its own setup subtracted. A move of more than 10% is a **warning**, not
   a failure: a new toolchain moves the counts too.
3. `ecad-bench allocations bench-allocations.json`: DHAT allocation blocks per conveyor workload, setup-subtracted, retained as CI evidence. Establish the initial allocation-count reference from the first successful corrected-head CI; RSS is a separate measurement.

When a change is meant to change the counts (a new rule, a changed drawing), refresh the
baselines in the same PR and say why:

    cargo build --release -p ecad-bench
    ECAD_BENCH_MACHINE=sandbox target/release/ecad-bench suite --size smoke --out bench/baseline-smoke.json
    target/release/ecad-bench instructions bench/instructions-conveyor.json --update

## The controlled run (the Legion)

Wall clock and memory baselines come only from one machine under recorded conditions:

    bench/run-controlled.sh --label legion --repeat 3 --drop-caches [--underlay <real.dwg>]

It requires a clean unchanged commit, builds first, prepares all open fixtures before eviction,
and waits for an idle machine before each measurement. Each disk-cold open follows a successful
cache drop and reads an existing fixture without preloading it. The first sample is disk-cold;
the remaining samples in that file are warm opens. Each process suite reports process-first
(after setup) and warm iterations for all workloads; it is not a disk-cold suite.

Fixtures are prepared on the checkout filesystem, not `/tmp`, and memory-backed filesystems are refused.
Results in `bench/results/`: `conditions.txt`, `filesystem.txt`, `disk-cold-<project>-<n>.json`,
`process-<n>.json`, and `COMPLETE` only after successful completion. The asset's SHA-256,
exact revision, power, toolchain and machine conditions are recorded; no asset is committed.
For acceptance use full size, at least three repeats, cache eviction, a representative
non-confidential real DXF/DWG, mains power and no other work on the machine. A generated
asset is useful for exploration but does not demonstrate the real-asset workload. A forced,
smoke, generated-asset, no-eviction or short run is marked exploratory. Check power/load/conditions throughout
before promoting a result. Compare same-machine process suites with `ecad-bench compare`;
it reports warm median and range, and peak RSS. Retain the disk-cold repetitions separately
and report their first-sample median and spread. Do not mix disk-cold and process-first samples.

## Recorded

- `baseline-smoke.json`: counts for the smoke size (times from the build sandbox, informative).
- `instructions-conveyor.json`: instructions per workload on the conveyor, rustc 1.97.0.
- Controlled Legion wall-clock and peak-RSS baseline: **NOT RUN / DEFERRED** by the owner on 27 Nov 2026. M7 accepted and M10 authorized, but the qualified D41 run remains mandatory before first production release. Tracked in [#39](https://github.com/kandyman991/ELECTRIC_STUDIO/issues/39). Run when access returns; do not treat sandbox or hosted measurements as Legion evidence. [Decision](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7228169838).
- Initial allocation reference: exact head `90d6dfbf4b707aeba99256d727c8da26a8c09f58`, [successful benchmark job](https://github.com/kandyman991/ELECTRIC_STUDIO/actions/runs/46037120317/job/140635343964), [retained evidence](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7226987361). Release conveyor, DHAT setup-subtracted heap allocation blocks (not a hard regression threshold):

| Workload | Allocation blocks |
|---|---:|
| open | 4404 |
| save | 2914 |
| edit | 41730 |
| check | 41728 |
| netlist | 2514 |
| hash | 19617 |
| render | 103514 |
| pdf | 406429 |

First observations on the build sandbox (2 cores, release), for the full size: `check` takes
about 8 s and `render` about 16 s on 2 000 devices and 240 pages, against 68 ms and 85 ms on
200 devices and 24 pages; both grow faster than linearly with the project and are the first
candidates for work once the controlled baseline exists. `netlist` stays at about 20 ms.

Performance follow-up (after the baseline): profile full-project checks and page rendering at multiple sizes, identify repeated whole-project traversals, and target interactive responsiveness before any synchronous full-project checks become part of the editor. Do not change electrical semantics or drawing goldens to optimize.
