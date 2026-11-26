//! The project: pages, symbols, devices, their functions on pages, and the connections between
//! connection points. Connections refer to connection points, never to coordinates, so moving a
//! symbol can never break a wire.
//!
//! Every change goes through [`Project::apply`], which returns the command that undoes it.

use crate::geometry::{Direction, Point, Rotation};
use crate::parts::{Part, PartRef};
use crate::sheet::Sheet;
use crate::symbol::{ContactKind, IoKind, Role, SymbolDef, SymbolId};
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
id_type!(CableId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    /// Page number as printed, e.g. `1`, `2`, `3a`. Unique in the project.
    pub name: String,
    /// What the page shows, e.g. "Motor starter". Printed in the title block.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// A second line for the title, e.g. the description in a second language.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// Position in the project; pages are shown sorted by it.
    #[serde(default)]
    pub order: u32,
}

impl Page {
    pub fn new(name: &str, order: u32) -> Self {
        Page { id: PageId::new(), name: name.to_owned(), description: String::new(), subtitle: String::new(), order }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    /// IEC 81346 reference designation, e.g. `-K1` or `=A1+B2-K1`.
    pub tag: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Catalogue parts this device is built from, with quantities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartRef>,
}

impl Device {
    pub fn new(id: DeviceId, tag: &str, description: &str) -> Self {
        Device { id, tag: tag.to_owned(), description: description.to_owned(), parts: Vec::new() }
    }
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
    /// Terminal designations shown for this drawing, by connection-point name, where they
    /// differ from the symbol's: a device's second NO contact shows `23`/`24` for the
    /// symbol's `13`/`14`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub designations: BTreeMap<String, String>,
    /// Extra fields some drawings carry, e.g. a PLC channel's `address`, `symbol`, `comment`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Property keys of a PLC channel.
pub const ADDRESS: &str = "address";
pub const SYMBOL: &str = "symbol";
pub const COMMENT: &str = "comment";

impl Function {
    /// A property's value, or `""`.
    pub fn property(&self, key: &str) -> &str {
        self.properties.get(key).map_or("", String::as_str)
    }

    /// The designation shown for one of this function's connection points.
    pub fn designation<'a>(&'a self, point: &'a str) -> &'a str {
        self.designations.get(point).map_or(point, String::as_str)
    }

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
    /// Cross-section, colour and cable core, as far as they are known.
    #[serde(default, flatten)]
    pub spec: WireSpec,
}

/// What a wire is made of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSpec {
    /// e.g. `1,5` or `2.5` (mm²), as the user writes it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cross_section: String,
    /// IEC 60757 colour code, e.g. `BU`, `BK`, `GNYE`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub colour: String,
    /// The cable core this wire is, if it runs in a cable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cable: Option<CableCore>,
}

/// A core of a cable: the cable and the core's name (`1`, `BN`, `GNYE`…).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CableCore {
    pub cable: CableId,
    pub core: String,
}

/// A physical cable: its tag, type and cores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cable {
    pub id: CableId,
    /// e.g. `-WM10A`.
    pub tag: String,
    /// e.g. `4G2,5` or `H07RN-F 4x2,5`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Number of cores.
    #[serde(default)]
    pub cores: u32,
    /// The cable has a shield (drawn as SCH).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shielded: bool,
}

/// Compare terminal numbers naturally: numbers as numbers, then text.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    }
}

/// One channel of a PLC card.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChannelRef {
    pub function: FunctionId,
    pub page: PageId,
    pub channel: String,
    pub kind: IoKind,
    pub address: String,
    pub symbol: String,
    pub comment: String,
    pub location: String,
}

/// One terminal of a strip.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TerminalRef {
    pub function: FunctionId,
    pub page: PageId,
    pub number: String,
    /// Where the terminal is drawn, e.g. `1.3`.
    pub location: String,
}

/// One contact of a device, for cross-reference lists.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContactRef {
    pub function: FunctionId,
    pub page: PageId,
    pub kind: ContactKind,
    /// Terminal designations, a pair per pole joined with `-`, poles with a space: `13-14`,
    /// `1-2 3-4 5-6`.
    pub terminals: String,
    /// The poles as (first, second) designations: `[("13", "14")]`.
    pub poles: Vec<(String, String)>,
    /// Where the contact is drawn, e.g. `2.4`.
    pub location: String,
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
    AddPage(Page),
    /// Only an empty page can be removed (remove its symbols first, in the same batch).
    RemovePage(PageId),
    RenamePage {
        id: PageId,
        name: String,
    },
    DescribePage {
        id: PageId,
        description: String,
    },
    /// The page title's second line.
    SetPageSubtitle {
        id: PageId,
        subtitle: String,
    },
    /// The project's name and title-block fields.
    SetProjectInfo {
        name: String,
        info: ProjectInfo,
    },
    /// Sheet size, columns and rows.
    SetSheet(Sheet),
    /// How new tags and wire numbers are formed.
    SetNaming(crate::Naming),
    /// Put the pages in this order (every page, once).
    ReorderPages(Vec<PageId>),
    /// Make a function part of another device, with the designations it gets there.
    Reassign {
        id: FunctionId,
        device: DeviceId,
        designations: BTreeMap<String, String>,
    },
    /// Add a cable (its tag must be new).
    AddCable(Cable),
    /// Change a cable's tag, type, cores or shield.
    UpdateCable(Cable),
    /// Remove a cable no wire runs in.
    RemoveCable(CableId),
    /// Set a wire's cross-section, colour and cable core.
    SetWireSpec {
        id: ConnectionId,
        spec: WireSpec,
    },
    /// Add a symbol to the project's library (its id must be new), e.g. a black box.
    AddSymbol(SymbolDef),
    /// Remove a symbol nothing is drawn with.
    RemoveSymbol(SymbolId),
    /// Add a part to the catalogue (its number must be new).
    AddPart(Part),
    /// Replace a catalogue entry with the same number.
    UpdatePart(Part),
    /// Remove a catalogue entry no device uses.
    RemovePart(String),
    /// Set the parts a device is built from.
    SetDeviceParts {
        id: DeviceId,
        parts: Vec<PartRef>,
    },
    /// Set a drawing's properties (a PLC channel's address, symbol, comment).
    SetProperties {
        id: FunctionId,
        properties: BTreeMap<String, String>,
    },
    /// Set a drawing's terminal designations (e.g. renumber a terminal).
    SetDesignations {
        id: FunctionId,
        designations: BTreeMap<String, String>,
    },
    /// Several commands applied together: all of them or none.
    Batch(Vec<Command>),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("no page {0:?}")]
    NoPage(PageId),
    #[error("a tag or wire number format needs one {{n}} (or {{n:02}}) and only {{page}} and {{letters}} (tags only)")]
    InvalidNaming,
    #[error("the project name must not be empty")]
    EmptyProjectName,
    #[error("a sheet has 1 to 50 columns, numbered from 0 or 1, and up to 26 rows")]
    InvalidSheet,
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
    #[error("page `{0}` already exists")]
    DuplicatePageName(String),
    #[error("page name must not be empty")]
    EmptyPageName,
    #[error("page `{0}` still has symbols on it")]
    PageNotEmpty(String),
    #[error("the new page order must list every page once")]
    BadPageOrder,
    #[error("there is no device `{0}`")]
    UnknownTag(String),
    #[error("there is no cable `{0}`")]
    UnknownCable(String),
    #[error("part `{0}` is already in the catalogue")]
    DuplicatePart(String),
    #[error("part `{0}` is not in the catalogue")]
    UnknownPart(String),
    #[error("part `{0}` is used by {1}")]
    PartInUse(String, String),
    #[error("a black box needs at least one terminal, and every terminal a unique, non-empty name")]
    InvalidBlackBox,
    #[error("no cable {0:?}")]
    NoCable(CableId),
    #[error("cable {0} still has wires in it")]
    CableInUse(String),
    #[error("a cable core needs a name, e.g. 1 or BN")]
    EmptyCore,
    #[error("core {1} of cable {0} is already used")]
    CoreInUse(String, String),
    #[error("symbol `{0}` already exists")]
    DuplicateSymbol(String),
    #[error("symbol `{0}` is used by {1}")]
    SymbolInUse(String, String),
    #[error("part number must not be empty")]
    EmptyPartNumber,
    #[error("quantity must be at least 1")]
    ZeroQuantity,
    #[error("{0}:{1} already exists")]
    DuplicateNumber(String, String),
    #[error("the terminal or channel number must not be empty")]
    EmptyNumber,
    #[error("PLC address {0} is already used by {1}")]
    DuplicateAddress(String, String),
    #[error("PLC symbol `{0}` is already used by {1}")]
    DuplicatePlcSymbol(String, String),
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
    /// The sheet every page is drawn on.
    #[serde(default)]
    pub sheet: Sheet,
    /// The parts catalogue, by part number.
    #[serde(default)]
    pub parts: BTreeMap<String, Part>,
    /// What the title block shows besides the project name.
    #[serde(default)]
    pub info: ProjectInfo,
    /// How new tags and wire numbers are formed.
    #[serde(default)]
    pub naming: crate::Naming,
    /// Cables, by id; wires refer to their cores.
    #[serde(default)]
    pub cables: BTreeMap<CableId, Cable>,
}

/// Title-block fields of a project. All free text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectInfo {
    pub customer: String,
    /// Plant or machine, e.g. "Cooling spiral".
    pub plant: String,
    /// The diagram's own code or number.
    pub code: String,
    pub designer: String,
    pub date: String,
    /// Newest last; the title block shows the last few.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revisions: Vec<Revision>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Revision {
    /// e.g. `0`, `1`, `A`.
    pub index: String,
    pub change: String,
    pub date: String,
    /// Who made it (initials).
    pub by: String,
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
    /// Pages in project order.
    pub fn pages_in_order(&self) -> Vec<&Page> {
        let mut v: Vec<&Page> = self.pages.values().collect();
        v.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        v
    }
    /// The address of a point on a page: `<page>.<column>` (`2.4`) on a sheet without rows,
    /// `<page>/<column><row>` (`A3/2F`) on one with rows.
    pub fn location(&self, page: PageId, p: Point) -> Result<String> {
        let name = &self.page(page)?.name;
        let sep = if self.sheet.rows > 0 { "/" } else { "." };
        Ok(format!("{name}{sep}{}", self.sheet.address(p)))
    }

    /// How a drawing is referred to from elsewhere (a contact to its coil, an interruption
    /// point to its partner): `/2.4`, or `A3/2F` when the location already has its slash.
    pub fn reference(&self, f: FunctionId) -> Result<String> {
        let at = self.function_location(f)?;
        Ok(if self.sheet.rows > 0 { at } else { format!("/{at}") })
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
        Ok((self.device(f.device)?.tag.clone(), f.designation(&r.point).to_owned()))
    }

    // ---- cross-references ----

    /// The role of a function's symbol (main function, contact, single).
    pub fn role(&self, f: FunctionId) -> Result<Role> {
        Ok(self.symbol(&self.function(f)?.symbol)?.role)
    }

    /// All drawings of a device, in id order.
    pub fn functions_of(&self, device: DeviceId) -> impl Iterator<Item = &Function> {
        self.functions.values().filter(move |f| f.device == device)
    }

    /// A device's main function (its coil), if it has one.
    pub fn main_function(&self, device: DeviceId) -> Option<&Function> {
        self.functions_of(device).find(|f| self.role(f.id) == Ok(Role::Main))
    }

    /// Where a function is drawn: `<page>.<column>`.
    pub fn function_location(&self, f: FunctionId) -> Result<String> {
        let f = self.function(f)?;
        self.location(f.page, f.position)
    }

    /// A device's contacts with their terminals and addresses, sorted by terminals.
    pub fn contacts_of(&self, device: DeviceId) -> Vec<ContactRef> {
        let mut out: Vec<ContactRef> = self
            .functions_of(device)
            .filter_map(|f| {
                let sym = self.symbol(&f.symbol).ok()?;
                let Role::Contact { kind } = sym.role else { return None };
                let names: Vec<&str> = sym.connection_points.iter().map(|cp| f.designation(&cp.name)).collect();
                let poles: Vec<(String, String)> = names
                    .chunks(2)
                    .map(|p| (p[0].to_owned(), p.get(1).map_or_else(String::new, |s| (*s).to_owned())))
                    .collect();
                let terminals = poles
                    .iter()
                    .map(|(a, b)| if b.is_empty() { a.clone() } else { format!("{a}-{b}") })
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(ContactRef {
                    function: f.id,
                    page: f.page,
                    kind,
                    terminals,
                    poles,
                    location: self.location(f.page, f.position).ok()?,
                })
            })
            .collect();
        out.sort_by(|a, b| a.terminals.cmp(&b.terminals));
        out
    }

    /// Designations for a new drawing of `device` with `symbol`:
    /// - a contact gets IEC numbering, where the first digit counts the device's contacts
    ///   (1x, 2x, 3x…) and the second is the symbol's own (3/4 for NO, 1/2 for NC);
    /// - a terminal gets the strip's next free number on both its sides;
    /// - anything else keeps the symbol's own names (empty map).
    pub fn next_designations(&self, device: DeviceId, symbol: &SymbolId) -> BTreeMap<String, String> {
        let Ok(sym) = self.symbol(symbol) else { return BTreeMap::new() };
        if sym.role == Role::Terminal {
            let n = self.next_terminal_number(device);
            return sym.connection_points.iter().map(|cp| (cp.name.clone(), n.clone())).collect();
        }
        if let Role::PlcChannel { .. } = sym.role {
            // Channels count from 0 on each card.
            let used: Vec<u32> = self.plc_channels_of(device).iter().filter_map(|c| c.channel.parse().ok()).collect();
            let n = (0..).find(|n| !used.contains(n)).unwrap_or(0).to_string();
            return sym.connection_points.iter().map(|cp| (cp.name.clone(), n.clone())).collect();
        }
        // Main contacts and everything else keep the symbol's own numbers.
        if !matches!(sym.role, Role::Contact { kind } if kind != ContactKind::Power) {
            return BTreeMap::new();
        }
        let used: Vec<u32> = self
            .contacts_of(device)
            .iter()
            .filter_map(|c| c.terminals.split('-').next()?.parse::<u32>().ok().map(|n| n / 10))
            .collect();
        let n = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        sym.connection_points
            .iter()
            .map(|cp| {
                let last = cp.name.chars().last().map(String::from).unwrap_or_default();
                (cp.name.clone(), format!("{n}{last}"))
            })
            .collect()
    }

    /// The number of a terminal or PLC channel: the designation of its first connection point.
    pub fn drawing_number(&self, f: FunctionId) -> Option<String> {
        let f = self.function(f).ok()?;
        let sym = self.symbol(&f.symbol).ok()?;
        sym.role.numbered().then(|| sym.connection_points.first().map(|cp| f.designation(&cp.name).to_owned()))?
    }

    /// A terminal's number.
    pub fn terminal_number(&self, f: FunctionId) -> Option<String> {
        (self.role(f).ok()? == Role::Terminal).then(|| self.drawing_number(f))?
    }

    /// Whether a device should have catalogue parts: anything drawn with a real symbol, but not
    /// junctions, potential points or interruption points, which are drawing aids.
    pub fn needs_part(&self, device: DeviceId) -> bool {
        self.functions_of(device).any(|f| {
            self.symbol(&f.symbol).is_ok_and(|s| !s.bridged || s.role == Role::Terminal)
                && !self.role(f.id).is_ok_and(Role::joins_drawings)
        })
    }

    /// A PLC card's channels, in channel order.
    pub fn plc_channels_of(&self, card: DeviceId) -> Vec<ChannelRef> {
        let mut out: Vec<ChannelRef> = self
            .functions_of(card)
            .filter_map(|f| {
                let Role::PlcChannel { kind } = self.role(f.id).ok()? else { return None };
                Some(ChannelRef {
                    function: f.id,
                    page: f.page,
                    channel: self.drawing_number(f.id)?,
                    kind,
                    address: f.property(ADDRESS).to_owned(),
                    symbol: f.property(SYMBOL).to_owned(),
                    comment: f.property(COMMENT).to_owned(),
                    location: self.location(f.page, f.position).ok()?,
                })
            })
            .collect();
        out.sort_by(|a, b| natural(&a.channel, &b.channel));
        out
    }

    /// The next free PLC address of a kind, following the highest one in the project:
    /// Siemens-style `I0.0`…`I0.7`, `I1.0` (bits wrap at 8) for digital, `IW256`, `IW258` (words
    /// in steps of 2) for analog; `Q`/`QW` for outputs. Digital starts at `I0.0`/`Q0.0`;
    /// analog has no start of its own (empty).
    pub fn next_address(&self, kind: IoKind) -> String {
        let (prefix, analog) = match kind {
            IoKind::DigitalIn => ("I", false),
            IoKind::DigitalOut => ("Q", false),
            IoKind::AnalogIn => ("IW", true),
            IoKind::AnalogOut => ("QW", true),
        };
        let used = self.functions.values().filter_map(|f| {
            let addr = f.property(ADDRESS).strip_prefix(prefix)?;
            if analog {
                addr.parse::<u32>().ok().map(|w| (w, 0))
            } else {
                let (byte, bit) = addr.split_once('.')?;
                Some((byte.parse::<u32>().ok()?, bit.parse::<u32>().ok().filter(|b| *b < 8)?))
            }
        });
        match (used.max(), analog) {
            (None, true) => String::new(),
            (None, false) => format!("{prefix}0.0"),
            (Some((w, _)), true) => format!("{prefix}{}", w + 2),
            (Some((byte, 7)), false) => format!("{prefix}{}.0", byte + 1),
            (Some((byte, bit)), false) => format!("{prefix}{byte}.{}", bit + 1),
        }
    }

    /// Properties for a new drawing with `symbol`: a PLC channel gets the next free address.
    pub fn new_drawing_properties(&self, symbol: &SymbolId) -> BTreeMap<String, String> {
        match self.symbol(symbol).map(|s| s.role) {
            Ok(Role::PlcChannel { kind }) => {
                let address = self.next_address(kind);
                if address.is_empty() { BTreeMap::new() } else { [(ADDRESS.to_owned(), address)].into() }
            }
            _ => BTreeMap::new(),
        }
    }

    /// The terminals of a strip, in terminal order (numbers compare as numbers: 2 before 10).
    pub fn terminals_of(&self, strip: DeviceId) -> Vec<TerminalRef> {
        let mut out: Vec<TerminalRef> = self
            .functions_of(strip)
            .filter_map(|f| {
                Some(TerminalRef {
                    function: f.id,
                    page: f.page,
                    number: self.terminal_number(f.id)?,
                    location: self.location(f.page, f.position).ok()?,
                })
            })
            .collect();
        out.sort_by(|a, b| natural(&a.number, &b.number));
        out
    }

    /// One more than the strip's highest numeric terminal, or 1.
    pub fn next_terminal_number(&self, strip: DeviceId) -> String {
        let highest = self.terminals_of(strip).iter().filter_map(|t| t.number.parse::<u32>().ok()).max().unwrap_or(0);
        (highest + 1).to_string()
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
                self.check_part_refs(&d.parts)?;
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
                if let Err(e) = self.check_terminal(id).and_then(|()| self.check_plc(id)) {
                    self.functions.remove(&id);
                    return Err(e);
                }
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
            Command::AddPage(page) => {
                if self.pages.contains_key(&page.id) {
                    return Err(ModelError::DuplicateId(page.id.0));
                }
                self.check_page_name(&page.name, None)?;
                let id = page.id;
                self.pages.insert(id, page);
                Ok(Command::RemovePage(id))
            }
            Command::RemovePage(id) => {
                let name = self.page(id)?.name.clone();
                if self.functions.values().any(|f| f.page == id) {
                    return Err(ModelError::PageNotEmpty(name));
                }
                let page = self.pages.remove(&id).ok_or(ModelError::NoPage(id))?;
                Ok(Command::AddPage(page))
            }
            Command::RenamePage { id, name } => {
                let name = name.trim().to_owned();
                self.check_page_name(&name, Some(id))?;
                let page = self.pages.get_mut(&id).ok_or(ModelError::NoPage(id))?;
                let old = std::mem::replace(&mut page.name, name);
                Ok(Command::RenamePage { id, name: old })
            }
            Command::DescribePage { id, description } => {
                let page = self.pages.get_mut(&id).ok_or(ModelError::NoPage(id))?;
                let old = std::mem::replace(&mut page.description, description.trim().to_owned());
                Ok(Command::DescribePage { id, description: old })
            }
            Command::SetPageSubtitle { id, subtitle } => {
                let page = self.pages.get_mut(&id).ok_or(ModelError::NoPage(id))?;
                let old = std::mem::replace(&mut page.subtitle, subtitle.trim().to_owned());
                Ok(Command::SetPageSubtitle { id, subtitle: old })
            }
            Command::SetProjectInfo { name, info } => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Err(ModelError::EmptyProjectName);
                }
                let old_name = std::mem::replace(&mut self.name, name);
                let old_info = std::mem::replace(&mut self.info, info);
                Ok(Command::SetProjectInfo { name: old_name, info: old_info })
            }
            Command::SetNaming(naming) => {
                if !naming.is_valid() {
                    return Err(ModelError::InvalidNaming);
                }
                Ok(Command::SetNaming(std::mem::replace(&mut self.naming, naming)))
            }
            Command::SetSheet(sheet) => {
                if !sheet.is_valid() {
                    return Err(ModelError::InvalidSheet);
                }
                Ok(Command::SetSheet(std::mem::replace(&mut self.sheet, sheet)))
            }
            Command::ReorderPages(order) => {
                let mut sorted = order.clone();
                sorted.sort();
                sorted.dedup();
                if sorted.len() != order.len() || sorted != self.pages.keys().copied().collect::<Vec<_>>() {
                    return Err(ModelError::BadPageOrder);
                }
                let old: Vec<PageId> = self.pages_in_order().iter().map(|p| p.id).collect();
                for (i, id) in order.iter().enumerate() {
                    if let Some(p) = self.pages.get_mut(id) {
                        p.order = i as u32;
                    }
                }
                Ok(Command::ReorderPages(old))
            }
            Command::Reassign { id, device, designations } => {
                self.device(device)?;
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old_device = std::mem::replace(&mut f.device, device);
                let old_designations = std::mem::replace(&mut f.designations, designations);
                Ok(Command::Reassign { id, device: old_device, designations: old_designations })
            }
            Command::AddCable(cable) => {
                let cable = Cable { tag: cable.tag.trim().to_owned(), ..cable };
                self.check_cable(&cable)?;
                if self.cables.contains_key(&cable.id) {
                    return Err(ModelError::DuplicateId(cable.id.0));
                }
                let id = cable.id;
                self.cables.insert(id, cable);
                Ok(Command::RemoveCable(id))
            }
            Command::UpdateCable(cable) => {
                let cable = Cable { tag: cable.tag.trim().to_owned(), ..cable };
                self.check_cable(&cable)?;
                let slot = self.cables.get_mut(&cable.id).ok_or(ModelError::NoCable(cable.id))?;
                Ok(Command::UpdateCable(std::mem::replace(slot, cable)))
            }
            Command::RemoveCable(id) => {
                if self.connections.values().any(|c| c.spec.cable.as_ref().is_some_and(|k| k.cable == id)) {
                    let tag = self.cables.get(&id).map(|c| c.tag.clone()).unwrap_or_default();
                    return Err(ModelError::CableInUse(tag));
                }
                let cable = self.cables.remove(&id).ok_or(ModelError::NoCable(id))?;
                Ok(Command::AddCable(cable))
            }
            Command::SetWireSpec { id, spec } => {
                let spec = WireSpec {
                    cross_section: spec.cross_section.trim().to_owned(),
                    colour: spec.colour.trim().to_uppercase(),
                    cable: spec.cable.map(|k| CableCore { core: k.core.trim().to_owned(), ..k }),
                };
                self.check_core(id, spec.cable.as_ref())?;
                let c = self.connections.get_mut(&id).ok_or(ModelError::NoConnection(id))?;
                Ok(Command::SetWireSpec { id, spec: std::mem::replace(&mut c.spec, spec) })
            }
            Command::AddSymbol(symbol) => {
                if self.symbols.contains_key(&symbol.id) {
                    return Err(ModelError::DuplicateSymbol(symbol.id.0));
                }
                let id = symbol.id.clone();
                self.symbols.insert(id.clone(), symbol);
                Ok(Command::RemoveSymbol(id))
            }
            Command::RemoveSymbol(id) => {
                if let Some(f) = self.functions.values().find(|f| f.symbol == id) {
                    let tag = self.device(f.device).map(|d| d.tag.clone()).unwrap_or_default();
                    return Err(ModelError::SymbolInUse(id.0, tag));
                }
                let symbol = self.symbols.remove(&id).ok_or(ModelError::NoSymbol(id))?;
                Ok(Command::AddSymbol(symbol))
            }
            Command::AddPart(part) => {
                let part = Part { number: part.number.trim().to_owned(), ..part };
                if part.number.is_empty() {
                    return Err(ModelError::EmptyPartNumber);
                }
                if self.parts.contains_key(&part.number) {
                    return Err(ModelError::DuplicatePart(part.number));
                }
                let number = part.number.clone();
                self.parts.insert(number.clone(), part);
                Ok(Command::RemovePart(number))
            }
            Command::UpdatePart(part) => {
                let slot =
                    self.parts.get_mut(&part.number).ok_or_else(|| ModelError::UnknownPart(part.number.clone()))?;
                let old = std::mem::replace(slot, part);
                Ok(Command::UpdatePart(old))
            }
            Command::RemovePart(number) => {
                let mut users: Vec<&str> = self
                    .devices
                    .values()
                    .filter(|d| d.parts.iter().any(|p| p.number == number))
                    .map(|d| d.tag.as_str())
                    .collect();
                if !users.is_empty() {
                    users.sort();
                    return Err(ModelError::PartInUse(number, users.join(", ")));
                }
                let part = self.parts.remove(&number).ok_or(ModelError::UnknownPart(number))?;
                Ok(Command::AddPart(part))
            }
            Command::SetDeviceParts { id, parts } => {
                self.check_part_refs(&parts)?;
                let d = self.devices.get_mut(&id).ok_or(ModelError::NoDevice(id))?;
                let old = std::mem::replace(&mut d.parts, parts);
                Ok(Command::SetDeviceParts { id, parts: old })
            }
            Command::SetProperties { id, properties } => {
                let properties = properties
                    .into_iter()
                    .map(|(k, v)| (k, v.trim().to_owned()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect();
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.properties, properties);
                if let Err(e) = self.check_plc(id) {
                    if let Some(f) = self.functions.get_mut(&id) {
                        f.properties = old;
                    }
                    return Err(e);
                }
                Ok(Command::SetProperties { id, properties: old })
            }
            Command::SetDesignations { id, designations } => {
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.designations, designations);
                if let Err(e) = self.check_terminal(id) {
                    if let Some(f) = self.functions.get_mut(&id) {
                        f.designations = old;
                    }
                    return Err(e);
                }
                Ok(Command::SetDesignations { id, designations: old })
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

    /// A terminal's or channel's number is non-empty and unique on its device.
    fn check_terminal(&self, f: FunctionId) -> Result<()> {
        let Some(number) = self.drawing_number(f) else { return Ok(()) };
        if number.trim().is_empty() {
            return Err(ModelError::EmptyNumber);
        }
        let device = self.function(f)?.device;
        let taken = self
            .functions_of(device)
            .any(|g| g.id != f && self.drawing_number(g.id).as_deref() == Some(number.as_str()));
        if taken {
            return Err(ModelError::DuplicateNumber(self.device(device)?.tag.clone(), number));
        }
        Ok(())
    }

    /// A PLC channel's address and symbol, when set, are unique in the project.
    fn check_plc(&self, f: FunctionId) -> Result<()> {
        let func = self.function(f)?;
        if !matches!(self.role(f)?, Role::PlcChannel { .. }) {
            return Ok(());
        }
        for (key, is_address) in [(ADDRESS, true), (SYMBOL, false)] {
            let value = func.property(key);
            if value.is_empty() {
                continue;
            }
            let other = self.functions.values().find(|g| {
                g.id != f && matches!(self.role(g.id), Ok(Role::PlcChannel { .. })) && g.property(key) == value
            });
            if let Some(g) = other {
                let by = format!("{}:{}", self.device(g.device)?.tag, self.drawing_number(g.id).unwrap_or_default());
                return Err(if is_address {
                    ModelError::DuplicateAddress(value.to_owned(), by)
                } else {
                    ModelError::DuplicatePlcSymbol(value.to_owned(), by)
                });
            }
        }
        Ok(())
    }

    fn check_part_refs(&self, parts: &[PartRef]) -> Result<()> {
        for p in parts {
            if !self.parts.contains_key(&p.number) {
                return Err(ModelError::UnknownPart(p.number.clone()));
            }
            if p.quantity == 0 {
                return Err(ModelError::ZeroQuantity);
            }
        }
        Ok(())
    }

    fn check_page_name(&self, name: &str, except: Option<PageId>) -> Result<()> {
        if name.trim().is_empty() {
            return Err(ModelError::EmptyPageName);
        }
        match self.pages.values().find(|p| p.name == name) {
            Some(p) if Some(p.id) != except => Err(ModelError::DuplicatePageName(name.to_owned())),
            _ => Ok(()),
        }
    }

    /// A cable's tag is non-empty and unique.
    fn check_cable(&self, cable: &Cable) -> Result<()> {
        if cable.tag.is_empty() {
            return Err(ModelError::EmptyTag);
        }
        if self.cables.values().any(|c| c.id != cable.id && c.tag == cable.tag) {
            return Err(ModelError::DuplicateTag(cable.tag.clone()));
        }
        Ok(())
    }

    /// A wire's cable exists, and no other wire uses the same core.
    fn check_core(&self, wire: ConnectionId, core: Option<&CableCore>) -> Result<()> {
        let Some(core) = core else { return Ok(()) };
        let cable = self.cables.get(&core.cable).ok_or(ModelError::NoCable(core.cable))?;
        if core.core.is_empty() {
            return Err(ModelError::EmptyCore);
        }
        let taken = self.connections.values().any(|c| c.id != wire && c.spec.cable.as_ref() == Some(core));
        if taken {
            return Err(ModelError::CoreInUse(cable.tag.clone(), core.core.clone()));
        }
        Ok(())
    }

    fn check_wire_number(&self, number: &str, except: Option<ConnectionId>) -> Result<()> {
        // Per potential, wires share numbers; clashes between potentials are a check instead.
        if self.naming.wire_numbering == crate::WireNumbering::PerPotential {
            return Ok(());
        }
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
        self.check_core(c.id, c.spec.cable.as_ref())?;
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

    /// Bounding box (`min`, `max`) of the symbols and wires on a page, or `None` when empty.
    pub fn content_bounds(&self, page: PageId) -> Option<(Point, Point)> {
        let mut pts = Vec::new();
        for f in self.functions_on(page) {
            if let Ok(Some((lo, hi))) = self.function_bounds(f.id) {
                pts.extend([lo, hi]);
            }
        }
        for c in self.connections_on(page) {
            pts.extend(self.route(c).unwrap_or_default());
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
        }))
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
