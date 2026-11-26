//! Symbol definitions: geometry plus named connection points, in symbol-local millimetres.

use crate::geometry::{Direction, Point};
use serde::{Deserialize, Serialize};

/// A symbol's identity in the library, e.g. `iec.coil`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub String);

impl SymbolId {
    pub fn new(s: &str) -> Self {
        SymbolId(s.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Graphic {
    Line {
        a: Point,
        b: Point,
    },
    /// A dashed line, e.g. the mechanical link between the poles of a switch.
    Dashed {
        a: Point,
        b: Point,
    },
    Circle {
        center: Point,
        radius: f64,
    },
    Text {
        at: Point,
        height: f64,
        value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectionPointDef {
    /// Designation printed beside the point, e.g. `A1`, `13`.
    pub name: String,
    pub at: Point,
    pub leaves: Direction,
}

/// The kind of contact a contact symbol draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactKind {
    NormallyOpen,
    NormallyClosed,
    /// Main (power) contacts of a contactor, numbered 1-2, 3-4, 5-6 by the symbol itself.
    Power,
}

impl ContactKind {
    /// `NO` or `NC`, as printed in contact lists.
    pub fn short(self) -> &'static str {
        match self {
            ContactKind::NormallyOpen => "NO",
            ContactKind::NormallyClosed => "NC",
            ContactKind::Power => "main",
        }
    }
}

/// The kind of a PLC channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoKind {
    DigitalIn,
    DigitalOut,
    AnalogIn,
    AnalogOut,
}

impl IoKind {
    /// `DI`, `DO`, `AI` or `AO`.
    pub fn short(self) -> &'static str {
        match self {
            IoKind::DigitalIn => "DI",
            IoKind::DigitalOut => "DO",
            IoKind::AnalogIn => "AI",
            IoKind::AnalogOut => "AO",
        }
    }
}

/// What a symbol stands for within its device, which decides cross-references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Role {
    /// A device drawn once, e.g. a terminal or a supply point.
    #[default]
    Single,
    /// A device's main function, e.g. a relay or contactor coil. It lists its contacts.
    Main,
    /// A contact of a device; it refers back to the device's main function.
    Contact { kind: ContactKind },
    /// A potential definition point: its device's tag is the potential's name (`L+`, `M`,
    /// `L1`), and all its drawings, on any page, are the same potential.
    Potential,
    /// An interruption point: all drawings of its device are connected, so a potential
    /// continues on another page. Each shows where its partners are.
    Interruption,
    /// One terminal of a terminal strip: the strip is the device (`-X2`), each terminal is
    /// one drawing with its own number (`-X2:3`). Its two sides are bridged.
    Terminal,
    /// One channel of a PLC card: the card is the device (`-KF1`), each channel is one
    /// drawing with its own number (`-KF1:3`), an address (`I0.3`) and a symbol name.
    PlcChannel { kind: IoKind },
}

impl Role {
    /// Roles whose drawings carry a number unique on their device (terminals, PLC channels).
    pub fn numbered(self) -> bool {
        matches!(self, Role::Terminal | Role::PlcChannel { .. })
    }
}

impl Role {
    /// Whether all drawings of a device with this role are one potential.
    pub fn joins_drawings(self) -> bool {
        matches!(self, Role::Potential | Role::Interruption)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolDef {
    pub id: SymbolId,
    pub description: String,
    /// Prefix for new device tags, e.g. `-K` gives `-K1`, `-K2`…
    #[serde(default)]
    pub tag_prefix: String,
    #[serde(default)]
    pub role: Role,
    /// All connection points are internally connected (a junction).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bridged: bool,
    /// Don't print the device tag beside the symbol (junctions).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_tag: bool,
    /// Don't print connection-point designations (junctions, potential and interruption points).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_point_labels: bool,
    /// Where the symbol is listed in the library, e.g. "Contacts"; empty for "Other".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    pub graphics: Vec<Graphic>,
    pub connection_points: Vec<ConnectionPointDef>,
}

impl SymbolDef {
    /// Bounding box of the geometry and connection points in symbol-local coordinates
    /// (`min`, `max`), or `None` for an empty symbol.
    pub fn bounds(&self) -> Option<(Point, Point)> {
        let mut pts: Vec<Point> = self.connection_points.iter().map(|c| c.at).collect();
        for g in &self.graphics {
            match g {
                Graphic::Line { a, b } | Graphic::Dashed { a, b } => pts.extend([*a, *b]),
                Graphic::Circle { center, radius } => {
                    pts.push(Point::new(center.x - radius, center.y - radius));
                    pts.push(Point::new(center.x + radius, center.y + radius));
                }
                Graphic::Text { at, .. } => pts.push(*at),
            }
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
        }))
    }

    pub fn connection_point(&self, name: &str) -> Option<&ConnectionPointDef> {
        self.connection_points.iter().find(|c| c.name == name)
    }
}
