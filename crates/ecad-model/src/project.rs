//! The project: pages, symbols, devices, their functions on pages, and the connections between
//! connection points. Connections refer to connection points, never to coordinates, so moving a
//! symbol can never break a wire.
//!
//! Every change goes through [`Project::apply`], which returns the command that undoes it.

use crate::geometry::{Direction, Point, Rotation};
use crate::symbol::{SymbolDef, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub id: FunctionId,
    pub device: DeviceId,
    pub symbol: SymbolId,
    pub page: PageId,
    pub position: Point,
    #[serde(default)]
    pub rotation: Rotation,
}

impl Function {
    /// A symbol-local point in page coordinates.
    pub fn to_page(&self, local: Point) -> Point {
        self.position.offset(self.rotation.apply(local))
    }
}

/// A connection point of a function, by its designation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionRef {
    pub function: FunctionId,
    pub point: String,
}

impl ConnectionRef {
    pub fn new(function: FunctionId, point: &str) -> Self {
        ConnectionRef { function, point: point.to_owned() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub from: ConnectionRef,
    pub to: ConnectionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_number: Option<String>,
}

/// One undoable change. [`Project::apply`] returns the command that reverses it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    AddDevice(Device),
    /// Only a device with no functions can be removed.
    RemoveDevice(DeviceId),
    RenameDevice {
        id: DeviceId,
        tag: String,
    },
    SetDescription {
        id: DeviceId,
        description: String,
    },
    Place(Function),
    /// Removes a function and every connection to it.
    Unplace(FunctionId),
    Move {
        id: FunctionId,
        to: Point,
    },
    Rotate {
        id: FunctionId,
        to: Rotation,
    },
    Connect(Connection),
    Disconnect(ConnectionId),
    /// Set or clear a wire's number. Numbers are unique in the project.
    SetWireNumber {
        id: ConnectionId,
        number: Option<String>,
    },
    /// Several commands applied together: all of them or none.
    Batch(Vec<Command>),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("no page {0:?}")]
    NoPage(PageId),
    #[error("no device {0:?}")]
    NoDevice(DeviceId),
    #[error("no function {0:?}")]
    NoFunction(FunctionId),
    #[error("no connection {0:?}")]
    NoConnection(ConnectionId),
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
    #[error("device tag must not be empty")]
    EmptyTag,
    #[error("device `{0}` is still drawn on a page")]
    DeviceInUse(String),
    #[error("wire number `{0}` is already used")]
    DuplicateWireNumber(String),
    #[error("id {0} is already used")]
    DuplicateId(Uuid),
}

pub type Result<T> = std::result::Result<T, ModelError>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub pages: BTreeMap<PageId, Page>,
    pub symbols: BTreeMap<SymbolId, SymbolDef>,
    pub devices: BTreeMap<DeviceId, Device>,
    pub functions: BTreeMap<FunctionId, Function>,
    pub connections: BTreeMap<ConnectionId, Connection>,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Project { name: name.to_owned(), ..Project::default() }
    }

    // ---- lookups ----

    pub fn page(&self, id: PageId) -> Result<&Page> {
        self.pages.get(&id).ok_or(ModelError::NoPage(id))
    }
    pub fn device(&self, id: DeviceId) -> Result<&Device> {
        self.devices.get(&id).ok_or(ModelError::NoDevice(id))
    }
    pub fn device_by_tag(&self, tag: &str) -> Option<&Device> {
        self.devices.values().find(|d| d.tag == tag)
    }
    pub fn function(&self, id: FunctionId) -> Result<&Function> {
        self.functions.get(&id).ok_or(ModelError::NoFunction(id))
    }
    pub fn connection(&self, id: ConnectionId) -> Result<&Connection> {
        self.connections.get(&id).ok_or(ModelError::NoConnection(id))
    }
    pub fn symbol(&self, id: &SymbolId) -> Result<&SymbolDef> {
        self.symbols.get(id).ok_or_else(|| ModelError::NoSymbol(id.clone()))
    }
    pub fn functions_on(&self, page: PageId) -> impl Iterator<Item = &Function> {
        self.functions.values().filter(move |f| f.page == page)
    }
    /// Connections drawn on a page (both ends are always on the same page in M1).
    pub fn connections_on(&self, page: PageId) -> impl Iterator<Item = &Connection> {
        self.connections.values().filter(move |c| self.function(c.from.function).is_ok_and(|f| f.page == page))
    }
    /// The connection attached to a connection point, if any.
    pub fn connection_at(&self, r: &ConnectionRef) -> Option<&Connection> {
        self.connections.values().find(|c| c.from == *r || c.to == *r)
    }

    /// `(device tag, point)` for display and reports.
    pub fn describe(&self, r: &ConnectionRef) -> Result<(String, String)> {
        let f = self.function(r.function)?;
        Ok((self.device(f.device)?.tag.clone(), r.point.clone()))
    }

    /// The lowest free tag `<prefix><n>`, n from 1.
    pub fn next_tag(&self, prefix: &str) -> String {
        (1..).map(|n| format!("{prefix}{n}")).find(|t| self.device_by_tag(t).is_none()).unwrap_or_default()
    }

    // ---- library edits (not undoable) ----

    pub fn add_page(&mut self, page: Page) -> PageId {
        let id = page.id;
        self.pages.insert(id, page);
        id
    }

    pub fn add_symbol(&mut self, symbol: SymbolDef) {
        self.symbols.insert(symbol.id.clone(), symbol);
    }

    // ---- undoable edits ----

    /// Apply a command; returns the command that undoes it. On error nothing changes.
    pub fn apply(&mut self, cmd: Command) -> Result<Command> {
        match cmd {
            Command::AddDevice(d) => {
                if self.devices.contains_key(&d.id) {
                    return Err(ModelError::DuplicateId(d.id.0));
                }
                self.check_tag(&d.tag, None)?;
                let id = d.id;
                self.devices.insert(id, d);
                Ok(Command::RemoveDevice(id))
            }
            Command::RemoveDevice(id) => {
                let tag = self.device(id)?.tag.clone();
                if self.functions.values().any(|f| f.device == id) {
                    return Err(ModelError::DeviceInUse(tag));
                }
                let d = self.devices.remove(&id).ok_or(ModelError::NoDevice(id))?;
                Ok(Command::AddDevice(d))
            }
            Command::RenameDevice { id, tag } => {
                self.check_tag(&tag, Some(id))?;
                let d = self.devices.get_mut(&id).ok_or(ModelError::NoDevice(id))?;
                let old = std::mem::replace(&mut d.tag, tag);
                Ok(Command::RenameDevice { id, tag: old })
            }
            Command::SetDescription { id, description } => {
                let d = self.devices.get_mut(&id).ok_or(ModelError::NoDevice(id))?;
                let old = std::mem::replace(&mut d.description, description);
                Ok(Command::SetDescription { id, description: old })
            }
            Command::Place(f) => {
                if self.functions.contains_key(&f.id) {
                    return Err(ModelError::DuplicateId(f.id.0));
                }
                self.page(f.page)?;
                self.device(f.device)?;
                self.symbol(&f.symbol)?;
                let id = f.id;
                self.functions.insert(id, f);
                Ok(Command::Unplace(id))
            }
            Command::Unplace(id) => {
                self.function(id)?;
                let attached: Vec<ConnectionId> = self
                    .connections
                    .values()
                    .filter(|c| c.from.function == id || c.to.function == id)
                    .map(|c| c.id)
                    .collect();
                let mut undo = Vec::new();
                for c in attached {
                    if let Some(c) = self.connections.remove(&c) {
                        undo.push(Command::Connect(c));
                    }
                }
                let f = self.functions.remove(&id).ok_or(ModelError::NoFunction(id))?;
                undo.insert(0, Command::Place(f));
                Ok(Command::Batch(undo))
            }
            Command::Move { id, to } => {
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.position, to);
                Ok(Command::Move { id, to: old })
            }
            Command::Rotate { id, to } => {
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.rotation, to);
                Ok(Command::Rotate { id, to: old })
            }
            Command::Connect(c) => {
                if self.connections.contains_key(&c.id) {
                    return Err(ModelError::DuplicateId(c.id.0));
                }
                self.check_connection(&c)?;
                let id = c.id;
                self.connections.insert(id, c);
                Ok(Command::Disconnect(id))
            }
            Command::Disconnect(id) => {
                let c = self.connections.remove(&id).ok_or(ModelError::NoConnection(id))?;
                Ok(Command::Connect(c))
            }
            Command::SetWireNumber { id, number } => {
                let number = number.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
                self.connection(id)?;
                if let Some(n) = &number {
                    self.check_wire_number(n, Some(id))?;
                }
                let c = self.connections.get_mut(&id).ok_or(ModelError::NoConnection(id))?;
                let old = std::mem::replace(&mut c.wire_number, number);
                Ok(Command::SetWireNumber { id, number: old })
            }
            Command::Batch(cmds) => {
                let mut undo = Vec::with_capacity(cmds.len());
                for cmd in cmds {
                    match self.apply(cmd) {
                        Ok(inv) => undo.push(inv),
                        Err(e) => {
                            // Roll back what this batch already did.
                            for inv in undo.into_iter().rev() {
                                let _ = self.apply(inv);
                            }
                            return Err(e);
                        }
                    }
                }
                undo.reverse();
                Ok(Command::Batch(undo))
            }
        }
    }

    fn check_tag(&self, tag: &str, except: Option<DeviceId>) -> Result<()> {
        if tag.trim().is_empty() {
            return Err(ModelError::EmptyTag);
        }
        match self.device_by_tag(tag) {
            Some(d) if Some(d.id) != except => Err(ModelError::DuplicateTag(tag.to_owned())),
            _ => Ok(()),
        }
    }

    fn check_wire_number(&self, number: &str, except: Option<ConnectionId>) -> Result<()> {
        match self.connections.values().find(|c| c.wire_number.as_deref() == Some(number)) {
            Some(c) if Some(c.id) != except => Err(ModelError::DuplicateWireNumber(number.to_owned())),
            _ => Ok(()),
        }
    }

    fn check_connection(&self, c: &Connection) -> Result<()> {
        if c.from == c.to {
            return Err(ModelError::SelfConnection);
        }
        if let Some(n) = &c.wire_number {
            self.check_wire_number(n, None)?;
        }
        self.connection_point(&c.from)?;
        self.connection_point(&c.to)?;
        if self.function(c.from.function)?.page != self.function(c.to.function)?.page {
            return Err(ModelError::CrossPage);
        }
        for end in [&c.from, &c.to] {
            if let Some(other) = self.connection_at(end) {
                let far = if other.from == *end { &other.to } else { &other.from };
                return Err(ModelError::AlreadyConnected(self.describe(end)?, self.describe(far)?));
            }
        }
        Ok(())
    }

    // ---- convenience wrappers (no undo record) ----

    pub fn add_device(&mut self, device: Device) -> Result<DeviceId> {
        let id = device.id;
        self.apply(Command::AddDevice(device)).map(|_| id)
    }
    pub fn place(&mut self, function: Function) -> Result<FunctionId> {
        let id = function.id;
        self.apply(Command::Place(function)).map(|_| id)
    }
    pub fn connect(&mut self, connection: Connection) -> Result<ConnectionId> {
        let id = connection.id;
        self.apply(Command::Connect(connection)).map(|_| id)
    }
    pub fn move_function(&mut self, id: FunctionId, to: Point) -> Result<()> {
        self.apply(Command::Move { id, to }).map(|_| ())
    }

    // ---- geometry derived from the model ----

    /// Where a connection point sits on its page, and which way a wire leaves it.
    pub fn connection_point(&self, r: &ConnectionRef) -> Result<(Point, Direction)> {
        let f = self.function(r.function)?;
        let s = self.symbol(&f.symbol)?;
        let cp = s
            .connection_point(&r.point)
            .ok_or_else(|| ModelError::NoConnectionPoint { symbol: s.id.0.clone(), point: r.point.clone() })?;
        Ok((f.to_page(cp.at), f.rotation.apply_dir(cp.leaves)))
    }

    /// A function's box on its page (`min`, `max`): its symbol's geometry and connection points,
    /// rotated and placed.
    pub fn function_bounds(&self, id: FunctionId) -> Result<Option<(Point, Point)>> {
        let f = self.function(id)?;
        let Some((lo, hi)) = self.symbol(&f.symbol)?.bounds() else { return Ok(None) };
        let corners = [lo, hi, Point::new(lo.x, hi.y), Point::new(hi.x, lo.y)].map(|c| f.to_page(c));
        let (mut min, mut max) = (corners[0], corners[0]);
        for c in &corners[1..] {
            min = Point::new(min.x.min(c.x), min.y.min(c.y));
            max = Point::new(max.x.max(c.x), max.y.max(c.y));
        }
        Ok(Some((min, max)))
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

/// Orthogonal route between two points leaving in the given directions (also used for the
/// rubber-band wire while drawing).
pub fn route_between(a: Point, da: Direction, b: Point, db: Direction) -> Vec<Point> {
    let facing =
        (a.x == b.x && da.is_vertical() && db.is_vertical()) || (a.y == b.y && !da.is_vertical() && !db.is_vertical());
    if facing && heads(da, a, b) && heads(db, b, a) {
        return vec![a, b];
    }
    let (sa, sb) = (step(da), step(db));
    let a1 = Point::new(a.x + sa.x * ROUTE_STUB, a.y + sa.y * ROUTE_STUB);
    let b1 = Point::new(b.x + sb.x * ROUTE_STUB, b.y + sb.y * ROUTE_STUB);
    // Keep going along the start direction if that brings us closer (split the other axis at
    // its midpoint); if it would lead away, turn first.
    let vertical_mid = if heads(da, a1, b1) { da.is_vertical() } else { !da.is_vertical() };
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
        if let [.., q, r] = out.as_slice()
            && ((q.x == r.x && r.x == p.x) || (q.y == r.y && r.y == p.y))
        {
            out.pop();
        }
        out.push(p);
    }
    out
}
