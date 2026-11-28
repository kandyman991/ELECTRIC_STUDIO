//! The project: pages, symbols, devices, their functions on pages, and the connections between
//! connection points. Connections refer to connection points, never to coordinates, so moving a
//! symbol can never break a wire.
//!
//! Every change goes through [`Project::apply`], which returns the command that undoes it.

use crate::connectivity::{GroupId, NodeId, NodeKind, TerminalId};
use crate::geometry::{Direction, Point, Rotation};
use crate::parts::{Part, PartRef};
use crate::sheet::Sheet;
use crate::symbol::{ContactKind, IoKind, Role, SymbolDef, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// The gap between the sort keys of pages added in a row, so a page fits between any two.
pub const PAGE_STEP: u64 = 1 << 16;

/// Sort keys putting the pages in `order`, changing as few as possible of `old`: the longest run
/// of pages already in increasing key order keeps its keys, and each other page takes a key
/// between its neighbours. Only if there is no room between two is every page re-keyed, one
/// [`PAGE_STEP`] apart.
pub fn sparse_keys(order: &[PageId], old: &BTreeMap<PageId, u64>) -> BTreeMap<PageId, u64> {
    let keys: Vec<u64> = order.iter().map(|id| old.get(id).copied().unwrap_or(0)).collect();
    // Longest strictly increasing subsequence of the keys, O(n log n).
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; keys.len()];
    for i in 0..keys.len() {
        let at = tails.partition_point(|&t| keys[t] < keys[i]);
        if at > 0 {
            prev[i] = tails[at - 1];
        }
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut keep = vec![false; keys.len()];
    let mut i = tails.last().copied().unwrap_or(usize::MAX);
    while i != usize::MAX {
        keep[i] = true;
        i = prev[i];
    }
    let mut out = BTreeMap::new();
    let mut lower = 0u64;
    let mut i = 0;
    while i < order.len() {
        if keep[i] {
            out.insert(order[i], keys[i]);
            lower = keys[i];
            i += 1;
            continue;
        }
        // A run of pages to place between `lower` and the next kept key.
        let end = (i..order.len()).find(|&j| keep[j]).unwrap_or(order.len());
        let upper = if end < order.len() { keys[end] } else { lower.saturating_add(PAGE_STEP * (end - i + 1) as u64) };
        let gap = upper.saturating_sub(lower) / (end - i + 1) as u64;
        if gap == 0 {
            return order.iter().enumerate().map(|(n, id)| (*id, PAGE_STEP * (n as u64 + 1))).collect();
        }
        for (n, id) in order[i..end].iter().enumerate() {
            out.insert(*id, lower + gap * (n as u64 + 1));
        }
        lower += gap * (end - i) as u64;
        i = end;
    }
    out
}

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
id_type!(
    /// A product in the project's catalogue (D7). Its manufacturer and order number are
    /// properties: two makers' products with the same number are two products.
    ProductId
);

/// The namespace of ids derived from legacy data (format 1 products, D7).
const LEGACY: Uuid = Uuid::from_u128(0x6563_6164_2d6c_6567_6163_792d_6964_7300);

impl DeviceId {
    /// The device of a node drawing: none (D1). Node drawings show a node, not a device.
    pub const NONE: DeviceId = DeviceId(Uuid::nil());

    pub fn is_none(&self) -> bool {
        self.0.is_nil()
    }

    fn none() -> Self {
        DeviceId::NONE
    }
}

impl ProductId {
    /// The id a product numbered `number` gets when a format 1 project (where numbers were
    /// unique) is read, and in the built-in samples: the same on every conversion.
    pub fn for_number(number: &str) -> Self {
        ProductId(Uuid::new_v5(&LEGACY, format!("product/{}", number.trim()).as_bytes()))
    }
}
id_type!(CableId);
id_type!(BoxId);
id_type!(
    /// A rail, duct or device footprint on a mounting plate.
    PanelItemId
);

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
    pub order: u64,
}

impl Page {
    pub fn new(name: &str, order: u64) -> Self {
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
    /// The device this drawing shows; [`DeviceId::NONE`] for a node drawing.
    #[serde(default = "DeviceId::none", skip_serializing_if = "DeviceId::is_none")]
    pub device: DeviceId,
    /// The node a node drawing (a junction dot, a potential or interruption point) shows: an
    /// explicit binding (D1, D33). `None` for a device's drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    pub symbol: SymbolId,
    pub page: PageId,
    pub position: Point,
    #[serde(default)]
    pub rotation: Rotation,
    /// Flipped left to right (before the rotation), e.g. a switch drawn the other way round on
    /// a horizontal line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    /// Designations for the terminals this drawing creates, by connection-point name, where
    /// they differ from the symbol's: a device's second NO contact gets `23`/`24` for the
    /// symbol's `13`/`14`. Read by [`Command::Place`], which moves them onto the terminals and
    /// leaves this empty: a placed drawing's designations are its terminals'
    /// ([`Project::designation`]). Format 1 page files store them here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub designations: BTreeMap<String, String>,
    /// The terminal each connection point shows (D2), for drawings of physical connection
    /// points. [`Command::Place`] fills in the points not given, with new terminals; a point
    /// given an existing terminal of the same device shows that one (a repeated drawing).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub terminals: BTreeMap<String, TerminalId>,
    /// Extra fields some drawings carry, e.g. a PLC channel's `address`, `symbol`, `comment`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// A physical connection point of a device (D2), persistent from format 2 on: its id never
/// changes when it is renumbered, redrawn or drawn more than once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEntry {
    pub id: TerminalId,
    pub device: DeviceId,
    /// The physical terminal it belongs to (a strip terminal's sides and levels share one).
    pub group: GroupId,
    /// The point within the group (a strip terminal's side or level); empty for an ordinary
    /// device terminal.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub point: String,
    /// The printed designation, an editable property: `A1`, `13`, a strip terminal's `3`.
    pub designation: String,
    /// A PLC channel's typed data, on the channel's terminal (D2): a PLC channel is a terminal
    /// of the card, with its address as a typed property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelData>,
}

/// What a PLC channel is: its kind, its address as written, its symbol and comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelData {
    pub kind: crate::IoKind,
    /// The address as entered (`%I0.3`, `i0.3`); [`crate::plc_address`] reads it canonically.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub symbol: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

impl ChannelData {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            ADDRESS => Some(&self.address),
            SYMBOL => Some(&self.symbol),
            COMMENT => Some(&self.comment),
            _ => None,
        }
    }

    fn set(&mut self, key: &str, value: String) {
        match key {
            ADDRESS => self.address = value,
            SYMBOL => self.symbol = value,
            COMMENT => self.comment = value,
            _ => {}
        }
    }
}

/// The property keys kept as a PLC channel's typed data, on its terminal.
pub const CHANNEL_KEYS: [&str; 3] = [ADDRESS, SYMBOL, COMMENT];

fn check_node(n: NodeEntry) -> Result<NodeEntry> {
    let n = NodeEntry { name: n.name.trim().to_owned(), scope: n.scope.trim().to_owned(), ..n };
    if n.kind != NodeKind::Junction && n.name.is_empty() {
        return Err(ModelError::EmptyNodeName(n.kind.label()));
    }
    let scope = if n.scope.is_empty() { project_scope() } else { n.scope };
    Ok(NodeEntry { scope, ..n })
}

/// The naming scope of a format 1 project's potentials and interruptions, and the default.
pub const PROJECT_SCOPE: &str = "project";

fn project_scope() -> String {
    PROJECT_SCOPE.to_owned()
}

fn is_project_scope(s: &str) -> bool {
    s == PROJECT_SCOPE
}

/// A logical connection point with no physical terminal (D1): a junction, a named potential,
/// an interruption. Persistent from format 2 on; its drawings bind to it by id, never by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEntry {
    pub id: NodeId,
    pub kind: NodeKind,
    /// The displayed name (`L+`, `Q-FWD`). A junction needs none; one converted from format 1
    /// keeps its tag (`J1`), so its drawings stay as they were.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The naming scope (D33): a lookup by name gives kind and scope, and refuses ambiguity.
    /// Independent networks may use the same name, in the same scope or not; only the id says
    /// which network a drawing is part of.
    #[serde(default = "project_scope", skip_serializing_if = "is_project_scope")]
    pub scope: String,
    /// What it is, as people describe it (`24 V DC`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl NodeEntry {
    /// A potential or interruption in the project scope, or a junction (no name).
    pub fn new(id: NodeId, kind: NodeKind, name: &str) -> Self {
        NodeEntry { id, kind, name: name.trim().to_owned(), scope: project_scope(), description: String::new() }
    }
}

/// Property keys of a PLC channel.
pub const ADDRESS: &str = "address";
pub const SYMBOL: &str = "symbol";
pub const COMMENT: &str = "comment";

impl Function {
    /// A property's value, or `""`.
    /// A symbol-local point in page coordinates.
    pub fn to_page(&self, local: Point) -> Point {
        let local = if self.mirrored { Point::new(-local.x, local.y) } else { local };
        self.position.offset(self.rotation.apply(local))
    }

    /// A symbol-local direction on the page.
    pub fn to_page_dir(&self, d: crate::Direction) -> crate::Direction {
        let d = match (self.mirrored, d) {
            (true, crate::Direction::Left) => crate::Direction::Right,
            (true, crate::Direction::Right) => crate::Direction::Left,
            (_, d) => d,
        };
        self.rotation.apply_dir(d)
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
    /// Where the wire's middle run lies, when it isn't halfway: the y of a horizontal middle run
    /// or the x of a vertical one. Set to keep wires off each other and off symbols.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<f64>,
}

/// A wire not drawn yet (D1): it joins two ends, terminals or nodes, and appears in the wire
/// list, the netlist and the checks; drawing it ([`Command::DrawWire`]) makes it a drawn
/// [`Connection`] with the same id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: ConnectionId,
    pub from: crate::Endpoint,
    pub to: crate::Endpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_number: Option<String>,
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

/// A dashed box on a page around part of the drawing, labelled with where that part is or
/// what it belongs to: a location (`+A101`), a connector housing (`-XCPR1`) or a field device
/// (`EXT`). Devices drawn inside it are in that place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocationBox {
    pub id: BoxId,
    pub page: PageId,
    pub min: Point,
    pub max: Point,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl LocationBox {
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
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

/// Refuse a value carrying a NaN or infinite number (a coordinate, a size).
fn finite<T: Serialize>(value: &T) -> Result<()> {
    crate::finite::check(value).map_err(|e| ModelError::NotFinite(e.0))
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
    /// The poles as (first, second) designations: `[("13", "14")]`; a changeover is one pole
    /// `("12/14", "11")`.
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
    /// Flip a drawing left to right (`to`: mirrored or not).
    Mirror {
        id: FunctionId,
        to: bool,
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
    /// How wire lengths are estimated.
    SetWireLengthSettings(crate::WireLengthSettings),
    /// Put an underlay behind a page, change it, or (`None`) take it away.
    SetUnderlay {
        page: PageId,
        underlay: Option<Box<crate::underlay::Underlay>>,
    },
    /// Put the pages in this order (every page, once). Only the pages that move get a new
    /// sort key; the others keep theirs (sparse keys, so a page fits between two).
    ReorderPages(Vec<PageId>),
    /// Give these pages these exact sort keys, leaving the others': how a reorder is undone.
    SetPageKeys(BTreeMap<PageId, u64>),
    /// Show another device's terminals with this drawing (D2): its points get terminals of
    /// `device`, with these designations, and its wires go with them: a rewiring. The
    /// terminals only this drawing showed are removed. `terminals`: the ids to use, by point
    /// (how a reassign is undone); empty for new ones.
    Reassign {
        id: FunctionId,
        device: DeviceId,
        designations: BTreeMap<String, String>,
        #[serde(default)]
        terminals: BTreeMap<String, TerminalId>,
    },
    /// A wire not drawn yet, between two terminals or nodes (D1).
    AddWire(Wire),
    /// Remove a wire that isn't drawn.
    RemoveWire(ConnectionId),
    /// Draw a wire not drawn yet, between drawn points that show its ends: it keeps its id,
    /// number and make.
    DrawWire {
        id: ConnectionId,
        from: ConnectionRef,
        to: ConnectionRef,
    },
    /// A node (D1): a junction, potential or interruption, drawn or not yet.
    AddNode(NodeEntry),
    /// Remove a node no drawing shows.
    RemoveNode(NodeId),
    /// Rename or rescope a node (its kind and id don't change).
    SetNode(NodeEntry),
    /// Bind a node drawing to another node of its kind: an explicit engineering operation,
    /// the only way two drawings become one network (D33).
    Rebind {
        id: FunctionId,
        node: NodeId,
    },
    /// A terminal, drawn or not yet (format 2 files list them before the drawings).
    AddTerminal(TerminalEntry),
    /// Remove a terminal no drawing shows.
    RemoveTerminal(TerminalId),
    /// Draw a location box (at least 4 mm each way).
    /// Make a page a layout page with this mounting plate (`None`: an ordinary page again).
    SetPanel {
        page: PageId,
        panel: Option<crate::panel::Panel>,
    },
    AddPanelItem(crate::panel::PanelItem),
    UpdatePanelItem(crate::panel::PanelItem),
    RemovePanelItem(PanelItemId),
    AddBox(LocationBox),
    /// Move, resize or relabel a box.
    UpdateBox(LocationBox),
    RemoveBox(BoxId),
    /// Add a cable (its tag must be new).
    AddCable(Cable),
    /// Change a cable's tag, type, cores or shield.
    UpdateCable(Cable),
    /// Remove a cable no wire runs in.
    RemoveCable(CableId),
    /// Move a wire's middle run (`None`: halfway again).
    SetWireBend {
        id: ConnectionId,
        bend: Option<f64>,
    },
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
    RemovePart(ProductId),
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
    #[error("a wire's spare length is 0 to 10000 mm, and its rounding 0 (off) or 1 to 10000 mm")]
    InvalidWireLengthSettings,
    #[error("the project name must not be empty")]
    EmptyProjectName,
    #[error("a sheet has 1 to 50 columns, numbered from 0 or 1, and up to 26 rows")]
    InvalidSheet,
    #[error("a wire's bend must be a finite number")]
    InvalidBend,
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
    #[error("wire {0} is not in the project, or is drawn")]
    NoUndrawnWire(Uuid),
    #[error("a wire's two ends must differ and exist")]
    BadWireEnds,
    #[error("those points don't show wire {0}'s ends")]
    WireDrawnElsewhere(Uuid),
    #[error("node {0} is not in the project")]
    NoNode(Uuid),
    #[error("node {0} is shown by a drawing")]
    NodeInUse(Uuid),
    #[error("a {0} symbol shows a node of that kind, not a device")]
    NodeDrawingNeedsNode(&'static str),
    #[error("a device's symbol can't show a node")]
    NotANodeSymbol,
    #[error("a junction is drawn once")]
    JunctionDrawnTwice,
    #[error("a {0} needs a name")]
    EmptyNodeName(&'static str),
    #[error("more than one {0} is named `{1}` in scope `{2}`; say which")]
    AmbiguousNode(&'static str, String, String),
    #[error("no {0} is named `{1}` in scope `{2}`")]
    UnknownNode(&'static str, String, String),
    #[error("terminal {0} is not in the project")]
    NoTerminal(Uuid),
    #[error("terminal {0} is shown by a drawing")]
    TerminalInUse(Uuid),
    #[error("a drawing can only show its own device's terminals")]
    TerminalOfOtherDevice,
    #[error("part `{0}` is already in the catalogue")]
    DuplicatePart(String),
    #[error("more than one manufacturer makes a part numbered `{0}`; say which")]
    AmbiguousPart(String),
    #[error("part `{0}` is not in the catalogue")]
    UnknownPart(String),
    #[error("part `{0}` is used by {1}")]
    PartInUse(String, String),
    #[error("a black box needs at least one terminal, and every terminal a unique, non-empty name")]
    InvalidBlackBox,
    #[error("no box {0:?}")]
    NoBox(BoxId),
    #[error("no panel item {0:?}")]
    NoPanelItem(PanelItemId),
    #[error("page `{0}` has no mounting plate")]
    NoPanel(String),
    #[error("a plate, rail or duct needs a size above zero")]
    InvalidPanelSize,
    #[error(
        "an underlay needs a scale above 0, polylines of two points or more within its size, at most {} points, and to stay within {} km of the page",
        crate::underlay::MAX_POINTS,
        crate::underlay::MAX_PAGE_COORD / 1e6
    )]
    InvalidUnderlay,
    #[error("`{0}` is already on a mounting plate")]
    AlreadyOnPanel(String),
    #[error("page `{0}` still has a mounting plate with items on it")]
    PanelNotEmpty(String),
    #[error("a box must be at least 4 mm wide and high")]
    BoxTooSmall,
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
    #[error("can't compute the content hash: {0}")]
    Hash(String),
    #[error("a number isn't finite (NaN or infinity): {0}")]
    NotFinite(String),
    #[error("the netlist is incomplete: wires end on points their symbols no longer have ({0})")]
    IncompleteNetlist(String),
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
    /// The junctions, potentials and interruptions (D1), by id.
    #[serde(default)]
    pub nodes: BTreeMap<NodeId, NodeEntry>,
    /// Wires not drawn yet (D1), by id. Drawn wires are [`Project::connections`].
    #[serde(default)]
    pub undrawn: BTreeMap<ConnectionId, Wire>,
    /// The physical connection points of the devices (D2), by id.
    #[serde(default)]
    pub terminals: BTreeMap<TerminalId, TerminalEntry>,
    /// The parts catalogue (the project's products), by product id.
    #[serde(default)]
    pub parts: BTreeMap<ProductId, Part>,
    /// What the title block shows besides the project name.
    #[serde(default)]
    pub info: ProjectInfo,
    /// How new tags and wire numbers are formed.
    #[serde(default)]
    pub naming: crate::Naming,
    /// How wire lengths are estimated from the panel layout.
    #[serde(default)]
    pub wire_length_settings: crate::WireLengthSettings,
    /// Cables, by id; wires refer to their cores.
    #[serde(default)]
    pub cables: BTreeMap<CableId, Cable>,
    /// Location boxes on the pages.
    #[serde(default)]
    pub boxes: BTreeMap<BoxId, LocationBox>,
    /// The mounting plate of each layout page.
    #[serde(default)]
    pub panels: BTreeMap<PageId, crate::panel::Panel>,
    /// Rails, ducts and device footprints on the plates.
    #[serde(default)]
    pub panel_items: BTreeMap<PanelItemId, crate::panel::PanelItem>,
    /// Drawings shown behind pages to redraw over (saved in files of their own).
    #[serde(skip)]
    pub underlays: BTreeMap<PageId, crate::underlay::Underlay>,
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
        if f.node.is_none() {
            self.device(f.device)?;
        }
        Ok((self.label_of(f).to_owned(), self.designation(f, &r.point).to_owned()))
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
                let names: Vec<&str> = sym.connection_points.iter().map(|cp| self.designation(f, &cp.name)).collect();
                let (poles, terminals) = if let (ContactKind::Changeover, [common, nc, no]) = (kind, names.as_slice()) {
                    // One row, read `11 12/14`: the common, then the closed and open sides.
                    (vec![(format!("{nc}/{no}"), (*common).to_owned())], format!("{common}-{nc}-{no}"))
                } else {
                    let poles: Vec<(String, String)> = names
                        .chunks(2)
                        .map(|p| (p[0].to_owned(), p.get(1).map_or_else(String::new, |s| (*s).to_owned())))
                        .collect();
                    let terminals = poles
                        .iter()
                        .map(|(a, b)| if b.is_empty() { a.clone() } else { format!("{a}-{b}") })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (poles, terminals)
                };
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

    /// The contact blocks a device's parts bring, in parts order. A part used more than once
    /// brings its blocks again one position on, as printed on stacked blocks: `13-14` twice is
    /// `13-14` and `23-24`.
    pub fn contact_blocks_of(&self, device: DeviceId) -> Vec<Vec<String>> {
        let Ok(d) = self.device(device) else { return Vec::new() };
        let mut out = Vec::new();
        for r in &d.parts {
            let Some(part) = self.parts.get(&r.product) else { continue };
            for copy in 0..r.quantity.max(1) {
                for block in &part.contacts {
                    let points = crate::parts::block_points(block);
                    let shifted: Option<Vec<String>> = points
                        .iter()
                        .map(|p| p.parse::<u32>().ok().filter(|n| *n >= 10).map(|n| (n + 10 * copy).to_string()))
                        .collect();
                    // Numbers that can't move a position on (not two digits) only count once.
                    match shifted {
                        Some(s) => out.push(s),
                        None if copy == 0 => out.push(points),
                        None => {}
                    }
                }
            }
        }
        out
    }

    /// Numbers for a new contact of `kind` from the device's contact blocks: the first block
    /// with that contact free. A changeover block also gives a normally open (`11-14`) or
    /// closed (`11-12`) contact; its common may then be shared. `None` when no block fits.
    fn designations_from_blocks(&self, device: DeviceId, kind: ContactKind) -> Option<Vec<String>> {
        // The numbers of each contact already drawn for the device.
        let drawn: Vec<Vec<String>> = self
            .functions_of(device)
            .filter(|f| matches!(self.role(f.id), Ok(Role::Contact { .. })))
            .filter_map(|f| {
                let sym = self.symbol(&f.symbol).ok()?;
                Some(sym.connection_points.iter().map(|cp| self.designation(f, &cp.name).to_owned()).collect())
            })
            .collect();
        let taken = |numbers: &[&String]| drawn.iter().any(|d| d.iter().eq(numbers.iter().copied()));
        for block in self.contact_blocks_of(device) {
            let Some(block_kind) = crate::parts::block_kind(&block.join("-")) else { continue };
            let all: Vec<&String> = block.iter().collect();
            // The block's points the contact takes, and the drawings that would already use them.
            let (take, uses): (Vec<&String>, Vec<Vec<&String>>) = match (kind, block_kind, block.as_slice()) {
                (ContactKind::Changeover, ContactKind::Changeover, [c, nc, no]) => {
                    (all.clone(), vec![all.clone(), vec![c, nc], vec![c, no]])
                }
                (k, b, _) if k == b => (all.clone(), vec![all.clone()]),
                (ContactKind::NormallyOpen, ContactKind::Changeover, [c, _, no]) => {
                    (vec![c, no], vec![all.clone(), vec![c, no]])
                }
                (ContactKind::NormallyClosed, ContactKind::Changeover, [c, nc, _]) => {
                    (vec![c, nc], vec![all.clone(), vec![c, nc]])
                }
                _ => continue,
            };
            if !uses.iter().any(|u| taken(u)) {
                return Some(take.into_iter().cloned().collect());
            }
        }
        None
    }

    /// Designations for a new drawing of `device` with `symbol`:
    /// - a contact takes the numbers of the first free contact on its device's parts' contact
    ///   blocks; without one it gets IEC numbering, where the first digit counts the device's
    ///   contacts (1x, 2x, 3x…) and the second is the symbol's own (3/4 for NO, 1/2 for NC);
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
        if let Role::Contact { kind } = sym.role
            && let Some(numbers) = self.designations_from_blocks(device, kind)
            && numbers.len() == sym.connection_points.len()
        {
            return sym.connection_points.iter().map(|cp| cp.name.clone()).zip(numbers).collect();
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

    /// What a drawing shows: a device (its symbol is a device's) or a node of the symbol's kind,
    /// bound by id. A junction is drawn once.
    fn check_owner(&self, f: &Function) -> Result<()> {
        let sym = self.symbol(&f.symbol)?;
        match (crate::connectivity::node_kind(sym), f.node) {
            (None, None) => self.device(f.device).map(drop),
            (None, Some(_)) => Err(ModelError::NotANodeSymbol),
            (Some(kind), None) => Err(ModelError::NodeDrawingNeedsNode(kind.label())),
            (Some(kind), Some(n)) => {
                let node = self.nodes.get(&n).ok_or(ModelError::NoNode(n.0))?;
                if node.kind != kind {
                    return Err(ModelError::NodeDrawingNeedsNode(kind.label()));
                }
                if !f.device.is_none() {
                    return Err(ModelError::NodeDrawingNeedsNode(kind.label()));
                }
                if kind == NodeKind::Junction && self.functions.values().any(|g| g.id != f.id && g.node == Some(n)) {
                    return Err(ModelError::JunctionDrawnTwice);
                }
                Ok(())
            }
        }
    }

    /// What a drawing of `tag` (a format 1 device id, tag and description) with `symbol` shows
    /// in format 2, creating it if it is missing: a device's symbol, that device; a potential or
    /// interruption point, the node the device stood for ([`NodeId::for_device`], named after
    /// the tag); a junction dot, the node of drawing `f` ([`NodeId::for_junction`]). Returns the
    /// drawing's device (or [`DeviceId::NONE`]) and node. Used to build the samples and to
    /// convert format 1.
    pub fn legacy_owner(
        &mut self,
        device: DeviceId,
        tag: &str,
        description: &str,
        symbol: &SymbolId,
        f: FunctionId,
    ) -> Result<(DeviceId, Option<NodeId>)> {
        let kind = crate::connectivity::node_kind(self.symbol(symbol)?);
        let Some(kind) = kind else {
            if !self.devices.contains_key(&device) {
                self.add_device(Device::new(device, tag, description))?;
            }
            return Ok((device, None));
        };
        let id = if kind == NodeKind::Junction { NodeId::for_junction(f) } else { NodeId::for_device(device) };
        if !self.nodes.contains_key(&id) {
            // A format 1 junction keeps its tag as its display name (section 9).
            let node = NodeEntry { description: description.to_owned(), ..NodeEntry::new(id, kind, tag) };
            self.apply(Command::AddNode(node))?;
        }
        Ok((DeviceId::NONE, Some(id)))
    }

    /// The one node of `kind` named `name` in `scope` (D33): an error if there is none, or more
    /// than one (independent networks with one name).
    pub fn node_by_name(&self, kind: NodeKind, name: &str, scope: &str) -> Result<&NodeEntry> {
        let name = name.trim();
        let found: Vec<&NodeEntry> =
            self.nodes.values().filter(|n| n.kind == kind && n.name == name && n.scope == scope).collect();
        match found.as_slice() {
            [one] => Ok(one),
            [] => Err(ModelError::UnknownNode(kind.label(), name.to_owned(), scope.to_owned())),
            _ => Err(ModelError::AmbiguousNode(kind.label(), name.to_owned(), scope.to_owned())),
        }
    }

    /// The kind of node a symbol's drawings show, if they show a node.
    pub fn node_kind_of(&self, symbol: &SymbolId) -> Option<NodeKind> {
        self.symbol(symbol).ok().and_then(crate::connectivity::node_kind)
    }

    /// The name a drawing is labelled with: its node's name, or its device's tag.
    pub fn label_of(&self, f: &Function) -> &str {
        match f.node {
            Some(n) => self.nodes.get(&n).map_or("", |n| n.name.as_str()),
            None => self.device(f.device).map_or("", |d| d.tag.as_str()),
        }
    }

    /// The drawings of a node.
    pub fn drawings_of_node(&self, node: NodeId) -> impl Iterator<Item = &Function> {
        self.functions.values().filter(move |f| f.node == Some(node))
    }

    /// The designation a drawing shows for one of its connection points: its terminal's, or
    /// the point's own name (node drawings, a point without a terminal).
    pub fn designation<'a>(&'a self, f: &'a Function, point: &'a str) -> &'a str {
        f.terminals.get(point).and_then(|t| self.terminals.get(t)).map_or(point, |t| t.designation.as_str())
    }

    /// Whether a wire not drawn yet ends on `e`.
    pub fn wire_uses(&self, e: crate::Endpoint) -> bool {
        self.undrawn.values().any(|w| w.from == e || w.to == e)
    }

    /// `(tag or name, designation)` of a wire's end, drawn or not.
    pub fn describe_end(&self, e: &crate::Endpoint) -> (String, String) {
        match e {
            crate::Endpoint::Terminal(t) => match self.terminals.get(t) {
                Some(t) => (self.device(t.device).map(|d| d.tag.clone()).unwrap_or_default(), t.designation.clone()),
                None => (String::new(), String::new()),
            },
            crate::Endpoint::Node(n) => (self.nodes.get(n).map(|n| n.name.clone()).unwrap_or_default(), String::new()),
        }
    }

    /// A PLC channel drawing's typed data: on the terminal of its first point.
    pub fn channel_of(&self, f: &Function) -> Option<&ChannelData> {
        f.terminals.values().filter_map(|t| self.terminals.get(t)).find_map(|t| t.channel.as_ref())
    }

    /// A drawing's property: a PLC channel's address, symbol and comment from its terminal,
    /// anything else from the drawing.
    pub fn property<'a>(&'a self, f: &'a Function, key: &str) -> &'a str {
        if let Some(value) = self.channel_of(f).and_then(|c| c.get(key)) {
            return value;
        }
        f.properties.get(key).map_or("", String::as_str)
    }

    /// A drawing's properties as one map: its own, and a channel's typed data.
    pub fn properties_of(&self, f: &Function) -> BTreeMap<String, String> {
        let mut out = f.properties.clone();
        if let Some(c) = self.channel_of(f) {
            for key in CHANNEL_KEYS {
                let v = c.get(key).unwrap_or("");
                if !v.is_empty() {
                    out.insert(key.to_owned(), v.to_owned());
                }
            }
        }
        out
    }

    /// The designations a drawing shows that differ from its symbol's point names.
    pub fn designations_of(&self, f: &Function) -> BTreeMap<String, String> {
        f.terminals
            .iter()
            .filter_map(|(point, t)| self.terminals.get(t).map(|t| (point, &t.designation)))
            .filter(|(point, d)| *point != *d)
            .map(|(point, d)| (point.clone(), d.clone()))
            .collect()
    }

    /// Insert a drawing, giving each of its physical connection points a terminal: the one it
    /// names, if it exists (it must be the drawing's device's), or a new one with the id it
    /// names or the drawing's default id, and the designation `f.designations` gives it (else
    /// the point's name). Returns the terminals created. Nothing changes on error.
    fn attach_terminals(&mut self, mut f: Function) -> Result<Vec<TerminalId>> {
        let sym = self.symbol(&f.symbol)?.clone();
        let mut created = Vec::new();
        let mut new_entries = Vec::new();
        if crate::connectivity::bears_terminals(&sym) {
            let numbered = sym.role.numbered();
            for cp in &sym.connection_points {
                let id = f
                    .terminals
                    .get(&cp.name)
                    .copied()
                    .unwrap_or_else(|| crate::connectivity::default_terminal(f.id, &cp.name));
                match self.terminals.get(&id) {
                    Some(t) if t.device != f.device => return Err(ModelError::TerminalOfOtherDevice),
                    Some(_) => {}
                    None if new_entries.iter().any(|t: &TerminalEntry| t.id == id) => {}
                    None => {
                        let designation = f.designations.get(&cp.name).cloned().unwrap_or_else(|| cp.name.clone());
                        let (group, point) = crate::connectivity::default_group(f.id, id, &cp.name, numbered);
                        // A channel's typed data goes on the terminal of its first point.
                        let channel = match sym.role {
                            Role::PlcChannel { kind }
                                if new_entries.is_empty() && cp.name == sym.connection_points[0].name =>
                            {
                                let mut c = ChannelData {
                                    kind,
                                    address: String::new(),
                                    symbol: String::new(),
                                    comment: String::new(),
                                };
                                for key in CHANNEL_KEYS {
                                    c.set(key, f.properties.get(key).cloned().unwrap_or_default());
                                }
                                Some(c)
                            }
                            _ => None,
                        };
                        new_entries.push(TerminalEntry { id, device: f.device, group, point, designation, channel });
                    }
                }
                f.terminals.insert(cp.name.clone(), id);
            }
        }
        for t in new_entries {
            created.push(t.id);
            self.terminals.insert(t.id, t);
        }
        f.designations.clear();
        if matches!(sym.role, Role::PlcChannel { .. }) {
            for key in CHANNEL_KEYS {
                f.properties.remove(key);
            }
        }
        self.functions.insert(f.id, f);
        Ok(created)
    }

    /// Take a drawing out, removing the terminals only it showed. Returns the drawing as it
    /// would be placed again: its terminal ids and their designations.
    fn detach_terminals(&mut self, id: FunctionId) -> Result<Function> {
        let mut f = self.functions.remove(&id).ok_or(ModelError::NoFunction(id))?;
        f.designations = self.designations_of(&f);
        // A channel's data back on the drawing, so placing it again restores it.
        if let Some(c) = self.channel_of(&f) {
            for key in CHANNEL_KEYS {
                let value = c.get(key).unwrap_or("").to_owned();
                if !value.is_empty() {
                    f.properties.insert(key.to_owned(), value);
                }
            }
        }
        for t in f.terminals.values() {
            let shown = self.functions.values().any(|g| g.terminals.values().any(|u| u == t));
            // A terminal a wire not drawn yet ends on stays, undrawn.
            if !shown && !self.wire_uses(crate::Endpoint::Terminal(*t)) {
                self.terminals.remove(t);
            }
        }
        Ok(f)
    }

    /// A drawing and the terminals it shows, to put back exactly if a change fails.
    fn clone_drawing_state(&self, id: FunctionId) -> Result<(Function, Vec<TerminalEntry>)> {
        let f = self.function(id)?.clone();
        let terminals = f.terminals.values().filter_map(|t| self.terminals.get(t).cloned()).collect();
        Ok((f, terminals))
    }

    fn restore_drawing_state(&mut self, id: FunctionId, (f, terminals): (Function, Vec<TerminalEntry>)) {
        if let Some(now) = self.functions.remove(&id) {
            for t in now.terminals.values() {
                if !self.functions.values().any(|g| g.terminals.values().any(|u| u == t)) {
                    self.terminals.remove(t);
                }
            }
        }
        for t in terminals {
            self.terminals.insert(t.id, t);
        }
        self.functions.insert(id, f);
    }

    /// The number of a terminal or PLC channel: the designation of its first connection point.
    pub fn drawing_number(&self, f: FunctionId) -> Option<String> {
        let f = self.function(f).ok()?;
        let sym = self.symbol(&f.symbol).ok()?;
        sym.role.numbered().then(|| sym.connection_points.first().map(|cp| self.designation(f, &cp.name).to_owned()))?
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
                    address: self.property(f, ADDRESS).to_owned(),
                    symbol: self.property(f, SYMBOL).to_owned(),
                    comment: self.property(f, COMMENT).to_owned(),
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
            let addr = self.property(f, ADDRESS).strip_prefix(prefix)?;
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
        // Node names count too, so a new junction or potential gets a name of its own.
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|t| self.device_by_tag(t).is_none() && !self.nodes.values().any(|n| n.name == *t))
            .unwrap_or_default()
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
                if self.panel_item_of(id).is_some() {
                    return Err(ModelError::AlreadyOnPanel(tag));
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
                finite(&f)?;
                if self.functions.contains_key(&f.id) {
                    return Err(ModelError::DuplicateId(f.id.0));
                }
                self.page(f.page)?;
                self.check_owner(&f)?;
                let id = f.id;
                let created = self.attach_terminals(f)?;
                if let Err(e) = self.check_terminal(id).and_then(|()| self.check_plc(id)) {
                    self.functions.remove(&id);
                    for t in created {
                        self.terminals.remove(&t);
                    }
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
                let f = self.detach_terminals(id)?;
                undo.insert(0, Command::Place(f));
                Ok(Command::Batch(undo))
            }
            Command::Move { id, to } => {
                finite(&to)?;
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.position, to);
                Ok(Command::Move { id, to: old })
            }
            Command::Rotate { id, to } => {
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.rotation, to);
                Ok(Command::Rotate { id, to: old })
            }
            Command::Mirror { id, to } => {
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                let old = std::mem::replace(&mut f.mirrored, to);
                Ok(Command::Mirror { id, to: old })
            }
            Command::Connect(c) => {
                if self.connections.contains_key(&c.id) || self.undrawn.contains_key(&c.id) {
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
                if !self.undrawn.contains_key(&id) {
                    self.connection(id)?;
                }
                if let Some(n) = &number {
                    self.check_wire_number(n, Some(id))?;
                }
                // Drawn or not, renumbering changes no end (D1).
                let slot = match self.connections.get_mut(&id) {
                    Some(c) => &mut c.wire_number,
                    None => &mut self.undrawn.get_mut(&id).ok_or(ModelError::NoConnection(id))?.wire_number,
                };
                let old = std::mem::replace(slot, number);
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
                if self.functions.values().any(|f| f.page == id)
                    || self.boxes.values().any(|b| b.page == id)
                    || self.panels.contains_key(&id)
                    || self.underlays.contains_key(&id)
                {
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
            Command::SetWireLengthSettings(settings) => {
                if !settings.is_valid() {
                    return Err(ModelError::InvalidWireLengthSettings);
                }
                Ok(Command::SetWireLengthSettings(std::mem::replace(&mut self.wire_length_settings, settings)))
            }
            Command::SetSheet(sheet) => {
                finite(&sheet)?;
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
                let old: BTreeMap<PageId, u64> = self.pages.values().map(|p| (p.id, p.order)).collect();
                for (id, key) in sparse_keys(&order, &old) {
                    if let Some(p) = self.pages.get_mut(&id) {
                        p.order = key;
                    }
                }
                Ok(Command::SetPageKeys(old))
            }
            Command::SetPageKeys(keys) => {
                if let Some(id) = keys.keys().find(|id| !self.pages.contains_key(id)) {
                    return Err(ModelError::NoPage(*id));
                }
                let old: BTreeMap<PageId, u64> =
                    keys.keys().filter_map(|id| self.pages.get(id).map(|p| (*id, p.order))).collect();
                for (id, key) in keys {
                    if let Some(p) = self.pages.get_mut(&id) {
                        p.order = key;
                    }
                }
                Ok(Command::SetPageKeys(old))
            }
            Command::Reassign { id, device, designations, terminals } => {
                self.device(device)?;
                let before = self.clone_drawing_state(id)?;
                let old = self.detach_terminals(id)?;
                let old_device = old.device;
                let old_designations = old.designations.clone();
                let old_terminals = old.terminals.clone();
                let mut moved = Function { device, designations, terminals, ..old };
                for (point, t) in &old_terminals {
                    moved.terminals.entry(point.clone()).or_insert_with(|| {
                        // A new connection point of the other device, the same on every run.
                        TerminalId(Uuid::new_v5(&t.0, device.0.as_bytes()))
                    });
                }
                // The same rules as placing it there: no second terminal or channel with its
                // number, no PLC address used twice.
                let placed = self
                    .attach_terminals(moved)
                    .and_then(|_| self.check_terminal(id))
                    .and_then(|()| self.check_plc(id));
                if let Err(e) = placed {
                    self.restore_drawing_state(id, before);
                    return Err(e);
                }
                Ok(Command::Reassign {
                    id,
                    device: old_device,
                    designations: old_designations,
                    terminals: old_terminals,
                })
            }
            Command::AddWire(w) => {
                if self.connections.contains_key(&w.id) || self.undrawn.contains_key(&w.id) {
                    return Err(ModelError::DuplicateId(w.id.0));
                }
                let exists = |e: &crate::Endpoint| match e {
                    crate::Endpoint::Terminal(t) => self.terminals.contains_key(t),
                    crate::Endpoint::Node(n) => self.nodes.contains_key(n),
                };
                if w.from == w.to || !exists(&w.from) || !exists(&w.to) {
                    return Err(ModelError::BadWireEnds);
                }
                let w = Wire { wire_number: w.wire_number.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()), ..w };
                if let Some(n) = &w.wire_number {
                    self.check_wire_number(n, Some(w.id))?;
                }
                let id = w.id;
                self.undrawn.insert(id, w);
                Ok(Command::RemoveWire(id))
            }
            Command::RemoveWire(id) => {
                let w = self.undrawn.remove(&id).ok_or(ModelError::NoUndrawnWire(id.0))?;
                Ok(Command::AddWire(w))
            }
            Command::DrawWire { id, from, to } => {
                let w = self.undrawn.get(&id).cloned().ok_or(ModelError::NoUndrawnWire(id.0))?;
                let c = self.connectivity();
                let ends = (c.endpoint(&from), c.endpoint(&to));
                if ends != (Some(w.from), Some(w.to)) && ends != (Some(w.to), Some(w.from)) {
                    return Err(ModelError::WireDrawnElsewhere(id.0));
                }
                self.undrawn.remove(&id);
                let drawn =
                    Connection { id, from, to, wire_number: w.wire_number.clone(), spec: w.spec.clone(), bend: None };
                if let Err(e) = self.check_connection(&drawn) {
                    self.undrawn.insert(id, w);
                    return Err(e);
                }
                self.connections.insert(id, drawn);
                Ok(Command::Batch(vec![Command::Disconnect(id), Command::AddWire(w)]))
            }
            Command::AddNode(n) => {
                if self.nodes.contains_key(&n.id) {
                    return Err(ModelError::DuplicateId(n.id.0));
                }
                let n = check_node(n)?;
                let id = n.id;
                self.nodes.insert(id, n);
                Ok(Command::RemoveNode(id))
            }
            Command::RemoveNode(id) => {
                if self.functions.values().any(|f| f.node == Some(id)) || self.wire_uses(crate::Endpoint::Node(id)) {
                    return Err(ModelError::NodeInUse(id.0));
                }
                let n = self.nodes.remove(&id).ok_or(ModelError::NoNode(id.0))?;
                Ok(Command::AddNode(n))
            }
            Command::SetNode(n) => {
                let old = self.nodes.get(&n.id).ok_or(ModelError::NoNode(n.id.0))?;
                let n = check_node(NodeEntry { kind: old.kind, ..n })?;
                let old = self.nodes.insert(n.id, n).ok_or(ModelError::NoNode(Uuid::nil()))?;
                Ok(Command::SetNode(old))
            }
            Command::Rebind { id, node } => {
                let f = self.function(id)?.clone();
                let old = f.node.ok_or(ModelError::NotANodeSymbol)?;
                self.check_owner(&Function { node: Some(node), ..f.clone() })?;
                if let Some(f) = self.functions.get_mut(&id) {
                    f.node = Some(node);
                }
                Ok(Command::Rebind { id, node: old })
            }
            Command::AddTerminal(t) => {
                if self.terminals.contains_key(&t.id) {
                    return Err(ModelError::DuplicateId(t.id.0));
                }
                self.device(t.device)?;
                let id = t.id;
                self.terminals.insert(id, t);
                Ok(Command::RemoveTerminal(id))
            }
            Command::RemoveTerminal(id) => {
                if self.functions.values().any(|f| f.terminals.values().any(|t| *t == id))
                    || self.wire_uses(crate::Endpoint::Terminal(id))
                {
                    return Err(ModelError::TerminalInUse(id.0));
                }
                let t = self.terminals.remove(&id).ok_or(ModelError::NoTerminal(id.0))?;
                Ok(Command::AddTerminal(t))
            }
            Command::SetUnderlay { page, underlay } => {
                self.page(page)?;
                if underlay.as_ref().is_some_and(|u| !u.is_valid()) {
                    return Err(ModelError::InvalidUnderlay);
                }
                let old = match underlay {
                    Some(u) => self.underlays.insert(page, *u),
                    None => self.underlays.remove(&page),
                };
                Ok(Command::SetUnderlay { page, underlay: old.map(Box::new) })
            }
            Command::SetPanel { page, panel } => {
                let name = self.page(page)?.name.clone();
                if let Some(p) = &panel
                    && !(p.width > 0.0 && p.height > 0.0 && p.width.is_finite() && p.height.is_finite())
                {
                    return Err(ModelError::InvalidPanelSize);
                }
                if panel.is_none() && self.panel_items.values().any(|i| i.page == page) {
                    return Err(ModelError::PanelNotEmpty(name));
                }
                let old = match panel {
                    Some(p) => self.panels.insert(page, p),
                    None => self.panels.remove(&page),
                };
                Ok(Command::SetPanel { page, panel: old })
            }
            Command::AddPanelItem(item) => {
                self.check_panel_item(&item, None)?;
                if self.panel_items.contains_key(&item.id) {
                    return Err(ModelError::DuplicateId(item.id.0));
                }
                let id = item.id;
                self.panel_items.insert(id, item);
                Ok(Command::RemovePanelItem(id))
            }
            Command::UpdatePanelItem(item) => {
                if !self.panel_items.contains_key(&item.id) {
                    return Err(ModelError::NoPanelItem(item.id));
                }
                self.check_panel_item(&item, Some(item.id))?;
                let slot = self.panel_items.get_mut(&item.id).ok_or(ModelError::NoPanelItem(item.id))?;
                Ok(Command::UpdatePanelItem(std::mem::replace(slot, item)))
            }
            Command::RemovePanelItem(id) => {
                let item = self.panel_items.remove(&id).ok_or(ModelError::NoPanelItem(id))?;
                Ok(Command::AddPanelItem(item))
            }
            Command::AddBox(b) => {
                finite(&b)?;
                let b = self.checked_box(b)?;
                if self.boxes.contains_key(&b.id) {
                    return Err(ModelError::DuplicateId(b.id.0));
                }
                let id = b.id;
                self.boxes.insert(id, b);
                Ok(Command::RemoveBox(id))
            }
            Command::UpdateBox(b) => {
                finite(&b)?;
                let b = self.checked_box(b)?;
                let slot = self.boxes.get_mut(&b.id).ok_or(ModelError::NoBox(b.id))?;
                Ok(Command::UpdateBox(std::mem::replace(slot, b)))
            }
            Command::RemoveBox(id) => {
                let b = self.boxes.remove(&id).ok_or(ModelError::NoBox(id))?;
                Ok(Command::AddBox(b))
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
            Command::SetWireBend { id, bend } => {
                if bend.is_some_and(|b| !b.is_finite()) {
                    return Err(ModelError::InvalidBend);
                }
                let c = self.connections.get_mut(&id).ok_or(ModelError::NoConnection(id))?;
                Ok(Command::SetWireBend { id, bend: std::mem::replace(&mut c.bend, bend) })
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
                finite(&symbol)?;
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
                let part = self.check_part(part, true)?;
                let id = part.id;
                self.parts.insert(id, part);
                Ok(Command::RemovePart(id))
            }
            Command::UpdatePart(part) => {
                let part = self.check_part(part, false)?;
                let slot = self.parts.get_mut(&part.id).ok_or_else(|| ModelError::UnknownPart(part.number.clone()))?;
                let old = std::mem::replace(slot, part);
                Ok(Command::UpdatePart(old))
            }
            Command::RemovePart(id) => {
                let name = self.parts.get(&id).map_or_else(|| id.0.to_string(), Part::label);
                let mut users: Vec<&str> = self
                    .devices
                    .values()
                    .filter(|d| d.parts.iter().any(|p| p.product == id))
                    .map(|d| d.tag.as_str())
                    .collect();
                if !users.is_empty() {
                    users.sort();
                    return Err(ModelError::PartInUse(name, users.join(", ")));
                }
                let part = self.parts.remove(&id).ok_or(ModelError::UnknownPart(name))?;
                Ok(Command::AddPart(part))
            }
            Command::SetDeviceParts { id, parts } => {
                self.check_part_refs(&parts)?;
                let d = self.devices.get_mut(&id).ok_or(ModelError::NoDevice(id))?;
                let old = std::mem::replace(&mut d.parts, parts);
                Ok(Command::SetDeviceParts { id, parts: old })
            }
            Command::SetProperties { id, properties } => {
                let mut properties: BTreeMap<String, String> = properties
                    .into_iter()
                    .map(|(k, v)| (k, v.trim().to_owned()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect();
                let before = self.clone_drawing_state(id)?;
                let old = self.properties_of(&before.0);
                // A channel's address, symbol and comment go to its terminal, typed.
                let channel = self
                    .function(id)?
                    .terminals
                    .values()
                    .copied()
                    .find(|t| self.terminals.get(t).is_some_and(|t| t.channel.is_some()));
                if let Some(t) = channel.and_then(|t| self.terminals.get_mut(&t)).and_then(|t| t.channel.as_mut()) {
                    for key in CHANNEL_KEYS {
                        t.set(key, properties.remove(key).unwrap_or_default());
                    }
                }
                let f = self.functions.get_mut(&id).ok_or(ModelError::NoFunction(id))?;
                f.properties = properties;
                if let Err(e) = self.check_plc(id) {
                    self.restore_drawing_state(id, before);
                    return Err(e);
                }
                Ok(Command::SetProperties { id, properties: old })
            }
            Command::SetDesignations { id, designations } => {
                // The designations are the drawing's terminals': renumbering changes no id
                // and no wire.
                let f = self.function(id)?.clone();
                let mut old = BTreeMap::new();
                for (point, t) in &f.terminals {
                    let Some(entry) = self.terminals.get_mut(t) else { continue };
                    let new = designations.get(point).cloned().unwrap_or_else(|| point.clone());
                    if new != entry.designation {
                        old.insert(point.clone(), std::mem::replace(&mut entry.designation, new));
                    }
                }
                // Points the map leaves out show the symbol's name again; undo restores exactly.
                let undo: BTreeMap<String, String> = f
                    .terminals
                    .iter()
                    .filter_map(|(point, t)| {
                        let now = self.terminals.get(t)?.designation.clone();
                        Some((point.clone(), old.get(point).cloned().unwrap_or(now)))
                    })
                    .collect();
                if let Err(e) = self.check_terminal(id) {
                    for (point, d) in old {
                        if let Some(entry) = f.terminals.get(&point).and_then(|t| self.terminals.get_mut(t)) {
                            entry.designation = d;
                        }
                    }
                    return Err(e);
                }
                Ok(Command::SetDesignations { id, designations: undo })
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
        let mine: Vec<TerminalId> = self.function(f)?.terminals.values().copied().collect();
        // Another drawing of the same terminal (a repeated representation) is no second one.
        let taken = self.functions_of(device).any(|g| {
            g.id != f
                && !g.terminals.values().any(|t| mine.contains(t))
                && self.drawing_number(g.id).as_deref() == Some(number.as_str())
        });
        if taken {
            return Err(ModelError::DuplicateNumber(self.device(device)?.tag.clone(), number));
        }
        Ok(())
    }

    /// A PLC channel's address and symbol, when set, are unique in the project; addresses
    /// however they are written.
    fn check_plc(&self, f: FunctionId) -> Result<()> {
        let func = self.function(f)?;
        if !matches!(self.role(f)?, Role::PlcChannel { .. }) {
            return Ok(());
        }
        for (key, is_address) in [(ADDRESS, true), (SYMBOL, false)] {
            let value = self.property(func, key);
            if value.is_empty() {
                continue;
            }
            let other = self.functions.values().find(|g| {
                g.id != f
                    && matches!(self.role(g.id), Ok(Role::PlcChannel { .. }))
                    && if is_address {
                        // `%I0.0` and `I00.0` are the same input.
                        crate::plc_address::same(self.property(g, key), value)
                    } else {
                        self.property(g, key) == value
                    }
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

    /// A product as it may enter the catalogue: a non-empty number (trimmed), finite sizes, and
    /// no other product with the same manufacturer and number (the same number from two makers
    /// is fine). `new`: the id must be free; otherwise it must exist.
    fn check_part(&self, part: Part, new: bool) -> Result<Part> {
        finite(&part)?;
        let part =
            Part { number: part.number.trim().to_owned(), manufacturer: part.manufacturer.trim().to_owned(), ..part };
        if part.number.is_empty() {
            return Err(ModelError::EmptyPartNumber);
        }
        if new && self.parts.contains_key(&part.id) {
            return Err(ModelError::DuplicateId(part.id.0));
        }
        let key = part.key();
        if self.parts.values().any(|other| other.id != part.id && other.key() == key) {
            return Err(ModelError::DuplicatePart(part.label()));
        }
        Ok(part)
    }

    /// The products numbered `number` (trimmed), by any manufacturer.
    pub fn parts_numbered(&self, number: &str) -> Vec<&Part> {
        let number = number.trim();
        self.parts.values().filter(|p| p.number == number).collect()
    }

    /// The one product numbered `number`: an error if there is none, or more than one maker's.
    pub fn part_by_number(&self, number: &str) -> Result<&Part> {
        match self.parts_numbered(number).as_slice() {
            [one] => Ok(one),
            [] => Err(ModelError::UnknownPart(number.trim().to_owned())),
            _ => Err(ModelError::AmbiguousPart(number.trim().to_owned())),
        }
    }

    fn check_part_refs(&self, parts: &[PartRef]) -> Result<()> {
        for p in parts {
            if !self.parts.contains_key(&p.product) {
                return Err(ModelError::UnknownPart(p.product.0.to_string()));
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

    /// A box with its corners in order, its label trimmed, on an existing page.
    /// A panel item must be on a layout page; a rail or duct needs a length (and a duct a
    /// width); a device must exist and be on no other plate position.
    fn check_panel_item(&self, item: &crate::panel::PanelItem, replacing: Option<PanelItemId>) -> Result<()> {
        use crate::panel::PanelItemKind;
        let name = self.page(item.page)?.name.clone();
        if !self.panels.contains_key(&item.page) {
            return Err(ModelError::NoPanel(name));
        }
        if !(item.at.x.is_finite() && item.at.y.is_finite()) {
            return Err(ModelError::InvalidPanelSize);
        }
        let positive = |v: f64| v > 0.0 && v.is_finite();
        match &item.kind {
            PanelItemKind::Rail if !positive(item.length) => Err(ModelError::InvalidPanelSize),
            PanelItemKind::Duct { width, depth }
                if !(positive(item.length) && positive(*width) && positive(*depth)) =>
            {
                Err(ModelError::InvalidPanelSize)
            }
            PanelItemKind::Device { device } => {
                let tag = self.device(*device)?.tag.clone();
                match self.panel_item_of(*device) {
                    Some(other) if Some(other.id) != replacing => Err(ModelError::AlreadyOnPanel(tag)),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn checked_box(&self, b: LocationBox) -> Result<LocationBox> {
        self.page(b.page)?;
        let min = Point::new(b.min.x.min(b.max.x), b.min.y.min(b.max.y));
        let max = Point::new(b.min.x.max(b.max.x), b.min.y.max(b.max.y));
        if max.x - min.x < 4.0 || max.y - min.y < 4.0 {
            return Err(ModelError::BoxTooSmall);
        }
        Ok(LocationBox { min, max, label: b.label.trim().to_owned(), ..b })
    }

    /// The boxes on a page.
    pub fn boxes_on(&self, page: PageId) -> impl Iterator<Item = &LocationBox> {
        self.boxes.values().filter(move |b| b.page == page)
    }

    /// Where a drawing is: the labels of the boxes around it, outermost first (`+A101 -XCPR1`).
    pub fn box_labels(&self, f: FunctionId) -> Vec<String> {
        let Ok(f) = self.function(f) else { return Vec::new() };
        let mut around: Vec<&LocationBox> =
            self.boxes_on(f.page).filter(|b| b.contains(f.position) && !b.label.is_empty()).collect();
        let area = |b: &LocationBox| (b.max.x - b.min.x) * (b.max.y - b.min.y);
        around.sort_by(|a, b| area(b).total_cmp(&area(a)));
        around.into_iter().map(|b| b.label.clone()).collect()
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
        let drawn = self.connections.values().any(|c| Some(c.id) != except && c.wire_number.as_deref() == Some(number));
        let undrawn = self.undrawn.values().any(|w| Some(w.id) != except && w.wire_number.as_deref() == Some(number));
        if drawn || undrawn { Err(ModelError::DuplicateWireNumber(number.to_owned())) } else { Ok(()) }
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
    /// Place a drawing. A node symbol drawn for a device, as format 1 drew potentials,
    /// interruptions and junctions, becomes a drawing of the node that device stands for
    /// ([`Project::legacy_owner`]), and the device goes if nothing else draws it: how the
    /// samples are built.
    pub fn place(&mut self, mut function: Function) -> Result<FunctionId> {
        let id = function.id;
        let is_node = crate::connectivity::node_kind(self.symbol(&function.symbol)?).is_some();
        let known = NodeId::for_device(function.device);
        if is_node
            && function.node.is_none()
            && !self.devices.contains_key(&function.device)
            && self.nodes.contains_key(&known)
        {
            // A device that already became its node.
            function.device = DeviceId::NONE;
            function.node = Some(known);
        }
        if is_node && function.node.is_none() && !function.device.is_none() {
            let d = self.device(function.device)?.clone();
            let (device, node) = self.legacy_owner(d.id, &d.tag, &d.description, &function.symbol, id)?;
            if self.functions_of(d.id).next().is_none() {
                self.apply(Command::RemoveDevice(d.id))?;
            }
            function.device = device;
            function.node = node;
        }
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
        Ok((f.to_page(cp.at), f.to_page_dir(cp.leaves)))
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
    /// The wires that are protective earth conductors, drawn dashed: green-yellow, numbered or
    /// on a potential named PE or PEN, or ending on a point named PE (PE', PE1…).
    pub fn protective_earth_wires(&self) -> std::collections::BTreeSet<ConnectionId> {
        let pe_name = |s: &str| {
            let s = s.trim().to_ascii_uppercase();
            s == "PE" || s == "PEN"
        };
        let mut out: std::collections::BTreeSet<ConnectionId> = self
            .potentials()
            .into_iter()
            .filter(|p| p.names.iter().any(|n| pe_name(n)))
            .flat_map(|p| p.connections)
            .collect();
        let pe_point = |r: &ConnectionRef| {
            let Ok(f) = self.function(r.function) else { return false };
            let d = self.designation(f, &r.point).to_ascii_uppercase();
            d.strip_prefix("PE").is_some_and(|rest| rest.chars().all(|ch| ch == '\'' || ch.is_ascii_digit()))
        };
        for c in self.connections.values() {
            let colour = c.spec.colour.to_ascii_uppercase().replace(['-', '/', ' '], "");
            if colour == "GNYE"
                || colour == "YEGN"
                || c.wire_number.as_deref().is_some_and(pe_name)
                || pe_point(&c.from)
                || pe_point(&c.to)
            {
                out.insert(c.id);
            }
        }
        out
    }

    pub fn route(&self, c: &Connection) -> Result<Vec<Point>> {
        let (a, da) = self.connection_point(&c.from)?;
        let (b, db) = self.connection_point(&c.to)?;
        Ok(route_bent(a, da, b, db, c.bend))
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
    route_bent(a, da, b, db, None)
}

/// Like [`route_between`], with the middle run at `bend` (its y when it runs horizontally, its x
/// when vertically) instead of halfway. A bend doesn't straighten a wire whose ends face each
/// other.
pub fn route_bent(a: Point, da: Direction, b: Point, db: Direction, bend: Option<f64>) -> Vec<Point> {
    let Some(mid) = middle_run(a, da, b, db) else { return vec![a, b] };
    let (sa, sb) = (step(da), step(db));
    let a1 = Point::new(a.x + sa.x * ROUTE_STUB, a.y + sa.y * ROUTE_STUB);
    let b1 = Point::new(b.x + sb.x * ROUTE_STUB, b.y + sb.y * ROUTE_STUB);
    let at = bend.unwrap_or(mid.at);
    let mid = if mid.horizontal {
        [Point::new(a1.x, at), Point::new(b1.x, at)]
    } else {
        [Point::new(at, a1.y), Point::new(at, b1.y)]
    };
    simplify(vec![a, a1, mid[0], mid[1], b1, b])
}

/// A route's middle run, the part a bend moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiddleRun {
    /// Whether it runs horizontally (a bend is then its y) or vertically (its x).
    pub horizontal: bool,
    /// Where it lies without a bend: halfway between the ends' stubs.
    pub at: f64,
    /// The range between the stubs: a bend outside it makes the wire double back.
    pub between: (f64, f64),
}

/// The middle run of the route between two points, or `None` when the ends face each other and
/// the wire is one straight line.
pub fn middle_run(a: Point, da: Direction, b: Point, db: Direction) -> Option<MiddleRun> {
    let facing =
        (a.x == b.x && da.is_vertical() && db.is_vertical()) || (a.y == b.y && !da.is_vertical() && !db.is_vertical());
    if facing && heads(da, a, b) && heads(db, b, a) {
        return None;
    }
    let (sa, sb) = (step(da), step(db));
    let a1 = Point::new(a.x + sa.x * ROUTE_STUB, a.y + sa.y * ROUTE_STUB);
    let b1 = Point::new(b.x + sb.x * ROUTE_STUB, b.y + sb.y * ROUTE_STUB);
    // Keep going along the start direction if that brings us closer (split the other axis at
    // its midpoint); if it would lead away, turn first.
    let horizontal = if heads(da, a1, b1) { da.is_vertical() } else { !da.is_vertical() };
    let (p, q) = if horizontal { (a1.y, b1.y) } else { (a1.x, b1.x) };
    Some(MiddleRun { horizontal, at: (p + q) / 2.0, between: (p.min(q), p.max(q)) })
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

#[cfg(test)]
mod page_key_tests {
    use super::*;

    fn ids(n: u128) -> Vec<PageId> {
        (1..=n).map(PageId::from_u128).collect()
    }

    #[test]
    fn moving_one_page_changes_only_its_key() {
        let pages = ids(5);
        let old: BTreeMap<PageId, u64> =
            pages.iter().enumerate().map(|(i, p)| (*p, PAGE_STEP * (i as u64 + 1))).collect();
        // Page 5 moved between 1 and 2.
        let order = vec![pages[0], pages[4], pages[1], pages[2], pages[3]];
        let new = sparse_keys(&order, &old);
        let changed: Vec<_> = pages.iter().filter(|p| new[p] != old[p]).collect();
        assert_eq!(changed, [&pages[4]]);
        let mut sorted = order.clone();
        sorted.sort_by_key(|p| new[p]);
        assert_eq!(sorted, order);
    }

    #[test]
    fn without_room_every_page_is_rekeyed_in_order() {
        let pages = ids(3);
        let old: BTreeMap<PageId, u64> = [(pages[0], 1), (pages[1], 2), (pages[2], 3)].into();
        let order = vec![pages[0], pages[2], pages[1]];
        let new = sparse_keys(&order, &old);
        let mut sorted = order.clone();
        sorted.sort_by_key(|p| new[p]);
        assert_eq!(sorted, order);
    }

    #[test]
    fn a_reorder_undoes_to_the_exact_keys() {
        let mut p = Project::new("t");
        for (i, name) in ["1", "2", "3"].iter().enumerate() {
            p.add_page(Page { id: PageId::from_u128(i as u128 + 1), ..Page::new(name, PAGE_STEP * (i as u64 + 1)) });
        }
        let before = p.clone();
        let order = vec![PageId::from_u128(3), PageId::from_u128(1), PageId::from_u128(2)];
        let undo = p.apply(Command::ReorderPages(order.clone())).unwrap();
        assert_eq!(p.pages_in_order().iter().map(|pg| pg.id).collect::<Vec<_>>(), order);
        assert_eq!(p.pages[&PageId::from_u128(1)].order, PAGE_STEP, "unmoved pages keep their keys");
        p.apply(undo).unwrap();
        assert_eq!(p, before);
        let unknown: BTreeMap<PageId, u64> = [(PageId::from_u128(9), 1)].into();
        assert_eq!(p.apply(Command::SetPageKeys(unknown)).unwrap_err(), ModelError::NoPage(PageId::from_u128(9)));
    }
}
