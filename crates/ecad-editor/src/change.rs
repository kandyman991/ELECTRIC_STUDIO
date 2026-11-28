//! Change sets: prepare, preview, approve, apply (AI0, sections 11.3, 11.4 and 11.8).
//!
//! - **Prepare** lowers registry operations ([`crate::registry`]) on a scratch copy into one
//!   list of commands, against a base [`Revision`], with the actor trusted code assigned. It
//!   is stored as `proposals/<id>.json`; preparing never changes engineering content.
//! - **Preview** applies it to a copy and reports the impact: devices, terminals and wires
//!   added, changed and removed (ids with tags), the pages whose drawings change, checks before
//!   and after, and direct and indirect safety impact (D19).
//! - **Approval** is a person's act in the app's review panel, bound to the proposal's exact
//!   hash, the reviewed revision, the permitted scope and the separate confirmations it needs
//!   (safety impact, global operations). It applies the proposal there and then, or, for a
//!   proposal from trusted automation, issues an approval record that `ecad apply <id>
//!   --expect <revision>` consumes. Assistant (MCP) proposals are applied in the app only.
//! - **Apply** is one transaction and one undo step. It re-verifies the folder, the revision,
//!   the hash, the scope and the preview, and commits the project, the consumed approval and
//!   the change record together. Stale, modified, replayed, wrong-scope and revoked proposals
//!   are refused, and refusals are recorded.
//! - **Change records** (`changes/<id>.json`) are append-only: applied change sets, trusted
//!   automation runs, the app's saved edits (with their undo steps), rejections, refusals and
//!   accepted outside changes.
//!
//! Within Electric Studio's trust boundary only (D31): an agent running as the engineer can do
//! anything the engineer can.

use crate::registry::{self, Class};
use ecad_model::{Command, Project};
use ecad_project::state::{self, Progress, Quiet};
use ecad_project::{ProjectError, Revision};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Who did something, as trusted application code says (never the client).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Actor {
    /// The person using the app.
    Person { user: String },
    /// Trusted CLI automation: the OS user, host, command line and program version.
    Cli { user: String, host: String, command: String, version: String },
    /// An assistant through MCP; the client's name and version are its own claim.
    Assistant { client: String, client_version: String, session: String },
    /// Changes made outside Electric Studio, accepted by a person: not attributable (D43).
    External,
}

fn os_user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "unknown".into())
}

fn host() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_owned()))
        .unwrap_or_else(|| "unknown".into())
}

impl Actor {
    /// The person using the app on this computer.
    pub fn person() -> Actor {
        Actor::Person { user: os_user() }
    }
    /// The CLI, for this run.
    pub fn cli(args: &[String]) -> Actor {
        Actor::Cli { user: os_user(), host: host(), command: args.join(" "), version: env!("CARGO_PKG_VERSION").into() }
    }
    pub fn is_assistant(&self) -> bool {
        matches!(self, Actor::Assistant { .. })
    }
    pub fn label(&self) -> String {
        match self {
            Actor::Person { user } => format!("person {user}"),
            Actor::Cli { user, .. } => format!("CLI automation ({user})"),
            Actor::Assistant { client, client_version, .. } => {
                format!("assistant via MCP ({client} {client_version}, claimed)")
            }
            Actor::External => "external, unknown".into(),
        }
    }
}

/// The time, UTC, as `2026-11-28T09:35:00Z`.
pub fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A new id: random enough to never collide in one project, never a secret.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// One operation of a change set, as the registry names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpCall {
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

/// What a change set may do: the classes of its operations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub classes: BTreeSet<Class>,
}

impl Scope {
    pub fn global(&self) -> bool {
        self.classes.contains(&Class::Global)
    }
    pub fn within(&self, permitted: &Scope) -> bool {
        self.classes.is_subset(&permitted.classes)
    }
}

/// A proposed modification, not applied (section 11.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    /// Assigned by trusted code: the client never names itself.
    pub actor: Actor,
    pub created: String,
    /// The revision it was prepared against.
    pub base: Revision,
    pub operations: Vec<OpCall>,
    pub scope: Scope,
    /// What the operations lowered to.
    pub commands: Vec<Command>,
}

impl ChangeSet {
    /// SHA-256 of its canonical content: what an approval is bound to.
    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        format!("sha256:{}", state::sha256(&bytes))
    }
    /// The undo label it is applied with.
    pub fn label(&self) -> String {
        format!("{} [change set {}]", self.title, self.id.get(..8).unwrap_or(&self.id))
    }
}

/// Lower `ops` against `project` into a change set. Operations run one after the other, each
/// on the result of the ones before. An assistant can't weaken a safety classification (D19).
pub fn prepare(
    project: &Project,
    base: Revision,
    actor: Actor,
    title: &str,
    rationale: &str,
    ops: Vec<OpCall>,
) -> Result<ChangeSet, String> {
    if ops.is_empty() {
        return Err("a change set needs at least one operation".into());
    }
    let mut scratch = project.clone();
    let mut commands = Vec::new();
    let mut scope = Scope::default();
    for call in &ops {
        let def = registry::find(&call.op).ok_or_else(|| format!("unknown operation `{}`", call.op))?;
        if def.class == Class::Query {
            return Err(format!("`{}` is a query, not an operation", call.op));
        }
        if actor.is_assistant() && !def.mcp {
            return Err(format!("`{}` is not offered to assistants", call.op));
        }
        let lowered = registry::lower(&scratch, &call.op, &call.args).map_err(|e| format!("{}: {e}", call.op))?;
        scratch.apply(Command::Batch(lowered.commands.clone())).map_err(|e| e.to_string())?;
        commands.extend(lowered.commands);
        scope.classes.insert(def.class);
    }
    if actor.is_assistant() && weakens_safety(&commands) {
        return Err("an assistant can't remove or weaken a safety classification (D19)".into());
    }
    let title = if title.trim().is_empty() {
        ops.iter().filter_map(|c| registry::find(&c.op)).map(|d| d.title).collect::<Vec<_>>().join(", ")
    } else {
        title.trim().to_owned()
    };
    Ok(ChangeSet {
        id: new_id(),
        title,
        rationale: rationale.to_owned(),
        actor,
        created: now(),
        base,
        operations: ops,
        scope,
        commands,
    })
}

fn weakens_safety(cmds: &[Command]) -> bool {
    cmds.iter().any(|c| match c {
        // Refuse even a transient or no-op unmark in an assistant transaction.
        Command::SetSafetyRelated { related: false, .. } => true,
        Command::Batch(inner) => weakens_safety(inner),
        _ => false,
    })
}

/// One entity a change set adds, changes or removes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub change: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckDelta {
    pub errors_before: usize,
    pub warnings_before: usize,
    pub errors_after: usize,
    pub warnings_after: usize,
    pub new: Vec<String>,
    pub new_errors: Vec<String>,
    pub resolved: Vec<String>,
}

/// What a change does (section 12.5's change-impact analysis, built once for every use).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    pub devices: Vec<Item>,
    pub terminals: Vec<Item>,
    pub wires: Vec<Item>,
    /// Page names whose drawing changes.
    pub pages: Vec<String>,
    /// Other changes: nodes, cables, numbering, settings, bridges.
    pub other: Vec<String>,
    pub checks: CheckDelta,
    /// Safety-related items the change touches.
    pub safety_direct: Vec<String>,
    /// Items it reaches through shared potentials and nets (D19).
    pub safety_indirect: Vec<String>,
    /// The engineering content hash after the change.
    pub result: String,
    /// SHA-256 of all the above: what an approval records as reviewed.
    pub digest: String,
}

impl Impact {
    pub fn safety(&self) -> bool {
        !self.safety_direct.is_empty() || !self.safety_indirect.is_empty()
    }
    pub fn summary(&self) -> String {
        format!(
            "{} devices, {} terminals, {} wires changed; pages {}; checks {}→{} errors, {}→{} warnings; safety: {} direct, {} indirect",
            self.devices.len(),
            self.terminals.len(),
            self.wires.len(),
            if self.pages.is_empty() { "none".into() } else { self.pages.join(", ") },
            self.checks.errors_before,
            self.checks.errors_after,
            self.checks.warnings_before,
            self.checks.warnings_after,
            self.safety_direct.len(),
            self.safety_indirect.len()
        )
    }
}

fn tag_of(p: &Project, d: ecad_model::DeviceId) -> String {
    p.device(d).map(|d| d.tag.clone()).unwrap_or_default()
}

/// The impact of going from `before` to `after` (a change set's preview, or outside changes'
/// comparison with the last commit). Cancellable.
pub fn compare(before: &Project, after: &Project, progress: &dyn Progress) -> Result<Impact, String> {
    let cancelled = || if progress.cancelled() { Err("cancelled".to_owned()) } else { Ok(()) };
    let mut i = Impact::default();
    progress.step(0, 4, "comparing devices and wires");
    cancelled()?;
    let mut changed_devices: BTreeSet<ecad_model::DeviceId> = BTreeSet::new();
    let ids: BTreeSet<_> = before.devices.keys().chain(after.devices.keys()).copied().collect();
    for id in ids {
        let (a, b) = (before.devices.get(&id), after.devices.get(&id));
        if a == b {
            continue;
        }
        changed_devices.insert(id);
        let show = |d: Option<&ecad_model::Device>| d.map(|d| d.tag.clone()).unwrap_or_default();
        let change = match (a, b) {
            (None, _) => "added",
            (_, None) => "removed",
            (Some(x), Some(y)) if x.tag != y.tag => "renamed",
            _ => "changed",
        };
        i.devices.push(Item { id: id.0.to_string(), change: change.into(), before: show(a), after: show(b) });
    }
    let tids: BTreeSet<_> = before.terminals.keys().chain(after.terminals.keys()).copied().collect();
    for id in tids {
        let (a, b) = (before.terminals.get(&id), after.terminals.get(&id));
        if a == b {
            continue;
        }
        let label = |p: &Project, t: Option<&ecad_model::TerminalEntry>| {
            t.map(|t| format!("{}:{}", tag_of(p, t.device), t.designation)).unwrap_or_default()
        };
        for t in [a, b].into_iter().flatten() {
            changed_devices.insert(t.device);
        }
        let change = match (a, b) {
            (None, _) => "added",
            (_, None) => "removed",
            _ => "changed",
        };
        i.terminals.push(Item {
            id: id.0.to_string(),
            change: change.into(),
            before: label(before, a),
            after: label(after, b),
        });
    }
    let number = |p: &Project, w: ecad_model::ConnectionId| -> Option<String> {
        p.connections.get(&w).map(|c| c.wire_number.clone().unwrap_or_default())
    };
    let wids: BTreeSet<_> = before
        .connections
        .keys()
        .chain(after.connections.keys())
        .chain(before.undrawn.keys())
        .chain(after.undrawn.keys())
        .copied()
        .collect();
    let mut pages: BTreeSet<ecad_model::PageId> = BTreeSet::new();
    let mut changed_wires = BTreeSet::new();
    for id in wids {
        let (a, b) = (before.connections.get(&id), after.connections.get(&id));
        let (ua, ub) = (before.undrawn.get(&id), after.undrawn.get(&id));
        if a == b && ua == ub {
            continue;
        }
        changed_wires.insert(id);
        for (p, c) in [(before, a), (after, b)] {
            if let Some(c) = c
                && let Ok(f) = p.function(c.from.function)
            {
                pages.insert(f.page);
            }
        }
        let show = |p: &Project, drawn: Option<&ecad_model::Connection>, undrawn: Option<&ecad_model::Wire>| {
            drawn
                .map(|_| number(p, id).unwrap_or_default())
                .or_else(|| undrawn.map(|w| w.wire_number.clone().unwrap_or_default()))
                .unwrap_or_default()
        };
        let change = match (a.or(None).is_some() || ua.is_some(), b.is_some() || ub.is_some()) {
            (false, _) => "added",
            (_, false) => "removed",
            _ => "changed",
        };
        i.wires.push(Item {
            id: id.0.to_string(),
            change: change.into(),
            before: show(before, a, ua),
            after: show(after, b, ub),
        });
    }
    progress.step(1, 4, "pages");
    cancelled()?;
    let fids: BTreeSet<_> = before.functions.keys().chain(after.functions.keys()).copied().collect();
    for id in fids {
        let (a, b) = (before.functions.get(&id), after.functions.get(&id));
        if a != b {
            for f in [a, b].into_iter().flatten() {
                pages.insert(f.page);
                if f.node.is_none() {
                    changed_devices.insert(f.device);
                }
            }
        }
    }
    // A renamed device's tag is drawn on every page that shows it.
    for d in &changed_devices {
        for p in [before, after] {
            pages.extend(p.functions_of(*d).map(|f| f.page));
        }
    }
    let pids: BTreeSet<_> = before.pages.keys().chain(after.pages.keys()).copied().collect();
    let global_drawing_change = before.sheet != after.sheet || before.name != after.name || before.info != after.info;
    for id in pids {
        if global_drawing_change
            || before.pages.get(&id) != after.pages.get(&id)
            || before.panels.get(&id) != after.panels.get(&id)
        {
            pages.insert(id);
        }
    }
    for (id, item) in before.panel_items.iter().chain(&after.panel_items) {
        if before.panel_items.get(id) != after.panel_items.get(id) {
            pages.insert(item.page);
            if let ecad_model::PanelItemKind::Device { device } = item.kind {
                changed_devices.insert(device);
            }
        }
    }
    for (id, location) in before.boxes.iter().chain(&after.boxes) {
        if before.boxes.get(id) != after.boxes.get(id) {
            pages.insert(location.page);
        }
    }
    let mut names: Vec<String> = pages
        .iter()
        .filter_map(|id| after.page(*id).or_else(|_| before.page(*id)).ok().map(|p| p.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    i.pages = names;
    for (what, differs) in [
        ("nodes", before.nodes != after.nodes),
        ("cables", before.cables != after.cables),
        ("bridges", before.bridges != after.bridges),
        ("numbering (reserved, retired, policies)", before.numbering != after.numbering),
        ("products", before.parts != after.parts),
        ("symbol definitions", before.symbols != after.symbols),
        ("sheet definition", before.sheet != after.sheet),
        ("project title and metadata", before.name != after.name || before.info != after.info),
        ("location boxes", before.boxes != after.boxes),
        ("safety classification", before.safety_related != after.safety_related),
        ("automation scope", before.automation != after.automation),
        ("naming", before.naming != after.naming),
        ("panel layout", before.panels != after.panels || before.panel_items != after.panel_items),
        ("underlays", before.underlays != after.underlays),
    ] {
        if differs {
            i.other.push(what.into());
        }
    }
    progress.step(2, 4, "checks");
    cancelled()?;
    let key = |c: &ecad_rules::Check| format!("{}: {}", c.rule, c.message);
    let (cb, ca) = (ecad_rules::check(before), ecad_rules::check(after));
    let errors = |v: &[ecad_rules::Check]| v.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
    i.checks.errors_before = errors(&cb);
    i.checks.warnings_before = cb.len() - i.checks.errors_before;
    i.checks.errors_after = errors(&ca);
    i.checks.warnings_after = ca.len() - i.checks.errors_after;
    let kb: BTreeSet<String> = cb.iter().map(key).collect();
    let ka: BTreeSet<String> = ca.iter().map(key).collect();
    i.checks.new = ka.difference(&kb).cloned().collect();
    i.checks.new_errors =
        ca.iter().filter(|c| c.severity == ecad_rules::Severity::Error && !kb.contains(&key(c))).map(key).collect();
    i.checks.resolved = kb.difference(&ka).cloned().collect();
    progress.step(3, 4, "safety impact");
    cancelled()?;
    let marked: BTreeSet<ecad_model::DeviceId> =
        before.safety_related.iter().chain(&after.safety_related).copied().collect();
    // Devices sharing a net with a safety-related device's terminal, before or after.
    let mut reached: BTreeSet<ecad_model::DeviceId> = BTreeSet::new();
    let mut safety_wires = BTreeSet::new();
    let mut safety_endpoints = BTreeSet::new();
    let mut safety_nets = [BTreeSet::new(), BTreeSet::new()];
    for (index, p) in [before, after].into_iter().enumerate() {
        let device_of = |e: &ecad_model::Endpoint| match e {
            ecad_model::Endpoint::Terminal(t) => p.terminals.get(t).map(|t| t.device),
            ecad_model::Endpoint::Node(_) => None,
        };
        for net in p.netlist().nets {
            let on: BTreeSet<_> = net.endpoints.iter().filter_map(device_of).collect();
            if on.iter().any(|d| marked.contains(d)) {
                safety_wires.extend(net.wires.iter().copied());
                safety_endpoints.extend(net.endpoints.iter().copied());
                safety_nets[index].insert((net.endpoints.clone(), net.names.clone(), net.wires.clone()));
                for wire in net.wires.iter().filter(|w| changed_wires.contains(w)) {
                    i.safety_indirect.push(format!("wire {} (on a safety-related potential)", wire.0));
                }
                // Named supply and interruption nets also carry safety dependencies.
                // Conservatively require review rather than assuming a shared supply
                // is unrelated (D19; the conveyor's -F3 is a mandatory example).
                reached.extend(on);
            }
        }
    }
    if safety_nets[0] != safety_nets[1] {
        i.safety_indirect.push("topology of a safety-related potential changed".into());
    }
    for (id, node) in before.nodes.iter().chain(&after.nodes) {
        if safety_endpoints.contains(&ecad_model::Endpoint::Node(*id)) && before.nodes.get(id) != after.nodes.get(id) {
            i.safety_indirect.push(format!("node {} (on a safety-related potential)", node.name));
        }
    }
    for bridge in before.bridges.symmetric_difference(&after.bridges) {
        if [bridge.a, bridge.b].iter().any(|t| safety_endpoints.contains(&ecad_model::Endpoint::Terminal(*t))) {
            i.safety_indirect.push("bridge on a safety-related potential changed".into());
        }
    }
    let label = |d: &ecad_model::DeviceId| {
        let t = tag_of(after, *d);
        if t.is_empty() { tag_of(before, *d) } else { t }
    };
    for d in &changed_devices {
        if marked.contains(d) {
            i.safety_direct.push(label(d));
        } else if reached.contains(d) {
            i.safety_indirect.push(format!("{} (shares a potential with a safety-related device)", label(d)));
        }
    }
    if before.safety_related != after.safety_related {
        for d in before.safety_related.symmetric_difference(&after.safety_related) {
            let l = format!("{} (classification changed)", label(d));
            if !i.safety_direct.contains(&l) {
                i.safety_direct.push(l);
            }
        }
    }
    for c in after.cables.values().chain(before.cables.values()) {
        if before.cables.get(&c.id) == after.cables.get(&c.id) {
            continue;
        }
        let carries = |p: &Project| {
            p.connections
                .values()
                .any(|w| safety_wires.contains(&w.id) && w.spec.cable.as_ref().is_some_and(|k| k.cable == c.id))
                || p.undrawn
                    .values()
                    .any(|w| safety_wires.contains(&w.id) && w.spec.cable.as_ref().is_some_and(|k| k.cable == c.id))
        };
        if carries(before) || carries(after) {
            i.safety_indirect.push(format!("cable {} (carries a safety-related device's signals)", c.tag));
        }
    }
    i.safety_direct.sort();
    i.safety_direct.dedup();
    i.safety_indirect.sort();
    i.safety_indirect.dedup();
    i.result = after.content_hash().map_err(|e| e.to_string())?;
    i.digest = format!("sha256:{}", state::sha256(&serde_json::to_vec(&i).unwrap_or_default()));
    progress.step(4, 4, "done");
    Ok(i)
}

/// Preview a change set on `project` (which isn't changed).
pub fn preview(project: &Project, cs: &ChangeSet, progress: &dyn Progress) -> Result<Impact, String> {
    if cs.actor.is_assistant() && weakens_safety(&cs.commands) {
        return Err("an assistant can't remove or weaken a safety classification (D19)".into());
    }
    let mut after = project.clone();
    after.underlays = project.underlays.clone();
    after.apply(Command::Batch(cs.commands.clone())).map_err(|e| format!("it no longer applies: {e}"))?;
    compare(project, &after, progress)
}

/// The separate confirmations a person gave in the review panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmations {
    /// The change has a direct or indirect safety impact (D19).
    pub safety: bool,
    /// The change contains a global operation (renumbering), authorised separately.
    pub global: bool,
}

/// The confirmations a change set needs; `Err` names what is missing.
pub fn check_confirmations(cs: &ChangeSet, impact: &Impact, given: Confirmations) -> Result<(), String> {
    if impact.safety() && !given.safety {
        return Err("this change has a safety impact: confirm it separately".into());
    }
    if cs.scope.global() && !given.global {
        return Err("this change contains a global operation: authorise it separately".into());
    }
    Ok(())
}

/// Where an approval may be consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Interface {
    /// Applied in the app when approved.
    App,
    /// Issued for `ecad apply <id> --expect <revision>`.
    Cli,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ApprovalState {
    Issued,
    Consumed { by: Actor, at: String, record: String },
    Revoked { reason: String, at: String },
}

/// A person's approval of one exact proposal (F04).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub proposal: String,
    /// The proposal's hash, as reviewed.
    pub hash: String,
    /// The revision it was reviewed at.
    pub revision: Revision,
    /// What it permits.
    pub scope: Scope,
    pub confirmations: Confirmations,
    /// The preview's digest, as reviewed: applying re-previews and must match.
    pub impact: String,
    pub interface: Interface,
    pub issued_by: Actor,
    pub issued_at: String,
    #[serde(flatten)]
    pub state: ApprovalState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    /// A change set applied (in the app or by `ecad apply`).
    ChangeSet,
    /// A person's edits in the app, as saved (undo steps included).
    Edits,
    /// A trusted automation run (`--apply`).
    Automation,
    /// Outside changes accepted by a person.
    Reconciliation,
    /// A proposal rejected in the review panel.
    Rejection,
    /// An attempt refused: what, who and why.
    Refusal,
    /// An approval issued for the CLI.
    Approval,
}

/// An engineering change record (D17), append-only, `changes/<id>.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub id: String,
    pub at: String,
    pub actor: Actor,
    pub kind: RecordKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Revision>,
    /// The engineering content hash afterwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// `applied`, `rejected`, `refused: …`, `accepted`, `issued`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<Impact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

impl ChangeRecord {
    pub fn new(actor: Actor, kind: RecordKind, title: &str, outcome: &str) -> ChangeRecord {
        ChangeRecord {
            id: new_id(),
            at: now(),
            actor,
            kind,
            title: title.to_owned(),
            proposal: None,
            hash: None,
            approval: None,
            scope: None,
            base: None,
            result: None,
            outcome: outcome.to_owned(),
            impact: None,
            steps: Vec::new(),
        }
    }
    pub fn for_proposal(mut self, cs: &ChangeSet) -> Self {
        self.proposal = Some(cs.id.clone());
        self.hash = Some(cs.hash());
        self.scope = Some(cs.scope.clone());
        self.base = Some(cs.base.clone());
        self
    }
    /// Its file: sortable by time.
    pub fn path(&self) -> String {
        let at: String = self.at.chars().filter(char::is_ascii_alphanumeric).collect();
        format!("changes/{at}-{}.json", self.id)
    }
    pub fn file(&self) -> (String, Vec<u8>) {
        (self.path(), serde_json::to_vec_pretty(self).unwrap_or_default())
    }
}

fn json_file<T: Serialize>(path: String, value: &T) -> (String, Vec<u8>) {
    (path, serde_json::to_vec_pretty(value).unwrap_or_default())
}

pub fn proposal_path(id: &str) -> String {
    format!("proposals/{id}.json")
}
pub fn approval_path(id: &str) -> String {
    format!("approvals/{id}.json")
}

fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("`{id}` is not a proposal id"));
    }
    Ok(())
}

fn project_err(e: ProjectError) -> String {
    e.to_string()
}

/// Read a proposal from the folder, checked against the committed manifest (a proposal
/// modified outside Electric Studio is refused).
pub fn read_proposal(dir: &Path, id: &str) -> Result<ChangeSet, String> {
    check_id(id)?;
    let bytes = state::read_workflow(dir, &proposal_path(id)).map_err(project_err)?;
    serde_json::from_slice(&bytes).map_err(|e| format!("proposal {id}: {e}"))
}

/// Every approval in the folder.
pub fn approvals(dir: &Path) -> Result<Vec<Approval>, String> {
    let mut out = Vec::new();
    for path in state::workflow_files(dir).map_err(project_err)?.into_iter().filter(|p| p.starts_with("approvals/")) {
        let bytes = state::read_workflow(dir, &path).map_err(project_err)?;
        out.push(serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    Ok(out)
}

/// Every change record in the folder, oldest first.
pub fn records(dir: &Path) -> Result<Vec<ChangeRecord>, String> {
    let mut out = Vec::new();
    for path in state::workflow_files(dir).map_err(project_err)?.into_iter().filter(|p| p.starts_with("changes/")) {
        let bytes = state::read_workflow(dir, &path).map_err(project_err)?;
        out.push(serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    Ok(out)
}

/// Where a proposal stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    /// Approved for the CLI, not applied yet.
    Approved,
    Applied,
    Rejected,
}

/// A proposal's status from its approvals and records.
pub fn status(approvals: &[Approval], records: &[ChangeRecord], id: &str) -> Status {
    let mine = |r: &&ChangeRecord| r.proposal.as_deref() == Some(id);
    if records.iter().filter(mine).any(|r| r.kind == RecordKind::ChangeSet && r.outcome == "applied") {
        return Status::Applied;
    }
    if records.iter().filter(mine).any(|r| r.kind == RecordKind::Rejection) {
        return Status::Rejected;
    }
    if approvals.iter().any(|a| a.proposal == id && a.state == ApprovalState::Issued) {
        return Status::Approved;
    }
    Status::Pending
}

/// A proposal with its status, for listing.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub change_set: ChangeSet,
    pub hash: String,
    pub status: Status,
}

/// Every proposal in the folder, newest first.
pub fn proposals(dir: &Path) -> Result<Vec<Listed>, String> {
    let approvals = approvals(dir)?;
    let records = records(dir)?;
    let mut out = Vec::new();
    for path in state::workflow_files(dir).map_err(project_err)?.into_iter().filter(|p| p.starts_with("proposals/")) {
        let bytes = state::read_workflow(dir, &path).map_err(project_err)?;
        let cs: ChangeSet = serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let status = status(&approvals, &records, &cs.id);
        out.push(Listed { hash: cs.hash(), status, change_set: cs });
    }
    out.sort_by(|a, b| b.change_set.created.cmp(&a.change_set.created));
    Ok(out)
}

/// Adopt a folder saved before AI0 (its first committed manifest), recorded by `actor`.
pub fn adopt(dir: &Path, actor: Actor) -> Result<Revision, String> {
    let record = ChangeRecord::new(actor, RecordKind::Reconciliation, "Adopted a project saved before AI0", "adopted");
    state::adopt(dir, record.file()).map_err(project_err)
}

/// Store a prepared change set as a proposal (`proposals/<id>.json`), if the folder is still at
/// its base revision and has no outside changes. Engineering content and the generation are
/// unchanged.
pub fn save_proposal(dir: &Path, cs: &ChangeSet) -> Result<Revision, String> {
    let _lock = ecad_project::store::Lock::take(dir).map_err(project_err)?;
    ecad_project::store::recover(dir).map_err(project_err)?;
    let committed = state::committed(dir).map_err(project_err)?;
    if committed != cs.base {
        return Err(format!(
            "stale: the project is at {committed}, the proposal was prepared at {}; reload and prepare it again",
            cs.base
        ));
    }
    state::commit_workflow_locked(dir, vec![json_file(proposal_path(&cs.id), cs)]).map_err(project_err)
}

/// Record a refusal (best effort: a folder with outside changes takes no record until it is
/// reconciled, and the refusal is still reported to the caller).
fn refuse_locked(dir: &Path, actor: &Actor, cs: Option<&ChangeSet>, what: &str, reason: &str) -> String {
    let mut r = ChangeRecord::new(actor.clone(), RecordKind::Refusal, what, &format!("refused: {reason}"));
    if let Some(cs) = cs {
        r = r.for_proposal(cs);
    }
    let _ = state::commit_workflow_locked(dir, vec![r.file()]);
    format!("refused: {reason}")
}

/// Issue an approval of a proposal for `ecad apply` (the app's review panel). The folder must
/// be at the proposal's base with no outside changes; assistant proposals are app-only.
pub fn issue_cli_approval(
    dir: &Path,
    id: &str,
    reviewed_hash: &str,
    given: Confirmations,
    by: Actor,
) -> Result<Approval, String> {
    if !matches!(by, Actor::Person { .. }) {
        return Err("only a person approves, in the app".into());
    }
    let _lock = ecad_project::store::Lock::take(dir).map_err(project_err)?;
    ecad_project::store::recover(dir).map_err(project_err)?;
    let cs = read_proposal(dir, id)?;
    if cs.hash() != reviewed_hash {
        return Err("the proposal changed since it was reviewed: review it again".into());
    }
    if cs.actor.is_assistant() {
        return Err("an assistant's proposal is applied in the app only, never handed to the CLI".into());
    }
    let committed = state::committed(dir).map_err(project_err)?;
    if committed != cs.base {
        return Err(format!("stale: the project is at {committed}, the proposal was prepared at {}", cs.base));
    }
    let (approvals, records) = (approvals(dir)?, records(dir)?);
    if status(&approvals, &records, id) != Status::Pending {
        return Err("this proposal is not pending".into());
    }
    let project = ecad_project::load(dir).map_err(project_err)?;
    let impact = preview(&project, &cs, &Quiet)?;
    check_confirmations(&cs, &impact, given)?;
    let approval = Approval {
        id: new_id(),
        proposal: cs.id.clone(),
        hash: cs.hash(),
        revision: committed,
        scope: cs.scope.clone(),
        confirmations: given,
        impact: impact.digest.clone(),
        interface: Interface::Cli,
        issued_by: by.clone(),
        issued_at: now(),
        state: ApprovalState::Issued,
    };
    let mut record = ChangeRecord::new(by, RecordKind::Approval, &cs.title, "issued").for_proposal(&cs);
    record.approval = Some(approval.id.clone());
    state::commit_workflow_locked(dir, vec![json_file(approval_path(&approval.id), &approval), record.file()])
        .map_err(project_err)?;
    Ok(approval)
}

/// Reject a proposal (the app's review panel).
pub fn reject(dir: &Path, id: &str, by: Actor) -> Result<(), String> {
    if !matches!(by, Actor::Person { .. }) {
        return Err("only a person rejects, in the app".into());
    }
    let _lock = ecad_project::store::Lock::take(dir).map_err(project_err)?;
    ecad_project::store::recover(dir).map_err(project_err)?;
    let cs = read_proposal(dir, id)?;
    let (all, records) = (approvals(dir)?, records(dir)?);
    if !matches!(status(&all, &records, id), Status::Pending | Status::Approved) {
        return Err("this proposal is already applied or rejected".into());
    }
    let record = ChangeRecord::new(by, RecordKind::Rejection, &cs.title, "rejected").for_proposal(&cs);
    let mut workflow = vec![record.file()];
    for mut approval in all.into_iter().filter(|a| a.proposal == id && a.state == ApprovalState::Issued) {
        approval.state = ApprovalState::Revoked { reason: "proposal rejected".into(), at: now() };
        workflow.push(approval_file(&approval));
    }
    state::commit_workflow_locked(dir, workflow).map_err(project_err)?;
    Ok(())
}

/// `ecad apply <id> --expect <revision>` (D42, F04): apply an exact reviewed proposal with its
/// app-issued approval, consuming the approval atomically with the change and its record.
/// Refused, and recorded, when the folder isn't at `expect`, the proposal is an assistant's,
/// stale, modified, outside the approved scope, unapproved, already consumed or revoked, or no
/// longer previews as reviewed.
pub fn apply_approved(dir: &Path, id: &str, expect: &Revision, actor: Actor) -> Result<ChangeRecord, String> {
    let _lock = ecad_project::store::Lock::take(dir).map_err(project_err)?;
    ecad_project::store::recover(dir).map_err(project_err)?;
    let v = state::verify(dir, &Quiet).map_err(project_err)?;
    if !v.clean() {
        return Err(format!(
            "refused: the folder changed outside Electric Studio ({}); reconcile it in the app",
            v.summary()
        ));
    }
    let cs = match read_proposal(dir, id) {
        Ok(cs) => cs,
        Err(e) => return Err(refuse_locked(dir, &actor, None, &format!("apply {id}"), &e)),
    };
    let refuse = |reason: &str| refuse_locked(dir, &actor, Some(&cs), &cs.title, reason);
    let committed = state::committed(dir).map_err(project_err)?;
    if &committed != expect {
        return Err(refuse(&format!("the project is at {committed}, not the expected {expect}")));
    }
    if cs.actor.is_assistant() {
        return Err(refuse("an assistant's proposal is applied in the app only"));
    }
    if cs.base != committed {
        return Err(refuse(&format!("stale: prepared at {}, the project is at {committed}", cs.base)));
    }
    let hash = cs.hash();
    let all = approvals(dir)?;
    if status(&all, &records(dir)?, id) == Status::Rejected {
        return Err(refuse("the proposal was rejected"));
    }
    let mine: Vec<&Approval> = all.iter().filter(|a| a.proposal == cs.id).collect();
    // An issued approval first: a revoked or used one beside it (an earlier approval of the
    // same proposal) doesn't stand in its way; with none issued, the used or revoked one
    // says why.
    let rank = |a: &Approval| match a.state {
        ApprovalState::Issued => 0,
        ApprovalState::Consumed { .. } => 1,
        ApprovalState::Revoked { .. } => 2,
    };
    let candidate =
        mine.iter().filter(|a| a.interface == Interface::Cli && a.hash == hash).min_by_key(|a| rank(a)).copied();
    let Some(approval) = candidate else {
        let why = if mine.iter().any(|a| a.hash != hash) {
            "the proposal was modified after it was approved"
        } else if mine.iter().any(|a| a.interface != Interface::Cli) {
            "its approval is for the app, not the CLI (wrong scope)"
        } else {
            "no approval from the app for this proposal"
        };
        return Err(refuse(why));
    };
    match &approval.state {
        ApprovalState::Issued => {}
        ApprovalState::Consumed { .. } => return Err(refuse("its approval was already used (replay)")),
        ApprovalState::Revoked { reason, .. } => return Err(refuse(&format!("its approval was revoked: {reason}"))),
    }
    if approval.revision != committed {
        return Err(refuse("its approval is for another revision (stale)"));
    }
    if !cs.scope.within(&approval.scope) {
        return Err(refuse("the proposal is outside the approved scope"));
    }
    let mut project = ecad_project::load(dir).map_err(project_err)?;
    let impact = match preview(&project, &cs, &Quiet) {
        Ok(i) => i,
        Err(e) => return Err(refuse(&e)),
    };
    if impact.digest != approval.impact {
        return Err(refuse("it no longer previews as it was reviewed"));
    }
    if let Err(e) = check_confirmations(&cs, &impact, approval.confirmations) {
        return Err(refuse(&e));
    }
    if let Err(e) = project.apply(Command::Batch(cs.commands.clone())) {
        return Err(refuse(&e.to_string()));
    }
    let mut record = ChangeRecord::new(actor.clone(), RecordKind::ChangeSet, &cs.title, "applied").for_proposal(&cs);
    record.approval = Some(approval.id.clone());
    record.result = Some(impact.result.clone());
    record.impact = Some(impact);
    let mut consumed = approval.clone();
    consumed.state = ApprovalState::Consumed { by: actor, at: now(), record: record.id.clone() };
    let extras = state::Extras { workflow: vec![json_file(approval_path(&consumed.id), &consumed), record.file()] };
    ecad_project::save_locked(&project, dir, extras).map_err(project_err)?;
    Ok(record)
}

/// What a trusted automation run did, or would do.
#[derive(Debug)]
pub struct Automated {
    pub impact: Impact,
    pub result: Value,
    /// The record written, when applied.
    pub record: Option<ChangeRecord>,
}

/// Trusted automation (D42): run one operation; with `apply`, apply it, but only within the
/// project's automation scope (set by a person in the app), never a global or safety-related
/// change, and record it. Without `apply` it is a dry run: the impact, nothing written.
pub fn automate(dir: &Path, op: &str, args: &Value, apply: bool, actor: Actor) -> Result<Automated, String> {
    let def = registry::find(op).ok_or_else(|| format!("unknown operation `{op}`"))?;
    let _lock = ecad_project::store::Lock::take(dir).map_err(project_err)?;
    ecad_project::store::recover(dir).map_err(project_err)?;
    let v = state::verify(dir, &Quiet).map_err(project_err)?;
    if v.committed.is_some() && !v.clean() {
        return Err(format!(
            "refused: the folder changed outside Electric Studio ({}); reconcile it in the app",
            v.summary()
        ));
    }
    let mut project = ecad_project::load(dir).map_err(project_err)?;
    let lowered = registry::lower(&project, op, args)?;
    let mut after = project.clone();
    after.apply(Command::Batch(lowered.commands.clone())).map_err(|e| e.to_string())?;
    let impact = compare(&project, &after, &Quiet)?;
    if !apply {
        return Ok(Automated { impact, result: lowered.result, record: None });
    }
    let refuse = |reason: &str| refuse_locked(dir, &actor, None, def.title, reason);
    let Some(class) = def.class.automation() else {
        return Err(refuse(&format!(
            "`{op}` is a {:?} operation, outside any automation scope: prepare a change set, have it approved in the app, then `ecad apply <id> --expect <revision>`",
            def.class
        )));
    };
    if !project.automation.allowed.contains(&class) {
        return Err(refuse(&format!(
            "the project's automation scope doesn't allow {} operations (a person sets it in the app)",
            class.label()
        )));
    }
    if impact.safety() {
        return Err(refuse("a safety-related change is outside any automation scope: use a reviewed change set"));
    }
    project.apply(Command::Batch(lowered.commands)).map_err(|e| e.to_string())?;
    let mut record = ChangeRecord::new(actor, RecordKind::Automation, def.title, "applied");
    record.scope = Some(Scope { classes: [def.class].into() });
    record.base = Some(state::committed(dir).map_err(project_err)?);
    record.result = Some(impact.result.clone());
    record.steps = vec![format!("{op} {args}")];
    record.impact = Some(impact.clone());
    ecad_project::save_locked(&project, dir, state::Extras { workflow: vec![record.file()] }).map_err(project_err)?;
    Ok(Automated { impact, result: lowered.result, record: Some(record) })
}

/// Approvals still issued, revoked (rewritten) because `reason`: for an acceptance of outside
/// changes, which invalidates them (D43).
pub fn revoke_all(dir: &Path, reason: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    Ok(approvals(dir)?
        .into_iter()
        .filter(|a| a.state == ApprovalState::Issued)
        .map(|mut a| {
            a.state = ApprovalState::Revoked { reason: reason.to_owned(), at: now() };
            json_file(approval_path(&a.id), &a)
        })
        .collect())
}

/// An approval, as a workflow file.
pub fn approval_file(a: &Approval) -> (String, Vec<u8>) {
    json_file(approval_path(&a.id), a)
}

/// Group a map of named values for display, keeping order.
pub fn items_by_change(items: &[Item]) -> BTreeMap<String, Vec<&Item>> {
    let mut out: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for i in items {
        out.entry(i.change.clone()).or_default().push(i);
    }
    out
}
