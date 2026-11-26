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
    Line { a: Point, b: Point },
    Circle { center: Point, radius: f64 },
    Text { at: Point, height: f64, value: String },
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
}

impl ContactKind {
    /// `NO` or `NC`, as printed in contact lists.
    pub fn short(self) -> &'static str {
        match self {
            ContactKind::NormallyOpen => "NO",
            ContactKind::NormallyClosed => "NC",
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
                Graphic::Line { a, b } => pts.extend([*a, *b]),
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
