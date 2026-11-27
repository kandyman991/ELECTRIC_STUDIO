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
- **Times** of every iteration, ns. The first is the cold one (a fresh process); the rest are warm.
- **Peak memory** (Linux): during setup, and during the iterations after the peak is reset
  (`/proc/self/clear_refs`), so it includes the project already in memory.

## In CI (every PR)

The `bench` job builds in release and runs:

1. `ecad-bench suite --size smoke`, then `ecad-bench compare` against `baseline-smoke.json`: any
   difference in a **count fails** the job. Times are printed but not compared (another machine).
2. `ecad-bench instructions instructions-conveyor.json`: each workload's instruction count under
   Valgrind (cachegrind), its own setup subtracted. A move of more than 10% is a **warning**, not
   a failure: a new toolchain moves the counts too.

When a change is meant to change the counts (a new rule, a changed drawing), refresh the
baselines in the same PR and say why:

    cargo build --release -p ecad-bench
    ECAD_BENCH_MACHINE=sandbox target/release/ecad-bench suite --size smoke --out bench/baseline-smoke.json
    target/release/ecad-bench instructions bench/instructions-conveyor.json --update

## The controlled run (the Legion)

Wall clock and memory baselines come only from one machine under recorded conditions:

    bench/run-controlled.sh --label legion --repeat 3 --drop-caches [--underlay <real.dwg>]

It waits for the machine to be idle, records the CPU, kernel, governor, power supply, load,
memory and toolchain before and after, runs the full suite once right after dropping the page
cache (cold) and then `--repeat` times (warm), and writes everything to `bench/results/`.
A run worth keeping becomes `bench/baseline-legion.json`; later runs on the same machine compare
times and memory against it with `ecad-bench compare` (the label and the build profile must
match).

## Recorded

- `baseline-smoke.json`: counts for the smoke size (times from the build sandbox, informative).
- `instructions-conveyor.json`: instructions per workload on the conveyor, rustc 1.97.0.
- A wall-clock baseline is pending the first controlled run on the Legion.

First observations on the build sandbox (2 cores, release), for the full size: `check` takes
about 8 s and `render` about 16 s on 2 000 devices and 240 pages, against 68 ms and 85 ms on
200 devices and 24 pages; both grow faster than linearly with the project and are the first
candidates for work once the controlled baseline exists. `netlist` stays at about 20 ms.
