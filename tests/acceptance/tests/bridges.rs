//! M10 corrective findings R7 and R8: internal bridges are canonical project data
//! (`bridges.json`), independent of the drawings that depict them; drawn wire ends change only
//! through explicit engineering operations; undrawn terminals keep their typed meaning and stay
//! in the engineering reports.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::connectivity::{Bridge, TerminalKind};
use ecad_model::{
    Command, ConnectionId, Endpoint, FunctionId, History, NodeKind, Point, Project, Rotation, TerminalId, Wire,
    conveyor::conveyor,
};
use std::collections::BTreeMap;

fn save_and_load(p: &Project) -> Project {
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(p, tmp.path()).unwrap();
    ecad_project::load(tmp.path()).unwrap()
}

/// The drawing of `tag` showing designation `d`.
fn drawing_of(p: &Project, tag: &str, d: &str) -> FunctionId {
    let dev = p.device_by_tag(tag).unwrap().id;
    p.functions_of(dev).find(|f| f.terminals.keys().any(|pt| p.designation(f, pt) == d)).unwrap().id
}

fn m_node(p: &Project) -> Endpoint {
    Endpoint::Node(p.node_by_name(NodeKind::Potential, "M", ecad_model::PROJECT_SCOPE).unwrap().id)
}

fn undrawn_wire(p: &mut Project, from: TerminalId, to: Endpoint) -> ConnectionId {
    let id = ConnectionId::new();
    p.apply(Command::AddWire(Wire {
        id,
        from: Endpoint::Terminal(from),
        to,
        wire_number: None,
        spec: Default::default(),
    }))
    .unwrap();
    id
}

/// Every drawn and undrawn wire with its resolved ends.
fn ends(p: &Project) -> BTreeMap<ConnectionId, (Option<Endpoint>, Option<Endpoint>)> {
    let c = p.connectivity();
    let mut out: BTreeMap<_, _> =
        p.connections.values().map(|w| (w.id, (c.endpoint(&w.from), c.endpoint(&w.to)))).collect();
    out.extend(p.undrawn.values().map(|w| (w.id, (Some(w.from), Some(w.to)))));
    out
}

/// A second representation of an existing drawing: same symbol, same terminals, elsewhere.
fn repeat(p: &Project, f: FunctionId, dx: f64) -> ecad_model::Function {
    let mut copy = p.function(f).unwrap().clone();
    copy.id = FunctionId::new();
    copy.position = Point::new(copy.position.x + dx, copy.position.y);
    copy
}

#[test]
fn a_repeated_representation_depicts_existing_bridges_and_cannot_add_one() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let bridges = p.bridges.clone();
    let net = p.netlist();
    // The same bridged terminals shown again: allowed, and nothing about the bridges changes.
    let again = repeat(&p, f, 20.0);
    let undo = p.apply(Command::Place(again.clone())).unwrap();
    assert_eq!(p.bridges, bridges, "a representation adds no bridge");
    assert_eq!(p.netlist().nets.len(), net.nets.len());
    p.apply(undo).unwrap();
    assert_eq!(p.bridges, bridges);
    // A depicted bridge cannot be removed, even with multiple representations.
    let b12 = Bridge::new(t["1"], t["2"]);
    let before = p.clone();
    assert_eq!(p.apply(Command::RemoveBridge(b12)).unwrap_err(), ecad_model::ModelError::BridgeDepicted);
    assert_eq!(p, before);
    assert_eq!(save_and_load(&p), p);
    // Once the drawing is gone, an engineering bridge may be removed; a new depiction
    // cannot silently restore it.
    let later = repeat(&p, f, 40.0);
    let m = m_node(&p);
    for id in t.values() {
        undrawn_wire(&mut p, *id, m);
    }
    p.apply(Command::Unplace(f)).unwrap();
    let restore = p.apply(Command::RemoveBridge(b12)).unwrap();
    let before = p.clone();
    assert_eq!(save_and_load(&p), p);
    let err = p.apply(Command::Place(later)).unwrap_err();
    assert!(matches!(err, ecad_model::ModelError::UnbridgedRepresentation), "{err:?}");
    assert_eq!(p, before, "nothing changes on refusal");
    p.apply(restore).unwrap();
    assert_eq!(p.bridges, bridges);
}

#[test]
fn bridges_are_explicit_engineering_operations_with_exact_undo() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let original = p.clone();
    let net_of = |p: &Project, x: &str| {
        p.netlist().nets.iter().position(|n| n.endpoints.contains(&Endpoint::Terminal(t[x]))).unwrap()
    };
    let mut h = History::default();
    // A bridge across the levels joins their nets; it is engineering data, saved and loaded.
    let cross = Bridge::new(t["1"], t["3"]);
    assert_ne!(net_of(&p, "1"), net_of(&p, "3"));
    h.execute(&mut p, "bridge", Command::AddBridge(cross)).unwrap();
    assert_eq!(net_of(&p, "1"), net_of(&p, "3"));
    assert_eq!(save_and_load(&p).bridges, p.bridges);
    assert!(matches!(p.apply(Command::AddBridge(cross)), Err(ecad_model::ModelError::DuplicateBridge)));
    // A bridge needs two terminals of one device.
    let other = p.connectivity().terminals.values().find(|x| x.device != p.terminals[&t["1"]].device).unwrap().id;
    assert!(matches!(p.apply(Command::AddBridge(Bridge::new(t["1"], other))), Err(ecad_model::ModelError::BadBridge)));
    assert!(matches!(
        p.apply(Command::AddBridge(Bridge::new(t["1"], TerminalId(ConnectionId::new().0)))),
        Err(ecad_model::ModelError::BadBridge)
    ));
    // Removing a depicted bridge is refused, but the explicit, non-depicted cross bridge
    // can be removed, saved and reloaded, undone and redone.
    let before = p.clone();
    assert_eq!(p.apply(Command::RemoveBridge(Bridge::new(t["3"], t["4"]))).unwrap_err(),
        ecad_model::ModelError::BridgeDepicted);
    assert_eq!(p, before);
    h.execute(&mut p, "unbridge", Command::RemoveBridge(cross)).unwrap();
    assert_eq!(p, original);
    assert_eq!(save_and_load(&p), p);
    assert_eq!(p.apply(Command::RemoveBridge(cross)).unwrap_err(), ecad_model::ModelError::NoBridge);
    h.undo(&mut p).unwrap();
    assert_eq!(p, before);
    h.undo(&mut p).unwrap();
    assert_eq!(p, original);
    h.redo(&mut p).unwrap();
    assert_eq!(p, before);
    h.redo(&mut p).unwrap();
    assert_eq!(p, original);
    assert_eq!(save_and_load(&p), p);
}

#[test]
fn a_bridged_terminal_cannot_be_removed_while_its_bridge_exists() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let m = m_node(&p);
    for x in ["1", "2", "3", "4"] {
        undrawn_wire(&mut p, t[x], m);
    }
    p.apply(Command::Unplace(f)).unwrap();
    // Free terminal 1 of its wire: its bridge still holds it.
    let w = p.undrawn.values().find(|w| w.from == Endpoint::Terminal(t["1"])).unwrap().id;
    p.apply(Command::RemoveWire(w)).unwrap();
    assert!(p.apply(Command::RemoveTerminal(t["1"])).is_err(), "bridged");
    p.apply(Command::RemoveBridge(Bridge::new(t["1"], t["2"]))).unwrap();
    p.apply(Command::RemoveTerminal(t["1"])).unwrap();
}

#[test]
fn a_failed_batch_rolls_back_terminals_and_bridges_exactly() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let m = m_node(&p);
    undrawn_wire(&mut p, t["1"], m);
    let before = p.clone();
    let bad = Bridge::new(t["1"], TerminalId(ConnectionId::new().0));
    let err = p.apply(Command::Batch(vec![
        Command::Unplace(f),
        Command::RemoveBridge(Bridge::new(t["1"], t["2"])),
        Command::AddBridge(bad),
    ]));
    assert!(err.is_err());
    assert_eq!(p, before, "terminals, bridges and drawings exactly as before");
    // A failed placement leaves no bridge behind either.
    let mut broken = repeat(&p, f, 30.0);
    broken.id = FunctionId::new();
    broken.terminals.clear();
    broken.page = ecad_model::PageId::new();
    assert!(p.apply(Command::Place(broken)).is_err());
    assert_eq!(p, before);
}

#[test]
fn reassigning_a_drawing_creates_the_new_devices_bridges_and_keeps_the_old_ones() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let old_t = p.function(f).unwrap().terminals.clone();
    let m = m_node(&p);
    for x in ["1", "2", "3", "4"] {
        undrawn_wire(&mut p, old_t[x], m);
    }
    let strip = p.device(p.function(f).unwrap().device).unwrap().clone();
    let new_dev = ecad_model::Device { id: ecad_model::DeviceId::new(), tag: "-X9".into(), ..strip };
    p.apply(Command::AddDevice(new_dev.clone())).unwrap();
    let bridges = p.bridges.clone();
    let before = p.clone();
    let undo = p
        .apply(Command::Reassign {
            id: f,
            device: new_dev.id,
            designations: Default::default(),
            terminals: Default::default(),
        })
        .unwrap();
    let new_t = p.function(f).unwrap().terminals.clone();
    assert!(new_t.values().all(|x| p.terminals[x].device == new_dev.id));
    assert!(bridges.iter().all(|b| p.bridges.contains(b)), "the old strip keeps its bridges (its points stay)");
    assert!(
        p.bridges.contains(&Bridge::new(new_t["1"], new_t["2"]))
            && p.bridges.contains(&Bridge::new(new_t["3"], new_t["4"]))
    );
    assert!(!p.bridges.contains(&Bridge::new(new_t["1"], new_t["3"])), "levels isolated");
    assert_eq!(save_and_load(&p).bridges, p.bridges);
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();
    assert_eq!(p.function(f).unwrap().terminals, new_t);
}

/// Graphical editing never changes what a drawn wire connects; only explicit engineering
/// operations (rebinding a node drawing, reassigning a drawing) do.
#[test]
fn graphical_operations_never_change_drawn_wire_ends() {
    let mut p = conveyor().unwrap();
    let original = ends(&p);
    let mut h = History::default();
    let w = p.connections.values().next().unwrap().clone();
    let f = w.from.function;
    let at = p.function(f).unwrap().position;
    let pages: Vec<_> = p.pages_in_order().iter().map(|pg| pg.id).collect();
    let mut reversed = pages.clone();
    reversed.reverse();
    let keys: BTreeMap<_, _> =
        pages.iter().enumerate().map(|(i, id)| (*id, (i as u64 + 1) * 3 * ecad_model::PAGE_STEP)).collect();
    let x3 = drawing_of(&p, "-X3", "1");
    let renumber: BTreeMap<String, String> =
        p.function(x3).unwrap().terminals.keys().map(|pt| (pt.clone(), "77".to_owned())).collect();
    for (name, cmd) in [
        ("move", Command::Move { id: f, to: Point::new(at.x + 8.0, at.y + 4.0) }),
        ("rotate", Command::Rotate { id: f, to: Rotation::R90 }),
        ("mirror", Command::Mirror { id: f, to: true }),
        ("bend", Command::SetWireBend { id: w.id, bend: Some(at.y + 6.0) }),
        ("reorder", Command::ReorderPages(reversed)),
        ("keys", Command::SetPageKeys(keys)),
        ("renumber", Command::SetDesignations { id: x3, designations: renumber }),
        ("wire number", Command::SetWireNumber { id: w.id, number: Some("G1".into()) }),
    ] {
        h.execute(&mut p, name, cmd).unwrap();
        assert_eq!(ends(&p), original, "{name} keeps every wire's ends");
    }
    assert_eq!(ends(&save_and_load(&p)), original);
    // Rebinding a junction or potential drawing is explicit rewiring: its wires now end on
    // the other node.
    let node_drawing = p
        .functions
        .values()
        .find(|d| d.node.is_some() && d.node.is_some() && p.node_kind_of(&d.symbol) == Some(NodeKind::Potential));
    if let Some(d) = node_drawing.cloned() {
        let target = ecad_model::NodeEntry {
            id: ecad_model::NodeId::new(),
            kind: NodeKind::Potential,
            name: "REBOUND".into(),
            scope: ecad_model::PROJECT_SCOPE.into(),
            description: String::new(),
        };
        h.execute(&mut p, "node", Command::AddNode(target.clone())).unwrap();
        h.execute(&mut p, "rebind", Command::Rebind { id: d.id, node: target.id }).unwrap();
        assert_ne!(ends(&p), original, "rebinding is explicit rewiring");
        assert!(
            ends(&p)
                .values()
                .any(|(a, b)| *a == Some(Endpoint::Node(target.id)) || *b == Some(Endpoint::Node(target.id)))
        );
    }
    while h.undo(&mut p).unwrap().is_some() {}
    assert_eq!(p, conveyor().unwrap());
}

#[test]
fn bridges_json_is_required_and_validated_on_load() {
    let p = conveyor().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let path = tmp.path().join("bridges.json");
    let good = std::fs::read_to_string(&path).unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_str(&good).unwrap();
    assert_eq!(list.len(), p.bridges.len());
    let write = |v: &serde_json::Value| std::fs::write(&path, serde_json::to_string(v).unwrap()).unwrap();
    // Ends out of order.
    let mut swapped = list.clone();
    let (a, b) = (swapped[0]["a"].clone(), swapped[0]["b"].clone());
    swapped[0]["a"] = b;
    swapped[0]["b"] = a;
    write(&serde_json::Value::Array(swapped));
    assert!(ecad_project::load(tmp.path()).is_err(), "out-of-order ends refused");
    // Duplicate.
    let mut dup = list.clone();
    dup.push(list[0].clone());
    write(&serde_json::Value::Array(dup));
    assert!(ecad_project::load(tmp.path()).is_err(), "duplicate refused");
    // Unknown terminal.
    let mut unknown = list.clone();
    unknown[0]["b"] = serde_json::Value::String("ffffffff-ffff-ffff-ffff-ffffffffffff".into());
    write(&serde_json::Value::Array(unknown));
    assert!(ecad_project::load(tmp.path()).is_err(), "bridge to a missing terminal refused");
    // A bridge dropped by hand: the drawing that depicts it no longer loads (fail closed).
    write(&serde_json::Value::Array(list[1..].to_vec()));
    assert!(ecad_project::load(tmp.path()).is_err(), "a depicted bridge must exist");
    // Missing file.
    std::fs::remove_file(&path).unwrap();
    assert!(ecad_project::load(tmp.path()).is_err(), "bridges.json is required");
    std::fs::write(&path, good).unwrap();
    assert_eq!(ecad_project::load(tmp.path()).unwrap(), p);
}

/// R8: a PLC channel no drawing shows keeps its typed meaning, reserves its address, and
/// stays in the PLC I/O list; undrawn strip terminals stay in the terminal diagram.
#[test]
fn undrawn_plc_channels_and_strip_terminals_stay_typed_and_reported() {
    let mut p = conveyor().unwrap();
    let card = p.device_by_tag("-A1").unwrap().id;
    let channel = p.plc_channels_of(card).into_iter().next().unwrap();
    let f = channel.function;
    let drawing = p.function(f).unwrap().clone();
    let t = *drawing.terminals.values().find(|t| p.terminals[*t].channel.is_some()).unwrap();
    let address = p.terminals[&t].channel.as_ref().unwrap().address.clone();
    let kind = p.terminals[&t].channel.as_ref().unwrap().kind;
    let m = m_node(&p);
    undrawn_wire(&mut p, t, m);
    let next_before = p.next_address(kind);
    let rows_before = ecad_reports::plc_io_list(&p).len();
    let before = p.clone();
    let undo = p.apply(Command::Unplace(f)).unwrap();
    // Typed from the canonical record.
    let c = p.connectivity();
    let TerminalKind::PlcChannel(ch) = &c.terminals[&t].kind else { panic!("{:?}", c.terminals[&t].kind) };
    assert_eq!((ch.kind, ch.address_text.as_str()), (kind, address.as_str()));
    assert!(ch.address.is_some());
    // Still reserved: the next free address does not hand it out again.
    assert_eq!(p.next_address(kind), next_before);
    // Still listed, as not drawn, with what its wire reaches.
    let rows = ecad_reports::plc_io_list(&p);
    assert_eq!(rows.len(), rows_before);
    let row = rows.iter().find(|r| r.address == address).unwrap();
    assert_eq!((row.card.as_str(), row.location.as_str(), row.wired_to.as_str()), ("-A1", "not drawn", "M"));
    // A second channel on the same address is refused even though the first is undrawn.
    let mut clash = repeat(&before, f, 10.0);
    clash.terminals.clear();
    clash.properties.insert(ecad_model::ADDRESS.into(), address.clone());
    assert!(p.apply(Command::Place(clash)).is_err(), "address still reserved");
    // Saved and loaded.
    let loaded = save_and_load(&p);
    assert_eq!(ecad_reports::plc_io_list(&loaded), rows);
    // Drawn again by the same terminal: the drawn row returns.
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();

    // Strip terminals no drawing shows.
    let mut q = conveyor().unwrap();
    let x3 = drawing_of(&q, "-X3", "1");
    let tt = q.function(x3).unwrap().terminals.clone();
    let qm = m_node(&q);
    for x in ["1", "2", "3", "4"] {
        undrawn_wire(&mut q, tt[x], qm);
    }
    q.apply(Command::Unplace(x3)).unwrap();
    let c = q.connectivity();
    assert!(matches!(c.terminals[&tt["1"]].kind, TerminalKind::Strip));
    let rows = ecad_reports::terminal_diagram(&q);
    let undrawn: Vec<_> = rows.iter().filter(|r| r.strip == "-X3" && r.location == "not drawn").collect();
    assert_eq!(undrawn.len(), 2, "two levels of one four-point terminal: {rows:#?}");
    assert!(undrawn.iter().all(|r| r.inside == "M" && r.outside == "M"));
}


#[test]
fn detach_retains_intermediate_bridge_points_and_external_canonical_endpoints() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let extra = ecad_model::TerminalEntry {
        id: TerminalId(ConnectionId::new().0),
        point: "extra".into(),
        ..p.terminals[&t["2"]].clone()
    };
    p.apply(Command::AddTerminal(extra.clone())).unwrap();
    p.apply(Command::AddBridge(Bridge::new(t["2"], extra.id))).unwrap();
    let m = m_node(&p);
    undrawn_wire(&mut p, t["1"], m);
    let before = p.clone();
    let same_net = |p: &Project| p.netlist().nets.iter().any(|n| {
        n.endpoints.contains(&Endpoint::Terminal(t["1"]))
            && n.endpoints.contains(&Endpoint::Terminal(extra.id))
    });
    let undo = p.apply(Command::Unplace(f)).unwrap();
    assert!(same_net(&p));
    assert!(p.terminals.contains_key(&t["2"]), "unwired intermediate point survives");
    assert!(p.bridges.contains(&Bridge::new(t["2"], extra.id)));
    assert!(!p.terminals.contains_key(&t["3"]) && !p.terminals.contains_key(&t["4"]), "unused isolated level removed");
    let detached = p.clone();
    assert_eq!(save_and_load(&p), detached);
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();
    assert_eq!(p, detached);
    p = before.clone();
    let device = ecad_model::Device { id: ecad_model::DeviceId::new(), tag: "-X90".into(), ..p.device(p.function(f).unwrap().device).unwrap().clone() };
    p.apply(Command::AddDevice(device.clone())).unwrap();
    let before = p.clone();
    let cmd = Command::Reassign { id: f, device: device.id, designations: Default::default(), terminals: Default::default() };
    let bad = Command::AddBridge(Bridge::new(t["1"], TerminalId(ConnectionId::new().0)));
    assert!(p.apply(Command::Batch(vec![cmd.clone(), bad])).is_err());
    assert_eq!(p, before);
    let undo = p.apply(cmd).unwrap();
    assert!(same_net(&p));
    assert_eq!(save_and_load(&p), p);
    let moved = p.clone();
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();
    assert_eq!(p, moved);
}

#[test]
fn save_refuses_a_bridge_inconsistent_with_its_drawing() {
    let mut p = conveyor().unwrap();
    let f = drawing_of(&p, "-X3", "1");
    let t = p.function(f).unwrap().terminals.clone();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let before = p.clone();
    p.bridges.remove(&Bridge::new(t["1"], t["2"]));
    assert!(ecad_project::save(&p, tmp.path()).is_err());
    assert_eq!(ecad_project::load(tmp.path()).unwrap(), before);
}
