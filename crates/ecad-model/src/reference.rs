//! The MVP reference project: ten pages of a real cooling-spiral control panel, redrawn with
//! neutral names (customer "Example Foods"). It exercises everything the MVP needs on a real
//! drawing: multi-pole power devices on busbars, a motor on an inverter, motor protection,
//! a 24 V supply, a two-channel emergency chain through connectors, a safety module, PLC
//! inputs and outputs with their addresses, cables and terminals, wire numbers per potential.
//!
//! Positions are in millimetres from the frame's bottom-left corner (the sheet's origin is set
//! so the frame starts at 0, 0); the sheet has columns 0-7 and rows A-F like the original.

use crate::geometry::{Point, Rotation};
use crate::library::{BlackBox, black_box};
use crate::naming::Naming;
use crate::panel::{Panel, PanelItem, PanelItemKind, RAIL_WIDTH};
use crate::parts::{Part, PartRef};
use crate::project::*;
use crate::sheet::Sheet;
use crate::symbol::SymbolId;
use std::collections::BTreeMap;

/// Builds the project page by page, refering to devices by tag and to points by designation.
struct Builder {
    p: Project,
    page: PageId,
    next: u128,
}

type F = FunctionId;

impl Builder {
    fn id(&mut self) -> u128 {
        self.next += 1;
        self.next
    }

    fn page(&mut self, name: &str, description: &str, subtitle: &str) -> Result<()> {
        let order = self.p.pages.len() as u32;
        let id = PageId::from_u128(0x100 + u128::from(order));
        self.p.apply(Command::AddPage(Page {
            id,
            name: name.into(),
            description: description.into(),
            subtitle: subtitle.into(),
            order,
        }))?;
        self.page = id;
        Ok(())
    }

    fn device(&mut self, tag: &str) -> Result<DeviceId> {
        if let Some(d) = self.p.device_by_tag(tag) {
            return Ok(d.id);
        }
        let id = DeviceId::from_u128(self.id());
        self.p.add_device(Device::new(id, tag, ""))?;
        Ok(id)
    }

    fn describe(&mut self, tag: &str, description: &str) -> Result<()> {
        let id = self.device(tag)?;
        self.p.apply(Command::SetDescription { id, description: description.into() }).map(drop)
    }

    /// A drawing of `tag` with `symbol` at (x, y), numbered as the model numbers it.
    fn put_r(&mut self, tag: &str, symbol: &str, x: f64, y: f64, rotation: Rotation) -> Result<F> {
        let device = self.device(tag)?;
        let symbol = SymbolId::new(symbol);
        let designations = self.p.next_designations(device, &symbol);
        let properties = self.p.new_drawing_properties(&symbol);
        self.place(device, symbol, x, y, rotation, designations, properties)
    }

    fn put(&mut self, tag: &str, symbol: &str, x: f64, y: f64) -> Result<F> {
        self.put_r(tag, symbol, x, y, Rotation::R0)
    }

    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        device: DeviceId,
        symbol: SymbolId,
        x: f64,
        y: f64,
        rotation: Rotation,
        designations: BTreeMap<String, String>,
        properties: BTreeMap<String, String>,
    ) -> Result<F> {
        let id = F::from_u128(self.id());
        self.p.place(Function {
            id,
            device,
            symbol,
            page: self.page,
            position: Point::new(x, y),
            rotation,
            designations,
            properties,
            mirrored: false,
        })?;
        Ok(id)
    }

    fn terminal(&mut self, strip: &str, symbol: &str, number: &str, x: f64, y: f64) -> Result<F> {
        let device = self.device(strip)?;
        let symbol = SymbolId::new(symbol);
        let designations = [("1".to_owned(), number.to_owned()), ("2".to_owned(), number.to_owned())].into();
        self.place(device, symbol, x, y, Rotation::R0, designations, BTreeMap::new())
    }

    /// A PLC channel with its address, symbol and comment.
    fn channel(&mut self, card: &str, symbol: &str, x: f64, y: f64, address: &str, name: &str) -> Result<F> {
        let f = self.put(card, symbol, x, y)?;
        let properties = [(ADDRESS.to_owned(), address.to_owned()), (SYMBOL.to_owned(), name.to_owned())]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .collect();
        self.p.apply(Command::SetProperties { id: f, properties })?;
        Ok(f)
    }

    fn junction(&mut self, x: f64, y: f64) -> Result<F> {
        let tag = format!("J{}", self.next + 1);
        self.put(&tag, "iec.junction", x, y)
    }

    /// An off-page arrow: `facing` is the direction its wire leaves in.
    fn arrow(&mut self, name: &str, x: f64, y: f64, facing: crate::Direction) -> Result<F> {
        let rotation = match facing {
            crate::Direction::Up => Rotation::R0,
            crate::Direction::Left => Rotation::R90,
            crate::Direction::Down => Rotation::R180,
            crate::Direction::Right => Rotation::R270,
        };
        self.put_r(name, "iec.interruption", x, y, rotation)
    }

    /// The connection point of `f` named or designated `d`.
    fn pin(&self, f: F, d: &str) -> Result<ConnectionRef> {
        let func = self.p.function(f)?;
        let sym = self.p.symbol(&func.symbol)?;
        // A point's own name first (a terminal's two sides share one number), then its
        // designation (a contact numbered 21-22 on a symbol whose points are 11 and 12).
        sym.connection_point(d)
            .or_else(|| sym.connection_points.iter().find(|cp| func.designation(&cp.name) == d))
            .map(|cp| ConnectionRef::new(f, &cp.name))
            .ok_or_else(|| ModelError::NoConnectionPoint { symbol: func.symbol.0.clone(), point: d.into() })
    }

    fn wire_spec(&mut self, a: (F, &str), b: (F, &str), number: Option<&str>, spec: WireSpec) -> Result<ConnectionId> {
        let id = ConnectionId::from_u128(self.id());
        let (from, to) = (self.pin(a.0, a.1)?, self.pin(b.0, b.1)?);
        self.p.connect(Connection { bend: None, id, from, to, wire_number: number.map(str::to_owned), spec })?;
        Ok(id)
    }

    /// Flips a drawing left to right.
    fn mirror(&mut self, f: F) -> Result<()> {
        self.p.apply(Command::Mirror { id: f, to: true }).map(drop)
    }

    /// Puts a wire's middle run at `at` (its y when horizontal, its x when vertical).
    fn bend(&mut self, id: ConnectionId, at: f64) -> Result<()> {
        self.p.apply(Command::SetWireBend { id, bend: Some(at) }).map(drop)
    }

    fn w(&mut self, a: (F, &str), b: (F, &str)) -> Result<ConnectionId> {
        self.wire_spec(a, b, None, WireSpec::default())
    }

    fn wn(&mut self, a: (F, &str), b: (F, &str), number: &str) -> Result<ConnectionId> {
        self.wire_spec(a, b, Some(number), WireSpec::default())
    }

    fn ws(
        &mut self,
        a: (F, &str),
        b: (F, &str),
        number: &str,
        cross_section: &str,
        colour: &str,
    ) -> Result<ConnectionId> {
        let spec = WireSpec { cross_section: cross_section.into(), colour: colour.into(), cable: None };
        self.wire_spec(a, b, Some(number), spec)
    }

    fn cable(&mut self, tag: &str, kind: &str, cores: u32, shielded: bool) -> Result<CableId> {
        let id = CableId::from_u128(self.id());
        self.p.apply(Command::AddCable(Cable { id, tag: tag.into(), kind: kind.into(), cores, shielded }))?;
        Ok(id)
    }

    fn core(&mut self, wire: ConnectionId, cable: CableId, core: &str, cross_section: &str) -> Result<()> {
        let spec = WireSpec {
            cross_section: cross_section.into(),
            colour: String::new(),
            cable: Some(CableCore { cable, core: core.into() }),
        };
        self.p.apply(Command::SetWireSpec { id: wire, spec }).map(drop)
    }

    fn boxed(&mut self, label: &str, min: (f64, f64), max: (f64, f64)) -> Result<()> {
        let id = BoxId::from_u128(self.id());
        let page = self.page;
        let b =
            LocationBox { id, page, min: Point::new(min.0, min.1), max: Point::new(max.0, max.1), label: label.into() };
        self.p.apply(Command::AddBox(b)).map(drop)
    }

    /// A black box device: its own symbol, then one drawing of it.
    fn black(&mut self, tag: &str, spec: BlackBox, x: f64, y: f64) -> Result<F> {
        let id = format!("box.{tag}");
        self.p.apply(Command::AddSymbol(black_box(&id, &spec)))?;
        let f = self.put(tag, &id, x, y)?;
        self.describe(tag, &spec.description)?;
        Ok(f)
    }

    /// Catalogue parts for a device: (number, quantity, manufacturer, description).
    fn parts(&mut self, tag: &str, parts: &[(&str, u32, &str, &str)]) -> Result<()> {
        self.parts_split(tag, parts, &[])
    }

    /// Parts for a terminal strip: the first list per terminal, the second once for the strip.
    fn parts_split(
        &mut self,
        tag: &str,
        parts: &[(&str, u32, &str, &str)],
        per_strip: &[(&str, u32, &str, &str)],
    ) -> Result<()> {
        for (number, _, manufacturer, description) in parts.iter().chain(per_strip) {
            if !self.p.parts.contains_key(*number) {
                self.p.apply(Command::AddPart(Part {
                    number: (*number).into(),
                    manufacturer: (*manufacturer).into(),
                    description: (*description).into(),
                    code: String::new(),
                    contacts: CONTACT_BLOCKS
                        .iter()
                        .find(|(n, _)| n == number)
                        .map(|(_, blocks)| blocks.iter().map(|b| (*b).to_owned()).collect())
                        .unwrap_or_default(),
                    width: size_of(number).map(|s| s.0),
                    height: size_of(number).map(|s| s.1),
                    depth: size_of(number).map(|s| s.2),
                }))?;
            }
        }
        let id = self.device(tag)?;
        let refs = parts
            .iter()
            .map(|(n, q, ..)| PartRef::new(n, *q))
            .chain(per_strip.iter().map(|(n, q, ..)| PartRef::per_strip(n, *q)))
            .collect();
        self.p.apply(Command::SetDeviceParts { id, parts: refs }).map(drop)
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

use crate::Direction::{Down, Left, Right, Up};

/// The auxiliary contacts printed on the parts used here, as the catalogue gives them.
const CONTACT_BLOCKS: &[(&str, &[&str])] = &[
    ("ZBE101", &["13-14"]),
    ("ZBE102", &["11-12"]),
    ("1122880000", &["11-12-14"]),
    ("3RT2016-2BB41", &["13-14"]),
    ("3RT2036-1NB30", &["13-14", "21-22"]),
    ("3RT2038-1NB30", &["13-14", "21-22"]),
];

/// Sizes (width along the rail, height, depth; mm) of the parts mounted in the cabinet, from
/// the makers' data sheets where known, otherwise typical for the kind (marked ~).
const PART_SIZES: &[(&str, f64, f64, f64)] = &[
    ("38316006", 128.0, 150.0, 120.0),   // ~ switch-disconnector 4P NH00
    ("563930", 108.0, 90.0, 60.0),       // ~ distribution block 4P
    ("5SU1344-1KK80", 72.0, 90.0, 70.0), // RCBO 3P+N, 4 modules
    ("5SU1356-1KK25", 36.0, 90.0, 70.0), // RCBO 1P+N, 2 modules
    ("5SU1356-1KK06", 36.0, 90.0, 70.0),
    ("09967", 45.0, 90.0, 60.0),                // ~ DIN-rail socket
    ("TMF", 17.5, 88.0, 46.0),                  // ~ cabinet thermostat
    ("2301137", 36.0, 81.0, 68.0),              // fuse holder 1P+N 10x38
    ("2303038", 54.0, 81.0, 68.0),              // fuse holder 3P 10x38
    ("S8VK-C24024", 40.0, 125.0, 110.0),        // power supply 240 W
    ("6SL3210-1KE13-2AF1", 73.0, 196.0, 225.0), // inverter, frame size AA
    ("3RV2011-1HA10", 45.0, 97.0, 97.0),        // motor protection, size S00
    ("2906032", 45.0, 99.0, 112.0),             // ~ electronic protection, 4 channels
    ("CS MP202M0", 45.0, 108.0, 114.0),         // ~ safety module
    ("3RT2016-2BB41", 45.0, 77.0, 98.0),        // contactor S00, spring terminals
    ("3RT2036-1NB30", 55.0, 114.0, 130.0),      // contactor S2
    ("3RT2038-1NB30", 55.0, 114.0, 130.0),
    ("1122880000", 6.4, 90.0, 87.0),            // relay module, 6.4 mm
    ("6ES7214-1AG40-0XB0", 110.0, 100.0, 75.0), // CPU 1214C
    ("6ES7223-1BL32-0XB0", 70.0, 100.0, 75.0),  // SM 1223, 32 I/O
    ("1608510000", 5.1, 60.0, 47.0),            // ZDU 2.5
    ("1608640000", 5.1, 60.0, 47.0),            // ZPE 2.5
    ("0383560000", 8.5, 46.0, 30.0),            // end bracket EW 35
    ("1608740000", 2.1, 60.0, 30.0),            // end plate ZAP/TW1
];

fn size_of(number: &str) -> Option<(f64, f64, f64)> {
    PART_SIZES.iter().find(|(n, ..)| *n == number).map(|(_, w, h, d)| (*w, *h, *d))
}

const SIEMENS: &str = "Siemens";
const SCHNEIDER: &str = "Schneider Electric";
const WEIDMULLER: &str = "Weidmuller";

/// The reference project.
pub fn reference_project() -> Result<Project> {
    let mut p = Project::new("Cooling spiral control panel");
    for s in crate::library::iec() {
        p.add_symbol(s);
    }
    p.sheet = Sheet { origin: Point::new(-20.0, -10.0), columns: 8, first_column: 0, rows: 6, ..Sheet::default() };
    p.naming = Naming::by_page();
    p.info = ProjectInfo {
        customer: "Example Foods".into(),
        plant: "Cooling spiral".into(),
        code: "EX-0001".into(),
        designer: "A.B.".into(),
        date: "2026-11-11".into(),
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
    let mut b = Builder { p, page: PageId::from_u128(0), next: 0x1000 };
    power_distribution(&mut b)?;
    power_branches(&mut b)?;
    spiral_motor(&mut b)?;
    fan_motors(&mut b)?;
    supply_24v(&mut b)?;
    emergency_chain(&mut b)?;
    safety_module(&mut b)?;
    inputs_cpu(&mut b)?;
    outputs_cpu(&mut b)?;
    outputs_card(&mut b)?;
    terminal_strips(&mut b)?;
    cabinet_layout(&mut b)?;
    // As the editor does for each new wire: move middle runs off other wires and symbols.
    let pages: Vec<PageId> = b.p.pages.keys().copied().collect();
    for page in pages {
        for cmd in b.p.tidy_wires(page) {
            b.p.apply(cmd)?;
        }
    }
    number_wires(&mut b.p)?;
    Ok(b.p)
}

/// L7: the mounting plate as on the original's inner-cabinet layout: 895 x 1870 mm, horizontal
/// ducts at the original's heights with ducts up both sides, a rail between each pair, the
/// inverter on the plate in the tall gap and the terminal strips along the bottom.
fn cabinet_layout(b: &mut Builder) -> Result<()> {
    const W: f64 = 895.0;
    const H: f64 = 1870.0;
    b.page("L7", "Inner cabinet layout", "Layout interno quadro")?;
    let page = b.page;
    b.p.apply(Command::SetPanel { page, panel: Some(Panel { width: W, height: H }) })?;
    let item = |b: &mut Builder, kind: PanelItemKind, x: f64, y: f64, length: f64, vertical: bool| {
        let id = PanelItemId::from_u128(b.id());
        b.p.apply(Command::AddPanelItem(PanelItem {
            id,
            page,
            kind,
            at: Point::new(x, y),
            length,
            vertical,
            part: String::new(),
        }))
        .map(drop)
    };
    let duct = |width: f64| PanelItemKind::Duct { width, depth: 80.0 };
    // Horizontal ducts, by their top's distance from the top of the plate (as dimensioned on
    // the original), and width; the top and bottom ones run the full width.
    let ducts = [
        (0.0, 40.0),
        (230.0, 60.0),
        (430.0, 60.0),
        (675.0, 40.0),
        (1035.0, 60.0),
        (1255.0, 60.0),
        (1445.0, 60.0),
        (1635.0, 80.0),
    ];
    let (left, right) = (60.0, 80.0);
    for (k, (from_top, width)) in ducts.iter().enumerate() {
        let full = k == 0 || k == ducts.len() - 1;
        let (x, length) = if full { (0.0, W) } else { (left, W - left - right) };
        item(b, duct(*width), x, H - from_top - width, length, false)?;
    }
    // Up both sides between the top and bottom ducts.
    let (low, high) = (H - 1635.0, H - 40.0);
    item(b, duct(left), 0.0, low, high - low, true)?;
    item(b, duct(right), W - right, low, high - low, true)?;
    // A rail in the middle of each gap between ducts, devices along it from the left; the
    // tall gap (675-1035) has the inverter and the main switch straight on the plate.
    let gaps: Vec<(f64, f64)> = ducts.windows(2).map(|p| (p[0].0 + p[0].1, p[1].0)).collect();
    let rows: [&[&str]; 7] = [
        &["P2QF1", "P2QF3", "P3Q1", "P3Q2", "P3FU4", "P3FU3", "M3FU1", "P3XS2", "P3S1"],
        &["P2WB1", "A1FE1", "P3GD1"],
        &["C1D1", "C1A3", "A3S1", "A3KA1", "A3KA2", "A3KM1", "A3KM2"],
        &[],
        &["A3KM4", "A3KM5", "O0KM1", "O0KA1"],
        &["M10QF1", "M10QF2", "O3KM1", "O3KM2", "O3KM3", "O3KM4", "O3KM5", "O3KM6", "O3KA0", "O3KA1", "O3KA5", "O3KA6"],
        &["X22", "X31", "X32", "X33", "X90"],
    ];
    let place = |b: &mut Builder, tag: &str, x: f64, centre: f64| -> Result<f64> {
        let device = b.device(tag)?;
        let (w, h) = b.p.device_size(device).0;
        item(b, PanelItemKind::Device { device }, x, centre - h / 2.0, 0.0, false)?;
        Ok(x + w + 5.0)
    };
    for ((top, bottom), tags) in gaps.iter().zip(rows) {
        let centre = H - (top + bottom) / 2.0;
        if tags.is_empty() {
            let mut x = left + 40.0;
            for tag in ["MU3", "P2QS1"] {
                x = place(b, tag, x, centre)? + 60.0;
            }
            continue;
        }
        item(b, PanelItemKind::Rail, left, centre - RAIL_WIDTH / 2.0, W - left - right, false)?;
        let mut x = left + 10.0;
        for tag in tags {
            x = place(b, tag, x, centre)?;
        }
    }
    // Under the bottom duct: the full-width rail with -X0.
    let centre = (H - 1635.0 - 80.0) / 2.0;
    item(b, PanelItemKind::Rail, 0.0, centre - RAIL_WIDTH / 2.0, W, false)?;
    place(b, "X0", 20.0, centre)?;
    Ok(())
}

/// Feed-through terminals per terminal; end brackets and end plates once per strip; on -X0 the
/// PE terminals (ZPE) per PE terminal.
fn terminal_strips(b: &mut Builder) -> Result<()> {
    let zdu = [("1608510000", 1, WEIDMULLER, "Feed-through terminal ZDU 2.5")];
    for (strip, plates) in [("X0", 1), ("X22", 1), ("X31", 4), ("X32", 4), ("X33", 4), ("X90", 1)] {
        b.parts_split(
            strip,
            &zdu,
            &[
                ("0383560000", 2, WEIDMULLER, "End bracket EW 35 for TS 35"),
                ("1608740000", plates, WEIDMULLER, "End plate / partition ZAP/TW1"),
            ],
        )?;
    }
    // -X0 mixes kinds: feed-through terminals count per normal terminal, ZPE per PE terminal.
    b.parts("X0", &[("1608640000", 1, WEIDMULLER, "Modular PE terminal ZPE 2.5")])?;
    let x0 = b.device("X0")?;
    let mut parts = vec![
        PartRef::per_terminal_of("1608510000", 1, "iec.terminal"),
        PartRef::per_terminal_of("1608640000", 1, "iec.terminal_pe"),
        PartRef::per_strip("0383560000", 2),
        PartRef::per_strip("1608740000", 1),
    ];
    parts.retain(|r| b.p.parts.contains_key(&r.number));
    b.p.apply(Command::SetDeviceParts { id: x0, parts }).map(drop)
}

/// Every potential without a number gets one, as drawing it in the editor would: its name, or
/// the number one of its wires already has, or the page's next free number.
fn number_wires(p: &mut Project) -> Result<()> {
    for pot in p.potentials() {
        if pot.connections.is_empty() {
            continue;
        }
        let existing = pot.connections.iter().find_map(|c| p.connection(*c).ok().and_then(|c| c.wire_number.clone()));
        let number = match pot.name().or(existing) {
            Some(n) => n,
            None => {
                let page =
                    p.connection(pot.connections[0]).and_then(|c| p.function(c.from.function)).map(|f| f.page)?;
                p.next_wire_number(page)
            }
        };
        for c in &pot.connections {
            if p.connection(*c)?.wire_number.as_deref() != Some(number.as_str()) {
                p.apply(Command::SetWireNumber { id: *c, number: Some(number.clone()) })?;
            }
        }
    }
    Ok(())
}

/// P2: the main switch with fuses, the distribution block, and the 400 V busbars P201-P204
/// with two branches: the defrost resistors (-QF1, -A3KM4) and the heating cables
/// (-QF3, -A3KM5).
fn power_distribution(b: &mut Builder) -> Result<()> {
    b.page("P2", "Power supply distribution", "Derivazione potenza")?;
    // As on the original, the supply runs left to right on one line per conductor: through
    // the main switch (turned and mirrored, so L1 stays on top) and the distribution block, on
    // to the right edge as busbars P201-P204, tapped by the two branches below.
    let phases = ["L1", "L2", "L3", "N"];
    let rows = [256.0, 248.0, 240.0, 232.0];
    let mut supply = Vec::new();
    for (name, y) in phases.iter().zip(rows) {
        supply.push(b.put_r(name, "iec.supply", 28.0, y, Rotation::R90)?);
    }
    b.describe("L1", "400 V 50 Hz")?;
    let qs1 = b.put_r("P2QS1", "iec.disconnector_fuse_3", 64.0, rows[0], Rotation::R90)?;
    b.mirror(qs1)?;
    b.describe("P2QS1", "Main switch with fuses 63 A")?;
    b.parts(
        "P2QS1",
        &[
            ("38316006", 1, "Socomec", "Switch-disconnector 4P 63 A for NH00 fuses"),
            ("14132111", 1, "Socomec", "External handle S1, black, IP65"),
            ("1500663", 3, "Example", "Fuse NH00 gG 63 A 500 V"),
            ("64200000", 1, "Socomec", "Fused neutral bar NH00 160 A"),
            ("14290000", 1, "Socomec", "Door-interlock guide cone"),
            ("14001050", 1, "Socomec", "Shaft extension S1-S2 500 mm 10x10"),
        ],
    )?;
    // The distribution block: a black box with each pole's input on the left joined inside to
    // its outlet on the right.
    let poles = ["L1", "L2", "L3", "N"];
    let outs = ["L1'", "L2'", "L3'", "N'"];
    let block = BlackBox {
        description: String::new(),
        tag_prefix: "-WB".into(),
        top: Vec::new(),
        bottom: Vec::new(),
        left: names(&poles),
        right: names(&outs),
        bridges: poles.iter().zip(outs).map(|(a, b)| names(&[a, b])).collect(),
    };
    let wb1 = b.black("P2WB1", block, 100.0, rows[0] + 8.0)?;
    b.describe("P2WB1", "Distribution block 4P 80 A")?;
    let wb: Vec<(F, &str, &str)> = poles.iter().zip(outs).map(|(p, o)| (wb1, *p, o)).collect();
    b.parts("P2WB1", &[("563930", 1, "Example", "Power distribution block 4P 80 A, 10 outlets")])?;
    for (i, (input, output)) in [("1", "2"), ("3", "4"), ("5", "6")].iter().enumerate() {
        b.ws((supply[i], "1"), (qs1, input), phases[i], "16", "BK")?;
        b.ws((qs1, output), (wb[i].0, wb[i].1), &format!("P20{}", i + 1), "16", "BK")?;
    }
    b.ws((supply[3], "1"), (wb[3].0, wb[3].1), "N", "16", "BU")?;

    let (qf1_x, qf3_x) = (216.0, 296.0);
    let mut taps = BTreeMap::new();
    for (i, y) in rows.iter().enumerate() {
        let number = format!("P20{}", i + 1);
        let mut last = (wb[i].0, wb[i].2.to_owned());
        let mut xs = vec![qf1_x + 8.0 * i as f64];
        if i == 0 {
            xs.push(qf3_x);
        }
        if i == 3 {
            xs.push(qf3_x + 8.0);
        }
        for x in xs {
            let j = b.junction(x, *y)?;
            b.wn((last.0, &last.1), (j, "3"), &number)?;
            taps.insert((i, x as i64), j);
            last = (j, "4".to_owned());
        }
        let out = b.arrow(&number, 368.0, *y, Left)?;
        b.wn((last.0, &last.1), (out, "1"), &number)?;
    }
    let qf1 = b.put("P2QF1", "iec.breaker_3pn_rcd", qf1_x, 192.0)?;
    b.describe("P2QF1", "Defrost resistors 80 A, 300 mA")?;
    b.parts("P2QF1", &[("5SU1344-1KK80", 1, SIEMENS, "RCBO 3P+N C80 300 mA")])?;
    for (i, pin) in ["1", "3", "5", "N"].iter().enumerate() {
        let tap = taps[&(i, (qf1_x + 8.0 * i as f64) as i64)];
        b.w((tap, "2"), (qf1, pin))?;
    }
    let km4 = b.put("A3KM4", "iec.contact_main_3", qf1_x, 148.0)?;
    for (i, (from, to)) in [("2", "1"), ("4", "3"), ("6", "5")].iter().enumerate() {
        b.ws((qf1, from), (km4, to), &format!("P23{}", 6 + i), "16", "BK")?;
    }
    for (i, pin) in ["2", "4", "6"].iter().enumerate() {
        let out = b.arrow(&format!("P21{}", i + 1), qf1_x + 8.0 * i as f64, 112.0, Up)?;
        b.ws((km4, pin), (out, "1"), &format!("P21{}", i + 1), "10", "BK")?;
    }
    let qf3 = b.put("P2QF3", "iec.breaker_1pn_rcd", qf3_x, 192.0)?;
    b.describe("P2QF3", "Heating cables 25 A, 300 mA")?;
    b.parts("P2QF3", &[("5SU1356-1KK25", 1, SIEMENS, "RCBO 1P+N C25 300 mA")])?;
    b.w((taps[&(0, qf3_x as i64)], "2"), (qf3, "1"))?;
    b.w((taps[&(3, (qf3_x + 8.0) as i64)], "2"), (qf3, "N"))?;
    let km5 = b.put("A3KM5", "iec.contact_main_3", qf3_x, 148.0)?;
    b.ws((qf3, "2"), (km5, "1"), "P242", "4", "BK")?;
    b.ws((qf3, "N'"), (km5, "3"), "P243", "4", "BK")?;
    for (pin, n) in [("2", "P220"), ("4", "P221")] {
        let x = if pin == "2" { qf3_x } else { qf3_x + 8.0 };
        let out = b.arrow(n, x, 112.0, Up)?;
        b.ws((km5, pin), (out, "1"), n, "4", "BK")?;
    }
    Ok(())
}

/// P3: the service sockets, the cabinet fan and light, the light feed and the 24 V supply,
/// all tapped from the busbars.
fn power_branches(b: &mut Builder) -> Result<()> {
    b.page("P3", "Power supply distribution", "Derivazione potenza")?;
    let rows = [256.0, 248.0, 240.0, 232.0];
    // Taps per busbar: (x, branch) in order.
    let taps: [&[f64]; 4] = [&[88.0], &[284.0], &[232.0, 336.0], &[96.0, 240.0, 292.0]];
    let mut at = BTreeMap::new();
    for (i, y) in rows.iter().enumerate() {
        let number = format!("P20{}", i + 1);
        let start = b.arrow(&number, 28.0, *y, Right)?;
        let mut last = (start, "1");
        for x in taps[i] {
            let j = b.junction(*x, *y)?;
            b.wn(last, (j, "3"), &number)?;
            at.insert((i, *x as i64), j);
            last = (j, "4");
        }
        if i == 3 {
            // N ends at the last branch, the 24 V supply's fuse.
            at.insert((3, 344), last.0);
        } else {
            let out = b.arrow(&number, 368.0, *y, Left)?;
            b.wn(last, (out, "1"), &number)?;
        }
    }
    let tap = |i: usize, x: f64| at[&(i, x as i64)];
    // Service sockets, fan with thermostat, cabinet light.
    let q1 = b.put("P3Q1", "iec.breaker_1pn_rcd", 88.0, 200.0)?;
    b.parts("P3Q1", &[("5SU1356-1KK06", 1, SIEMENS, "RCBO 1P+N C6 30 mA type AC")])?;
    b.ws((tap(0, 88.0), "2"), (q1, "1"), "P201", "1,5", "BK")?;
    b.ws((tap(3, 96.0), "2"), (q1, "N"), "P204", "1,5", "BU")?;
    // Below the breaker, N (P302) and under it L (P301) run right as two lines; each load hangs
    // from them in its own column: two sockets, the thermostat with the fan, the light.
    let xs2 = b.put("P3XS2", "iec.socket_2pe", 128.0, 128.0)?;
    let xs1 = b.put("P3XS1", "iec.socket_2pe", 160.0, 128.0)?;
    b.describe("P3XS1", "Service socket, max 800 W")?;
    b.parts("P3XS2", &[("09967", 1, "Vimar", "Socket 2P+E 16 A, DIN rail")])?;
    let s1 = b.put("P3S1", "iec.temperature_switch_no", 184.0, 136.0)?;
    b.describe("P3S1", "Cabinet thermostat")?;
    b.parts("P3S1", &[("TMF", 1, "Cosmotec", "Cabinet thermostat")])?;
    let ms1 = b.put("P3MS1", "iec.motor_1", 188.0, 100.0)?;
    b.describe("P3MS1", "Cabinet fan with filter 250 x 250")?;
    b.parts(
        "P3MS1",
        &[("GHV2500220", 1, "Cosmotec", "Fan 250 x 250 mm 230 V"), ("GHF25", 1, "Cosmotec", "Filter 250 x 250 mm")],
    )?;
    let hl1 = b.put("P3HL1", "iec.lamp", 216.0, 132.0)?;
    b.describe("P3HL1", "Cabinet light 5 W")?;
    b.parts("P3HL1", &[("FLL-230565U-S", 1, "Fandis", "LED lamp FLL 230 V 5 W")])?;
    let mut last = (q1, "2");
    for (k, (x, load, pin)) in [(120.0, xs2, "L"), (152.0, xs1, "L"), (184.0, s1, "13")].into_iter().enumerate() {
        let j = b.junction(x, 164.0)?;
        let feed = b.wn(last, (j, "3"), "P301")?;
        if k == 0 {
            b.bend(feed, 164.0)?;
        }
        b.wn((j, "2"), (load, pin), "P301")?;
        last = (j, "4");
    }
    b.wn(last, (hl1, "X1"), "P301")?;
    let mut last = (q1, "N'");
    for (k, (x, load, pin)) in [(128.0, xs2, "N"), (160.0, xs1, "N"), (200.0, ms1, "2")].into_iter().enumerate() {
        let j = b.junction(x, 172.0)?;
        let feed = b.wn(last, (j, "3"), "P302")?;
        if k == 0 {
            b.bend(feed, 172.0)?;
        }
        let drop = b.wn((j, "2"), (load, pin), "P302")?;
        if load == ms1 {
            // The fan's return turns right just above the fan, leaving room for the
            // thermostat's tag.
            b.bend(drop, 112.0)?;
        }
        last = (j, "4");
    }
    // The light's return goes up on its right to the end of N.
    let back = b.wn((hl1, "X2"), last, "P302")?;
    b.bend(back, 228.0)?;
    b.wn((s1, "14"), (ms1, "1"), "P303")?;
    // Light feed fuse, cell light breaker (to P5), 24 V supply.
    let fu4 = b.put("P3FU4", "iec.fuse_1pn", 232.0, 200.0)?;
    b.parts(
        "P3FU4",
        &[("2301137", 1, "Italweber", "Fuse holder 1P+N 10x38 32 A"), ("1421004", 1, "Italweber", "Fuse 10x38 gG 4 A")],
    )?;
    b.ws((tap(2, 232.0), "2"), (fu4, "1"), "P203", "1,5", "BK")?;
    b.ws((tap(3, 240.0), "2"), (fu4, "N"), "P204", "1,5", "BU")?;
    let q2 = b.put("P3Q2", "iec.breaker_1pn_rcd", 284.0, 200.0)?;
    b.parts("P3Q2", &[("5SU1356-1KK06", 1, SIEMENS, "RCBO 1P+N C6 30 mA type AC")])?;
    b.ws((tap(1, 284.0), "2"), (q2, "1"), "P202", "1,5", "BK")?;
    b.ws((tap(3, 292.0), "2"), (q2, "N"), "P204", "1,5", "BU")?;
    for (pin, n, y) in [("2", "P309", 140.0), ("N'", "P310", 152.0)] {
        let out = b.arrow(n, 368.0, y, Left)?;
        let wire = b.ws((q2, pin), (out, "1"), n, "1,5", if pin == "2" { "BK" } else { "BU" })?;
        // Straight across at the arrow's height.
        b.bend(wire, y)?;
    }
    let fu3 = b.put("P3FU3", "iec.fuse_1pn", 336.0, 200.0)?;
    b.parts(
        "P3FU3",
        &[("2301137", 1, "Italweber", "Fuse holder 1P+N 10x38 32 A"), ("1421004", 1, "Italweber", "Fuse 10x38 gG 4 A")],
    )?;
    b.ws((tap(2, 336.0), "2"), (fu3, "1"), "P203", "1,5", "BK")?;
    b.ws((tap(3, 344.0), "4"), (fu3, "N"), "P204", "1,5", "BU")?;
    let gd1 = b.put("P3GD1", "iec.power_supply", 340.0, 120.0)?;
    b.describe("P3GD1", "Power supply 24 V DC 10 A")?;
    b.parts("P3GD1", &[("S8VK-C24024", 1, "Omron", "Power supply 24 V 10 A, DIN rail")])?;
    b.wn((fu3, "2"), (gd1, "L"), "P307")?;
    b.wn((fu3, "N'"), (gd1, "N"), "P308")?;
    let plus = b.arrow("+A100", 336.0, 72.0, Up)?;
    let minus = b.arrow("-A100", 344.0, 72.0, Up)?;
    b.ws((gd1, "+"), (plus, "1"), "+A100", "2,5", "BU")?;
    b.ws((gd1, "-"), (minus, "1"), "-A100", "2,5", "BU")?;
    Ok(())
}

/// M3: the spiral's belt motor on inverter -MU3, through fuses -M3FU1, terminals -X0 and
/// shielded cable -WM3.
fn spiral_motor(b: &mut Builder) -> Result<()> {
    b.page("M3", "Spiral cooling belt motor", "Spirale raffreddamento")?;
    let mut fuse_in = Vec::new();
    for (i, y) in [256.0, 248.0, 240.0].iter().enumerate() {
        let number = format!("P20{}", i + 1);
        let start = b.arrow(&number, 28.0, *y, Right)?;
        let j = b.junction(60.0 + 8.0 * i as f64, *y)?;
        b.wn((start, "1"), (j, "3"), &number)?;
        let out = b.arrow(&number, 368.0, *y, Left)?;
        b.wn((j, "4"), (out, "1"), &number)?;
        fuse_in.push(j);
    }
    let fu = b.put("M3FU1", "iec.fuse_3", 60.0, 212.0)?;
    b.parts(
        "M3FU1",
        &[("2303038", 1, "Italweber", "Fuse holder 3P 10x38 32 A"), ("1421010", 3, "Italweber", "Fuse 10x38 gG 10 A")],
    )?;
    let mu3 = b.black(
        "MU3",
        BlackBox {
            description: "Inverter 1,1 kW".into(),
            tag_prefix: "-T".into(),
            top: names(&["U1", "V1", "W1", "PE", "69", "34", "16", "17"]),
            // PE' is the motor-side earth.
            bottom: names(&["U2", "V2", "W2", "PE'"]),
            left: Vec::new(),
            right: Vec::new(),
            bridges: Vec::new(),
        },
        52.0,
        180.0,
    )?;
    b.parts("MU3", &[("6SL3210-1KE13-2AF1", 1, SIEMENS, "Inverter 3 AC 380-480 V 1,1 kW")])?;
    for (i, (pin, inv)) in [("1", "U1"), ("3", "V1"), ("5", "W1")].iter().enumerate() {
        b.ws((fuse_in[i], "2"), (fu, pin), &format!("P20{}", i + 1), "2,5", "BK")?;
        let out = ["2", "4", "6"][i];
        b.ws((fu, out), (mu3, inv), &format!("M30{}", i + 1), "2,5", "BK")?;
    }
    for (pin, x) in [("69", 92.0), ("34", 100.0)] {
        let zero = b.arrow("-A100", x, 216.0, Down)?;
        b.wn((zero, "1"), (mu3, pin), "-A100")?;
    }
    for (pin, n, x) in [("16", "A907", 108.0), ("17", "A908", 116.0)] {
        let from = b.arrow(n, x, 216.0, Down)?;
        b.wn((from, "1"), (mu3, pin), n)?;
    }
    let wm3 = b.cable("WM3", "4x1,5", 4, true)?;
    let m3 = b.put("M3", "iec.motor_3", 68.0, 96.0)?;
    b.describe("M3", "Belt motor 0,55 kW 400 V 1,3 A")?;
    for (i, (inv, motor)) in [("U2", "U"), ("V2", "V"), ("W2", "W")].iter().enumerate() {
        let x = 60.0 + 8.0 * i as f64;
        let t = b.terminal("X0", "iec.terminal", &(i + 1).to_string(), x, 128.0)?;
        b.w((mu3, inv), (t, "1"))?;
        let c = b.w((t, "2"), (m3, motor))?;
        b.core(c, wm3, &(i + 1).to_string(), "1,5")?;
    }
    // The motor's earth through PE terminal -X0:PE on the cable's fourth core; the shield on
    // -X0:SC-M3, a PE terminal too.
    let pe = b.terminal("X0", "iec.terminal_pe", "PE", 84.0, 128.0)?;
    b.wn((mu3, "PE'"), (pe, "1"), "PE")?;
    let c = b.wn((pe, "2"), (m3, "PE"), "PE")?;
    let spec = WireSpec {
        cross_section: "1,5".into(),
        colour: "GNYE".into(),
        cable: Some(CableCore { cable: wm3, core: "PE".into() }),
    };
    b.p.apply(Command::SetWireSpec { id: c, spec })?;
    b.terminal("X0", "iec.terminal_pe", "SC-M3", 92.0, 128.0)?;
    Ok(())
}

/// M10A: two spiral fans, each on a motor-protection breaker, terminals -X0 and a cable.
fn fan_motors(b: &mut Builder) -> Result<()> {
    b.page("M10A", "Spiral fans 1-2", "Ventilatori 1-2 raffreddamento")?;
    let rows = [228.0, 220.0, 212.0];
    let columns = [60.0, 180.0];
    let mut taps: Vec<Vec<F>> = vec![Vec::new(), Vec::new()];
    for (i, y) in rows.iter().enumerate() {
        let number = format!("M100{}", i + 4);
        let start = b.arrow(&number, 28.0, *y, Right)?;
        let mut last = (start, "1");
        for (k, x) in columns.iter().enumerate() {
            let j = b.junction(x + 8.0 * i as f64, *y)?;
            b.wn(last, (j, "3"), &number)?;
            taps[k].push(j);
            last = (j, "4");
        }
    }
    for (k, x) in columns.iter().enumerate() {
        let (qf, motor, cable) = [("M10QF1", "M10A", "WM10A"), ("M10QF2", "M10B", "WM10B")][k];
        let q = b.put(qf, "iec.motor_protection_3", *x, 168.0)?;
        b.describe(qf, "Motor protection 5,5-8 A")?;
        b.parts(qf, &[("3RV2011-1HA10", 1, SIEMENS, "Motor protection breaker 5,5-8 A")])?;
        let w = b.cable(cable, "4x2,5", 4, false)?;
        let m = b.put(motor, "iec.motor_3", x + 8.0, 64.0)?;
        b.describe(motor, "Fan 2,6 kW 400 V 5,75 A")?;
        for (i, (top, bottom, phase)) in [("1", "2", "U"), ("3", "4", "V"), ("5", "6", "W")].iter().enumerate() {
            b.ws((taps[k][i], "2"), (q, top), &format!("M100{}", i + 4), "2,5", "BK")?;
            let n = 6 + 3 * k + i;
            let t = b.terminal("X0", "iec.terminal", &n.to_string(), x + 8.0 * i as f64, 104.0)?;
            b.ws((q, bottom), (t, "1"), &format!("M10{:02}", n + 1), "2,5", "BK")?;
            let c = b.w((t, "2"), (m, phase))?;
            b.core(c, w, &(i + 1).to_string(), "2,5")?;
        }
    }
    Ok(())
}

/// A1: the 24 V supply through the four-channel electronic fuse -A1FE1.
fn supply_24v(b: &mut Builder) -> Result<()> {
    b.page("A1", "24 V supply", "Alimentazione 24V")?;
    let fe1 = b.black(
        "A1FE1",
        BlackBox {
            description: "Electronic fuse 4 x 24 V".into(),
            tag_prefix: "-F".into(),
            top: names(&["IN+"]),
            bottom: names(&["0V", "1", "2", "3", "4"]),
            left: Vec::new(),
            right: Vec::new(),
            bridges: Vec::new(),
        },
        92.0,
        200.0,
    )?;
    b.parts("A1FE1", &[("2906032", 1, "Phoenix Contact", "Electronic protection 4 channels 24 V DC 1-10 A")])?;
    let plus = b.arrow("+A100", 100.0, 236.0, Down)?;
    b.ws((plus, "1"), (fe1, "IN+"), "+A100", "2,5", "BU")?;
    let zero = b.arrow("-A100", 24.0, 140.0, Up)?;
    b.ws((fe1, "0V"), (zero, "1"), "-A100", "2,5", "BU")?;
    for (pin, n, x) in [("1", "+A101", 108.0), ("2", "+A102", 116.0), ("3", "+A103", 124.0)] {
        let out = b.arrow(n, x, 100.0, Up)?;
        b.ws((fe1, pin), (out, "1"), n, "1,5", "BU")?;
    }
    // Channel 4 feeds the PLC outputs, cut by the emergency relay -A3KA1: its changeover drawn
    // upside down, common 11 on top, the open side 14 below.
    b.parts("A3KA1", &[("1122880000", 1, WEIDMULLER, "Relay 24 V DC, 1 CO 6 A, with LED")])?;
    let ka1 = b.put_r("A3KA1", "iec.contact_co", 132.0, 136.0, Rotation::R180)?;
    b.wn((fe1, "4"), (ka1, "11"), "A104")?;
    let out = b.arrow("+A104", 132.0, 100.0, Up)?;
    b.ws((ka1, "14"), (out, "1"), "+A104", "1,5", "BU")?;
    Ok(())
}

/// A2: the two-channel emergency stop chain: the cabinet button, the remote one in the HMI
/// box, and three on the spiral through connectors -XCPR1 to -XCPR3.
fn emergency_chain(b: &mut Builder) -> Result<()> {
    b.page("A2", "Emergency push-buttons", "Circuito emergenza pulsanti")?;
    let stops = [
        ("A2SB1", None, "Cabinet emergency stop"),
        ("A2SB2", Some(("X90", "J90", "XHMI")), "Remote emergency stop"),
        ("A2SB3", Some(("X31", "J131", "XCPR1")), "Emergency stop, spiral inlet 1"),
        ("A2SB4", Some(("X32", "J132", "XCPR2")), "Emergency stop, spiral inlet 2"),
        ("A2SB5", Some(("X33", "J133", "XCPR3")), "Emergency stop, spiral inlet 3"),
    ];
    let estop_parts = [
        ("ZB4BS844", 1, SCHNEIDER, "Emergency stop head, red mushroom, push-pull"),
        ("ZBE102", 2, SCHNEIDER, "Contact block 1 NC"),
        ("ZBE101", 2, SCHNEIDER, "Contact block 1 NO"),
        ("ZB4BZ009", 1, SCHNEIDER, "Mounting collar"),
        ("080XTGR01", 1, "GE Power Controls", "Emergency label, yellow, 59 mm"),
    ];
    // Columns: each stop gets a slot; channel 2 runs 16 mm right of channel 1.
    let slots = [(32.0, 236.0), (32.0, 140.0), (152.0, 236.0), (152.0, 140.0), (272.0, 236.0)];
    let mut ends: [Option<(F, &str)>; 2] = [None, None];
    for (ch, start) in [("A331", 0usize), ("A332", 1)].iter().map(|(n, c)| (*n, *c)) {
        let x = slots[0].0 + 16.0 * start as f64;
        let a = b.arrow(ch, x, 262.0, Down)?;
        ends[start] = Some((a, "1"));
    }
    for (k, (tag, via, description)) in stops.iter().enumerate() {
        b.describe(tag, description)?;
        if k < 2 {
            b.parts(tag, &estop_parts)?;
        }
        let (x0, y0) = slots[k];
        // Moving up to the top of the next column: break both channels with arrows, as the
        // original does, rather than looping wires across the page.
        if k > 0 && y0 > slots[k - 1].1 {
            for (ch, end) in ends.iter_mut().enumerate() {
                let Some(prev) = *end else { continue };
                let name = format!("ES{k}{}", ch + 1);
                let at = b.p.function(prev.0)?.position;
                let out = b.arrow(&name, at.x, at.y - 12.0, Down)?;
                b.w(prev, (out, "1"))?;
                let back = b.arrow(&name, x0 + 16.0 * ch as f64, 262.0, Down)?;
                *end = Some((back, "1"));
            }
        }
        #[allow(clippy::needless_range_loop)] // `ch` is the channel: it picks numbers and symbols.
        for ch in 0..2 {
            let x = x0 + 16.0 * ch as f64;
            let symbol = if ch == 0 { "iec.emergency_stop_nc" } else { "iec.contact_nc" };
            let mut y = y0;
            // Into the box: terminal, connector pin.
            let mut chain: Vec<(F, &str, &str)> = Vec::new();
            if let Some((strip, plug, _)) = via {
                let base = if *strip == "X90" { 4 } else { 3 };
                let t = b.terminal(strip, "iec.terminal", &(base + ch).to_string(), x, y)?;
                let pin = b.terminal(plug, "iec.plug_pin", &(base + ch).to_string(), x, y - 14.0)?;
                chain.push((t, "1", "2"));
                chain.push((pin, "1", "2"));
                y -= 34.0;
            }
            // Numbered from the stop's contact blocks (or counted, without parts): 11-12, 21-22.
            let c = b.put(tag, symbol, x, y)?;
            chain.push((c, if ch == 0 { "11" } else { "21" }, if ch == 0 { "12" } else { "22" }));
            if let Some((strip, plug, _)) = via {
                let base = if *strip == "X90" { 6 } else { 5 };
                let pin = b.terminal(plug, "iec.plug_pin", &(base + ch).to_string(), x, y - 22.0)?;
                let t = b.terminal(strip, "iec.terminal", &(base + ch).to_string(), x, y - 36.0)?;
                chain.push((pin, "1", "2"));
                chain.push((t, "1", "2"));
            }
            for (f, input, output) in chain {
                if let Some(prev) = ends[ch] {
                    b.w(prev, (f, input))?;
                }
                ends[ch] = Some((f, output));
            }
        }
        if let Some((_, _, label)) = via {
            b.boxed(label, (x0 - 10.0, y0 - 66.0), (x0 + 26.0, y0 - 8.0))?;
        }
    }
    for (ch, n) in [(0usize, "A301"), (1, "A302")] {
        let out = b.arrow(n, slots[4].0 + 16.0 * ch as f64, 120.0, Up)?;
        if let Some(prev) = ends[ch] {
            b.wn(prev, (out, "1"), n)?;
        }
    }
    Ok(())
}

/// A3: the safety module -A3S1 with its reset button, the safety contactors -A3KM1/-A3KM2 and
/// the relays and contactors it releases.
fn safety_module(b: &mut Builder) -> Result<()> {
    b.page("A3", "Emergency circuit, safety module", "Circuito emergenza centralina")?;
    let s1 = b.black(
        "A3S1",
        BlackBox {
            description: "Safety module".into(),
            tag_prefix: "-K".into(),
            top: names(&["A1", "T1", "T2", "T3", "I11", "I12", "I13", "I14"]),
            bottom: names(&["A2", "O01", "O02", "O03", "OS1", "OS2", "OS3", "I21"]),
            left: Vec::new(),
            right: Vec::new(),
            bridges: Vec::new(),
        },
        120.0,
        176.0,
    )?;
    b.parts("A3S1", &[("CS MP202M0", 1, "Pizzato Elettrica", "Programmable multifunction safety module")])?;
    // Supply rails: +A101 along the top, -A100 along the bottom.
    let plus = b.arrow("+A101", 28.0, 240.0, Right)?;
    let zero = b.arrow("-A100", 28.0, 40.0, Right)?;
    let mut top = (plus, "1");
    let mut rail = BTreeMap::new();
    for x in [32.0, 64.0, 96.0, 128.0] {
        let j = b.junction(x, 240.0)?;
        b.wn(top, (j, "3"), "+A101")?;
        rail.insert(x as i64, j);
        top = (j, "4");
    }
    b.w((rail[&128], "2"), (s1, "A1"))?;
    // Reset button between +A101 and T1, its lamp from O01.
    let sh1 = b.put("A3SH1", "iec.pushbutton_no", 96.0, 208.0)?;
    b.describe("A3SH1", "Reset, illuminated blue")?;
    b.parts(
        "A3SH1",
        &[
            ("ZB4BW363", 1, SCHNEIDER, "Illuminated pushbutton head, blue, spring return"),
            ("ZBVB6", 1, SCHNEIDER, "Light block, blue, 24 V LED"),
            ("ZBE101", 1, SCHNEIDER, "Contact block 1 NO"),
            ("ZB4BZ009", 1, SCHNEIDER, "Mounting collar"),
            ("1716630000", 1, WEIDMULLER, "Marker holder SM-H 27/18, black"),
        ],
    )?;
    b.w((rail[&96], "2"), (sh1, "13"))?;
    b.wn((sh1, "14"), (s1, "T1"), "A309")?;
    let lamp = b.put("A3SH1", "iec.lamp", 136.0, 116.0)?;
    b.wn((s1, "O01"), (lamp, "X1"), "A310")?;
    // Emergency channels from A2.
    for (pin, n, x) in [("I11", "A301", 232.0), ("I12", "A302", 240.0)] {
        let from = b.arrow(n, x, 220.0, Down)?;
        b.wn((from, "1"), (s1, pin), n)?;
    }
    // Safety outputs to the safety contactors and the safety relay.
    let mut bottom = (zero, "1");
    let mut zeros = BTreeMap::new();
    for x in [32.0, 64.0, 128.0, 136.0, 160.0, 208.0, 248.0, 288.0] {
        let j = b.junction(x, 40.0)?;
        b.wn(bottom, (j, "3"), "-A100")?;
        zeros.insert(x as i64, j);
        bottom = (j, "4");
    }
    // The module's 0 V and the lamp's each straight down to their own junction.
    b.w((s1, "A2"), (zeros[&128], "1"))?;
    b.w((lamp, "X2"), (zeros[&136], "1"))?;
    let coils = [
        ("A3KM1", "iec.coil_contactor", "OS1", 160.0, "A311"),
        ("A3KM2", "iec.coil_contactor", "OS2", 208.0, "A312"),
        ("A3KA2", "iec.coil_relay", "OS3", 248.0, "A340"),
    ];
    for (tag, symbol, out, x, n) in coils {
        let c = b.put(tag, symbol, x, 88.0)?;
        b.wn((s1, out), (c, "A1"), n)?;
        b.w((c, "A2"), (zeros[&(x as i64)], "1"))?;
    }
    b.describe("A3KM1", "Safety contactor 1")?;
    b.describe("A3KM2", "Safety contactor 2")?;
    b.describe("A3KA2", "Safety relay")?;
    let contactor = [
        ("3RT2016-2BB41", 1, SIEMENS, "Contactor 4 kW, 1 NO, 24 V DC"),
        ("3RH2911-2HA22", 1, SIEMENS, "Auxiliary block 2 NO + 2 NC"),
    ];
    b.parts("A3KM1", &contactor)?;
    b.parts("A3KM2", &contactor)?;
    let relay = [("1122880000", 1, WEIDMULLER, "Relay 24 V DC, 1 CO 6 A, with LED")];
    b.parts("A3KA2", &relay)?;
    // -A3KA2 releases -A3KM4; -A3KM4 releases -A3KM5 and -A3KA1.
    let ka2 = b.put_r("A3KA2", "iec.contact_co", 64.0, 200.0, Rotation::R180)?;
    b.w((rail[&64], "2"), (ka2, "11"))?;
    let km4 = b.put("A3KM4", "iec.coil_contactor", 64.0, 88.0)?;
    b.describe("A3KM4", "Defrost resistors contactor")?;
    b.parts("A3KM4", &[("3RT2038-1NB30", 1, SIEMENS, "Contactor 37 kW, 24 V DC")])?;
    b.wn((ka2, "14"), (km4, "A1"), "A366")?;
    b.w((km4, "A2"), (zeros[&64], "1"))?;
    let km4_no = b.put("A3KM4", "iec.contact_no", 32.0, 200.0)?;
    b.w((rail[&32], "2"), (km4_no, "13"))?;
    let km5 = b.put("A3KM5", "iec.coil_contactor", 32.0, 88.0)?;
    b.describe("A3KM5", "Heating cables contactor")?;
    b.parts("A3KM5", &[contactor[0]])?;
    let j = b.junction(32.0, 160.0)?;
    b.wn((km4_no, "14"), (j, "1"), "A365")?;
    b.w((j, "2"), (km5, "A1"))?;
    b.w((km5, "A2"), (zeros[&32], "1"))?;
    let ka1 = b.put("A3KA1", "iec.coil_relay", 288.0, 88.0)?;
    b.describe("A3KA1", "Emergency relay, PLC outputs")?;
    b.parts("A3KA1", &relay)?;
    b.w((j, "4"), (ka1, "A1"))?;
    b.w((ka1, "A2"), (zeros[&288], "1"))?;
    Ok(())
}

/// I1: the CPU's digital inputs I0.0 to I0.6, fed from +A103.
fn inputs_cpu(b: &mut Builder) -> Result<()> {
    b.page("I1", "Digital inputs CPU", "Ingressi digitali CPU")?;
    b.describe("C1D1", "CPU 1214C DC/DC/DC")?;
    b.parts("C1D1", &[("6ES7214-1AG40-0XB0", 1, SIEMENS, "CPU 1214C, 14 DI / 10 DO / 2 AI, 24 V DC")])?;
    let columns = [40.0, 84.0, 128.0, 172.0, 228.0, 280.0, 332.0];
    let start = b.arrow("+A103", 28.0, 252.0, Right)?;
    let mut last = (start, "1");
    let mut taps = Vec::new();
    for x in columns {
        let j = b.junction(x, 252.0)?;
        b.wn(last, (j, "3"), "+A103")?;
        taps.push(j);
        last = (j, "4");
    }
    let end = b.arrow("+A103", 368.0, 252.0, Left)?;
    b.wn(last, (end, "1"), "+A103")?;
    let input = |b: &mut Builder, k: usize, x: f64, name: &str| -> Result<F> {
        b.channel("C1D1", "iec.plc_di", x, 56.0, &format!("I0.{k}"), name)
    };
    // I0.0 emergency reset: both safety contactors closed.
    let km1 = b.put("A3KM1", "iec.contact_no", columns[0], 220.0)?;
    let km2 = b.put("A3KM2", "iec.contact_no", columns[0], 184.0)?;
    b.w((taps[0], "2"), (km1, "13"))?;
    b.wn((km1, "14"), (km2, "13"), "I013")?;
    let i0 = input(b, 0, columns[0], "Emergency_Reset")?;
    b.wn((km2, "14"), (i0, "1"), "I000")?;
    // Push-buttons.
    let buttons = [
        ("I0SB1", "iec.pushbutton_no", "Siren_Off", "ZB4BA2", "Pushbutton head, black"),
        ("I0SB2", "iec.pushbutton_no", "Reset", "ZB4BA5", "Pushbutton head, yellow"),
        ("I0SH3", "iec.pushbutton_no", "Start", "ZB4BW333", "Illuminated pushbutton head, green"),
        ("I0SB4", "iec.pushbutton_no", "Stop", "ZB4BA4", "Pushbutton head, red"),
    ];
    for (k, (tag, symbol, name, head, head_text)) in buttons.iter().enumerate() {
        let x = columns[k + 1];
        let sb = b.put(tag, symbol, x, 184.0)?;
        b.w((taps[k + 1], "2"), (sb, "13"))?;
        let i = input(b, k + 1, x, name)?;
        b.wn((sb, "14"), (i, "1"), &format!("I00{}", k + 1))?;
        b.describe(tag, &name.replace('_', " "))?;
        b.parts(
            tag,
            &[
                (head, 1, SCHNEIDER, head_text),
                ("ZBE101", 1, SCHNEIDER, "Contact block 1 NO"),
                ("ZB4BZ009", 1, SCHNEIDER, "Mounting collar"),
            ],
        )?;
    }
    // The illuminated start button also has its light block.
    b.parts(
        "I0SH3",
        &[
            ("ZB4BW333", 1, SCHNEIDER, "Illuminated pushbutton head, green"),
            ("ZBVB3", 1, SCHNEIDER, "Light block, green, 24 V LED"),
            ("ZBE101", 1, SCHNEIDER, "Contact block 1 NO"),
            ("ZB4BZ009", 1, SCHNEIDER, "Mounting collar"),
        ],
    )?;
    // I0.5 spiral door 3 open: magnetic sensor through connector -XCPR3.
    let x = columns[5];
    let sq4 = b.put("A5SQ4", "iec.proximity_magnetic_nc", x, 208.0)?;
    b.describe("A5SQ4", "Spiral door 3 closed")?;
    let pin = b.terminal("J133", "iec.plug_pin", "21", x, 176.0)?;
    let t = b.terminal("X33", "iec.terminal", "21", x, 156.0)?;
    b.w((taps[5], "2"), (sq4, "11"))?;
    b.w((sq4, "12"), (pin, "1"))?;
    b.w((pin, "2"), (t, "1"))?;
    let i5 = input(b, 5, x, "Door3_Open")?;
    b.wn((t, "2"), (i5, "1"), "I005")?;
    b.boxed("XCPR3", (x - 16.0, 168.0), (x + 12.0, 222.0))?;
    // I0.6 cabinet emergency stop pressed: a spare NO of -A2SB1.
    let x = columns[6];
    // The stop's NO block: 13-14.
    let sb1 = b.put("A2SB1", "iec.contact_no", x, 184.0)?;
    b.w((taps[6], "2"), (sb1, "13"))?;
    let i6 = input(b, 6, x, "Emergency_Cabinet")?;
    b.wn((sb1, "14"), (i6, "1"), "I006")?;
    Ok(())
}

/// O1: the CPU's digital outputs Q0.0 to Q0.7 to their loads, returning to -A100.
fn outputs_cpu(b: &mut Builder) -> Result<()> {
    b.page("O1", "Digital outputs CPU", "Uscite digitali CPU")?;
    let columns = [32.0, 80.0, 124.0, 172.0, 220.0, 264.0, 304.0, 348.0];
    let names =
        ["Upstream_Ok", "Lamp_Start", "", "Cell_Light", "Lamp_Door2", "Column_Green", "Column_Red", "Column_Siren"];
    let mut outs = Vec::new();
    for (k, x) in columns.iter().enumerate() {
        outs.push(b.channel("C1D1", "iec.plc_do", *x, 228.0, &format!("Q0.{k}"), names[k])?);
    }
    let start = b.arrow("-A100", 28.0, 72.0, Right)?;
    let mut last = (start, "1");
    let mut rail = BTreeMap::new();
    for (k, x) in columns.iter().enumerate() {
        if k == 2 {
            continue;
        }
        let j = b.junction(*x, 72.0)?;
        b.wn(last, (j, "3"), "-A100")?;
        rail.insert(k, j);
        last = (j, "4");
    }
    let end = b.arrow("-A100", 368.0, 72.0, Left)?;
    b.wn(last, (end, "1"), "-A100")?;
    let ka1 = b.put("O0KA1", "iec.coil_relay", columns[0], 120.0)?;
    b.describe("O0KA1", "Upstream consent relay")?;
    b.parts("O0KA1", &[("1122880000", 1, WEIDMULLER, "Relay 24 V DC, 1 CO 6 A, with LED")])?;
    let sh3 = b.put("I0SH3", "iec.lamp", columns[1], 120.0)?;
    let x22 = b.terminal("X22", "iec.terminal", "2", columns[2], 160.0)?;
    let km1 = b.put("O0KM1", "iec.coil_contactor", columns[3], 120.0)?;
    b.describe("O0KM1", "Cell light contactor")?;
    b.parts("O0KM1", &[("3RT2016-2BB41", 1, SIEMENS, "Contactor 4 kW, 1 NO, 24 V DC")])?;
    let x32 = b.terminal("X32", "iec.terminal", "22", columns[4], 168.0)?;
    let j132 = b.terminal("J132", "iec.plug_pin", "22", columns[4], 150.0)?;
    let hl1 = b.put("O0HL1", "iec.lamp", columns[4], 120.0)?;
    b.describe("O0HL1", "Spiral door 2 open")?;
    b.boxed("XCPR2", (columns[4] - 12.0, 104.0), (columns[4] + 16.0, 158.0))?;
    let hl5 = b.put("O0HL5", "iec.lamp", columns[5], 120.0)?;
    let hl6 = b.put("O0HL6", "iec.lamp", columns[6], 120.0)?;
    let ha7 = b.put("O0HA7", "iec.siren", columns[7], 120.0)?;
    b.boxed("O0HLA8", (columns[5] - 12.0, 104.0), (columns[7] + 20.0, 136.0))?;
    for (tag, text) in [("O0HL5", "Column lamp, green"), ("O0HL6", "Column lamp, red"), ("O0HA7", "Column siren")] {
        b.describe(tag, text)?;
    }
    let numbers: Vec<String> = (0..8).map(|k| format!("O00{k}")).collect();
    b.wn((outs[0], "1"), (ka1, "A1"), &numbers[0])?;
    b.wn((outs[1], "1"), (sh3, "X1"), &numbers[1])?;
    b.wn((outs[2], "1"), (x22, "1"), &numbers[2])?;
    b.wn((outs[3], "1"), (km1, "A1"), &numbers[3])?;
    b.wn((outs[4], "1"), (x32, "1"), &numbers[4])?;
    b.w((x32, "2"), (j132, "1"))?;
    b.w((j132, "2"), (hl1, "X1"))?;
    b.wn((outs[5], "1"), (hl5, "X1"), &numbers[5])?;
    b.wn((outs[6], "1"), (hl6, "X1"), &numbers[6])?;
    b.wn((outs[7], "1"), (ha7, "1"), &numbers[7])?;
    for (k, f, pin) in
        [(0, ka1, "A2"), (1, sh3, "X2"), (3, km1, "A2"), (4, hl1, "X2"), (5, hl5, "X2"), (6, hl6, "X2"), (7, ha7, "2")]
    {
        b.w((f, pin), (rail[&k], "1"))?;
    }
    Ok(())
}

/// O3: the outputs Q3.0 to Q3.7 of the I/O card -C1A3: relays and contactors, with the
/// defrost contactors interlocked through each other's NC contacts.
fn outputs_card(b: &mut Builder) -> Result<()> {
    b.page("O3", "Digital outputs C1A3", "Uscite digitali C1A3")?;
    b.describe("C1A3", "Digital I/O 16 DI / 16 DO")?;
    b.parts("C1A3", &[("6ES7223-1BL32-0XB0", 1, SIEMENS, "Digital I/O SM 1223, 16 DI / 16 DO, 24 V DC")])?;
    let columns = [24.0, 68.0, 112.0, 156.0, 200.0, 244.0, 288.0, 332.0];
    let names = [
        "Cold_Request",
        "Defrost_Request",
        "Snorkel_Heater",
        "Dryer_Heater",
        "Heating_Shutters",
        "Heating_Boxes",
        "Defrost_1",
        "Defrost_2",
    ];
    let mut outs = Vec::new();
    for (k, x) in columns.iter().enumerate() {
        outs.push(b.channel("C1A3", "iec.plc_do", *x, 228.0, &format!("Q3.{k}"), names[k])?);
    }
    // -A100 along the bottom, +A103 for the interlocked defrost contactors along the top right.
    let start = b.arrow("-A100", 14.0, 72.0, Right)?;
    let coil_x = [24.0, 68.0, 112.0, 156.0, 200.0, 244.0, 288.0, 310.0, 332.0, 356.0];
    let mut last = (start, "1");
    let mut rail = BTreeMap::new();
    for x in coil_x {
        let j = b.junction(x, 72.0)?;
        b.wn(last, (j, "3"), "-A100")?;
        rail.insert(x as i64, j);
        last = (j, "4");
    }
    let relay = [("1122880000", 1, WEIDMULLER, "Relay 24 V DC, 1 CO 6 A, with LED")];
    let contactor = [("3RT2016-2BB41", 1, SIEMENS, "Contactor 4 kW, 1 NO, 24 V DC")];
    let simple = [
        ("O3KA0", "iec.coil_relay"),
        ("O3KA1", "iec.coil_relay"),
        ("O3KM1", "iec.coil_contactor"),
        ("O3KM2", "iec.coil_contactor"),
        ("O3KM3", "iec.coil_contactor"),
        ("O3KM4", "iec.coil_contactor"),
    ];
    for (k, (tag, symbol)) in simple.iter().enumerate() {
        let c = b.put(tag, symbol, columns[k], 112.0)?;
        b.wn((outs[k], "1"), (c, "A1"), &format!("O30{k}"))?;
        b.w((c, "A2"), (rail[&(columns[k] as i64)], "1"))?;
        b.parts(tag, if symbol.ends_with("relay") { &relay } else { &contactor })?;
    }
    // Defrost 1: Q3.6 through -O3KM6's NC releases -O3KA5, whose NO pulls in -O3KM5.
    // Defrost 2 the same way round, so the two can't run together.
    let plus = b.arrow("+A103", 252.0, 264.0, Right)?;
    let ja = b.junction(310.0, 264.0)?;
    let jb = b.junction(356.0, 264.0)?;
    b.wn((plus, "1"), (ja, "3"), "+A103")?;
    b.wn((ja, "4"), (jb, "3"), "+A103")?;
    let pairs = [
        (6usize, "O3KM6", "O3KA5", "O3KM5", 288.0, 310.0, ja, ["O308", "O309"]),
        (7, "O3KM5", "O3KA6", "O3KM6", 332.0, 356.0, jb, ["O310", "O311"]),
    ];
    // Parts first, so that the contacts take their numbers from them.
    for (_, _, relay_tag, contactor_tag, ..) in pairs {
        b.parts(relay_tag, &relay)?;
        b.parts(contactor_tag, &[("3RT2036-1NB30", 1, SIEMENS, "Contactor 22 kW, 24 V DC")])?;
    }
    for (k, nc_of, relay_tag, contactor_tag, x, x_no, j, numbers) in pairs {
        let nc = b.put(nc_of, "iec.contact_nc", x, 164.0)?;
        b.wn((outs[k], "1"), (nc, "21"), &format!("O30{k}"))?;
        let ka = b.put(relay_tag, "iec.coil_relay", x, 112.0)?;
        b.wn((nc, "22"), (ka, "A1"), numbers[0])?;
        b.w((ka, "A2"), (rail[&(x as i64)], "1"))?;
        // The relay's changeover as on the original: the open side 14 on top, common 11 below.
        let co = b.put(relay_tag, "iec.contact_co", x_no, 164.0)?;
        b.w((j, "2"), (co, "14"))?;
        let km = b.put(contactor_tag, "iec.coil_contactor", x_no, 112.0)?;
        b.wn((co, "11"), (km, "A1"), numbers[1])?;
        b.w((km, "A2"), (rail[&(x_no as i64)], "1"))?;
    }
    let end = b.arrow("-A100", 368.0, 72.0, Left)?;
    b.wn(last, (end, "1"), "-A100")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protective_earth_wires_are_found() {
        let p = reference_project().unwrap();
        let earth = p.protective_earth_wires();
        let numbered = |n: &str| p.connections.values().filter(|c| c.wire_number.as_deref() == Some(n)).count();
        assert_eq!(numbered("PE"), 2, "inverter to -X0:PE, -X0:PE to the motor");
        for c in p.connections.values() {
            let pe = c.wire_number.as_deref() == Some("PE");
            assert_eq!(earth.contains(&c.id), pe, "{:?}", c.wire_number);
        }
    }

    #[test]
    fn the_reference_project_builds_and_has_its_pages() {
        let p = reference_project().unwrap();
        let pages: Vec<&str> = p.pages_in_order().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(pages, ["P2", "P3", "M3", "M10A", "A1", "A2", "A3", "I1", "O1", "O3", "L7"]);
    }
}
