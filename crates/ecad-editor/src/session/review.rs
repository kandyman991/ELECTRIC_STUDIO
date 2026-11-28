//! The app's review panel and committed-state handling (AI0): proposals listed and reviewed,
//! approved in the app only (applied there as one transaction and one undo step, or issued
//! for `ecad apply`), rejected; outside changes reconciled; long operations with progress and
//! cancellation; the release check; the project's automation scope and safety markings.
//!
//! The session knows the committed revision it loaded or last saved, and whether the editor
//! moved since (every change, undo and redo counts, so content that returns to an earlier value
//! is still a later revision).

use super::Session;
use crate::change::{self, Actor, Confirmations, Impact, Status};
use crate::task::Task;
use ecad_project::state::{self, Quiet};
use ecad_project::{Revision, Verification};
use serde::Serialize;

/// A proposal as listed in the panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalRow {
    pub id: String,
    pub title: String,
    pub actor: String,
    pub created: String,
    pub status: Status,
    pub operations: Vec<String>,
}

/// The proposal under review: everything the panel shows, and what an approval binds to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewInfo {
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub actor: String,
    pub assistant: bool,
    pub hash: String,
    pub base: String,
    /// Prepared against another revision than the one open: it can't be approved.
    pub stale: Option<String>,
    pub status: Status,
    pub operations: Vec<String>,
    /// Affected devices, terminals and wires: what, before, after.
    pub affected: Vec<(String, String, String)>,
    pub pages: Vec<String>,
    pub other: Vec<String>,
    pub checks: String,
    pub new_checks: Vec<String>,
    pub resolved_checks: Vec<String>,
    pub safety_direct: Vec<String>,
    pub safety_indirect: Vec<String>,
    pub needs_safety_confirmation: bool,
    pub needs_global_confirmation: bool,
    /// Whether it may be approved for the CLI (trusted automation's proposals only).
    pub cli: bool,
}

/// Outside changes found at opening (D43), until reconciled.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReconcileInfo {
    pub summary: String,
    /// The comparison with the last commit, when the committed contents could be reconstructed.
    pub impact: Option<String>,
    pub affected: Vec<(String, String, String)>,
    /// The committed contents can't all be reconstructed: accepting needs an acknowledgement.
    pub incomplete: bool,
    /// The files on disk don't load: only restoring is possible.
    pub unloadable: Option<String>,
}

/// A long operation's progress, for the frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaskInfo {
    pub title: String,
    pub done: usize,
    pub total: usize,
    pub what: String,
}

#[derive(Debug)]
pub(super) enum Running {
    Verify(Task<Result<Verification, String>>),
}

#[derive(Debug)]
pub(super) struct Review {
    pub(super) info: ReviewInfo,
    pub(super) change_set: change::ChangeSet,
    pub(super) impact: Option<Impact>,
}

#[derive(Debug)]
pub(super) struct Reconcile {
    pub(super) info: ReconcileInfo,
}

fn affected(i: &Impact) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (kind, items) in [("device", &i.devices), ("terminal", &i.terminals), ("wire", &i.wires)] {
        for it in items {
            out.push((
                format!("{kind} {} ({})", if it.before.is_empty() { &it.after } else { &it.before }, it.change),
                it.before.clone(),
                it.after.clone(),
            ));
        }
    }
    out
}

impl Session {
    /// The revision the editor is at: the committed one until anything changes.
    pub fn working_revision(&self) -> Revision {
        let moved = self.editor.revision() - self.committed_at;
        if moved == 0 {
            return self.committed.clone();
        }
        Revision {
            project: self.committed.project.clone(),
            generation: self.committed.generation + moved,
            content: ecad_project::content_digest(self.editor.project()).unwrap_or_default(),
        }
    }

    pub fn committed_revision(&self) -> &Revision {
        &self.committed
    }

    /// After opening or saving: the folder's committed revision, and outside changes.
    pub(super) fn after_open(&mut self) {
        // A project saved before AI0 gets its first committed manifest (the compatibility path).
        if state::committed(&self.dir).is_ok_and(|r| r.generation == 0) && self.dir.join("project.json").exists() {
            let _ = change::adopt(&self.dir, Actor::person());
        }
        self.committed = state::committed(&self.dir).unwrap_or(Revision {
            project: String::new(),
            generation: 0,
            content: String::new(),
        });
        self.committed_at = self.editor.revision();
        self.review = None;
        self.reconcile = None;
        match state::verify(&self.dir, &Quiet) {
            Ok(v) if v.committed.is_some() && !v.clean() => self.note_outside_changes(&v, None),
            Ok(v) if !v.unexpected.is_empty() => {
                self.message = Some(format!("Unexpected files, ignored: {}", v.unexpected.join(", ")));
            }
            Ok(_) => {}
            Err(e) => self.message = Some(format!("The folder's committed state can't be read: {e}")),
        }
        self.refresh_proposals();
    }

    pub(super) fn note_outside_changes(&mut self, v: &Verification, unloadable: Option<String>) {
        let incomplete = !v.damaged_objects.is_empty();
        let (impact, affected_items) = match (incomplete, &unloadable, state::load_committed(&self.dir)) {
            (false, None, Ok(committed)) => match change::compare(&committed, self.editor.project(), &Quiet) {
                Ok(i) => (Some(i.summary()), affected(&i)),
                Err(e) => (Some(format!("comparison failed: {e}")), Vec::new()),
            },
            (_, _, Err(e)) => {
                (Some(format!("the last commit can't be reconstructed: {e}; no complete comparison")), Vec::new())
            }
            _ => (None, Vec::new()),
        };
        self.reconcile = Some(Reconcile {
            info: ReconcileInfo { summary: v.summary(), impact, affected: affected_items, incomplete, unloadable },
        });
        self.message = Some(format!(
            "Changed outside Electric Studio: {}. Restore the committed files or accept the changes",
            v.summary()
        ));
    }

    pub(super) fn refresh_proposals(&mut self) {
        self.proposals = match change::proposals(&self.dir) {
            Ok(list) => list
                .into_iter()
                .map(|l| ProposalRow {
                    id: l.change_set.id.clone(),
                    title: l.change_set.title.clone(),
                    actor: l.change_set.actor.label(),
                    created: l.change_set.created.clone(),
                    status: l.status,
                    operations: l.change_set.operations.iter().map(|o| o.op.clone()).collect(),
                })
                .collect(),
            Err(_) => Vec::new(),
        };
    }

    pub fn proposals(&self) -> &[ProposalRow] {
        &self.proposals
    }

    pub(super) fn review_info(&self) -> Option<ReviewInfo> {
        self.review.as_ref().map(|r| r.info.clone())
    }

    pub(super) fn reconcile_info(&self) -> Option<ReconcileInfo> {
        self.reconcile.as_ref().map(|r| r.info.clone())
    }

    pub(super) fn task_info(&self) -> Option<TaskInfo> {
        self.task.as_ref().map(|t| match t {
            Running::Verify(t) => {
                let (done, total, what) = t.progress();
                TaskInfo { title: t.title.clone(), done, total, what }
            }
        })
    }

    /// Open a proposal for review: its preview against the project as it is open.
    pub(super) fn review_proposal(&mut self, id: &str) {
        self.refresh_proposals();
        let cs = match change::read_proposal(&self.dir, id) {
            Ok(cs) => cs,
            Err(e) => {
                self.message = Some(format!("Can't review: {e}"));
                return;
            }
        };
        let working = self.working_revision();
        let stale = (cs.base != working).then(|| {
            format!("prepared at {}, the project here is at {working}: it can't be approved; prepare it again", cs.base)
        });
        let impact = change::preview(self.editor.project(), &cs, &Quiet);
        let status = self.proposals.iter().find(|p| p.id == cs.id).map_or(Status::Pending, |p| p.status.clone());
        let (affected_items, pages, other, checks, new_checks, resolved, direct, indirect, safety) = match &impact {
            Ok(i) => (
                affected(i),
                i.pages.clone(),
                i.other.clone(),
                format!(
                    "{} → {} errors, {} → {} warnings",
                    i.checks.errors_before, i.checks.errors_after, i.checks.warnings_before, i.checks.warnings_after
                ),
                i.checks.new.clone(),
                i.checks.resolved.clone(),
                i.safety_direct.clone(),
                i.safety_indirect.clone(),
                i.safety(),
            ),
            Err(e) => (
                Vec::new(),
                Vec::new(),
                Vec::new(),
                format!("can't be previewed: {e}"),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                false,
            ),
        };
        let info = ReviewInfo {
            id: cs.id.clone(),
            title: cs.title.clone(),
            rationale: cs.rationale.clone(),
            actor: cs.actor.label(),
            assistant: cs.actor.is_assistant(),
            hash: cs.hash(),
            base: cs.base.to_string(),
            stale,
            status,
            operations: cs.operations.iter().map(|o| format!("{} {}", o.op, o.args)).collect(),
            affected: affected_items,
            pages,
            other,
            checks,
            new_checks,
            resolved_checks: resolved,
            safety_direct: direct,
            safety_indirect: indirect,
            needs_safety_confirmation: safety,
            needs_global_confirmation: cs.scope.global(),
            cli: matches!(cs.actor, Actor::Cli { .. }),
        };
        self.review = Some(Review { info, change_set: cs, impact: impact.ok() });
    }

    /// Approve the proposal under review and apply it here, as one transaction: the change,
    /// the consumed approval and the change record are committed together, as one undo step.
    pub(super) fn approve_and_apply(&mut self, given: Confirmations) {
        let Some(r) = self.review.take() else { return };
        let result = self.try_apply(&r, given);
        match result {
            Ok(msg) => {
                self.message = Some(msg);
                self.review = None;
                self.refresh_proposals();
            }
            Err(e) => {
                self.message = Some(format!("Not applied: {e}"));
                self.review = Some(r);
            }
        }
    }

    fn try_apply(&mut self, r: &Review, given: Confirmations) -> Result<String, String> {
        if self.reconcile.is_some() {
            return Err("reconcile the outside changes first".into());
        }
        let impact = r.impact.as_ref().ok_or("it can't be previewed")?;
        change::check_confirmations(&r.change_set, impact, given)?;
        if r.change_set.actor.is_assistant() && !impact.checks.new_errors.is_empty() {
            return Err(format!(
                "an assistant's proposal that adds check errors can't be applied as it is: {}",
                impact.checks.new_errors.join("; ")
            ));
        }
        let _lock = ecad_project::store::Lock::take(&self.dir).map_err(|e| e.to_string())?;
        ecad_project::store::recover(&self.dir).map_err(|e| e.to_string())?;
        // The folder as committed, untouched outside Electric Studio, and the editor at it.
        let v = state::verify(&self.dir, &Quiet).map_err(|e| e.to_string())?;
        if v.committed.is_some() && !v.clean() {
            return Err(format!("the folder changed outside Electric Studio since it was opened ({})", v.summary()));
        }
        let committed = state::committed(&self.dir).map_err(|e| e.to_string())?;
        if committed != self.committed {
            return Err(format!("stale: the folder was committed by someone else since (now {committed})"));
        }
        let working = self.working_revision();
        if r.change_set.base != working {
            return Err(format!("stale: prepared at {}, the project here is at {working}", r.change_set.base));
        }
        // The exact proposal reviewed: re-read, it must have the same hash.
        let cs = change::read_proposal(&self.dir, &r.change_set.id)?;
        if cs.hash() != r.info.hash {
            return Err("the proposal was modified after it was reviewed: review it again".into());
        }
        let (approvals, records) = (change::approvals(&self.dir)?, change::records(&self.dir)?);
        if change::status(&approvals, &records, &cs.id) != Status::Pending {
            return Err("this proposal was already applied, approved or rejected".into());
        }
        let again = change::preview(self.editor.project(), &cs, &Quiet)?;
        if again.digest != impact.digest {
            return Err("it no longer previews as reviewed: review it again".into());
        }
        let person = Actor::person();
        let mut record = change::ChangeRecord::new(person.clone(), change::RecordKind::ChangeSet, &cs.title, "applied")
            .for_proposal(&cs);
        let approval = change::Approval {
            id: change::new_id(),
            proposal: cs.id.clone(),
            hash: cs.hash(),
            revision: working,
            scope: cs.scope.clone(),
            confirmations: given,
            impact: impact.digest.clone(),
            interface: change::Interface::App,
            issued_by: person.clone(),
            issued_at: change::now(),
            state: change::ApprovalState::Consumed { by: person, at: change::now(), record: record.id.clone() },
        };
        record.approval = Some(approval.id.clone());
        record.result = Some(impact.result.clone());
        record.impact = Some(impact.clone());
        self.editor.run(&cs.label(), cs.commands.clone()).map_err(|e| e.to_string())?;
        let extras = state::Extras { workflow: vec![change::approval_file(&approval), record.file()] };
        match ecad_project::save_locked(self.editor.project(), &self.dir, extras) {
            Ok((seen, revision)) => {
                self.seen = Some(seen);
                self.editor.mark_saved();
                self.committed = revision;
                self.committed_at = self.editor.revision();
                Ok(format!("Applied “{}” ({}) as one step; undo reverts it", cs.title, cs.actor.label()))
            }
            Err(e) => {
                // Not committed: the editor goes back to where it was.
                let _ = self.editor.undo();
                Err(format!("not saved, so not applied: {e}"))
            }
        }
    }

    pub(super) fn approve_for_cli(&mut self, given: Confirmations) {
        let Some((id, hash)) = self.review.as_ref().map(|r| (r.change_set.id.clone(), r.info.hash.clone())) else {
            return;
        };
        if self.working_revision() != self.committed || self.editor.is_dirty() {
            self.message =
                Some("Save or undo your changes first: an approval for the CLI binds to the committed revision".into());
            return;
        }
        self.message = Some(match change::issue_cli_approval(&self.dir, &id, &hash, given, Actor::person()) {
            Ok(a) => format!(
                "Approved for the CLI: run `ecad apply {} {} --expect {}`",
                self.dir.display(),
                a.proposal,
                a.revision
            ),
            Err(e) => format!("Not approved: {e}"),
        });
        let message = self.message.take();
        self.review_proposal(&id);
        self.message = message;
    }

    pub(super) fn reject_proposal(&mut self) {
        let Some(r) = self.review.take() else { return };
        self.message = Some(match change::reject(&self.dir, &r.change_set.id, Actor::person()) {
            Ok(()) => format!("Rejected “{}”", r.change_set.title),
            Err(e) => format!("Not rejected: {e}"),
        });
        self.refresh_proposals();
    }

    /// Accept outside changes as a new committed generation (D43), with a change record by the
    /// external, unknown actor; approvals issued before are revoked.
    pub(super) fn accept_outside_changes(&mut self, acknowledge_incomplete: bool) {
        let Some(rc) = &self.reconcile else { return };
        if let Some(e) = &rc.info.unloadable {
            self.message = Some(format!("The files don't load ({e}): restore them instead"));
            return;
        }
        if rc.info.incomplete && !acknowledge_incomplete {
            self.message = Some(
                "The last commit can't be fully reconstructed: acknowledge that the comparison is incomplete to accept"
                    .into(),
            );
            return;
        }
        let mut record = change::ChangeRecord::new(
            Actor::External,
            change::RecordKind::Reconciliation,
            "Outside changes accepted",
            "accepted",
        );
        record.steps = vec![rc.info.summary.clone(), format!("accepted by {}", Actor::person().label())];
        record.base = Some(self.committed.clone());
        let revoked = match change::revoke_all(&self.dir, "outside changes accepted") {
            Ok(revoked) => revoked,
            Err(e) => {
                self.message = Some(format!("Not accepted: could not revoke approvals: {e}"));
                return;
            }
        };
        record.steps.extend(revoked.iter().map(|(p, _)| format!("revoked {p}")));
        match state::accept(&self.dir, record.file(), revoked) {
            Ok((project, revision)) => {
                let page = self.editor.page();
                match crate::Editor::new(project, page).or_else(|_| {
                    let p = self.editor.project().clone();
                    crate::Editor::new(p, page)
                }) {
                    Ok(e) => self.editor = e,
                    Err(e) => self.message = Some(e.to_string()),
                }
                self.seen = ecad_project::fingerprint(&self.dir).ok();
                self.committed = revision;
                self.committed_at = self.editor.revision();
                self.reconcile = None;
                self.message =
                    Some("Accepted the outside changes as a new revision (recorded as external, unknown)".into());
                self.refresh_proposals();
            }
            Err(e) => self.message = Some(format!("Not accepted: {e}")),
        }
    }

    /// Put the committed files back; what was there is quarantined, never deleted.
    pub(super) fn restore_committed(&mut self) {
        match state::restore(&self.dir) {
            Ok(files) => match super::load_editor_checked(&self.dir) {
                Ok((editor, seen)) => {
                    self.editor = editor;
                    self.seen = Some(seen);
                    self.after_open();
                    self.message = Some(format!("Restored from the last commit: {}", files.join(", ")));
                }
                Err(e) => self.message = Some(format!("Restored, but it doesn't open: {e}")),
            },
            Err(e) => self.message = Some(format!("Not restored: {e}")),
        }
    }

    pub(super) fn start_verify(&mut self) {
        let dir = self.dir.clone();
        self.task = Some(Running::Verify(Task::spawn("Verifying the project folder", move |p| {
            state::verify(&dir, p).map_err(|e| e.to_string())
        })));
    }

    pub(super) fn cancel_task(&mut self) {
        if let Some(Running::Verify(t)) = &self.task {
            t.cancel();
        }
    }

    /// A finished task's result, said; called on every event and frame.
    pub(super) fn poll_task(&mut self) {
        let done = matches!(&self.task, Some(Running::Verify(t)) if t.finished());
        if !done {
            return;
        }
        if let Some(Running::Verify(t)) = self.task.take() {
            self.message = Some(match t.join() {
                Some(Ok(v)) if v.committed.is_some() && !v.clean() => {
                    let s = v.summary();
                    self.note_outside_changes(&v, None);
                    format!("Verification: {s}")
                }
                Some(Ok(v)) => format!("Verification: {}", v.summary()),
                Some(Err(e)) if e == "cancelled" => "Verification cancelled; nothing changed".into(),
                Some(Err(e)) => format!("Verification failed: {e}"),
                None => "Verification stopped".into(),
            });
        }
    }

    /// Wait for a running task (tests and the CLI).
    pub fn wait_for_task(&mut self) {
        while self.task.as_ref().is_some_and(|t| match t {
            Running::Verify(t) => !t.finished(),
        }) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.poll_task();
    }

    pub(super) fn release_check(&mut self) {
        let r = ecad_rules::release::validate(self.editor.project());
        self.message = Some(format!("Release check at {}: {}", self.working_revision(), r.summary()));
        self.release = Some(r);
    }
}
