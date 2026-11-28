//! S0 stable numbering on the conveyor: sequential allocation (D22), reserved and retired
//! identifiers (D23), resource-aware allocation with exhaustion, inserted page names (D25),
//! persistence and exact history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_editor::Editor;
use ecad_model::conveyor::conveyor;
use ecad_model::numbering::Kind;
use ecad_model::{Command, Identifier, IoKind, ModelError, PageSuffix, Policies, Project, Record, Strategy};

fn save_and_load(p: &Project) -> Project {
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(p, tmp.path()).unwrap();
    ecad_project::load(tmp.path()).unwrap()
}

fn page(p: &Project, name: &str) -> ecad_model::PageId {
    p.pages.values().find(|pg| pg.name == name).unwrap().id
}

fn tag(t: &str) -> Identifier {
    Identifier::Tag { tag: t.into() }
}

fn address(a: &str) -> Identifier {
    Identifier::Address { address: a.into() }
}

/// Remove a device with everything that draws it, as the editor does: one transaction that
/// retires what it frees.
fn remove_device(p: &mut Project, t: &str) -> Command {
    let d = p.device_by_tag(t).unwrap().id;
    let mut cmds: Vec<Command> = p.functions_of(d).map(|f| Command::Unplace(f.id)).collect();
    if let Some(item) = p.panel_item_of(d) {
        cmds.push(Command::RemovePanelItem(item.id));
    }
    cmds.push(Command::RemoveDevice(d));
    let cmds = p.removal(cmds).unwrap();
    p.apply(Command::Batch(cmds)).unwrap()
}

fn sensor_symbol(p: &Project) -> ecad_model::SymbolId {
    let b1 = p.device_by_tag("-B1").unwrap().id;
    p.functions_of(b1).next().unwrap().symbol.clone()
}

#[test]
fn removing_b2_retires_it_and_the_next_sensor_is_b3() {
    let mut p = conveyor().unwrap();
    let symbol = sensor_symbol(&p);
    let page5 = page(&p, "5");
    assert_eq!(p.new_tag(&symbol, page5), "-B3");
    let before = p.clone();
    let undo = remove_device(&mut p, "-B2");
    assert!(p.numbering.retired.contains_key(&tag("-B2")), "{:?}", p.numbering.retired);
    // Sequential (D22): the gap stays and -B2 is not handed out again.
    assert_eq!(p.new_tag(&symbol, page5), "-B3");
    // Even the lowest-free policy never reuses a retired tag.
    let mut lowest = p.clone();
    let strategies = [(Kind::Tag, Strategy::LowestFree)].into();
    lowest.apply(Command::SetNumberingPolicies(Policies { strategies, ..Default::default() })).unwrap();
    assert_eq!(lowest.new_tag(&symbol, page5), "-B3");
    // Using -B2 again is explicit: refused until reclaimed.
    let again = ecad_model::Device::new(ecad_model::DeviceId::new(), "-B2", "");
    assert!(matches!(p.apply(Command::AddDevice(again.clone())), Err(ModelError::IdentifierRetired(_))));
    let mut reclaimed = p.clone();
    reclaimed.apply(Command::Reclaim(vec![tag("-B2")])).unwrap();
    reclaimed.apply(Command::AddDevice(again)).unwrap();
    // Nothing else was renumbered: every other device keeps its id and tag.
    for d in before.devices.values().filter(|d| d.tag != "-B2") {
        assert_eq!(p.devices.get(&d.id).map(|x| x.tag.as_str()), Some(d.tag.as_str()));
    }
    // Saved and loaded, then undone exactly.
    let loaded = save_and_load(&p);
    assert_eq!(loaded.numbering, p.numbering);
    let redo = p.apply(undo).unwrap();
    assert_eq!(p, before);
    p.apply(redo).unwrap();
    assert!(p.numbering.retired.contains_key(&tag("-B2")));
}

#[test]
fn a_new_di_skips_the_reserved_four_and_reserved_addresses_need_unreserving() {
    let mut p = conveyor().unwrap();
    let next = p.next_address(IoKind::DigitalIn);
    // The four inputs after the highest in use are kept for the planned extension.
    let ordinal = |a: &str| {
        let (byte, bit) = a[1..].split_once('.').unwrap();
        byte.parse::<u32>().unwrap() * 8 + bit.parse::<u32>().unwrap()
    };
    let at = |n: u32| format!("I{}.{}", n / 8, n % 8);
    let first = ordinal(&next);
    let reserved: Vec<Record> = (first..first + 4).map(|n| Record::new(address(&at(n)), "extension")).collect();
    let undo = p.apply(Command::Reserve(reserved.clone())).unwrap();
    assert_eq!(p.next_address(IoKind::DigitalIn), at(first + 4), "skips the reserved four");
    // Through the shared editor path, as the app, the CLI and MCP place a channel.
    let mut e = Editor::new(p.clone(), page(&p, "5")).unwrap();
    let f = e.script_place("iec.plc_di", page(&p, "5"), 380.0, 56.0, Some("-A1")).unwrap();
    let t = *e.project().function(f).unwrap().terminals.values().next().unwrap();
    let channel = e.project().terminals[&t].channel.clone().unwrap();
    assert_eq!(channel.address, at(first + 4));
    // An explicit assignment of a reserved address is refused, and changes nothing.
    let before = e.project().clone();
    let err =
        e.script_set_plc_channel("-A1", &e.project().terminals[&t].designation.clone(), Some(&at(first)), None, None);
    assert!(err.is_err(), "{err:?}");
    assert_eq!(e.project(), &before);
    // Unreserving is the explicit choice; then it can be used.
    p.apply(undo).unwrap();
    assert!(p.numbering.reserved.is_empty());
    // Invalid reservations: in use, twice, non-canonical, both reserved and retired.
    let used = p.terminals.values().find_map(|t| t.channel.as_ref().map(|c| c.address.clone())).unwrap();
    let canon = ecad_model::plc_address::canonical(&used).unwrap();
    assert!(matches!(
        p.apply(Command::Reserve(vec![Record::new(address(&canon), "")])),
        Err(ModelError::IdentifierInUse(_))
    ));
    let twice = vec![Record::new(address(&at(first)), ""), Record::new(address(&at(first)), "")];
    assert!(matches!(p.apply(Command::Reserve(twice)), Err(ModelError::AlreadyReserved(_))));
    assert!(matches!(
        p.apply(Command::Reserve(vec![Record::new(address("%i 0.9"), "")])),
        Err(ModelError::BadIdentifier(_))
    ));
    p.apply(Command::Retire(vec![Record::new(address(&at(first)), "broken input")])).unwrap();
    assert!(matches!(
        p.apply(Command::Reserve(vec![Record::new(address(&at(first)), "")])),
        Err(ModelError::IdentifierRetired(_))
    ));
    assert_eq!(p.next_address(IoKind::DigitalIn), at(first + 1), "retired addresses are skipped too");
    let original = conveyor().unwrap();
    let before = p.clone();
    assert!(
        p.apply(Command::Batch(vec![Command::Reserve(reserved), Command::Reserve(vec![Record::new(tag(""), "")])]))
            .is_err()
    );
    assert_eq!(p, before, "a failed transaction leaves nothing reserved");
    assert_ne!(p, original);
}

#[test]
fn allocating_past_the_last_free_channel_reports_exhaustion_and_adds_nothing() {
    let p = conveyor().unwrap();
    let card = p.device_by_tag("-A1").unwrap().id;
    let used = p.terminals.values().filter(|t| t.device == card && t.channel.is_some()).count() as u32;
    // A card of one more channel than it uses: one free slot, then none.
    let mut q = p.clone();
    q.apply(Command::SetCapacity { device: card, capacity: Some(used + 1) }).unwrap();
    assert!(q.allocate_channel(card).is_ok());
    assert!(matches!(
        q.apply(Command::SetCapacity { device: card, capacity: Some(used - 1) }),
        Err(ModelError::BadCapacity(_))
    ));
    let mut e2 = Editor::new(q.clone(), page(&q, "5")).unwrap();
    e2.script_place("iec.plc_di", page(&q, "5"), 380.0, 56.0, Some("-A1")).unwrap();
    let full = e2.project().clone();
    let err = e2.script_place("iec.plc_di", page(&q, "5"), 384.0, 56.0, Some("-A1")).unwrap_err();
    assert!(err.contains("no free channel"), "{err}");
    assert_eq!(e2.project(), &full, "nothing added");
    // A reserved spare counts as taken.
    let mut r = p.clone();
    r.apply(Command::SetCapacity { device: card, capacity: Some(used + 1) }).unwrap();
    let spare = r.allocate_channel(card).unwrap();
    r.apply(Command::Reserve(vec![Record::new(Identifier::Terminal { device: card, designation: spare }, "spare")]))
        .unwrap();
    assert!(matches!(r.allocate_channel(card), Err(ModelError::Exhausted(_))));
    // Removing the card refuses while its numbering names it; the removal operation takes the
    // capacity and reservations with it.
    let before = r.clone();
    let undo = remove_device(&mut r, "-A1");
    assert!(!r.numbering.names_device(card));
    let removed = r.clone();
    let redo = r.apply(undo).unwrap();
    assert_eq!(r, before);
    r.apply(redo).unwrap();
    assert_eq!(r, removed);
    let persisted = save_and_load(&r);
    assert_eq!(persisted.numbering, r.numbering);
}

#[test]
fn cable_cores_are_allocated_from_the_cables_size() {
    let mut p = conveyor().unwrap();
    let w1 = p.cables.values().find(|c| c.tag == "-W1").unwrap().clone();
    let w2 = p.cables.values().find(|c| c.tag == "-W2").unwrap().id;
    assert!(matches!(p.allocate_core(w2), Err(ModelError::Exhausted(_))), "BN, BU and BK hold all three cores");
    let in_w1 =
        p.connections.values().filter(|c| c.spec.cable.as_ref().is_some_and(|k| k.cable == w1.id)).count() as u32;
    if in_w1 < w1.cores {
        let core = p.allocate_core(w1.id).unwrap();
        let id = Identifier::Core { cable: w1.id, core: core.clone() };
        p.apply(Command::Reserve(vec![Record::new(id, "spare")])).unwrap();
        assert_ne!(p.allocate_core(w1.id).ok(), Some(core));
    }
    // A reservation outside the cable is invalid.
    let outside = Identifier::Core { cable: w1.id, core: (w1.cores + 1).to_string() };
    assert!(matches!(p.apply(Command::Reserve(vec![Record::new(outside, "")])), Err(ModelError::BadIdentifier(_))));
}

#[test]
fn a_page_inserted_after_3_is_3a_and_no_other_page_changes() {
    let mut p = conveyor().unwrap();
    let three = page(&p, "3");
    assert_eq!(p.new_page_name(Some(three)), "3a");
    let before = p.pages.clone();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let files_before = files(tmp.path());
    let new = ecad_model::Page::new("", 0);
    let id = new.id;
    let undo = p.apply(Command::Batch(p.insert_page(Some(three), new))).unwrap();
    assert_eq!(p.pages[&id].name, "3a");
    for (pid, pg) in &before {
        assert_eq!(&p.pages[pid], pg, "page {} untouched", pg.name);
    }
    let order: Vec<&str> = p.pages_in_order().iter().map(|pg| pg.name.as_str()).collect();
    assert_eq!(order, ["1", "2", "3", "3a", "4", "5", "L1"]);
    ecad_project::save(&p, tmp.path()).unwrap();
    let files_after = files(tmp.path());
    let changed: Vec<&String> = files_after
        .iter()
        .filter(|(name, bytes)| files_before.get(*name) != Some(bytes))
        .map(|(name, _)| name)
        .collect();
    // project.json lists the pages; the new page has a file of its own; nothing else changes.
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(changed.iter().any(|n| n.as_str() == "project.json"));
    assert!(changed.iter().any(|n| n.contains(&id.0.to_string())));
    // After 3a comes 3b; inserting after 3 again skips the taken suffix; the dotted policy.
    assert_eq!(p.new_page_name(Some(id)), "3b");
    assert_eq!(p.new_page_name(Some(three)), "3b");
    let mut dotted = p.clone();
    dotted
        .apply(Command::SetNumberingPolicies(Policies { page_suffix: PageSuffix::Dotted, ..Default::default() }))
        .unwrap();
    assert_eq!(dotted.new_page_name(Some(three)), "3.1");
    // Appended at the end: the next page number (L1 is the last page; numbers continue).
    assert_eq!(p.new_page_name(None), "6");
    p.apply(undo).unwrap();
    assert_eq!(p.pages, before);
}

fn files(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

#[test]
fn numbering_json_round_trips_and_conflicting_records_are_refused() {
    let mut p = conveyor().unwrap();
    let card = p.device_by_tag("-A1").unwrap().id;
    let used = p.terminals.values().filter(|t| t.device == card && t.channel.is_some()).count() as u32;
    p.apply(Command::SetCapacity { device: card, capacity: Some(used + 4) }).unwrap();
    p.apply(Command::Reserve(vec![Record::new(tag("-K10"), "future section"), Record::new(address("I3.0"), "")]))
        .unwrap();
    remove_device(&mut p, "-B2");
    let strategies = [(Kind::Wire, Strategy::LowestFree)].into();
    p.apply(Command::SetNumberingPolicies(Policies { strategies, page_suffix: PageSuffix::Dotted })).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let loaded = ecad_project::load(tmp.path()).unwrap();
    assert_eq!(loaded, p);
    assert_eq!(loaded.content_hash().unwrap(), p.content_hash().unwrap());
    let path = tmp.path().join("numbering.json");
    let good = std::fs::read_to_string(&path).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&good).unwrap();
    let refuse = |v: &serde_json::Value, why: &str| {
        std::fs::write(&path, serde_json::to_string_pretty(v).unwrap()).unwrap();
        assert!(ecad_project::load(tmp.path()).is_err(), "{why}");
    };
    // A retired tag that is in use.
    let mut v = json.clone();
    v["retired"].as_array_mut().unwrap().push(serde_json::json!({"kind": "tag", "tag": "-K1"}));
    refuse(&v, "retired but in use");
    // Reserved and retired at once.
    let mut v = json.clone();
    v["reserved"].as_array_mut().unwrap().push(serde_json::json!({"kind": "tag", "tag": "-B2"}));
    refuse(&v, "reserved and retired");
    // Listed twice.
    let mut v = json.clone();
    let first = v["reserved"][0].clone();
    v["reserved"].as_array_mut().unwrap().push(first);
    refuse(&v, "duplicate");
    // A capacity below what the card uses.
    let mut v = json.clone();
    v["capacities"][card.0.to_string()] = serde_json::json!(1);
    refuse(&v, "capacity too small");
    // A reservation on a device that doesn't exist.
    let mut v = json.clone();
    v["reserved"].as_array_mut().unwrap().push(serde_json::json!({
        "kind": "terminal", "device": "00000000-0000-0000-0000-00000000dead", "designation": "1"
    }));
    refuse(&v, "unknown device");
    // An unknown kind.
    json["retired"].as_array_mut().unwrap().push(serde_json::json!({"kind": "sticker", "tag": "x"}));
    refuse(&json, "unknown kind");
    // Without numbering.json (a format 2 project saved before S0): the defaults.
    std::fs::remove_file(&path).unwrap();
    let plain = ecad_project::load(tmp.path()).unwrap();
    assert!(plain.numbering.is_empty());
    std::fs::write(&path, good).unwrap();
    assert_eq!(ecad_project::load(tmp.path()).unwrap(), p);
}

#[test]
fn numbering_commands_undo_and_redo_exactly() {
    let mut p = conveyor().unwrap();
    let original = p.clone();
    let mut h = ecad_model::History::default();
    h.execute(&mut p, "reserve", Command::Reserve(vec![Record::new(tag("-K10"), "a"), Record::new(tag("-K11"), "b")]))
        .unwrap();
    h.execute(&mut p, "unreserve", Command::Unreserve(vec![tag("-K11")])).unwrap();
    h.execute(&mut p, "retire", Command::Retire(vec![Record::new(tag("-K12"), "gone")])).unwrap();
    h.execute(&mut p, "reclaim", Command::Reclaim(vec![tag("-K12")])).unwrap();
    let strategies = [(Kind::Tag, Strategy::LowestFree)].into();
    h.execute(&mut p, "policy", Command::SetNumberingPolicies(Policies { strategies, ..Default::default() })).unwrap();
    let card = p.device_by_tag("-A1").unwrap().id;
    h.execute(&mut p, "cap", Command::SetCapacity { device: card, capacity: Some(64) }).unwrap();
    let end = p.clone();
    while h.undo(&mut p).unwrap().is_some() {}
    assert_eq!(p, original);
    while h.redo(&mut p).unwrap().is_some() {}
    assert_eq!(p, end);
    assert_eq!(p.numbering.reserved.get(&tag("-K10")).map(String::as_str), Some("a"));
    // Reclaiming what isn't retired and unreserving what isn't reserved are refused.
    assert!(matches!(p.apply(Command::Reclaim(vec![tag("-K99")])), Err(ModelError::NotRetired(_))));
    assert!(matches!(p.apply(Command::Unreserve(vec![tag("-K99")])), Err(ModelError::NotReserved(_))));
    // A reserved tag can't be taken by a new device or a rename.
    let k1 = p.device_by_tag("-K1").unwrap().id;
    assert!(matches!(
        p.apply(Command::RenameDevice { id: k1, tag: "-K10".into() }),
        Err(ModelError::IdentifierReserved(_))
    ));
}

#[test]
fn renumbering_is_previewed_confirmed_and_keeps_every_identity() {
    use ecad_model::renumber::Scope;
    let mut p = conveyor().unwrap();
    // Make gaps: remove -B1 (retired), so devices -B2… and wires have holes.
    remove_device(&mut p, "-B1");
    p.apply(Command::Reserve(vec![Record::new(tag("-B1"), "kept for the infeed sensor")]))
        .map(|_| ())
        .unwrap_or_else(|e| assert!(matches!(e, ModelError::IdentifierRetired(_)), "{e:?}"));
    for scope in [Scope::Wires, Scope::Devices, Scope::Pages] {
        let before = p.clone();
        let r = p.preview_renumbering(scope).unwrap();
        assert_eq!(p, before, "a preview changes nothing");
        // Applied only to the project it was previewed on.
        let mut moved = p.clone();
        let k1 = moved.device_by_tag("-K1").unwrap().id;
        moved.apply(Command::SetDescription { id: k1, description: "changed".into() }).unwrap();
        assert!(matches!(moved.apply_renumbering(&r), Err(ModelError::StalePreview)));
        let undo = p.apply_renumbering(&r).unwrap();
        // Ids never change: only names.
        assert_eq!(p.devices.keys().collect::<Vec<_>>(), before.devices.keys().collect::<Vec<_>>());
        assert_eq!(p.terminals.keys().collect::<Vec<_>>(), before.terminals.keys().collect::<Vec<_>>());
        assert_eq!(p.connections.keys().collect::<Vec<_>>(), before.connections.keys().collect::<Vec<_>>());
        assert_eq!(p.netlist().nets.len(), before.netlist().nets.len());
        // No reserved or retired name is handed out.
        for d in p.devices.values() {
            assert!(p.available(&tag(&d.tag)), "{}", d.tag);
        }
        for c in r.changes.iter() {
            assert_ne!(c.from, c.to);
        }
        p.apply(undo).unwrap();
        assert_eq!(p, before, "{scope:?} undoes exactly");
    }
    // Devices: -B2 is now the first -B in drawing order, but -B1 is retired: it stays -B2.
    let r = p.preview_renumbering(Scope::Devices).unwrap();
    assert!(!r.changes.iter().any(|c| c.to == "-B1"), "{:?}", r.changes);
    // Terminals of a strip: undrawn groups keep their numbers.
    let x3 = p.device_by_tag("-X3").unwrap().id;
    let r = p.preview_renumbering(Scope::Terminals(x3)).unwrap();
    let mut q = p.clone();
    q.apply_renumbering(&r).unwrap();
    assert_eq!(q.terminals.keys().collect::<Vec<_>>(), p.terminals.keys().collect::<Vec<_>>());
}

#[test]
fn replacing_a_device_keeps_its_identity_and_reconciles_terminals_explicitly() {
    let p = conveyor().unwrap();
    let q1 = p.device_by_tag("-Q1").unwrap().clone();
    let before = p.clone();
    let terminals: Vec<&ecad_model::TerminalEntry> = p.terminals.values().filter(|t| t.device == q1.id).collect();
    // The new product's auxiliary contact is 53-54 instead of 13-14: a person maps them.
    let renames: std::collections::BTreeMap<_, _> = terminals
        .iter()
        .filter_map(|t| match t.designation.as_str() {
            "13" => Some((t.id, "53".to_owned())),
            "14" => Some((t.id, "54".to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(renames.len(), 2);
    let part = p.parts.values().next().unwrap().clone();
    let parts = vec![ecad_model::PartRef::to(part.id, 1)];
    let mut e = Editor::new(p.clone(), page(&p, "1")).unwrap();
    e.replace_device(q1.id, parts.clone(), renames.clone()).unwrap();
    let after = e.project();
    let d = &after.devices[&q1.id];
    assert_eq!((d.tag.as_str(), &d.parts), (q1.tag.as_str(), &parts), "same id and tag, new product");
    assert_eq!(after.terminals.keys().collect::<Vec<_>>(), before.terminals.keys().collect::<Vec<_>>());
    assert_eq!(after.functions, before.functions, "drawings untouched");
    assert_eq!(after.connections.keys().collect::<Vec<_>>(), before.connections.keys().collect::<Vec<_>>());
    assert_eq!(after.panel_items, before.panel_items, "layout position kept");
    assert!(after.numbering.retired.is_empty(), "replacement retires nothing");
    for (t, d) in &renames {
        assert_eq!(&after.terminals[t].designation, d);
    }
    // Unmapped terminals keep their designations; nothing is dropped.
    for t in before.terminals.values().filter(|t| t.device == q1.id && !renames.contains_key(&t.id)) {
        assert_eq!(after.terminals[&t.id].designation, t.designation);
    }
    let saved = save_and_load(after);
    assert_eq!(&saved, after);
    e.key(ecad_editor::Key::Undo);
    assert_eq!(e.project(), &before);
    // Onto a reserved designation, or an empty one, is refused, changing nothing.
    let mut reserved = before.clone();
    reserved
        .apply(Command::Reserve(vec![Record::new(
            Identifier::Terminal { device: q1.id, designation: "53".into() },
            "",
        )]))
        .unwrap();
    let held = reserved.clone();
    let err = reserved.apply(Command::ReplaceDevice { id: q1.id, parts: vec![], designations: renames.clone() });
    assert!(matches!(err, Err(ModelError::IdentifierReserved(_))), "{err:?}");
    assert_eq!(reserved, held);
    let first = *renames.keys().next().unwrap();
    let mut q = before.clone();
    let err =
        q.apply(Command::ReplaceDevice { id: q1.id, parts: vec![], designations: [(first, " ".to_owned())].into() });
    assert!(err.is_err());
    assert_eq!(q, before);
}

#[test]
fn page_renumbering_keeps_a_family_together_and_skips_blocked_staging_names() {
    use ecad_model::renumber::Scope;
    let mut p = Project::new("page family");
    let root = ecad_model::Page::new("8", 10);
    let child = ecad_model::Page::new("8a", 20);
    for pg in [&root, &child] {
        p.apply(Command::AddPage(pg.clone())).unwrap();
    }
    for name in ["1a".to_owned(), format!("~renumbering~{}", root.id.0)] {
        p.apply(Command::Reserve(vec![Record::new(Identifier::Page { name }, "future")])).unwrap();
    }
    let before = p.clone();
    let preview = p.preview_renumbering(Scope::Pages).unwrap();
    let undo = p.apply_renumbering(&preview).unwrap();
    assert_eq!(p.pages[&root.id].name, "2");
    assert_eq!(p.pages[&child.id].name, "2a");
    p.apply(undo).unwrap();
    assert_eq!(p, before);
}

#[test]
fn numeric_limits_preserve_series_order_and_address_range_endpoints() {
    let mut p = Project::new("limits");
    let pg = ecad_model::Page::new("18446744073709551615", u64::MAX);
    p.apply(Command::AddPage(pg.clone())).unwrap();
    let new = ecad_model::Page::new("", 0);
    let id = new.id;
    p.apply(Command::Batch(p.insert_page(None, new))).unwrap();
    assert_eq!(p.pages[&id].name, "18446744073709551616");
    assert!(p.pages[&id].order > p.pages[&pg.id].order);
    assert!(p.parse_identifiers("address:IW0..IW3").is_err());
}

#[test]
fn card_slot_bounds_agree_on_reserve_resize_and_strip_renumber() {
    use ecad_model::renumber::Scope;
    let mut p = conveyor().unwrap();
    let card = p.device_by_tag("-A1").unwrap().id;
    let spare = Identifier::Terminal { device: card, designation: "20".into() };
    p.apply(Command::Reserve(vec![Record::new(spare.clone(), "last")])).unwrap();
    assert!(p.apply(Command::SetCapacity { device: card, capacity: Some(20) }).is_err());
    p.apply(Command::Unreserve(vec![spare.clone()])).unwrap();
    p.apply(Command::SetCapacity { device: card, capacity: Some(20) }).unwrap();
    assert!(p.apply(Command::Reserve(vec![Record::new(spare, "")])).is_err());
    assert!(p.preview_renumbering(Scope::Terminals(card)).is_err());
}
