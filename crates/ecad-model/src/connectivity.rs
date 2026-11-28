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
    /// Different physical groups of one device print the same designation: a coil drawn twice,
    /// two contacts both numbered 13, two strip drawings numbered alike. Format 1 can't tell
    /// which, so they are kept as separate terminals until a person reconciles them. Lists
    /// every group and every terminal involved.
    SharedDesignation { device: DeviceId, designation: String, groups: Vec<GroupId>, terminals: Vec<TerminalId> },
    /// A device drawn both as a potential point and as an interruption point. Its drawings stay
    /// one node, as before (named, as a potential), so no net is split or renamed; M10 must
    /// decide what it is.
    MixedLogicalRoles { device: DeviceId, node: NodeId },
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
        // Logical roles each device is drawn with: potential, interruption.
        let mut logical_roles: BTreeMap<DeviceId, (bool, bool)> = BTreeMap::new();
        let mut junctions: BTreeMap<DeviceId, Vec<NodeId>> = BTreeMap::new();

        // Functions in id order, points in symbol order, so ambiguities list in a stable order.
        for f in self.functions.values() {
            let Ok(sym) = self.symbol(&f.symbol) else { continue };
            let Ok(device) = self.device(f.device) else { continue };
            let refs = sym.connection_points.iter().map(|cp| ConnectionRef::new(f.id, &cp.name));

            if sym.role.joins_drawings() {
                let id = NodeId(v5(&format!("node/{}", f.device.0)));
                let roles = logical_roles.entry(f.device).or_default();
                if sym.role == Role::Potential {
                    roles.0 = true;
                } else {
                    roles.1 = true;
                }
                // The kind is settled below from all its drawings, whatever their order.
                let node = c.nodes.entry(id).or_insert_with(|| Node {
                    id,
                    kind: NodeKind::Interruption,
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
        for (device, (potential, interruption)) in logical_roles {
            let id = NodeId(v5(&format!("node/{}", device.0)));
            if let Some(node) = c.nodes.get_mut(&id) {
                // As before M7a: drawn as a potential point anywhere, it names its net.
                node.kind = if potential { NodeKind::Potential } else { NodeKind::Interruption };
            }
            if potential && interruption {
                c.ambiguities.push(Ambiguity::MixedLogicalRoles { device, node: id });
            }
        }
        for ((device, designation), groups) in by_designation {
            if groups.len() > 1 {
                let terminals = groups.values().flatten().copied().collect();
                let groups = groups.into_keys().collect();
                c.ambiguities.push(Ambiguity::SharedDesignation { device, designation, groups, terminals });
            }
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

    /// A device drawn as both a potential point and an interruption point (the model allows
    /// it): one node, named as a potential whichever drawing sorts first, its net kept whole and
    /// still clashing with another named potential, and reported for M10.
    #[test]
    fn a_device_drawn_as_potential_and_interruption_keeps_its_name_in_either_order() {
        for interruption_first in [true, false] {
            let mut p = sample::start_circuit().unwrap();
            let l_plus = DeviceId::from_u128(0x101);
            let existing = p.functions_of(l_plus).next().unwrap().id;
            // An interruption drawing of L+ whose id sorts before or after the potential's.
            let id = if interruption_first { FunctionId::from_u128(0x1) } else { FunctionId::from_u128(0xffff) };
            assert_eq!(id < existing, interruption_first);
            let f = Function { id, ..function(l_plus, "iec.interruption", 100.0, &[]) };
            p.place(f).unwrap();
            // Wire the interruption drawing to M: L+ is now shorted to M.
            let m = p.functions_of(DeviceId::from_u128(0x104)).next().unwrap().id;
            let m_point = ConnectionRef::new(m, "1");
            if let Some(old) = p.connection_at(&m_point).map(|c| c.id) {
                p.apply(Command::Disconnect(old)).unwrap();
            }
            p.apply(Command::Connect(Connection {
                id: ConnectionId::new(),
                from: ConnectionRef::new(id, "1"),
                to: m_point,
                wire_number: None,
                spec: Default::default(),
                bend: None,
            }))
            .unwrap();
            let c = p.connectivity();
            let node = c.nodes.values().find(|n| n.device == l_plus).unwrap();
            assert_eq!((node.kind, node.name.as_str(), node.drawings.len()), (NodeKind::Potential, "L+", 2));
            assert!(
                c.ambiguities
                    .iter()
                    .any(|a| matches!(a, Ambiguity::MixedLogicalRoles { device, .. } if *device == l_plus))
            );
            let clash: Vec<Net> = p.netlist().nets.into_iter().filter(|n| n.names.len() > 1).collect();
            assert_eq!(clash.len(), 1, "order {interruption_first}");
            assert_eq!(clash[0].names, ["L+", "M"]);
            // Exactly as before M7a.
            let names = |v: Vec<crate::Potential>| {
                let mut n: Vec<Vec<String>> = v.into_iter().map(|p| p.names).collect();
                n.sort();
                n
            };
            assert_eq!(names(p.potentials()), names(p.potentials_legacy()));
        }
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
