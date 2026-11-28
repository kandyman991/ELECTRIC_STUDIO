//! The AI0 pilot, end to end: S0 device renumbering on the conveyor.
//!
//! 1. An MCP client prepares the proposal (`prepare_renumber_devices`): stored, nothing applied.
//! 2. The desktop session lists it and reviews it: the affected devices with their old and new
//!    tags, the pages, the checks, the safety impact; approving needs the separate confirmations
//!    (global operation, safety impact).
//! 3. Approved, it is applied as one transaction: tags renumbered, the consumed approval and the
//!    change record committed with it; one undo restores the original engineering state.
//! 4. A consumed approval can't be used again; a stale proposal can't be approved.
//!
//! `the_pilot_runs_through_the_mcp_server_and_the_desktop_session` drives the server in
//! process. CI also runs `the_official_client_s_proposal_is_approved_in_the_desktop_session`
//! after `tests/client/check.py`, whose proposal (made through the official MCP Python SDK) it
//! approves in the session and then checks with the `ecad` CLI.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_editor::change::{ApprovalState, RecordKind, Status};
use ecad_editor::session::{Session, UiEvent};
use serde_json::{Value, json};
use std::path::Path;

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn call(server: &mut ecad_mcp::Server, name: &str, args: Value) -> (Value, bool) {
    let reply = server.handle(&request(9, "tools/call", json!({ "name": name, "arguments": args }))).unwrap();
    let result = &reply["result"];
    let failed = result["isError"].as_bool().unwrap();
    let value = result.get("structuredContent").cloned().unwrap_or_else(|| result["content"][0]["text"].clone());
    (value, failed)
}

/// Make the conveyor's tags out of drawing order, so renumbering changes some: as a project
/// edited over time would be.
fn conveyor_with_gaps(dir: &Path) {
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    for (from, to) in [("-K2", "-K7"), ("-S1", "-S9")] {
        let id = p.device_by_tag(from).unwrap().id;
        p.apply(ecad_model::Command::RenameDevice { id, tag: to.into() }).unwrap();
    }
    ecad_project::save(&p, dir).unwrap();
}

/// Steps 2–4 in the desktop session, for a proposal already in the folder.
pub fn approve_in_the_desktop_session(dir: &Path, id: &str) {
    let original = ecad_project::load(dir).unwrap();
    let original_hash = original.content_hash().unwrap();
    let before_revision = ecad_project::state::committed(dir).unwrap();
    let mut s = Session::open(dir).unwrap();
    let frame = s.frame(None, [0, 0, 0]);
    let row = frame.proposals.iter().find(|p| p.id == id).expect("listed in the review panel");
    assert_eq!(row.status, Status::Pending);
    assert!(row.actor.starts_with("assistant via MCP"), "{}", row.actor);
    s.handle(UiEvent::ReviewProposal { id: id.into() });
    let review = s.frame(None, [0, 0, 0]).review.expect("under review");
    assert!(review.stale.is_none(), "{review:?}");
    assert!(review.assistant && !review.cli);
    // The affected devices with their old and new tags.
    let renamed: Vec<&(String, String, String)> =
        review.affected.iter().filter(|(w, _, _)| w.starts_with("device")).collect();
    assert!(renamed.iter().any(|(_, a, b)| a == "-K7" && b == "-K2"), "{renamed:?}");
    assert!(renamed.iter().any(|(_, a, b)| a == "-S9" && b == "-S2"), "{renamed:?}");
    // The emergency stop -S0 is renamed too: a direct safety impact, confirmed separately.
    assert!(review.safety_direct.iter().any(|d| d.contains("-S")), "{:?}", review.safety_direct);
    assert!(review.needs_safety_confirmation);
    assert!(renamed.iter().all(|(_, a, b)| a != b));
    assert!(!review.pages.is_empty(), "pages whose drawings change");
    assert!(review.checks.contains("errors"));
    assert!(review.needs_global_confirmation, "renumbering is a global operation");
    // Nothing applied by reviewing.
    assert_eq!(s.editor().project().content_hash().unwrap(), original_hash);
    // Without the separate confirmations it is refused, and nothing changes.
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: false });
    assert!(s.frame(None, [0, 0, 0]).status.contains("global operation"), "{}", s.frame(None, [0, 0, 0]).status);
    assert_eq!(s.editor().project().content_hash().unwrap(), original_hash);
    if review.needs_safety_confirmation {
        s.handle(UiEvent::ApproveProposal { confirm_safety: false, confirm_global: true });
        assert!(s.frame(None, [0, 0, 0]).status.contains("safety"));
    }
    // Approved: applied as one transaction.
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    let status = s.frame(None, [0, 0, 0]).status;
    assert!(status.starts_with("Applied"), "{status}");
    let p = s.editor().project();
    assert_eq!(p.device_by_tag("-K2").map(|d| d.id), original.device_by_tag("-K7").map(|d| d.id), "ids kept");
    assert!(p.device_by_tag("-K7").is_none());
    assert!(!s.editor().is_dirty(), "committed with its approval and record");
    let after = ecad_project::state::committed(dir).unwrap();
    assert_eq!(after.generation, before_revision.generation + 1);
    assert_eq!(ecad_project::load(dir).unwrap(), *p, "on disk");
    let approvals = ecad_editor::change::approvals(dir).unwrap();
    let a = approvals.iter().find(|a| a.proposal == id).unwrap();
    assert!(matches!(a.state, ApprovalState::Consumed { .. }) && a.confirmations.global);
    let records = ecad_editor::change::records(dir).unwrap();
    let r = records.iter().find(|r| r.proposal.as_deref() == Some(id) && r.kind == RecordKind::ChangeSet).unwrap();
    assert_eq!(r.outcome, "applied");
    assert!(matches!(&r.actor, ecad_editor::change::Actor::Person { .. }));
    assert!(r.impact.as_ref().is_some_and(|i| !i.devices.is_empty()));
    assert_eq!(s.frame(None, [0, 0, 0]).proposals.iter().find(|p| p.id == id).unwrap().status, Status::Applied);
    // Approving it again: refused (consumed).
    s.handle(UiEvent::ReviewProposal { id: id.into() });
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    let refused = s.frame(None, [0, 0, 0]).status;
    assert!(refused.starts_with("Not applied"), "{refused}");
    s.handle(UiEvent::CloseReview);
    // One undo restores the original engineering state exactly.
    s.handle(UiEvent::Key { key: ecad_editor::KeyName::Undo });
    assert_eq!(s.editor().project().content_hash().unwrap(), original_hash);
    assert_eq!(s.editor().project(), &original);
    // Saved, it is a later revision with the same content: the old approval stays consumed,
    // and the proposal (prepared at the first revision) is stale.
    s.handle(UiEvent::Save);
    let undone = ecad_project::state::committed(dir).unwrap();
    assert_eq!(undone.content, before_revision.content);
    assert!(undone.generation > after.generation, "{undone} after {after}");
    let records = ecad_editor::change::records(dir).unwrap();
    assert!(records.iter().any(|r| r.kind == RecordKind::Edits && r.steps.iter().any(|st| st.starts_with("Undo: "))));
    s.handle(UiEvent::ReviewProposal { id: id.into() });
    assert!(s.frame(None, [0, 0, 0]).review.unwrap().stale.is_some());
}

#[test]
fn the_pilot_runs_through_the_mcp_server_and_the_desktop_session() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    conveyor_with_gaps(dir);
    let before = std::fs::read(dir.join("devices.json")).unwrap();
    let mut server = ecad_mcp::Server::open(dir).unwrap();
    let init = server.handle(&request(
        0,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "pilot-client", "version": "0.1" } }),
    ));
    assert!(init.unwrap()["result"].is_object());
    let (r, failed) = call(
        &mut server,
        "prepare_renumber_devices",
        json!({ "title": "Renumber devices", "rationale": "tags in drawing order" }),
    );
    assert!(!failed, "{r}");
    let id = r["proposal"].as_str().unwrap().to_owned();
    assert!(r["impact"]["devices"].as_array().unwrap().len() >= 2, "{r}");
    // Preparing changes nothing.
    assert_eq!(std::fs::read(dir.join("devices.json")).unwrap(), before);
    let cs = ecad_editor::change::read_proposal(dir, &id).unwrap();
    match &cs.actor {
        ecad_editor::change::Actor::Assistant { client, .. } => assert_eq!(client, "pilot-client"),
        other => panic!("{other:?}"),
    }
    approve_in_the_desktop_session(dir, &id);
}

/// After `tests/client/check.py`: the official client's proposal, in the desktop session, then
/// the CLI. Set `ECAD_PILOT` to the JSON file check.py wrote.
#[test]
#[ignore = "run in CI after tests/client/check.py"]
fn the_official_client_s_proposal_is_approved_in_the_desktop_session() {
    let pilot: Value = serde_json::from_slice(&std::fs::read(std::env::var("ECAD_PILOT").unwrap()).unwrap()).unwrap();
    let dir = Path::new(pilot["project"].as_str().unwrap());
    let id = pilot["proposal"].as_str().unwrap();
    approve_in_the_desktop_session(dir, id);
    // The CLI sees the committed result.
    let ecad = pilot["ecad"].as_str().unwrap();
    let out = std::process::Command::new(ecad).args(["verify", dir.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let out = std::process::Command::new(ecad).args(["proposals", dir.to_str().unwrap(), "--json"]).output().unwrap();
    let list: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(list["proposals"].as_array().unwrap().iter().any(|p| p["id"] == id && p["status"] == "applied"), "{list}");
}

/// Used by check.py's project setup: the conveyor with tags out of drawing order.
#[test]
#[ignore = "helper for tests/client/check.py"]
fn write_the_pilot_project() {
    conveyor_with_gaps(Path::new(&std::env::var("ECAD_PILOT_PROJECT").unwrap()));
}
