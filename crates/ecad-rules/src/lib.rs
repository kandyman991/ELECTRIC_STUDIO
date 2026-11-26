//! Checks on the electrical model. They run on every change in the editor and with
//! `ecad check` in the CLI, and only read the model: fixing what they find is up to the user.
//!
//! Some mistakes never reach here because the model refuses them outright: duplicate device
//! tags, wire numbers, terminal and channel numbers, PLC addresses and symbols, wires to a point
//! that is already wired.

use ecad_model::{ConnectionId, ConnectionRef, DeviceId, FunctionId, PageId, Point, Project, Role};
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
    ("wire-number-shared", Severity::Error, "One wire number is used on two different potentials"),
    (
        "wire-numbers-differ",
        Severity::Warning,
        "With one number per potential: a potential's wires carry different numbers",
    ),
    ("unconnected-point", Severity::Warning, "A connection point has no wire"),
    ("junction-unused", Severity::Warning, "A junction joins fewer than three wires"),
    ("lonely-interruption", Severity::Warning, "An interruption point has no partner with the same name"),
    ("wire-unnumbered", Severity::Warning, "A wire has no number"),
    ("missing-part", Severity::Warning, "A device has no catalogue part, so it is missing from the BOM"),
    ("plc-no-address", Severity::Warning, "A PLC channel has no address"),
    ("cable-cores", Severity::Warning, "A cable carries more wires than it has cores"),
    ("overlap", Severity::Warning, "Two symbols overlap on a page"),
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
        let Ok(conn) = self.project.connection(c) else { return };
        let Ok(from) = self.project.function(conn.from.function) else { return };
        // Located at the wire's start, as the wire list shows it.
        let location = self.project.location(from.page, from.position).unwrap_or_default();
        let page = from.page;
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

/// Run every check, errors first, then by page and column.
pub fn check(project: &Project) -> Vec<Check> {
    let mut out = Out { project, checks: Vec::new() };
    potential_clashes(&mut out);
    wire_numbers(&mut out);
    unconnected(&mut out);
    lonely_interruptions(&mut out);
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
        if matches!(project.role(f.id), Ok(Role::PlcChannel { .. })) && f.property(ecad_model::ADDRESS).is_empty() {
            let n = project.drawing_number(f.id).unwrap_or_default();
            let tag = project.device(f.device).map(|d| d.tag.clone()).unwrap_or_default();
            out.at_function("plc-no-address", f.id, format!("{tag}:{n} has no PLC address"));
        }
    }
    overlaps(&mut out);
    for cable in project.cables.values() {
        let wires: Vec<&ecad_model::Connection> = project
            .connections
            .values()
            .filter(|c| c.spec.cable.as_ref().is_some_and(|k| k.cable == cable.id))
            .collect();
        if cable.cores > 0
            && wires.len() > cable.cores as usize
            && let Some(w) = wires.first()
        {
            let message = format!("Cable {} has {} cores but carries {} wires", cable.tag, cable.cores, wires.len());
            out.at_connection("cable-cores", w.id, message);
        }
    }

    let order: BTreeMap<PageId, usize> = project.pages_in_order().iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    // Sorted by where they are: column, then row, as a reader scans a page.
    let place = |c: &Check| {
        let f = match c.target {
            Target::Function(f) => Some(f),
            Target::Connection(id) => project.connection(id).ok().map(|c| c.from.function),
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
        let mut numbers: Vec<(&str, ConnectionId)> = p
            .connections
            .iter()
            .filter_map(|c| project.connection(*c).ok())
            .filter_map(|c| c.wire_number.as_deref().map(|n| (n, c.id)))
            .collect();
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
                let tag = project.device(f.device).map(|d| d.tag.clone()).unwrap_or_default();
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

fn lonely_interruptions(out: &mut Out) {
    let project = out.project;
    let mut drawings: BTreeMap<DeviceId, Vec<FunctionId>> = BTreeMap::new();
    for f in project.functions.values() {
        if project.role(f.id) == Ok(Role::Interruption) {
            drawings.entry(f.device).or_default().push(f.id);
        }
    }
    for (device, fs) in drawings {
        if let [only] = fs.as_slice() {
            let tag = project.device(device).map(|d| d.tag.clone()).unwrap_or_default();
            out.at_function("lonely-interruption", *only, format!("Interruption point {tag} has no partner"));
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
                        project
                            .function(*f)
                            .and_then(|f| project.device(f.device))
                            .map(|d| format!("{}{n}", d.tag))
                            .unwrap_or_default()
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
            let elsewhere =
                ["lonely-interruption", "overlap", "wire-number-shared", "wire-numbers-differ", "cable-cores"];
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
}
