//! Connectivity as the engineering model sees it, derived from today's drawings (M7a).
//!
//! Today a wire joins two *drawn* connection points (a function and one of its symbol's
//! points). This module derives, in memory and with no change to the files, the model that
//! format 2 (M10) will store (docs/PLATFORM-ARCHITECTURE.md section 4.5):
//!
//! - a [`Terminal`] for each physical connection point of a device, with a deterministic
//!   [`TerminalId`], its physical group (a strip terminal by its number, a device terminal by its
//!   designation), its point within the group (the strip side or level) and its kind;
//! - a [`Node`] for each logical connection point with no physical terminal: a junction, a named
//!   potential, an interruption;
//! - explicit [`Bridge`]s inside a device (the two sides of a strip terminal, a black box's
//!   joined terminals), never inferred from a printed number;
//! - the [`Netlist`]: which endpoints are one electrical net.
//!
//! Ids are UUID v5 from the legacy endpoint (the drawing and symbol point), so the same project
//! gives the same ids on every load. Repeated representations of one logical point (a potential
//! drawn on several pages, both drawings of an interruption) are one node. Format 1 can't prove
//! that two drawings of a physical device show the same point: a coil drawn twice looks just
//! like a second contact numbered like the first. So physical points that share a printed
//! designation stay separate terminals and are reported as an [`Ambiguity`] for a person to
//! reconcile. A matching label never establishes connectivity.

use crate::project::{ADDRESS, COMMENT, ConnectionId, ConnectionRef, DeviceId, FunctionId, Project, SYMBOL};
use crate::symbol::{IoKind, Role, SymbolDef};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// The namespace of every derived id.
const NAMESPACE: Uuid = Uuid::from_u128(0x9c1e_45a2_7d3b_4f60_8a51_e2c4_6b0d_17f3);

fn v5(key: &str) -> Uuid {
    Uuid::new_v5(&NAMESPACE, key.as_bytes())
}

/// A physical connection point of a device. Persistent from format 2 on; derived until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TerminalId(pub Uuid);

/// A logical connection point with no physical terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeId(pub Uuid);

/// One end of a wire: a terminal or a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "camelCase")]
pub enum Endpoint {
    Terminal(TerminalId),
    Node(NodeId),
}

/// What a terminal is.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalKind {
    /// A terminal of an ordinary device: a coil's A1, a contact's 13.
    Device,
    /// One side or level of a terminal-strip terminal.
    Strip,
    /// A PLC card's channel, with its data typed.
    PlcChannel(PlcChannel),
}

/// A PLC channel's data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlcChannel {
    pub kind: IoKind,
    /// The address as entered, e.g. `i0.3` or `%I0.3`.
    pub address_text: String,
    /// The address in canonical form (`I0.3`), if it is one.
    pub address: Option<String>,
    pub symbol: String,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Terminal {
    pub id: TerminalId,
    pub device: DeviceId,
    /// The physical terminal this point belongs to: a strip terminal by its number, a device
    /// terminal by its designation. Points of one group share the printed designation.
    pub group: String,
    /// The printed designation.
    pub designation: String,
    /// The point within the group: the side or level of a strip terminal (the symbol's point
    /// name); empty for an ordinary device terminal.
    pub point: String,
    pub kind: TerminalKind,
    /// Where it is drawn. In format 1, exactly one drawing point.
    pub drawings: Vec<ConnectionRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// Three or more wires meeting with no physical terminal.
    Junction,
    /// A named potential (`L+`, `M`, `L1`), drawn by potential points on any number of pages.
    Potential,
    /// A potential continued on another page.
    Interruption,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// The displayed name (`L+`, `I1`); empty for a junction.
    pub name: String,
    /// The naming scope the name is unique in. Format 1 projects have one scope, `project`.
    pub scope: String,
    /// The device that stands for it in format 1.
    pub device: DeviceId,
    pub drawings: Vec<ConnectionRef>,
}

/// An explicit electrical joint inside a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Bridge {
    pub a: TerminalId,
    pub b: TerminalId,
}

/// A mapping that can't be decided from the drawings, kept apart and reported.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Ambiguity {
    /// Different drawn points of one device print the same designation: a coil drawn twice, or
    /// two contacts both numbered 13. Format 1 can't tell which, so they are kept as separate
    /// terminals until a person reconciles them.
    SharedDesignation { device: DeviceId, designation: String, terminals: Vec<TerminalId> },
    /// A junction device drawn more than once: each drawing is its own junction.
    JunctionDrawnTwice { device: DeviceId, nodes: Vec<NodeId> },
}

/// The derived connectivity of a project.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Connectivity {
    pub terminals: BTreeMap<TerminalId, Terminal>,
    pub nodes: BTreeMap<NodeId, Node>,
    pub bridges: Vec<Bridge>,
    pub ambiguities: Vec<Ambiguity>,
    /// Each drawn connection point's endpoint.
    #[serde(skip)]
    by_point: BTreeMap<(FunctionId, String), Endpoint>,
}

impl Connectivity {
    /// The endpoint a drawn connection point stands for.
    pub fn endpoint(&self, r: &ConnectionRef) -> Option<Endpoint> {
        self.by_point.get(&(r.function, r.point.clone())).copied()
    }

    /// Every drawing of an endpoint.
    pub fn drawings(&self, e: Endpoint) -> &[ConnectionRef] {
        match e {
            Endpoint::Terminal(t) => self.terminals.get(&t).map_or(&[], |t| &t.drawings),
            Endpoint::Node(n) => self.nodes.get(&n).map_or(&[], |n| &n.drawings),
        }
    }
}

fn is_junction(sym: &SymbolDef) -> bool {
    sym.role == Role::Single && sym.bridged
}

impl Project {
    /// Terminals, nodes and bridges, derived from the drawings.
    pub fn connectivity(&self) -> Connectivity {
        let mut c = Connectivity::default();
        // Physical points by (device, group, point key), to find points that share a designation.
        let mut physical: BTreeMap<(DeviceId, String, String), TerminalId> = BTreeMap::new();
        let mut shared: BTreeMap<(DeviceId, String), Vec<TerminalId>> = BTreeMap::new();
        let mut junctions: BTreeMap<DeviceId, Vec<NodeId>> = BTreeMap::new();

        // Functions in id order, points in symbol order, so ambiguities list in a stable order.
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            let Ok(device) = self.device(f.device) else { continue };
            let refs = sym.connection_points.iter().map(|cp| ConnectionRef::new(f.id, &cp.name));

            if sym.role.joins_drawings() {
                let id = NodeId(v5(&format!("node/{}", f.device.0)));
                let kind = if sym.role == Role::Potential { NodeKind::Potential } else { NodeKind::Interruption };
                let node = c.nodes.entry(id).or_insert_with(|| Node {
                    id,
                    kind,
                    name: device.tag.clone(),
                    scope: "project".into(),
                    device: f.device,
                    drawings: Vec::new(),
                });
                for r in refs {
                    c.by_point.insert((r.function, r.point.clone()), Endpoint::Node(id));
                    node.drawings.push(r);
                }
                continue;
            }
            if is_junction(sym) {
                let id = NodeId(v5(&format!("node/junction/{}", f.id.0)));
                junctions.entry(f.device).or_default().push(id);
                let drawings: Vec<ConnectionRef> = refs.collect();
                for r in &drawings {
                    c.by_point.insert((r.function, r.point.clone()), Endpoint::Node(id));
                }
                c.nodes.insert(
                    id,
                    Node {
                        id,
                        kind: NodeKind::Junction,
                        name: String::new(),
                        scope: "project".into(),
                        device: f.device,
                        drawings,
                    },
                );
                continue;
            }

            let numbered = sym.role.numbered();
            let number = if numbered { self.drawing_number(f.id) } else { None };
            let mut mine: Vec<(String, TerminalId)> = Vec::new();
            for cp in &sym.connection_points {
                let designation = f.designation(&cp.name).to_owned();
                let (group, point) = match &number {
                    Some(n) => (n.clone(), cp.name.clone()),
                    None => (designation.clone(), String::new()),
                };
                let key = (f.device, group.clone(), point.clone());
                let id = TerminalId(v5(&format!("terminal/{}/{}", f.id.0, cp.name)));
                match physical.get(&key) {
                    Some(first) => {
                        // Another point of this device prints the same designation: kept apart.
                        let list = shared.entry((f.device, designation.clone())).or_default();
                        if list.is_empty() {
                            list.push(*first);
                        }
                        list.push(id);
                    }
                    None => {
                        physical.insert(key, id);
                    }
                }
                let kind = match sym.role {
                    Role::Terminal => TerminalKind::Strip,
                    Role::PlcChannel { kind } => {
                        let text = f.property(ADDRESS).to_owned();
                        TerminalKind::PlcChannel(PlcChannel {
                            kind,
                            address: crate::plc_address::canonical(&text),
                            address_text: text,
                            symbol: f.property(SYMBOL).to_owned(),
                            comment: f.property(COMMENT).to_owned(),
                        })
                    }
                    _ => TerminalKind::Device,
                };
                c.terminals.insert(
                    id,
                    Terminal { id, device: f.device, group, designation, point, kind, drawings: Vec::new() },
                );
                let r = ConnectionRef::new(f.id, &cp.name);
                c.by_point.insert((r.function, r.point.clone()), Endpoint::Terminal(id));
                if let Some(t) = c.terminals.get_mut(&id) {
                    t.drawings.push(r);
                }
                mine.push((cp.name.clone(), id));
            }
            // Explicit joints: all points of a bridged symbol (a strip terminal's sides), and a
            // symbol's bridge groups (a black box's joined terminals, one level of a terminal).
            let mut groups: Vec<Vec<TerminalId>> = Vec::new();
            if sym.bridged {
                groups.push(mine.iter().map(|(_, t)| *t).collect());
            }
            for g in &sym.bridges {
                groups.push(mine.iter().filter(|(n, _)| g.contains(n)).map(|(_, t)| *t).collect());
            }
            for g in groups {
                for w in g.windows(2) {
                    if w[0] != w[1] {
                        let (a, b) = if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) };
                        c.bridges.push(Bridge { a, b });
                    }
                }
            }
        }
        c.bridges.sort();
        c.bridges.dedup();
        for ((device, designation), terminals) in shared {
            c.ambiguities.push(Ambiguity::SharedDesignation { device, designation, terminals });
        }
        for (device, nodes) in junctions {
            if nodes.len() > 1 {
                c.ambiguities.push(Ambiguity::JunctionDrawnTwice { device, nodes });
            }
        }
        c
    }

    /// Which endpoints are one electrical net, through wires, bridges and nodes.
    pub fn netlist(&self) -> Netlist {
        netlist_of(self, &self.connectivity())
    }
}

/// One electrical net.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Net {
    /// Every endpoint on it, sorted.
    pub endpoints: Vec<Endpoint>,
    /// The names of the potential nodes on it, sorted. More than one is a conflict.
    pub names: Vec<String>,
    /// Every wire on it, sorted.
    pub wires: Vec<ConnectionId>,
}

/// Every net with a wire or a name, and the wires whose ends couldn't be resolved.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Netlist {
    pub nets: Vec<Net>,
    /// Wires with an end on a point that no longer exists (a symbol changed underneath).
    pub unresolved: Vec<ConnectionId>,
}

/// Union-find over indices.
struct Sets(Vec<usize>);

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

pub(crate) fn netlist_of(project: &Project, c: &Connectivity) -> Netlist {
    let endpoints: Vec<Endpoint> =
        c.terminals.keys().map(|t| Endpoint::Terminal(*t)).chain(c.nodes.keys().map(|n| Endpoint::Node(*n))).collect();
    let index: BTreeMap<Endpoint, usize> = endpoints.iter().enumerate().map(|(i, e)| (*e, i)).collect();
    let mut sets = Sets((0..endpoints.len()).collect());
    let mut unresolved = Vec::new();
    let mut wire_at: Vec<(ConnectionId, usize)> = Vec::new();
    for w in project.connections.values() {
        match (c.endpoint(&w.from).and_then(|e| index.get(&e)), c.endpoint(&w.to).and_then(|e| index.get(&e))) {
            (Some(&a), Some(&b)) => {
                sets.join(a, b);
                wire_at.push((w.id, a));
            }
            _ => unresolved.push(w.id),
        }
    }
    for b in &c.bridges {
        if let (Some(&x), Some(&y)) = (index.get(&Endpoint::Terminal(b.a)), index.get(&Endpoint::Terminal(b.b))) {
            sets.join(x, y);
        }
    }
    let mut nets: BTreeMap<usize, Net> = BTreeMap::new();
    for (i, e) in endpoints.iter().enumerate() {
        let net = nets.entry(sets.find(i)).or_insert_with(|| Net {
            endpoints: Vec::new(),
            names: Vec::new(),
            wires: Vec::new(),
        });
        net.endpoints.push(*e);
        if let Endpoint::Node(n) = e
            && let Some(node) = c.nodes.get(n)
            && node.kind == NodeKind::Potential
            && !net.names.contains(&node.name)
        {
            net.names.push(node.name.clone());
        }
    }
    for (w, at) in wire_at {
        if let Some(net) = nets.get_mut(&sets.find(at)) {
            net.wires.push(w);
        }
    }
    let mut nets: Vec<Net> = nets
        .into_values()
        .filter(|n| !n.wires.is_empty() || !n.names.is_empty())
        .map(|mut n| {
            n.names.sort();
            n.endpoints.sort();
            n.wires.sort();
            n
        })
        .collect();
    // Named nets first, by name; then by their first endpoint.
    nets.sort_by(|a, b| match (a.names.first(), b.names.first()) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.endpoints.cmp(&b.endpoints)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.endpoints.cmp(&b.endpoints),
    });
    unresolved.sort();
    Netlist { nets, unresolved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;
    use crate::*;

    fn place(p: &mut Project, device: DeviceId, symbol: &str, x: f64, designations: &[(&str, &str)]) -> FunctionId {
        let f = Function {
            id: FunctionId::new(),
            device,
            symbol: SymbolId::new(symbol),
            page: sample::PAGE_1,
            position: Point::new(x, 90.0),
            rotation: Rotation::R0,
            mirrored: false,
            designations: designations.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect(),
            properties: Default::default(),
        };
        let id = f.id;
        p.place(f).unwrap();
        id
    }

    #[test]
    fn every_wire_end_is_a_terminal_or_a_node_with_the_same_ids_every_time() {
        let all = [
            sample::start_circuit(),
            sample::with_contacts(),
            sample::with_potentials(),
            sample::with_terminals(),
            sample::with_plc(),
            sample::with_power(),
            sample::library_overview(),
            crate::reference::reference_project(),
        ];
        for p in all {
            let p = p.unwrap();
            let c = p.connectivity();
            for w in p.connections.values() {
                assert!(c.endpoint(&w.from).is_some() && c.endpoint(&w.to).is_some(), "{}: {:?}", p.name, w.id);
            }
            assert!(p.netlist().unresolved.is_empty(), "{}", p.name);
            assert_eq!(c, p.clone().connectivity(), "{}: deterministic", p.name);
            assert!(c.ambiguities.is_empty(), "{}: {:?}", p.name, c.ambiguities);
        }
    }

    #[test]
    fn potentials_and_interruptions_are_one_node_across_pages_and_junctions_are_nodes() {
        let p = sample::with_potentials().unwrap();
        let c = p.connectivity();
        let l_plus: Vec<&Node> = c.nodes.values().filter(|n| n.name == "L+").collect();
        assert_eq!(l_plus.len(), 1);
        assert_eq!(l_plus[0].kind, NodeKind::Potential);
        assert_eq!(l_plus[0].drawings.len(), 2, "drawn on both pages, one node");
        let i1 = c.nodes.values().find(|n| n.name == "I1").unwrap();
        assert_eq!((i1.kind, i1.drawings.len()), (NodeKind::Interruption, 2));
        let j = c.endpoint(&ConnectionRef::new(sample::JUNCTION_1, "1")).unwrap();
        assert!(matches!(j, Endpoint::Node(n) if c.nodes[&n].kind == NodeKind::Junction));
        // Every point of the junction is the one node.
        let sym = p.symbol(&SymbolId::new("iec.junction")).unwrap();
        for cp in &sym.connection_points {
            assert_eq!(c.endpoint(&ConnectionRef::new(sample::JUNCTION_1, &cp.name)), Some(j));
        }
    }

    #[test]
    fn a_strip_terminal_is_one_group_with_two_sides_bridged_explicitly() {
        let p = sample::with_terminals().unwrap();
        let c = p.connectivity();
        let a = c.endpoint(&ConnectionRef::new(sample::X2_1, "1")).unwrap();
        let b = c.endpoint(&ConnectionRef::new(sample::X2_1, "2")).unwrap();
        let (Endpoint::Terminal(a), Endpoint::Terminal(b)) = (a, b) else { panic!("terminals") };
        assert_ne!(a, b, "each side is its own connection point");
        let (ta, tb) = (&c.terminals[&a], &c.terminals[&b]);
        assert_eq!((ta.group.as_str(), ta.designation.as_str()), ("1", "1"));
        assert_eq!((tb.group.as_str(), tb.designation.as_str()), ("1", "1"), "both sides print the same number");
        assert_eq!((ta.point.as_str(), tb.point.as_str()), ("1", "2"));
        assert_eq!(ta.kind, TerminalKind::Strip);
        assert!(c.bridges.contains(&Bridge { a: a.min(b), b: a.max(b) }));
        // The next terminal on the strip is a different group, not bridged to the first.
        let Some(Endpoint::Terminal(other)) = c.endpoint(&ConnectionRef::new(sample::X2_2, "1")) else { panic!() };
        assert_ne!(c.terminals[&other].group, ta.group);
        assert!(!c.bridges.iter().any(|x| [x.a, x.b].contains(&a) && [x.a, x.b].contains(&other)));
    }

    #[test]
    fn levels_of_a_terminal_stay_isolated() {
        let mut p = sample::start_circuit().unwrap();
        let mut two_level = p.symbol(&SymbolId::new("iec.terminal")).unwrap().clone();
        two_level.id = SymbolId::new("test.terminal_2level");
        two_level.bridged = false;
        let cp = two_level.connection_points[0].clone();
        two_level.connection_points = ["1a", "1b", "2a", "2b"]
            .iter()
            .enumerate()
            .map(|(i, n)| ConnectionPointDef { name: (*n).into(), at: Point::new(i as f64 * 2.0, 0.0), ..cp.clone() })
            .collect();
        two_level.bridges = vec![vec!["1a".into(), "1b".into()], vec!["2a".into(), "2b".into()]];
        p.add_symbol(two_level);
        let strip = p.add_device(Device::new(DeviceId::new(), "-X9", "")).unwrap();
        let n = [("1a", "4"), ("1b", "4"), ("2a", "4"), ("2b", "4")];
        let f = place(&mut p, strip, "test.terminal_2level", 0.0, &n);
        let c = p.connectivity();
        let t = |pt: &str| match c.endpoint(&ConnectionRef::new(f, pt)) {
            Some(Endpoint::Terminal(t)) => t,
            other => panic!("{other:?}"),
        };
        let ids = [t("1a"), t("1b"), t("2a"), t("2b")];
        assert!(ids.iter().all(|i| c.terminals[i].group == "4"), "one physical terminal");
        let bridged = |x: TerminalId, y: TerminalId| c.bridges.contains(&Bridge { a: x.min(y), b: x.max(y) });
        assert!(bridged(ids[0], ids[1]) && bridged(ids[2], ids[3]));
        assert!(!bridged(ids[0], ids[2]) && !bridged(ids[1], ids[3]), "levels isolated");
        assert!(c.ambiguities.is_empty(), "points of one group may share the number: {:?}", c.ambiguities);
    }

    #[test]
    fn a_shared_number_on_a_device_is_kept_apart_and_reported() {
        let mut p = sample::with_contacts().unwrap();
        let k1 = DeviceId::from_u128(0x103);
        let before = p.connectivity().terminals.len();
        // The coil drawn a second time: format 1 can't prove it's the same coil, so its points
        // are new terminals, reported for reconciliation rather than merged.
        let again = place(&mut p, k1, "iec.coil", 60.0, &[]);
        let c = p.connectivity();
        assert_eq!(c.terminals.len(), before + 2);
        assert_ne!(
            c.endpoint(&ConnectionRef::new(again, "A1")),
            c.endpoint(&ConnectionRef::new(sample::K1_COIL, "A1"))
        );
        let shared: Vec<&str> = c
            .ambiguities
            .iter()
            .filter_map(|a| match a {
                Ambiguity::SharedDesignation { device, designation, terminals } if *device == k1 => {
                    assert_eq!(terminals.len(), 2);
                    Some(designation.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(shared, ["A1", "A2"]);
        // The netlist doesn't join them either: the second coil's A1 is on no net yet.
        let net_of = |r: ConnectionRef| {
            let e = c.endpoint(&r).unwrap();
            p.netlist().nets.into_iter().position(|n| n.endpoints.contains(&e))
        };
        assert!(net_of(ConnectionRef::new(sample::K1_COIL, "A1")).is_some());
        assert_eq!(net_of(ConnectionRef::new(again, "A1")), None);
    }

    #[test]
    fn a_junction_drawn_twice_is_reported() {
        let mut p = sample::with_potentials().unwrap();
        let j1 = p.function(sample::JUNCTION_1).unwrap().device;
        place(&mut p, j1, "iec.junction", 120.0, &[]);
        let c = p.connectivity();
        assert!(
            c.ambiguities.iter().any(|a| matches!(a, Ambiguity::JunctionDrawnTwice { nodes, .. } if nodes.len() == 2))
        );
    }

    #[test]
    fn plc_channels_are_card_terminals_with_typed_data() {
        let mut p = sample::with_plc().unwrap();
        let mut props = p.function(sample::KF1_DI0).unwrap().properties.clone();
        props.insert(ADDRESS.into(), "%i0.0".into());
        p.apply(Command::SetProperties { id: sample::KF1_DI0, properties: props }).unwrap();
        let c = p.connectivity();
        let channels: Vec<&PlcChannel> = c
            .terminals
            .values()
            .filter(|t| t.device == sample::KF1)
            .filter_map(|t| match &t.kind {
                TerminalKind::PlcChannel(ch) => Some(ch),
                _ => None,
            })
            .collect();
        assert!(!channels.is_empty());
        let di0 = c
            .terminals
            .values()
            .find(|t| t.drawings.iter().any(|r| r.function == sample::KF1_DI0))
            .and_then(|t| match &t.kind {
                TerminalKind::PlcChannel(ch) => Some(ch.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (di0.kind, di0.address_text.as_str(), di0.address.as_deref()),
            (IoKind::DigitalIn, "%i0.0", Some("I0.0"))
        );
    }
}
