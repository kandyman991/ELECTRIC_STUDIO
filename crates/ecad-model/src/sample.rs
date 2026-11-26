//! Small built-in projects, on the 4 mm grid, used by tests, golden files and `ecad demo`.

use crate::geometry::{Point, Rotation};
use crate::parts::{Part, PartRef};
use crate::project::*;
use crate::symbol::*;

/// The symbols the samples use: the built-in library.
pub fn symbols() -> Vec<SymbolDef> {
    crate::library::iec()
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
        description: "Start circuit".into(),
        subtitle: String::new(),
        order: 0,
    });

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
            properties: Default::default(),
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
            spec: Default::default(),
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
        code: String::new(),
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
    p.apply(Command::AddPage(Page {
        id: PAGE_2,
        name: "2".into(),
        description: "Contacts".into(),
        subtitle: String::new(),
        order: 1,
    }))?;
    let k1 = DeviceId::from_u128(0x103);
    // Placed relative to the coil, so both pages share the sheet's columns sensibly.
    let coil = p.function(K1_COIL)?.position;
    for (id, symbol, dx) in [(K1_NO, "iec.contact_no", 40.0), (K1_NC, "iec.contact_nc", 120.0)] {
        let symbol = SymbolId::new(symbol);
        let designations = p.next_designations(k1, &symbol);
        p.place(Function {
            id,
            device: k1,
            symbol,
            page: PAGE_2,
            position: Point::new(coil.x + dx, coil.y),
            rotation: Rotation::R0,
            designations,
            properties: Default::default(),
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
            properties: Default::default(),
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
            spec: Default::default(),
        })?;
    }
    Ok(p)
}

pub const X2_1: FunctionId = FunctionId::from_u128(0x20c);
pub const X2_2: FunctionId = FunctionId::from_u128(0x20d);

/// [`with_potentials`] with the field pushbutton -S1 wired through terminal strip -X2, as a
/// cabinet would be: L+ → -X2:1 → -S1:13, -S1:14 → -X2:2 → -K1:A1. Each terminal uses one
/// feed-through terminal from the catalogue.
pub fn with_terminals() -> Result<Project> {
    let mut p = with_potentials()?;
    let strip = DeviceId::from_u128(0x10a);
    p.apply(Command::AddPart(Part {
        number: "EX-TB-4".into(),
        manufacturer: "Example".into(),
        description: "Feed-through terminal 4 mm²".into(),
        code: String::new(),
    }))?;
    p.add_device(Device { parts: vec![PartRef::new("EX-TB-4", 1)], ..Device::new(strip, "-X2", "Field connections") })?;
    let symbol = SymbolId::new("iec.terminal");
    for (id, y) in [(X2_1, 56.0), (X2_2, 26.0)] {
        let designations = p.next_designations(strip, &symbol);
        p.place(Function {
            id,
            device: strip,
            symbol: symbol.clone(),
            page: PAGE_1,
            position: Point::new(0.0, y),
            rotation: Rotation::R0,
            designations,
            properties: Default::default(),
        })?;
    }
    p.apply(Command::Disconnect(ConnectionId::from_u128(0x301)))?;
    p.apply(Command::Disconnect(ConnectionId::from_u128(0x302)))?;
    let wires = [
        (0x301, "W1", (0x201, "1"), (0x20c, "1")),
        (0x309, "W9", (0x20c, "2"), (0x202, "13")),
        (0x302, "W2", (0x202, "14"), (0x20d, "1")),
        (0x30a, "W10", (0x20d, "2"), (0x203, "A1")),
    ];
    for (id, number, (fa, pa), (fb, pb)) in wires {
        p.connect(Connection {
            id: ConnectionId::from_u128(id),
            from: ConnectionRef::new(FunctionId::from_u128(fa), pa),
            to: ConnectionRef::new(FunctionId::from_u128(fb), pb),
            wire_number: Some(number.into()),
            spec: Default::default(),
        })?;
    }
    Ok(p)
}

pub const KF1: DeviceId = DeviceId::from_u128(0x10b);
pub const KF1_DI0: FunctionId = FunctionId::from_u128(0x20e);
pub const KF1_DO0: FunctionId = FunctionId::from_u128(0x210);

/// [`with_terminals`] plus PLC card -KF1 on a new page 3: a field sensor -B1 (drawn as an NO
/// contact) wired from L+ to digital input -KF1:0, a second input -KF1:1 from L+ through -S1's
/// spare NC contact, digital output -KF1:2 driving coil -K3 to M, and an analog input -KF1:3 left
/// unwired. Addresses follow automatically (I0.0, I0.1, Q0.0); the analog channel gets IW256
/// by hand, and the two digital inputs carry symbols and comments.
pub fn with_plc() -> Result<Project> {
    let mut p = with_terminals()?;
    let page3 = PageId::from_u128(0x3);
    p.apply(Command::AddPage(Page {
        id: page3,
        name: "3".into(),
        description: "PLC inputs and outputs".into(),
        subtitle: "Ingressi e uscite PLC".into(),
        order: 2,
    }))?;
    // The style of the reference project: columns 0-7, rows A-F, a full title block.
    p.apply(Command::SetSheet(crate::Sheet { columns: 8, first_column: 0, rows: 6, ..p.sheet }))?;
    let info = ProjectInfo {
        customer: "Example Foods".into(),
        plant: "Cooling spiral".into(),
        code: "EX-0001".into(),
        designer: "A.B.".into(),
        date: "2026-11-26".into(),
        revisions: vec![
            Revision { index: "0".into(), change: "First issue".into(), date: "2026-11-11".into(), by: "A.B.".into() },
            Revision {
                index: "1".into(),
                change: "After testing".into(),
                date: "2026-11-26".into(),
                by: "A.B.".into(),
            },
        ],
    };
    p.apply(Command::SetProjectInfo { name: "Sample: PLC I/O".into(), info })?;
    p.add_device(Device::new(KF1, "-KF1", "PLC I/O card"))?;
    p.add_device(Device::new(DeviceId::from_u128(0x10c), "-B1", "Guard door closed"))?;
    p.add_device(Device::new(DeviceId::from_u128(0x10d), "-K3", "Pump contactor"))?;
    let fid = FunctionId::from_u128;
    let mut add = |dev: DeviceId, sym: &str, f: FunctionId, (x, y): (f64, f64), rotation| -> Result<()> {
        let symbol = SymbolId::new(sym);
        let designations = p.next_designations(dev, &symbol);
        let properties = p.new_drawing_properties(&symbol);
        p.place(Function {
            id: f,
            device: dev,
            symbol,
            page: page3,
            position: Point::new(x, y),
            rotation,
            designations,
            properties,
        })?;
        Ok(())
    };
    add(DeviceId::from_u128(0x101), "iec.supply", fid(0x211), (-72.0, 56.0), Rotation::R0)?;
    add(DeviceId::from_u128(0x10c), "iec.contact_no", fid(0x212), (-72.0, 36.0), Rotation::R0)?;
    add(KF1, "iec.plc_di", KF1_DI0, (-72.0, 14.0), Rotation::R0)?;
    add(DeviceId::from_u128(0x101), "iec.supply", fid(0x217), (-24.0, 56.0), Rotation::R0)?;
    add(DeviceId::from_u128(0x102), "iec.contact_nc", fid(0x213), (-24.0, 36.0), Rotation::R0)?;
    add(KF1, "iec.plc_di", fid(0x20f), (-24.0, 14.0), Rotation::R0)?;
    add(KF1, "iec.plc_do", KF1_DO0, (24.0, 14.0), Rotation::R0)?;
    add(DeviceId::from_u128(0x10d), "iec.coil", fid(0x214), (24.0, -16.0), Rotation::R0)?;
    add(DeviceId::from_u128(0x104), "iec.supply", fid(0x215), (24.0, -40.0), Rotation::R180)?;
    add(KF1, "iec.plc_ai", fid(0x216), (72.0, 14.0), Rotation::R0)?;
    let props =
        |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect::<Vec<_>>();
    let mut set = |f: FunctionId, pairs: &[(&str, &str)]| {
        let mut properties: std::collections::BTreeMap<String, String> = p.function(f)?.properties.clone();
        properties.extend(props(pairs));
        p.apply(Command::SetProperties { id: f, properties }).map(drop)
    };
    set(KF1_DI0, &[(SYMBOL, "Door_Closed"), (COMMENT, "Guard door closed")])?;
    set(fid(0x20f), &[(SYMBOL, "Start_Spare"), (COMMENT, "Start button, NC contact")])?;
    set(KF1_DO0, &[(SYMBOL, "Pump_On")])?;
    set(fid(0x216), &[(ADDRESS, "IW256"), (SYMBOL, "Tank_Level"), (COMMENT, "Tank level 4-20 mA")])?;
    let wires = [
        (0x30b, "W11", (0x211, "1"), (0x212, "13")),
        (0x30c, "W12", (0x212, "14"), (0x20e, "1")),
        (0x30d, "W13", (0x217, "1"), (0x213, "11")),
        (0x30e, "W14", (0x213, "12"), (0x20f, "1")),
        (0x30f, "W15", (0x210, "1"), (0x214, "A1")),
        (0x310, "W16", (0x214, "A2"), (0x215, "1")),
    ];
    for (id, number, (fa, pa), (fb, pb)) in wires {
        p.connect(Connection {
            id: ConnectionId::from_u128(id),
            from: ConnectionRef::new(fid(fa), pa),
            to: ConnectionRef::new(fid(fb), pb),
            wire_number: Some(number.into()),
            spec: Default::default(),
        })?;
    }
    Ok(p)
}

pub const PAGE_4: PageId = PageId::from_u128(0x4);
pub const QF1: DeviceId = DeviceId::from_u128(0x111);

/// [`with_plc`] plus a power page 4: L1-L3 feed switch-disconnector -QS1, motor-protection
/// breaker -QF1 and -K1's main contacts; a second feed with N goes through residual-current
/// breaker -QF2. -QF1's auxiliary contact 13-14 is drawn on page 3, so -QF1 has a contact
/// table and the contact points back.
pub fn with_power() -> Result<Project> {
    let mut p = with_plc()?;
    p.apply(Command::AddPage(Page {
        id: PAGE_4,
        name: "4".into(),
        description: "Power distribution".into(),
        subtitle: "Distribuzione potenza".into(),
        order: 3,
    }))?;
    let fid = FunctionId::from_u128;
    let device = |p: &mut Project, id: u128, tag: &str, desc: &str| -> Result<DeviceId> {
        let d = DeviceId::from_u128(id);
        if p.device(d).is_err() {
            p.add_device(Device::new(d, tag, desc))?;
        }
        Ok(d)
    };
    let place = |p: &mut Project, dev: DeviceId, sym: &str, f: u128, page: PageId, (x, y): (f64, f64)| -> Result<()> {
        let symbol = SymbolId::new(sym);
        let designations = p.next_designations(dev, &symbol);
        p.place(Function {
            id: fid(f),
            device: dev,
            symbol,
            page,
            position: Point::new(x, y),
            rotation: Rotation::R0,
            designations,
            properties: Default::default(),
        })
        .map(drop)
    };
    let l1 = device(&mut p, 0x10e, "L1", "400 V")?;
    let l2 = device(&mut p, 0x10f, "L2", "400 V")?;
    let l3 = device(&mut p, 0x110, "L3", "400 V")?;
    let n = device(&mut p, 0x114, "N", "Neutral")?;
    let qs1 = device(&mut p, 0x112, "-QS1", "Main switch")?;
    let qf1 = device(&mut p, 0x111, "-QF1", "Motor protection")?;
    let qf2 = device(&mut p, 0x113, "-QF2", "Heating cables")?;
    let k1 = DeviceId::from_u128(0x103);
    // Feed 1: L1-L3 → -QS1 → -QF1 → -K1 main contacts.
    for (i, (dev, f)) in [(l1, 0x220), (l2, 0x221), (l3, 0x222)].into_iter().enumerate() {
        // Stepped down, so each name stays clear of the next phase's wire.
        place(&mut p, dev, "iec.supply", f, PAGE_4, (-100.0 + crate::library::POLE * i as f64, 72.0 - 4.0 * i as f64))?;
    }
    place(&mut p, qs1, "iec.disconnector_fuse_3", 0x223, PAGE_4, (-100.0, 44.0))?;
    place(&mut p, qf1, "iec.motor_protection_3", 0x224, PAGE_4, (-100.0, 16.0))?;
    place(&mut p, k1, "iec.contact_main_3", 0x225, PAGE_4, (-100.0, -12.0))?;
    // Feed 2: L1-L3 and N → -QF2.
    for (i, (dev, f)) in [(l1, 0x226), (l2, 0x227), (l3, 0x228), (n, 0x229)].into_iter().enumerate() {
        place(&mut p, dev, "iec.supply", f, PAGE_4, (-20.0 + crate::library::POLE * i as f64, 72.0 - 4.0 * i as f64))?;
    }
    place(&mut p, qf2, "iec.breaker_3pn_rcd", 0x22a, PAGE_4, (-20.0, 44.0))?;
    // -QF1's auxiliary contact, on the PLC page.
    place(&mut p, qf1, "iec.contact_no", 0x22b, PageId::from_u128(0x3), (120.0, 36.0))?;

    let mut wires: Vec<(u128, u128, &str, u128, &str)> = Vec::new();
    for (i, (top, bottom)) in [("1", "2"), ("3", "4"), ("5", "6")].into_iter().enumerate() {
        let i = i as u128;
        wires.push((0x320 + i, 0x220 + i, "1", 0x223, top));
        wires.push((0x323 + i, 0x223, bottom, 0x224, top));
        wires.push((0x326 + i, 0x224, bottom, 0x225, top));
        wires.push((0x329 + i, 0x226 + i, "1", 0x22a, top));
    }
    wires.push((0x32c, 0x229, "1", 0x22a, "N"));
    for (id, fa, pa, fb, pb) in wires {
        p.connect(Connection {
            id: ConnectionId::from_u128(id),
            from: ConnectionRef::new(fid(fa), pa),
            to: ConnectionRef::new(fid(fb), pb),
            wire_number: None,
            spec: Default::default(),
        })?;
    }
    // A motor on -K1's main contacts, through cable -WM1; the feed and motor wires carry their
    // cross-section and colour.
    let m1 = device(&mut p, 0x115, "-M1", "Conveyor motor")?;
    place(&mut p, m1, "iec.motor_3", 0x22c, PAGE_4, (-92.0, -48.0))?;
    let wm1 = CableId::from_u128(0x500);
    p.apply(Command::AddCable(Cable { id: wm1, tag: "-WM1".into(), kind: "4G2,5".into(), cores: 4, shielded: true }))?;
    for (i, (contact, motor)) in [("2", "U"), ("4", "V"), ("6", "W")].into_iter().enumerate() {
        let id = ConnectionId::from_u128(0x330 + i as u128);
        p.connect(Connection {
            id,
            from: ConnectionRef::new(fid(0x225), contact),
            to: ConnectionRef::new(fid(0x22c), motor),
            wire_number: None,
            spec: WireSpec {
                cross_section: "2,5".into(),
                colour: "BK".into(),
                cable: Some(CableCore { cable: wm1, core: (i + 1).to_string() }),
            },
        })?;
    }
    for i in 0..3u128 {
        let spec = WireSpec { cross_section: "16".into(), colour: "BK".into(), cable: None };
        p.apply(Command::SetWireSpec { id: ConnectionId::from_u128(0x320 + i), spec })?;
    }
    // Number every wire as the reference project does: one number per potential, named
    // potentials by their name, the others by page (101, 401…).
    p.apply(Command::SetNaming(crate::Naming::by_page()))?;
    p.apply(Command::Batch(p.renumber_all_wires()))?;
    Ok(p)
}

/// Every library symbol placed once, category by category, one page per few categories: for
/// reviewing the library and as a golden file.
pub fn library_overview() -> Result<Project> {
    let mut p = Project::new("Symbol library");
    for s in symbols() {
        p.add_symbol(s);
    }
    p.sheet = crate::Sheet { columns: 8, first_column: 0, rows: 6, ..crate::Sheet::default() };
    let (lo, hi) = p.sheet.frame();
    let mut categories: Vec<String> = p.symbols.values().map(|s| s.category.clone()).collect();
    categories.sort();
    categories.dedup();
    let mut page_no = 0u32;
    let mut x: f64;
    let mut y = f64::MIN;
    // Fixed ids, so the output is the same every time (golden files).
    let mut page = PageId::from_u128(0);
    let mut next_id = 0x1000u128;
    let new_page = |p: &mut Project, n: u32, title: &str| -> Result<PageId> {
        let id = PageId::from_u128(u128::from(n));
        p.apply(Command::AddPage(Page {
            id,
            name: n.to_string(),
            description: title.into(),
            subtitle: String::new(),
            order: n,
        }))?;
        Ok(id)
    };
    // 48 mm cells, 40 mm rows, from the top-left of the drawing area.
    let (cell_w, cell_h) = (48.0, 40.0);
    let left = lo.x + 20.0;
    let top = hi.y - crate::sheet::COLUMN_BAND - 24.0;
    let bottom = lo.y + crate::sheet::TITLE_BLOCK_HEIGHT + 20.0;
    for cat in &categories {
        let ids: Vec<SymbolId> = p.symbols.values().filter(|s| &s.category == cat).map(|s| s.id.clone()).collect();
        // Each category starts on a new row.
        x = f64::MAX;
        for id in ids {
            if x + cell_w > hi.x - 10.0 {
                x = left;
                y -= cell_h;
            }
            if y < bottom || page_no == 0 {
                page_no += 1;
                page = new_page(&mut p, page_no, &format!("Symbol library from {cat}"))?;
                y = top;
                x = left;
            }
            let tag = p.new_tag(&id, page);
            next_id += 1;
            let device = DeviceId::from_u128(next_id);
            p.add_device(Device::new(device, &tag, ""))?;
            let designations = p.next_designations(device, &id);
            p.place(Function {
                id: FunctionId::from_u128(next_id),
                device,
                symbol: id.clone(),
                page,
                position: Point::new(x, y),
                rotation: Rotation::R0,
                designations,
                properties: Default::default(),
            })?;
            x += cell_w;
        }
    }
    Ok(p)
}
