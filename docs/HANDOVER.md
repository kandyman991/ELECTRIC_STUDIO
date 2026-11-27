# Development handover

The current state of development, for whoever picks it up next. It is kept up to date with every
merged PR.

## Where things stand

- **Architecture:** approved and merged (PR #31). All decisions D1–D43 are resolved:
  - docs/PLATFORM-ARCHITECTURE.md holds the architecture and its decision record;
  - docs/ROADMAP.md holds the milestones and acceptance criteria.
- **Review policy:** milestone-based. Implementation slices are merged once CI is green. Independent
  review happens when a milestone is ready for formal acceptance, or when a significant
  architectural issue needs a decision.
- **Current milestone:** M7, model foundations.
  - M7a (PR #33, merged): derived connectivity, netlist, checks, typed PLC data, content hash.
  - M7b (PR #34): the conveyor acceptance project (schematic-first), its acceptance tests and
    the benchmark harness.
  - **Next:** close the independent review findings on PR #35, verify exact-head CI, and complete the first controlled benchmark
    run on the Legion (`bench/run-controlled.sh`), which records the wall-clock baseline. Then M10.
- **On hold:** PR #32 (component library, draft), reused in M9 after reconciliation (D13).

## How to build and test

```sh
export CARGO_TARGET_DIR=<a shared target dir> CARGO_PROFILE_DEV_DEBUG=0   # faster local builds
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- CI runs fmt, clippy, tests on Ubuntu and Windows (the conveyor acceptance tests included), the
  benchmark job (smoke suite, counts against the baseline, instruction counts), a Windows build,
  and the MCP server against the official client.
- Expected results: `ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-acceptance --test conveyor` (and
  `-p ecad-view --test golden`); benchmark baselines as in bench/README.md. Any change to the
  conveyor or a rule refreshes them in the same PR, with the reason.
- `ecad-bench` is slow in debug: build it with `--release` (about 10 minutes cold on 2 cores).
- CI's toolchain is newer than some local ones, so newer clippy lints can appear there first.

## Conventions

- Every change goes through a `Command` (`Project::apply`), and editing behaviour lives in
  `ecad-editor` (AGENTS.md).
- One branch and one PR per milestone slice. Commits end with the Co-Authored-By and
  Claude-Session lines.
- No real customer or company names in the repository.
- The owner's SPAC project and the LibreDWG corpus are used locally only.

## M7 notes

- **Derived ids:** terminal ids are UUID v5 of `terminal/<function uuid>/<point>`; node ids are
  `node/<device uuid>` (potential or interruption) or `node/junction/<function uuid>`; group ids
  are `group/drawing/<function uuid>` (numbered drawings) or `group/terminal/<terminal uuid>`.
  M10 persists these, so they must not change.
- **Ambiguities** are reported, never merged. M10's conversion has to reconcile them explicitly:
  - shared designations;
  - mixed potential and interruption roles;
  - junctions drawn twice.
- **The oracle:** `Project::potentials_legacy` (test-only) is the pre-M7a potential computation.
  The netlist must keep matching it on every sample and the reference project.
- **The conveyor** (`crates/ecad-model/src/conveyor.rs`) is built with the shared `Builder`
  (`builder.rs`, also used by the reference project) and has ids from `0xC0_0000`; its ids,
  tags and drawing are pinned by the acceptance tests, so a change there is deliberate. It
  checks with 0 errors and 0 warnings; keep it that way (CI runs `ecad check --strict` on it).
- **Benchmarks:** the synthetic project replicates the conveyor through commands (tags
  `=M<k>`, pages `<k>.<page>`, PLC addresses moved up two bytes per copy). First observation:
  at the full size (2 000 devices, 240 pages) `check` (~8 s) and `render` (~16 s) grow faster
  than linearly; candidates for work once the Legion baseline exists.

## M7 acceptance closure (PR #35)

Formal acceptance remains pending. Corrective work covers multi-level terminal report identity and completeness, strict benchmark comparison, prepared-fixture disk-cold open, allocation counting with DHAT, and controlled benchmark provenance. The first corrected-head CI allocation artifact and qualified Legion baseline must be retained. No M10 work until M7 is accepted. Track check/render scaling as a dedicated profiling follow-up, not a sandbox-timing acceptance gate.
