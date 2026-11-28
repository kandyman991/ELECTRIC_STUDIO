//! The conveyor acceptance project (docs/ROADMAP.md, "Acceptance project: the conveyor").
//!
//! A 3 m belt conveyor with one reversible drive, two box sensors, operator controls on the
//! cabinet door and an emergency stop with a safety relay. This is the M7 stage: drawn
//! **schematic-first**, page by page, with today's operations, as a person would draw it. Later
//! milestones add stages built through the machine-centric workflow, and the acceptance tests
//! compare them with this one.
//!
//! Every id is fixed, so the project is the same on every build. Products come from the
//! fictional maker `Example`.
//!
//! Pages:
//! - 1: supply, main switch, motor protector, reversing contactors, motor terminals, motor;
//! - 2: 24 V supply;
//! - 3: emergency stop and safety relay;
//! - 4: PLC outputs to the contactors and lamps;
//! - 5: PLC inputs from the buttons, sensors and feedback contacts;
//! - L1: the cabinet mounting plate.

use crate::Direction::{Down, Up};
use crate::ProductId;
use crate::builder::{Builder, F};
use crate::geometry::{Point, Rotation};
use crate::library::BlackBox;
use crate::panel::{Panel, PanelItem, PanelItemKind, RAIL_WIDTH};
use crate::parts::{Part, PartRef};
use crate::project::*;
use crate::sheet::Sheet;
use crate::symbol::{ConnectionPointDef, Graphic, Role, SymbolDef, SymbolId};

/// The maker of every product in the conveyor.
pub const MAKER: &str = "Example";

/// The two-level terminal the sensors are wired through: level 1 (points 1 and 2) and level 2
/// (points 3 and 4), each bridged across, the levels isolated from each other.
pub const TWO_LEVEL: &str = "conveyor.terminal_2level";

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The two-level terminal symbol: a project symbol, so the shared symbol library and its golden
/// drawings are untouched.
fn two_level_terminal() -> SymbolDef {
    let cp =
        |name: &str, x: f64, y: f64, leaves| ConnectionPointDef { name: name.into(), at: Point::new(x, y), leaves };
    let line = |a: (f64, f64), b: (f64, f64)| Graphic::Line { a: Point::new(a.0, a.1), b: Point::new(b.0, b.1) };
    SymbolDef {
        id: SymbolId::new(TWO_LEVEL),
        description: "Terminal, two levels".into(),
        tag_prefix: "-X".into(),
        role: Role::Terminal,
        bridged: false,
        bridges: vec![names(&["1", "2"]), names(&["3", "4"])],
        hide_tag: false,
        hide_point_labels: false,
        category: "Terminals".into(),
        graphics: vec![
            line((-2.0, -2.0), (2.0, -2.0)),
            line((2.0, -2.0), (2.0, 2.0)),
            line((2.0, 2.0), (-2.0, 2.0)),
            line((-2.0, 2.0), (-2.0, -2.0)),
            line((6.0, -2.0), (10.0, -2.0)),
            line((10.0, -2.0), (10.0, 2.0)),
            line((10.0, 2.0), (6.0, 2.0)),
            line((6.0, 2.0), (6.0, -2.0)),
            Graphic::Dashed { a: Point::new(2.0, 0.0), b: Point::new(6.0, 0.0) },
        ],
        connection_points: vec![
            cp("1", 0.0, 4.0, Up),
            cp("2", 0.0, -4.0, Down),
            cp("3", 8.0, 4.0, Up),
            cp("4", 8.0, -4.0, Down),
        ],
    }
}

impl Builder {
    /// A product from [`MAKER`] in the catalogue, once.
    fn product(
        &mut self,
        number: &str,
        description: &str,
        contacts: &[&str],
        size: Option<(f64, f64, f64)>,
    ) -> Result<()> {
        if self.p.parts.contains_key(&ProductId::for_number(number)) {
            return Ok(());
        }
        self.apply(Command::AddPart(Part {
            id: ProductId::for_number(number),
            number: number.into(),
            manufacturer: MAKER.into(),
            description: description.into(),
            code: String::new(),
            contacts: names(contacts),
            width: size.map(|s| s.0),
            height: size.map(|s| s.1),
            depth: size.map(|s| s.2),
        }))
        .map(drop)
    }

    /// Products for a device: (number, quantity, description), all from [`MAKER`].
    fn products(&mut self, tag: &str, parts: &[(&str, u32, &str)], size: Option<(f64, f64, f64)>) -> Result<()> {
        self.products_with(tag, parts, &[], size)
    }

    /// Products for a device whose product carries contact blocks (`13-14`, `21-22`).
    fn products_with(
        &mut self,
        tag: &str,
        parts: &[(&str, u32, &str)],
        contacts: &[&str],
        size: Option<(f64, f64, f64)>,
    ) -> Result<()> {
        for (number, _, description) in parts {
            self.product(number, description, contacts, size)?;
        }
        let id = self.device(tag)?;
        let refs = parts.iter().map(|(n, q, _)| PartRef::new(n, *q)).collect();
        self.apply(Command::SetDeviceParts { id, parts: refs }).map(drop)
    }

    /// Products for a terminal strip, each counted once per terminal drawn with its symbol:
    /// (number, symbol, description).
    fn strip_products(&mut self, tag: &str, parts: &[(&str, &str, &str)], size: (f64, f64, f64)) -> Result<()> {
        for (number, _, description) in parts {
            self.product(number, description, &[], Some(size))?;
        }
        let id = self.device(tag)?;
        let refs = parts.iter().map(|(n, symbol, _)| PartRef::per_terminal_of(n, 1, symbol)).collect();
        self.apply(Command::SetDeviceParts { id, parts: refs }).map(drop)
    }

    /// A two-level strip terminal: both levels print `number`.
    fn terminal2(&mut self, strip: &str, number: &str, x: f64, y: f64) -> Result<F> {
        let device = self.device(strip)?;
        let designations = ["1", "2", "3", "4"].iter().map(|p| ((*p).to_owned(), number.to_owned())).collect();
        self.place(device, SymbolId::new(TWO_LEVEL), x, y, Rotation::R0, designations, Default::default())
    }

    /// A PLC input of `-A1` drawn turned, its wire leaving downwards, at (x, y).
    fn channel_r(&mut self, address: &str, x: f64, y: f64, name: &str) -> Result<F> {
        let f = self.put_r("-A1", "iec.plc_di", x, y, Rotation::R180)?;
        let properties = [(ADDRESS.to_owned(), address.to_owned()), (SYMBOL.to_owned(), name.to_owned())].into();
        self.apply(Command::SetProperties { id: f, properties })?;
        Ok(f)
    }

    /// A potential point (`L+`, `M`, `L1`...) drawn at (x, y), its wire leaving `down` or up.
    fn potential(&mut self, name: &str, x: f64, y: f64, down: bool) -> Result<F> {
        let rotation = if down { Rotation::R0 } else { Rotation::R180 };
        self.put_r(name, "iec.supply", x, y, rotation)
    }
}

/// The conveyor, drawn schematic-first.
pub fn conveyor() -> Result<Project> {
    build(false).map(|(project, _)| project)
}

/// The same schematic-first builder's forward steps, for registry conformance testing.
/// This is fixture data, not a way to execute a command through the engineering interface.
pub fn conveyor_steps() -> Result<(Project, Vec<Command>)> {
    build(true)
}

fn build(record: bool) -> Result<(Project, Vec<Command>)> {
    let mut p = Project::new("Belt conveyor");
    for s in crate::library::iec() {
        p.add_symbol(s);
    }
    let mut b = Builder { p, page: PageId::from_u128(0), next: 0xC0_0000, trace: record.then(Vec::new) };
    b.apply(Command::AddSymbol(two_level_terminal()))?;
    b.apply(Command::SetSheet(Sheet {
        origin: Point::new(-20.0, -10.0),
        columns: 8,
        first_column: 0,
        rows: 6,
        ..Sheet::default()
    }))?;
    let info = ProjectInfo {
        customer: "Example Logistics".into(),
        plant: "Belt conveyor 3 m".into(),
        code: "EX-CONV-1".into(),
        designer: "A.B.".into(),
        date: "2026-11-27".into(),
        revisions: vec![Revision {
            index: "0".into(),
            change: "First issue".into(),
            date: "2026-11-27".into(),
            by: "A.B.".into(),
        }],
    };
    b.apply(Command::SetProjectInfo { name: "Belt conveyor".into(), info })?;
    power(&mut b)?;
    supply_24v(&mut b)?;
    safety(&mut b)?;
    outputs(&mut b)?;
    inputs(&mut b)?;
    cabinet_layout(&mut b)?;
    // The emergency stop and the safety relay are safety-related (section 11.5, AI0).
    for tag in ["-S0", "-K3"] {
        if let Some(d) = b.p.device_by_tag(tag).map(|d| d.id) {
            b.apply(Command::SetSafetyRelated { device: d, related: true })?;
        }
    }
    let pages: Vec<PageId> = b.p.pages.keys().copied().collect();
    for page in pages {
        for cmd in b.p.tidy_wires(page) {
            b.apply(cmd)?;
        }
    }
    for cmd in b.p.renumber_all_wires() {
        b.apply(cmd)?;
    }
    Ok((b.p, b.trace.unwrap_or_default()))
}

/// Page 1: supply 3/N/PE 400/230 V, main switch -Q0, motor protector -Q1, forward -K1 and
/// reverse -K2 (L1 and L3 swapped), the 24 V supply's breaker -F2, terminals -X2, motor -M1
/// through cable -W1.
fn power(b: &mut Builder) -> Result<()> {
    b.page("1", "Supply and conveyor drive", "Alimentazione e motore nastro")?;
    let xs = [40.0, 48.0, 56.0];
    let phases = ["L1", "L2", "L3"];
    let mut supply = Vec::new();
    for (name, x) in phases.iter().zip(xs) {
        supply.push(b.potential(name, x, 256.0, true)?);
    }
    b.describe("L1", "3/N/PE AC 400/230 V 50 Hz")?;
    let n = b.potential("N", 136.0, 256.0, true)?;
    let pe = b.potential("PE", 72.0, 256.0, true)?;

    let q0 = b.put("-Q0", "iec.breaker_3", 40.0, 228.0)?;
    b.describe("-Q0", "Main switch 16 A, lockable")?;
    b.products("-Q0", &[("EX-MS-16", 1, "Main switch 3P 16 A, lockable handle")], Some((54.0, 90.0, 70.0)))?;
    for (i, pin) in ["1", "3", "5"].iter().enumerate() {
        b.w((supply[i], "1"), (q0, pin))?;
    }
    // Under the main switch L1 is tapped for the 24 V supply's breaker.
    let tap = b.junction(xs[0], 204.0)?;
    b.w((q0, "2"), (tap, "1"))?;
    let f2 = b.put("-F2", "iec.breaker_1pn_rcd", 128.0, 180.0)?;
    b.describe("-F2", "24 V supply feed C6, 30 mA")?;
    b.products("-F2", &[("EX-RCBO-C6", 1, "RCBO 1P+N C6 30 mA")], Some((36.0, 90.0, 70.0)))?;
    b.w((tap, "4"), (f2, "1"))?;
    b.w((n, "1"), (f2, "N"))?;
    let to_psu_l = b.arrow("PS-L", 128.0, 150.0, Up)?;
    let to_psu_n = b.arrow("PS-N", 136.0, 150.0, Up)?;
    b.w((f2, "2"), (to_psu_l, "1"))?;
    b.w((f2, "N'"), (to_psu_n, "1"))?;

    let q1 = b.put("-Q1", "iec.motor_protection_3", 40.0, 180.0)?;
    b.describe("-Q1", "Motor protection 1.1-1.6 A")?;
    b.products_with(
        "-Q1",
        &[("EX-MPCB-1.6", 1, "Motor protection circuit breaker 1.1-1.6 A, 1 NO")],
        &["13-14"],
        Some((45.0, 97.0, 97.0)),
    )?;
    b.w((tap, "2"), (q1, "1"))?;
    b.w((q0, "4"), (q1, "3"))?;
    b.w((q0, "6"), (q1, "5"))?;
    // Under the protector each phase feeds both contactors, on staggered rows so the reversing
    // wires cross instead of running on each other; -K2 takes L1 and L3 the other way.
    let mut out = Vec::new();
    for (i, pin) in ["2", "4", "6"].iter().enumerate() {
        let j = b.junction(xs[i], 164.0 - 4.0 * i as f64)?;
        b.w((q1, pin), (j, "1"))?;
        out.push(j);
    }
    let k1 = b.put("-K1", "iec.contact_main_3", 40.0, 132.0)?;
    let k2 = b.put("-K2", "iec.contact_main_3", 88.0, 132.0)?;
    for tag in ["-K1", "-K2"] {
        b.describe(tag, if tag == "-K1" { "Conveyor forward" } else { "Conveyor reverse" })?;
        b.products_with(
            tag,
            &[("EX-CT-4KW-24DC", 1, "Contactor 4 kW AC-3, 24 V DC coil, 1 NO + 1 NC")],
            &["13-14", "21-22"],
            Some((45.0, 77.0, 98.0)),
        )?;
    }
    for (i, pin) in ["1", "3", "5"].iter().enumerate() {
        b.w((out[i], "2"), (k1, pin))?;
    }
    // Reverse: L1 to -K2:5, L2 to -K2:3, L3 to -K2:1.
    // Each runs right, then straight down into its pin, so the three never share a run.
    for (i, (pin, x)) in [("5", 104.0), ("3", 96.0), ("1", 88.0)].iter().enumerate() {
        let wire = b.w((out[i], "4"), (k2, pin))?;
        b.bend(wire, *x)?;
    }
    // Both contactors' outputs meet on U, V, W above the motor terminals, staggered again.
    let mut motor_lines = Vec::new();
    for (i, pin) in ["2", "4", "6"].iter().enumerate() {
        let j = b.junction(xs[i], 112.0 - 4.0 * i as f64)?;
        b.w((k1, pin), (j, "1"))?;
        b.w((k2, pin), (j, "4"))?;
        motor_lines.push(j);
    }
    let mut x2 = Vec::new();
    for (i, number) in ["1", "2", "3"].iter().enumerate() {
        let t = b.terminal("-X2", "iec.terminal", number, xs[i], 88.0)?;
        b.w((motor_lines[i], "2"), (t, "1"))?;
        x2.push(t);
    }
    let x2pe = b.terminal("-X2", "iec.terminal_pe", "PE", 72.0, 88.0)?;
    b.w((pe, "1"), (x2pe, "1"))?;
    b.strip_products(
        "-X2",
        &[
            ("EX-TB-2.5", "iec.terminal", "Terminal 2.5 mm², grey"),
            ("EX-TB-2.5-PE", "iec.terminal_pe", "Terminal 2.5 mm², green-yellow, PE"),
        ],
        (5.1, 60.0, 47.0),
    )?;
    let m1 = b.put("-M1", "iec.motor_3", 48.0, 56.0)?;
    b.describe("-M1", "Conveyor motor 0.55 kW, 400 V, 1.5 A, 4 pole")?;
    b.products("-M1", &[("EX-MOT-0.55-4P", 1, "Three-phase motor 0.55 kW 400 V 4 pole IE3")], None)?;
    let w1 = b.cable("-W1", "4G1,5", 4, false)?;
    for (i, (pin, core)) in [("U", "1"), ("V", "2"), ("W", "3")].iter().enumerate() {
        let wire = b.w((x2[i], "2"), (m1, pin))?;
        b.core(wire, w1, core, "1,5")?;
    }
    let wire = b.w((x2pe, "2"), (m1, "PE"))?;
    b.core(wire, w1, "GNYE", "1,5")?;
    b.boxed("+CONV.DRV", (24.0, 32.0), (84.0, 72.0))?;
    Ok(())
}

/// Page 2: the 24 V DC supply -T1 and its output protection -F3, giving L+ and M.
fn supply_24v(b: &mut Builder) -> Result<()> {
    b.page("2", "24 V DC supply", "Alimentazione 24 V DC")?;
    let l = b.arrow("PS-L", 76.0, 240.0, Down)?;
    let n = b.arrow("PS-N", 84.0, 240.0, Down)?;
    let t1 = b.put("-T1", "iec.power_supply", 80.0, 200.0)?;
    b.describe("-T1", "Power supply 230 V AC / 24 V DC 5 A")?;
    b.products("-T1", &[("EX-PS-24-5", 1, "Power supply 24 V DC 5 A, DIN rail")], Some((40.0, 125.0, 110.0)))?;
    b.w((l, "1"), (t1, "L"))?;
    b.w((n, "1"), (t1, "N"))?;
    let f3 = b.put("-F3", "iec.fuse", 76.0, 160.0)?;
    b.describe("-F3", "24 V circuit protection 4 A")?;
    b.products("-F3", &[("EX-FH-10x38", 1, "Fuse holder 1P 10x38 with fuse 4 A gG")], Some((18.0, 81.0, 68.0)))?;
    b.w((t1, "+"), (f3, "1"))?;
    let plus = b.potential("L+", 76.0, 128.0, false)?;
    b.describe("L+", "24 V DC")?;
    b.w((f3, "2"), (plus, "1"))?;
    let zero = b.potential("M", 84.0, 128.0, false)?;
    b.describe("M", "0 V DC")?;
    b.w((t1, "-"), (zero, "1"))?;
    Ok(())
}

/// Page 3: emergency stop -S0 (two channels) into the safety relay -K3, whose safe outputs
/// feed the contactor coils (through interruptions to page 4) and whose auxiliary contact
/// reports to the PLC.
fn safety(b: &mut Builder) -> Result<()> {
    b.page("3", "Emergency stop", "Arresto di emergenza")?;
    let relay = BlackBox {
        description: "Safety relay 24 V DC, 2 channels, 2 NO safe + 1 NO auxiliary".into(),
        tag_prefix: "-K".into(),
        top: names(&["A1", "S11", "S21", "13", "23", "33"]),
        bottom: names(&["A2", "S12", "S22", "14", "24", "34"]),
        left: Vec::new(),
        right: Vec::new(),
        bridges: Vec::new(),
    };
    let k3 = b.black("-K3", relay, 160.0, 180.0)?;
    b.products(
        "-K3",
        &[("EX-SR-2CH", 1, "Safety relay 24 V DC, 2 channels, 2 NO + 1 NO aux")],
        Some((22.5, 100.0, 120.0)),
    )?;
    let plus = b.potential("L+", 120.0, 256.0, true)?;
    let zero = b.potential("M", 120.0, 112.0, false)?;
    let feed = b.junction(120.0, 236.0)?;
    b.w((plus, "1"), (feed, "1"))?;
    b.w((feed, "2"), (k3, "A1"))?;
    b.w((k3, "A2"), (zero, "1"))?;
    // The two channels of the emergency stop, each a contact of -S0.
    let ch1 = b.put("-S0", "iec.emergency_stop_nc", 40.0, 200.0)?;
    let ch2 = b.put("-S0", "iec.emergency_stop_nc", 64.0, 200.0)?;
    b.describe("-S0", "Emergency stop, 2 NC")?;
    b.products("-S0", &[("EX-ES-2NC", 1, "Emergency stop mushroom, twist release, 2 NC")], None)?;
    b.w((k3, "S11"), (ch1, "11"))?;
    b.w((ch1, "12"), (k3, "S12"))?;
    b.w((k3, "S21"), (ch2, "21"))?;
    b.w((ch2, "22"), (k3, "S22"))?;
    // Safe outputs: the PLC outputs reach the coils only through them.
    for (from, through, to) in [("Q-FWD", "13", "14"), ("Q-REV", "23", "24")] {
        let a = b.arrow(from, 200.0 + if through == "13" { 0.0 } else { 16.0 }, 236.0, Down)?;
        b.w((a, "1"), (k3, through))?;
        let out = b.arrow(&format!("{from}-SAFE"), 200.0 + if through == "13" { 0.0 } else { 16.0 }, 128.0, Up)?;
        b.w((k3, to), (out, "1"))?;
    }
    // The auxiliary contact tells the PLC the relay is on.
    b.w((feed, "3"), (k3, "33"))?;
    let ok = b.arrow("SAFE-OK", 248.0, 128.0, Up)?;
    b.w((k3, "34"), (ok, "1"))?;
    b.boxed("+CAB.D", (24.0, 176.0), (84.0, 224.0))?;
    Ok(())
}

/// Page 4: PLC outputs: forward and reverse through the safety relay and each other's NC
/// interlock to the coils; the running and fault lamps.
fn outputs(b: &mut Builder) -> Result<()> {
    b.page("4", "PLC outputs", "Uscite PLC")?;
    b.describe("-A1", "PLC 24 V DC, 14 DI / 10 DO")?;
    b.products(
        "-A1",
        &[("EX-PLC-14-10", 1, "Compact PLC 24 V DC, 14 DI / 10 DO, Ethernet")],
        Some((110.0, 100.0, 75.0)),
    )?;
    let zero_at = 72.0;
    let (fwd_x, rev_x) = (40.0, 104.0);
    for (k, (x, name, coil, interlock)) in
        [(fwd_x, "Q-FWD", "-K1", "-K2"), (rev_x, "Q-REV", "-K2", "-K1")].iter().enumerate()
    {
        // Out to the safety relay in one column, back from it in the next.
        let out = b.channel("-A1", "iec.plc_do", *x, 248.0, &format!("Q0.{k}"), name)?;
        let to_relay = b.arrow(name, *x, 224.0, Up)?;
        b.w((out, "1"), (to_relay, "1"))?;
        let back_x = x + 16.0;
        let back = b.arrow(&format!("{name}-SAFE"), back_x, 224.0, Down)?;
        let nc = b.put(interlock, "iec.contact_nc", back_x, 192.0)?;
        b.w((back, "1"), (nc, "11"))?;
        let c = b.put(coil, "iec.coil", back_x, 152.0)?;
        b.w((nc, "12"), (c, "A1"))?;
        let m = b.potential("M", back_x, zero_at, false)?;
        b.w((c, "A2"), (m, "1"))?;
    }
    for (k, (x, tag, name, what)) in
        [(168.0, "-H1", "Running", "green"), (232.0, "-H2", "Fault", "red")].iter().enumerate()
    {
        let out = b.channel("-A1", "iec.plc_do", *x, 248.0, &format!("Q0.{}", k + 2), name)?;
        let h = b.put(tag, "iec.lamp", *x, 160.0)?;
        b.describe(tag, &format!("{name} lamp, {what}"))?;
        b.products(
            tag,
            &[(&format!("EX-LAMP-24-{}", what[..1].to_uppercase()), 1, &format!("Pilot lamp 24 V DC, {what}"))],
            None,
        )?;
        b.w((out, "1"), (h, "X1"))?;
        let m = b.potential("M", *x, zero_at, false)?;
        b.w((h, "X2"), (m, "1"))?;
    }
    b.boxed("+CAB.D", (152.0, 140.0), (248.0, 180.0))?;
    Ok(())
}

/// Page 5: PLC inputs: start, stop and jog buttons on the door, the infeed and outfeed
/// sensors through two-level terminals -X3 and cables -W2, -W3, the protector's and the
/// safety relay's contacts, the contactors' feedback.
fn inputs(b: &mut Builder) -> Result<()> {
    b.page("5", "PLC inputs", "Ingressi PLC")?;
    let columns = [32.0, 72.0, 112.0, 152.0, 208.0, 264.0, 304.0, 336.0, 368.0];
    // L+ runs left to right along the top; each column it feeds but the last is a junction.
    // Column 6 (the safety relay's contact) is fed from page 3 instead.
    let start = b.put_r("L+", "iec.supply", 16.0, 248.0, Rotation::R270)?;
    let mut last = (start, "1");
    let mut feeds = Vec::new();
    for (k, x) in columns.iter().enumerate() {
        if k == 6 || k + 1 == columns.len() {
            feeds.push(last);
        } else {
            let j = b.junction(*x, 248.0)?;
            b.w(last, (j, "3"))?;
            feeds.push((j, "2"));
            last = (j, "4");
        }
    }
    let input = |b: &mut Builder, k: usize, x: f64, name: &str| -> Result<F> {
        let address = if k < 8 { format!("I0.{k}") } else { format!("I1.{}", k - 8) };
        b.channel("-A1", "iec.plc_di", x, 56.0, &address, name)
    };
    // Door buttons.
    for (k, (tag, symbol, name, top, bottom)) in [
        ("-S1", "iec.pushbutton_no", "Start", "13", "14"),
        ("-S2", "iec.pushbutton_nc", "Stop", "11", "12"),
        ("-S3", "iec.pushbutton_no", "Jog_Reverse", "13", "14"),
    ]
    .iter()
    .enumerate()
    {
        let x = columns[k];
        let sb = b.put(tag, symbol, x, 200.0)?;
        b.w(feeds[k], (sb, top))?;
        let i = input(b, k, x, name)?;
        b.w((sb, bottom), (i, "1"))?;
        b.describe(tag, &name.replace('_', " "))?;
        let colour = ["green", "red", "black"][k];
        b.products(tag, &[(&format!("EX-PB-{}", colour.to_uppercase()), 1, &format!("Pushbutton {colour}"))], None)?;
    }
    b.boxed("+CAB.D", (20.0, 184.0), (124.0, 216.0))?;
    // Sensors in the field, under their terminals: brown +24 V through level 1 of a two-level
    // terminal, blue 0 V through level 2, black signal through a single terminal up to its
    // input, which is drawn turned above the terminal.
    let zero = b.put_r("M", "iec.supply", 136.0, 176.0, Rotation::R270)?;
    let mut zero_last = (zero, "1");
    for (k, (sensor, x, location)) in
        [("-B1", columns[3], "+CONV.IN"), ("-B2", columns[4], "+CONV.OUT")].iter().enumerate()
    {
        let number_power = (2 * k + 1).to_string();
        let number_signal = (2 * k + 2).to_string();
        let t2 = b.terminal2("-X3", &number_power, *x, 160.0)?;
        let ts = b.terminal("-X3", "iec.terminal", &number_signal, *x + 16.0, 160.0)?;
        let sensor_box = BlackBox {
            description: "Photoelectric sensor, PNP NO, 24 V DC, M12".into(),
            tag_prefix: "-B".into(),
            top: names(&["BN", "BU", "BK"]),
            bottom: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
            bridges: Vec::new(),
        };
        let s = b.black(sensor, sensor_box, *x, 112.0)?;
        b.products(sensor, &[("EX-PE-PNP-M12", 1, "Photoelectric sensor PNP NO 24 V DC, M12")], None)?;
        b.w(feeds[3 + k], (t2, "1"))?;
        let cable = b.cable(&format!("-W{}", k + 2), "3x0,34 M12", 3, false)?;
        let bn = b.w((t2, "2"), (s, "BN"))?;
        b.core(bn, cable, "BN", "0,34")?;
        let bu = b.w((s, "BU"), (t2, "4"))?;
        b.core(bu, cable, "BU", "0,34")?;
        let bk = b.w((s, "BK"), (ts, "2"))?;
        b.core(bk, cable, "BK", "0,34")?;
        if k == 0 {
            let j = b.junction(*x + 8.0, 176.0)?;
            b.w(zero_last, (j, "4"))?;
            b.w((j, "2"), (t2, "3"))?;
            zero_last = (j, "3");
        } else {
            b.w(zero_last, (t2, "3"))?;
        }
        let i = b.channel_r(&format!("I0.{}", 3 + k), *x + 16.0, 200.0, if k == 0 { "Infeed" } else { "Outfeed" })?;
        b.w((ts, "1"), (i, "1"))?;
        b.describe(sensor, if k == 0 { "Box at infeed" } else { "Box at outfeed" })?;
        b.boxed(location, (*x - 12.0, 96.0), (*x + 28.0, 132.0))?;
    }
    b.strip_products(
        "-X3",
        &[
            ("EX-TB2L-2.5", TWO_LEVEL, "Two-level terminal 2.5 mm²"),
            ("EX-TB-2.5", "iec.terminal", "Terminal 2.5 mm², grey"),
        ],
        (5.1, 70.0, 55.0),
    )?;
    // Protector tripped, safety relay on, contactor feedback.
    let q1 = b.put("-Q1", "iec.contact_no", columns[5], 200.0)?;
    b.w(feeds[5], (q1, "13"))?;
    let i = input(b, 5, columns[5], "Protector_OK")?;
    b.w((q1, "14"), (i, "1"))?;
    let ok = b.arrow("SAFE-OK", columns[6], 216.0, Down)?;
    let i = input(b, 6, columns[6], "Safety_OK")?;
    b.w((ok, "1"), (i, "1"))?;
    for (k, tag) in ["-K1", "-K2"].iter().enumerate() {
        let x = columns[7 + k];
        let c = b.put(tag, "iec.contact_no", x, 200.0)?;
        b.w(feeds[7 + k], (c, "13"))?;
        let i = input(b, 7 + k, x, if k == 0 { "Forward_On" } else { "Reverse_On" })?;
        b.w((c, "14"), (i, "1"))?;
    }
    Ok(())
}

/// L1: the mounting plate, 600 x 400 mm: ducts along the top, middle and bottom and up both
/// sides, two rails between them.
fn cabinet_layout(b: &mut Builder) -> Result<()> {
    const W: f64 = 600.0;
    const H: f64 = 400.0;
    b.page("L1", "Cabinet layout", "Layout quadro")?;
    let page = b.page;
    b.apply(Command::SetPanel { page, panel: Some(Panel { width: W, height: H }) })?;
    let item = |b: &mut Builder, kind: PanelItemKind, x: f64, y: f64, length: f64, vertical: bool| {
        let id = PanelItemId::from_u128(b.id());
        b.apply(Command::AddPanelItem(PanelItem {
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
    let duct = PanelItemKind::Duct { width: 40.0, depth: 60.0 };
    for y in [H - 40.0, 180.0, 0.0] {
        item(b, duct.clone(), 0.0, y, W, false)?;
    }
    item(b, duct.clone(), 0.0, 40.0, H - 80.0, true)?;
    item(b, duct, W - 40.0, 40.0, H - 80.0, true)?;
    let place = |b: &mut Builder, tag: &str, x: f64, centre: f64| -> Result<f64> {
        let device = b.device(tag)?;
        let (w, h) = b.p.device_size(device).0;
        item(b, PanelItemKind::Device { device }, x, centre - h / 2.0, 0.0, false)?;
        Ok(x + w + 5.0)
    };
    let rows: [(f64, &[&str]); 2] =
        [(290.0, &["-Q0", "-F2", "-T1", "-F3", "-K3"]), (100.0, &["-Q1", "-K1", "-K2", "-A1", "-X2", "-X3"])];
    for (centre, tags) in rows {
        item(b, PanelItemKind::Rail, 40.0, centre - RAIL_WIDTH / 2.0, W - 80.0, false)?;
        let mut x = 50.0;
        for tag in tags {
            x = place(b, tag, x, centre)?;
        }
    }
    Ok(())
}
