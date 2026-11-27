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
  - M7a (PR #33): derived connectivity, netlist, checks, typed PLC data, content hash. Its four
    review findings are corrected; merged once CI is green.
  - M7b: the conveyor acceptance project (schematic-first) and the benchmark harness. In
    progress.
- **On hold:** PR #32 (component library, draft), reused in M9 after reconciliation (D13).

## How to build and test

```sh
export CARGO_TARGET_DIR=<a shared target dir> CARGO_PROFILE_DEV_DEBUG=0   # faster local builds
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- CI runs fmt, clippy, tests on Ubuntu and Windows, a Windows build, and the MCP server against
  the official client.
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
