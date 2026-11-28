//! The CLI's authority classes (AI0, D42), with the desktop session as the app: read-only
//! commands, dry runs and `--apply` within the project's automation scope, and consequential
//! changes through `ecad prepare`, an approval issued in the app, and `ecad apply --expect`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_editor::change::{ApprovalState, RecordKind};
use ecad_editor::session::{Session, UiEvent};
use std::path::Path;
use std::process::{Command, Output};

fn ecad(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ecad")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn revision(dir: &str) -> String {
    let o = ecad(&["revision", dir]);
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_owned()
}

fn conveyor(root: &Path) -> String {
    let dir = root.join("conveyor");
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    let id = p.device_by_tag("-K2").unwrap().id;
    p.apply(ecad_model::Command::RenameDevice { id, tag: "-K7".into() }).unwrap();
    ecad_project::save(&p, &dir).unwrap();
    dir.to_str().unwrap().to_owned()
}

fn refusals(dir: &str) -> usize {
    ecad_editor::change::records(Path::new(dir)).unwrap().iter().filter(|r| r.kind == RecordKind::Refusal).count()
}

#[test]
fn renumbering_devices_goes_through_prepare_app_approval_and_apply_expect() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = conveyor(tmp.path());
    let devices = std::fs::read(Path::new(&dir).join("devices.json")).unwrap();
    // A global operation is never applied with --apply.
    let o = ecad(&["renumber", &dir, "devices", "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("global operation"), "{}", text(&o));
    assert_eq!(std::fs::read(Path::new(&dir).join("devices.json")).unwrap(), devices);
    // Prepared: a proposal, nothing else changes.
    let base = revision(&dir);
    let o = ecad(&["prepare", &dir, "renumber_devices", "--title", "Tags in drawing order", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let id = v["proposal"].as_str().unwrap().to_owned();
    assert_eq!(v["base"], base.as_str());
    assert_eq!(revision(&dir), base, "preparing doesn't advance the revision");
    assert_eq!(std::fs::read(Path::new(&dir).join("devices.json")).unwrap(), devices);
    // Without an approval from the app: refused, recorded, nothing changes.
    let o = ecad(&["apply", &dir, &id, "--expect", &base]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no approval from the app"), "{}", text(&o));
    assert_eq!(refusals(&dir), 1);
    assert_eq!(revision(&dir), base);
    // The app reviews it and approves it for the CLI (confirming the global operation and the
    // safety impact separately).
    let mut s = Session::open(Path::new(&dir)).unwrap();
    s.handle(UiEvent::ReviewProposal { id: id.clone() });
    let review = s.frame(None, [0, 0, 0]).review.unwrap();
    assert!(review.cli && !review.assistant);
    assert!(review.affected.iter().any(|(_, a, b)| a == "-K7" && b == "-K2"), "{:?}", review.affected);
    s.handle(UiEvent::ApproveForCli { confirm_safety: true, confirm_global: false });
    assert!(s.frame(None, [0, 0, 0]).status.starts_with("Not approved"), "{}", s.frame(None, [0, 0, 0]).status);
    s.handle(UiEvent::ApproveForCli { confirm_safety: true, confirm_global: true });
    let status = s.frame(None, [0, 0, 0]).status;
    assert!(status.contains("ecad apply"), "{status}");
    assert_eq!(revision(&dir), base, "approval bookkeeping doesn't advance the revision");
    // --expect a wrong revision: refused.
    let wrong = base.replace('@', "@9");
    let o = ecad(&["apply", &dir, &id, "--expect", &wrong]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    // Applied: exactly that proposal at exactly that revision, consuming the approval.
    let o = ecad(&["apply", &dir, &id, "--expect", &base]);
    assert!(o.status.success(), "{}", text(&o));
    let after = revision(&dir);
    assert_ne!(after, base);
    let p = ecad_project::load(Path::new(&dir)).unwrap();
    assert!(p.device_by_tag("-K2").is_some() && p.device_by_tag("-K7").is_none());
    let a = ecad_editor::change::approvals(Path::new(&dir)).unwrap();
    assert!(a.iter().any(|a| a.proposal == id
        && matches!(&a.state, ApprovalState::Consumed { by: ecad_editor::change::Actor::Cli { .. }, .. })));
    let records = ecad_editor::change::records(Path::new(&dir)).unwrap();
    let r = records.iter().find(|r| r.kind == RecordKind::ChangeSet && r.proposal.as_deref() == Some(&id)).unwrap();
    match &r.actor {
        ecad_editor::change::Actor::Cli { command, version, .. } => {
            assert!(command.contains("apply") && !version.is_empty());
        }
        other => panic!("{other:?}"),
    }
    // Replayed: refused (the approval is consumed, the revision moved on).
    let o = ecad(&["apply", &dir, &id, "--expect", &after]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("refused"), "{}", text(&o));
    assert_eq!(revision(&dir), after);
    let o = ecad(&["verify", &dir]);
    assert!(o.status.success(), "{}", text(&o));
}

#[test]
fn trusted_automation_applies_only_within_the_scope_a_person_set() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = conveyor(tmp.path());
    let base = revision(&dir);
    // A dry run: read-only, the impact printed.
    let o = ecad(&["reserve", &dir, "tag:-K90..-K91", "--note", "next section"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("dry run"), "{}", text(&o));
    assert_eq!(revision(&dir), base);
    // --apply outside the scope: refused and recorded.
    let o = ecad(&["reserve", &dir, "tag:-K90..-K91", "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("automation scope"), "{}", text(&o));
    assert_eq!(refusals(&dir), 1);
    // A person allows numbering automation in the app (and saves it).
    let mut s = Session::open(Path::new(&dir)).unwrap();
    s.handle(UiEvent::SetAutomation { classes: vec![ecad_model::policy::AutomationClass::Numbering] });
    s.handle(UiEvent::Save);
    assert_eq!(s.frame(None, [0, 0, 0]).automation, [ecad_model::policy::AutomationClass::Numbering]);
    let o = ecad(&["reserve", &dir, "tag:-K90..-K91", "--note", "next section", "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    let p = ecad_project::load(Path::new(&dir)).unwrap();
    assert!(p.numbering.reserved.contains_key(&ecad_model::Identifier::Tag { tag: "-K91".into() }));
    let records = ecad_editor::change::records(Path::new(&dir)).unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.kind == RecordKind::Automation && matches!(r.actor, ecad_editor::change::Actor::Cli { .. }))
    );
    // Edits are still outside the scope.
    let o = ecad(&["run", &dir, "edit_device", "tag=-K1", "description=x", "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    // A safety-related change is outside any scope, even with the class allowed.
    let mut s = Session::open(Path::new(&dir)).unwrap();
    s.handle(UiEvent::SetAutomation { classes: vec![ecad_model::policy::AutomationClass::Edit] });
    s.handle(UiEvent::Save);
    let o = ecad(&["run", &dir, "edit_device", "tag=-S0", "description=changed", "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("safety"), "{}", text(&o));
    let o = ecad(&["run", &dir, "edit_device", "tag=-M1", "description=Conveyor motor", "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
}

#[test]
fn read_only_commands_and_release_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = conveyor(tmp.path());
    let o = ecad(&["operations", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(
        v["registry"].as_array().unwrap().iter().any(|d| d["name"] == "renumber_devices" && d["class"] == "global")
    );
    let o = ecad(&["check", &dir, "--release", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["releasable"], o.status.success());
    assert!(v["note"].as_str().unwrap().contains("not electrical or functional safety certification"));
    let out = tmp.path().join("out");
    let o = ecad(&["export", &dir, out.to_str().unwrap(), "--only", "reports"]);
    assert!(text(&o).contains("draft export"), "{}", text(&o));
}
