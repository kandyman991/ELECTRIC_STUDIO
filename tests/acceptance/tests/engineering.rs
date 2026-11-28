//! AI0 acceptance: the engineering interface's refusals and recovery, through the desktop
//! session (the app's logic), the MCP server and the CLI's library paths, on the conveyor.
//!
//! For every refusal the test checks the project (unchanged), the approval state and the
//! audit record. The S0 retrofit regressions live in `retrofit.rs` and still run.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_editor::change::{self, Actor, ApprovalState, Confirmations, OpCall, RecordKind, Status};
use ecad_editor::session::{Session, UiEvent};
use ecad_project::state::{self, Quiet};
use serde_json::json;
use std::path::Path;

fn conveyor(dir: &Path) {
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    let id = p.device_by_tag("-K2").unwrap().id;
    p.apply(ecad_model::Command::RenameDevice { id, tag: "-K7".into() }).unwrap();
    ecad_project::save(&p, dir).unwrap();
}

fn assistant() -> Actor {
    Actor::Assistant { client: "test-client".into(), client_version: "1".into(), session: "s".into() }
}

fn cli() -> Actor {
    Actor::Cli { user: "ci".into(), host: "h".into(), command: "ecad apply".into(), version: "0".into() }
}

/// Prepare and store a proposal as MCP or `ecad prepare` would.
fn propose(dir: &Path, actor: Actor, ops: Vec<OpCall>) -> change::ChangeSet {
    let project = ecad_project::load(dir).unwrap();
    let cs = change::prepare(&project, state::committed(dir).unwrap(), actor, "", "", ops).unwrap();
    change::save_proposal(dir, &cs).unwrap();
    cs
}

fn op(name: &str, args: serde_json::Value) -> OpCall {
    OpCall { op: name.into(), args }
}

fn status(s: &Session) -> String {
    s.frame(None, [0, 0, 0]).status
}

fn records(dir: &Path, kind: RecordKind) -> usize {
    change::records(dir).unwrap().iter().filter(|r| r.kind == kind).count()
}

#[test]
fn a_multi_operation_change_set_applies_as_one_transaction_and_one_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let original = ecad_project::load(dir).unwrap();
    let cs = propose(
        dir,
        assistant(),
        vec![
            op("add_page", json!({ "after": "3" })),
            op("place_symbol", json!({ "symbol": "iec.coil", "page": "3a", "x": 100, "y": 100 })),
            op("edit_device", json!({ "tag": "-M1", "description": "Conveyor motor" })),
            op("reserve", json!({ "identifiers": "tag:-K90..-K92", "note": "extension" })),
        ],
    );
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: cs.id.clone() });
    let r = s.frame(None, [0, 0, 0]).review.unwrap();
    assert!(r.pages.contains(&"3a".to_owned()), "{:?}", r.pages);
    assert!(r.other.iter().any(|o| o.contains("numbering")));
    assert!(!r.needs_global_confirmation);
    s.handle(UiEvent::ApproveProposal { confirm_safety: r.needs_safety_confirmation, confirm_global: false });
    assert!(status(&s).starts_with("Applied"), "{}", status(&s));
    let p = s.editor().project();
    assert!(p.pages.values().any(|pg| pg.name == "3a"));
    assert_eq!(p.device_by_tag("-M1").unwrap().description, "Conveyor motor");
    // Unrelated installed identifiers and drawings are intact (the S0 guarantee).
    for (id, d) in &original.devices {
        if d.tag != "-M1" {
            assert_eq!(p.devices.get(id), Some(d), "{}", d.tag);
        }
    }
    for (id, f) in &original.functions {
        assert_eq!(p.functions.get(id), Some(f));
    }
    // One undo restores the original engineering state.
    s.handle(UiEvent::Key { key: ecad_editor::KeyName::Undo });
    assert_eq!(s.editor().project(), &original);
    assert_eq!(s.editor().undo_label(), None);
}

#[test]
fn stale_and_modified_proposals_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let stale = propose(dir, assistant(), vec![op("edit_device", json!({ "tag": "-M1", "description": "x" }))]);
    // Someone commits a change: the proposal's base is gone.
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::RefreshProposals);
    let mut p = s.editor().project().clone();
    let k1 = p.device_by_tag("-K1").unwrap().id;
    p.apply(ecad_model::Command::SetDescription { id: k1, description: "changed".into() }).unwrap();
    drop(s);
    ecad_project::save(&p, dir).unwrap();
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: stale.id.clone() });
    assert!(s.frame(None, [0, 0, 0]).review.unwrap().stale.is_some());
    let before = s.editor().project().clone();
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    assert!(status(&s).contains("stale"), "{}", status(&s));
    assert_eq!(s.editor().project(), &before);
    assert!(change::approvals(dir).unwrap().is_empty());
    // Content back to an earlier value is still a later revision: a proposal against the
    // first revision stays stale (the generation moved on).
    let mut back = before.clone();
    back.apply(ecad_model::Command::SetDescription {
        id: k1,
        description: ecad_model::conveyor::conveyor().unwrap().device(k1).unwrap().description.clone(),
    })
    .unwrap();
    ecad_project::save(&back, dir).unwrap();
    assert_eq!(state::committed(dir).unwrap().content, stale.base.content);
    assert_ne!(state::committed(dir).unwrap(), stale.base);
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: stale.id.clone() });
    assert!(s.frame(None, [0, 0, 0]).review.unwrap().stale.is_some(), "same content, later generation");

    // Modified after review: the proposal file is changed by someone (outside Electric Studio).
    let fresh = propose(dir, assistant(), vec![op("edit_device", json!({ "tag": "-M1", "description": "y" }))]);
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: fresh.id.clone() });
    let path = dir.join(change::proposal_path(&fresh.id));
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    v["title"] = json!("something else");
    std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let before = s.editor().project().clone();
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    assert!(status(&s).starts_with("Not applied"), "{}", status(&s));
    assert_eq!(s.editor().project(), &before);
    assert_eq!(ecad_project::load(dir).unwrap(), before, "nothing committed");
    assert!(change::approvals(dir).unwrap().is_empty());
}

#[test]
fn cli_approvals_are_bound_to_hash_revision_scope_and_used_once() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let cs = propose(dir, cli(), vec![op("renumber_devices", json!({}))]);
    let base = cs.base.clone();
    // An assistant's proposal can never be approved for (or applied by) the CLI.
    let a = propose(dir, assistant(), vec![op("edit_device", json!({ "tag": "-M1", "description": "z" }))]);
    let e = change::issue_cli_approval(
        dir,
        &a.id,
        &a.hash(),
        Confirmations { safety: true, global: true },
        Actor::person(),
    );
    assert!(e.unwrap_err().contains("app only"));
    let e = change::apply_approved(dir, &a.id, &base, cli()).unwrap_err();
    assert!(e.contains("app only"), "{e}");
    // Only a person approves.
    assert!(
        change::issue_cli_approval(dir, &cs.id, &cs.hash(), Confirmations { safety: true, global: true }, cli())
            .is_err()
    );
    // Missing confirmations: not approved.
    assert!(
        change::issue_cli_approval(
            dir,
            &cs.id,
            &cs.hash(),
            Confirmations { safety: true, global: false },
            Actor::person()
        )
        .is_err()
    );
    // Wrong scope: an approval permitting less than the proposal does (written by trusted code
    // as an older app might).
    let project = ecad_project::load(dir).unwrap();
    let impact = change::preview(&project, &cs, &Quiet).unwrap();
    let narrow = change::Approval {
        id: change::new_id(),
        proposal: cs.id.clone(),
        hash: cs.hash(),
        revision: base.clone(),
        scope: change::Scope { classes: [ecad_editor::registry::Class::Edit].into() },
        confirmations: Confirmations { safety: true, global: true },
        impact: impact.digest.clone(),
        interface: change::Interface::Cli,
        issued_by: Actor::person(),
        issued_at: change::now(),
        state: ApprovalState::Issued,
    };
    state::commit_workflow(dir, vec![change::approval_file(&narrow)]).unwrap();
    let e = change::apply_approved(dir, &cs.id, &base, cli()).unwrap_err();
    assert!(e.contains("outside the approved scope"), "{e}");
    assert_eq!(state::committed(dir).unwrap(), base, "nothing applied");
    // A proper approval from the app; two applies at once: exactly one wins.
    let mut revoked = narrow.clone();
    revoked.state = ApprovalState::Revoked { reason: "test".into(), at: change::now() };
    state::commit_workflow(dir, vec![change::approval_file(&revoked)]).unwrap();
    // Revoked approvals of the same proposal on either side of the good one, whatever order
    // they are read in, don't stand in its way.
    for id in ["00000000-0000-0000-0000-000000000000", "ffffffff-ffff-ffff-ffff-ffffffffffff"] {
        let mut r = revoked.clone();
        r.id = id.into();
        r.scope = cs.scope.clone();
        state::commit_workflow(dir, vec![change::approval_file(&r)]).unwrap();
    }
    let good = change::issue_cli_approval(
        dir,
        &cs.id,
        &cs.hash(),
        Confirmations { safety: true, global: true },
        Actor::person(),
    )
    .unwrap();
    let results: Vec<Result<change::ChangeRecord, String>> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..4).map(|_| sc.spawn(|| change::apply_approved(dir, &cs.id, &base, cli()))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let ok = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(ok, 1, "{results:?}");
    for e in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(e.contains("refused") || e.contains("busy") || e.contains("saving"), "{e}");
    }
    let approvals = change::approvals(dir).unwrap();
    let used = approvals.iter().find(|a| a.id == good.id).unwrap();
    assert!(matches!(used.state, ApprovalState::Consumed { .. }));
    let applied = change::records(dir)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == RecordKind::ChangeSet && r.outcome == "applied")
        .count();
    assert_eq!(applied, 1);
    // Losers either found the folder locked (nothing read, nothing written) or, once the
    // winner released it, were refused as no longer at the expected revision (recorded).
    let refused_after = results.iter().filter(|r| matches!(r, Err(e) if e.starts_with("refused"))).count();
    assert_eq!(records(dir, RecordKind::Refusal), 2 + refused_after, "app-only and wrong-scope refusals recorded");
    // Replay at the new revision: refused.
    let now = state::committed(dir).unwrap();
    let e = change::apply_approved(dir, &cs.id, &now, cli()).unwrap_err();
    assert!(e.contains("refused"), "{e}");
    assert_eq!(state::committed(dir).unwrap(), now);
    assert_eq!(
        change::status(&change::approvals(dir).unwrap(), &change::records(dir).unwrap(), &cs.id),
        Status::Applied
    );
}

#[test]
fn outside_changes_between_preview_and_apply_are_refused_and_reconciled() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let cs = propose(dir, assistant(), vec![op("edit_device", json!({ "tag": "-M1", "description": "q" }))]);
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: cs.id.clone() });
    // A program edits a page file directly.
    let page = std::fs::read_dir(dir.join("pages")).unwrap().next().unwrap().unwrap().path();
    let mut bytes = std::fs::read(&page).unwrap();
    bytes.extend_from_slice(b"\n");
    std::fs::write(&page, &bytes).unwrap();
    let before = s.editor().project().clone();
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    assert!(status(&s).contains("outside Electric Studio"), "{}", status(&s));
    assert_eq!(s.editor().project(), &before);
    assert!(change::approvals(dir).unwrap().is_empty());
    // Saving over it is refused too.
    s.handle(UiEvent::Rename { tag: "-K7".into() });
    // Reopened: the session shows the reconciliation and refuses to save until it is done.
    let mut s = Session::open(dir).unwrap();
    let rc = s.frame(None, [0, 0, 0]).reconcile.expect("outside changes reported");
    assert!(rc.summary.contains("modified"), "{}", rc.summary);
    assert!(rc.impact.is_some() && !rc.incomplete);
    s.handle(UiEvent::Save);
    assert!(status(&s).starts_with("Not saved"), "{}", status(&s));
    // Restore: the committed file comes back; the changed one is quarantined, not deleted.
    s.handle(UiEvent::RestoreCommitted);
    assert!(status(&s).starts_with("Restored"), "{}", status(&s));
    assert!(s.frame(None, [0, 0, 0]).reconcile.is_none());
    assert!(state::verify(dir, &Quiet).unwrap().clean());
    let quarantined: Vec<_> = walk(&dir.join(".ecad/quarantine"));
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(std::fs::read(&quarantined[0]).unwrap(), bytes);
    // Accept instead: a real outside edit (a device renamed in devices.json).
    let issued = propose(dir, cli(), vec![op("edit_device", json!({ "tag": "-M1", "description": "w" }))]);
    change::issue_cli_approval(
        dir,
        &issued.id,
        &issued.hash(),
        Confirmations { safety: true, global: true },
        Actor::person(),
    )
    .unwrap();
    let path = dir.join("devices.json");
    let text = std::fs::read_to_string(&path).unwrap().replace("\"-M1\"", "\"-M9\"");
    std::fs::write(&path, text).unwrap();
    // The CLI refuses while the folder has outside changes.
    let e = change::apply_approved(dir, &issued.id, &state::committed(dir).unwrap(), cli()).unwrap_err();
    assert!(e.contains("outside Electric Studio"), "{e}");
    let mut s = Session::open(dir).unwrap();
    let rc = s.frame(None, [0, 0, 0]).reconcile.unwrap();
    assert!(rc.affected.iter().any(|(_, a, b)| a == "-M1" && b == "-M9"), "{:?}", rc.affected);
    let g = state::committed(dir).unwrap().generation;
    s.handle(UiEvent::AcceptOutsideChanges { acknowledge_incomplete: false });
    assert!(status(&s).starts_with("Accepted"), "{}", status(&s));
    assert_eq!(state::committed(dir).unwrap().generation, g + 1);
    assert!(s.editor().project().device_by_tag("-M9").is_some());
    let r = change::records(dir)
        .unwrap()
        .into_iter()
        .find(|r| r.kind == RecordKind::Reconciliation && r.outcome == "accepted")
        .unwrap();
    assert_eq!(r.actor, Actor::External);
    // The approval issued before is revoked.
    let a = change::approvals(dir).unwrap().into_iter().find(|a| a.proposal == issued.id).unwrap();
    assert!(matches!(a.state, ApprovalState::Revoked { .. }));
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

#[test]
fn missing_damaged_unexpected_and_unsafe_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    // Unexpected: listed and ignored, never loaded or deleted.
    std::fs::write(dir.join("notes.txt"), "mine").unwrap();
    std::fs::create_dir_all(dir.join("pages/extra")).unwrap();
    std::fs::write(dir.join("pages/extra/x.bin"), "?").unwrap();
    let v = state::verify(dir, &Quiet).unwrap();
    assert!(v.clean(), "{}", v.summary());
    assert!(v.unexpected.contains(&"notes.txt".to_owned()));
    let s = Session::open(dir).unwrap();
    assert!(s.frame(None, [0, 0, 0]).status.contains("Unexpected files"));
    drop(s);
    assert!(dir.join("notes.txt").exists());
    // Missing: reported; opening shows the last commit is restorable; never recreated by guess.
    std::fs::remove_file(dir.join("wires.json")).unwrap();
    let v = state::verify(dir, &Quiet).unwrap();
    assert_eq!(v.missing, ["wires.json"]);
    let mut s = Session::open(dir).unwrap();
    let rc = s.frame(None, [0, 0, 0]).reconcile.unwrap();
    assert!(rc.unloadable.is_some(), "the files don't load without wires.json");
    s.handle(UiEvent::AcceptOutsideChanges { acknowledge_incomplete: true });
    assert!(!dir.join("wires.json").exists(), "never accepted unloadable, never recreated");
    s.handle(UiEvent::RestoreCommitted);
    assert!(dir.join("wires.json").exists());
    assert!(state::verify(dir, &Quiet).unwrap().clean());
    // Damaged: an invalid file is quarantined on restore, never overwritten in place.
    std::fs::write(dir.join("nodes.json"), "{ not json").unwrap();
    assert!(ecad_project::load(dir).is_err());
    let mut s = Session::open(dir).unwrap();
    assert!(s.frame(None, [0, 0, 0]).reconcile.unwrap().unloadable.is_some());
    s.handle(UiEvent::RestoreCommitted);
    assert!(ecad_project::load(dir).is_ok());
    assert!(walk(&dir.join(".ecad/quarantine")).iter().any(|p| std::fs::read_to_string(p).unwrap() == "{ not json"));
    // Damaged retained contents: the comparison is incomplete, and accepting needs an
    // acknowledgement.
    let m = state::read_manifest(dir).unwrap().unwrap();
    let object = dir.join(state::OBJECTS).join(&m.files["devices.json"].sha256);
    std::fs::write(&object, "damaged").unwrap();
    let path = dir.join("devices.json");
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("\"-M1\"", "\"-M8\"")).unwrap();
    let mut s = Session::open(dir).unwrap();
    let rc = s.frame(None, [0, 0, 0]).reconcile.unwrap();
    assert!(rc.incomplete, "{rc:?}");
    s.handle(UiEvent::AcceptOutsideChanges { acknowledge_incomplete: false });
    assert!(status(&s).contains("acknowledge"), "{}", status(&s));
    s.handle(UiEvent::AcceptOutsideChanges { acknowledge_incomplete: true });
    assert!(status(&s).starts_with("Accepted"), "{}", status(&s));
    assert!(state::verify(dir, &Quiet).unwrap().clean());
    // Unsafe paths in a manifest are refused at opening.
    let mut m = state::read_manifest(dir).unwrap().unwrap();
    m.workflow.insert("changes/../../etc/passwd".into(), m.files["project.json"].clone());
    std::fs::write(dir.join(state::MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(state::read_manifest(dir).is_err());
    assert!(state::verify(dir, &Quiet).is_err());
}

#[test]
fn a_project_saved_before_ai0_opens_and_is_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    for sub in [".ecad", "proposals", "approvals", "changes"] {
        let _ = std::fs::remove_dir_all(dir.join(sub));
    }
    assert_eq!(state::committed(dir).unwrap().generation, 0);
    let files = |d: &Path| -> Vec<(String, Vec<u8>)> {
        ecad_project::store::project_files(d)
            .unwrap()
            .into_iter()
            .map(|n| (n.clone(), std::fs::read(d.join(n)).unwrap()))
            .collect()
    };
    let before = files(dir);
    // A format 1 folder is adopted as it is: still format 1, byte for byte.
    let f1 = tempfile::tempdir().unwrap();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("conveyor/format1"), f1.path());
    let f1_before = files(f1.path());
    let _ = Session::open(f1.path()).unwrap();
    assert_eq!(state::committed(f1.path()).unwrap().generation, 1);
    assert_eq!(files(f1.path()), f1_before);
    assert_eq!(ecad_project::format_of(f1.path()).unwrap(), 1);
    assert!(state::verify(f1.path(), &Quiet).unwrap().clean());
    // An MCP server adopts it, so its proposal has a base.
    let mut server = ecad_mcp::Server::open(dir).unwrap();
    let init =
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } });
    server.handle(&init);
    assert_eq!(state::committed(dir).unwrap().generation, 1);
    let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "prepare_renumber_devices", "arguments": {} } });
    let reply = server.handle(&call).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let s = Session::open(dir).unwrap();
    assert_eq!(s.frame(None, [0, 0, 0]).proposals.len(), 1);
    assert!(change::records(dir).unwrap().iter().any(|r| r.outcome == "adopted"));
    assert_eq!(files(dir), before, "adoption writes no engineering file");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn long_operations_report_progress_and_cancel_without_changing_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let before = ecad_project::fingerprint(dir).unwrap();
    // Verification, cancelled before it starts: nothing verified, nothing changed.
    let cancelled = ecad_editor::task::Shared::default();
    cancelled.cancel();
    assert!(matches!(state::verify(dir, &cancelled), Err(ecad_project::ProjectError::Cancelled)));
    // A preview, cancelled.
    let project = ecad_project::load(dir).unwrap();
    let cs = change::prepare(
        &project,
        state::committed(dir).unwrap(),
        cli(),
        "",
        "",
        vec![op("renumber_devices", json!({}))],
    )
    .unwrap();
    assert_eq!(change::preview(&project, &cs, &cancelled).unwrap_err(), "cancelled");
    // In the session: progress while it runs, cancellation from the UI.
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::VerifyProject);
    assert!(s.frame(None, [0, 0, 0]).task.is_some() || status(&s).starts_with("Verification"));
    s.handle(UiEvent::CancelTask);
    s.wait_for_task();
    let st = status(&s);
    assert!(st.contains("cancelled") || st.contains("verified"), "{st}");
    assert!(s.frame(None, [0, 0, 0]).task.is_none());
    // Run to the end: verified.
    s.handle(UiEvent::VerifyProject);
    s.wait_for_task();
    assert!(status(&s).contains("matches its committed manifest"), "{}", status(&s));
    assert_eq!(ecad_project::fingerprint(dir).unwrap(), before);
    // A progress report counts up to the total.
    let shared = ecad_editor::task::Shared::default();
    state::verify(dir, &shared).unwrap();
    let (done, total, _) = shared.progress();
    assert!(total > 10 && done == total, "{done}/{total}");
}

#[test]
fn release_validation_is_one_policy_for_every_path() {
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    let r = ecad_rules::release::validate(&p);
    let errors = ecad_rules::check(&p).iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
    assert_eq!(r.blocking.len(), errors);
    // A device whose product isn't in the catalogue blocks a release.
    let k1 = p.device_by_tag("-K1").unwrap().id;
    let mut parts = p.device(k1).unwrap().parts.clone();
    parts.push(ecad_model::PartRef { product: ecad_model::ProductId::new(), ..parts[0].clone() });
    p.devices.get_mut(&k1).unwrap().parts = parts;
    let r = ecad_rules::release::validate(&p);
    assert!(r.blocking.iter().any(|b| b.rule == "unresolved-product"), "{r:?}");
    assert!(!r.releasable());
    // The same from the registry query (MCP, CLI) and the session.
    let q = ecad_editor::registry::query(&p, "release_check", &json!({})).unwrap();
    assert_eq!(q["releasable"], false);
    assert!(q["note"].as_str().unwrap().contains("not electrical or functional safety certification"));
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&ecad_model::conveyor::conveyor().unwrap(), tmp.path()).unwrap();
    let mut s = Session::open(tmp.path()).unwrap();
    s.handle(UiEvent::ReleaseCheck);
    assert!(s.frame(None, [0, 0, 0]).release.is_some());
    assert!(status(&s).starts_with("Release check at"), "{}", status(&s));
}

#[test]
fn rejection_and_the_app_s_own_edits_are_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let cs = propose(dir, assistant(), vec![op("edit_device", json!({ "tag": "-M1", "description": "r" }))]);
    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: cs.id.clone() });
    s.handle(UiEvent::RejectProposal);
    assert!(status(&s).starts_with("Rejected"), "{}", status(&s));
    assert_eq!(s.frame(None, [0, 0, 0]).proposals[0].status, Status::Rejected);
    s.handle(UiEvent::ReviewProposal { id: cs.id.clone() });
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    assert!(status(&s).starts_with("Not applied"), "a rejected proposal isn't applied: {}", status(&s));
    // A person's own edits, saved: one record by the person with the steps.
    s.handle(UiEvent::CloseReview);
    s.handle(UiEvent::SetAutomation { classes: vec![ecad_model::policy::AutomationClass::Edit] });
    s.handle(UiEvent::Key { key: ecad_editor::KeyName::Undo });
    s.handle(UiEvent::Key { key: ecad_editor::KeyName::Redo });
    s.handle(UiEvent::Save);
    let r = change::records(dir).unwrap().into_iter().find(|r| r.kind == RecordKind::Edits).unwrap();
    assert!(matches!(r.actor, Actor::Person { .. }));
    assert_eq!(r.steps, ["Set automation scope", "Undo: Set automation scope", "Redo: Set automation scope"]);
}

#[test]
fn an_interrupted_apply_leaves_the_folder_consistent() {
    // A save that stops part way (here: before its journal) leaves the previous generation
    // complete; the approval stays issued and no change record appears. The store's own tests
    // inject failures at every step; this checks the apply path on top of it with a lock held
    // by another program (the apply can't start, and changes nothing).
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor(dir);
    let cs = propose(dir, cli(), vec![op("renumber_devices", json!({}))]);
    let a = change::issue_cli_approval(
        dir,
        &cs.id,
        &cs.hash(),
        Confirmations { safety: true, global: true },
        Actor::person(),
    )
    .unwrap();
    let lock = ecad_project::store::Lock::take(dir).unwrap();
    let e = change::apply_approved(dir, &cs.id, &cs.base, cli()).unwrap_err();
    assert!(e.contains("saving"), "{e}");
    drop(lock);
    // Staged files of an interrupted save without its journal are discarded on the next run.
    std::fs::write(dir.join("devices.json.ecad-new"), "partial").unwrap();
    let record = change::apply_approved(dir, &cs.id, &cs.base, cli()).unwrap();
    assert!(!dir.join("devices.json.ecad-new").exists());
    assert_eq!(record.approval.as_deref(), Some(a.id.as_str()));
    assert!(state::verify(dir, &Quiet).unwrap().clean());
}
