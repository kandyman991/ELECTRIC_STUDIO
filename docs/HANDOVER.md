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
- **Current milestone:** M10, format 2 canonical connectivity: **implemented, awaiting
  independent milestone review** (M10a #37, M10b #38, M10c #40, M10b′ and M10d #41). Not
  accepted; S0 does not start until the review is complete.
  - M7a (PR #33) and M7b (PR #34) merged; corrective PR #35 merged at `e258a59`.
  - M7 formally accepted on 27 Nov 2026 with R5 (controlled Legion wall-clock and peak RSS)
    explicitly deferred by the owner, not passed. All six exact-head CI jobs passed at `90d6dfb`.
  - **Next:** independent review of M10 for formal acceptance (see "M10 plan and state"
    below for the evidence and known limitations).
  - **Outstanding pre-release gate:** qualified D41 Legion benchmark ([#39](https://github.com/kandyman991/ELECTRIC_STUDIO/issues/39)) when access returns;
    retain recorded conditions and cold/warm timing and RSS evidence before first production release.
- **On hold:** PR #32 (component library, draft), reused in M9 after reconciliation (D13).

## How to build and test

```sh
export CARGO_TARGET_DIR=<a shared target dir> CARGO_PROFILE_DEV_DEBUG=0   # faster local builds
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- CI runs fmt, clippy, tests on Ubuntu and Windows (the conveyor acceptance tests included), the
  benchmark job (smoke suite, counts against the baseline, instruction and allocation counts), a Windows build,
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

M7 accepted with the owner-approved R5 exception. R1–R4 validated: multi-level terminal report identity and completeness, strict benchmark comparison, prepared-fixture disk-cold open, allocation counting with DHAT, and controlled benchmark provenance. [Validation evidence and allocation reference](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7226987361); [owner decision](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/35#issuecomment-7228169838). The qualified Legion run has not been performed and is mandatory before first production release. Track check/render scaling as a dedicated profiling follow-up, not a sandbox-timing acceptance gate.

## M10 plan and state

M10 (format 2: canonical connectivity) is implemented in four slices, each merged when CI is
green; independent review comes at M10 acceptance (docs/ROADMAP.md).

| Slice | Scope | State |
| --- | --- | --- |
| M10a (PR #37, merged) | Format 2 groundwork: `FORMAT = 2`, format 1 read and converted on load, first-save note (`ecad_project::upgrade_note`); products by canonical id (`ProductId`, `products.json`, `PartRef.product`, two makers' same number coexist; BOM one line per product); sparse page sort keys (`PAGE_STEP`, `sparse_keys`, `SetPageKeys` for undo); the M7 conveyor committed as a format 1 fixture (`tests/acceptance/conveyor/format1`, written by `ecad new-sample` at e258a59) and shown to convert to the same project and drawings | Merged |
| M10b | Terminals as entities: `Project.terminals` (`TerminalEntry`: id, device, group, point, designation) saved in `terminals.json`; `Function.terminals` maps each point to the terminal it shows; `Place` creates missing terminals (id `terminal/<drawing>/<point>` as M7, designation from `Function.designations`, then cleared) or shows existing ones of the same device (repeated drawings, no new endpoint); `Unplace` removes the terminals only it showed and undo restores them exactly; `SetDesignations` renumbers terminals (ids and wires unchanged); `AddTerminal`/`RemoveTerminal` (undrawn terminals); `Reassign` gives the drawing the other device's terminals (rewiring, so named in the editor) with exact undo; `Project::designation(f, point)` replaces `Function::designation` | PR #38, merged |
| M10b′ | Typed PLC channel data: `ChannelData` (kind, address as written, symbol, comment) on the terminal of the channel's first point, in `terminals.json`; `Place` moves the channel keys off the drawing (undo restores them); `SetProperties` routes them to the terminal; `Project::property(f, key)` / `properties_of(f)` read them everywhere | PR #41 |
| M10c | Nodes (D1, D33): `Project.nodes` (`NodeEntry`: id, kind, name, scope, description) saved in `nodes.json`; junctions, potentials and interruptions are no longer devices; a node drawing is a `Function` with `node: Some(id)` and `device: DeviceId::NONE` (omitted on disk); `Place` checks the symbol's kind against the node's, and a junction is drawn once; `AddNode`/`RemoveNode`/`SetNode`/`Rebind` (the explicit join); `Project::node_by_name(kind, name, scope)` refuses ambiguity; `label_of(f)` gives a drawing's tag or node name. Format 1 conversion (`ecad_project::load_with_findings`) gives nodes the ids M7 derived (`NodeId::for_device`, `for_junction`), keeps tags as names (junctions' too, so drawings are identical), and keeps ambiguous mappings apart with a `ConversionFinding` each: potential and interruption sharing a name (`NodeId::interruption_of`), an interruption joining differently named potentials (each drawing `NodeId::split`), a junction drawn twice. The app then saves only on a second save, the CLI only with `--accept-conversion`, MCP not at all. Fixture: `tests/acceptance/ambiguous/` | PR #40, merged |
| M10d | Wires (D1): `wires.json` holds every wire (id, ends as terminal or node, number, make); page files hold only wire graphics (wire id, the drawn points, bend). In memory a drawn wire is a `Connection` (its graphics plus number and make) whose ends are read through the drawings' persistent terminal and node maps; a wire not drawn yet is a `Wire` in `Project.undrawn` (`AddWire`, `RemoveWire`, `DrawWire` keeps id, number and make). Load checks that each drawn wire's graphics show the ends `wires.json` gives (`WireDrawnElsewhere` otherwise). Undrawn wires are in the netlist, the wire list ("not drawn") and the checks (`wire-not-drawn`); terminals and nodes they end on can't be removed. Script and MCP `add_wire`; MCP `list_wires` says `drawn`. Acceptance: `tests/acceptance/tests/wires.rs` | PR #41 |

Notes:
- **Product ids** of format 1 projects and of the built-in samples are UUID v5 of the number
  (`ProductId::for_number`), so converting twice gives the same ids and `PartRef::new(number)`
  still names a sample's product. Products made in the app or imported from CSV get random
  ids; imports match the catalogue by manufacturer and number.
- **Retired files:** a save removes format 1's top-level `parts.json` (`store::RETIRED`).
- **Page keys:** format 1's dense order becomes `PAGE_STEP * (i + 1)`. Samples built in code
  may still use small keys; only the relative order matters.

Known limitations and review points (M10):
- **Node drawings are `Function`s** with `device: DeviceId::NONE` (nil, omitted on disk) and
  `node: Some(id)`, rather than a separate `NodeDrawing` type: routing, rendering, selection
  and editing work unchanged. Code that needs a device checks `f.node` first.
- **A drawn wire's ends are not stored twice in memory:** they are read from its graphics
  through the persistent terminal and node maps, which renumbering, moving and re-routing
  never change; `wires.json` stores them, and load verifies they agree.
- **Wire numbering** (`renumber_all_wires`, per-potential numbering) applies to drawn wires;
  an undrawn wire keeps the number it is given.
- **The editor UI** has no panel for undrawn wires yet (they are added by script or MCP and
  shown by the wire list and checks); schematic generation (M13) draws them.
- **New potentials placed in the editor** are new nodes; joining one to an existing
  potential is the explicit `bind_selected` (renaming to a taken name is refused).
- **Format 1 junctions keep their tag as display name** (section 9: tags become display
  names), so converted drawings are byte-identical; new junctions are named `J<n>`.
- **Performance:** sandbox smoke timings are within noise except `open`, about +20% on the
  reference project from the load-time check that wire graphics show their saved ends (one
  connectivity derivation). Instruction counts are compared in CI.
