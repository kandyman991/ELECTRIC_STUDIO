//! The editor driven the way a shell drives it: pointer events in page millimetres and keys.

use super::*;
use ecad_model::sample::{K1_COIL, PAGE_1, start_circuit};

fn editor() -> Editor {
    Editor::new(start_circuit().unwrap(), PAGE_1).unwrap()
}

fn click(e: &mut Editor, x: f64, y: f64) {
    let p = Point::new(x, y);
    e.pointer_move(p);
    e.pointer_down(p);
    e.pointer_up(p);
}

fn drag(e: &mut Editor, from: (f64, f64), to: (f64, f64)) {
    e.pointer_move(Point::new(from.0, from.1));
    e.pointer_down(Point::new(from.0, from.1));
    for i in 1..=4 {
        let t = f64::from(i) / 4.0;
        e.pointer_move(Point::new(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
    }
    e.pointer_up(Point::new(to.0, to.1));
}

fn tag_at(e: &Editor, x: f64, y: f64) -> Option<String> {
    e.project()
        .functions
        .values()
        .find(|f| f.position == Point::new(x, y))
        .map(|f| e.project().device(f.device).unwrap().tag.clone())
}

fn wire_numbers(e: &Editor) -> Vec<String> {
    let mut v: Vec<String> = e.project().connections.values().filter_map(|c| c.wire_number.clone()).collect();
    v.sort();
    v
}

#[test]
fn dragging_a_symbol_snaps_to_the_grid_and_is_one_undo_step() {
    let mut e = editor();
    let rev = e.revision();
    // Grab the coil box (centre at 0,12) and drag it by an off-grid amount.
    drag(&mut e, (1.0, 12.0), (31.3, 17.9));
    let pos = e.project().function(K1_COIL).unwrap().position;
    assert_eq!(pos, Point::new(32.0, 16.0), "(30.3, 17.9) after the grab offset, snapped to the 4 mm grid");
    assert!(e.revision() > rev && e.is_dirty());
    assert_eq!(e.undo_label(), Some("Move -K1"));
    e.key(Key::Undo);
    assert_eq!(e.project().function(K1_COIL).unwrap().position, Point::new(0.0, 12.0));
    assert_eq!(e.status(), "Undid: Move -K1");
    assert_eq!(e.undo_label(), None, "the whole drag was a single step");
}

#[test]
fn the_place_tool_numbers_new_devices_and_undo_removes_them() {
    let mut e = editor();
    e.set_tool(Tool::Place(SymbolId::new("iec.coil")));
    e.pointer_move(Point::new(41.0, 39.0));
    assert!(e.overlays().iter().any(|o| matches!(o, Overlay::Ghost { lines } if lines.len() == 6)));
    click(&mut e, 41.0, 39.0);
    click(&mut e, 61.0, 39.0);
    assert_eq!(tag_at(&e, 40.0, 40.0).as_deref(), Some("-K2"));
    assert_eq!(tag_at(&e, 60.0, 40.0).as_deref(), Some("-K3"));
    e.key(Key::Undo);
    e.key(Key::Undo);
    assert_eq!(e.project(), &start_circuit().unwrap());
}

#[test]
fn the_wire_tool_connects_two_points_and_reuses_free_numbers() {
    let mut e = editor();
    // Remove W2 (between -S1:14 at 0,32 and -K1:A1 at 0,20) by clicking it and pressing Delete.
    click(&mut e, 0.2, 26.0);
    assert!(matches!(e.selection(), Some(Selected::Connection(_))));
    e.key(Key::Delete);
    assert_eq!(wire_numbers(&e), ["W1", "W3"]);

    e.set_tool(Tool::Wire);
    let open = e.overlays().iter().filter(|o| matches!(o, Overlay::Marker { kind: Marker::Open, .. })).count();
    assert_eq!(open, 2, "-S1:14 and -K1:A1 are free");
    click(&mut e, 0.3, 31.8);
    e.pointer_move(Point::new(0.1, 20.4));
    assert!(e.overlays().iter().any(|o| matches!(o, Overlay::RubberBand { points: p } if p.len() == 2)));
    click(&mut e, 0.1, 20.4);
    assert_eq!(wire_numbers(&e), ["W1", "W2", "W3"]);
    assert_eq!(e.undo_label(), Some("Draw wire W2"));
}

#[test]
fn wiring_an_occupied_point_explains_why_and_changes_nothing() {
    let mut e = editor();
    e.set_tool(Tool::Wire);
    click(&mut e, 0.0, 48.0); // -S1:13 already has W1
    assert_eq!(e.status(), "-S1:13 already has wire W1");
    assert!(!e.is_dirty());
    assert!(!e.overlays().iter().any(|o| matches!(o, Overlay::RubberBand { .. })));
}

#[test]
fn mirror_flips_the_selected_symbol_and_undoes() {
    let mut e = editor();
    click(&mut e, 2.0, 12.0);
    e.key(Key::Mirror);
    assert!(e.project().function(K1_COIL).unwrap().mirrored);
    assert_eq!(e.undo_label(), Some("Mirror -K1"));
    e.key(Key::Mirror);
    assert!(!e.project().function(K1_COIL).unwrap().mirrored);
    assert_eq!(e.undo_label(), Some("Unmirror -K1"));
    e.key(Key::Undo);
    e.key(Key::Undo);
    assert_eq!(e.project(), &start_circuit().unwrap());
}

#[test]
fn rotate_and_delete_act_on_the_selected_symbol() {
    let mut e = editor();
    click(&mut e, 2.0, 12.0);
    assert_eq!(e.selection(), Some(Selected::Function(K1_COIL)));
    e.key(Key::Rotate);
    assert_eq!(e.project().function(K1_COIL).unwrap().rotation, Rotation::R90);
    e.key(Key::Delete);
    assert!(e.project().function(K1_COIL).is_err());
    assert!(e.project().device_by_tag("-K1").is_none(), "its only drawing went, so the device went too");
    assert_eq!(wire_numbers(&e), ["W1"]);
    e.key(Key::Undo);
    e.key(Key::Undo);
    assert_eq!(e.project(), &start_circuit().unwrap());
    assert_eq!(e.selection(), None, "selection doesn't point at things undo brought back");
}

#[test]
fn renaming_checks_for_duplicate_tags() {
    let mut e = editor();
    click(&mut e, 2.0, 12.0);
    assert_eq!(e.rename_selected("-S1"), Err(ModelError::DuplicateTag("-S1".into())));
    e.rename_selected("-K10").unwrap();
    assert!(e.project().device_by_tag("-K10").is_some());
    assert_eq!(e.undo_label(), Some("Rename -K1 to -K10"));
}

#[test]
fn points_win_over_symbols_and_symbols_over_wires() {
    let mut e = editor();
    e.pointer_move(Point::new(0.0, 20.0));
    assert_eq!(e.hover(), Some(&Hit::Point(ConnectionRef::new(K1_COIL, "A1"))));
    e.pointer_move(Point::new(3.0, 12.0));
    assert_eq!(e.hover(), Some(&Hit::Function(K1_COIL)));
    e.pointer_move(Point::new(30.0, 30.0));
    assert_eq!(e.hover(), None);
    let (lo, hi) = e.extents().unwrap();
    assert!(lo.y <= -12.0 && hi.y >= 64.0);
}

#[test]
fn escape_cancels_a_wire_then_leaves_the_tool() {
    let mut e = editor();
    e.set_tool(Tool::Place(SymbolId::new("iec.coil")));
    e.key(Key::Escape);
    assert_eq!(e.tool(), &Tool::Select);
    click(&mut e, 2.0, 12.0);
    e.key(Key::Escape);
    assert_eq!(e.selection(), None);
}

#[test]
fn the_side_panel_edits_descriptions_and_wire_numbers() {
    let mut e = editor();
    click(&mut e, 2.0, 12.0);
    e.describe_selected("  Pump contactor ").unwrap();
    assert_eq!(e.project().device_by_tag("-K1").unwrap().description, "Pump contactor");
    click(&mut e, 0.2, 26.0); // W2
    assert_eq!(e.number_selected_wire("W1"), Err(ModelError::DuplicateWireNumber("W1".into())));
    e.number_selected_wire("102").unwrap();
    assert_eq!(wire_numbers(&e), ["102", "W1", "W3"]);
    assert_eq!(e.undo_label(), Some("Renumber wire W2 to 102"));
    e.key(Key::Undo);
    e.key(Key::Undo);
    assert_eq!(e.project(), &start_circuit().unwrap());
}

#[test]
fn undoing_back_to_the_saved_state_is_clean_and_a_new_branch_is_not() {
    let mut e = editor();
    e.mark_saved();
    click(&mut e, 2.0, 12.0);
    e.key(Key::Rotate);
    assert!(e.is_dirty());
    e.key(Key::Undo);
    assert!(!e.is_dirty(), "back at the saved state");
    e.key(Key::Redo);
    e.mark_saved();
    e.key(Key::Undo);
    assert!(e.is_dirty());
    e.key(Key::Delete); // a different change: the saved state (rotated) can't be reached any more
    e.key(Key::Undo);
    assert!(e.is_dirty(), "unrotated is not what was saved");
}

#[test]
fn a_coil_gets_contacts_on_another_page_with_the_next_numbers() {
    use ecad_model::sample::{K1_NO, with_contacts};
    let mut e = Editor::new(with_contacts().unwrap(), PAGE_1).unwrap();
    let k1 = e.project().function(K1_COIL).unwrap().device;
    let page2 = e.project().function(K1_NO).unwrap().page;
    e.set_page(page2).unwrap();
    e.set_tool(Tool::PlaceFor { symbol: SymbolId::new("iec.contact_no"), device: k1 });
    click(&mut e, 0.0, 60.0);
    assert_eq!(e.tool(), &Tool::Select, "one contact, then back to Select");
    assert_eq!(e.undo_label(), Some("Place -K1 contact 33-34"));
    let terminals: Vec<String> = e.project().contacts_of(k1).iter().map(|c| c.terminals.clone()).collect();
    assert_eq!(terminals, ["13-14", "21-22", "33-34"]);
    e.key(Key::Undo);
    assert_eq!(e.project(), &with_contacts().unwrap());
}

#[test]
fn a_stray_contact_can_join_a_coil_and_its_old_device_goes() {
    let mut e = editor();
    // Place a new NC contact: it becomes device -S2 with terminals 11-12.
    e.set_tool(Tool::Place(SymbolId::new("iec.contact_nc")));
    click(&mut e, 40.0, 40.0);
    assert!(e.project().device_by_tag("-S2").is_some());
    assert_eq!(e.reassign_selected("-K9"), Err(ModelError::UnknownTag("-K9".into())));
    e.reassign_selected(" -K1 ").unwrap();
    assert!(e.project().device_by_tag("-S2").is_none(), "nothing else drew -S2");
    let k1 = e.project().device_by_tag("-K1").unwrap().id;
    let contacts = e.project().contacts_of(k1);
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].terminals, "11-12", "first contact of -K1 is 1x; NC keeps 1/2");
    assert_eq!(e.undo_label(), Some("Move -S2 to -K1"));
    e.key(Key::Undo);
    assert!(e.project().device_by_tag("-S2").is_some());
}

#[test]
fn wires_into_a_junction_take_its_free_points_and_a_selected_wire_shows_its_potential() {
    use ecad_model::sample::{JUNCTION_1, with_potentials};
    let mut e = Editor::new(with_potentials().unwrap(), PAGE_1).unwrap();
    // The junction at (0,-8) has W3 on point 1, W8 on 2 and W7 on 4: hovering it offers 3.
    e.set_tool(Tool::Wire);
    e.pointer_move(Point::new(0.1, -8.1));
    assert_eq!(e.hover(), Some(&Hit::Point(ConnectionRef::new(JUNCTION_1, "3"))));
    // Selecting W3 highlights the other M wires on this page (W7, W8), not W1.
    e.set_tool(Tool::Select);
    click(&mut e, 0.2, -2.0);
    let w3 = e.project().connections.values().find(|c| c.wire_number.as_deref() == Some("W3")).unwrap().id;
    assert_eq!(e.selection(), Some(Selected::Connection(w3)));
    let highlighted = e.overlays().iter().filter(|o| matches!(o, Overlay::Potential { .. })).count();
    assert_eq!(highlighted, 2);
    assert_eq!(e.potential_of(w3).and_then(|p| p.name()).as_deref(), Some("M"));
}

/// The start circuit with the coil moved 16 mm right, so that its wires bend.
fn editor_with_bends() -> Editor {
    let mut e = editor();
    drag(&mut e, (1.0, 12.0), (17.0, 12.0));
    e
}

/// A wire with a middle run, and the middle of that run.
fn bent_wire(e: &Editor) -> (ConnectionId, ecad_model::project::MiddleRun, Point) {
    for c in e.project().connections.values() {
        if let Some(run) = e.middle_run(c.id) {
            let route = e.project().route(c).unwrap();
            // The middle run is the segment at `run.at` along the run's direction.
            let seg = route
                .windows(2)
                .find(|s| {
                    if run.horizontal {
                        s[0].y == run.at && s[1].y == run.at
                    } else {
                        s[0].x == run.at && s[1].x == run.at
                    }
                })
                .unwrap();
            let mid = Point::new((seg[0].x + seg[1].x) / 2.0, (seg[0].y + seg[1].y) / 2.0);
            return (c.id, run, mid);
        }
    }
    panic!("no bent wire");
}

#[test]
fn dragging_a_wire_moves_its_middle_run_on_the_grid_as_one_undo_step() {
    let mut e = editor_with_bends();
    let (id, run, mid) = bent_wire(&e);
    // A click with a little jitter only selects the wire.
    e.pointer_move(mid);
    e.pointer_down(mid);
    let jitter = if run.horizontal { Point::new(mid.x, mid.y + 0.7) } else { Point::new(mid.x + 0.7, mid.y) };
    e.pointer_move(jitter);
    e.pointer_up(jitter);
    assert_eq!(e.project().connection(id).unwrap().bend, None);
    assert_eq!(e.selection(), Some(Selected::Connection(id)));

    let to = if run.horizontal { (mid.x, mid.y - 7.3) } else { (mid.x - 7.3, mid.y) };
    drag(&mut e, (mid.x, mid.y), to);
    let bend = e.project().connection(id).unwrap().bend.unwrap();
    let want = if run.horizontal { (to.1 / GRID).round() * GRID } else { (to.0 / GRID).round() * GRID };
    assert_eq!(bend, want);
    let route = e.project().route(e.project().connection(id).unwrap()).unwrap();
    assert!(route.iter().any(|p| if run.horizontal { p.y == bend } else { p.x == bend }), "{route:?}");
    assert!(e.undo_label().is_some_and(|l| l.starts_with("Move wire")), "{:?}", e.undo_label());

    e.straighten_selected_wire().unwrap();
    assert_eq!(e.project().connection(id).unwrap().bend, None);
    e.key(Key::Undo);
    assert_eq!(e.project().connection(id).unwrap().bend, Some(bend));
    e.key(Key::Undo);
    assert_eq!(e.project().connection(id).unwrap().bend, None, "the whole drag was one step");
    assert_eq!(e.undo_label(), Some("Move -K1"));
}

#[test]
fn tidy_wires_moves_a_wire_off_another_and_undo_puts_it_back() {
    let mut e = editor_with_bends();
    assert_eq!(e.tidy_wires().unwrap(), 0, "nothing clashes yet");
    let (id, run, _) = bent_wire(&e);
    // Push the middle run onto every other wire's line until one clashes, then tidy.
    let page = e.page();
    let clashing = (-40..=40).map(|k| run.at + f64::from(k) * GRID).find(|at| {
        let mut p = e.project().clone();
        p.apply(Command::SetWireBend { id, bend: Some(*at) }).unwrap();
        !p.wire_clashes(page).is_empty()
    });
    let at = clashing.expect("some bend puts the wire on another");
    e.execute("test", Command::SetWireBend { id, bend: Some(at) }).unwrap();
    assert!(!e.project().wire_clashes(page).is_empty());
    let moved = e.tidy_wires().unwrap();
    assert!(moved >= 1);
    assert!(e.project().wire_clashes(page).is_empty(), "{:?}", e.project().wire_clashes(page));
    assert_eq!(e.undo_label(), Some("Tidy wires"));
    e.key(Key::Undo);
    assert_eq!(e.project().connection(id).unwrap().bend, Some(at));
}

/// The reference project on its layout page, with the plate's mapping to paper.
fn layout_editor() -> (Editor, ecad_model::panel::PanelFrame) {
    let p = ecad_model::reference::reference_project().unwrap();
    let page = p.pages.values().find(|pg| pg.name == "L7").unwrap().id;
    let frame = p.panel_frame(page).unwrap();
    (Editor::new(p, page).unwrap(), frame)
}

fn item_of(e: &Editor, tag: &str) -> ecad_model::PanelItem {
    let d = e.project().device_by_tag(tag).unwrap().id;
    e.project().panel_item_of(d).unwrap().clone()
}

#[test]
fn a_footprint_is_dragged_onto_another_rail_and_deleted_back_to_the_unplaced_list() {
    let (mut e, frame) = layout_editor();
    assert!(e.on_layout_page());
    let qf1 = item_of(&e, "P2QF1");
    let (lo, hi) = e.project().panel_item_rect(&qf1);
    let middle = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    // The second rail from the top: centre at 1870 - (290 + 430) / 2.
    let rail_centre = 1870.0 - (290.0 + 430.0) / 2.0;
    let target = Point::new(middle.x + 400.0, rail_centre + 12.0);
    let (a, b) = (frame.to_paper(middle), frame.to_paper(target));
    drag(&mut e, (a.x, a.y), (b.x, b.y));
    let moved = item_of(&e, "P2QF1");
    let (lo, hi) = e.project().panel_item_rect(&moved);
    assert_eq!((lo.y + hi.y) / 2.0, rail_centre, "snapped onto the rail's centre line");
    assert_eq!(moved.at.x % 5.0, 0.0, "on the 5 mm grid");
    assert_eq!(e.undo_label(), Some("Move P2QF1"));
    assert_eq!(e.selection(), Some(Selected::PanelItem(qf1.id)));

    let d = e.project().device_by_tag("P2QF1").unwrap().id;
    e.key(Key::Delete);
    assert!(e.project().unplaced_devices().contains(&d));
    e.key(Key::Undo);
    e.key(Key::Undo);
    assert_eq!(item_of(&e, "P2QF1"), qf1);
}

#[test]
fn an_unplaced_device_is_placed_on_a_rail_and_rails_and_ducts_are_drawn() {
    let (mut e, frame) = layout_editor();
    // The cabinet fan has parts but no size and isn't on the plate.
    let m3 = e.project().device_by_tag("P3MS1").unwrap().id;
    assert!(e.project().unplaced_devices().contains(&m3));
    e.set_tool(Tool::PlaceOnPanel(m3));
    let rail_centre = 1870.0 - (290.0 + 430.0) / 2.0;
    let at = frame.to_paper(Point::new(700.0, rail_centre - 10.0));
    click(&mut e, at.x, at.y);
    let item = item_of(&e, "P3MS1");
    let (lo, hi) = e.project().panel_item_rect(&item);
    assert_eq!((lo.y + hi.y) / 2.0, rail_centre);
    assert_eq!(e.tool(), &Tool::Select);
    assert_eq!(e.undo_label(), Some("Place P3MS1"));

    // R turns it upright about its middle.
    e.key(Key::Rotate);
    assert!(item_of(&e, "P3MS1").vertical);

    // A rail and a duct drawn by dragging along the plate's empty right part of the tall gap.
    let rails =
        |e: &Editor| e.project().panel_items.values().filter(|i| i.kind == ecad_model::PanelItemKind::Rail).count();
    let before = rails(&e);
    e.set_tool(Tool::Rail);
    let (a, b) = (frame.to_paper(Point::new(500.0, 1000.0)), frame.to_paper(Point::new(780.0, 1003.0)));
    drag(&mut e, (a.x, a.y), (b.x, b.y));
    assert_eq!(rails(&e), before + 1);
    let Some(Selected::PanelItem(id)) = e.selection() else { panic!("the new rail is selected") };
    let rail = e.project().panel_items[&id].clone();
    assert_eq!((rail.length, rail.vertical, rail.at.y), (280.0, false, 1000.0 - 17.5));
    e.set_selected_panel_item(300.0, 0.0, 0.0, "TS35-2M").unwrap();
    assert_eq!((e.project().panel_items[&id].length, e.project().panel_items[&id].part.as_str()), (300.0, "TS35-2M"));
    e.set_tool(Tool::Duct);
    let (a, b) = (frame.to_paper(Point::new(480.0, 900.0)), frame.to_paper(Point::new(482.0, 760.0)));
    drag(&mut e, (a.x, a.y), (b.x, b.y));
    let Some(Selected::PanelItem(id)) = e.selection() else { panic!("the new duct is selected") };
    let duct = &e.project().panel_items[&id];
    assert!(duct.vertical && duct.length == 140.0);
    assert!(matches!(duct.kind, ecad_model::PanelItemKind::Duct { width, .. } if width == DUCT_WIDTH));
}

#[test]
fn layout_pages_are_added_resized_and_deleted_with_their_items() {
    let (mut e, _) = layout_editor();
    let pages = e.project().pages.len();
    e.add_layout_page(600.0, 800.0).unwrap();
    assert!(e.on_layout_page());
    assert_eq!(e.project().pages.len(), pages + 1);
    assert_eq!(e.project().page(e.page()).unwrap().name, "L1");
    e.set_panel_size(800.0, 1000.0).unwrap();
    assert_eq!(e.project().panels[&e.page()].width, 800.0);
    // Back to L7 and delete it: plate and items go, the devices stay (drawn elsewhere).
    let l7 = e.project().pages.values().find(|p| p.name == "L7").unwrap().id;
    e.set_page(l7).unwrap();
    let items = e.project().panel_items.len();
    assert!(items > 0);
    e.delete_page().unwrap();
    assert!(e.project().panel_items.is_empty() && e.project().device_by_tag("P2QF1").is_some());
    e.key(Key::Undo);
    assert_eq!(e.project().panel_items.len(), items);
}
