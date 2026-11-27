//! The electrical project model: the single source of truth for Electric Studio.
//!
//! Drawings, reports and exports are all derived from this model. This crate must never depend
//! on CADCraft or on any UI crate (see AGENTS.md).

mod builder;
pub mod connectivity;
pub mod content_hash;
pub mod conveyor;
pub mod finite;
pub mod geometry;
pub mod history;
pub mod layout;
pub mod lengths;
pub mod library;
pub mod naming;
pub mod panel;
pub mod parts;
pub mod plc_address;
pub mod potential;
pub mod project;
pub mod reference;
pub mod sample;
pub mod sheet;
pub mod symbol;
pub mod underlay;

pub use connectivity::{Connectivity, Endpoint, Net, Netlist, NodeId, TerminalId};
pub use geometry::{Direction, Point, Rotation};
pub use history::History;
pub use lengths::{WireLength, WireLengthSettings};
pub use naming::{Naming, WireNumbering};
pub use panel::{Panel, PanelItem, PanelItemKind};
pub use parts::{Part, PartRef};
pub use potential::Potential;
pub use project::*;
pub use sheet::Sheet;
pub use symbol::*;

#[cfg(test)]
mod tests {
    use super::*;
    use sample::{K1_COIL, PAGE_1, start_circuit};

    fn assert_routes_land_on_terminals(p: &Project) {
        for c in p.connections.values() {
            let route = p.route(c).unwrap();
            assert_eq!(route[0], p.connection_point(&c.from).unwrap().0, "{:?}", c.wire_number);
            assert_eq!(*route.last().unwrap(), p.connection_point(&c.to).unwrap().0);
            for seg in route.windows(2) {
                assert!(seg[0].x == seg[1].x || seg[0].y == seg[1].y, "segment not orthogonal: {seg:?}");
            }
        }
    }

    fn wire(p: &Project, number: &str) -> Connection {
        p.connections.values().find(|c| c.wire_number.as_deref() == Some(number)).unwrap().clone()
    }

    #[test]
    fn wires_follow_a_moved_function() {
        let mut p = start_circuit().unwrap();
        assert_routes_land_on_terminals(&p);
        p.move_function(K1_COIL, Point::new(40.0, 16.0)).unwrap();
        assert_routes_land_on_terminals(&p);
        assert_eq!(p.connection_point(&ConnectionRef::new(K1_COIL, "A1")).unwrap().0, Point::new(40.0, 24.0));
    }

    #[test]
    fn wires_follow_a_rotated_function() {
        let mut p = start_circuit().unwrap();
        p.apply(Command::Rotate { id: K1_COIL, to: Rotation::R90 }).unwrap();
        let (a1, dir) = p.connection_point(&ConnectionRef::new(K1_COIL, "A1")).unwrap();
        assert_eq!((a1, dir), (Point::new(-8.0, 12.0), Direction::Left));
        assert_routes_land_on_terminals(&p);
    }

    #[test]
    fn routes_leave_terminals_outward_and_never_cross_their_symbol() {
        let mut p = start_circuit().unwrap();
        p.move_function(K1_COIL, Point::new(40.0, 16.0)).unwrap();
        p.apply(Command::Rotate { id: K1_COIL, to: Rotation::R90 }).unwrap();
        // The rotated coil's box spans x 37..43, y 11..21; A2 faces right at (48, 16).
        for c in p.connections.values() {
            let route = p.route(c).unwrap();
            for seg in route.windows(2) {
                let (x0, x1) = (seg[0].x.min(seg[1].x), seg[0].x.max(seg[1].x));
                let (y0, y1) = (seg[0].y.min(seg[1].y), seg[0].y.max(seg[1].y));
                let crosses_box = x0 < 43.0 && x1 > 37.0 && y0 < 21.0 && y1 > 11.0;
                assert!(!crosses_box, "{:?} crosses the coil: {route:?}", c.wire_number);
            }
        }
        let route = p.route(&wire(&p, "W3")).unwrap();
        assert_eq!(route[1], Point::new(48.0 + project::ROUTE_STUB, 16.0), "leaves A2 to the right first");
    }

    #[test]
    fn aligned_facing_points_get_a_straight_wire() {
        let p = start_circuit().unwrap();
        assert!(p.connections.values().all(|c| p.route(c).unwrap().len() == 2));
    }

    #[test]
    fn a_connection_point_takes_one_wire() {
        let mut p = start_circuit().unwrap();
        let err = p
            .connect(Connection {
                bend: None,
                id: ConnectionId::new(),
                from: ConnectionRef::new(K1_COIL, "A1"),
                to: ConnectionRef::new(K1_COIL, "A2"),
                wire_number: None,
                spec: Default::default(),
            })
            .unwrap_err();
        assert_eq!(err, ModelError::AlreadyConnected(("-K1".into(), "A1".into()), ("-S1".into(), "14".into())));
    }

    #[test]
    fn unknown_points_and_tags_are_rejected() {
        let mut p = start_circuit().unwrap();
        let err = p
            .connect(Connection {
                bend: None,
                id: ConnectionId::new(),
                from: ConnectionRef::new(K1_COIL, "A3"),
                to: ConnectionRef::new(K1_COIL, "A1"),
                wire_number: None,
                spec: Default::default(),
            })
            .unwrap_err();
        assert!(matches!(err, ModelError::NoConnectionPoint { .. }));
        let dup = p.add_device(Device::new(DeviceId::new(), "-K1", ""));
        assert_eq!(dup.unwrap_err(), ModelError::DuplicateTag("-K1".into()));
        assert_eq!(p.connections_on(PAGE_1).count(), 3);
    }

    #[test]
    fn unplace_removes_its_wires_and_undo_restores_everything() {
        let original = start_circuit().unwrap();
        let mut p = original.clone();
        let mut h = History::new();
        h.execute(&mut p, "Delete -K1", Command::Unplace(K1_COIL)).unwrap();
        assert!(p.function(K1_COIL).is_err());
        assert_eq!(p.connections.len(), 1, "W2 and W3 went with the coil");
        assert_eq!(h.undo(&mut p).unwrap().as_deref(), Some("Delete -K1"));
        assert_eq!(p, original);
        h.redo(&mut p).unwrap();
        assert_eq!(p.connections.len(), 1);
        h.undo(&mut p).unwrap();
        assert_eq!(p, original);
        assert!(!h.can_undo() && h.can_redo());
    }

    #[test]
    fn a_failing_batch_changes_nothing() {
        let original = start_circuit().unwrap();
        let mut p = original.clone();
        let err = p.apply(Command::Batch(vec![
            Command::Move { id: K1_COIL, to: Point::new(100.0, 100.0) },
            Command::Disconnect(wire(&original, "W1").id),
            Command::RenameDevice { id: DeviceId::from_u128(0x103), tag: "-S1".into() },
        ]));
        assert_eq!(err.unwrap_err(), ModelError::DuplicateTag("-S1".into()));
        assert_eq!(p, original);
    }

    #[test]
    fn wire_numbers_are_unique_and_descriptions_undo() {
        let mut p = start_circuit().unwrap();
        let w2 = wire(&p, "W2").id;
        assert_eq!(
            p.apply(Command::SetWireNumber { id: w2, number: Some("W1".into()) }).unwrap_err(),
            ModelError::DuplicateWireNumber("W1".into())
        );
        let undo = p.apply(Command::SetWireNumber { id: w2, number: Some(" 102 ".into()) }).unwrap();
        assert_eq!(p.connection(w2).unwrap().wire_number.as_deref(), Some("102"), "trimmed");
        p.apply(undo).unwrap();
        assert_eq!(p.connection(w2).unwrap().wire_number.as_deref(), Some("W2"));
        p.apply(Command::SetWireNumber { id: w2, number: Some("  ".into()) }).unwrap();
        assert_eq!(p.connection(w2).unwrap().wire_number, None, "blank clears it");

        let k1 = DeviceId::from_u128(0x103);
        let undo = p.apply(Command::SetDescription { id: k1, description: "Pump contactor".into() }).unwrap();
        assert_eq!(p.device(k1).unwrap().description, "Pump contactor");
        p.apply(undo).unwrap();
        assert_eq!(p.device(k1).unwrap().description, "Motor contactor");
    }

    #[test]
    fn function_bounds_follow_rotation() {
        let mut p = start_circuit().unwrap();
        assert_eq!(p.function_bounds(K1_COIL).unwrap(), Some((Point::new(-5.0, 4.0), Point::new(5.0, 20.0))));
        p.apply(Command::Rotate { id: K1_COIL, to: Rotation::R90 }).unwrap();
        assert_eq!(p.function_bounds(K1_COIL).unwrap(), Some((Point::new(-8.0, 7.0), Point::new(8.0, 17.0))));
    }

    #[test]
    fn pages_are_added_renamed_reordered_and_removed_with_undo() {
        let original = start_circuit().unwrap();
        let mut p = original.clone();
        let mut h = History::new();
        let two = Page::new("2", 1);
        let two_id = two.id;
        h.execute(&mut p, "Add page 2", Command::AddPage(two)).unwrap();
        assert_eq!(
            p.apply(Command::AddPage(Page::new("2", 5))).unwrap_err(),
            ModelError::DuplicatePageName("2".into())
        );
        assert_eq!(
            p.apply(Command::RenamePage { id: two_id, name: "1".into() }).unwrap_err(),
            ModelError::DuplicatePageName("1".into())
        );
        assert_eq!(
            p.apply(Command::RenamePage { id: two_id, name: " ".into() }).unwrap_err(),
            ModelError::EmptyPageName
        );
        h.execute(&mut p, "Rename", Command::RenamePage { id: two_id, name: " 2a ".into() }).unwrap();
        h.execute(&mut p, "Describe", Command::DescribePage { id: two_id, description: "Motor".into() }).unwrap();
        h.execute(&mut p, "Move first", Command::ReorderPages(vec![two_id, PAGE_1])).unwrap();
        let names: Vec<&str> = p.pages_in_order().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["2a", "1"]);
        assert_eq!(p.apply(Command::ReorderPages(vec![two_id])).unwrap_err(), ModelError::BadPageOrder);
        assert_eq!(p.apply(Command::RemovePage(PAGE_1)).unwrap_err(), ModelError::PageNotEmpty("1".into()));
        h.execute(&mut p, "Remove", Command::RemovePage(two_id)).unwrap();
        while h.can_undo() {
            h.undo(&mut p).unwrap();
        }
        assert_eq!(p, original);
    }

    #[test]
    fn locations_name_the_page_and_column() {
        let p = start_circuit().unwrap();
        let k1 = p.function(K1_COIL).unwrap();
        let col = p.sheet.column_of(k1.position);
        assert_eq!(p.location(PAGE_1, k1.position).unwrap(), format!("1.{col}"));
        let (lo, hi) = p.sheet.frame();
        let (c_lo, c_hi) = p.content_bounds(PAGE_1).unwrap();
        assert!(lo.x < c_lo.x && lo.y < c_lo.y && hi.x > c_hi.x && hi.y > c_hi.y, "the sample sits on its sheet");
    }

    #[test]
    fn contacts_get_iec_numbers_and_refer_to_their_coil() {
        let p = sample::with_contacts().unwrap();
        let k1 = DeviceId::from_u128(0x103);
        let contacts = p.contacts_of(k1);
        let summary: Vec<(String, &str, String)> =
            contacts.iter().map(|c| (c.terminals.clone(), c.kind.short(), c.location.clone())).collect();
        let col = |f| p.sheet.column_of(p.function(f).unwrap().position);
        assert_eq!(
            summary,
            [
                ("13-14".to_owned(), "NO", format!("2.{}", col(sample::K1_NO))),
                ("21-22".to_owned(), "NC", format!("2.{}", col(sample::K1_NC))),
            ]
        );
        assert_eq!(p.main_function(k1).map(|f| f.id), Some(K1_COIL));
        assert_eq!(p.function_location(K1_COIL).unwrap(), format!("1.{}", col(K1_COIL)));
        // A third contact takes the next free number; the sample pushbutton -S1 has no coil.
        let third = p.next_designations(k1, &SymbolId::new("iec.contact_no"));
        assert_eq!(third.get("13").map(String::as_str), Some("33"));
        assert!(p.next_designations(k1, &SymbolId::new("iec.coil")).is_empty());
        assert!(p.main_function(DeviceId::from_u128(0x102)).is_none());
        // Wires and reports name the contact by its own designation.
        assert_eq!(p.describe(&ConnectionRef::new(sample::K1_NC, "11")).unwrap(), ("-K1".into(), "21".into()));
    }

    #[test]
    fn contacts_take_their_numbers_from_the_parts_contact_blocks() {
        use crate::parts::{block_kind, parse_blocks};
        assert_eq!(parse_blocks("13-14, 21-22  11-12-14").unwrap(), ["13-14", "21-22", "11-12-14"]);
        assert_eq!(parse_blocks("13-15").unwrap_err(), "13-15");
        assert_eq!(block_kind("57-58"), Some(ContactKind::NormallyOpen));

        let mut p = sample::with_contacts().unwrap();
        let k1 = p.function(K1_COIL).unwrap().device;
        let place = |p: &mut Project, symbol: &str, x: f64| {
            let symbol = SymbolId::new(symbol);
            let designations = p.next_designations(k1, &symbol);
            let id = FunctionId::new();
            let page = p.function(K1_COIL).unwrap().page;
            p.place(Function {
                id,
                device: k1,
                symbol,
                page,
                position: Point::new(x, 200.0),
                rotation: Default::default(),
                designations,
                properties: Default::default(),
                mirrored: false,
            })
            .unwrap();
            id
        };
        // A relay with one changeover, used twice: whole, then (the open side) as an NO.
        p.apply(Command::AddPart(Part {
            number: "R-1CO".into(),
            manufacturer: String::new(),
            description: String::new(),
            code: String::new(),
            contacts: vec!["11-12-14".into()],
            width: None,
            height: None,
            depth: None,
        }))
        .unwrap();
        p.apply(Command::AddPart(Part {
            number: "BLK-NO".into(),
            manufacturer: String::new(),
            description: String::new(),
            code: String::new(),
            contacts: vec!["13-14".into()],
            width: None,
            height: None,
            depth: None,
        }))
        .unwrap();
        // Drop the sample's two contacts so the parts decide.
        for f in [sample::K1_NO, sample::K1_NC] {
            for c in p
                .connections
                .values()
                .filter(|c| c.from.function == f || c.to.function == f)
                .map(|c| c.id)
                .collect::<Vec<_>>()
            {
                p.apply(Command::Disconnect(c)).unwrap();
            }
            p.apply(Command::Unplace(f)).unwrap();
        }
        p.apply(Command::SetDeviceParts { id: k1, parts: vec![PartRef::new("R-1CO", 1), PartRef::new("BLK-NO", 2)] })
            .unwrap();
        assert_eq!(p.contact_blocks_of(k1), [vec!["11", "12", "14"], vec!["13", "14"], vec!["23", "24"]]);
        let co = place(&mut p, "iec.contact_co", 100.0);
        let co_ref = p.contacts_of(k1).into_iter().find(|c| c.function == co).unwrap();
        assert_eq!((co_ref.terminals.as_str(), co_ref.kind.short()), ("11-12-14", "CO"));
        assert_eq!(co_ref.poles, [("12/14".to_owned(), "11".to_owned())]);
        // The changeover is taken, so NO contacts come from the stacked blocks: 13-14, then 23-24.
        let numbers = |p: &Project, f| {
            let f = p.function(f).unwrap();
            let sym = p.symbol(&f.symbol).unwrap();
            sym.connection_points.iter().map(|cp| f.designation(&cp.name).to_owned()).collect::<Vec<_>>()
        };
        let no1 = place(&mut p, "iec.contact_no", 110.0);
        let no2 = place(&mut p, "iec.contact_no", 120.0);
        assert_eq!(numbers(&p, no1), ["13", "14"]);
        assert_eq!(numbers(&p, no2), ["23", "24"]);
        // With the changeover drawn only as its closed side, an NO uses its open side (11-14).
        p.apply(Command::Unplace(co)).unwrap();
        let nc = place(&mut p, "iec.contact_nc", 130.0);
        assert_eq!(numbers(&p, nc), ["11", "12"]);
        p.apply(Command::Unplace(no1)).unwrap();
        p.apply(Command::Unplace(no2)).unwrap();
        let no = place(&mut p, "iec.contact_no", 140.0);
        assert_eq!(numbers(&p, no), ["11", "14"], "the common is shared");
        // Once every block is used, numbering falls back to counting.
        place(&mut p, "iec.contact_no", 150.0);
        let extra = place(&mut p, "iec.contact_no", 160.0);
        assert!(!numbers(&p, extra).is_empty());
    }

    #[test]
    fn a_mirrored_drawing_flips_its_points_before_turning() {
        let mut p = start_circuit().unwrap();
        // A changeover's closed side 12 is left of its common; mirrored it is on the right.
        let device = DeviceId::new();
        p.add_device(Device::new(device, "-K9", "")).unwrap();
        let f = FunctionId::new();
        p.place(Function {
            id: f,
            device,
            symbol: SymbolId::new("iec.contact_co"),
            page: PAGE_1,
            position: Point::new(100.0, 100.0),
            rotation: Rotation::R0,
            mirrored: false,
            designations: Default::default(),
            properties: Default::default(),
        })
        .unwrap();
        let at = |p: &Project, point: &str| p.connection_point(&ConnectionRef::new(f, point)).unwrap();
        assert_eq!(at(&p, "12").0, Point::new(92.0, 108.0));
        let undo = p.apply(Command::Mirror { id: f, to: true }).unwrap();
        assert_eq!(at(&p, "12"), (Point::new(108.0, 108.0), Direction::Up));
        // Turned a quarter after the flip: 12 ends up below, wires leave it to the left.
        p.apply(Command::Rotate { id: f, to: Rotation::R90 }).unwrap();
        assert_eq!(at(&p, "12"), (Point::new(92.0, 108.0), Direction::Left));
        p.apply(Command::Rotate { id: f, to: Rotation::R0 }).unwrap();
        p.apply(undo).unwrap();
        assert_eq!(at(&p, "12").0, Point::new(92.0, 108.0));
        // A right-leaving point leaves left when mirrored.
        let motor = FunctionId::new();
        p.place(Function {
            id: motor,
            device,
            symbol: SymbolId::new("iec.motor_3"),
            page: PAGE_1,
            position: Point::new(200.0, 100.0),
            rotation: Rotation::R0,
            mirrored: true,
            designations: Default::default(),
            properties: Default::default(),
        })
        .unwrap();
        let (pe, leaves) = p.connection_point(&ConnectionRef::new(motor, "PE")).unwrap();
        assert_eq!((pe, leaves), (Point::new(188.0, 92.0), Direction::Left));
    }

    #[test]
    fn reassign_moves_a_contact_to_another_device_and_undoes() {
        let original = sample::with_contacts().unwrap();
        let mut p = original.clone();
        let s1 = DeviceId::from_u128(0x102);
        let undo =
            p.apply(Command::Reassign { id: sample::K1_NO, device: s1, designations: Default::default() }).unwrap();
        assert_eq!(p.contacts_of(DeviceId::from_u128(0x103)).len(), 1);
        assert_eq!(p.describe(&ConnectionRef::new(sample::K1_NO, "13")).unwrap(), ("-S1".into(), "13".into()));
        p.apply(undo).unwrap();
        assert_eq!(p, original);
    }

    #[test]
    fn the_parts_catalogue_and_device_parts_follow_their_rules() {
        let original = start_circuit().unwrap();
        let mut p = original.clone();
        let mut h = History::new();
        let k1 = DeviceId::from_u128(0x103);
        let part = |n: &str| Part {
            number: n.into(),
            manufacturer: "M".into(),
            description: String::new(),
            code: String::new(),
            contacts: Vec::new(),
            width: None,
            height: None,
            depth: None,
        };
        assert_eq!(
            p.apply(Command::AddPart(part("EX-CT-24DC"))).unwrap_err(),
            ModelError::DuplicatePart("EX-CT-24DC".into())
        );
        assert_eq!(p.apply(Command::AddPart(part("  "))).unwrap_err(), ModelError::EmptyPartNumber);
        h.execute(&mut p, "Add", Command::AddPart(part(" EX-NEW "))).unwrap();
        assert!(p.parts.contains_key("EX-NEW"), "trimmed");
        assert_eq!(
            p.apply(Command::RemovePart("EX-CT-24DC".into())).unwrap_err(),
            ModelError::PartInUse("EX-CT-24DC".into(), "-K1".into())
        );
        assert_eq!(
            p.apply(Command::SetDeviceParts { id: k1, parts: vec![PartRef::new("NOPE", 1)] }).unwrap_err(),
            ModelError::UnknownPart("NOPE".into())
        );
        assert_eq!(
            p.apply(Command::SetDeviceParts { id: k1, parts: vec![PartRef::new("EX-NEW", 0)] }).unwrap_err(),
            ModelError::ZeroQuantity
        );
        let parts = vec![PartRef::new("EX-CT-24DC", 1), PartRef::new("EX-AUX-1NO1NC", 2)];
        h.execute(&mut p, "Parts", Command::SetDeviceParts { id: k1, parts: parts.clone() }).unwrap();
        assert_eq!(p.device(k1).unwrap().parts, parts);
        h.execute(&mut p, "Edit", Command::UpdatePart(Part { description: "new".into(), ..part("EX-NEW") })).unwrap();
        h.execute(&mut p, "Remove", Command::RemovePart("EX-NEW".into())).unwrap();
        while h.can_undo() {
            h.undo(&mut p).unwrap();
        }
        assert_eq!(p, original);
    }

    #[test]
    fn terminals_are_numbered_naturally_and_uniquely_per_strip() {
        let mut p = sample::with_terminals().unwrap();
        let strip = DeviceId::from_u128(0x10a);
        let numbers = |p: &Project| p.terminals_of(strip).iter().map(|t| t.number.clone()).collect::<Vec<_>>();
        assert_eq!(numbers(&p), ["1", "2"]);
        assert_eq!(p.describe(&ConnectionRef::new(sample::X2_2, "2")).unwrap(), ("-X2".into(), "2".into()));
        // Both sides of a terminal are one potential: L+ reaches -S1:13 through -X2:1.
        let w9 = p.connections.values().find(|c| c.wire_number.as_deref() == Some("W9")).unwrap().id;
        assert_eq!(p.potential_of(w9).and_then(|x| x.name()).as_deref(), Some("L+"));
        let ten: std::collections::BTreeMap<String, String> =
            [("1".into(), "10".into()), ("2".into(), "10".into())].into();
        p.apply(Command::SetDesignations { id: sample::X2_1, designations: ten }).unwrap();
        assert_eq!(numbers(&p), ["2", "10"], "10 sorts after 2");
        assert_eq!(p.next_terminal_number(strip), "11");
        let two: std::collections::BTreeMap<String, String> =
            [("1".into(), "2".into()), ("2".into(), "2".into())].into();
        assert_eq!(
            p.apply(Command::SetDesignations { id: sample::X2_1, designations: two }).unwrap_err(),
            ModelError::DuplicateNumber("-X2".into(), "2".into())
        );
        assert_eq!(numbers(&p), ["2", "10"], "a refused renumber changes nothing");
    }

    #[test]
    fn cables_have_unique_tags_and_cores() {
        let mut p = start_circuit().unwrap();
        let w = CableId::from_u128(1);
        let cable = Cable { id: w, tag: "-W1".into(), kind: "3G1,5".into(), cores: 3, shielded: false };
        p.apply(Command::AddCable(cable.clone())).unwrap();
        assert_eq!(
            p.apply(Command::AddCable(Cable { id: CableId::from_u128(2), ..cable.clone() })).unwrap_err(),
            ModelError::DuplicateTag("-W1".into())
        );
        let wires: Vec<ConnectionId> = p.connections.keys().copied().collect();
        let core = |n: &str| WireSpec { cable: Some(CableCore { cable: w, core: n.into() }), ..WireSpec::default() };
        p.apply(Command::SetWireSpec { id: wires[0], spec: core("1") }).unwrap();
        assert_eq!(
            p.apply(Command::SetWireSpec { id: wires[1], spec: core("1") }).unwrap_err(),
            ModelError::CoreInUse("-W1".into(), "1".into())
        );
        assert_eq!(p.apply(Command::RemoveCable(w)).unwrap_err(), ModelError::CableInUse("-W1".into()));
        // Colours are upper case; undo puts the wire back.
        let spec = WireSpec { colour: " bu ".into(), cross_section: "1,5".into(), cable: None };
        let undo = p.apply(Command::SetWireSpec { id: wires[0], spec }).unwrap();
        assert_eq!(p.connection(wires[0]).unwrap().spec.colour, "BU");
        p.apply(undo).unwrap();
        assert_eq!(p.connection(wires[0]).unwrap().spec, core("1"));
    }

    #[test]
    fn boxes_tell_where_devices_are_and_plug_pins_number_like_terminals() {
        let mut p = sample::with_power().unwrap();
        let motor = FunctionId::from_u128(0x22c);
        assert_eq!(p.box_labels(motor), ["EXT"]);
        // An inner box around the motor too: outermost first.
        let inner = LocationBox {
            id: BoxId::from_u128(0x601),
            page: sample::PAGE_4,
            min: Point::new(-104.0, -64.0),
            max: Point::new(-76.0, -40.0),
            label: " -XCPR1 ".into(),
        };
        p.apply(Command::AddBox(inner.clone())).unwrap();
        assert_eq!(p.box_labels(motor), ["EXT", "-XCPR1"]);
        let tiny = LocationBox { id: BoxId::from_u128(0x602), max: Point::new(-102.0, -62.0), ..inner };
        assert_eq!(p.apply(Command::AddBox(tiny)).unwrap_err(), ModelError::BoxTooSmall);
        // A page with a box isn't empty.
        p.apply(Command::AddPage(Page::new("9", 9))).unwrap();
        let nine = p.pages.values().find(|x| x.name == "9").unwrap().id;
        let b = LocationBox {
            id: BoxId::from_u128(0x603),
            page: nine,
            min: Point::ZERO,
            max: Point::new(8.0, 8.0),
            label: String::new(),
        };
        p.apply(Command::AddBox(b)).unwrap();
        assert_eq!(p.apply(Command::RemovePage(nine)).unwrap_err(), ModelError::PageNotEmpty("9".into()));

        // Plug pins are numbered on their connector like terminals on a strip.
        let xd = DeviceId::from_u128(0x700);
        p.add_device(Device::new(xd, "-XD1", "")).unwrap();
        let pin = SymbolId::new("iec.plug_pin");
        assert_eq!(p.next_designations(xd, &pin).get("1").map(String::as_str), Some("1"));
    }

    #[test]
    fn references_follow_the_sheet_grid() {
        let mut p = sample::with_contacts().unwrap();
        let k1 = p.function(K1_COIL).unwrap().clone();
        let col = p.sheet.column_of(k1.position);
        assert_eq!(p.reference(K1_COIL).unwrap(), format!("/1.{col}"));
        let sheet = Sheet { columns: 8, first_column: 0, rows: 6, ..p.sheet };
        let undo = p.apply(Command::SetSheet(sheet)).unwrap();
        let at = p.sheet.address(k1.position);
        assert!(at.ends_with(|c: char| c.is_ascii_uppercase()), "{at}");
        assert_eq!(p.reference(K1_COIL).unwrap(), format!("1/{at}"));
        p.apply(undo).unwrap();
        assert_eq!(p.sheet.rows, 0);
        assert_eq!(p.apply(Command::SetSheet(Sheet { columns: 0, ..p.sheet })).unwrap_err(), ModelError::InvalidSheet);
        let info = ProjectInfo { customer: "Example".into(), ..ProjectInfo::default() };
        assert_eq!(
            p.apply(Command::SetProjectInfo { name: " ".into(), info: info.clone() }).unwrap_err(),
            ModelError::EmptyProjectName
        );
        let undo = p.apply(Command::SetProjectInfo { name: "Panel A".into(), info }).unwrap();
        assert_eq!((p.name.as_str(), p.info.customer.as_str()), ("Panel A", "Example"));
        p.apply(undo).unwrap();
        assert_eq!(p.info, ProjectInfo::default());
    }

    #[test]
    fn plc_channels_are_numbered_per_card_and_addresses_follow_on() {
        let mut p = sample::with_plc().unwrap();
        let channels = |p: &Project| {
            p.plc_channels_of(sample::KF1)
                .iter()
                .map(|c| format!("{} {} {}", c.channel, c.kind.short(), c.address))
                .collect::<Vec<_>>()
        };
        assert_eq!(channels(&p), ["0 DI I0.0", "1 DI I0.1", "2 DO Q0.0", "3 AI IW256"]);
        assert_eq!(p.next_address(IoKind::DigitalIn), "I0.2");
        assert_eq!(p.next_address(IoKind::DigitalOut), "Q0.1");
        assert_eq!(p.next_address(IoKind::AnalogIn), "IW258");
        assert_eq!(p.next_address(IoKind::AnalogOut), "", "no analog output yet: no guess");
        assert_eq!(
            p.next_designations(sample::KF1, &SymbolId::new("iec.plc_do")).get("1").map(String::as_str),
            Some("4")
        );

        // Bits wrap at 8: after I0.7 comes I1.0.
        let set = |p: &mut Project, f, key: &str, value: &str| {
            let mut properties = p.function(f).unwrap().properties.clone();
            properties.insert(key.into(), value.into());
            p.apply(Command::SetProperties { id: f, properties })
        };
        set(&mut p, sample::KF1_DI0, ADDRESS, "I0.7").unwrap();
        assert_eq!(p.next_address(IoKind::DigitalIn), "I1.0");

        // Addresses and symbols are unique; a refused change leaves the channel as it was.
        assert_eq!(
            set(&mut p, sample::KF1_DI0, ADDRESS, "I0.1").unwrap_err(),
            ModelError::DuplicateAddress("I0.1".into(), "-KF1:1".into())
        );
        assert_eq!(
            set(&mut p, sample::KF1_DI0, SYMBOL, "Pump_On").unwrap_err(),
            ModelError::DuplicatePlcSymbol("Pump_On".into(), "-KF1:2".into())
        );
        assert_eq!(p.function(sample::KF1_DI0).unwrap().property(ADDRESS), "I0.7");

        // Blank values are dropped, and the change undoes.
        let undo = set(&mut p, sample::KF1_DI0, COMMENT, "  ").unwrap();
        assert_eq!(p.function(sample::KF1_DI0).unwrap().property(COMMENT), "");
        p.apply(undo).unwrap();
        assert_eq!(p.function(sample::KF1_DI0).unwrap().property(COMMENT), "Guard door closed");

        // Channel numbers are unique on the card.
        let one: std::collections::BTreeMap<String, String> = [("1".into(), "1".into())].into();
        assert_eq!(
            p.apply(Command::SetDesignations { id: sample::KF1_DO0, designations: one }).unwrap_err(),
            ModelError::DuplicateNumber("-KF1".into(), "1".into())
        );
    }

    #[test]
    fn devices_in_use_cannot_be_removed_and_tags_are_numbered() {
        let mut p = start_circuit().unwrap();
        let k1 = DeviceId::from_u128(0x103);
        assert_eq!(p.apply(Command::RemoveDevice(k1)).unwrap_err(), ModelError::DeviceInUse("-K1".into()));
        assert_eq!(p.next_tag("-K"), "-K2");
        assert_eq!(p.next_tag("-Q"), "-Q1");
        assert_eq!(p.apply(Command::RenameDevice { id: k1, tag: " ".into() }).unwrap_err(), ModelError::EmptyTag);
    }
}
