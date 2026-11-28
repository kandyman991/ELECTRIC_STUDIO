//! M7 acceptance: the belt conveyor (`ecad_model::conveyor`), drawn schematic-first with today's
//! operations, must resolve every wire end to a terminal or a node, with the same ids on every
//! build and after a save and load; keep physical groups and points unique; share a designation
//! across a strip terminal's sides with an explicit bridge and keep its levels apart; draw a node
//! many times without making more endpoints; and give the same netlist, reports, content hash
//! and pages every time.
//!
//! The hostile cases at the end damage the conveyor the ways format 1 allows and check that each
//! is reported, never merged or silently dropped.
//!
//! To accept intended changes to the expected results:
//! `ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-acceptance --test conveyor`, then review the diff.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::connectivity::{Ambiguity, NodeKind, TerminalKind};
use ecad_model::{
    Command, ConnectionRef, Endpoint, Function, FunctionId, ModelError, PageId, Point, Project, Rotation, SymbolId,
    conveyor::conveyor,
};
use ecad_rules::Severity;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The reports whose output is pinned.
const REPORTS: [&str; 6] = ["netlist", "bom", "wire-list", "terminal-diagram", "plc-io-list", "cable-list"];
const PNG_SIZE: u32 = 600;
const MAX_DIFFERING: f64 = 0.005;
const CHANNEL_TOLERANCE: u8 = 48;

fn dir(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conveyor").join(sub)
}

fn updating() -> bool {
    std::env::var_os("ECAD_UPDATE_GOLDEN").is_some()
}

/// Compare text with an expected file, ignoring line-ending differences.
fn expect_text(path: &Path, got: &str) {
    if updating() {
        std::fs::write(path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with ECAD_UPDATE_GOLDEN=1)", path.display()))
        .replace("\r\n", "\n");
    let got = got.replace("\r\n", "\n");
    if got != want {
        let line = got.lines().zip(want.lines()).position(|(a, b)| a != b).map_or(0, |i| i + 1);
        panic!("{} differs from the expected file, first at line {line}", path.display());
    }
}

fn save_and_load(p: &Project) -> Project {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("conveyor");
    ecad_project::save(p, &dir).unwrap();
    ecad_project::load(&dir).unwrap()
}

fn tag_of(p: &Project, f: FunctionId) -> String {
    let d = p.function(f).unwrap().device;
    p.device(d).unwrap().tag.clone()
}

/// The one drawing of `tag` with `symbol`, numbered `number` if given.
fn drawing(p: &Project, tag: &str, symbol: &str, number: Option<&str>) -> FunctionId {
    let device = p.device_by_tag(tag).unwrap_or_else(|| panic!("no {tag}")).id;
    let found: Vec<FunctionId> = p
        .functions_of(device)
        .filter(|f| f.symbol.0.as_str() == symbol)
        .filter(|f| number.is_none_or(|n| p.drawing_number(f.id).as_deref() == Some(n)))
        .map(|f| f.id)
        .collect();
    assert_eq!(found.len(), 1, "{tag} {symbol} {number:?}: {found:?}");
    found[0]
}

fn page_named(p: &Project, name: &str) -> PageId {
    p.pages_in_order().iter().find(|pg| pg.name == name).unwrap().id
}

/// Every wire end's endpoint, keyed by wire and end.
fn wire_ends(p: &Project) -> BTreeMap<(String, bool), Endpoint> {
    let c = p.connectivity();
    let mut out = BTreeMap::new();
    for w in p.connections.values() {
        for (is_to, end) in [(false, &w.from), (true, &w.to)] {
            let e = c.endpoint(end).unwrap_or_else(|| {
                panic!("wire {:?} end {}:{} resolves to nothing", w.wire_number, tag_of(p, end.function), end.point)
            });
            out.insert((w.id.0.to_string(), is_to), e);
        }
    }
    out
}

fn net_index(p: &Project, r: &ConnectionRef) -> usize {
    let e = p.connectivity().endpoint(r).unwrap();
    p.netlist().nets.iter().position(|n| n.endpoints.contains(&e)).unwrap_or_else(|| panic!("{r:?} on no net"))
}

#[test]
fn checks_find_nothing() {
    let p = conveyor().unwrap();
    let checks = ecad_rules::check(&p);
    let errors = checks.iter().filter(|c| c.severity == Severity::Error).count();
    assert_eq!(errors, 0, "{checks:#?}");
    // Warnings are allowed by the acceptance criteria; the conveyor is drawn to have none, so a
    // new one is a regression in the drawing or in a rule.
    assert!(checks.is_empty(), "{checks:#?}");
}

#[test]
fn every_wire_end_resolves_with_the_same_ids_on_every_build_and_load() {
    let p = conveyor().unwrap();
    let ends = wire_ends(&p);
    assert_eq!(ends.len(), 2 * p.connections.len());
    assert!(p.netlist().is_complete());

    let again = conveyor().unwrap();
    assert_eq!(again.content_hash().unwrap(), p.content_hash().unwrap(), "the build is deterministic");
    assert_eq!(wire_ends(&again), ends);

    let loaded = save_and_load(&p);
    assert_eq!(wire_ends(&loaded), ends);
    assert_eq!(loaded.connectivity(), p.connectivity());
}

#[test]
fn netlist_and_content_hash_survive_a_save_and_load() {
    let p = conveyor().unwrap();
    let loaded = save_and_load(&p);
    assert_eq!(loaded.netlist(), p.netlist());
    assert_eq!(loaded.content_hash().unwrap(), p.content_hash().unwrap());
    // And a second round trip changes nothing either.
    assert_eq!(save_and_load(&loaded).content_hash().unwrap(), p.content_hash().unwrap());
}

#[test]
fn groups_and_points_are_unique_and_nothing_is_ambiguous() {
    let p = conveyor().unwrap();
    let c = p.connectivity();
    assert!(c.ambiguities.is_empty(), "{:#?}", c.ambiguities);
    // Within a group each point appears once, and a group never spans devices.
    let mut seen = BTreeSet::new();
    let mut group_device = BTreeMap::new();
    for t in c.terminals.values() {
        assert!(seen.insert((t.group, t.point.clone())), "{t:?} repeats a point of its group");
        assert_eq!(*group_device.entry(t.group).or_insert(t.device), t.device, "{t:?}");
    }
    // A designation is printed by one group per device.
    let mut by_designation: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for t in c.terminals.values() {
        by_designation.entry((t.device, t.designation.clone())).or_default().insert(t.group);
    }
    for ((device, designation), groups) in by_designation {
        assert_eq!(groups.len(), 1, "{} {designation}", p.device(device).unwrap().tag);
    }
}

#[test]
fn a_strip_terminal_shares_its_designation_across_both_sides_with_an_explicit_bridge() {
    let p = conveyor().unwrap();
    let c = p.connectivity();
    let f = drawing(&p, "-X2", "iec.terminal", Some("1"));
    let ids: Vec<_> = ["1", "2"]
        .iter()
        .map(|pt| match c.endpoint(&ConnectionRef::new(f, pt)) {
            Some(Endpoint::Terminal(t)) => c.terminals[&t].clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_ne!(ids[0].id, ids[1].id);
    assert_eq!(ids[0].group, ids[1].group);
    assert_eq!((ids[0].designation.as_str(), ids[1].designation.as_str()), ("1", "1"));
    assert!(matches!(ids[0].kind, TerminalKind::Strip));
    assert!(c.bridges.iter().any(|b| (b.a, b.b) == (ids[0].id, ids[1].id) || (b.a, b.b) == (ids[1].id, ids[0].id)));
    assert_eq!(net_index(&p, &ConnectionRef::new(f, "1")), net_index(&p, &ConnectionRef::new(f, "2")));
}

#[test]
fn the_levels_of_a_two_level_terminal_stay_isolated() {
    let p = conveyor().unwrap();
    let netlist = p.netlist();
    for number in ["1", "3"] {
        let f = drawing(&p, "-X3", ecad_model::conveyor::TWO_LEVEL, Some(number));
        let net = |pt: &str| net_index(&p, &ConnectionRef::new(f, pt));
        assert_eq!(net("1"), net("2"), "-X3:{number} level 1 is bridged");
        assert_eq!(net("3"), net("4"), "-X3:{number} level 2 is bridged");
        assert_ne!(net("1"), net("3"), "-X3:{number} levels are isolated");
        assert_eq!(netlist.nets[net("1")].names, ["L+"]);
        assert_eq!(netlist.nets[net("3")].names, ["M"]);
        let c = p.connectivity();
        let group: BTreeSet<_> = ["1", "2", "3", "4"]
            .iter()
            .map(|pt| match c.endpoint(&ConnectionRef::new(f, pt)) {
                Some(Endpoint::Terminal(t)) => c.terminals[&t].group,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(group.len(), 1, "one physical terminal");
    }
}

#[test]
fn repeated_representations_do_not_create_extra_endpoints() {
    let p = conveyor().unwrap();
    let c = p.connectivity();
    let node = |name: &str| {
        let found: Vec<_> = c.nodes.values().filter(|n| n.name == name).collect();
        assert_eq!(found.len(), 1, "{name} is one node");
        found[0].clone()
    };
    let pages_of = |drawings: &[ConnectionRef]| -> BTreeSet<String> {
        drawings.iter().map(|r| p.page(p.function(r.function).unwrap().page).unwrap().name.clone()).collect()
    };
    for (name, kind, pages) in [
        ("L+", NodeKind::Potential, vec!["2", "5"]),
        ("M", NodeKind::Potential, vec!["2", "4", "5"]),
        ("Q-FWD", NodeKind::Interruption, vec!["3", "4"]),
        ("Q-REV", NodeKind::Interruption, vec!["3", "4"]),
        ("SAFE-OK", NodeKind::Interruption, vec!["3", "5"]),
        ("PS-L", NodeKind::Interruption, vec!["1", "2"]),
    ] {
        let n = node(name);
        assert_eq!(n.kind, kind, "{name}");
        let drawn = pages_of(&n.drawings);
        for page in pages {
            assert!(drawn.contains(page), "{name} drawn on page {page}: {drawn:?}");
        }
    }
    assert!(node("M").drawings.len() >= 5, "M is drawn many times and is still one node");
    // Every drawn point of a numbered or ordinary drawing is exactly one terminal; every node
    // drawing maps to one node. So endpoints are counted once however often they're drawn.
    let drawn_points: usize = c.terminals.values().map(|t| t.drawings.len()).sum();
    assert_eq!(drawn_points, c.terminals.len(), "format 1: one drawing per terminal");
    // A device drawn on several pages (the contactors, the PLC card) keeps one id.
    for tag in ["-K1", "-K2", "-A1", "-Q1"] {
        let device = p.device_by_tag(tag).unwrap().id;
        let drawings = p.functions_of(device).count();
        assert!(drawings > 1, "{tag} is drawn {drawings} times");
        let pages: BTreeSet<_> = p.functions_of(device).map(|f| f.page).collect();
        assert!(pages.len() > 1, "{tag} spans pages");
        assert!(c.terminals.values().filter(|t| t.device == device).count() >= drawings);
    }
}

#[test]
fn plc_channels_are_typed() {
    let p = conveyor().unwrap();
    let c = p.connectivity();
    let mut addresses: Vec<String> = c
        .terminals
        .values()
        .filter_map(|t| match &t.kind {
            TerminalKind::PlcChannel(ch) => ch.address.clone(),
            _ => None,
        })
        .collect();
    addresses.sort();
    let want: Vec<String> = ["I0.0", "I0.1", "I0.2", "I0.3", "I0.4", "I0.5", "I0.6", "I0.7", "I1.0"]
        .iter()
        .chain(["Q0.0", "Q0.1", "Q0.2", "Q0.3"].iter())
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(addresses, want);
}

#[test]
fn reports_hash_and_counts_match_the_expected_results() {
    let p = conveyor().unwrap();
    for name in REPORTS {
        let csv = ecad_reports::csv(&p, name).unwrap().unwrap();
        expect_text(&dir("expected").join(format!("{name}.csv")), &csv);
    }
    let c = p.connectivity();
    let netlist = p.netlist();
    let summary = serde_json::json!({
        "contentHash": p.content_hash().unwrap(),
        "pages": p.pages.len(),
        "devices": p.devices.len(),
        "drawings": p.functions.len(),
        "wires": p.connections.len(),
        "terminals": c.terminals.len(),
        "nodes": c.nodes.len(),
        "bridges": c.bridges.len(),
        "nets": netlist.nets.len(),
    });
    expect_text(
        &dir("expected").join("summary.json"),
        &format!("{}\n", serde_json::to_string_pretty(&summary).unwrap()),
    );
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

#[test]
fn pages_match_the_golden_drawings() {
    let p = conveyor().unwrap();
    for page in p.pages_in_order() {
        let stem = format!("page-{}", page.name);
        let dxf = ecad_view::page_dxf(&p, page.id).unwrap();
        let png = ecad_view::page_png(&p, page.id, PNG_SIZE, PNG_SIZE).unwrap();
        let (dxf_path, png_path) = (dir("pages").join(format!("{stem}.dxf")), dir("pages").join(format!("{stem}.png")));
        if updating() {
            std::fs::write(&dxf_path, &dxf).unwrap();
            std::fs::write(&png_path, &png).unwrap();
            continue;
        }
        expect_text(&dxf_path, &dxf);
        let (w1, h1, got) = decode(&png);
        let (w2, h2, exp) = decode(&std::fs::read(&png_path).unwrap());
        assert_eq!((w1, h1), (w2, h2), "{stem}.png size changed");
        let differing = got
            .chunks(4)
            .zip(exp.chunks(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE))
            .count();
        let share = differing as f64 / f64::from(w1 * h1);
        assert!(share <= MAX_DIFFERING, "{stem}.png: {:.2}% of pixels differ", share * 100.0);
    }
}

// Hostile cases: the conveyor damaged the ways format 1 allows.

fn extra_drawing(p: &Project, tag: &str, symbol: &str, page: &str, at: Point) -> Function {
    Function {
        node: None,
        terminals: Default::default(),
        id: FunctionId::new(),
        device: p.device_by_tag(tag).unwrap().id,
        symbol: SymbolId::new(symbol),
        page: page_named(p, page),
        position: at,
        rotation: Rotation::R0,
        mirrored: false,
        designations: Default::default(),
        properties: Default::default(),
    }
}

fn has_rule(p: &Project, rule: &str) -> bool {
    ecad_rules::check(p).iter().any(|c| c.rule == rule)
}

#[test]
fn hostile_a_coil_drawn_twice_is_kept_apart_and_reported() {
    let mut p = conveyor().unwrap();
    let before = p.netlist();
    let again = p.place(extra_drawing(&p, "-K1", "iec.coil", "4", Point::new(360.0, 40.0))).unwrap();
    let c = p.connectivity();
    let k1 = p.device_by_tag("-K1").unwrap().id;
    let shared: Vec<&str> = c
        .ambiguities
        .iter()
        .filter_map(|a| match a {
            Ambiguity::SharedDesignation { device, designation, groups, .. } if *device == k1 => {
                assert_eq!(groups.len(), 2);
                Some(designation.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(shared, ["A1", "A2"]);
    assert!(c.endpoint(&ConnectionRef::new(again, "A1")).is_some(), "its own terminal, not merged");
    assert_eq!(p.netlist().nets, before.nets, "no net changes");
    assert!(has_rule(&p, "shared-designation"));
}

#[test]
fn hostile_a_wire_end_lost_to_a_symbol_change_is_an_error_and_blocks_the_netlist() {
    let mut p = conveyor().unwrap();
    let mut lamp = p.symbol(&SymbolId::new("iec.lamp")).unwrap().clone();
    lamp.connection_points.retain(|cp| cp.name != "X2");
    p.add_symbol(lamp);
    let n = p.netlist();
    assert!(!n.is_complete());
    assert_eq!(n.unresolved.len(), 2, "both lamps' X2");
    let errors: Vec<_> = ecad_rules::check(&p).into_iter().filter(|c| c.rule == "wire-end-missing").collect();
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|c| c.severity == Severity::Error));
    assert!(matches!(ecad_reports::csv(&p, "netlist"), Err(ModelError::IncompleteNetlist(_))));
}

/// Since M10c a junction is a node drawn once: the commands refuse a second drawing, and one
/// put in behind their back is reported.
#[test]
fn hostile_a_junction_drawn_twice_is_refused_or_reported() {
    let mut p = conveyor().unwrap();
    let page5 = page_named(&p, "5");
    let j = p.functions.values().find(|f| f.page == page5 && f.symbol.0.as_str() == "iec.junction").unwrap().clone();
    let again = Function { id: FunctionId::new(), position: Point::new(380.0, 20.0), ..j.clone() };
    assert_eq!(p.apply(Command::Place(again.clone())).unwrap_err(), ModelError::JunctionDrawnTwice);
    p.functions.insert(again.id, again);
    let c = p.connectivity();
    assert!(
        c.ambiguities
            .iter()
            .any(|a| matches!(a, Ambiguity::JunctionDrawnTwice { drawings, .. } if drawings.len() == 2))
    );
    assert!(has_rule(&p, "junction-drawn-twice"));
}

#[test]
fn hostile_a_non_finite_coordinate_is_refused_and_changes_nothing() {
    let mut p = conveyor().unwrap();
    let hash = p.content_hash().unwrap();
    let bad = extra_drawing(&p, "-K1", "iec.coil", "4", Point::new(f64::NAN, 40.0));
    assert!(matches!(p.apply(Command::Place(bad)), Err(ModelError::NotFinite(_))));
    assert_eq!(p.content_hash().unwrap(), hash);
}

#[test]
fn an_edit_changes_the_hash_and_undoing_it_restores_it() {
    let mut p = conveyor().unwrap();
    let hash = p.content_hash().unwrap();
    let f = drawing(&p, "-H1", "iec.lamp", None);
    let undo = p.apply(Command::Move { id: f, to: Point::new(176.0, 160.0) }).unwrap();
    assert_ne!(p.content_hash().unwrap(), hash);
    p.apply(undo).unwrap();
    assert_eq!(p.content_hash().unwrap(), hash);
}

#[test]
fn multi_level_terminal_reports_include_every_wired_point() {
    let p = conveyor().unwrap();
    let rows = ecad_reports::terminal_diagram(&p);
    for (number, sensor) in [("1", "-B1"), ("3", "-B2")] {
        let levels: Vec<_> = rows.iter().filter(|r| r.strip == "-X3" && r.terminal == number).collect();
        assert_eq!(levels.len(), 2);
        for (row, potential, a, b, core) in [(levels[0], "L+", "1", "2", "BN"), (levels[1], "M", "3", "4", "BU")] {
            assert_eq!(row.potential, potential);
            assert_eq!((row.inside_point.as_str(), row.outside_point.as_str()), (a, b));
            assert_eq!(row.bridge_points, format!("{a}|{b}"));
            assert_eq!(row.outside, format!("{sensor}:{core}"));
            assert!(!row.inside.is_empty());
        }
    }
    let wires = ecad_reports::wire_list(&p).unwrap();
    for (number, sensor) in [("1", "-B1"), ("3", "-B2")] {
        let brown = wires.iter().find(|w| w.from_tag == "-X3" && w.to_tag == sensor && w.core == "BN").unwrap();
        let blue = wires.iter().find(|w| w.from_tag == sensor && w.to_tag == "-X3" && w.core == "BU").unwrap();
        assert_eq!((brown.from_point.as_str(), brown.from_terminal_point.as_str()), (number, "2"));
        assert_eq!((blue.to_point.as_str(), blue.to_terminal_point.as_str()), (number, "4"));
        assert_eq!((brown.potential.as_str(), blue.potential.as_str()), ("L+", "M"));
    }
}

// M10: the conveyor as M7 saved it (format 1, `conveyor/format1`, written by `ecad new-sample`
// at the M7 acceptance commit) converts on load to the same project.

fn format1() -> Project {
    ecad_project::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conveyor").join("format1")).unwrap()
}

#[test]
fn the_format_1_conveyor_converts_to_the_same_project() {
    let built = conveyor().unwrap();
    let a = format1();
    let b = format1();
    assert_eq!(a, b, "converting twice gives the same project");
    assert_eq!(a.parts, built.parts, "products keep their ids from their numbers");
    assert_eq!(a.devices, built.devices);
    assert_eq!(a.functions, built.functions);
    assert_eq!(a.connections, built.connections);
    assert_eq!(a.connectivity(), built.connectivity(), "the same terminals and nodes, same ids");
    assert_eq!(a.netlist(), built.netlist());
    let order = |p: &Project| p.pages_in_order().iter().map(|pg| pg.id).collect::<Vec<_>>();
    assert_eq!(order(&a), order(&built), "pages in the same order, now with sparse keys");
    assert!(a.pages.values().all(|pg| pg.order % ecad_model::PAGE_STEP == 0));
    // The same drawings, byte for byte.
    for page in a.pages_in_order() {
        assert_eq!(
            ecad_view::page_dxf(&a, page.id).unwrap(),
            ecad_view::page_dxf(&built, page.id).unwrap(),
            "page {}",
            page.name
        );
    }
    // Saved, it is format 2 and reads back identical.
    let loaded = save_and_load(&a);
    assert_eq!(loaded, a);
    assert_eq!(loaded.content_hash().unwrap(), a.content_hash().unwrap());
}

#[test]
fn renumbering_a_terminal_group_changes_its_designation_not_its_ids_or_wires() {
    let mut p = conveyor().unwrap();
    let f = drawing(&p, "-X3", ecad_model::conveyor::TWO_LEVEL, Some("1"));
    let ends = wire_ends(&p);
    let wires = p.connections.clone();
    let ids: BTreeMap<String, ecad_model::TerminalId> = p.function(f).unwrap().terminals.clone();
    let renumber: BTreeMap<String, String> = ids.keys().map(|pt| (pt.clone(), "9".to_owned())).collect();
    p.apply(Command::SetDesignations { id: f, designations: renumber }).unwrap();
    assert_eq!(p.drawing_number(f).as_deref(), Some("9"));
    assert!(ids.values().all(|t| p.terminals[t].designation == "9"), "every level of the group");
    assert_eq!(p.function(f).unwrap().terminals, ids, "the same terminals");
    assert_eq!(wire_ends(&p), ends, "every wire ends where it did");
    assert_eq!(p.connections, wires);
    // And so after a save and load (terminals.json).
    let loaded = save_and_load(&p);
    assert_eq!(loaded.terminals, p.terminals);
    assert_eq!(wire_ends(&loaded), ends);
}

#[test]
fn plc_channel_data_is_typed_on_the_channel_terminal_and_persists() {
    let p = conveyor().unwrap();
    let channels: Vec<&ecad_model::TerminalEntry> = p.terminals.values().filter(|t| t.channel.is_some()).collect();
    assert_eq!(channels.len(), 13, "one per channel drawing");
    for t in &channels {
        let c = t.channel.as_ref().unwrap();
        let drawing = p.functions.values().find(|f| f.terminals.values().any(|x| *x == t.id)).unwrap();
        let role = p.role(drawing.id).unwrap();
        assert!(matches!(role, ecad_model::Role::PlcChannel { kind } if kind == c.kind), "typed kind");
        assert!(!c.address.is_empty() && !drawing.properties.contains_key(ecad_model::ADDRESS), "on the terminal only");
    }
    let loaded = save_and_load(&p);
    assert_eq!(loaded.terminals, p.terminals, "terminals.json keeps the channel data");
}
