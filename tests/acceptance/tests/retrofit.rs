//! S0 mandatory retrofit acceptance: a realistic modification of the installed conveyor.
//!
//! Starting from the original project: plan an extension (four DI addresses reserved), insert
//! a schematic page between pages 3 and 4, add a sensor, a PLC input for it, terminals, wires
//! and a cable whose cores are allocated, remove the jog button, replace the motor protector
//! through the explicit replacement operation, and join two numbered potentials.
//!
//! Every step goes through the model's shared allocation (the same calls the editor, the CLI
//! and MCP make) and one history. The test proves by id that everything outside the
//! modification keeps its identity, designation, resource assignment and drawing geometry,
//! that removal renumbers nothing and retires what it frees, and that this holds after save and
//! load and through undo and redo. It runs on the conveyor as built (one number per wire) and
//! numbered per potential.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::{
    Cable, CableId, Command, Connection, ConnectionId, ConnectionRef, Device, DeviceId, Function, FunctionId, History,
    Identifier, IoKind, PageId, Point, Project, Record, Rotation, SymbolId, TerminalId,
};
use std::collections::{BTreeMap, BTreeSet};

fn page(p: &Project, name: &str) -> PageId {
    p.pages.values().find(|pg| pg.name == name).unwrap().id
}

fn exec(p: &mut Project, h: &mut History, label: &str, cmd: Command) {
    h.execute(p, label, cmd).unwrap_or_else(|e| panic!("{label}: {e}"));
}

/// A new device drawn with `symbol`: the next tag, allocated designations and properties.
fn place_new(p: &mut Project, h: &mut History, symbol: &str, page: PageId, at: (f64, f64)) -> FunctionId {
    let symbol = SymbolId::new(symbol);
    let tag = p.new_tag(&symbol, page);
    let device = DeviceId::new();
    let id = FunctionId::new();
    let mut probe = p.clone();
    probe.apply(Command::AddDevice(Device::new(device, &tag, ""))).unwrap();
    let designations = probe.allocate_designations(device, &symbol).unwrap();
    let f = drawing(id, device, symbol.clone(), page, at, designations, p.new_drawing_properties(&symbol));
    exec(
        p,
        h,
        &format!("place {tag}"),
        Command::Batch(vec![Command::AddDevice(Device::new(device, &tag, "")), Command::Place(f)]),
    );
    id
}

/// Another drawing of an existing device: a strip's next terminal, a card's next channel.
fn place_another(
    p: &mut Project,
    h: &mut History,
    symbol: &str,
    tag: &str,
    page: PageId,
    at: (f64, f64),
) -> FunctionId {
    let symbol = SymbolId::new(symbol);
    let device = p.device_by_tag(tag).unwrap().id;
    let designations = p.allocate_designations(device, &symbol).unwrap();
    let id = FunctionId::new();
    let f = drawing(id, device, symbol.clone(), page, at, designations, p.new_drawing_properties(&symbol));
    exec(p, h, &format!("place {tag}"), Command::Place(f));
    id
}

fn drawing(
    id: FunctionId,
    device: DeviceId,
    symbol: SymbolId,
    page: PageId,
    at: (f64, f64),
    designations: BTreeMap<String, String>,
    properties: BTreeMap<String, String>,
) -> Function {
    Function {
        id,
        device,
        node: None,
        terminals: Default::default(),
        symbol,
        page,
        position: Point::new(at.0, at.1),
        rotation: Rotation::R0,
        designations,
        properties,
        mirrored: false,
    }
}

fn wire(p: &mut Project, h: &mut History, from: (FunctionId, &str), to: (FunctionId, &str)) -> ConnectionId {
    let (from, to) = (ConnectionRef::new(from.0, from.1), ConnectionRef::new(to.0, to.1));
    let number = p.number_for_new_wire(&from, &to);
    let id = ConnectionId::new();
    let c = Connection { id, from, to, wire_number: Some(number), spec: Default::default(), bend: None };
    exec(p, h, "wire", Command::Connect(c));
    id
}

/// What the retrofit touches by design; everything else must be identical by id.
#[derive(Default)]
struct Scope {
    devices: BTreeSet<DeviceId>,
    terminals: BTreeSet<TerminalId>,
    functions: BTreeSet<FunctionId>,
    wires: BTreeSet<ConnectionId>,
}

/// Every pre-existing entity outside `scope` is unchanged: same id and the same record (tags,
/// designations, PLC channel data, wire numbers, cores, ends, positions, pages, nodes,
/// bridges, cables, layout).
fn assert_unaffected(before: &Project, after: &Project, scope: &Scope, when: &str) {
    for (id, d) in &before.devices {
        if !scope.devices.contains(id) {
            assert_eq!(after.devices.get(id), Some(d), "{when}: device {}", d.tag);
        }
    }
    for (id, t) in &before.terminals {
        if !scope.terminals.contains(id) {
            assert_eq!(after.terminals.get(id), Some(t), "{when}: terminal {}", t.designation);
        }
    }
    for (id, f) in &before.functions {
        if !scope.functions.contains(id) {
            assert_eq!(after.functions.get(id), Some(f), "{when}: drawing {}", before.label_of(f));
        }
    }
    for (id, c) in &before.connections {
        if !scope.wires.contains(id) {
            assert_eq!(after.connections.get(id), Some(c), "{when}: wire {:?}", c.wire_number);
        }
    }
    let before_ends = before.connectivity();
    let after_ends = after.connectivity();
    for (id, c) in &before.connections {
        if !scope.wires.contains(id) {
            let a = after.connection(*id).unwrap();
            assert_eq!(
                (before_ends.endpoint(&c.from), before_ends.endpoint(&c.to)),
                (after_ends.endpoint(&a.from), after_ends.endpoint(&a.to)),
                "{when}: ends of {:?}",
                c.wire_number
            );
        }
    }
    for (id, pg) in &before.pages {
        assert_eq!(after.pages.get(id), Some(pg), "{when}: page {} keeps its name and key", pg.name);
    }
    assert_eq!(after.nodes, before.nodes, "{when}: nodes");
    for (id, c) in &before.cables {
        assert_eq!(after.cables.get(id), Some(c), "{when}: cable {}", c.tag);
    }
    let removed: BTreeSet<TerminalId> =
        scope.terminals.iter().filter(|t| !after.terminals.contains_key(t)).copied().collect();
    for b in &before.bridges {
        if !removed.contains(&b.a) && !removed.contains(&b.b) {
            assert!(after.bridges.contains(b), "{when}: bridge kept");
        }
    }
    assert_eq!(after.panel_items, before.panel_items, "{when}: layout positions");
    assert_eq!(after.boxes, before.boxes, "{when}: location boxes");
}

struct Retrofit {
    after: Project,
    scope: Scope,
    joined: (String, String, ConnectionId),
}

fn retrofit(original: &Project) -> (Project, History, Retrofit) {
    let mut p = original.clone();
    let mut h = History::default();

    // 0. The planned extension keeps the next four inputs free.
    let next = p.next_address(IoKind::DigitalIn);
    assert_eq!(next, "I1.1");
    let reserved: Vec<Record> = ["I1.1", "I1.2", "I1.3", "I1.4"]
        .iter()
        .map(|a| Record::new(Identifier::Address { address: (*a).into() }, "extension"))
        .collect();
    exec(&mut p, &mut h, "reserve", Command::Reserve(reserved));

    // 1. A schematic page between 3 and 4: 3a, no other page renamed or re-keyed.
    let new_page = ecad_model::Page::new("", 0);
    let page_3a = new_page.id;
    let cmds = p.insert_page(Some(page(&p, "3")), new_page);
    assert_eq!(cmds.len(), 1, "a key between its neighbours: one command");
    exec(&mut p, &mut h, "page", Command::Batch(cmds));
    assert_eq!(p.pages[&page_3a].name, "3a");

    // 2. A sensor: the next -B tag, sequentially.
    let b1 = p.device_by_tag("-B1").unwrap().id;
    let sensor_symbol = p.functions_of(b1).next().unwrap().symbol.0.clone();
    let sensor = place_new(&mut p, &mut h, &sensor_symbol, page_3a, (100.0, 112.0));
    let b3 = p.function(sensor).unwrap().device;
    assert_eq!(p.devices[&b3].tag, "-B3");

    // 3. Its PLC input: the card's next channel, the next address past the reserved four.
    let input = place_another(&mut p, &mut h, "iec.plc_di", "-A1", page_3a, (160.0, 56.0));
    let channel_t = *p.function(input).unwrap().terminals.values().next().unwrap();
    let ch = p.terminals[&channel_t].clone();
    assert_eq!(ch.designation, "13", "after channels 0…12");
    assert_eq!(ch.channel.as_ref().unwrap().address, "I1.5", "skips the reserved I1.1…I1.4");

    // 4. Terminals on -X3: the strip's next numbers.
    let x5 = place_another(&mut p, &mut h, "iec.terminal", "-X3", page_3a, (100.0, 160.0));
    let x6 = place_another(&mut p, &mut h, "iec.terminal", "-X3", page_3a, (160.0, 160.0));
    let number = |p: &Project, f: FunctionId| p.designation(p.function(f).unwrap(), "1").to_owned();
    assert_eq!((number(&p, x5), number(&p, x6)), ("5".into(), "6".into()));

    // 5. Wires and a cable whose core is allocated.
    let w_a = wire(&mut p, &mut h, (sensor, "BK"), (x5, "2"));
    let w_b = wire(&mut p, &mut h, (x6, "1"), (input, "1"));
    let cable_tag = p.new_cable_tag("-W");
    assert_eq!(cable_tag, "-W4");
    let cable = CableId::new();
    exec(
        &mut p,
        &mut h,
        "cable",
        Command::AddCable(Cable { id: cable, tag: cable_tag, kind: "3x0,34 M12".into(), cores: 3, shielded: false }),
    );
    let core = p.allocate_core(cable).unwrap();
    assert_eq!(core, "1");
    let spec = ecad_model::WireSpec { cable: Some(ecad_model::CableCore { cable, core }), ..Default::default() };
    exec(&mut p, &mut h, "core", Command::SetWireSpec { id: w_a, spec });
    assert_eq!(p.allocate_core(cable).unwrap(), "2", "the next core");

    // 6. Remove the jog button -S3: nothing renumbered; its tag and wire numbers retired; its
    // PLC input (I0.2) stays assigned on the card.
    let s3 = p.device_by_tag("-S3").unwrap().id;
    let s3_drawings: Vec<FunctionId> = p.functions_of(s3).map(|f| f.id).collect();
    let s3_terminals: BTreeSet<TerminalId> = p.terminals.values().filter(|t| t.device == s3).map(|t| t.id).collect();
    let s3_wires: BTreeSet<ConnectionId> = p
        .connections
        .values()
        .filter(|c| s3_drawings.contains(&c.from.function) || s3_drawings.contains(&c.to.function))
        .map(|c| c.id)
        .collect();
    let s3_numbers: BTreeSet<String> = s3_wires.iter().filter_map(|c| p.wire_number(*c).map(str::to_owned)).collect();
    let mut cmds: Vec<Command> = s3_drawings.iter().map(|f| Command::Unplace(*f)).collect();
    cmds.push(Command::RemoveDevice(s3));
    let cmds = p.removal(cmds).unwrap();
    exec(&mut p, &mut h, "remove -S3", Command::Batch(cmds));
    assert!(p.numbering.retired.contains_key(&Identifier::Tag { tag: "-S3".into() }));
    for n in &s3_numbers {
        if !p.wire_number_in_use_by_others(n) {
            assert!(p.numbering.retired.contains_key(&Identifier::Wire { number: n.clone() }), "{n} retired");
        }
    }
    assert!(p.terminals.values().any(|t| t.channel.as_ref().is_some_and(|c| c.address == "I0.2")), "the input stays");
    assert_eq!(p.next_tag("-S"), "-S4", "-S3 is retired, not reused");

    // 7. Replace -Q1 with a product whose auxiliary contact is 53-54: same device, explicit
    // terminal reconciliation.
    let q1 = p.device_by_tag("-Q1").unwrap().id;
    let renames: BTreeMap<TerminalId, String> = p
        .terminals
        .values()
        .filter(|t| t.device == q1)
        .filter_map(|t| match t.designation.as_str() {
            "13" => Some((t.id, "53".to_owned())),
            "14" => Some((t.id, "54".to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(renames.len(), 2);
    let mut product = ecad_model::Part::new("EX-MPCB-1.6B");
    product.description = "Motor protection circuit breaker 1.1-1.6 A, 1 NO (53-54)".into();
    let parts = vec![ecad_model::PartRef::to(product.id, 1)];
    exec(&mut p, &mut h, "product", Command::AddPart(product));
    exec(&mut p, &mut h, "replace -Q1", Command::ReplaceDevice { id: q1, parts, designations: renames.clone() });
    assert!(!p.numbering.retired.contains_key(&Identifier::Tag { tag: "-Q1".into() }), "replacement retires nothing");

    // 8. Join the two numbered potentials (sensor side and input side) through the terminals.
    let na = p.wire_number(w_a).unwrap().to_owned();
    let nb = p.wire_number(w_b).unwrap().to_owned();
    let before_join = p.clone();
    let w_join = wire(&mut p, &mut h, (x5, "1"), (x6, "2"));
    assert_eq!(p.wire_number(w_a), Some(na.as_str()), "neither side is renumbered");
    assert_eq!(p.wire_number(w_b), Some(nb.as_str()));
    for (id, c) in &before_join.connections {
        assert_eq!(p.connections[id].wire_number, c.wire_number, "joining renumbers nothing");
    }

    let mut scope = Scope::default();
    scope.devices.extend([s3, q1]);
    scope.terminals.extend(s3_terminals);
    scope.terminals.extend(renames.keys().copied());
    scope.functions.extend(s3_drawings);
    scope.wires.extend(s3_wires);
    let after = p.clone();
    (p, h, Retrofit { after, scope, joined: (na, nb, w_join) })
}

trait InUse {
    fn wire_number_in_use_by_others(&self, n: &str) -> bool;
}

impl InUse for Project {
    fn wire_number_in_use_by_others(&self, n: &str) -> bool {
        self.connections.values().any(|c| c.wire_number.as_deref() == Some(n))
            || self.undrawn.values().any(|w| w.wire_number.as_deref() == Some(n))
    }
}

fn save_and_load(p: &Project) -> Project {
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(p, tmp.path()).unwrap();
    ecad_project::load(tmp.path()).unwrap()
}

fn run(original: Project, per_potential: bool) {
    let (mut p, mut h, r) = retrofit(&original);
    assert_unaffected(&original, &p, &r.scope, "after the retrofit");
    // Numbering situation after the join, reported explicitly.
    let checks = ecad_rules::check(&p);
    let (na, nb, w_join) = &r.joined;
    if per_potential {
        assert_ne!(na, nb);
        assert_eq!(p.wire_number(*w_join), Some(na.as_str()), "the from side's number");
        let differ: Vec<_> = checks.iter().filter(|c| c.rule == "wire-numbers-differ").collect();
        assert!(
            differ.iter().any(|c| c.message.contains(na.as_str()) && c.message.contains(nb.as_str())),
            "{differ:#?}"
        );
    } else {
        assert!(p.wire_number(*w_join).is_some_and(|n| n != na && n != nb), "a number of its own");
    }
    assert!(!checks.iter().any(|c| c.rule == "potential-clash"), "the join is no short");
    assert!(!checks.iter().any(|c| c.rule == "wire-number-shared"));
    // No retired identifier was handed out again.
    for id in p.numbering.retired.keys() {
        assert!(!p.in_use(id), "{} reused", p.describe_identifier(id));
    }

    // Saved and loaded: identical, and the comparison holds again.
    let loaded = save_and_load(&p);
    assert_eq!(loaded, r.after);
    assert_unaffected(&original, &loaded, &r.scope, "after save and load");
    assert_eq!(loaded.content_hash().unwrap(), r.after.content_hash().unwrap());

    // Undone step by step to the original exactly, then redone to the retrofit exactly.
    let mut steps = 0;
    while h.undo(&mut p).unwrap().is_some() {
        steps += 1;
    }
    assert!(steps >= 12, "{steps}");
    assert_eq!(p, original, "undo restores the original exactly");
    while h.redo(&mut p).unwrap().is_some() {}
    assert_eq!(p, r.after, "redo restores the retrofit exactly");
    assert_unaffected(&original, &p, &r.scope, "after undo and redo");
    let reloaded = save_and_load(&p);
    assert_eq!(reloaded, r.after);
}

#[test]
fn the_conveyor_retrofit_changes_only_what_it_modifies() {
    run(ecad_model::conveyor::conveyor().unwrap(), false);
}

#[test]
fn the_conveyor_retrofit_numbered_per_potential_keeps_both_joined_numbers() {
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    // The installation as numbered per potential (the original documentation).
    p.apply(Command::SetNaming(ecad_model::Naming {
        wire_numbering: ecad_model::WireNumbering::PerPotential,
        ..Default::default()
    }))
    .unwrap();
    p.apply(Command::Batch(p.renumber_all_wires())).unwrap();
    run(p, true);
}

/// Joining two named potentials is still a short: conflict detection is unchanged, and still
/// nothing is renumbered.
#[test]
fn joining_named_potentials_is_still_reported_as_a_clash() {
    let mut p = ecad_model::conveyor::conveyor().unwrap();
    let l_plus = p.functions.values().find(|f| f.node.is_some() && p.label_of(f) == "L+").unwrap().id;
    let m = p.functions.values().find(|f| f.node.is_some() && p.label_of(f) == "M").unwrap().id;
    let numbers: BTreeMap<ConnectionId, Option<String>> =
        p.connections.values().map(|c| (c.id, c.wire_number.clone())).collect();
    let mut h = History::default();
    // Free points: a new drawing of each potential.
    let page5 = page(&p, "5");
    let a = FunctionId::new();
    let b = FunctionId::new();
    for (id, from) in [(a, l_plus), (b, m)] {
        let mut f = p.function(from).unwrap().clone();
        f.id = id;
        f.page = page5;
        f.position = Point::new(f.position.x + 300.0, f.position.y);
        exec(&mut p, &mut h, "point", Command::Place(f));
    }
    let point = |p: &Project, f| p.symbol(&p.function(f).unwrap().symbol).unwrap().connection_points[0].name.clone();
    let (pa, pb) = (point(&p, a), point(&p, b));
    wire(&mut p, &mut h, (a, &pa), (b, &pb));
    assert!(ecad_rules::check(&p).iter().any(|c| c.rule == "potential-clash"));
    for (id, n) in numbers {
        assert_eq!(p.connections[&id].wire_number, n);
    }
}

/// Interrupted and conflicting requests: two allocations computed from the same state can't
/// both be applied; a transaction that fails part way leaves nothing behind.
#[test]
fn conflicting_allocations_and_interrupted_transactions_change_nothing() {
    let original = ecad_model::conveyor::conveyor().unwrap();
    let mut p = original.clone();
    let page5 = page(&p, "5");
    let symbol = SymbolId::new("iec.coil");
    // Two requests see the same next tag; the second is refused.
    let tag = p.new_tag(&symbol, page5);
    p.apply(Command::AddDevice(Device::new(DeviceId::new(), &tag, ""))).unwrap();
    let before = p.clone();
    assert!(p.apply(Command::AddDevice(Device::new(DeviceId::new(), &tag, ""))).is_err());
    assert_eq!(p, before);
    // An address allocated, then reserved by someone else before it is used: refused.
    let address = p.next_address(IoKind::DigitalIn);
    p.apply(Command::Reserve(vec![Record::new(Identifier::Address { address: address.clone() }, "taken meanwhile")]))
        .unwrap();
    let a1 = p.device_by_tag("-A1").unwrap().id;
    let mut props = BTreeMap::new();
    props.insert(ecad_model::ADDRESS.to_owned(), address.clone());
    let f = drawing(
        FunctionId::new(),
        a1,
        SymbolId::new("iec.plc_di"),
        page5,
        (380.0, 56.0),
        p.next_designations(a1, &SymbolId::new("iec.plc_di")),
        props,
    );
    let before = p.clone();
    assert!(matches!(p.apply(Command::Place(f)), Err(ecad_model::ModelError::IdentifierReserved(_))));
    assert_eq!(p, before, "nothing placed");
    // A removal batch interrupted by a failing step: everything rolled back, nothing retired.
    let s3 = p.device_by_tag("-S3").unwrap().id;
    let mut cmds: Vec<Command> = p.functions_of(s3).map(|f| Command::Unplace(f.id)).collect();
    cmds.push(Command::RemoveDevice(s3));
    let mut cmds = p.removal(cmds).unwrap();
    cmds.push(Command::RenameDevice { id: DeviceId::new(), tag: "-Z1".into() });
    let before = p.clone();
    assert!(p.apply(Command::Batch(cmds)).is_err());
    assert_eq!(p, before, "rolled back, -S3 not retired");
    // Duplicate assignments are refused whatever the numbering says.
    let k1 = p.device_by_tag("-K1").unwrap().id;
    assert!(p.apply(Command::RenameDevice { id: k1, tag: "-K2".into() }).is_err());
}
