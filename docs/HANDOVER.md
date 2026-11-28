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
- **Current milestone:** AI0, engineering interface. The S0 acceptance decision below takes
  effect when the corrective PR passes all required checks on its final head and merges.
  - M10 format 2 canonical connectivity (M10a #37, M10b #38, M10c #40, M10b′ and M10d #41)
    formally accepted on 28 Nov 2026; corrective PR #42 merged at `6e223ec`.
    R1–R8 and R7.1/R7.2/R8.1 closed; [final acceptance evidence](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/42#issuecomment-7236646425).
  - M7a (PR #33) and M7b (PR #34) merged; corrective PR #35 merged at `e258a59`.
  - M7 formally accepted on 27 Nov 2026 with R5 (controlled Legion wall-clock and peak RSS)
    explicitly deferred by the owner, not passed. All six exact-head CI jobs passed at `90d6dfb`.
  - **AI0:** implemented in PR #48 (merged at `fea0ae4`), ready for independent milestone review; see "AI0 plan and state". Not accepted; the next milestone is not authorized.
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

## AI0 plan and state

AI0 (engineering interface, D16–D19, D31, D32, D34, D42, D43). The pilot came first: S0 device
renumbering on the conveyor through the official MCP client, approved in a desktop session.
It was proven end to end locally before the remaining operations moved to the registry. Pilot
and contract were merged as one slice once all of its exact-head checks passed.

| Slice | Scope | Tested head | CI (6 jobs) | Merge |
| --- | --- | --- | --- | --- |
| AI0a ([#48](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/48)) | Registry, change sets, authority, committed state, records, release validation, progress and cancellation, pilot, adversarial tests, docs | `3bb80dc` | fmt+clippy, test ubuntu, test windows, benchmarks, MCP official client (with the pilot step), build: all success | `fea0ae4` |
| AI0b ([#49](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/49)) | Evidence and limitations (docs only) | its head | docs-only checks | — |

Acceptance criteria:

| Criterion | Evidence |
| --- | --- |
| **Pilot:** an official-client proposal of S0 device renumbering is approved in the desktop app, shown with old/new tags, applied as one transaction; one undo restores | `crates/ecad-mcp/tests/client/check.py` (MCP Python SDK 2.3.0): prepares `renumber_devices`, previews and lists it, and asserts no engineering file changed. The CI step `pilot.rs` `the_official_client_s_proposal_is_approved_in_the_desktop_session` then reviews it in a `Session` (affected devices with old/new tags, e.g. -K7→-K2 and -S0→-S1, safety-direct -S1), approves and applies it (one commit, approval consumed, record by the assistant and the person), and checks that one undo gives the original project. The in-process test runs the same on every test run |
| Pilot through the CLI under its authority rules | `crates/ecad-cli/tests/authority.rs`: `ecad renumber --apply` is refused (global). Then `ecad prepare` → the app's *Approve for the CLI* → `ecad apply <id> --expect <revision>` applies it, and a replay is refused. Automation scope and read-only classes are also covered |
| One typed registry for editor, CLI and MCP; MCP tools generated from it; inventory of entry points including scripts | `ecad_editor::registry` (`DEFS`, `schema`, `lower`). MCP `tools.rs` generates the query and `prepare_*` tools from it; the CLI's `run`, `prepare` and `operations` use it. Inventory: docs/ARCHITECTURE.md "Entry points" |
| Consequential edits through the shared policy; actor identity assigned by trusted host code | `change::prepare`, `apply_approved` and `automate`, and the session's review. `Actor::person()`, `Actor::cli(args)`, the MCP assistant (from `initialize`), `External`. A proposal from an assistant can't carry a person actor; `pilot.rs` and `engineering.rs` check the recorded actors |
| Change sets: prepare, impact preview (objects, pages, checks, direct and indirect safety) | `change::preview` / `compare`. Tests: `engineering.rs` `a_multi_operation_change_set…` (pages, numbering), `pilot.rs` (old/new tags, safety direct and indirect), `ecad-editor` unit tests |
| App-only approval bound to hash, base, committed revision and scope; transactional apply | `session/review.rs` `try_apply`. `engineering.rs`: `stale_and_modified_proposals_are_refused` and `cli_approvals_are_bound_to_hash_revision_scope_and_used_once` (an assistant's proposal is app only, a narrower scope is refused, confirmations are required) |
| Engineering content distinct from committed revision, also when content returns to an earlier value | `Revision{project, generation, content}`. `stale_and_modified_proposals_are_refused` restores the earlier content: content equal, revision different, proposal stale |
| MCP `--write` removed; AI/MCP only query, prepare and preview | `main.rs` exits 2 on `--write` (`check.py` asserts it); the old edit names return an error; `tests/mcp.rs` |
| CLI classes; `ecad apply --expect`; approval consumed atomically; actor, scope and result persisted | `authority.rs`. `engineering.rs` concurrent apply: four threads, exactly one applies, the approval is Consumed, there is one ChangeSet record (actor, scope, base, result, impact), and losers change nothing. `ecad-project` `an_interrupted_commit_with_workflow_files_is_one_generation_or_the_other` |
| Last committed contents retained; manifest committed with the files, verified at open, preview, apply and save | `ecad_project::state` (`commit`, `verify`, `committed_files`). `engineering.rs` `outside_changes_between_preview_and_apply…` (session apply, save and CLI apply refused) and `missing_damaged_unexpected_and_unsafe_files` |
| Path safety; reconciliation of missing, damaged, unexpected and outside-changed files | `state.rs` `unsafe_paths_are_refused`. `engineering.rs`: restore quarantines and deletes nothing; accept records External and revokes approvals; damaged objects make the comparison incomplete and need an acknowledgement; unexpected files are ignored and kept |
| Compatibility path for existing projects | `engineering.rs` `a_project_saved_before_ai0_opens_and_is_adopted`: an MCP server and the session adopt the folder as generation 1; engineering files byte for byte, a format 1 folder stays format 1. `migration.rs` still passes |
| Persistent change records | `changes/<at>-<id>.json` for ChangeSet, Edits (with undo/redo steps), Automation, Reconciliation, Rejection, Refusal and Approval; `rejection_and_the_app_s_own_edits_are_recorded` |
| Shared release validation | `ecad_rules::release::validate` (check errors, unresolved products, safety classification), used by `ecad check/export --release`, MCP `release_check` and the app; `release_validation_is_one_policy_for_every_path` |
| Progress and cancellation for long operations | `ecad_editor::task`; cancellable `verify` and `compare`; the session's Verify task with Cancel. `long_operations_report_progress_and_cancel…` (nothing changed) |
| Validation through desktop session, CLI and official MCP client; interrupted saves | All of the above. Interrupted saves: `ecad-project` failure injection at every store and object step, plus `an_interrupted_apply_leaves_the_folder_consistent` (locked folder, staged leftovers) |
| S0 retrofit regressions retained | `tests/acceptance/tests/retrofit.rs` and `numbering.rs` unchanged in substance (`.ecad/` excluded when counting changed files) and passing |

Known limitations and review points (AI0):

- **Trust boundary (D31).** Enforcement is at application level. A program with folder access
  can write anything; that is detected at the next open, preview, apply or save. Actors are
  recorded as asserted: MCP's client name is a claim, and there is no enterprise
  authentication. Agents that control the desktop app itself are outside the boundary.
- **Indirect safety impact** covers devices on *unnamed* potentials with a safety-related
  terminal, and cables carrying one. Named supply potentials (L+, M) are excluded: otherwise
  every device would be indirectly safety-related. Finer rules need M8's section safety
  marking.
- **Records for some refusals.** A refusal because the folder changed outside Electric Studio
  can't be committed into that folder. It is reported, and the reconciliation that follows is
  recorded. Refusals for lock contention aren't recorded either, because nothing was read or
  changed.
- **Concurrency.** The lock is not blocking: a second concurrent apply gets "another program
  is saving" and can retry.
- **Approvals for the CLI** are recorded when issued; the app's own approvals when used (one
  transaction with the change).
- **Retained objects** hold the last commit only. History is S1's baselines.
- **Performance.** cachegrind, conveyor, against the S0 baseline:
  - save: 13.0M → 29.5M instructions (+127%). One SHA-256 verification pass over the folder
    per save, plus writing the manifest and objects. Mitigated: the content digest comes from
    the rendered files, byte-identical files keep their manifest hash, and objects are checked
    when read.
  - open: +9% (the M10/S0 figures, unchanged by AI0); other workloads within ±4%.
  - allocation blocks: save 5 226 → 9 088 (+74%: manifest, hashes, reading files to compare), others unchanged.
  - Smoke content hashes changed because the model now carries the `safety_related` and
    `automation` fields (and the conveyor marks -S0 and -K3).
  - Wall clock in the sandbox is noise; qualified timing remains #39.
- **Imports are not offered to assistants.** They run in the app or as CLI automation.

## S0 plan and state

S0 (stable numbering, D22–D25) is implemented in three slices, each merged when CI is green;
independent review comes at S0 acceptance.

| Slice | Scope | State |
| --- | --- | --- |
| S0a (PR #44) | `ecad_model::numbering`: identifiers, policies (sequential default, lowest free), `Reserve`/`Unreserve`, `Retire`/`Reclaim`, `SetNumberingPolicies`, `SetCapacity`; explicit assignments of reserved or retired identifiers refused at every assignment point; `Project::removal` retires what a removal frees, in the same transaction, and the editor's deletions use it; resource-aware allocation of tags, wires, strip terminals, PLC channels and addresses, cable cores, page names; `insert_page` with a key between neighbours; `numbering.json` | Merged |
| S0b (PR #45) | Joining keeps both numbers (`wire-numbers-differ`); `ecad_model::renumber` (preview, stale check, one undo step) for wires, devices, a strip's terminals, pages, in the app (preview dialog), the CLI (`ecad renumber … [--apply]`) and MCP (read-only `preview_renumbering`); CLI `numbering`, `reserve`, `unreserve`, `retire`, `reclaim` with identifier ranges; MCP `numbering`; `ReplaceDevice` | Merged |
| S0c (PR #46) | Retrofit acceptance (`tests/acceptance/tests/retrofit.rs`), adversarial tests, benchmarks, documentation | Merged |

Acceptance criteria (ROADMAP S0 row and the task's criteria):

| Criterion | Evidence |
| --- | --- |
| Adding a device, a wire, a terminal and a page changes only those | `retrofit.rs` `assert_unaffected` by id, after the retrofit, save/load and undo/redo; `numbering.rs` `a_page_inserted_after_3_is_3a_and_no_other_page_changes` (only `project.json` and the new page file change on save) |
| Joining two numbered potentials keeps both numbers and reports `wire-numbers-differ` | `retrofit.rs` per-potential run; `naming.rs` `tags_follow_the_format_and_potentials_share_one_number`; conflict detection kept: `joining_named_potentials_is_still_reported_as_a_clash` |
| Removing `-B2` changes nothing else and retires it; the next sensor is `-B3` | `numbering.rs` `removing_b2_retires_it_and_the_next_sensor_is_b3` (also under lowest free; reuse only after `Reclaim`) |
| A new DI for an unrelated device skips the reserved four | `retrofit.rs` (I1.1–I1.4 reserved, the new input is I1.5), `numbering.rs` `a_new_di_skips_the_reserved_four…` through the editor path |
| Allocating past the last free channel reports exhaustion and adds nothing | `numbering.rs` `allocating_past_the_last_free_channel…` (editor path, project unchanged), cores `cable_cores_are_allocated_from_the_cables_size` |
| A page inserted after 3 is `3a`, and no other page file changes | as above; `3b` after `3a`; dotted `3.1`; appended pages continue the numbers |
| Explicit renumbering with preview and confirmation; ids preserved | `numbering.rs` `renumbering_is_previewed_confirmed_and_keeps_every_identity` (stale preview refused, exact undo), session test, CLI and MCP tests |
| Persistence, exact undo/redo, rollback, conflicting records refused | `numbering_json_round_trips_and_conflicting_records_are_refused`, `numbering_commands_undo_and_redo_exactly`, `conflicting_allocations_and_interrupted_transactions_change_nothing` |
| Format 1 migration and format 2 compatibility | conversion test asserts equal pages and empty numbering; format 2 projects without `numbering.json` load with the defaults |

### Independent S0 acceptance decision

Reviewed the cumulative changes from M10 through S0c against D22–D25 and the matrix above.
The corrective PR covers removal of owners with scoped numbering, PLC slot bounds,
whole-family page renumbering, staging-name conflicts, strip-only renumbering, derived wire
numbers blocked by reservations, and counter and address-range boundaries. Its regression
tests extend the conveyor acceptance suite. The PR discussion records the verified head and
hosted job results. On verified merge, S0 is formally accepted and AI0 development is authorized.
The qualified Legion benchmark remains a pre-release gate under issue #39.

Known limitations and review points (S0):
- **Retirement is by removal operation**, not by the low-level `RemoveDevice`/`Disconnect`
  commands (which are also the inverses of additions, and must not retire). Every removal in
  the editor, CLI script and MCP goes through `Project::removal`; a custom caller of raw
  commands must do the same.
- **Bounded resources are freed, not retired,** on removal (section 12.6): PLC channels,
  strip terminal numbers, cores. They can be retired or reserved explicitly.
- **Capacities** (PLC card channels, strip terminals) are numbering data until M9 gives
  products their own; without one, channels count from 0 and strips sequentially.
- **Baselines** don't exist yet (S1): retirement records have a note, not the change record and
  last baseline; reuse after release needs approval once S1 adds releases.
- **Node names** (junctions `J<n>`, potential points) keep lowest-free naming, skipping
  reserved and retired tags; they are names, not engineering identifiers.
- **Renumbering** covers wires, device tags `<prefix><n>`, a strip's drawn terminal groups
  (undrawn groups keep their numbers) and numeric pages (suffixed pages follow their page).
  PLC addresses are not renumbered globally.
- **Replacement** takes explicit terminal renames by id; proposing a mapping from the new
  product's terminal pattern needs M9's product model.
- **Performance** (cachegrind, conveyor, sandbox, against main's baseline; M10's accepted head
  in brackets): open +9.0% (+8.0%), save +2.4% (+1.1%), hash +1.0% (+0.6%), edit and check
  −4.0% (−4.1%), netlist, render and pdf +0.2%. S0's own share: open about +1%
  (`numbering.json` read through its commands, availability lookups in every assignment
  check, O(log n) each), save +1.3% (`numbering.json` written), hash +0.4% (sparse page keys
  in the serialized pages). Allocation blocks: open 13 179 (+2.6%), save 5 226 (+2.3%), the
  rest unchanged. Workload counts match; content hashes changed only by the builders' sparse
  page keys. Allocation helpers scan linearly and run per edit, never per load. Sandbox wall
  clock is noise (±50% between runs); qualified timing remains #39.
- **MCP** stays read-only for numbering (D32): it lists numbering and previews renumbering;
  people apply renumbering in the app or with `ecad renumber --apply`.

## M10 plan and state

M10 (format 2: canonical connectivity) is implemented and accepted. All four slices and
corrective PR #42 are merged.

| Slice | Scope | State |
| --- | --- | --- |
| M10a (PR #37, merged) | Format 2 groundwork: `FORMAT = 2`, format 1 read and converted on load, first-save note (`ecad_project::upgrade_note`); products by canonical id (`ProductId`, `products.json`, `PartRef.product`, two makers' same number coexist; BOM one line per product); sparse page sort keys (`PAGE_STEP`, `sparse_keys`, `SetPageKeys` for undo); the M7 conveyor committed as a format 1 fixture (`tests/acceptance/conveyor/format1`, written by `ecad new-sample` at e258a59) and shown to convert to the same project and drawings | Merged |
| M10b | Terminals as entities: `Project.terminals` (`TerminalEntry`: id, device, group, point, designation) saved in `terminals.json`; `Function.terminals` maps each point to the terminal it shows; `Place` creates missing terminals (id `terminal/<drawing>/<point>` as M7, designation from `Function.designations`, then cleared) or shows existing ones of the same device (repeated drawings, no new endpoint); `Unplace` removes the terminals only it showed and undo restores them exactly; `SetDesignations` renumbers terminals (ids and wires unchanged); `AddTerminal`/`RemoveTerminal` (undrawn terminals); `Reassign` gives the drawing the other device's terminals (rewiring, so named in the editor) with exact undo; `Project::designation(f, point)` replaces `Function::designation` | PR #38, merged |
| M10b′ | Typed PLC channel data: `ChannelData` (kind, address as written, symbol, comment) on the terminal of the channel's first point, in `terminals.json`; `Place` moves the channel keys off the drawing (undo restores them); `SetProperties` routes them to the terminal; `Project::property(f, key)` / `properties_of(f)` read them everywhere | PR #41, merged |
| M10c | Nodes (D1, D33): `Project.nodes` (`NodeEntry`: id, kind, name, scope, description) saved in `nodes.json`; junctions, potentials and interruptions are no longer devices; a node drawing is a `Function` with `node: Some(id)` and `device: DeviceId::NONE` (omitted on disk); `Place` checks the symbol's kind against the node's, and a junction is drawn once; `AddNode`/`RemoveNode`/`SetNode`/`Rebind` (the explicit join); `Project::node_by_name(kind, name, scope)` refuses ambiguity; `label_of(f)` gives a drawing's tag or node name. Format 1 conversion (`ecad_project::load_with_findings`) gives nodes the ids M7 derived (`NodeId::for_device`, `for_junction`), keeps tags as names (junctions' too, so drawings are identical), and keeps ambiguous mappings apart with a `ConversionFinding` each: potential and interruption sharing a name (`NodeId::interruption_of`), an interruption joining differently named potentials (each drawing `NodeId::split`), a junction drawn twice. The app then saves only on a second save, the CLI only with `--accept-conversion`, MCP not at all. Fixture: `tests/acceptance/ambiguous/` | PR #40, merged |
| M10d | Wires (D1): `wires.json` holds every wire (id, ends as terminal or node, number, make); page files hold only wire graphics (wire id, the drawn points, bend). In memory a drawn wire is a `Connection` (its graphics plus number and make) whose ends are read through the drawings' persistent terminal and node maps; a wire not drawn yet is a `Wire` in `Project.undrawn` (`AddWire`, `RemoveWire`, `DrawWire` keeps id, number and make). Load checks that each drawn wire's graphics show the ends `wires.json` gives (`WireDrawnElsewhere` otherwise). Undrawn wires are in the netlist, the wire list ("not drawn") and the checks (`wire-not-drawn`); terminals and nodes they end on can't be removed. Script and MCP `add_wire`; MCP `list_wires` says `drawn`. Acceptance: `tests/acceptance/tests/wires.rs` | PR #41, merged |

Notes:
- **Product ids** of format 1 projects and of the built-in samples are UUID v5 of the number
  (`ProductId::for_number`), so converting twice gives the same ids and `PartRef::new(number)`
  still names a sample's product. Products made in the app or imported from CSV get random
  ids; imports match the catalogue by manufacturer and number.
- **Retired files:** a save removes format 1's top-level `parts.json` (`store::RETIRED`).
- **Page keys:** format 1's dense order becomes `PAGE_STEP * (i + 1)`. Samples built in code
  may still use small keys; only the relative order matters.

M10 acceptance corrections (PR #42, findings in
[the review](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/42#issuecomment-7234132366)):

| Finding | Resolution | Evidence |
|---|---|---|
| R1 canonical record integrity | Format 2 requires `terminals.json`, `nodes.json`, `wires.json`, `bridges.json`, `products.json`; duplicate ids, missing product ids, foreign page ownership, mixed legacy/canonical wire graphics and out-of-order bridge ends are refused; save refuses unresolved wire ends (`BadWireEnds`) | `wires.rs` `format_2_refuses_missing_or_duplicate_canonical_records`, `bridges.rs` `bridges_json_is_required_and_validated_on_load` |
| R2 exact terminal restoration | `RestoreDrawing` inverse snapshots terminal entries and the bridges touching them; undo, redo and failed transactions restore them exactly | `terminal_records_survive_unplace_and_undo_without_reconstruction`, `a_failed_batch_rolls_back_terminals_and_bridges_exactly` |
| R3 undrawn wire resources | Undrawn wires reserve cable cores and numbers (`check_core`, `SetWireSpec`, `RemoveCable`) | `undrawn_wire_reserves_cable_core_and_number` |
| R4 checks and reports cover undrawn wires | Wire numbering, potential clashes and the cable list include them | `undrawn_wire_short_is_reported_as_an_electrical_error` |
| R5 canonical PLC identity | `check_plc` by terminal identity, `plc_channels_of` deduplicated, `next_address` reserves every terminal's channel, drawn or not | `undrawn_plc_channels_and_strip_terminals_stay_typed_and_reported` |
| R6 physical group consistency | `check_terminal_point` (point unique per group), whole-group renumbering including undrawn points, `RemoveDevice` refuses remaining terminals | `renumber_includes_undrawn_points_of_the_same_physical_group` |
| R7 canonical bridges | `Project.bridges` in `bridges.json`; drawings only depict; repeated representations cannot add; drawing removal keeps surviving bridges; `AddBridge`/`RemoveBridge` explicit; format 1 conversion yields deterministic bridges equal to the built project; the temporary `BridgeInUse` safeguard is removed | `bridges.rs`, `deleting_a_bridge_drawing_keeps_the_bridges_of_surviving_undrawn_terminals`, `the_format_1_conveyor_converts_to_the_same_project` |
| R8 remaining coverage | Undrawn terminals typed from their record (PLC channel, strip point); PLC I/O list and terminal diagram list undrawn channels and strip terminals as "not drawn" with what their wires reach; graphical-only edits never change drawn wire ends, `Rebind`/`Reassign` do | `bridges.rs` `graphical_operations_never_change_drawn_wire_ends`, `reassigning_a_drawing_creates_the_new_devices_bridges_and_keeps_the_old_ones` |

### Final corrective acceptance (28 Nov 2026)

Exact head `05f838f4f846dc616e32b0dfba4f81cbddc62143`: all six jobs passed
([CI](https://github.com/kandyman991/ELECTRIC_STUDIO/actions/runs/46125168129),
[Windows build](https://github.com/kandyman991/ELECTRIC_STUDIO/actions/runs/46125168093)).
Linux and Windows each passed 12 bridge, 19 conveyor, 5 migration and 10 wire acceptance
tests, including depicted bridge removal refusal, intermediate endpoint retention, exact
history, nonadjacent bridge components, repeated/partial representations and undrawn
wire reporting. Original format 1 migration and golden drawings/reports passed.
Benchmark counts matched and instruction/allocation measurements were retained in the
[acceptance record](https://github.com/kandyman991/ELECTRIC_STUDIO/pull/42#issuecomment-7236646425).
The two instruction warnings were decreases (edit/check); the existing manual underlay
measurement remains ignored. Qualified Legion validation is NOT RUN (#39).
M10 accepted; S0 authorized. No release authorization implied.

Known limitations and review points (M10):
- **Bridges are canonical** and only change through `AddBridge`/`RemoveBridge`, through placing
  a drawing that creates the terminals it bridges, or with the terminals they join. A terminal
  can't be removed while bridged. There is no editor panel for bridges yet (script/model only).
- **Saving refuses a project with unresolved drawn wire ends** (R1), rather than writing
  something load would reject.
- **Undrawn report rows** identify canonical physical points and bridge components, with
  `not drawn` in the location column for points lacking representations. When no complete
  symbol representation supplies inside/outside order, the two legacy columns are ordered
  point slots, not inferred cabinet/field sides. Drawn and undrawn wires are aggregated by
  terminal identity across repeated representations.
- **Bridge mutation consistency:** removing a depicted bridge is refused until its
  representation is reconciled; save validates the same invariant as load. Detaching a
  drawing retains any bridge component touching a surviving terminal, including intermediate
  points without direct wires. Only wholly unused components are discarded.
- Format 2 was not released before M10 acceptance; interim development snapshots without
  `bridges.json` are refused, not silently reconstructed. Preserve the format 1 source for
  deterministic migration.
- **Node drawings are `Function`s** with `device: DeviceId::NONE` (nil, omitted on disk) and
  `node: Some(id)`, rather than a separate `NodeDrawing` type: routing, rendering, selection
  and editing work unchanged. Code that needs a device checks `f.node` first.
- **A drawn wire's ends are not stored twice in memory:** they are read from its graphics
  through the persistent terminal and node maps, which renumbering, moving and re-routing
  never change; `wires.json` stores them, and load verifies they agree.
- **Wire numbering** (`renumber_all_wires`, per-potential numbering) includes drawn and
  undrawn wires; ordinary graphical edits do not renumber either.
- **The editor UI** has no panel for undrawn wires yet (they are added by script or MCP and
  shown by the wire list and checks); schematic generation (M13) draws them.
- **New potentials placed in the editor** are new nodes; joining one to an existing
  potential is the explicit `bind_selected` (renaming to a taken name is refused).
- **Format 1 junctions keep their tag as display name** (section 9: tags become display
  names), so converted drawings are byte-identical; new junctions are named `J<n>`.
- **Performance:** sandbox smoke timings are within noise except `open`, about +20% on the
  reference project from the load-time check that wire graphics show their saved ends (one
  connectivity derivation). Instruction counts are compared in CI. PR #42 vs main's
  baseline (cachegrind, conveyor; 435fcb7 measured separately to attribute): open +8.0%
  (+7.4% from the R1–R6 corrections, +0.6% from R7's `bridges.json`), save +1.1% (+0.8% R7),
  edit and check −4.1% (R1–R6), hash +0.6% (R7, bridges are serialized), others within
  ±0.2%; workload counts identical, content hashes changed (bridges are part of the
  project). The open increase is, by estimate, mostly `check_terminal_point`, which scans every terminal
  for each terminal added: linear per edit, **quadratic on load** (about 64 k comparisons, ~1 M
  instructions, on the conveyor's 358 terminals). It stays within the 10% warning threshold; replacing it with
  an indexed check belongs with the profiling follow-up below, before projects of thousands
  of terminals. Allocation blocks moved by under 1%. Wall clock remains unqualified until
  the Legion run (#39).
