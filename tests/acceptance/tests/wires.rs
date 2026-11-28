//! M10d: wires are persistent, with terminal or node ends (D1), separate from how they are
//! drawn. Format 2 keeps every wire in `wires.json` and only its graphics in the page files.
//! A wire's id and ends survive renumbering, moving and re-routing its drawings, save and load,
//! and undo and redo; a wire can exist without a drawing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::{Command, ConnectionId, ConnectionRef, Endpoint, NodeKind, Point, Project, Wire, conveyor::conveyor};
use std::collections::BTreeMap;

fn ends(p: &Project) -> BTreeMap<ConnectionId, (Option<Endpoint>, Option<Endpoint>)> {
    let c = p.connectivity();
    let mut out: BTreeMap<_, _> =
        p.connections.values().map(|w| (w.id, (c.endpoint(&w.from), c.endpoint(&w.to)))).collect();
    out.extend(p.undrawn.values().map(|w| (w.id, (Some(w.from), Some(w.to)))));
    out
}

fn save_and_load(p: &Project) -> Project {
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(p, tmp.path()).unwrap();
    ecad_project::load(tmp.path()).unwrap()
}

fn point(p: &Project, tag: &str, designation: &str) -> ConnectionRef {
    let d = p.device_by_tag(tag).unwrap().id;
    let f = p.functions_of(d).find(|f| f.terminals.keys().any(|pt| p.designation(f, pt) == designation)).unwrap();
    let pt = f.terminals.keys().find(|pt| p.designation(f, pt) == designation).unwrap();
    ConnectionRef::new(f.id, pt)
}

#[test]
fn format_2_keeps_wires_apart_from_their_graphics() {
    let p = conveyor().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let wires: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(tmp.path().join("wires.json")).unwrap()).unwrap();
    assert_eq!(wires.len(), p.connections.len());
    assert!(
        wires.iter().all(|w| w["from"]["type"].is_string() && w["to"]["type"].is_string()),
        "terminal or node ends"
    );
    for e in std::fs::read_dir(tmp.path().join("pages")).unwrap() {
        let page: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(e.unwrap().path()).unwrap()).unwrap();
        assert!(page.get("connections").is_none(), "no electrical data in page files");
        for g in page["wires"].as_array().into_iter().flatten() {
            assert!(g["wire"].is_string() && g["from"]["function"].is_string());
            assert!(g.get("wire_number").is_none() && g.get("cross_section").is_none());
        }
    }
}

#[test]
fn a_wire_keeps_its_identity_through_renumbering_moves_routing_saves_and_undo() {
    let mut p = conveyor().unwrap();
    let before = ends(&p);
    let mut history = ecad_model::History::default();
    let w = p.connections.values().find(|w| w.wire_number.is_some()).unwrap().clone();
    let f = w.from.function;
    let at = p.function(f).unwrap().position;
    history.execute(&mut p, "renumber", Command::SetWireNumber { id: w.id, number: Some("X99".into()) }).unwrap();
    history.execute(&mut p, "move", Command::Move { id: f, to: Point::new(at.x + 4.0, at.y) }).unwrap();
    history.execute(&mut p, "bend", Command::SetWireBend { id: w.id, bend: Some(at.y + 2.0) }).unwrap();
    assert_eq!(ends(&p), before, "the same wires with the same ends");
    let loaded = save_and_load(&p);
    assert_eq!(ends(&loaded), before);
    assert_eq!(loaded.connection(w.id).unwrap().wire_number.as_deref(), Some("X99"));
    for _ in 0..3 {
        history.undo(&mut p).unwrap();
    }
    assert_eq!(p, conveyor().unwrap());
    for _ in 0..3 {
        history.redo(&mut p).unwrap();
    }
    assert_eq!(ends(&p), before);
}

#[test]
fn a_wire_need_not_be_drawn() {
    let mut p = conveyor().unwrap();
    let h1 = p.connectivity().endpoint(&point(&p, "-H1", "X2")).unwrap();
    let m = Endpoint::Node(p.node_by_name(NodeKind::Potential, "M", ecad_model::PROJECT_SCOPE).unwrap().id);
    let id = ConnectionId::new();
    let undo = p
        .apply(Command::AddWire(Wire {
            id,
            from: h1,
            to: m,
            wire_number: Some("W200".into()),
            spec: Default::default(),
        }))
        .unwrap();
    // In the netlist, the wire list and the checks, as not drawn yet.
    let net = p.netlist().nets.into_iter().find(|n| n.wires.contains(&id)).unwrap();
    assert_eq!(net.names, ["M"]);
    let rows = ecad_reports::wire_list(&p).unwrap();
    let row = rows.iter().find(|r| r.wire == "W200").unwrap();
    assert_eq!(
        (row.from_tag.as_str(), row.from_point.as_str(), row.to_tag.as_str(), row.route.as_str()),
        ("-H1", "X2", "M", "not drawn")
    );
    let checks = ecad_rules::check(&p);
    assert!(checks.iter().any(|c| c.rule == "wire-not-drawn" && c.message.contains("W200")), "{checks:#?}");
    // Saved and loaded: the same wire, still undrawn.
    let loaded = save_and_load(&p);
    assert_eq!(loaded.undrawn, p.undrawn);
    // Its ends can't go while it ends on them.
    let ecad_model::Endpoint::Node(m_node) = m else { unreachable!() };
    assert!(p.apply(Command::RemoveNode(m_node)).is_err());
    p.apply(undo).unwrap();
    assert_eq!(p, conveyor().unwrap());
}

#[test]
fn page_graphics_that_dont_show_a_wires_ends_are_refused_on_load() {
    let p = conveyor().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    // Point the first wire graphics of a page at another wire's point: the file says one
    // thing about the wire, the page another.
    let path = std::fs::read_dir(tmp.path().join("pages")).unwrap().map(|e| e.unwrap().path()).min().unwrap();
    let mut page: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let graphics = page["wires"].as_array_mut().unwrap();
    let other_to = graphics[1]["to"].clone();
    let other_from = graphics[1]["from"].clone();
    graphics[1]["from"] = graphics[0]["from"].clone();
    graphics[1]["to"] = graphics[0]["to"].clone();
    graphics[0]["from"] = other_from;
    graphics[0]["to"] = other_to;
    std::fs::write(&path, serde_json::to_string_pretty(&page).unwrap()).unwrap();
    let e = ecad_project::load(tmp.path()).unwrap_err().to_string();
    assert!(e.contains("don't show wire"), "{e}");
}

#[test]
fn format_2_refuses_missing_or_duplicate_canonical_records() {
    let p = conveyor().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for file in ["products.json", "terminals.json", "nodes.json", "wires.json"] {
        ecad_project::save(&p, root).unwrap();
        std::fs::remove_file(root.join(file)).unwrap();
        assert!(ecad_project::load(root).is_err(), "{file}");
    }
    ecad_project::save(&p, root).unwrap();
    let path = root.join("wires.json");
    let mut wires: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    wires.push(wires[0].clone());
    std::fs::write(&path, serde_json::to_vec(&wires).unwrap()).unwrap();
    assert!(ecad_project::load(root).is_err(), "duplicate wire id");
    ecad_project::save(&p, root).unwrap();
    let path = root.join("terminals.json");
    let mut terminals: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    terminals.remove(0);
    std::fs::write(&path, serde_json::to_vec(&terminals).unwrap()).unwrap();
    assert!(ecad_project::load(root).is_err(), "missing terminal record");
}

#[test]
fn undrawn_wire_reserves_cable_core_and_number() {
    let mut p = conveyor().unwrap();
    let from = p.connectivity().endpoint(&point(&p, "-H1", "X2")).unwrap();
    let to = Endpoint::Node(p.node_by_name(NodeKind::Potential, "M", ecad_model::PROJECT_SCOPE).unwrap().id);
    let used = p.connections.values().find_map(|c| c.spec.cable.clone()).unwrap();
    let wire = Wire {
        id: ConnectionId::new(),
        from,
        to,
        wire_number: None,
        spec: ecad_model::WireSpec { cable: Some(used.clone()), ..Default::default() },
    };
    let before = p.clone();
    assert!(p.apply(Command::AddWire(wire.clone())).is_err());
    assert_eq!(p, before);
    let core = ecad_model::CableCore { core: "test-unused".into(), ..used };
    let page = p.pages_in_order()[0].id;
    let number = p.next_wire_number(page);
    let wire = Wire {
        wire_number: Some(number.clone()),
        spec: ecad_model::WireSpec { cable: Some(core.clone()), ..Default::default() },
        ..wire
    };
    p.apply(Command::AddWire(wire.clone())).unwrap();
    assert_ne!(p.next_wire_number(page), number);
    assert!(p.apply(Command::RemoveCable(core.cable)).is_err());
    let duplicate = Wire { id: ConnectionId::new(), wire_number: None, ..wire };
    assert!(p.apply(Command::AddWire(duplicate)).is_err());
    assert!(ecad_reports::cable_list(&p).iter().any(|r| r.core == "test-unused" && r.wire == number));
    assert_eq!(p, save_and_load(&p));
}

#[test]
fn undrawn_wire_short_is_reported_as_an_electrical_error() {
    let mut p = conveyor().unwrap();
    let node = |name| Endpoint::Node(p.node_by_name(NodeKind::Potential, name, ecad_model::PROJECT_SCOPE).unwrap().id);
    let from = node("L+");
    let to = node("M");
    let wire = Wire { id: ConnectionId::from_u128(1), from, to, wire_number: None, spec: Default::default() };
    p.apply(Command::AddWire(wire)).unwrap();
    assert!(ecad_rules::check(&p).iter().any(|c| c.rule == "potential-clash"), "undrawn short must not disappear");
}

#[test]
fn terminal_records_survive_unplace_and_undo_without_reconstruction() {
    let mut p = conveyor().unwrap();
    let f = point(&p, "-H1", "X2").function;
    let t = p.function(f).unwrap().terminals["X2"];
    let entry = p.terminals.get_mut(&t).unwrap();
    entry.point = "custom-physical-point".into();
    let before = p.clone();
    let undo = p.apply(Command::Unplace(f)).unwrap();
    let removed = p.clone();
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();
    assert_eq!(p, removed);
}

#[test]
fn deleting_a_bridge_drawing_cannot_disconnect_surviving_undrawn_endpoints() {
    let mut p = conveyor().unwrap();
    let f = point(&p, "-X3", "1").function;
    let drawing = p.function(f).unwrap().clone();
    let terminal = |name| Endpoint::Terminal(drawing.terminals[name]);
    let m = Endpoint::Node(p.node_by_name(NodeKind::Potential, "M", ecad_model::PROJECT_SCOPE).unwrap().id);
    for end in [terminal("1"), terminal("2")] {
        p.apply(Command::AddWire(Wire {
            id: ConnectionId::new(),
            from: end,
            to: m,
            wire_number: None,
            spec: Default::default(),
        }))
        .unwrap();
    }
    let before = p.clone();
    assert_eq!(p.apply(Command::Unplace(f)).unwrap_err(), ecad_model::ModelError::BridgeInUse);
    assert_eq!(p, before);
}

#[test]
fn renumber_includes_undrawn_points_of_the_same_physical_group() {
    let mut p = conveyor().unwrap();
    let f = point(&p, "-X3", "1").function;
    let entry = p.terminals[&p.function(f).unwrap().terminals["1"]].clone();
    let extra = ecad_model::TerminalEntry {
        id: ecad_model::TerminalId(ConnectionId::new().0),
        point: "extra".into(),
        ..entry.clone()
    };
    p.apply(Command::AddTerminal(extra.clone())).unwrap();
    let duplicate = ecad_model::TerminalEntry { id: ecad_model::TerminalId(ConnectionId::new().0), ..extra.clone() };
    assert!(p.apply(Command::AddTerminal(duplicate)).is_err());
    let before = p.clone();
    let undo = p.apply(Command::SetDesignations { id: f, designations: [("1".into(), "90".into())].into() }).unwrap();
    assert_eq!(p.terminals[&extra.id].designation, "90");
    p.apply(undo).unwrap();
    assert_eq!(p, before);
}
