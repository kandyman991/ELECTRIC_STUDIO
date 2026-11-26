# Working on Electric Studio

Rules for every contributor, human or AI. Read docs/ARCHITECTURE.md first.

## Architecture rules

1. **The model is the source of truth.** `ecad-model` holds the project. Drawings, reports and
   exports are derived from it and regenerated; never store electrical meaning only in a drawing.
2. **Connections join connection points, never coordinates.** Wire geometry is always computed
   by `Project::route`.
3. **Only `ecad-view` depends on CADCraft.** `ecad-model` and `ecad-reports` must not, and CI
   checks this. `ecad-view`'s public API takes model types and returns bytes or model types.
4. **CADCraft is pinned to one commit.** Upgrading it is its own PR: bump the `rev` and the
   copied `[patch.crates-io]` block together, run the golden tests, and review every changed
   golden file. See docs/CADCRAFT.md.
5. **Every edit is a `Command`** applied through `Project::apply` (or `History::execute` to
   make it undoable). Don't mutate model collections from UI or I/O code.
6. **Editing behaviour lives in `ecad-editor`, not in the UI (`apps/ecad-app`, egui).** A shell only converts
   input into `UiEvent`s for `ecad_editor::Session` and paints the `Frame` it returns.
   Anything the UI decides on its own can't be tested and won't be available to the CLI or
   the future MCP server.

## Code rules

- No `unwrap`, `expect` or `panic!` outside tests (enforced by clippy). Return errors.
- No `unsafe`.
- `cargo fmt`, `cargo clippy --workspace --all-targets` with no warnings, `cargo test --workspace`.
- Golden files change only on purpose: `ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-view --test golden`,
  and say why in the PR.

## Workflow

- Work is organised in milestones M0 to M6 (see README). One branch and one PR per milestone
  or sub-milestone, e.g. `m1-editor-canvas`.
- PRs are reviewed by the owner. Nobody merges their own PR unless the owner says so.
- No licence has been chosen yet; don't add one without the owner's approval.
