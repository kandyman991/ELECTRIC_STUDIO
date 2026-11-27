//! Potentials: sets of connection points that are electrically the same, across all pages.
//!
//! Points are joined by wires, inside bridged symbols (junctions) and a symbol's bridges (a black
//! box's joined terminals), and across all drawings of a
//! potential or interruption device. A potential is named by the potential points on it (their
//! device tags, `L+`, `M`…); two different names on one potential means they are connected,
//! which is almost always a mistake (L+ shorted to M).

use crate::project::{ConnectionId, ConnectionRef, FunctionId, Project};
#[cfg(test)]
use crate::symbol::Role;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Potential {
    /// Names of the potential points on it, sorted. Usually one; none for an unnamed potential
    /// (e.g. between a switch and a coil); more than one is a conflict.
    pub names: Vec<String>,
    /// Every connection point on it.
    pub points: Vec<ConnectionRef>,
    /// Every wire on it.
    pub connections: Vec<ConnectionId>,
}

impl Potential {
    /// The name to show: the single name, the names joined with ` / ` for a conflict, or
    /// `None`.
    pub fn name(&self) -> Option<String> {
        if self.names.is_empty() { None } else { Some(self.names.join(" / ")) }
    }

    /// A warning when different potentials are connected.
    pub fn conflict(&self) -> Option<String> {
        (self.names.len() > 1).then(|| format!("{} are connected to each other", self.names.join(" and ")))
    }
}

/// Union-find over point indices (for the legacy computation kept as a test oracle).
#[cfg(test)]
struct Sets(Vec<usize>);

#[cfg(test)]
impl Sets {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

impl Project {
    /// All potentials that have a wire or a name, sorted by name (named first), then by their
    /// first point. Computed from the netlist ([`crate::connectivity`]): a potential is a net,
    /// with every drawing of its endpoints as its points.
    pub fn potentials(&self) -> Vec<Potential> {
        let c = self.connectivity();
        let netlist = crate::connectivity::netlist_of(self, &c);
        // Points in drawing order: functions by id, points in symbol order.
        let mut order: BTreeMap<(FunctionId, String), usize> = BTreeMap::new();
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            for cp in &sym.connection_points {
                let n = order.len();
                order.insert((f.id, cp.name.clone()), n);
            }
        }
        let rank = |r: &ConnectionRef| order.get(&(r.function, r.point.clone())).copied().unwrap_or(usize::MAX);
        let mut out: Vec<Potential> = netlist
            .nets
            .into_iter()
            .map(|n| {
                let mut points: Vec<ConnectionRef> =
                    n.endpoints.iter().flat_map(|e| c.drawings(*e).iter().cloned()).collect();
                points.sort_by_key(|r| rank(r));
                Potential { names: n.names, points, connections: n.wires }
            })
            .collect();
        out.sort_by(|a, b| match (a.names.first(), b.names.first()) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.points.first().map(rank).cmp(&b.points.first().map(rank)),
        });
        out
    }

    /// The computation before M7a, straight from the drawings: kept to check the netlist
    /// against.
    #[cfg(test)]
    pub(crate) fn potentials_legacy(&self) -> Vec<Potential> {
        // Index every connection point of every drawing.
        let mut index: BTreeMap<(FunctionId, String), usize> = BTreeMap::new();
        let mut refs: Vec<ConnectionRef> = Vec::new();
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            for cp in &sym.connection_points {
                index.insert((f.id, cp.name.clone()), refs.len());
                refs.push(ConnectionRef::new(f.id, &cp.name));
            }
        }
        let at = |r: &ConnectionRef| index.get(&(r.function, r.point.clone())).copied();
        let mut sets = Sets((0..refs.len()).collect());

        for c in self.connections.values() {
            if let (Some(a), Some(b)) = (at(&c.from), at(&c.to)) {
                sets.join(a, b);
            }
        }
        // Junctions join their own points; potential and interruption devices join all of
        // their drawings.
        let mut by_device: BTreeMap<crate::DeviceId, Vec<usize>> = BTreeMap::new();
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            let mine: Vec<usize> =
                sym.connection_points.iter().filter_map(|cp| index.get(&(f.id, cp.name.clone())).copied()).collect();
            if sym.bridged {
                for w in mine.windows(2) {
                    sets.join(w[0], w[1]);
                }
            }
            for group in &sym.bridges {
                let joined: Vec<usize> = group.iter().filter_map(|n| index.get(&(f.id, n.clone())).copied()).collect();
                for w in joined.windows(2) {
                    sets.join(w[0], w[1]);
                }
            }
            if sym.role.joins_drawings() {
                by_device.entry(f.device).or_default().extend(mine);
            }
        }
        for points in by_device.values() {
            for w in points.windows(2) {
                sets.join(w[0], w[1]);
            }
        }

        let mut groups: BTreeMap<usize, Potential> = BTreeMap::new();
        for (i, r) in refs.iter().enumerate() {
            let root = sets.find(i);
            let p = groups.entry(root).or_insert_with(|| Potential {
                names: Vec::new(),
                points: Vec::new(),
                connections: Vec::new(),
            });
            p.points.push(r.clone());
            if let Ok(f) = self.function(r.function)
                && self.role(f.id) == Ok(Role::Potential)
                && let Ok(d) = self.device(f.device)
                && !p.names.contains(&d.tag)
            {
                p.names.push(d.tag.clone());
            }
        }
        for c in self.connections.values() {
            if let Some(i) = at(&c.from) {
                let root = sets.find(i);
                if let Some(p) = groups.get_mut(&root) {
                    p.connections.push(c.id);
                }
            }
        }
        let mut out: Vec<Potential> = groups
            .into_values()
            .filter(|p| !p.connections.is_empty() || !p.names.is_empty())
            .map(|mut p| {
                p.names.sort();
                p
            })
            .collect();
        out.sort_by(|a, b| match (a.names.first(), b.names.first()) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        out
    }

    /// The potential a wire is on.
    pub fn potential_of(&self, connection: ConnectionId) -> Option<Potential> {
        self.potentials().into_iter().find(|p| p.connections.contains(&connection))
    }
}

#[cfg(test)]
mod tests {
    use crate::sample::{self, start_circuit};
    use crate::*;

    /// The same sets of points, names and wires as before the netlist, for every sample.
    #[test]
    fn potentials_from_the_netlist_match_the_drawings() {
        let projects = [
            sample::start_circuit(),
            sample::with_contacts(),
            sample::with_potentials(),
            sample::with_terminals(),
            sample::with_plc(),
            sample::with_power(),
            sample::library_overview(),
            crate::reference::reference_project(),
        ];
        for p in projects {
            let p = p.unwrap();
            type Canon = (Vec<String>, Vec<(FunctionId, String)>, Vec<ConnectionId>);
            let canon = |v: Vec<Potential>| {
                let mut v: Vec<Canon> = v
                    .into_iter()
                    .map(|mut x| {
                        let mut points: Vec<(FunctionId, String)> =
                            x.points.into_iter().map(|r| (r.function, r.point)).collect();
                        points.sort();
                        x.connections.sort();
                        (x.names, points, x.connections)
                    })
                    .collect();
                v.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
                v
            };
            assert_eq!(canon(p.potentials()), canon(p.potentials_legacy()), "{}", p.name);
        }
    }

    fn wire(p: &Project, number: &str) -> ConnectionId {
        p.connections.values().find(|c| c.wire_number.as_deref() == Some(number)).unwrap().id
    }

    #[test]
    fn the_sample_has_l_plus_m_and_an_unnamed_potential() {
        let p = start_circuit().unwrap();
        let name = |w| p.potential_of(wire(&p, w)).and_then(|p| p.name());
        assert_eq!(name("W1").as_deref(), Some("L+"));
        assert_eq!(name("W2"), None, "between the switch and the coil");
        assert_eq!(name("W3").as_deref(), Some("M"));
        let all = p.potentials();
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|x| x.conflict().is_none()));
    }

    #[test]
    fn potentials_continue_through_points_junctions_and_interruptions() {
        let p = sample::with_potentials().unwrap();
        // L+ is drawn on both pages; the wire on page 2 is on the same potential as W1.
        let l_plus = p.potential_of(wire(&p, "W1")).unwrap();
        assert!(l_plus.connections.contains(&wire(&p, "W4")), "{l_plus:?}");
        assert_eq!(l_plus.name().as_deref(), Some("L+"));
        // -K1:14 on page 2 continues through interruption point I1 to -K2:A1 on page 1.
        let through = p.potential_of(wire(&p, "W5")).unwrap();
        assert!(through.connections.contains(&wire(&p, "W6")));
        assert_eq!(through.name(), None);
        // The junction on page 1 joins W3 and -K2's return to M.
        let m = p.potential_of(wire(&p, "W7")).unwrap();
        assert!(m.connections.contains(&wire(&p, "W3")) && m.name().as_deref() == Some("M"));
    }

    #[test]
    fn connecting_two_named_potentials_is_flagged() {
        let mut p = start_circuit().unwrap();
        // Draw L+ and M once more each and wire those two drawings together: since all drawings
        // of a potential point are one potential, L+ is now shorted to M everywhere.
        let (l_plus, m) = (DeviceId::from_u128(0x101), DeviceId::from_u128(0x104));
        let mut extra = |device, x| {
            let f = Function {
                id: FunctionId::new(),
                device,
                symbol: SymbolId::new("iec.supply"),
                page: sample::PAGE_1,
                position: Point::new(x, 100.0),
                rotation: Rotation::R0,
                designations: Default::default(),
                properties: Default::default(),
                mirrored: false,
            };
            let id = f.id;
            p.place(f).unwrap();
            id
        };
        let (a, b) = (extra(l_plus, 40.0), extra(m, 60.0));
        p.connect(Connection {
            bend: None,
            id: ConnectionId::new(),
            from: ConnectionRef::new(a, "1"),
            to: ConnectionRef::new(b, "1"),
            wire_number: Some("W9".into()),
            spec: Default::default(),
        })
        .unwrap();
        let shorted = p.potential_of(wire(&p, "W9")).unwrap();
        assert_eq!(shorted.names, ["L+", "M"]);
        assert_eq!(shorted.name().as_deref(), Some("L+ / M"));
        assert_eq!(shorted.conflict().as_deref(), Some("L+ and M are connected to each other"));
        assert!(shorted.connections.contains(&wire(&p, "W1")) && shorted.connections.contains(&wire(&p, "W3")));
    }
}
