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
