//! The built-in IEC 60617 symbol library: the starter set every new project gets.
//!
//! All symbols are drawn in symbol-local millimetres. Connection points sit on the 4 mm grid
//! across the direction their wire leaves in, so wires between devices placed on the grid run
//! straight. Letter codes (the tag prefix, e.g. `-KM`) follow
//! IEC 81346-2 as panel builders commonly use it.

use crate::geometry::{Direction, Point};
use crate::symbol::*;

fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Graphic {
    Graphic::Line { a: Point::new(ax, ay), b: Point::new(bx, by) }
}

fn cp(name: &str, x: f64, y: f64, leaves: Direction) -> ConnectionPointDef {
    ConnectionPointDef { name: name.to_owned(), at: Point::new(x, y), leaves }
}

/// The whole starter library, each symbol with its category.
pub fn iec() -> Vec<SymbolDef> {
    let mut all = basics();
    all.extend(plc_channel_symbols());
    all.extend(power_symbols());
    all.extend(operator_symbols());
    all.extend(sensor_symbols());
    all.extend(load_symbols());
    for s in &mut all {
        if s.category.is_empty() {
            s.category = category_of(&s.id.0).to_owned();
        }
    }
    all
}

/// The library category of a built-in symbol.
fn category_of(id: &str) -> &'static str {
    let name = id.strip_prefix("iec.").unwrap_or(id);
    match name {
        "coil" | "coil_contactor" | "coil_relay" | "solenoid_valve" => "Coils and valves",
        "contact_no" | "contact_nc" | "contact_main_3" => "Contacts",
        n if n.starts_with("pushbutton") || n.starts_with("key_") || n.starts_with("selector") => "Operator devices",
        "emergency_stop_nc" | "illuminated_pushbutton_no" => "Operator devices",
        n if n.contains("switch_") || n.starts_with("proximity") => "Sensors and switches",
        n if n.starts_with("plc_") => "PLC",
        n if n.starts_with("breaker") || n.starts_with("motor_protection") || n.starts_with("fuse") => "Protection",
        "disconnector_fuse_3" => "Protection",
        "lamp" | "siren" => "Signalling",
        n if n.starts_with("motor") || n == "power_supply" || n == "socket_2pe" => "Loads and supplies",
        _ => "Wiring",
    }
}

fn basics() -> Vec<SymbolDef> {
    vec![
        SymbolDef {
            id: SymbolId::new("iec.coil"),
            description: "Relay or contactor coil".into(),
            tag_prefix: "-K".into(),
            role: Role::Main,
            bridged: false,
            hide_tag: false,
            hide_point_labels: false,
            category: String::new(),
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
            category: String::new(),
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
            category: String::new(),
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
            category: String::new(),
            graphics: vec![line(-2.0, 0.0, 2.0, 0.0)],
            connection_points: vec![cp("1", 0.0, 0.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.terminal"),
            description: "Terminal".into(),
            tag_prefix: "-X".into(),
            role: Role::Terminal,
            // A feed-through terminal: both sides are the same potential.
            bridged: true,
            hide_tag: false,
            hide_point_labels: true,
            category: String::new(),
            graphics: vec![
                Graphic::Circle { center: Point::ZERO, radius: 1.2 },
                line(0.0, 1.2, 0.0, 4.0),
                line(0.0, -1.2, 0.0, -4.0),
            ],
            // Top is the inside of the cabinet, bottom the field side.
            connection_points: vec![cp("1", 0.0, 4.0, Direction::Up), cp("2", 0.0, -4.0, Direction::Down)],
        },
        SymbolDef {
            id: SymbolId::new("iec.junction"),
            description: "Junction".into(),
            tag_prefix: "J".into(),
            role: Role::Single,
            bridged: true,
            hide_tag: true,
            hide_point_labels: true,
            category: String::new(),
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
            category: String::new(),
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

fn dashed(ax: f64, ay: f64, bx: f64, by: f64) -> Graphic {
    Graphic::Dashed { a: Point::new(ax, ay), b: Point::new(bx, by) }
}

fn rect(ax: f64, ay: f64, bx: f64, by: f64) -> Vec<Graphic> {
    vec![line(ax, ay, bx, ay), line(bx, ay, bx, by), line(bx, by, ax, by), line(ax, by, ax, ay)]
}

/// Pole spacing of multi-pole symbols, in mm.
pub const POLE: f64 = 8.0;

/// Connection points of `n` poles: top names leave up at y = 10, bottom names leave down at
/// y = -10 (room for the point labels clear of release boxes and fuses), pole `i` at x = 8·i. Listed top, bottom, top, bottom… so pairs are poles.
fn pole_points(names: &[(&str, &str)]) -> Vec<ConnectionPointDef> {
    names
        .iter()
        .enumerate()
        .flat_map(|(i, (top, bottom))| {
            let x = POLE * i as f64;
            [cp(top, x, 10.0, Direction::Up), cp(bottom, x, -10.0, Direction::Down)]
        })
        .collect()
}

/// An open switching pole at `x`: terminals, and a blade from the lower contact up to the left.
fn open_pole(x: f64) -> Vec<Graphic> {
    vec![line(x, 10.0, x, 3.0), line(x, -3.0, x - 3.0, 3.0), line(x, -3.0, x, -10.0)]
}

/// Multi-pole switching devices drawn as one symbol, with the dashed link between their
/// blades: contactor main contacts, circuit breakers, a motor-protection breaker, a residual-
/// current breaker and a switch-disconnector with fuses.
fn power_symbols() -> Vec<SymbolDef> {
    let three = [("1", "2"), ("3", "4"), ("5", "6")];
    let poles = |n: usize, extra: &dyn Fn(f64) -> Vec<Graphic>| -> Vec<Graphic> {
        (0..n)
            .flat_map(|i| {
                let x = POLE * i as f64;
                let mut g = open_pole(x);
                g.extend(extra(x));
                g
            })
            .collect()
    };
    // The breaker mark: a small cross on the fixed contact.
    let cross = |x: f64| vec![line(x - 0.8, 2.2, x + 0.8, 3.8), line(x - 0.8, 3.8, x + 0.8, 2.2)];
    let link = |n: usize| dashed(-6.0, 0.0, POLE * (n - 1) as f64 - 1.5, 0.0);
    let def = |id: &str, description: &str, prefix: &str, role: Role, graphics: Vec<Graphic>, points| SymbolDef {
        id: SymbolId::new(id),
        description: description.into(),
        tag_prefix: prefix.into(),
        role,
        bridged: false,
        hide_tag: false,
        hide_point_labels: false,
        category: String::new(),
        graphics,
        connection_points: points,
    };
    let mut contactor = poles(3, &|_| Vec::new());
    contactor.push(link(3));
    let mut breaker = poles(3, &cross);
    breaker.push(link(3));
    let mut motor = poles(3, &cross);
    motor.push(link(3));
    // Thermal and magnetic releases: one box across the poles, marked I>.
    motor.extend(rect(-2.0, -4.0, POLE * 2.0 + 2.0, -6.5));
    motor.push(Graphic::Text { at: Point::new(-7.5, -6.2), height: 2.0, value: "I>".into() });
    let mut rcd = poles(4, &cross);
    rcd.push(link(4));
    rcd.extend(rect(-2.0, -4.0, POLE * 3.0 + 2.0, -6.5));
    rcd.push(Graphic::Text { at: Point::new(-7.5, -6.2), height: 2.0, value: "ID".into() });
    // Switch-disconnector: a bar on each fixed contact, and a fuse below each blade.
    let mut disconnector: Vec<Graphic> = (0..3)
        .flat_map(|i| {
            let x = POLE * i as f64;
            let mut g = vec![line(x, 10.0, x, 4.0), line(x - 1.0, 4.0, x + 1.0, 4.0), line(x, -0.5, x - 3.0, 4.0)];
            g.extend(rect(x - 1.0, -1.5, x + 1.0, -6.5));
            g.push(line(x, -0.5, x, -10.0));
            g
        })
        .collect();
    disconnector.push(dashed(-6.0, 2.0, POLE * 2.0 - 1.5, 2.0));
    vec![
        def(
            "iec.contact_main_3",
            "Main contacts, 3 poles",
            "-K",
            Role::Contact { kind: ContactKind::Power },
            contactor,
            pole_points(&three),
        ),
        def("iec.breaker_3", "Circuit breaker, 3 poles", "-QF", Role::Main, breaker, pole_points(&three)),
        def(
            "iec.motor_protection_3",
            "Motor-protection circuit breaker, 3 poles",
            "-QF",
            Role::Main,
            motor,
            pole_points(&three),
        ),
        def(
            "iec.breaker_3pn_rcd",
            "Residual-current circuit breaker, 3 poles + N",
            "-QF",
            Role::Main,
            rcd,
            pole_points(&[("1", "2"), ("3", "4"), ("5", "6"), ("N", "N'")]),
        ),
        def(
            "iec.disconnector_fuse_3",
            "Switch-disconnector with fuses, 3 poles",
            "-QS",
            Role::Main,
            disconnector,
            pole_points(&three),
        ),
    ]
}

/// One symbol per PLC channel kind: a box with the kind written in it. Inputs are fed from
/// above (the sensor sits on the path above), outputs drive the load below.
fn plc_channel_symbols() -> Vec<SymbolDef> {
    [
        ("iec.plc_di", "PLC digital input", IoKind::DigitalIn),
        ("iec.plc_do", "PLC digital output", IoKind::DigitalOut),
        ("iec.plc_ai", "PLC analog input", IoKind::AnalogIn),
        ("iec.plc_ao", "PLC analog output", IoKind::AnalogOut),
    ]
    .into_iter()
    .map(|(id, description, kind)| {
        let input = matches!(kind, IoKind::DigitalIn | IoKind::AnalogIn);
        let (y, leaves): (f64, _) = if input { (6.0, Direction::Up) } else { (-6.0, Direction::Down) };
        SymbolDef {
            id: SymbolId::new(id),
            description: description.into(),
            tag_prefix: "-KF".into(),
            role: Role::PlcChannel { kind },
            bridged: false,
            hide_tag: false,
            hide_point_labels: true,
            category: String::new(),
            graphics: vec![
                line(-4.0, -3.0, 4.0, -3.0),
                line(4.0, -3.0, 4.0, 3.0),
                line(4.0, 3.0, -4.0, 3.0),
                line(-4.0, 3.0, -4.0, -3.0),
                line(0.0, y.signum() * 3.0, 0.0, y),
                Graphic::Text { at: Point::new(-1.8, -1.25), height: 2.5, value: kind.short().into() },
            ],
            connection_points: vec![cp("1", 0.0, y, leaves)],
        }
    })
    .collect()
}

/// A symbol with the usual flags: drawn, tagged and labelled.
fn sym(
    id: &str,
    description: &str,
    prefix: &str,
    role: Role,
    graphics: Vec<Graphic>,
    points: Vec<ConnectionPointDef>,
) -> SymbolDef {
    SymbolDef {
        id: SymbolId::new(id),
        description: description.into(),
        tag_prefix: prefix.into(),
        role,
        bridged: false,
        hide_tag: false,
        hide_point_labels: false,
        category: String::new(),
        graphics,
        connection_points: points,
    }
}

fn text(x: f64, y: f64, height: f64, value: &str) -> Graphic {
    Graphic::Text { at: Point::new(x, y), height, value: value.into() }
}

fn circle(x: f64, y: f64, r: f64) -> Graphic {
    Graphic::Circle { center: Point::new(x, y), radius: r }
}

/// An arc as short lines, angles in degrees counter-clockwise from +x.
fn arc(cx: f64, cy: f64, r: f64, from: f64, to: f64) -> Vec<Graphic> {
    let n = 8;
    let at = |i: usize| {
        let a = (from + (to - from) * i as f64 / n as f64).to_radians();
        (cx + r * a.cos(), cy + r * a.sin())
    };
    (0..n)
        .map(|i| {
            let (a, b) = (at(i), at(i + 1));
            line(a.0, a.1, b.0, b.1)
        })
        .collect()
}

/// A vertical contact with points at ±8: NO (blade from the upper contact out to the left) or
/// NC (blade crossing to the right under a hook). Returns the graphics and the x where an
/// operator's link meets the blade.
fn contact_body(kind: ContactKind) -> (Vec<Graphic>, f64) {
    match kind {
        ContactKind::NormallyClosed => (
            vec![
                line(0.0, 8.0, 0.0, 3.0),
                line(0.0, 3.0, 2.5, 3.0),
                line(0.0, -3.0, 3.5, 4.0),
                line(0.0, -3.0, 0.0, -8.0),
            ],
            1.6,
        ),
        _ => (vec![line(0.0, 8.0, 0.0, 3.0), line(0.0, 3.0, -3.0, -3.0), line(0.0, -3.0, 0.0, -8.0)], -1.5),
    }
}

fn contact_points(kind: ContactKind) -> Vec<ConnectionPointDef> {
    let (a, b) = if kind == ContactKind::NormallyClosed { ("11", "12") } else { ("13", "14") };
    vec![cp(a, 0.0, 8.0, Direction::Up), cp(b, 0.0, -8.0, Direction::Down)]
}

/// A contact worked by an operator drawn at x = -6: the operator's graphics, then a dashed link
/// to the blade.
fn operated(id: &str, description: &str, prefix: &str, kind: ContactKind, head: Vec<Graphic>) -> SymbolDef {
    let (mut g, blade) = contact_body(kind);
    g.push(dashed(-6.0, 0.0, blade, 0.0));
    g.extend(head);
    sym(id, description, prefix, Role::Contact { kind }, g, contact_points(kind))
}

/// The pushbutton operator: a bracket open to the left.
fn push_head() -> Vec<Graphic> {
    vec![line(-6.0, -1.5, -6.0, 1.5), line(-6.0, 1.5, -7.0, 1.5), line(-6.0, -1.5, -7.0, -1.5)]
}

/// A box with a quantity in it, for switches worked by pressure, temperature…
fn sensing_head(label: &str) -> Vec<Graphic> {
    let mut g = rect(-11.0, -1.75, -6.0, 1.75);
    g.push(text(-10.4, -0.9, 1.8, label));
    g
}

/// Pushbuttons, key and selector switches, emergency stop and an illuminated pushbutton.
fn operator_symbols() -> Vec<SymbolDef> {
    use ContactKind::{NormallyClosed as Nc, NormallyOpen as No};
    let key_head = vec![
        circle(-7.2, 0.0, 1.0),
        line(-6.2, 0.0, -6.0, 0.0),
        line(-8.2, 0.0, -9.0, 0.0),
        line(-9.0, 0.0, -9.0, -1.0),
    ];
    let turn_head = vec![line(-7.0, 1.5, -6.0, 1.5), line(-6.0, 1.5, -6.0, -1.5), line(-6.0, -1.5, -5.0, -1.5)];
    // A mushroom head that latches.
    let mut mushroom = arc(-6.0, 0.0, 2.2, 90.0, 270.0);
    mushroom.push(line(-6.0, 2.2, -6.0, -2.2));
    // The illuminated pushbutton: an NO contact with a lamp beside it, X1 above X2.
    let (mut lit, blade) = contact_body(No);
    lit.push(dashed(-6.0, 0.0, blade, 0.0));
    lit.extend(push_head());
    lit.extend(lamp_graphics(8.0));
    let mut lit_points = contact_points(No);
    lit_points.extend([cp("X1", 8.0, 8.0, Direction::Up), cp("X2", 8.0, -8.0, Direction::Down)]);
    vec![
        operated("iec.pushbutton_no", "Pushbutton, normally open", "-SB", No, push_head()),
        operated("iec.pushbutton_nc", "Pushbutton, normally closed", "-SB", Nc, push_head()),
        operated("iec.key_switch_no", "Key switch, normally open", "-SA", No, key_head),
        operated("iec.selector_no", "Selector switch (stays put), normally open", "-SA", No, turn_head),
        operated("iec.emergency_stop_nc", "Emergency stop, latching, normally closed", "-SB", Nc, mushroom),
        sym(
            "iec.illuminated_pushbutton_no",
            "Illuminated pushbutton, normally open",
            "-SH",
            Role::Single,
            lit,
            lit_points,
        ),
    ]
}

/// Limit, pressure, temperature and level switches and a magnetic proximity sensor.
fn sensor_symbols() -> Vec<SymbolDef> {
    use ContactKind::{NormallyClosed as Nc, NormallyOpen as No};
    // A limit switch's cam: a small triangle on the blade, no operator.
    let limit = |kind: ContactKind, id: &str, description: &str| {
        let (mut g, blade) = contact_body(kind);
        g.extend([
            line(blade, 0.0, blade - 1.6, 1.2),
            line(blade - 1.6, 1.2, blade - 1.6, -1.2),
            line(blade - 1.6, -1.2, blade, 0.0),
        ]);
        sym(id, description, "-SQ", Role::Contact { kind }, g, contact_points(kind))
    };
    let mut float = vec![circle(-8.0, 0.0, 1.5)];
    float.push(line(-10.5, -1.5, -5.5, -1.5));
    let mut proximity = vec![
        line(-8.0, 2.0, -6.0, 0.0),
        line(-6.0, 0.0, -8.0, -2.0),
        line(-8.0, -2.0, -10.0, 0.0),
        line(-10.0, 0.0, -8.0, 2.0),
    ];
    proximity.extend([line(-12.0, 1.5, -12.0, -1.5), line(-12.0, 1.5, -11.2, 1.5), line(-12.0, -1.5, -11.2, -1.5)]);
    vec![
        limit(No, "iec.limit_switch_no", "Limit switch, normally open"),
        limit(Nc, "iec.limit_switch_nc", "Limit switch, normally closed"),
        operated("iec.pressure_switch_no", "Pressure switch, normally open", "-SP", No, sensing_head("P>")),
        operated("iec.temperature_switch_no", "Thermostat, normally open", "-ST", No, sensing_head("t>")),
        operated("iec.level_switch_no", "Level switch (float), normally open", "-SL", No, float),
        operated("iec.proximity_magnetic_nc", "Magnetic proximity sensor, normally closed", "-BG", Nc, proximity),
    ]
}

/// A signal lamp centred at (x, 0) between points at ±8.
fn lamp_graphics(x: f64) -> Vec<Graphic> {
    let d = 2.5 * std::f64::consts::FRAC_1_SQRT_2;
    vec![
        circle(x, 0.0, 2.5),
        line(x - d, -d, x + d, d),
        line(x - d, d, x + d, -d),
        line(x, 2.5, x, 8.0),
        line(x, -2.5, x, -8.0),
    ]
}

/// Lamp, siren, contactor and relay coils, solenoid valve, motors, fuses, power supply, socket.
fn load_symbols() -> Vec<SymbolDef> {
    let up_down = |a: &str, b: &str| vec![cp(a, 0.0, 8.0, Direction::Up), cp(b, 0.0, -8.0, Direction::Down)];
    let coil = || {
        let mut g = rect(-5.0, -3.0, 5.0, 3.0);
        g.extend([line(0.0, 3.0, 0.0, 8.0), line(0.0, -3.0, 0.0, -8.0)]);
        g
    };
    let mut siren = rect(-2.5, -2.0, 2.5, 2.0);
    siren.extend([line(2.5, 1.0, 5.5, 3.0), line(5.5, 3.0, 5.5, -3.0), line(5.5, -3.0, 2.5, -1.0)]);
    siren.extend([line(0.0, 2.0, 0.0, 8.0), line(0.0, -2.0, 0.0, -8.0)]);
    let mut valve = coil();
    valve.push(dashed(5.0, 0.0, 9.0, 0.0));
    valve.extend([
        line(9.0, -2.0, 13.0, 2.0),
        line(13.0, 2.0, 13.0, -2.0),
        line(13.0, -2.0, 9.0, 2.0),
        line(9.0, 2.0, 9.0, -2.0),
    ]);
    // Motors: a circle under the supply points, PE on the right.
    let motor = |phases: &[(&str, f64)], label: &str| -> (Vec<Graphic>, Vec<ConnectionPointDef>) {
        // Centred on the grid, so PE (beside it) is on the grid too.
        let (cx, cy, r) = (0.0, -8.0, 6.0);
        let mut g = vec![circle(cx, cy, r), text(-1.6, -8.5, 3.0, "M"), text(-1.8, -12.0, 1.8, label)];
        let mut points = Vec::new();
        for (name, x) in phases {
            // Down from the point, then straight to the circle.
            let edge_x = x.clamp(-r * 0.7, r * 0.7);
            let edge_y = cy + (r * r - edge_x * edge_x).sqrt();
            g.push(line(*x, 8.0, *x, 3.0));
            g.push(line(*x, 3.0, edge_x, edge_y));
            points.push(cp(name, *x, 8.0, Direction::Up));
        }
        g.extend([line(r, cy, 12.0, cy), line(10.5, cy - 1.0, 13.5, cy - 1.0), line(11.2, cy - 1.7, 12.8, cy - 1.7)]);
        points.push(cp("PE", 12.0, cy, Direction::Right));
        (g, points)
    };
    let (m3, m3_points) = motor(&[("U", -8.0), ("V", 0.0), ("W", 8.0)], "3~");
    let (m1, m1_points) = motor(&[("1", -4.0), ("2", 4.0)], "1~");
    let fuse_pole = |x: f64| {
        let mut g = rect(x - 1.0, -3.5, x + 1.0, 3.5);
        g.push(line(x, 10.0, x, -10.0));
        g
    };
    let fuse_points = |names: &[(&str, &str)]| -> Vec<ConnectionPointDef> {
        names
            .iter()
            .enumerate()
            .flat_map(|(i, (a, b))| {
                let x = POLE * i as f64;
                [cp(a, x, 10.0, Direction::Up), cp(b, x, -10.0, Direction::Down)]
            })
            .collect()
    };
    let fuse3: Vec<Graphic> = (0..3).flat_map(|i| fuse_pole(POLE * i as f64)).collect();
    let mut fuse_pn: Vec<Graphic> = (0..2).flat_map(|i| fuse_pole(POLE * i as f64)).collect();
    fuse_pn.push(dashed(-5.0, 0.0, -1.0, 0.0));
    fuse_pn.push(dashed(1.0, 0.0, POLE - 1.0, 0.0));
    // Power supply: a box with a diagonal, AC in, DC out.
    let mut psu = rect(-10.0, -6.0, 10.0, 6.0);
    psu.extend([line(-10.0, -6.0, 10.0, 6.0), text(-8.5, 2.5, 2.0, "AC"), text(4.5, -4.5, 2.0, "DC")]);
    psu.extend([
        line(-4.0, 6.0, -4.0, 10.0),
        line(4.0, 6.0, 4.0, 10.0),
        line(-4.0, -6.0, -4.0, -10.0),
        line(4.0, -6.0, 4.0, -10.0),
    ]);
    let psu_points = vec![
        cp("L", -4.0, 10.0, Direction::Up),
        cp("N", 4.0, 10.0, Direction::Up),
        cp("+", -4.0, -10.0, Direction::Down),
        cp("-", 4.0, -10.0, Direction::Down),
    ];
    // Socket 2P+E: three pins into a socket outline.
    let mut socket: Vec<Graphic> = [-8.0, 0.0, 8.0].iter().map(|x| line(*x, 8.0, *x, 1.0)).collect();
    socket.extend(arc(0.0, 1.0, 10.0, 180.0, 360.0));
    socket.push(line(-10.0, 1.0, 10.0, 1.0));
    let socket_points =
        vec![cp("L", -8.0, 8.0, Direction::Up), cp("N", 0.0, 8.0, Direction::Up), cp("PE", 8.0, 8.0, Direction::Up)];
    vec![
        sym("iec.coil_contactor", "Contactor coil", "-KM", Role::Main, coil(), up_down("A1", "A2")),
        sym("iec.coil_relay", "Auxiliary relay coil", "-KA", Role::Main, coil(), up_down("A1", "A2")),
        sym("iec.solenoid_valve", "Solenoid valve", "-YV", Role::Single, valve, up_down("A1", "A2")),
        sym("iec.lamp", "Signal lamp", "-HL", Role::Single, lamp_graphics(0.0), up_down("X1", "X2")),
        sym("iec.siren", "Siren", "-HA", Role::Single, siren, up_down("1", "2")),
        sym("iec.motor_3", "Three-phase motor", "-M", Role::Single, m3, m3_points),
        sym("iec.motor_1", "Single-phase motor (fan)", "-M", Role::Single, m1, m1_points),
        sym("iec.fuse", "Fuse", "-FU", Role::Single, fuse_pole(0.0), fuse_points(&[("1", "2")])),
        sym(
            "iec.fuse_3",
            "Fuses, 3 poles",
            "-FU",
            Role::Single,
            fuse3,
            fuse_points(&[("1", "2"), ("3", "4"), ("5", "6")]),
        ),
        sym(
            "iec.fuse_1pn",
            "Fuse holder 1P+N, disconnecting",
            "-FU",
            Role::Single,
            fuse_pn,
            fuse_points(&[("1", "2"), ("N", "N'")]),
        ),
        sym("iec.power_supply", "Power supply AC/DC", "-GD", Role::Single, psu, psu_points),
        sym("iec.socket_2pe", "Socket 2P+E", "-XS", Role::Single, socket, socket_points),
    ]
}

/// Terminal pitch of a black box, in mm.
pub const BOX_PITCH: f64 = 8.0;

/// What a black box looks like: its terminals on each side, named as on the device.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlackBox {
    /// Printed inside the box, e.g. "Safety module".
    pub description: String,
    /// Letter code for the tag, e.g. `-A` or `-KF` (with the leading `-`).
    pub tag_prefix: String,
    /// Terminal names along each side, left to right (top, bottom) or top to bottom (left,
    /// right).
    pub top: Vec<String>,
    pub bottom: Vec<String>,
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl BlackBox {
    /// Every terminal name, side by side; names must be unique and non-empty.
    pub fn is_valid(&self) -> bool {
        let names: Vec<&String> = self.top.iter().chain(&self.bottom).chain(&self.left).chain(&self.right).collect();
        let unique: std::collections::BTreeSet<&String> = names.iter().copied().collect();
        !names.is_empty() && unique.len() == names.len() && names.iter().all(|n| !n.trim().is_empty())
    }
}

/// A device drawn as a box with its own terminals, e.g. an inverter or a safety module. The box
/// is sized to its terminals, 8 mm apart; their names are written inside, next to each
/// terminal, and the description in the middle. Top terminals leave up, bottom ones down, and
/// so on, 4 mm out from the box.
pub fn black_box(id: &str, spec: &BlackBox) -> SymbolDef {
    let along = |n: usize| BOX_PITCH * n.max(1) as f64;
    let w = along(spec.top.len().max(spec.bottom.len()).max(2)) + BOX_PITCH;
    let h = along(spec.left.len().max(spec.right.len()).max(2)) + BOX_PITCH;
    let (x0, y0, x1, y1) = (0.0, -h, w, 0.0);
    let mut g = rect(x0, y0, x1, y1);
    let mut points = Vec::new();
    let stub = 4.0;
    let label = 1.8;
    for (i, name) in spec.top.iter().enumerate() {
        let x = BOX_PITCH * (i as f64 + 1.0);
        g.push(line(x, y1, x, y1 + stub));
        g.push(text(x - 0.9, y1 - 1.0 - label, label, name));
        points.push(cp(name, x, y1 + stub, Direction::Up));
    }
    for (i, name) in spec.bottom.iter().enumerate() {
        let x = BOX_PITCH * (i as f64 + 1.0);
        g.push(line(x, y0, x, y0 - stub));
        g.push(text(x - 0.9, y0 + 1.0, label, name));
        points.push(cp(name, x, y0 - stub, Direction::Down));
    }
    for (i, name) in spec.left.iter().enumerate() {
        let y = -BOX_PITCH * (i as f64 + 1.0);
        g.push(line(x0, y, x0 - stub, y));
        g.push(text(x0 + 1.0, y - label / 2.0, label, name));
        points.push(cp(name, x0 - stub, y, Direction::Left));
    }
    for (i, name) in spec.right.iter().enumerate() {
        let y = -BOX_PITCH * (i as f64 + 1.0);
        g.push(line(x1, y, x1 + stub, y));
        let width = name.chars().count() as f64 * label * 0.8;
        g.push(text(x1 - 1.0 - width, y - label / 2.0, label, name));
        points.push(cp(name, x1 + stub, y, Direction::Right));
    }
    if !spec.description.is_empty() {
        let size = 2.5;
        let width = spec.description.chars().count() as f64 * size * 0.8;
        g.push(text((x0 + x1 - width) / 2.0, (y0 + y1) / 2.0 - size / 2.0, size, &spec.description));
    }
    SymbolDef {
        id: SymbolId::new(id),
        description: if spec.description.is_empty() { "Black box".into() } else { spec.description.clone() },
        tag_prefix: if spec.tag_prefix.is_empty() { "-A".into() } else { spec.tag_prefix.clone() },
        role: Role::Main,
        bridged: false,
        hide_tag: false,
        // The names are written inside the box.
        hide_point_labels: true,
        category: "Black boxes".into(),
        graphics: g,
        connection_points: points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_library_symbol_is_well_formed() {
        let all = iec();
        let ids: std::collections::BTreeSet<&str> = all.iter().map(|s| s.id.0.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "unique ids");
        assert!(all.len() >= 40, "{}", all.len());
        for s in &all {
            assert!(!s.category.is_empty() && !s.description.is_empty(), "{}", s.id.0);
            assert!(!s.connection_points.is_empty(), "{} has points", s.id.0);
            let names: std::collections::BTreeSet<&str> = s.connection_points.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names.len(), s.connection_points.len(), "{}: unique point names", s.id.0);
            for c in &s.connection_points {
                let across = if c.leaves.is_vertical() { c.at.x } else { c.at.y };
                assert!(
                    (across / 4.0).fract().abs() < 1e-9,
                    "{}:{} at {:?} is off the 4 mm grid",
                    s.id.0,
                    c.name,
                    c.at
                );
            }
        }
    }

    #[test]
    fn black_boxes_are_sized_to_their_terminals() {
        let spec = BlackBox {
            description: "Inverter".into(),
            tag_prefix: "-TA".into(),
            top: vec!["L1".into(), "L2".into(), "L3".into(), "PE".into()],
            bottom: vec!["U".into(), "V".into(), "W".into()],
            left: Vec::new(),
            right: vec!["DI0".into(), "DI1".into()],
        };
        assert!(spec.is_valid());
        let s = black_box("box.1", &spec);
        assert_eq!((s.tag_prefix.as_str(), s.role, s.category.as_str()), ("-TA", Role::Main, "Black boxes"));
        assert_eq!(s.connection_points.len(), 9);
        let pe = s.connection_point("PE").unwrap();
        assert_eq!((pe.at, pe.leaves), (Point::new(32.0, 4.0), Direction::Up));
        let (lo, hi) = s.bounds().unwrap();
        assert!(hi.x - lo.x >= 40.0, "four top terminals fit");
        assert!(!BlackBox { top: vec!["1".into(), "1".into()], ..BlackBox::default() }.is_valid(), "duplicate names");
        assert!(!BlackBox::default().is_valid(), "no terminals");
    }
}
