//! A small built-in project, on the 4 mm grid, used by tests, golden files and `ecad demo`.
//! The three symbols here are placeholders until the IEC 60617 library lands.

use crate::geometry::{Direction, Point, Rotation};
use crate::parts::{Part, PartRef};
use crate::project::*;
use crate::symbol::*;

fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Graphic {
    Graphic::Line { a: Point::new(ax, ay), b: Point::new(bx, by) }
}

fn cp(name: &str, x: f64, y: f64, leaves: Direction) -> ConnectionPointDef {
    ConnectionPointDef { name: name.to_owned(), at: Point::new(x, y), leaves }
}

pub fn symbols() -> Vec<SymbolDef> {
    vec![
        SymbolDef {
            id: SymbolId::new("iec.coil"),
            description: "Relay or contactor coil".into(),
            tag_prefix: "-K".into(),
            role: Role::Main,
            bridged: false,
            hide_tag: false,
            hide_point_labels: false,
            graphics: vec![
                line(-5.0, -3.0, 5.0, -3.0),
                line(5.0, -3.0, 5.0, 3.0),
                line(5.0, 3.0, -5.0, 3.0),
                line(-5.0, 3.0, -5.0, -3.0),
                line(0.0, 3.0, 0.0, 8.0),
                line(0.0, -3.0, 0.0, -8.0),
            ],
            connection_points: vec![cp("A1", 0.0, 8.0, Direction::Up), cp("A2", 0.0, -8.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.contact_no"),
            description: "Normally open contact".into(),
            tag_prefix: "-S".into(),
            role: Role::Contact { kind: ContactKind::NormallyOpen },
            bridged: false,
            hide_tag: false,
            hide_point_labels: false,
            graphics: vec![
                line(0.0, 8.0, 0.0, 3.0),
                line(0.0, 3.0, -3.0, -3.0),
                line(0.0, -3.0, 0.0, -8.0),
                line(-6.0, 0.0, -2.0, 0.0),
            ],
            connection_points: vec![cp("13", 0.0, 8.0, Direction::Up), cp("14", 0.0, -8.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.contact_nc"),
            description: "Normally closed contact".into(),
            tag_prefix: "-S".into(),
            role: Role::Contact { kind: ContactKind::NormallyClosed },
            bridged: false,
            hide_tag: false,
            hide_point_labels: false,
            graphics: vec![
                line(0.0, 8.0, 0.0, 3.0),
                line(0.0, 3.0, 2.5, 3.0),
                line(0.0, -3.0, 3.5, 4.0),
                line(0.0, -3.0, 0.0, -8.0),
                line(-6.0, 0.0, 1.0, 0.0),
            ],
            connection_points: vec![cp("11", 0.0, 8.0, Direction::Up), cp("12", 0.0, -8.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.supply"),
            description: "Potential point".into(),
            tag_prefix: "P".into(),
            role: Role::Potential,
            bridged: false,
            hide_tag: false,
            hide_point_labels: true,
            graphics: vec![line(-2.0, 0.0, 2.0, 0.0)],
            connection_points: vec![cp("1", 0.0, 0.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.junction"),
            description: "Junction".into(),
            tag_prefix: "J".into(),
            role: Role::Single,
            bridged: true,
            hide_tag: true,
            hide_point_labels: true,
            graphics: vec![
                // Concentric rings read as a solid dot at drawing scale.
                Graphic::Circle { center: Point::ZERO, radius: 0.2 },
                Graphic::Circle { center: Point::ZERO, radius: 0.4 },
                Graphic::Circle { center: Point::ZERO, radius: 0.6 },
                Graphic::Circle { center: Point::ZERO, radius: 0.8 },
            ],
            connection_points: vec![
                cp("1", 0.0, 0.0, Direction::Up),
                cp("2", 0.0, 0.0, Direction::Down),
                cp("3", 0.0, 0.0, Direction::Left),
                cp("4", 0.0, 0.0, Direction::Right),
            ],
        },
        SymbolDef {
            id: SymbolId::new("iec.interruption"),
            description: "Interruption point".into(),
            tag_prefix: "I".into(),
            role: Role::Interruption,
            bridged: false,
            hide_tag: false,
            hide_point_labels: true,
            graphics: vec![
                line(0.0, 0.0, 0.0, -2.0),
                line(-2.0, -2.0, 2.0, -2.0),
                line(2.0, -2.0, 0.0, -5.0),
                line(0.0, -5.0, -2.0, -2.0),
            ],
            connection_points: vec![cp("1", 0.0, 0.0, Direction::Up)],
        },
    ]
}

pub const PAGE_1: PageId = PageId::from_u128(0x1);
pub const K1_COIL: FunctionId = FunctionId::from_u128(0x203);

/// L+ → pushbutton -S1 → coil -K1 → M, on one page.
pub fn start_circuit() -> Result<Project> {
    let mut p = Project::new("Sample: start circuit");
    for s in symbols() {
        p.add_symbol(s);
    }
    p.add_page(Page { id: PAGE_1, name: "1".into(), description: "Start circuit".into(), order: 0 });

    let devices = [
        (0x101, "L+", "24 V DC", "iec.supply", 0x201, (0.0, 64.0), Rotation::R0),
        (0x102, "-S1", "Start", "iec.contact_no", 0x202, (0.0, 40.0), Rotation::R0),
        (0x103, "-K1", "Motor contactor", "iec.coil", 0x203, (0.0, 12.0), Rotation::R0),
        (0x104, "M", "0 V", "iec.supply", 0x204, (0.0, -12.0), Rotation::R180),
    ];
    for (dev, tag, desc, sym, fun, (x, y), rotation) in devices {
        let device = p.add_device(Device::new(DeviceId::from_u128(dev), tag, desc))?;
        p.place(Function {
            id: FunctionId::from_u128(fun),
            device,
            symbol: SymbolId::new(sym),
            page: PAGE_1,
            position: Point::new(x, y),
            rotation,
            designations: Default::default(),
        })?;
    }

    let wires = [
        (0x301, "W1", (0x201, "1"), (0x202, "13")),
        (0x302, "W2", (0x202, "14"), (0x203, "A1")),
        (0x303, "W3", (0x203, "A2"), (0x204, "1")),
    ];
    for (id, number, (fa, pa), (fb, pb)) in wires {
        p.connect(Connection {
            id: ConnectionId::from_u128(id),
            from: ConnectionRef::new(FunctionId::from_u128(fa), pa),
            to: ConnectionRef::new(FunctionId::from_u128(fb), pb),
            wire_number: Some(number.into()),
        })?;
    }
    if let Some((min, max)) = p.content_bounds(PAGE_1) {
        p.sheet = crate::Sheet::around(min, max);
    }
    for part in catalogue() {
        p.apply(Command::AddPart(part))?;
    }
    p.apply(Command::SetDeviceParts { id: DeviceId::from_u128(0x103), parts: vec![PartRef::new("EX-CT-24DC", 1)] })?;
    p.apply(Command::SetDeviceParts { id: DeviceId::from_u128(0x102), parts: vec![PartRef::new("EX-PB-GRN", 1)] })?;
    Ok(p)
}

/// A small made-up catalogue for the samples (not real order numbers).
pub fn catalogue() -> Vec<Part> {
    let part = |number: &str, description: &str| Part {
        number: number.into(),
        manufacturer: "Example".into(),
        description: description.into(),
    };
    vec![
        part("EX-CT-24DC", "Contactor 3 kW, 24 V DC coil, 1 NO auxiliary"),
        part("EX-AUX-1NO1NC", "Auxiliary contact block 1 NO + 1 NC"),
        part("EX-PB-GRN", "Pushbutton, green, 1 NO"),
    ]
}

pub const PAGE_2: PageId = PageId::from_u128(0x2);
pub const K1_NO: FunctionId = FunctionId::from_u128(0x205);
pub const K1_NC: FunctionId = FunctionId::from_u128(0x206);

/// The start circuit plus page 2, "Contacts", holding a normally open and a normally closed
/// contact of -K1, so the coil on page 1 and the contacts on page 2 refer to each other.
pub fn with_contacts() -> Result<Project> {
    let mut p = start_circuit()?;
    p.apply(Command::AddPage(Page { id: PAGE_2, name: "2".into(), description: "Contacts".into(), order: 1 }))?;
    let k1 = DeviceId::from_u128(0x103);
    // Placed relative to the coil, so both pages share the sheet's columns sensibly.
    let coil = p.function(K1_COIL)?.position;
    for (id, symbol, dx) in [(K1_NO, "iec.contact_no", 40.0), (K1_NC, "iec.contact_nc", 120.0)] {
        let symbol = SymbolId::new(symbol);
        let designations = p.next_contact_designations(k1, &symbol);
        p.place(Function {
            id,
            device: k1,
            symbol,
            page: PAGE_2,
            position: Point::new(coil.x + dx, coil.y),
            rotation: Rotation::R0,
            designations,
        })?;
    }
    Ok(p)
}

pub const JUNCTION_1: FunctionId = FunctionId::from_u128(0x20b);

/// [`with_contacts`] wired up across both pages, to show potentials:
/// - page 2: L+ drawn again, feeding -K1's NO contact (W4); its other side (W5) ends in
///   interruption point I1;
/// - page 1: I1 continues (W6) into a second coil -K2, whose return (W7) meets -K1's at a
///   junction on the way to M (W3, W8).
pub fn with_potentials() -> Result<Project> {
    let mut p = with_contacts()?;
    let fid = FunctionId::from_u128;
    let add = |p: &mut Project, dev: u128, tag: &str, sym: &str, f: u128, page, (x, y), rotation| -> Result<()> {
        let device = DeviceId::from_u128(dev);
        if p.device(device).is_err() {
            p.add_device(Device::new(device, tag, ""))?;
        }
        p.place(Function {
            id: fid(f),
            device,
            symbol: SymbolId::new(sym),
            page,
            position: Point::new(x, y),
            rotation,
            designations: Default::default(),
        })?;
        Ok(())
    };
    add(&mut p, 0x101, "L+", "iec.supply", 0x207, PAGE_2, (40.0, 36.0), Rotation::R0)?;
    add(&mut p, 0x107, "I1", "iec.interruption", 0x208, PAGE_2, (40.0, -12.0), Rotation::R0)?;
    add(&mut p, 0x107, "I1", "iec.interruption", 0x209, PAGE_1, (40.0, 40.0), Rotation::R180)?;
    add(&mut p, 0x108, "-K2", "iec.coil", 0x20a, PAGE_1, (40.0, 12.0), Rotation::R0)?;
    add(&mut p, 0x109, "J1", "iec.junction", 0x20b, PAGE_1, (0.0, -8.0), Rotation::R0)?;

    // Re-route W3 through the junction.
    p.apply(Command::Disconnect(ConnectionId::from_u128(0x303)))?;
    let wires = [
        (0x303, "W3", (0x203, "A2"), (0x20b, "1")),
        (0x304, "W4", (0x207, "1"), (0x205, "13")),
        (0x305, "W5", (0x205, "14"), (0x208, "1")),
        (0x306, "W6", (0x209, "1"), (0x20a, "A1")),
        (0x307, "W7", (0x20a, "A2"), (0x20b, "4")),
        (0x308, "W8", (0x20b, "2"), (0x204, "1")),
    ];
    for (id, number, (fa, pa), (fb, pb)) in wires {
        p.connect(Connection {
            id: ConnectionId::from_u128(id),
            from: ConnectionRef::new(fid(fa), pa),
            to: ConnectionRef::new(fid(fb), pb),
            wire_number: Some(number.into()),
        })?;
    }
    Ok(p)
}
