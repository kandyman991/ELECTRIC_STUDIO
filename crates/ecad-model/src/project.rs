//! The project: pages, symbols, devices, their functions on pages, and the connections between
//! connection points. Connections refer to connection points, never to coordinates, so moving a
//! symbol can never break a wire.

use crate::geometry::{Direction, Point, Rotation};
use crate::symbol::{SymbolDef, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// A fresh random id.
            pub fn new() -> Self {
                $name(Uuid::new_v4())
            }
            /// A fixed id, for reproducible samples and tests.
            pub const fn from_u128(v: u128) -> Self {
                $name(Uuid::from_u128(v))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

id_type!(PageId);
id_type!(
    /// A real component. Its tag can change; its id never does.
    DeviceId
);
id_type!(
    /// One drawing of a device on a page (a coil here, its contacts on other pages).
    FunctionId
);
id_type!(ConnectionId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    /// IEC 81346 reference designation, e.g. `-K1` or `=A1+B2-K1`.
    pub tag: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub id: FunctionId,
    pub device: DeviceId,
    pub symbol: SymbolId,
    pub page: PageId,
    pub position: Point,
    pub rotation: Rotation,
}

/// A connection point of a function, by its designation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRef {
    pub function: FunctionId,
    pub point: String,
}

impl ConnectionRef {
    pub fn new(function: FunctionId, point: &str) -> Self {
        ConnectionRef {
            function,
            point: point.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub from: ConnectionRef,
    pub to: ConnectionRef,
    pub wire_number: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("no page {0:?}")]
    NoPage(PageId),
    #[error("no device {0:?}")]
    NoDevice(DeviceId),
    #[error("no function {0:?}")]
    NoFunction(FunctionId),
    #[error("no symbol `{}`", .0.0)]
    NoSymbol(SymbolId),
    #[error("symbol `{symbol}` has no connection point `{point}`")]
    NoConnectionPoint { symbol: String, point: String },
    #[error("a connection must join two different connection points")]
    SelfConnection,
    #[error("connection point {}:{} is already connected to {}:{}", .0.0, .0.1, .1.0, .1.1)]
    AlreadyConnected((String, String), (String, String)),
    #[error("both ends of a connection must be on the same page (interruption points come in M2)")]
    CrossPage,
    #[error("device tag `{0}` is already used")]
    DuplicateTag(String),
}

pub type Result<T> = std::result::Result<T, ModelError>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub pages: Vec<Page>,
    pub symbols: BTreeMap<SymbolId, SymbolDef>,
    pub devices: Vec<Device>,
    pub functions: Vec<Function>,
    pub connections: Vec<Connection>,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Project {
            name: name.to_owned(),
            ..Project::default()
        }
    }

    // ---- lookups ----

    pub fn page(&self, id: PageId) -> Result<&Page> {
        self.pages
            .iter()
            .find(|p| p.id == id)
            .ok_or(ModelError::NoPage(id))
    }
    pub fn device(&self, id: DeviceId) -> Result<&Device> {
        self.devices
            .iter()
            .find(|d| d.id == id)
            .ok_or(ModelError::NoDevice(id))
    }
    pub fn function(&self, id: FunctionId) -> Result<&Function> {
        self.functions
            .iter()
            .find(|f| f.id == id)
            .ok_or(ModelError::NoFunction(id))
    }
    pub fn symbol(&self, id: &SymbolId) -> Result<&SymbolDef> {
        self.symbols
            .get(id)
            .ok_or_else(|| ModelError::NoSymbol(id.clone()))
    }
    pub fn functions_on(&self, page: PageId) -> impl Iterator<Item = &Function> {
        self.functions.iter().filter(move |f| f.page == page)
    }
    /// Connections drawn on a page (both ends are always on the same page in M0).
    pub fn connections_on(&self, page: PageId) -> impl Iterator<Item = &Connection> {
        self.connections
            .iter()
            .filter(move |c| self.function(c.from.function).is_ok_and(|f| f.page == page))
    }

    /// `(device tag, point)` for display and reports.
    pub fn describe(&self, r: &ConnectionRef) -> Result<(String, String)> {
        let f = self.function(r.function)?;
        Ok((self.device(f.device)?.tag.clone(), r.point.clone()))
    }

    // ---- edits (become undoable commands in M1) ----

    pub fn add_page(&mut self, page: Page) -> PageId {
        let id = page.id;
        self.pages.push(page);
        id
    }

    pub fn add_symbol(&mut self, symbol: SymbolDef) {
        self.symbols.insert(symbol.id.clone(), symbol);
    }

    pub fn add_device(&mut self, device: Device) -> Result<DeviceId> {
        if self.devices.iter().any(|d| d.tag == device.tag) {
            return Err(ModelError::DuplicateTag(device.tag));
        }
        let id = device.id;
        self.devices.push(device);
        Ok(id)
    }

    pub fn place(&mut self, function: Function) -> Result<FunctionId> {
        self.page(function.page)?;
        self.device(function.device)?;
        self.symbol(&function.symbol)?;
        let id = function.id;
        self.functions.push(function);
        Ok(id)
    }

    pub fn connect(&mut self, connection: Connection) -> Result<ConnectionId> {
        if connection.from == connection.to {
            return Err(ModelError::SelfConnection);
        }
        self.connection_point(&connection.from)?;
        self.connection_point(&connection.to)?;
        if self.function(connection.from.function)?.page
            != self.function(connection.to.function)?.page
        {
            return Err(ModelError::CrossPage);
        }
        for end in [&connection.from, &connection.to] {
            if let Some(other) = self
                .connections
                .iter()
                .find(|c| c.from == *end || c.to == *end)
            {
                let far = if other.from == *end {
                    &other.to
                } else {
                    &other.from
                };
                return Err(ModelError::AlreadyConnected(
                    self.describe(end)?,
                    self.describe(far)?,
                ));
            }
        }
        let id = connection.id;
        self.connections.push(connection);
        Ok(id)
    }

    pub fn move_function(&mut self, id: FunctionId, to: Point) -> Result<()> {
        let f = self
            .functions
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or(ModelError::NoFunction(id))?;
        f.position = to;
        Ok(())
    }

    // ---- geometry derived from the model ----

    /// Where a connection point sits on its page, and which way a wire leaves it.
    pub fn connection_point(&self, r: &ConnectionRef) -> Result<(Point, Direction)> {
        let f = self.function(r.function)?;
        let s = self.symbol(&f.symbol)?;
        let cp = s
            .connection_point(&r.point)
            .ok_or_else(|| ModelError::NoConnectionPoint {
                symbol: s.id.0.clone(),
                point: r.point.clone(),
            })?;
        Ok((
            f.position.offset(f.rotation.apply(cp.at)),
            f.rotation.apply_dir(cp.leaves),
        ))
    }

    /// Orthogonal route from one end to the other. Straight when the ends face each other on one
    /// line; otherwise each end leaves along its own direction for [`ROUTE_STUB`] before the
    /// route turns, so a wire never doubles back through its own symbol.
    pub fn route(&self, c: &Connection) -> Result<Vec<Point>> {
        let (a, da) = self.connection_point(&c.from)?;
        let (b, db) = self.connection_point(&c.to)?;
        Ok(route_between(a, da, b, db))
    }
}

/// How far a wire runs straight out of a connection point before it may turn (one 4 mm grid step).
pub const ROUTE_STUB: f64 = 4.0;

fn step(d: Direction) -> Point {
    match d {
        Direction::Up => Point::new(0.0, 1.0),
        Direction::Down => Point::new(0.0, -1.0),
        Direction::Left => Point::new(-1.0, 0.0),
        Direction::Right => Point::new(1.0, 0.0),
    }
}

/// Whether moving from `from` to `to` goes the way `d` points (or nowhere along its axis).
fn heads(d: Direction, from: Point, to: Point) -> bool {
    let s = step(d);
    (to.x - from.x) * s.x + (to.y - from.y) * s.y >= 0.0
}

fn route_between(a: Point, da: Direction, b: Point, db: Direction) -> Vec<Point> {
    let facing = (a.x == b.x && da.is_vertical() && db.is_vertical())
        || (a.y == b.y && !da.is_vertical() && !db.is_vertical());
    if facing && heads(da, a, b) && heads(db, b, a) {
        return vec![a, b];
    }
    let (sa, sb) = (step(da), step(db));
    let a1 = Point::new(a.x + sa.x * ROUTE_STUB, a.y + sa.y * ROUTE_STUB);
    let b1 = Point::new(b.x + sb.x * ROUTE_STUB, b.y + sb.y * ROUTE_STUB);
    // Keep going along the start direction if that brings us closer (split the other axis at
    // its midpoint); if it would lead away, turn first.
    let vertical_mid = if heads(da, a1, b1) {
        da.is_vertical()
    } else {
        !da.is_vertical()
    };
    let mid = if vertical_mid {
        let my = (a1.y + b1.y) / 2.0;
        [Point::new(a1.x, my), Point::new(b1.x, my)]
    } else {
        let mx = (a1.x + b1.x) / 2.0;
        [Point::new(mx, a1.y), Point::new(mx, b1.y)]
    };
    simplify(vec![a, a1, mid[0], mid[1], b1, b])
}

/// Drop repeated points and points in the middle of a straight run.
fn simplify(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (q, r) = (out[out.len() - 2], out[out.len() - 1]);
            if (q.x == r.x && r.x == p.x) || (q.y == r.y && r.y == p.y) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}
