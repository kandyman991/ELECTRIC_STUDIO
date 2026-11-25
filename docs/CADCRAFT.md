# CADCraft dependency

Electric Studio uses [CADCraft](https://github.com/storytold/cadcraft)'s crates as libraries.
CADCraft is MIT OR Apache-2.0; the ArtCraft name and logos are trademarks that the licence does
not cover, so we never use them.

## Current pin

| Item | Value |
| --- | --- |
| Commit | `0d4b100209990e485891d85c70ff465c8e709156` (24 Nov 2026, version 0.21.0) |
| Crates used | `cadcraft-doc`, `cadcraft-io` (which bring in `geom`, `render`) |
| Minimum Rust | 1.95 (CADCraft's requirement) |

## Rules

- Only `ecad-view` depends on CADCraft (CI checks `ecad-model` and `ecad-reports`).
- The root `Cargo.toml` repeats CADCraft's `[patch.crates-io]` block, because Cargo ignores
  patches declared inside dependencies. It must match the pinned commit's root `Cargo.toml`.
- No fork by default. Fixes go upstream as pull requests.

## Upgrading

1. Pick the new commit; read CADCraft's changes since the current pin.
2. Update `rev` for every `cadcraft-*` dependency in the root `Cargo.toml`.
3. Copy the new commit's `[patch.crates-io]` entries for `krilla` and `hayro-*` (skip the egui
   and winit ones unless we use `ui-egui`).
4. `cargo update -p cadcraft-doc -p cadcraft-io`, then `cargo test --workspace`.
5. If golden tests fail, inspect the output. Accept intended changes with
   `ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-view --test golden` and explain them in the PR.
6. Update the table above.

## What the M0 feasibility test showed

- Both crates build as external git dependencies (about 9.5 minutes cold, seconds after).
- Drawings, blocks, inserts with attributes, lines and text map directly onto schematic needs.
- Device tags stored as block attributes survive a DXF write and read.
- The data structures aren't stable yet (0.x), hence the adapter crate and golden files.
