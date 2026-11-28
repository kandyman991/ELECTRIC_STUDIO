//! Connectivity as the engineering model sees it, derived from today's drawings (M7a).
//!
//! Today a wire joins two *drawn* connection points (a function and one of its symbol's
//! points). This module derives, in memory and with no change to the files, the model that
//! format 2 (M10) will store (docs/PLATFORM-ARCHITECTURE.md section 4.5):
//!
//! - a [`Terminal`] for each physical connection point of a device, with a deterministic
//!   [`TerminalId`], its physical group ([`GroupId`]: a numbered drawing such as a strip
//!   terminal, or the point alone), its printed designation, its point within the group (the
//!   strip side or level) and its kind;
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
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// The namespace of every derived id.
const NAMESPACE: Uuid = Uuid::from_u128(0x9c1e_45a2_7d3b_4f60_8a51_e2c4_6b0d_17f3);

fn v5(key: &str) -> Uuid {
    Uuid::new_v5(&NAMESPACE, key.as_bytes())
}

/// A physical connection point of a device. Persistent from format 2 on; derived until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TerminalId(pub Uuid);

/// A logical connection point with no physical terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub Uuid);

/// A physical terminal: the group a terminal's points belong to (a strip terminal's sides and
/// levels). Derived from the drawing, not from the printed designation, so renumbering a
/// terminal never changes which group its points are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub Uuid);

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
    /// The physical terminal this point belongs to. A numbered drawing (a strip terminal, a
    /// PLC channel) is one group with a point per side or level; any other drawn point is a
    /// group of its own. Not the printed designation, which can change.
    pub group: GroupId,
    /// The printed designation: shared by the points of one group.
    pub designation: String,
    /// The point within the group: the side or level of a strip terminal (the symbol's point
    /// name); empty for an ordinary device terminal.
    pub point: String,
    pub kind: TerminalKind,
    /// Where it is drawn. In format 1, exactly one drawing point.
    pub drawings: Vec<ConnectionRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// Three or more wires meeting with no physical terminal.
    Junction,
    /// A named potential (`L+`, `M`, `L1`), drawn by potential points on any number of pages.
    Potential,
    /// A potential continued on another page.
    Interruption,
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Junction => "junction",
            NodeKind::Potential => "potential",
            NodeKind::Interruption => "interruption",
        }
    }
}

impl NodeId {
    /// A new node's id.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        NodeId(Uuid::new_v4())
    }

    /// The node a format 1 potential or interruption device becomes (the id M7 derived).
    pub fn for_device(d: DeviceId) -> Self {
        NodeId(v5(&format!("node/{}", d.0)))
    }

    /// The node a format 1 junction drawing becomes (the id M7 derived).
    pub fn for_junction(f: FunctionId) -> Self {
        NodeId(v5(&format!("node/junction/{}", f.0)))
    }

    /// The interruption a format 1 device drawn also as a potential becomes: kept apart.
    pub fn interruption_of(d: DeviceId) -> Self {
        NodeId(v5(&format!("node/{}/interruption", d.0)))
    }

    /// A node split off `self` for one drawing, when a format 1 mapping is ambiguous.
    pub fn split(self, f: FunctionId) -> Self {
        NodeId(v5(&format!("node/{}/{}", self.0, f.0)))
    }
}

/// The node kind a symbol's drawings show, if they show a node.
pub(crate) fn node_kind(sym: &SymbolDef) -> Option<NodeKind> {
    match sym.role {
        Role::Potential => Some(NodeKind::Potential),
        Role::Interruption => Some(NodeKind::Interruption),
        _ if is_junction(sym) => Some(NodeKind::Junction),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// The displayed name (`L+`, `I1`); empty for a junction.
    pub name: String,
    /// The naming scope (D33). Format 1 projects have one scope, `project`.
    pub scope: String,
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
    /// Different physical groups of one device print the same designation: a coil drawn twice,
    /// two contacts both numbered 13, two strip drawings numbered alike. Format 1 can't tell
    /// which, so they are kept as separate terminals until a person reconciles them. Lists
    /// every group and every terminal involved.
    SharedDesignation { device: DeviceId, designation: String, groups: Vec<GroupId>, terminals: Vec<TerminalId> },
    /// A node drawn with a symbol of another kind (a potential point showing an interruption):
    /// only possible behind the commands' back.
    MixedLogicalRoles { node: NodeId },
    /// A junction node drawn more than once: only possible behind the commands' back.
    JunctionDrawnTwice { node: NodeId, drawings: Vec<FunctionId> },
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

/// Whether a symbol's points are physical connection points (terminals), rather than a node's
/// drawing (a junction dot, a potential or interruption point).
pub(crate) fn bears_terminals(sym: &SymbolDef) -> bool {
    !sym.role.joins_drawings() && !is_junction(sym)
}

/// The terminal id a drawing's point gets when it is placed without one: the same formula M7
/// derived ids with, so a format 1 project converts to the same ids.
pub(crate) fn default_terminal(f: FunctionId, point: &str) -> TerminalId {
    TerminalId(v5(&format!("terminal/{}/{point}", f.0)))
}

/// The physical group and point key of a new terminal: a numbered drawing (a strip terminal, a
/// PLC channel) is one group with a point per side or level; any other point is its own.
pub(crate) fn default_group(f: FunctionId, t: TerminalId, point: &str, numbered: bool) -> (GroupId, String) {
    if numbered {
        (GroupId(v5(&format!("group/drawing/{}", f.0))), point.to_owned())
    } else {
        (GroupId(v5(&format!("group/terminal/{}", t.0))), String::new())
    }
}

impl Project {
    /// Terminals, nodes and bridges, derived from the drawings.
    pub fn connectivity(&self) -> Connectivity {
        let mut c = Connectivity::default();
        // Groups by (device, designation), to find designations printed by more than one group.
        let mut by_designation: BTreeMap<(DeviceId, String), BTreeMap<GroupId, Vec<TerminalId>>> = BTreeMap::new();
        // Functions in id order, points in symbol order, so ambiguities list in a stable order.
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            let refs = sym.connection_points.iter().map(|cp| ConnectionRef::new(f.id, &cp.name));

            // A node drawing: its points are the node it is bound to (D1).
            if let Some(n) = f.node {
                let Some(entry) = self.nodes.get(&n) else { continue };
                if node_kind(sym) != Some(entry.kind) {
                    c.ambiguities.push(Ambiguity::MixedLogicalRoles { node: n });
                }
                let node = c.nodes.entry(n).or_insert_with(|| Node {
                    id: n,
                    kind: entry.kind,
                    name: entry.name.clone(),
                    scope: entry.scope.clone(),
                    drawings: Vec::new(),
                });
                for r in refs {
                    c.by_point.insert((r.function, r.point.clone()), Endpoint::Node(n));
                    node.drawings.push(r);
                }
                continue;
            }
            if !bears_terminals(sym) {
                continue;
            }
            if self.device(f.device).is_err() {
                continue;
            }

            let numbered = sym.role.numbered();
            let mut mine: Vec<(String, TerminalId)> = Vec::new();
            for cp in &sym.connection_points {
                // The terminal the point shows (D2); a point the drawing has none for (its
                // symbol gained one) is derived as M7 did.
                let id = f.terminals.get(&cp.name).copied().unwrap_or_else(|| default_terminal(f.id, &cp.name));
                let r = ConnectionRef::new(f.id, &cp.name);
                c.by_point.insert((r.function, r.point.clone()), Endpoint::Terminal(id));
                mine.push((cp.name.clone(), id));
                if let Some(t) = c.terminals.get_mut(&id) {
                    // Another drawing of the same terminal: no new endpoint.
                    t.drawings.push(r);
                    continue;
                }
                let (designation, group, point) = match self.terminals.get(&id) {
                    Some(t) => (t.designation.clone(), t.group, t.point.clone()),
                    None => {
                        // A drawing put in without its terminals (only behind the commands'
                        // back): what it would have been given.
                        let (group, point) = default_group(f.id, id, &cp.name, numbered);
                        (f.designations.get(&cp.name).cloned().unwrap_or_else(|| cp.name.clone()), group, point)
                    }
                };
                by_designation.entry((f.device, designation.clone())).or_default().entry(group).or_default().push(id);
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
                c.terminals
                    .insert(id, Terminal { id, device: f.device, group, designation, point, kind, drawings: vec![r] });
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
        // Terminals no drawing shows yet: endpoints all the same.
        for t in self.terminals.values() {
            if let std::collections::btree_map::Entry::Vacant(slot) = c.terminals.entry(t.id) {
                by_designation
                    .entry((t.device, t.designation.clone()))
                    .or_default()
                    .entry(t.group)
                    .or_default()
                    .push(t.id);
                slot.insert(Terminal {
                    id: t.id,
                    device: t.device,
                    group: t.group,
                    designation: t.designation.clone(),
                    point: t.point.clone(),
                    kind: TerminalKind::Device,
                    drawings: Vec::new(),
                });
            }
        }
        c.bridges.sort();
        c.bridges.dedup();
        // Nodes no drawing shows yet; junctions drawn more than once.
        for n in self.nodes.values() {
            c.nodes.entry(n.id).or_insert_with(|| Node {
                id: n.id,
                kind: n.kind,
                name: n.name.clone(),
                scope: n.scope.clone(),
                drawings: Vec::new(),
            });
        }
        for n in c.nodes.values() {
            let mut drawings: Vec<FunctionId> = n.drawings.iter().map(|r| r.function).collect();
            drawings.dedup();
            if n.kind == NodeKind::Junction && drawings.len() > 1 {
                c.ambiguities.push(Ambiguity::JunctionDrawnTwice { node: n.id, drawings });
            }
        }
        for ((device, designation), groups) in by_designation {
            if groups.len() > 1 {
                let terminals = groups.values().flatten().copied().collect();
                let groups = groups.into_keys().collect();
                c.ambiguities.push(Ambiguity::SharedDesignation { device, designation, groups, terminals });
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

/// A wire end on a point that doesn't exist (the symbol was changed underneath it).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnresolvedEnd {
    pub wire: ConnectionId,
    pub end: ConnectionRef,
}

/// Every net with a wire or a name, and the wire ends that couldn't be resolved. A wire with an
/// unresolved end is on no net: the netlist is then incomplete, and says so.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Netlist {
    pub nets: Vec<Net>,
    pub unresolved: Vec<UnresolvedEnd>,
}

impl Netlist {
    /// Whether every wire end resolved.
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty()
    }
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
        let a = c.endpoint(&w.from).and_then(|e| index.get(&e)).copied();
        let b = c.endpoint(&w.to).and_then(|e| index.get(&e)).copied();
        if let (Some(a), Some(b)) = (a, b) {
            sets.join(a, b);
            wire_at.push((w.id, a));
        }
        for (end, at) in [(&w.from, a), (&w.to, b)] {
            if at.is_none() {
                unresolved.push(UnresolvedEnd { wire: w.id, end: end.clone() });
            }
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
    Netlist { nets, unresolved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;
    use crate::*;

    fn function(device: DeviceId, symbol: &str, x: f64, designations: &[(&str, &str)]) -> Function {
        Function {
            node: None,
            terminals: Default::default(),
            id: FunctionId::new(),
            device,
            symbol: SymbolId::new(symbol),
            page: sample::PAGE_1,
            position: Point::new(x, 90.0),
            rotation: Rotation::R0,
            mirrored: false,
            designations: designations.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect(),
            properties: Default::default(),
        }
    }

    fn place(p: &mut Project, device: DeviceId, symbol: &str, x: f64, designations: &[(&str, &str)]) -> FunctionId {
        let f = function(device, symbol, x, designations);
        let id = f.id;
        p.place(f).unwrap();
        id
    }

    // ---- M10c: nodes ----

    fn potential_drawing(node: NodeId, x: f64) -> Function {
        Function { node: Some(node), ..function(DeviceId::NONE, "iec.supply", x, &[]) }
    }

    #[test]
    fn potentials_are_nodes_bound_by_id_not_devices() {
        let p = sample::with_potentials().unwrap();
        assert!(p.device_by_tag("L+").is_none() && p.device_by_tag("J1").is_none());
        let l_plus = p.node_by_name(NodeKind::Potential, "L+", crate::PROJECT_SCOPE).unwrap();
        assert_eq!(l_plus.id, NodeId::for_device(DeviceId::from_u128(0x101)), "the id M7 derived");
        assert_eq!(p.drawings_of_node(l_plus.id).count(), 2, "page 1 and page 2");
        assert_eq!(p.nodes[&NodeId::for_junction(sample::JUNCTION_1)].kind, NodeKind::Junction);
    }

    #[test]
    fn independent_networks_with_the_same_labels_stay_separate() {
        let mut p = sample::start_circuit().unwrap();
        // A second L+, of another machine: its own node, in its own scope or in the same one.
        for scope in ["=M2", crate::PROJECT_SCOPE] {
            let other = NodeId::new();
            let node =
                crate::NodeEntry { scope: scope.into(), ..crate::NodeEntry::new(other, NodeKind::Potential, "L+") };
            p.apply(Command::AddNode(node)).unwrap();
            let f = potential_drawing(other, 300.0 + p.functions.len() as f64 * 8.0);
            let f_id = f.id;
            p.place(f).unwrap();
            let c = p.connectivity();
            let first = c.endpoint(&ConnectionRef::new(FunctionId::from_u128(0x201), "1")).unwrap();
            let second = c.endpoint(&ConnectionRef::new(f_id, "1")).unwrap();
            assert_ne!(first, second, "{scope}: same label, separate networks");
            let n = p.netlist();
            assert!(!n.nets.iter().any(|net| net.endpoints.contains(&first) && net.endpoints.contains(&second)));
        }
        // Two L+ in one scope: a lookup by name refuses to choose.
        assert_eq!(
            p.node_by_name(NodeKind::Potential, "L+", crate::PROJECT_SCOPE).unwrap_err(),
            ModelError::AmbiguousNode("potential", "L+".into(), crate::PROJECT_SCOPE.into())
        );
        assert!(p.node_by_name(NodeKind::Potential, "L+", "=M2").is_ok());
    }

    #[test]
    fn renaming_a_node_changes_no_id_and_only_rebinding_joins_networks() {
        let mut p = sample::with_potentials().unwrap();
        let l_plus = p.node_by_name(NodeKind::Potential, "L+", crate::PROJECT_SCOPE).unwrap().clone();
        let ends = |p: &Project| p.connectivity().by_point.clone();
        let before = ends(&p);
        let wires = p.connections.clone();
        let undo = p.apply(Command::SetNode(crate::NodeEntry { name: "24V".into(), ..l_plus.clone() })).unwrap();
        assert_eq!(p.nodes[&l_plus.id].name, "24V");
        assert_eq!(ends(&p), before, "every point shows the node it did");
        assert_eq!(p.connections, wires);
        p.apply(undo).unwrap();
        // A new potential point named L+ is its own node until bound explicitly.
        let other = NodeId::new();
        p.apply(Command::AddNode(crate::NodeEntry::new(other, NodeKind::Potential, "L+"))).unwrap();
        let f = potential_drawing(other, 300.0);
        let f_id = f.id;
        p.place(f).unwrap();
        assert_ne!(
            p.connectivity().endpoint(&ConnectionRef::new(f_id, "1")),
            before.get(&(FunctionId::from_u128(0x201), "1".into())).copied()
        );
        let undo = p.apply(Command::Rebind { id: f_id, node: l_plus.id }).unwrap();
        assert_eq!(p.connectivity().endpoint(&ConnectionRef::new(f_id, "1")), Some(Endpoint::Node(l_plus.id)));
        // Kinds must match; a drawn node can't be removed.
        let m = p.node_by_name(NodeKind::Potential, "M", crate::PROJECT_SCOPE).unwrap().id;
        let i1 = p.node_by_name(NodeKind::Interruption, "I1", crate::PROJECT_SCOPE).unwrap().id;
        assert!(p.apply(Command::Rebind { id: f_id, node: i1 }).is_err());
        assert_eq!(p.apply(Command::RemoveNode(m)).unwrap_err(), ModelError::NodeInUse(m.0));
        p.apply(undo).unwrap();
        assert_eq!(p.function(f_id).unwrap().node, Some(other));
        // A node symbol needs a node, a device symbol a device.
        let bare = function(DeviceId::NONE, "iec.supply", 320.0, &[]);
        assert_eq!(p.apply(Command::Place(bare)).unwrap_err(), ModelError::NodeDrawingNeedsNode("potential"));
        let k1 = DeviceId::from_u128(0x103);
        let coil_on_node = Function { node: Some(m), ..function(k1, "iec.coil", 340.0, &[]) };
        assert_eq!(p.apply(Command::Place(coil_on_node)).unwrap_err(), ModelError::NotANodeSymbol);
        assert_eq!(
            p.apply(Command::AddNode(crate::NodeEntry::new(NodeId::new(), NodeKind::Potential, " "))).unwrap_err(),
            ModelError::EmptyNodeName("potential")
        );
    }

    // ---- M10b: terminals as entities ----

    #[test]
    fn placing_gives_each_point_a_persistent_terminal_with_its_designation() {
        let p = sample::with_contacts().unwrap();
        let f = p.function(sample::K1_NO).unwrap();
        assert!(f.designations.is_empty(), "moved onto the terminals");
        assert_eq!(f.terminals.len(), 2);
        for (point, t) in &f.terminals {
            let entry = &p.terminals[t];
            assert_eq!(entry.device, f.device);
            assert_eq!(*t, default_terminal(f.id, point), "the id M7 derived");
            assert_eq!(p.designation(f, point), entry.designation);
        }
        assert_eq!(p.connectivity().terminals.len(), p.terminals.len(), "every derived terminal is stored");
    }

    #[test]
    fn removing_a_drawing_removes_its_terminals_and_undo_restores_them_exactly() {
        let mut p = sample::with_terminals().unwrap();
        // Renumber first, so undo has a designation to restore that isn't the point's name.
        let before_number = p.drawing_number(sample::X2_2).unwrap();
        let undo_renumber = p
            .apply(Command::SetDesignations {
                id: sample::X2_2,
                designations: [("1".into(), "7".into()), ("2".into(), "7".into())].into(),
            })
            .unwrap();
        let ids: Vec<TerminalId> = p.function(sample::X2_2).unwrap().terminals.values().copied().collect();
        let wires = p.connections.clone();
        let before = p.clone();
        let undo = p.apply(Command::Unplace(sample::X2_2)).unwrap();
        assert!(ids.iter().all(|t| !p.terminals.contains_key(t)));
        p.apply(undo).unwrap();
        assert_eq!(p, before, "same terminal ids, designations and wires");
        assert_eq!(p.connections, wires);
        p.apply(undo_renumber).unwrap();
        assert_eq!(p.drawing_number(sample::X2_2).unwrap(), before_number);
    }

    #[test]
    fn a_drawing_can_show_terminals_another_drawing_shows_without_new_endpoints() {
        let mut p = sample::start_circuit().unwrap();
        let coil = p.function(sample::K1_COIL).unwrap().clone();
        let endpoints = p.connectivity().terminals.len();
        let again = Function {
            id: FunctionId::new(),
            position: Point::new(200.0, 90.0),
            designations: Default::default(),
            ..coil.clone()
        };
        let id = again.id;
        p.place(again).unwrap();
        let c = p.connectivity();
        assert_eq!(c.terminals.len(), endpoints, "the same terminals, drawn twice");
        assert!(c.ambiguities.is_empty(), "{:?}", c.ambiguities);
        assert_eq!(c.endpoint(&ConnectionRef::new(id, "A1")), c.endpoint(&ConnectionRef::new(coil.id, "A1")));
        assert_eq!(c.drawings(c.endpoint(&ConnectionRef::new(id, "A1")).unwrap()).len(), 2);
        // Removing one drawing keeps the terminals the other shows.
        p.apply(Command::Unplace(id)).unwrap();
        assert!(coil.terminals.values().all(|t| p.terminals.contains_key(t)));
        // Another device's terminal can't be shown.
        let s1 = p.device_by_tag("-S1").unwrap().id;
        let stray = Function { id: FunctionId::new(), device: s1, ..coil.clone() };
        assert_eq!(p.place(stray).unwrap_err(), ModelError::TerminalOfOtherDevice);
    }

    #[test]
    fn a_terminal_need_not_be_drawn_and_cant_be_removed_while_drawn() {
        let mut p = sample::start_circuit().unwrap();
        let k1 = p.device_by_tag("-K1").unwrap().id;
        let t = crate::TerminalEntry {
            id: TerminalId(Uuid::from_u128(0x7e)),
            device: k1,
            group: GroupId(Uuid::from_u128(0x7f)),
            point: String::new(),
            designation: "X9".into(),
        };
        let undo = p.apply(Command::AddTerminal(t.clone())).unwrap();
        let c = p.connectivity();
        assert!(c.terminals[&t.id].drawings.is_empty());
        assert_eq!(p.apply(Command::AddTerminal(t.clone())).unwrap_err(), ModelError::DuplicateId(t.id.0));
        let drawn = *p.function(sample::K1_COIL).unwrap().terminals.values().next().unwrap();
        assert_eq!(p.apply(Command::RemoveTerminal(drawn)).unwrap_err(), ModelError::TerminalInUse(drawn.0));
        p.apply(undo).unwrap();
        assert!(!p.terminals.contains_key(&t.id));
    }

    #[test]
    fn reassigning_rewires_to_the_other_devices_terminals_and_undo_restores_the_ids() {
        let mut p = sample::with_contacts().unwrap();
        let before = p.clone();
        let old: Vec<TerminalId> = p.function(sample::K1_NO).unwrap().terminals.values().copied().collect();
        let s1 = p.device_by_tag("-S1").unwrap().id;
        let undo = p
            .apply(Command::Reassign {
                id: sample::K1_NO,
                device: s1,
                designations: Default::default(),
                terminals: Default::default(),
            })
            .unwrap();
        let f = p.function(sample::K1_NO).unwrap();
        assert!(f.terminals.values().all(|t| p.terminals[t].device == s1 && !old.contains(t)), "new terminals of -S1");
        assert!(old.iter().all(|t| !p.terminals.contains_key(t)), "-K1's terminals only it showed are gone");
        p.apply(undo).unwrap();
        assert_eq!(p, before);
    }

    fn terminal(c: &Connectivity, f: FunctionId, point: &str) -> TerminalId {
        match c.endpoint(&ConnectionRef::new(f, point)) {
            Some(Endpoint::Terminal(t)) => t,
            other => panic!("{other:?}"),
        }
    }

    fn all_samples() -> Vec<Project> {
        [
            sample::start_circuit(),
            sample::with_contacts(),
            sample::with_potentials(),
            sample::with_terminals(),
            sample::with_plc(),
            sample::with_power(),
            sample::library_overview(),
            crate::reference::reference_project(),
        ]
        .into_iter()
        .map(|p| p.unwrap())
        .collect()
    }

    #[test]
    fn every_wire_end_is_a_terminal_or_a_node_with_the_same_ids_every_time() {
        for p in all_samples() {
            let c = p.connectivity();
            for w in p.connections.values() {
                assert!(c.endpoint(&w.from).is_some() && c.endpoint(&w.to).is_some(), "{}: {:?}", p.name, w.id);
            }
            assert!(p.netlist().is_complete(), "{}", p.name);
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
        let sym = p.symbol(&SymbolId::new("iec.junction")).unwrap();
        for cp in &sym.connection_points {
            assert_eq!(c.endpoint(&ConnectionRef::new(sample::JUNCTION_1, &cp.name)), Some(j));
        }
    }

    /// A potential's node drawn with an interruption symbol: the commands refuse it (a drawing
    /// shows a node of its symbol's kind); behind their back it is reported.
    #[test]
    fn a_node_drawn_with_a_symbol_of_another_kind_is_refused_or_reported() {
        let mut p = sample::start_circuit().unwrap();
        let l_plus = p.node_by_name(NodeKind::Potential, "L+", crate::PROJECT_SCOPE).unwrap().id;
        let f = Function {
            node: Some(l_plus),
            device: DeviceId::NONE,
            ..function(DeviceId::NONE, "iec.interruption", 100.0, &[])
        };
        assert_eq!(p.apply(Command::Place(f.clone())).unwrap_err(), ModelError::NodeDrawingNeedsNode("interruption"));
        p.functions.insert(f.id, f);
        let c = p.connectivity();
        assert!(c.ambiguities.iter().any(|a| matches!(a, Ambiguity::MixedLogicalRoles { node } if *node == l_plus)));
    }

    #[test]
    fn a_strip_terminal_is_one_group_with_two_sides_bridged_explicitly() {
        let p = sample::with_terminals().unwrap();
        let c = p.connectivity();
        let (a, b) = (terminal(&c, sample::X2_1, "1"), terminal(&c, sample::X2_1, "2"));
        assert_ne!(a, b, "each side is its own connection point");
        let (ta, tb) = (&c.terminals[&a], &c.terminals[&b]);
        assert_eq!(ta.group, tb.group, "one physical terminal");
        assert_eq!((ta.designation.as_str(), tb.designation.as_str()), ("1", "1"), "both sides print one number");
        assert_eq!((ta.point.as_str(), tb.point.as_str()), ("1", "2"));
        assert_eq!(ta.kind, TerminalKind::Strip);
        assert!(c.bridges.contains(&Bridge { a: a.min(b), b: a.max(b) }));
        let other = terminal(&c, sample::X2_2, "1");
        assert_ne!(c.terminals[&other].group, ta.group);
        assert!(!c.bridges.iter().any(|x| [x.a, x.b].contains(&a) && [x.a, x.b].contains(&other)));
    }

    #[test]
    fn renumbering_a_terminal_keeps_its_group_and_its_ids() {
        let mut p = sample::with_terminals().unwrap();
        let before = p.connectivity();
        let (a, b) = (terminal(&before, sample::X2_1, "1"), terminal(&before, sample::X2_1, "2"));
        let renumbered = [("1".to_owned(), "41".to_owned()), ("2".to_owned(), "41".to_owned())].into();
        p.apply(Command::SetDesignations { id: sample::X2_1, designations: renumbered }).unwrap();
        let after = p.connectivity();
        assert_eq!((terminal(&after, sample::X2_1, "1"), terminal(&after, sample::X2_1, "2")), (a, b));
        assert_eq!(after.terminals[&a].group, before.terminals[&a].group);
        assert_eq!(after.terminals[&a].designation, "41");
        assert_eq!(p.netlist().nets.len(), sample::with_terminals().unwrap().netlist().nets.len());
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
        let ids = [terminal(&c, f, "1a"), terminal(&c, f, "1b"), terminal(&c, f, "2a"), terminal(&c, f, "2b")];
        assert!(ids.iter().all(|i| c.terminals[i].group == c.terminals[&ids[0]].group), "one physical terminal");
        let bridged = |x: TerminalId, y: TerminalId| c.bridges.contains(&Bridge { a: x.min(y), b: x.max(y) });
        assert!(bridged(ids[0], ids[1]) && bridged(ids[2], ids[3]));
        assert!(!bridged(ids[0], ids[2]) && !bridged(ids[1], ids[3]), "levels isolated");
        assert!(c.ambiguities.is_empty(), "points of one group share the number: {:?}", c.ambiguities);
    }

    #[test]
    fn a_shared_designation_is_kept_apart_and_reported_with_every_point() {
        let mut p = sample::with_contacts().unwrap();
        let k1 = DeviceId::from_u128(0x103);
        let before = p.connectivity().terminals.len();
        let again = place(&mut p, k1, "iec.coil", 60.0, &[]);
        let c = p.connectivity();
        assert_eq!(c.terminals.len(), before + 2, "not merged");
        assert_ne!(terminal(&c, again, "A1"), terminal(&c, sample::K1_COIL, "A1"));
        let shared: Vec<(&str, usize, usize)> = c
            .ambiguities
            .iter()
            .filter_map(|a| match a {
                Ambiguity::SharedDesignation { device, designation, groups, terminals } if *device == k1 => {
                    Some((designation.as_str(), groups.len(), terminals.len()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(shared, [("A1", 2, 2), ("A2", 2, 2)]);
        let net_of = |r: ConnectionRef| {
            let e = c.endpoint(&r).unwrap();
            p.netlist().nets.into_iter().position(|n| n.endpoints.contains(&e))
        };
        assert!(net_of(ConnectionRef::new(sample::K1_COIL, "A1")).is_some());
        assert_eq!(net_of(ConnectionRef::new(again, "A1")), None);
    }

    /// Two numbered drawings on one strip with the same number. The model's commands refuse
    /// this, but it is detected however it arises, with every point of both, whether their point
    /// names are disjoint or the same.
    #[test]
    fn numbered_drawings_sharing_a_number_are_reported_with_all_their_points() {
        for second in ["test.terminal_ab", "iec.terminal"] {
            let mut p = sample::with_terminals().unwrap();
            let mut ab = p.symbol(&SymbolId::new("iec.terminal")).unwrap().clone();
            ab.id = SymbolId::new("test.terminal_ab");
            for (cp, n) in ab.connection_points.iter_mut().zip(["a", "b"]) {
                cp.name = n.into();
            }
            p.add_symbol(ab);
            let strip = p.function(sample::X2_1).unwrap().device;
            let number = p.drawing_number(sample::X2_1).unwrap();
            let points: Vec<String> =
                p.symbol(&SymbolId::new(second)).unwrap().connection_points.iter().map(|c| c.name.clone()).collect();
            let designations: Vec<(&str, &str)> = points.iter().map(|n| (n.as_str(), number.as_str())).collect();
            // Behind the commands' back, as no command allows it.
            let f = function(strip, second, 200.0, &designations);
            p.functions.insert(f.id, f);
            let c = p.connectivity();
            let found: Vec<(usize, usize)> = c
                .ambiguities
                .iter()
                .filter_map(|a| match a {
                    Ambiguity::SharedDesignation { device, groups, terminals, .. } if *device == strip => {
                        Some((groups.len(), terminals.len()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(found, [(2, 4)], "{second}: both groups, all four points");
        }
        // Through the commands, reassigning a numbered drawing onto a number already taken is
        // refused, as placing it is.
        let mut p = sample::with_terminals().unwrap();
        let strip = p.function(sample::X2_1).unwrap().device;
        let n = p.drawing_number(sample::X2_1).unwrap();
        let taken = [("1".to_owned(), n.clone()), ("2".to_owned(), n)].into();
        let r = p.apply(Command::Reassign {
            id: sample::X2_2,
            device: strip,
            designations: taken,
            terminals: Default::default(),
        });
        assert!(matches!(r, Err(ModelError::DuplicateNumber(..))), "{r:?}");
    }

    #[test]
    fn a_wire_end_lost_to_a_symbol_change_makes_the_netlist_incomplete() {
        let mut p = sample::start_circuit().unwrap();
        let mut coil = p.symbol(&SymbolId::new("iec.coil")).unwrap().clone();
        coil.connection_points.retain(|cp| cp.name != "A2");
        p.add_symbol(coil);
        let n = p.netlist();
        assert!(!n.is_complete());
        assert_eq!(n.unresolved.len(), 1);
        assert_eq!((n.unresolved[0].end.function, n.unresolved[0].end.point.as_str()), (sample::K1_COIL, "A2"));
        assert!(!n.nets.iter().any(|net| net.wires.contains(&n.unresolved[0].wire)), "on no net");
    }

    #[test]
    fn a_junction_drawn_twice_is_refused_or_reported() {
        let mut p = sample::with_potentials().unwrap();
        let j1 = p.function(sample::JUNCTION_1).unwrap().node;
        let again = Function { node: j1, ..function(DeviceId::NONE, "iec.junction", 120.0, &[]) };
        assert_eq!(p.apply(Command::Place(again.clone())).unwrap_err(), ModelError::JunctionDrawnTwice);
        p.functions.insert(again.id, again);
        let c = p.connectivity();
        assert!(
            c.ambiguities
                .iter()
                .any(|a| matches!(a, Ambiguity::JunctionDrawnTwice { drawings, .. } if drawings.len() == 2))
        );
    }

    #[test]
    fn plc_channels_are_card_terminals_with_typed_data() {
        let mut p = sample::with_plc().unwrap();
        let mut props = p.function(sample::KF1_DI0).unwrap().properties.clone();
        props.insert(ADDRESS.into(), "%i0.0".into());
        p.apply(Command::SetProperties { id: sample::KF1_DI0, properties: props }).unwrap();
        let c = p.connectivity();
        let t = &c.terminals[&terminal(&c, sample::KF1_DI0, first_point(&p, sample::KF1_DI0))];
        let TerminalKind::PlcChannel(ch) = &t.kind else { panic!("{:?}", t.kind) };
        assert_eq!(
            (ch.kind, ch.address_text.as_str(), ch.address.as_deref()),
            (IoKind::DigitalIn, "%i0.0", Some("I0.0"))
        );
        assert_eq!(t.device, sample::KF1);
    }

    fn first_point(p: &Project, f: FunctionId) -> &str {
        let sym = p.symbol(&p.function(f).unwrap().symbol).unwrap();
        &sym.connection_points[0].name
    }
}
