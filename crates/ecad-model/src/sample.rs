//! A small built-in project used by tests, golden files and `ecad demo`.
//! The three symbols here are placeholders until the IEC 60617 library lands.

use crate::geometry::{Direction, Point, Rotation};
use crate::project::*;
use crate::symbol::*;

fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Graphic {
    Graphic::Line {
        a: Point::new(ax, ay),
        b: Point::new(bx, by),
    }
}

fn cp(name: &str, x: f64, y: f64, leaves: Direction) -> ConnectionPointDef {
    ConnectionPointDef {
        name: name.to_owned(),
        at: Point::new(x, y),
        leaves,
    }
}

pub fn symbols() -> Vec<SymbolDef> {
    vec![
        SymbolDef {
            id: SymbolId::new("iec.coil"),
            description: "Relay or contactor coil".into(),
            graphics: vec![
                line(-5.0, -3.0, 5.0, -3.0),
                line(5.0, -3.0, 5.0, 3.0),
                line(5.0, 3.0, -5.0, 3.0),
                line(-5.0, 3.0, -5.0, -3.0),
                line(0.0, 3.0, 0.0, 8.0),
                line(0.0, -3.0, 0.0, -8.0),
            ],
            connection_points: vec![
                cp("A1", 0.0, 8.0, Direction::Up),
                cp("A2", 0.0, -8.0, Direction::Down),
            ],
        },
        SymbolDef {
            id: SymbolId::new("iec.contact_no"),
            description: "Normally open contact".into(),
            graphics: vec![
                line(0.0, 8.0, 0.0, 3.0),
                line(0.0, 3.0, -3.0, -3.0),
                line(0.0, -3.0, 0.0, -8.0),
                line(-6.0, 0.0, -2.0, 0.0),
            ],
            connection_points: vec![
                cp("13", 0.0, 8.0, Direction::Up),
                cp("14", 0.0, -8.0, Direction::Down),
            ],
        },
        SymbolDef {
            id: SymbolId::new("iec.supply"),
            description: "Supply connection".into(),
            graphics: vec![line(-2.0, 0.0, 2.0, 0.0)],
            connection_points: vec![cp("1", 0.0, 0.0, Direction::Down)],
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
    p.add_page(Page {
        id: PAGE_1,
        name: "1".into(),
    });

    let devices = [
        (
            0x101,
            "-X1:L+",
            "24 V DC supply",
            "iec.supply",
            0x201,
            (0.0, 60.0),
            Rotation::R0,
        ),
        (
            0x102,
            "-S1",
            "Start",
            "iec.contact_no",
            0x202,
            (0.0, 40.0),
            Rotation::R0,
        ),
        (
            0x103,
            "-K1",
            "Motor contactor",
            "iec.coil",
            0x203,
            (0.0, 10.0),
            Rotation::R0,
        ),
        (
            0x104,
            "-X1:M",
            "0 V",
            "iec.supply",
            0x204,
            (0.0, -10.0),
            Rotation::R180,
        ),
    ];
    for (dev, tag, desc, sym, fun, (x, y), rotation) in devices {
        let device = p.add_device(Device {
            id: DeviceId::from_u128(dev),
            tag: tag.into(),
            description: desc.into(),
        })?;
        p.place(Function {
            id: FunctionId::from_u128(fun),
            device,
            symbol: SymbolId::new(sym),
            page: PAGE_1,
            position: Point::new(x, y),
            rotation,
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
    Ok(p)
}
