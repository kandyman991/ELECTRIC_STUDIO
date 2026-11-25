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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolDef {
    pub id: SymbolId,
    pub description: String,
    pub graphics: Vec<Graphic>,
    pub connection_points: Vec<ConnectionPointDef>,
}

impl SymbolDef {
    pub fn connection_point(&self, name: &str) -> Option<&ConnectionPointDef> {
        self.connection_points.iter().find(|c| c.name == name)
    }
}
