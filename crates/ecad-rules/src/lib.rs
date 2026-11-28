//! Checks on the electrical model. They run on every change in the editor and with
//! `ecad check` in the CLI, and only read the model: fixing what they find is up to the user.
//!
//! Some mistakes never reach here because the model refuses them outright: duplicate device
//! tags, wire numbers, terminal and channel numbers, PLC addresses and symbols, wires to a point
//! that is already wired.

pub mod release;
use ecad_model::{ConnectionId, ConnectionRef, FunctionId, PageId, Point, Project, Role};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The drawing is wrong: fix before building.
    Error,
    /// Probably unfinished or worth a second look.
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// What a check points at, so the editor can show and select it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum Target {
    Function(FunctionId),
    Connection(ConnectionId),
    /// A rail, duct or device footprint on a layout page.
    PanelItem(ecad_model::PanelItemId),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Check {
    /// Stable rule name, e.g. `unconnected-point`. See [`RULES`].
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
    pub target: Target,
    pub page: PageId,
    /// Page and column, e.g. `2.4`.
    pub location: String,
}

/// Every rule, with its severity and what it looks for.
pub const RULES: &[(&str, Severity, &str)] = &[
    ("potential-clash", Severity::Error, "Differently named potentials (L+ and M) are connected"),
    (
        "wire-end-missing",
        Severity::Error,
        "A wire ends on a connection point its symbol no longer has: the netlist is incomplete",
    ),
    ("wire-number-shared", Severity::Error, "One wire number is used on two different potentials"),
    (
        "wire-numbers-differ",
        Severity::Warning,
        "With one number per potential: a potential's wires carry different numbers",
    ),
    ("unconnected-point", Severity::Warning, "A connection point has no wire"),
    ("junction-unused", Severity::Warning, "A junction joins fewer than three wires"),
    (
        "shared-designation",
        Severity::Warning,
        "Two drawn points of one device print the same designation (drawn twice, or numbered twice?); they are kept apart until reconciled",
    ),
    (
        "junction-drawn-twice",
        Severity::Warning,
        "A junction is drawn more than once (only possible behind the commands' back)",
    ),
    (
        "mixed-logical-roles",
        Severity::Warning,
        "A node is drawn with a symbol of another kind (only possible behind the commands' back)",
    ),
    ("lonely-interruption", Severity::Warning, "An interruption point has no partner with the same name"),
    ("wire-unnumbered", Severity::Warning, "A wire has no number"),
    ("missing-part", Severity::Warning, "A device has no catalogue part, so it is missing from the BOM"),
    ("plc-no-address", Severity::Warning, "A PLC channel has no address"),
    (
        "contact-not-on-part",
        Severity::Warning,
        "A contact's numbers aren't on any contact block of its device's parts (too many contacts drawn?)",
    ),
    ("cable-cores", Severity::Warning, "A cable carries more wires than it has cores"),
    ("overlap", Severity::Warning, "Two symbols overlap on a page"),
    (
        "wire-overlap",
        Severity::Warning,
        "Two wires run on top of each other, side by side too close, or line up end to end",
    ),
    ("wire-through-symbol", Severity::Warning, "A wire runs through or along a symbol it isn't connected to"),
    ("panel-overlap", Severity::Warning, "Two devices overlap on a mounting plate"),
    ("panel-off-plate", Severity::Warning, "A rail, duct or device sticks out of its mounting plate"),
    (
        "panel-no-size",
        Severity::Warning,
        "A device on a mounting plate has no part with a size, so its footprint is a guess",
    ),
    ("wire-not-drawn", Severity::Warning, "A wire joins two ends but isn't drawn on any page yet"),
];

fn severity(rule: &str) -> Severity {
    RULES.iter().find(|(r, ..)| *r == rule).map_or(Severity::Warning, |(_, s, _)| *s)
}

struct Out<'a> {
    project: &'a Project,
    checks: Vec<Check>,
}

impl Out<'_> {
    fn at_function(&mut self, rule: &'static str, f: FunctionId, message: String) {
        let Ok(func) = self.project.function(f) else { return };
        let location = self.project.function_location(f).unwrap_or_default();
        let page = func.page;
        self.checks.push(Check {
            rule,
            severity: severity(rule),
            message,
            target: Target::Function(f),
            page,
            location,
        });
    }

    fn at_connection(&mut self, rule: &'static str, c: ConnectionId, message: String) {
        let project = self.project;
        let (page, location) = if let Ok(conn) = project.connection(c) {
            let Ok(from) = project.function(conn.from.function) else { return };
            (from.page, project.location(from.page, from.position).unwrap_or_default())
        } else if let Some(w) = project.undrawn.get(&c) {
            let connectivity = project.connectivity();
            let page = [w.from, w.to]
                .iter()
                .find_map(|e| {
                    connectivity.drawings(*e).first().and_then(|r| project.function(r.function).ok()).map(|f| f.page)
                })
                .or_else(|| project.pages_in_order().first().map(|p| p.id))
                .unwrap_or_else(|| PageId::from_u128(0));
            (page, String::new())
        } else {
            return;
        };
        self.checks.push(Check {
            rule,
            severity: severity(rule),
            message,
            target: Target::Connection(c),
            page,
            location,
        });
    }
}

impl Out<'_> {
    /// Located by the layout page's name.
    fn at_panel(&mut self, rule: &'static str, id: ecad_model::PanelItemId, message: String) {
        let Some(item) = self.project.panel_items.get(&id) else { return };
        let location = self.project.page(item.page).map(|p| p.name.clone()).unwrap_or_default();
        self.checks.push(Check {
            rule,
            severity: severity(rule),
            message,
            target: Target::PanelItem(id),
            page: item.page,
            location,
        });
    }
}

fn point(project: &Project, r: &ConnectionRef) -> String {
    project.describe(r).map(|(tag, p)| if p.is_empty() { tag } else { format!("{tag}:{p}") }).unwrap_or_default()
}

/// A junction: a bridged symbol whose points all sit in one place.
fn is_junction(project: &Project, f: FunctionId) -> bool {
    project
        .function(f)
        .and_then(|f| project.symbol(&f.symbol))
        .is_ok_and(|s| s.bridged && s.connection_points.len() > 2)
}

/// What the derived connectivity couldn't decide (`ecad_model::connectivity`), at the first
/// drawing of each point involved after the first.
fn ambiguities(out: &mut Out) {
    use ecad_model::connectivity::Ambiguity;
    let c = out.project.connectivity();
    for a in &c.ambiguities {
        match a {
            Ambiguity::SharedDesignation { device, designation, terminals, .. } => {
                let tag = out.project.device(*device).map(|d| d.tag.clone()).unwrap_or_default();
                let places: Vec<String> = terminals
                    .iter()
                    .filter_map(|t| c.terminals.get(t)?.drawings.first())
                    .map(|r| out.project.function_location(r.function).unwrap_or_default())
                    .collect();
                let message = format!("{tag}:{designation} is drawn at {} as different points", places.join(", "));
                let firsts: Vec<FunctionId> = terminals
                    .iter()
                    .skip(1)
                    .filter_map(|t| c.terminals.get(t)?.drawings.first())
                    .map(|r| r.function)
                    .collect();
                for f in firsts {
                    out.at_function("shared-designation", f, message.clone());
                }
            }
            Ambiguity::MixedLogicalRoles { node } => {
                let tag = c.nodes.get(node).map(|n| n.name.clone()).unwrap_or_default();
                let message = format!("{tag} is drawn with a symbol of another kind of node");
                if let Some(r) = c.nodes.get(node).and_then(|n| n.drawings.first()) {
                    out.at_function("mixed-logical-roles", r.function, message);
                }
            }
            Ambiguity::JunctionDrawnTwice { drawings, .. } => {
                let message = format!("A junction is drawn {} times", drawings.len());
                for f in drawings.iter().skip(1) {
                    out.at_function("junction-drawn-twice", *f, message.clone());
                }
            }
        }
    }
}

/// Wires whose ends the netlist couldn't resolve.
fn unresolved_ends(out: &mut Out) {
    for u in out.project.netlist().unresolved {
        let tag = out.project.function(u.end.function).ok().map(|f| out.project.label_of(f).to_owned());
        let message =
            format!("A wire ends on {}:{}, which its symbol no longer has", tag.unwrap_or_default(), u.end.point);
        out.at_connection("wire-end-missing", u.wire, message);
    }
}

/// Run every check, errors first, then by page and column.
pub fn check(project: &Project) -> Vec<Check> {
    let mut out = Out { project, checks: Vec::new() };
    potential_clashes(&mut out);
    wire_numbers(&mut out);
    unconnected(&mut out);
    lonely_interruptions(&mut out);
    undrawn_wires(&mut out);
    ambiguities(&mut out);
    unresolved_ends(&mut out);
    for c in project.connections.values() {
        if c.wire_number.is_none() {
            let message = format!("Wire {} – {} has no number", point(project, &c.from), point(project, &c.to));
            out.at_connection("wire-unnumbered", c.id, message);
        }
    }
    for d in project.devices.values() {
        if d.parts.is_empty()
            && project.needs_part(d.id)
            && let Some(f) = project.functions_of(d.id).next()
        {
            out.at_function("missing-part", f.id, format!("{} has no part", d.tag));
        }
    }
    for f in project.functions.values() {
        if matches!(project.role(f.id), Ok(Role::PlcChannel { .. }))
            && project.property(f, ecad_model::ADDRESS).is_empty()
        {
            let n = project.drawing_number(f.id).unwrap_or_default();
            let tag = project.label_of(f).to_owned();
            out.at_function("plc-no-address", f.id, format!("{tag}:{n} has no PLC address"));
        }
    }
    overlaps(&mut out);
    wire_layout(&mut out);
    contacts_on_parts(&mut out);
    panels(&mut out);
    for cable in project.cables.values() {
        let wires: Vec<ConnectionId> = project
            .connections
            .values()
            .filter(|c| c.spec.cable.as_ref().is_some_and(|k| k.cable == cable.id))
            .map(|c| c.id)
            .chain(
                project
                    .undrawn
                    .values()
                    .filter(|w| w.spec.cable.as_ref().is_some_and(|k| k.cable == cable.id))
                    .map(|w| w.id),
            )
            .collect();
        if cable.cores > 0
            && wires.len() > cable.cores as usize
            && let Some(w) = wires.first()
        {
            let message = format!("Cable {} has {} cores but carries {} wires", cable.tag, cable.cores, wires.len());
            out.at_connection("cable-cores", *w, message);
        }
    }

    let order: BTreeMap<PageId, usize> = project.pages_in_order().iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    // Sorted by where they are: column, then row, as a reader scans a page.
    let place = |c: &Check| {
        let f = match c.target {
            Target::Function(f) => Some(f),
            Target::Connection(id) => project.connection(id).ok().map(|c| c.from.function),
            Target::PanelItem(_) => None,
        };
        let at = f.and_then(|f| project.function(f).ok()).map_or(Point::ZERO, |f| f.position);
        (project.sheet.column_of(at), project.sheet.row_of(at).unwrap_or(0))
    };
    let mut checks = out.checks;
    checks.sort_by(|a, b| {
        (a.severity, order.get(&a.page), place(a), &a.message).cmp(&(
            b.severity,
            order.get(&b.page),
            place(b),
            &b.message,
        ))
    });
    checks
}

fn wire_numbers(out: &mut Out) {
    let project = out.project;
    let potentials = project.potentials();
    let mut owner: BTreeMap<&str, (usize, ConnectionId)> = BTreeMap::new();
    let mut reported = BTreeSet::new();
    for (i, p) in potentials.iter().enumerate() {
        let mut numbers: Vec<(&str, ConnectionId)> =
            p.connections.iter().filter_map(|c| project.wire_number(*c).map(|n| (n, *c))).collect();
        numbers.sort();
        let distinct: BTreeSet<&str> = numbers.iter().map(|(n, _)| *n).collect();
        if project.naming.wire_numbering == ecad_model::WireNumbering::PerPotential && distinct.len() > 1 {
            let list = distinct.iter().copied().collect::<Vec<_>>().join(", ");
            if let Some((_, c)) = numbers.first() {
                out.at_connection("wire-numbers-differ", *c, format!("One potential carries wire numbers {list}"));
            }
        }
        for (n, c) in numbers {
            match owner.get(n) {
                None => {
                    owner.insert(n, (i, c));
                }
                Some((j, _)) if *j != i && reported.insert(n) => {
                    out.at_connection("wire-number-shared", c, format!("Wire number {n} is used on two potentials"));
                }
                _ => {}
            }
        }
    }
}

fn potential_clashes(out: &mut Out) {
    for p in out.project.potentials() {
        let Some(message) = p.conflict() else { continue };
        let message = format!("Potentials {message}");
        if let Some(c) = p.connections.first() {
            out.at_connection("potential-clash", *c, message);
        } else if let Some(r) = p.points.first() {
            out.at_function("potential-clash", r.function, message);
        }
    }
}

fn unconnected(out: &mut Out) {
    let project = out.project;
    let mut wired: BTreeMap<FunctionId, BTreeSet<&str>> = BTreeMap::new();
    for c in project.connections.values() {
        for r in [&c.from, &c.to] {
            wired.entry(r.function).or_default().insert(r.point.as_str());
        }
    }
    for f in project.functions.values() {
        let Ok(sym) = project.symbol(&f.symbol) else { continue };
        let used = wired.get(&f.id);
        let count = used.map_or(0, BTreeSet::len);
        if is_junction(project, f.id) {
            if count < 3 {
                let tag = project.label_of(f).to_owned();
                let message = format!("Junction {tag} joins {count} wire{}", if count == 1 { "" } else { "s" });
                out.at_function("junction-unused", f.id, message);
            }
            continue;
        }
        for cp in &sym.connection_points {
            if !used.is_some_and(|u| u.contains(cp.name.as_str())) {
                let message = format!("{} is not connected", point(project, &ConnectionRef::new(f.id, &cp.name)));
                out.at_function("unconnected-point", f.id, message);
            }
        }
    }
}

/// Wires not drawn yet (D1): listed, at the first page one of their ends is drawn on.
fn undrawn_wires(out: &mut Out) {
    let project = out.project;
    for w in project.undrawn.values() {
        let end = |e| {
            let (tag, d) = project.describe_end(e);
            if d.is_empty() { tag } else { format!("{tag}:{d}") }
        };
        let name = w.wire_number.clone().unwrap_or_else(|| "A wire".into());
        out.at_connection(
            "wire-not-drawn",
            w.id,
            format!("{name} from {} to {} is not drawn yet", end(&w.from), end(&w.to)),
        );
        if w.wire_number.is_none() {
            out.at_connection("wire-unnumbered", w.id, format!("Wire {} – {} has no number", end(&w.from), end(&w.to)));
        }
    }
}

fn lonely_interruptions(out: &mut Out) {
    let project = out.project;
    // An interruption node's drawings (D1): bound by id, not by name.
    let mut drawings: BTreeMap<ecad_model::NodeId, Vec<FunctionId>> = BTreeMap::new();
    for f in project.functions.values() {
        if project.role(f.id) == Ok(Role::Interruption)
            && let Some(n) = f.node
        {
            drawings.entry(n).or_default().push(f.id);
        }
    }
    for fs in drawings.values() {
        if let [only] = fs.as_slice() {
            let tag = project.function(*only).map(|f| project.label_of(f).to_owned()).unwrap_or_default();
            out.at_function("lonely-interruption", *only, format!("Interruption point {tag} has no partner"));
        }
    }
}

/// Layout pages: footprints that overlap or leave the plate, and devices drawn at a guessed size.
fn panels(out: &mut Out) {
    let project = out.project;
    let name = |id: &ecad_model::PanelItemId| {
        project.panel_items.get(id).map_or_else(String::new, |i| match &i.kind {
            ecad_model::PanelItemKind::Device { device } => {
                project.device(*device).map(|d| d.tag.clone()).unwrap_or_default()
            }
            ecad_model::PanelItemKind::Rail => format!("A rail ({} mm)", i.length),
            ecad_model::PanelItemKind::Duct { width, depth } => format!("A duct {width} x {depth} ({} mm)", i.length),
        })
    };
    for page in project.panels.keys() {
        for (a, b) in project.panel_overlaps(*page) {
            out.at_panel("panel-overlap", a, format!("{} overlaps {}", name(&a), name(&b)));
        }
        for id in project.panel_off_plate(*page) {
            out.at_panel("panel-off-plate", id, format!("{} sticks out of the plate", name(&id)));
        }
        for item in project.panel_items_on(*page) {
            if let ecad_model::PanelItemKind::Device { device } = item.kind
                && !project.device_size(device).1
            {
                out.at_panel("panel-no-size", item.id, format!("{} has no part with a size", name(&item.id)));
            }
        }
    }
}

/// Contacts drawn for a device whose parts list contact blocks must be one of those blocks (or
/// one side of a changeover block).
fn contacts_on_parts(out: &mut Out) {
    let project = out.project;
    for d in project.devices.values() {
        let blocks = project.contact_blocks_of(d.id);
        if blocks.is_empty() {
            continue;
        }
        for f in project.functions_of(d.id) {
            let Ok(Role::Contact { kind }) = project.role(f.id) else { continue };
            if kind == ecad_model::ContactKind::Power {
                continue;
            }
            let Ok(sym) = project.symbol(&f.symbol) else { continue };
            let numbers: Vec<&str> = sym.connection_points.iter().map(|cp| project.designation(f, &cp.name)).collect();
            let fits = blocks.iter().any(|b| {
                let b: Vec<&str> = b.iter().map(String::as_str).collect();
                numbers == b || matches!(b.as_slice(), [c, nc, no] if numbers == [*c, *no] || numbers == [*c, *nc])
            });
            if !fits {
                let list = blocks.iter().map(|b| b.join("-")).collect::<Vec<_>>().join(" ");
                let message = format!("{} contact {} isn't on its parts ({list})", d.tag, numbers.join("-"));
                out.at_function("contact-not-on-part", f.id, message);
            }
        }
    }
}

/// Wires that a reader could take for one another or for part of a symbol ("Tidy wires" in the
/// editor fixes most).
fn wire_layout(out: &mut Out) {
    let project = out.project;
    let wire = |c: ConnectionId| {
        project.connections.get(&c).map_or_else(String::new, |w| match &w.wire_number {
            Some(n) => n.clone(),
            None => format!("{} – {}", point(project, &w.from), point(project, &w.to)),
        })
    };
    for page in project.pages.keys() {
        for clash in project.wire_clashes(*page) {
            match clash {
                ecad_model::layout::WireClash::Overlap { a, b, .. } => {
                    let message = format!("Wire {} runs on or close to wire {}", wire(a), wire(b));
                    out.at_connection("wire-overlap", a, message);
                }
                ecad_model::layout::WireClash::ThroughSymbol { wire: w, function, .. } => {
                    let tag = project.function(function).map(|f| project.label_of(f).to_owned()).unwrap_or_default();
                    out.at_connection("wire-through-symbol", w, format!("Wire {} runs through {tag}", wire(w)));
                }
            }
        }
    }
}

fn overlaps(out: &mut Out) {
    let project = out.project;
    // Symbols may touch (a contact's stub on a terminal's) but not cross.
    const SLACK: f64 = 0.5;
    for page in project.pages.keys() {
        let boxes: Vec<(FunctionId, Point, Point)> = project
            .functions_on(*page)
            .filter(|f| !is_junction(project, f.id))
            .filter_map(|f| {
                let (lo, hi) = project.function_bounds(f.id).ok()??;
                Some((f.id, Point::new(lo.x + SLACK, lo.y + SLACK), Point::new(hi.x - SLACK, hi.y - SLACK)))
            })
            .collect();
        for (i, (a, alo, ahi)) in boxes.iter().enumerate() {
            for (b, blo, bhi) in &boxes[i + 1..] {
                if alo.x < bhi.x && blo.x < ahi.x && alo.y < bhi.y && blo.y < ahi.y {
                    let tag = |f: &FunctionId| {
                        let n = project.drawing_number(*f).map(|n| format!(":{n}")).unwrap_or_default();
                        project.function(*f).map(|f| format!("{}{n}", project.label_of(f))).unwrap_or_default()
                    };
                    out.at_function("overlap", *a, format!("{} overlaps {}", tag(a), tag(b)));
                }
            }
        }
    }
}

/// `error 1.3  -K1:A1 is not connected`, one line per check.
pub fn summary(checks: &[Check]) -> String {
    checks.iter().map(|c| format!("{:<7} {:<6} {}\n", c.severity.as_str(), c.location, c.message)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::{Command, Connection, sample};

    fn rules(checks: &[Check]) -> Vec<String> {
        checks.iter().map(|c| format!("{} {} {}", c.rule, c.location, c.message)).collect()
    }

    #[test]
    fn the_finished_start_circuit_is_clean() {
        let p = sample::start_circuit().unwrap();
        assert_eq!(rules(&check(&p)), Vec::<String>::new());
    }

    #[test]
    fn finds_each_kind_of_problem() {
        let mut p = sample::with_plc().unwrap();
        // Connect L+ to M through a new wire: a clash.
        let w1 = p.connections.values().find(|c| c.wire_number.as_deref() == Some("W1")).unwrap().clone();
        p.apply(Command::Disconnect(w1.id)).unwrap();
        let m = ConnectionRef::new(ecad_model::FunctionId::from_u128(0x204), "1");
        p.apply(Command::Disconnect(p.connection_at(&m).unwrap().id)).unwrap();
        p.apply(Command::Connect(Connection {
            bend: None,
            id: ConnectionId::new(),
            from: w1.from.clone(),
            to: m,
            wire_number: None,
            spec: Default::default(),
        }))
        .unwrap();
        // An analog channel without an address.
        let ai = ecad_model::FunctionId::from_u128(0x216);
        p.apply(Command::SetProperties { id: ai, properties: Default::default() }).unwrap();
        let checks = check(&p);
        let found: BTreeSet<&str> = checks.iter().map(|c| c.rule).collect();
        for (rule, ..) in RULES {
            // Covered by their own tests.
            let elsewhere = [
                "lonely-interruption",
                "overlap",
                "wire-number-shared",
                "wire-numbers-differ",
                "cable-cores",
                "contact-not-on-part",
                "panel-overlap",
                "panel-off-plate",
                "panel-no-size",
                "shared-designation",
                "junction-drawn-twice",
                "mixed-logical-roles",
                "wire-end-missing",
                "wire-not-drawn",
            ];
            if !elsewhere.contains(rule) {
                assert!(found.contains(rule), "{rule} missing from {:#?}", rules(&checks));
            }
        }
        assert_eq!(checks[0].rule, "potential-clash", "errors first");
        assert_eq!(checks[0].message, "Potentials L+ and M are connected to each other");
        assert!(
            rules(&checks).contains(&"unconnected-point 1.1 -X2:1 is not connected".to_owned())
                || rules(&checks).iter().any(|r| r.ends_with("-X2:1 is not connected")),
            "{:#?}",
            rules(&checks)
        );
        assert!(rules(&checks).iter().any(|r| r.contains("-KF1:3 is not connected")));
        assert!(!rules(&checks).iter().any(|r| r.contains("I1")), "I1 has two drawings");
    }

    #[test]
    fn wire_numbers_shared_between_potentials_or_mixed_on_one() {
        let mut p = sample::start_circuit().unwrap();
        p.apply(Command::SetNaming(ecad_model::Naming::by_page())).unwrap();
        let wire =
            |p: &Project, n: &str| p.connections.values().find(|c| c.wire_number.as_deref() == Some(n)).unwrap().id;
        // W1 (L+ to -S1) and W2 (-S1 to -K1) are different potentials: give W2 the number W1.
        let w2 = wire(&p, "W2");
        p.apply(Command::SetWireNumber { id: w2, number: Some("W1".into()) }).unwrap();
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r.contains("Wire number W1 is used on two potentials")), "{r:#?}");
        // The sample's per-wire numbers are fine per potential: each potential has one wire.
        assert!(!r.iter().any(|r| r.starts_with("wire-numbers-differ")), "{r:#?}");
    }

    #[test]
    fn wires_on_top_of_each_other_are_reported() {
        let mut p = sample::start_circuit().unwrap();
        // Move the coil right so W2 and W3 bend, then put W2's middle run on W3's.
        p.apply(Command::Move { id: sample::K1_COIL, to: Point::new(16.0, 12.0) }).unwrap();
        assert!(!rules(&check(&p)).iter().any(|r| r.starts_with("wire-")), "tidy before");
        let w = |n: &str| p.connections.values().find(|c| c.wire_number.as_deref() == Some(n)).unwrap().id;
        let (w2, w3) = (w("W2"), w("W3"));
        let route3 = p.route(p.connection(w3).unwrap()).unwrap();
        let y = route3.windows(2).find(|s| s[0].y == s[1].y).unwrap()[0].y;
        p.apply(Command::SetWireBend { id: w2, bend: Some(y) }).unwrap();
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r.starts_with("wire-overlap") && r.contains("W2") && r.contains("W3")), "{r:#?}");
    }

    #[test]
    fn a_contact_the_parts_dont_have_is_reported() {
        let mut p = sample::with_contacts().unwrap();
        let k1 = p.function(sample::K1_COIL).unwrap().device;
        assert!(!rules(&check(&p)).iter().any(|r| r.starts_with("contact-not-on-part")), "no blocks, no check");
        p.apply(Command::AddPart(ecad_model::Part {
            contacts: vec!["13-14".into()],
            ..ecad_model::Part::new("CT-1NO")
        }))
        .unwrap();
        p.apply(Command::SetDeviceParts { id: k1, parts: vec![ecad_model::PartRef::new("CT-1NO", 1)] }).unwrap();
        let r: Vec<String> = rules(&check(&p)).into_iter().filter(|r| r.starts_with("contact-not-on-part")).collect();
        assert_eq!(r.len(), 1, "{r:#?}");
        assert!(r[0].ends_with("-K1 contact 21-22 isn't on its parts (13-14)"), "{r:#?}");
    }

    #[test]
    fn devices_overlapping_or_off_the_plate_are_reported() {
        let mut p = ecad_model::reference::reference_project().unwrap();
        assert!(!rules(&check(&p)).iter().any(|r| r.starts_with("panel-")), "the reference layout is clean");
        let find = |p: &ecad_model::Project, tag: &str| {
            let d = p.device_by_tag(tag).unwrap().id;
            p.panel_item_of(d).unwrap().clone()
        };
        // -P2QF3 onto -P2QF1, -X0 off the plate's left edge.
        let qf1 = find(&p, "P2QF1");
        let qf3 = find(&p, "P2QF3");
        p.apply(Command::UpdatePanelItem(ecad_model::PanelItem { at: qf1.at, ..qf3 })).unwrap();
        let x0 = find(&p, "X0");
        p.apply(Command::UpdatePanelItem(ecad_model::PanelItem { at: Point::new(-10.0, x0.at.y), ..x0 })).unwrap();
        let r = rules(&check(&p));
        assert!(
            r.iter()
                .any(|r| r == "panel-overlap L7 P2QF1 overlaps P2QF3" || r == "panel-overlap L7 P2QF3 overlaps P2QF1"),
            "{r:#?}"
        );
        assert!(r.iter().any(|r| r == "panel-off-plate L7 X0 sticks out of the plate"), "{r:#?}");
        // The thermostat's part loses its size: its footprint is a guess.
        let mut tmf = p.part_by_number("TMF").unwrap().clone();
        (tmf.width, tmf.height) = (None, None);
        p.apply(Command::UpdatePart(tmf)).unwrap();
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r == "panel-no-size L7 P3S1 has no part with a size"), "{r:#?}");
    }

    #[test]
    fn lonely_interruptions_junctions_and_overlaps() {
        let mut p = sample::with_potentials().unwrap();
        // Remove I1's drawing on page 1 (and its wire): the one on page 2 is left alone.
        let i1 = ecad_model::FunctionId::from_u128(0x209);
        let w6 = p.connection_at(&ConnectionRef::new(i1, "1")).unwrap().id;
        p.apply(Command::Disconnect(w6)).unwrap();
        p.apply(Command::Unplace(i1)).unwrap();
        // Move -K2 onto -K1.
        let k2 = ecad_model::FunctionId::from_u128(0x20a);
        p.apply(Command::Move { id: k2, to: Point::new(2.0, 12.0) }).unwrap();
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r.starts_with("lonely-interruption 2.") && r.ends_with("Interruption point I1 has no partner")), "{r:#?}");
        assert!(r.iter().any(|r| r.contains("-K1 overlaps -K2") || r.contains("-K2 overlaps -K1")), "{r:#?}");
        // The junction still joins three wires: W3, W7 and W8.
        assert!(!r.iter().any(|r| r.starts_with("junction-unused")), "{r:#?}");
    }

    #[test]
    fn a_designation_drawn_twice_is_reported_not_merged() {
        let mut p = sample::with_contacts().unwrap();
        let k1 = ecad_model::DeviceId::from_u128(0x103);
        let f = ecad_model::Function {
            node: None,
            terminals: Default::default(),
            id: FunctionId::new(),
            device: k1,
            symbol: ecad_model::SymbolId::new("iec.coil"),
            page: sample::PAGE_1,
            position: Point::new(60.0, 90.0),
            rotation: Default::default(),
            mirrored: false,
            designations: Default::default(),
            properties: Default::default(),
        };
        p.place(f).unwrap();
        let r = rules(&check(&p));
        let shared: Vec<&String> = r.iter().filter(|r| r.starts_with("shared-designation")).collect();
        assert_eq!(shared.len(), 2, "A1 and A2: {r:#?}");
        assert!(shared[0].contains("-K1:A") && shared[0].contains("as different points"), "{shared:?}");

        // A junction drawn a second time: refused by the commands, reported if it gets in.
        let mut p = sample::with_potentials().unwrap();
        let j = p.function(sample::JUNCTION_1).unwrap().clone();
        let again = ecad_model::Function { id: FunctionId::new(), position: Point::new(120.0, -8.0), ..j };
        assert_eq!(p.place(again.clone()).unwrap_err(), ecad_model::ModelError::JunctionDrawnTwice);
        p.functions.insert(again.id, again);
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r.starts_with("junction-drawn-twice") && r.contains("drawn 2 times")), "{r:#?}");
    }
    #[test]
    fn a_wire_not_drawn_yet_is_listed() {
        let mut p = sample::start_circuit().unwrap();
        let c = p.connectivity();
        let k1 = |pt: &str| c.endpoint(&ConnectionRef::new(sample::K1_COIL, pt)).unwrap();
        let s1 = c.endpoint(&ConnectionRef::new(ecad_model::FunctionId::from_u128(0x202), "13")).unwrap();
        let w = ecad_model::Wire {
            id: ConnectionId::new(),
            from: s1,
            to: k1("A2"),
            wire_number: Some("W9".into()),
            spec: Default::default(),
        };
        p.apply(Command::AddWire(w)).unwrap();
        let checks = check(&p);
        let found: Vec<&Check> = checks.iter().filter(|c| c.rule == "wire-not-drawn").collect();
        assert_eq!(found.len(), 1, "{:#?}", rules(&checks));
        assert_eq!(found[0].message, "W9 from -S1:13 to -K1:A2 is not drawn yet");
        assert_eq!(found[0].page, sample::PAGE_1);
    }

    #[test]
    fn a_wire_end_its_symbol_lost_is_an_error() {
        let mut p = sample::start_circuit().unwrap();
        let mut coil = p.symbol(&ecad_model::SymbolId::new("iec.coil")).unwrap().clone();
        coil.connection_points.retain(|cp| cp.name != "A2");
        p.add_symbol(coil);
        let checks = check(&p);
        let missing: Vec<&Check> = checks.iter().filter(|c| c.rule == "wire-end-missing").collect();
        assert_eq!(missing.len(), 1, "{:#?}", rules(&checks));
        assert_eq!(missing[0].severity, Severity::Error);
        assert!(missing[0].message.contains("-K1:A2"), "{}", missing[0].message);
    }

    #[test]
    fn a_node_drawn_with_a_symbol_of_another_kind_is_refused_or_reported() {
        let mut p = sample::start_circuit().unwrap();
        let l_plus = p.node_by_name(ecad_model::NodeKind::Potential, "L+", ecad_model::PROJECT_SCOPE).unwrap().id;
        let f = ecad_model::Function {
            node: Some(l_plus),
            terminals: Default::default(),
            id: FunctionId::new(),
            device: ecad_model::DeviceId::NONE,
            symbol: ecad_model::SymbolId::new("iec.interruption"),
            page: sample::PAGE_1,
            position: Point::new(100.0, 90.0),
            rotation: Default::default(),
            mirrored: false,
            designations: Default::default(),
            properties: Default::default(),
        };
        assert!(p.place(f.clone()).is_err());
        p.functions.insert(f.id, f);
        let r = rules(&check(&p));
        assert!(r.iter().any(|r| r.starts_with("mixed-logical-roles") && r.contains("L+ is drawn with")), "{r:#?}");
    }
}
