//! A project on disk is a folder of pretty-printed JSON files, so it diffs cleanly in git and
//! scripts or agents can read it:
//!
//! ```text
//! my-panel/
//!   project.json          format version, name, pages (with sparse sort keys)
//!   devices.json          all devices, sorted by tag; their products by id, number beside
//!   products.json         the project's products (the parts catalogue), by id
//!   terminals.json        the devices' terminals (D2): id, device, group, point, designation
//!   nodes.json            junctions, potentials and interruptions (D1), with scoped names
//!   bridges.json          internal bridges between terminals of one device (R7)
//!   cables.json           cables, sorted by tag
//!   symbols/<id>.json     one file per symbol used by the project
//!   wires.json            every wire (D1): id, its two ends (terminal or node), number, make
//!   pages/<page-id>.json  the drawings on that page, and the wire graphics drawn there
//!   underlays/<page-id>.json  a drawing shown behind that page, if any (compact)
//! ```
//!
//! Format 2 is written; format 1 is read and converted on load (docs/PLATFORM-ARCHITECTURE.md
//! section 9): `parts.json` becomes `products.json` with ids from [`ProductId::for_number`],
//! devices' parts are found by number, the dense page order becomes sparse keys, and each
//! drawing's designations become terminals with the ids M7 derived (`terminal/<drawing>/<point>`). The first
//! save of a format 1 project writes format 2, which older versions can't open:
//! [`format_of`] lets a program say so first.
//!
//! Loading replays everything through the model's commands, so a hand-edited file that breaks
//! a rule (duplicate tag, wire to a missing point) is rejected with the file named.
//!
//! Saving writes the whole folder as one step, one program at a time, and can refuse to save
//! over someone else's save ([`save_checked`]): see [`store`].

use ecad_model::{
    Cable, Command, Connection, Device, DeviceId, Function, LocationBox, ModelError, Naming, NodeEntry, NodeId,
    NodeKind, PAGE_STEP, Page, Part, PartRef, Point, ProductId, Project, ProjectInfo, Sheet, SymbolDef, SymbolId,
    TerminalEntry,
};

pub mod csv;
pub mod store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The format this program writes. It reads this and format 1.
pub const FORMAT: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
    #[error("{path}: {source}")]
    Model { path: PathBuf, source: ModelError },
    #[error("{path}: project format {found} is newer than this program understands ({FORMAT})")]
    Format { path: PathBuf, found: u32 },
    #[error("symbol id `{0}` can't be used as a file name (letters, digits, `.`, `_`, `-` only)")]
    BadSymbolId(String),
    #[error("{0}: the project changed on disk since it was opened (saved by another program?); not saved")]
    Changed(PathBuf),
    #[error("{0}: another program is saving this project right now; try again")]
    Busy(PathBuf),
    #[error("{path}: over {limit} MB, too big to read")]
    TooBig { path: PathBuf, limit: usize },
    #[error("{path}: invalid format-2 record: {reason}")]
    Canonical { path: PathBuf, reason: String },
}

/// Largest underlay file read back: a little over what [`ecad_model::underlay::MAX_POINTS`]
/// points can take.
pub const MAX_UNDERLAY_FILE: usize = 96 << 20;

/// Read at most `limit` bytes of a file: one more and it is refused, before anything larger is
/// held in memory (the file's stated size isn't trusted; it can grow while being read).
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(io(path))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(io(path))?;
    if bytes.len() > limit {
        return Err(ProjectError::TooBig { path: path.to_owned(), limit: limit >> 20 });
    }
    Ok(bytes)
}

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: u32,
    name: String,
    /// Missing in projects saved before M2a; such projects get a sheet around their drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sheet: Option<Sheet>,
    /// Title-block fields; missing in projects saved before G1.
    #[serde(default, skip_serializing_if = "is_default")]
    info: ProjectInfo,
    /// Tag and wire number formats; missing in projects saved before G1b.
    #[serde(default, skip_serializing_if = "is_default_naming")]
    naming: Naming,
    #[serde(default, skip_serializing_if = "is_default_wire_lengths")]
    wire_lengths: ecad_model::WireLengthSettings,
    pages: Vec<Page>,
}

fn is_default_wire_lengths(s: &ecad_model::WireLengthSettings) -> bool {
    *s == ecad_model::WireLengthSettings::default()
}

fn is_default_naming(n: &Naming) -> bool {
    *n == Naming::default()
}

fn is_default(info: &ProjectInfo) -> bool {
    *info == ProjectInfo::default()
}

/// How one wire is drawn on one page: the drawn points it joins and its bend.
#[derive(Serialize, Deserialize)]
struct WireGraphics {
    wire: ecad_model::ConnectionId,
    from: ecad_model::ConnectionRef,
    to: ecad_model::ConnectionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bend: Option<f64>,
}

/// A wire as saved in `wires.json`. Both canonical endpoints must resolve before save or load.
#[derive(Serialize, Deserialize)]
struct WireFile {
    id: ecad_model::ConnectionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<ecad_model::Endpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<ecad_model::Endpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wire_number: Option<String>,
    #[serde(default, flatten)]
    spec: ecad_model::WireSpec,
}

/// A device as saved: its products by id, with each product's number beside it for people
/// reading the file (format 1 had the number only).
#[derive(Serialize, Deserialize)]
struct DeviceFile {
    id: DeviceId,
    tag: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parts: Vec<PartRefFile>,
}

#[derive(Serialize, Deserialize)]
struct PartRefFile {
    /// Missing in format 1, which names the product by number only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    product: Option<ProductId>,
    #[serde(default)]
    number: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    quantity: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    per_strip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    only: Option<SymbolId>,
}

fn one() -> u32 {
    1
}

fn is_one(q: &u32) -> bool {
    *q == 1
}

impl DeviceFile {
    fn of(project: &Project, d: &Device) -> Self {
        let parts = d
            .parts
            .iter()
            .map(|r| PartRefFile {
                product: Some(r.product),
                number: project.parts.get(&r.product).map(|p| p.number.clone()).unwrap_or_default(),
                quantity: r.quantity,
                per_strip: r.per_strip,
                only: r.only.clone(),
            })
            .collect();
        DeviceFile { id: d.id, tag: d.tag.clone(), description: d.description.clone(), parts }
    }

    /// The device; a format 1 reference by number is the product [`ProductId::for_number`] gives it.
    fn into_device(self) -> Device {
        let parts = self
            .parts
            .into_iter()
            .map(|r| PartRef {
                product: r.product.unwrap_or_else(|| ProductId::for_number(&r.number)),
                quantity: r.quantity,
                per_strip: r.per_strip,
                only: r.only,
            })
            .collect();
        Device { parts, ..Device::new(self.id, &self.tag, &self.description) }
    }
}

/// A format 1 catalogue entry: a part without an id (its number was unique).
#[derive(Deserialize)]
struct PartV1 {
    number: String,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Default)]
struct PageFile {
    functions: Vec<Function>,
    /// Format 1: the wires drawn on the page, with their electrical data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    connections: Vec<Connection>,
    /// Format 2: how wires are drawn on the page; the wires themselves are in `wires.json`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wires: Vec<WireGraphics>,
    /// Location boxes; missing in projects saved before G4b.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    boxes: Vec<LocationBox>,
    /// A layout page's mounting plate and what is on it; missing before M5a.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    panel: Option<ecad_model::Panel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    panel_items: Vec<ecad_model::PanelItem>,
}

/// An underlay's file: compact, as it is geometry nobody reads by hand. Points are `[x, y]`,
/// fills six numbers per triangle.
#[derive(Serialize, Deserialize)]
struct UnderlayFile {
    source: String,
    at: [f64; 2],
    scale: f64,
    visible: bool,
    width: f64,
    height: f64,
    #[serde(default)]
    units: ecad_model::underlay::Units,
    #[serde(default)]
    units_from: ecad_model::underlay::UnitsFrom,
    lines: Vec<Vec<[f64; 2]>>,
    fills: Vec<[f64; 6]>,
}

impl UnderlayFile {
    fn of(u: &ecad_model::underlay::Underlay) -> UnderlayFile {
        let xy = |p: &Point| [p.x, p.y];
        UnderlayFile {
            source: u.source.clone(),
            at: xy(&u.at),
            scale: u.scale,
            visible: u.visible,
            width: u.shapes.width,
            height: u.shapes.height,
            units: u.units,
            units_from: u.units_from,
            lines: u.shapes.lines.iter().map(|l| l.iter().map(xy).collect()).collect(),
            fills: u.shapes.fills.iter().map(|[a, b, c]| [a.x, a.y, b.x, b.y, c.x, c.y]).collect(),
        }
    }

    fn into_underlay(self) -> ecad_model::underlay::Underlay {
        let pt = |[x, y]: [f64; 2]| Point::new(x, y);
        let shapes = ecad_model::underlay::UnderlayShapes {
            lines: self.lines.into_iter().map(|l| l.into_iter().map(pt).collect()).collect(),
            fills: self.fills.into_iter().map(|f| [pt([f[0], f[1]]), pt([f[2], f[3]]), pt([f[4], f[5]])]).collect(),
            width: self.width,
            height: self.height,
        };
        ecad_model::underlay::Underlay {
            source: self.source,
            shapes: std::sync::Arc::new(shapes),
            at: pt(self.at),
            scale: self.scale,
            visible: self.visible,
            units: self.units,
            units_from: self.units_from,
        }
    }
}

pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io { path: path.to_owned(), source }
}

fn model(path: &Path) -> impl FnOnce(ModelError) -> ProjectError + '_ {
    move |source| ProjectError::Model { path: path.to_owned(), source }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    serde_json::from_str(&text).map_err(|source| ProjectError::Json { path: path.to_owned(), source })
}

fn render_json<T: Serialize>(path: &Path, value: &T) -> Result<Vec<u8>> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|source| ProjectError::Json { path: path.to_owned(), source })?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn symbol_file_name(id: &str) -> Result<String> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(format!("{id}.json")) } else { Err(ProjectError::BadSymbolId(id.to_owned())) }
}

/// Every file of a project, as (path inside the folder, contents).
fn render(project: &Project) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    let pages: Vec<Page> = project.pages_in_order().into_iter().cloned().collect();
    let head = ProjectFile {
        format: FORMAT,
        name: project.name.clone(),
        sheet: Some(project.sheet),
        info: project.info.clone(),
        naming: project.naming.clone(),
        wire_lengths: project.wire_length_settings,
        pages,
    };
    files.push(("project.json".to_owned(), render_json(Path::new("project.json"), &head)?));

    let mut parts: Vec<&Part> = project.parts.values().collect();
    parts.sort_by(|a, b| a.key().cmp(&b.key()).then(a.id.cmp(&b.id)));
    files.push(("products.json".to_owned(), render_json(Path::new("products.json"), &parts)?));

    let mut cables: Vec<&Cable> = project.cables.values().collect();
    cables.sort_by(|a, b| a.tag.cmp(&b.tag));
    files.push(("cables.json".to_owned(), render_json(Path::new("cables.json"), &cables)?));

    let bridges: Vec<&ecad_model::connectivity::Bridge> = project.bridges.iter().collect();
    files.push(("bridges.json".to_owned(), render_json(Path::new("bridges.json"), &bridges)?));

    let mut nodes: Vec<&NodeEntry> = project.nodes.values().collect();
    nodes.sort_by(|a, b| (a.kind, &a.scope, &a.name, a.id).cmp(&(b.kind, &b.scope, &b.name, b.id)));
    files.push(("nodes.json".to_owned(), render_json(Path::new("nodes.json"), &nodes)?));

    let tag = |d| project.device(d).map(|d| d.tag.as_str()).unwrap_or("");
    let mut terminals: Vec<&TerminalEntry> = project.terminals.values().collect();
    terminals.sort_by(|a, b| {
        (tag(a.device), &a.designation, &a.point, a.id).cmp(&(tag(b.device), &b.designation, &b.point, b.id))
    });
    files.push(("terminals.json".to_owned(), render_json(Path::new("terminals.json"), &terminals)?));

    let mut devices: Vec<DeviceFile> = project.devices.values().map(|d| DeviceFile::of(project, d)).collect();
    devices.sort_by(|a, b| a.tag.cmp(&b.tag));
    files.push(("devices.json".to_owned(), render_json(Path::new("devices.json"), &devices)?));

    for s in project.symbols.values() {
        let name = format!("symbols/{}", symbol_file_name(&s.id.0)?);
        let bytes = render_json(Path::new(&name), s)?;
        files.push((name, bytes));
    }
    // Every wire with its ends; drawn ones' ends from their drawings.
    let c = project.connectivity();
    let mut wires: Vec<WireFile> = project
        .connections
        .values()
        .map(|w| WireFile {
            id: w.id,
            from: c.endpoint(&w.from),
            to: c.endpoint(&w.to),
            wire_number: w.wire_number.clone(),
            spec: w.spec.clone(),
        })
        .chain(project.undrawn.values().map(|w| WireFile {
            id: w.id,
            from: Some(w.from),
            to: Some(w.to),
            wire_number: w.wire_number.clone(),
            spec: w.spec.clone(),
        }))
        .collect();
    if wires.iter().any(|w| w.from.is_none() || w.to.is_none()) {
        return Err(model(Path::new("wires.json"))(ModelError::BadWireEnds));
    }
    wires.sort_by_key(|w| w.id);
    files.push(("wires.json".to_owned(), render_json(Path::new("wires.json"), &wires)?));

    for page in project.pages.keys() {
        let file = PageFile {
            functions: project.functions_on(*page).cloned().collect(),
            connections: Vec::new(),
            wires: project
                .connections_on(*page)
                .map(|w| WireGraphics { wire: w.id, from: w.from.clone(), to: w.to.clone(), bend: w.bend })
                .collect(),
            boxes: project.boxes_on(*page).cloned().collect(),
            panel: project.panels.get(page).copied(),
            panel_items: project.panel_items_on(*page).into_iter().cloned().collect(),
        };
        let name = format!("pages/{}.json", page.0);
        let bytes = render_json(Path::new(&name), &file)?;
        files.push((name, bytes));
    }
    for (page, u) in &project.underlays {
        let name = format!("underlays/{}.json", page.0);
        let mut bytes = serde_json::to_vec(&UnderlayFile::of(u))
            .map_err(|source| ProjectError::Json { path: PathBuf::from(&name), source })?;
        bytes.push(b'\n');
        files.push((name, bytes));
    }
    Ok(files)
}

/// Write the project into `dir` (created if needed). Files for removed pages or symbols are
/// deleted; nothing else in the folder is touched.
///
/// The save is all or nothing: if it stops part way (a full disk, a crash), the folder loads
/// as the project before the save, or once the save's journal is written, as the project after
/// it. See [`store`].
pub fn save(project: &Project, dir: &Path) -> Result<()> {
    save_checked(project, dir, None).map(|_| ())
}

/// Save, but only if the folder is still as it was when it had `expected` (a
/// [`fingerprint`]): another program's save in between is refused with
/// [`ProjectError::Changed`], and the folder is left alone. Returns the new fingerprint.
pub fn save_checked(project: &Project, dir: &Path, expected: Option<Fingerprint>) -> Result<Fingerprint> {
    let files = render(project)?;
    for d in [dir.to_owned(), dir.join("pages"), dir.join("symbols"), dir.join("underlays")] {
        std::fs::create_dir_all(&d).map_err(io(&d))?;
    }
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    if let Some(expected) = expected
        && fingerprint(dir)? != expected
    {
        return Err(ProjectError::Changed(dir.to_owned()));
    }
    store::commit(dir, &files)?;
    fingerprint(dir)
}

/// What a project folder holds now, to tell whether someone else saved over it: a hash of
/// every project file's name and contents. An empty or missing folder has one too.
pub type Fingerprint = u64;

/// The folder's [`Fingerprint`].
pub fn fingerprint(dir: &Path) -> Result<Fingerprint> {
    use std::hash::{Hash, Hasher};
    let mut names = store::project_files(dir)?;
    names.sort();
    // The same in every run of this program (fixed keys), which is all a comparison needs.
    let mut h = std::hash::DefaultHasher::new();
    for name in names {
        let path = dir.join(&name);
        name.hash(&mut h);
        // In pieces, so a large file (an underlay) is never held whole just to be hashed.
        store::each_chunk(&path, |chunk| chunk.hash(&mut h))?;
    }
    Ok(h.finish())
}

/// Load, and the folder's fingerprint as loaded, for a later [`save_checked`]. If another
/// program saves while this loads, it loads again, so the fingerprint is that of what loaded.
pub fn load_checked(dir: &Path) -> Result<(Project, Fingerprint)> {
    let mut before = fingerprint(dir)?;
    for _ in 0..3 {
        let project = load(dir)?;
        let after = fingerprint(dir)?;
        if after == before {
            return Ok((project, after));
        }
        before = after;
    }
    Err(ProjectError::Busy(dir.to_owned()))
}

/// The format of the project in `dir`, as its `project.json` says. A program saving a project
/// whose format is older than [`FORMAT`] should say first that older versions won't open it.
pub fn format_of(dir: &Path) -> Result<u32> {
    #[derive(Deserialize)]
    struct Head {
        format: u32,
    }
    Ok(read_json::<Head>(&dir.join("project.json"))?.format)
}

/// What to tell a person before a save upgrades the project in `dir` to this format: `Some` when
/// the folder holds a project of an older format (it then no longer opens in older versions).
pub fn upgrade_note(dir: &Path) -> Option<String> {
    let found = format_of(dir).ok()?;
    (found < FORMAT).then(|| {
        format!(
            "this saves the project in format {FORMAT}; versions of Electric Studio that read only format {found} can't open it any more"
        )
    })
}

/// An ambiguous format 1 mapping found while converting (section 9, D33): the nodes are kept
/// apart, nothing is merged or reconnected, and a person decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversionFinding {
    pub kind: FindingKind,
    /// The format 1 device's tag.
    pub name: String,
    /// The pages its drawings are on.
    pub pages: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// Drawn both as a potential point and as an interruption point: now a potential and an
    /// interruption of that name.
    PotentialAndInterruption,
    /// An interruption whose drawings joined differently named potentials: each drawing is now
    /// its own interruption, so none of those potentials is joined.
    InterruptionJoinsPotentials,
    /// A junction device drawn more than once: each drawing is its own junction.
    JunctionDrawnTwice,
}

impl std::fmt::Display for ConversionFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.kind {
            FindingKind::PotentialAndInterruption => {
                "is drawn as a potential and as an interruption; kept as two nodes"
            }
            FindingKind::InterruptionJoinsPotentials => {
                "joins differently named potentials; each of its drawings is now its own interruption"
            }
            FindingKind::JunctionDrawnTwice => "is a junction drawn more than once; each drawing is its own junction",
        };
        write!(f, "{} {what} (pages {})", self.name, self.pages.join(", "))
    }
}

/// The ambiguous mappings converting the project in `dir` finds: none for this format.
pub fn conversion_findings(dir: &Path) -> Result<Vec<ConversionFinding>> {
    Ok(load_with_findings(dir)?.1)
}

/// Read a project folder written by [`save`], in this format or format 1.
pub fn load(dir: &Path) -> Result<Project> {
    Ok(load_with_findings(dir)?.0)
}

/// [`load`], with what converting a format 1 project found ambiguous.
pub fn load_with_findings(dir: &Path) -> Result<(Project, Vec<ConversionFinding>)> {
    // A save that stopped part way is finished (if it got as far as its journal) or undone.
    if dir.join(store::JOURNAL).exists() || store::has_staged(dir)? {
        let _lock = store::Lock::take(dir)?;
        store::recover(dir)?;
    }
    let path = dir.join("project.json");
    let head: ProjectFile = read_json(&path)?;
    if head.format > FORMAT {
        return Err(ProjectError::Format { path, found: head.format });
    }
    let mut project = Project::new(&head.name);
    project.info = head.info;
    // Before the wires load, so per-potential numbering allows shared numbers.
    project.naming = head.naming;
    // Through the command, so a hand-edited file can't bring in settings the editor would refuse.
    project.apply(Command::SetWireLengthSettings(head.wire_lengths)).map_err(model(&path))?;
    let legacy = head.format < 2;
    let mut pages = head.pages;
    if legacy {
        // Format 1's dense order becomes sparse keys, in the same order.
        pages.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        for (i, page) in pages.iter_mut().enumerate() {
            page.order = PAGE_STEP * (i as u64 + 1);
        }
    }
    for page in pages {
        project.apply(Command::AddPage(page)).map_err(model(&path))?;
    }
    let sheet = head.sheet;

    let symbols_dir = dir.join("symbols");
    let mut symbol_paths: Vec<PathBuf> = std::fs::read_dir(&symbols_dir)
        .map_err(io(&symbols_dir))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    symbol_paths.sort();
    for p in symbol_paths {
        project.apply(Command::AddSymbol(read_json::<SymbolDef>(&p)?)).map_err(model(&p))?;
    }

    // The catalogue comes before the devices that use it; projects saved before M3a have none.
    let path = dir.join(if legacy { "parts.json" } else { "products.json" });
    if path.exists() && legacy {
        for part in read_json::<Vec<PartV1>>(&path)? {
            let mut fields = part.rest;
            fields.insert("id".into(), serde_json::to_value(ProductId::for_number(&part.number)).unwrap_or_default());
            fields.insert("number".into(), part.number.into());
            let part: Part = serde_json::from_value(fields.into())
                .map_err(|source| ProjectError::Json { path: path.clone(), source })?;
            project.apply(Command::AddPart(part)).map_err(model(&path))?;
        }
    } else if !legacy {
        for part in read_json::<Vec<Part>>(&path)? {
            project.apply(Command::AddPart(part)).map_err(model(&path))?;
        }
    }

    // Cables before the wires that run in them; projects saved before G4 have none.
    let path = dir.join("cables.json");
    if path.exists() {
        for c in read_json::<Vec<Cable>>(&path)? {
            project.apply(Command::AddCable(c)).map_err(model(&path))?;
        }
    }

    let path = dir.join("devices.json");
    let records = read_json::<Vec<DeviceFile>>(&path)?;
    let mut device_ids = std::collections::BTreeSet::new();
    for d in &records {
        if !device_ids.insert(d.id) {
            return Err(model(&path)(ModelError::DuplicateId(d.id.0)));
        }
        if !legacy && d.parts.iter().any(|r| r.product.is_none()) {
            return Err(ProjectError::Canonical {
                path: path.clone(),
                reason: "product reference has no canonical id".into(),
            });
        }
    }
    let devices: Vec<Device> = records.into_iter().map(DeviceFile::into_device).collect();
    let mut page_files: Vec<(ecad_model::PageId, PathBuf, PageFile)> = Vec::new();
    for id in project.pages.keys().copied().collect::<Vec<_>>() {
        let path = dir.join("pages").join(format!("{}.json", id.0));
        let file: PageFile = read_json(&path)?;
        page_files.push((id, path, file));
    }
    let (node_only, mut findings) =
        if legacy { legacy_nodes(&mut project, &devices, &mut page_files)? } else { Default::default() };
    for d in devices {
        if !node_only.contains(&d.id) {
            project.apply(Command::AddDevice(d)).map_err(model(&path))?;
        }
    }
    let path = dir.join("nodes.json");
    if !legacy {
        for n in read_json::<Vec<NodeEntry>>(&path)? {
            project.apply(Command::AddNode(n)).map_err(model(&path))?;
        }
    }

    // Terminals before the drawings that show them (format 2); format 1 has none: placing a
    // drawing gives it terminals from its designations.
    let path = dir.join("terminals.json");
    if !legacy {
        for t in read_json::<Vec<TerminalEntry>>(&path)? {
            project.apply(Command::AddTerminal(t)).map_err(model(&path))?;
        }
        // Internal bridges, canonical (R7): before the drawings that depict them.
        let path = dir.join("bridges.json");
        for b in read_json::<Vec<ecad_model::connectivity::Bridge>>(&path)? {
            if b.a >= b.b {
                return Err(ProjectError::Canonical { path: path.clone(), reason: "bridge ends out of order".into() });
            }
            project.apply(Command::AddBridge(b)).map_err(model(&path))?;
        }
    }

    // Place every page's functions before any connection, then connect.
    let mut pages = Vec::new();
    for (id, path, file) in page_files {
        for f in &file.functions {
            if !legacy {
                let sym = project.symbol(&f.symbol).map_err(model(&path))?;
                let valid = f.designations.is_empty()
                    && if project.node_kind_of(&f.symbol).is_some() {
                        f.terminals.is_empty()
                    } else {
                        f.terminals.len() == sym.connection_points.len()
                            && sym
                                .connection_points
                                .iter()
                                .all(|cp| f.terminals.get(&cp.name).is_some_and(|t| project.terminals.contains_key(t)))
                    };
                if !valid
                    || (matches!(sym.role, ecad_model::Role::PlcChannel { .. })
                        && [ecad_model::ADDRESS, ecad_model::SYMBOL, ecad_model::COMMENT]
                            .iter()
                            .any(|k| f.properties.contains_key(*k)))
                {
                    return Err(model(&path)(ModelError::InvalidTerminalBindings(f.id)));
                }
            }
            if f.page != id {
                return Err(ProjectError::Canonical {
                    path: path.clone(),
                    reason: "drawing belongs to another page".into(),
                });
            }
            project.apply(Command::Place(f.clone())).map_err(model(&path))?;
        }
        for b in &file.boxes {
            project.apply(Command::AddBox(b.clone())).map_err(model(&path))?;
        }
        if let Some(panel) = file.panel {
            project.apply(Command::SetPanel { page: id, panel: Some(panel) }).map_err(model(&path))?;
        }
        for item in &file.panel_items {
            project.apply(Command::AddPanelItem(item.clone())).map_err(model(&path))?;
        }
        // A drawing behind the page; projects saved before M6c have none.
        let underlay = dir.join("underlays").join(format!("{}.json", id.0));
        if underlay.exists() {
            // Read with a limit, so a huge file is refused before it is held or decoded.
            let bytes = read_bounded(&underlay, MAX_UNDERLAY_FILE)?;
            let file: UnderlayFile = serde_json::from_slice(&bytes)
                .map_err(|source| ProjectError::Json { path: underlay.clone(), source })?;
            drop(bytes);
            let points = file.lines.iter().map(Vec::len).sum::<usize>() + 3 * file.fills.len();
            if points > ecad_model::underlay::MAX_POINTS {
                return Err(model(&underlay)(ModelError::InvalidUnderlay));
            }
            let u = file.into_underlay();
            project.apply(Command::SetUnderlay { page: id, underlay: Some(Box::new(u)) }).map_err(model(&underlay))?;
        }
        if (!legacy && !file.connections.is_empty()) || (legacy && !file.wires.is_empty()) {
            return Err(ProjectError::Canonical {
                path,
                reason: "mixed legacy connections and canonical wire graphics".into(),
            });
        }
        pages.push((id, path, file.connections, file.wires));
    }
    // Format 2: the wires, then how each drawn one is drawn.
    let wires_path = dir.join("wires.json");
    let mut wires = std::collections::BTreeMap::new();
    if !legacy {
        for w in read_json::<Vec<WireFile>>(&wires_path)? {
            let id = w.id;
            if w.from.is_none() || w.to.is_none() {
                return Err(model(&wires_path)(ModelError::BadWireEnds));
            }
            if wires.insert(id, w).is_some() {
                return Err(model(&wires_path)(ModelError::DuplicateId(id.0)));
            }
        }
    }
    let mut drawn = Vec::new();
    for (page_id, path, connections, graphics) in pages {
        for c in connections {
            project.apply(Command::Connect(c)).map_err(model(&path))?;
        }
        for g in graphics {
            if project.function(g.from.function).map_err(model(&path))?.page != page_id
                || project.function(g.to.function).map_err(model(&path))?.page != page_id
            {
                return Err(ProjectError::Canonical {
                    path: path.clone(),
                    reason: "wire graphics belong to another page".into(),
                });
            }
            let w = wires.remove(&g.wire).ok_or_else(|| model(&path)(ModelError::NoConnection(g.wire)))?;
            let c = Connection {
                id: g.wire,
                from: g.from,
                to: g.to,
                wire_number: w.wire_number.clone(),
                spec: w.spec.clone(),
                bend: g.bend,
            };
            project.apply(Command::Connect(c)).map_err(model(&path))?;
            drawn.push(w);
        }
    }
    // A drawn wire's graphics must show its ends: they are the wire's identity, the drawing
    // only shows it.
    if !drawn.is_empty() {
        let c = project.connectivity();
        for w in &drawn {
            let Ok(conn) = project.connection(w.id) else { continue };
            let shown = (c.endpoint(&conn.from), c.endpoint(&conn.to));
            let saved = (w.from, w.to);
            let swapped = (w.to, w.from);
            if shown != saved && shown != swapped {
                return Err(model(&wires_path)(ModelError::WireDrawnElsewhere(w.id.0)));
            }
        }
    }
    for (_, w) in wires {
        let (Some(from), Some(to)) = (w.from, w.to) else {
            return Err(model(&wires_path)(ModelError::BadWireEnds));
        };
        let wire = ecad_model::Wire { id: w.id, from, to, wire_number: w.wire_number, spec: w.spec };
        project.apply(Command::AddWire(wire)).map_err(model(&wires_path))?;
    }
    if legacy {
        findings.extend(split_joining_interruptions(&mut project).map_err(model(dir))?);
    }
    project.sheet = match sheet {
        Some(s) => s,
        None => legacy_sheet(&project),
    };
    Ok((project, findings))
}

type Pages = [(ecad_model::PageId, PathBuf, PageFile)];

fn page_names(project: &Project, pages: impl IntoIterator<Item = ecad_model::PageId>) -> Vec<String> {
    let mut names: Vec<String> =
        pages.into_iter().filter_map(|p| project.page(p).ok()).map(|p| p.name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// Format 1 drew potentials, interruptions and junctions as devices. Each becomes a node (D1):
/// a potential or interruption device the node [`NodeId::for_device`] gives it, named after its
/// tag; each junction drawing its own node ([`NodeId::for_junction`]), the ids M7 derived. The
/// drawings are rewritten to show their node. A device drawn both ways becomes a potential and
/// an interruption ([`NodeId::interruption_of`]), and a junction drawn twice two junctions: both
/// reported, never merged. Returns the devices that only stood for nodes.
fn legacy_nodes(
    project: &mut Project,
    devices: &[Device],
    pages: &mut Pages,
) -> Result<(std::collections::BTreeSet<DeviceId>, Vec<ConversionFinding>)> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut kinds: BTreeMap<DeviceId, BTreeSet<NodeKind>> = BTreeMap::new();
    let mut on_pages: BTreeMap<DeviceId, BTreeSet<ecad_model::PageId>> = BTreeMap::new();
    let mut as_device: BTreeSet<DeviceId> = BTreeSet::new();
    let mut junctions: BTreeMap<DeviceId, usize> = BTreeMap::new();
    for (page, _, file) in pages.iter() {
        for f in &file.functions {
            match project.node_kind_of(&f.symbol) {
                Some(kind) => {
                    kinds.entry(f.device).or_default().insert(kind);
                    on_pages.entry(f.device).or_default().insert(*page);
                    if kind == NodeKind::Junction {
                        *junctions.entry(f.device).or_default() += 1;
                    }
                }
                None => {
                    as_device.insert(f.device);
                }
            }
        }
    }
    let by_id: BTreeMap<DeviceId, &Device> = devices.iter().map(|d| (d.id, d)).collect();
    for (_, path, file) in pages.iter_mut() {
        for f in &mut file.functions {
            let Some(kind) = project.node_kind_of(&f.symbol) else { continue };
            let device = f.device;
            let (tag, description) =
                by_id.get(&device).map_or((String::new(), String::new()), |d| (d.tag.clone(), d.description.clone()));
            let mixed = kinds
                .get(&device)
                .is_some_and(|k| k.contains(&NodeKind::Potential) && k.contains(&NodeKind::Interruption));
            let id = match kind {
                NodeKind::Junction => NodeId::for_junction(f.id),
                NodeKind::Interruption if mixed => NodeId::interruption_of(device),
                _ => NodeId::for_device(device),
            };
            if !project.nodes.contains_key(&id) {
                // Tags become display names, junctions' included (section 9).
                let node = NodeEntry { description, ..NodeEntry::new(id, kind, &tag) };
                project.apply(Command::AddNode(node)).map_err(model(path))?;
            }
            f.device = DeviceId::NONE;
            f.node = Some(id);
        }
    }
    let mut findings = Vec::new();
    for (device, k) in &kinds {
        let tag = by_id.get(device).map_or_else(String::new, |d| d.tag.clone());
        let pages = page_names(project, on_pages.get(device).into_iter().flatten().copied());
        if k.contains(&NodeKind::Potential) && k.contains(&NodeKind::Interruption) {
            findings.push(ConversionFinding {
                kind: FindingKind::PotentialAndInterruption,
                name: tag.clone(),
                pages: pages.clone(),
            });
        }
        if junctions.get(device).is_some_and(|n| *n > 1) {
            findings.push(ConversionFinding { kind: FindingKind::JunctionDrawnTwice, name: tag, pages });
        }
    }
    let node_only = kinds.keys().filter(|d| !as_device.contains(d)).copied().collect();
    Ok((node_only, findings))
}

/// An interruption whose drawings, taken one by one, reach more than one potential name joined
/// those potentials in format 1. Converted, each of its drawings becomes its own interruption
/// ([`NodeId::split`]), so nothing is joined that a label alone joined; reported.
fn split_joining_interruptions(project: &mut Project) -> std::result::Result<Vec<ConversionFinding>, ModelError> {
    let mut findings = Vec::new();
    let interruptions: Vec<NodeEntry> =
        project.nodes.values().filter(|n| n.kind == NodeKind::Interruption).cloned().collect();
    for node in interruptions {
        let drawings: Vec<ecad_model::Function> = project.drawings_of_node(node.id).cloned().collect();
        if drawings.len() < 2 {
            continue;
        }
        // Each drawing on its own node, to see which potentials it reaches by itself.
        let mut undo = Vec::new();
        for f in &drawings {
            let split = NodeEntry { id: node.id.split(f.id), ..node.clone() };
            undo.push(project.apply(Command::AddNode(split.clone()))?);
            undo.push(project.apply(Command::Rebind { id: f.id, node: split.id })?);
        }
        let netlist = project.netlist();
        let names: std::collections::BTreeSet<&String> = netlist
            .nets
            .iter()
            .filter(|n| {
                n.endpoints
                    .iter()
                    .any(|e| drawings.iter().any(|f| *e == ecad_model::Endpoint::Node(node.id.split(f.id))))
            })
            .flat_map(|n| n.names.iter())
            .collect();
        if names.len() > 1 {
            project.apply(Command::RemoveNode(node.id))?;
            let pages = page_names(project, drawings.iter().map(|f| f.page));
            findings.push(ConversionFinding {
                kind: FindingKind::InterruptionJoinsPotentials,
                name: node.name.clone(),
                pages,
            });
        } else {
            for cmd in undo.into_iter().rev() {
                project.apply(cmd)?;
            }
        }
    }
    Ok(findings)
}

/// A sheet for a project saved before sheets existed: placed around everything drawn, so no
/// page content ends up outside the frame.
fn legacy_sheet(project: &Project) -> Sheet {
    let boxes: Vec<_> = project.pages.keys().filter_map(|p| project.content_bounds(*p)).collect();
    let Some(&(mut lo, mut hi)) = boxes.first() else { return Sheet::default() };
    for (a, b) in &boxes[1..] {
        lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Point::new(hi.x.max(b.x), hi.y.max(b.y));
    }
    Sheet::around(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_makers_products_with_one_number_save_and_load_by_id() {
        let mut p = ecad_model::sample::start_circuit().unwrap();
        let other = Part {
            id: ProductId::new(),
            manufacturer: "Other".into(),
            ..p.part_by_number("EX-CT-24DC").unwrap().clone()
        };
        p.apply(Command::AddPart(other.clone())).unwrap();
        let k1 = p.device_by_tag("-K1").unwrap().id;
        let mut parts = p.device(k1).unwrap().parts.clone();
        parts.push(PartRef::to(other.id, 2));
        p.apply(Command::SetDeviceParts { id: k1, parts }).unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&p, dir.path()).unwrap();
        assert!(!dir.path().join("parts.json").exists());
        let devices = std::fs::read_to_string(dir.path().join("devices.json")).unwrap();
        assert!(devices.contains(&format!("\"product\": \"{}\"", other.id.0)), "by id");
        assert!(devices.contains("\"number\": \"EX-CT-24DC\""), "the number beside it");
        let q = load(dir.path()).unwrap();
        assert_eq!(q, p);
        assert_eq!(q.parts_numbered("EX-CT-24DC").len(), 2);
        assert_eq!(format_of(dir.path()).unwrap(), FORMAT);
    }

    #[test]
    fn a_format_1_project_converts_on_load_and_is_saved_as_format_2() {
        let p = ecad_model::sample::with_plc().unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&p, dir.path()).unwrap();
        // Write it back as format 1 would have: parts.json without ids, devices by number,
        // dense page order.
        let root = dir.path();
        let products: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("products.json")).unwrap()).unwrap();
        let parts: Vec<serde_json::Value> = products
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let mut v = v.clone();
                v.as_object_mut().unwrap().remove("id");
                v
            })
            .collect();
        std::fs::write(root.join("parts.json"), serde_json::to_string_pretty(&parts).unwrap()).unwrap();
        std::fs::remove_file(root.join("products.json")).unwrap();
        let mut devices: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("devices.json")).unwrap()).unwrap();
        for d in devices.as_array_mut().unwrap() {
            for r in d.get_mut("parts").and_then(|v| v.as_array_mut()).into_iter().flatten() {
                r.as_object_mut().unwrap().remove("product");
            }
        }
        // Nodes back as the devices format 1 drew them with: a device per potential or
        // interruption, one per junction drawing; no nodes file.
        let nodes: Vec<NodeEntry> =
            serde_json::from_str(&std::fs::read_to_string(root.join("nodes.json")).unwrap()).unwrap();
        std::fs::remove_file(root.join("nodes.json")).unwrap();
        let stand_in = |n: &NodeEntry| DeviceId::from_u128(n.id.0.as_u128() ^ 1);
        for n in &nodes {
            devices
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "id": stand_in(n), "tag": n.name, "description": n.description }));
        }
        std::fs::write(root.join("devices.json"), serde_json::to_string_pretty(&devices).unwrap()).unwrap();
        // Wires back in the page files with their numbers and make, no wires file.
        let wires: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(root.join("wires.json")).unwrap()).unwrap();
        std::fs::remove_file(root.join("wires.json")).unwrap();
        // Designations back on the drawings, no terminals file.
        let terminals: Vec<TerminalEntry> =
            serde_json::from_str(&std::fs::read_to_string(root.join("terminals.json")).unwrap()).unwrap();
        std::fs::remove_file(root.join("terminals.json")).unwrap();
        for entry in std::fs::read_dir(root.join("pages")).unwrap() {
            let path = entry.unwrap().path();
            let mut page: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let graphics = page.as_object_mut().unwrap().remove("wires").unwrap_or_default();
            let connections: Vec<serde_json::Value> = graphics
                .as_array()
                .into_iter()
                .flatten()
                .map(|g| {
                    let w = wires.iter().find(|w| w["id"] == g["wire"]).unwrap();
                    let mut c = w.as_object().unwrap().clone();
                    c.remove("from");
                    c.remove("to");
                    c.insert("from".into(), g["from"].clone());
                    c.insert("to".into(), g["to"].clone());
                    if let Some(b) = g.get("bend") {
                        c.insert("bend".into(), b.clone());
                    }
                    serde_json::Value::Object(c)
                })
                .collect();
            page["connections"] = connections.into();
            for f in page["functions"].as_array_mut().unwrap() {
                if let Some(node) = f.as_object_mut().unwrap().remove("node") {
                    let n = nodes.iter().find(|n| serde_json::to_value(n.id).unwrap() == node).unwrap();
                    f["device"] = serde_json::to_value(stand_in(n)).unwrap();
                }
                let Some(map) = f.as_object_mut().unwrap().remove("terminals") else { continue };
                let mut designations = serde_json::Map::new();
                for (point, id) in map.as_object().unwrap() {
                    let t = terminals.iter().find(|t| serde_json::to_value(t.id).unwrap() == *id).unwrap();
                    if t.designation != *point {
                        designations.insert(point.clone(), t.designation.clone().into());
                    }
                }
                if !designations.is_empty() {
                    f["designations"] = designations.into();
                }
                // A channel's typed data back as the drawing's properties.
                for (_, id) in map.as_object().unwrap() {
                    let t = terminals.iter().find(|t| serde_json::to_value(t.id).unwrap() == *id).unwrap();
                    if let Some(c) = &t.channel {
                        for (k, v) in [("address", &c.address), ("symbol", &c.symbol), ("comment", &c.comment)] {
                            if !v.is_empty() {
                                f["properties"][k] = v.clone().into();
                            }
                        }
                    }
                }
            }
            std::fs::write(&path, serde_json::to_string_pretty(&page).unwrap()).unwrap();
        }
        let mut head: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("project.json")).unwrap()).unwrap();
        head["format"] = 1.into();
        let n = head["pages"].as_array().unwrap().len();
        for (i, page) in head["pages"].as_array_mut().unwrap().iter_mut().enumerate() {
            page["order"] = (n - 1 - i).into();
        }
        std::fs::write(root.join("project.json"), serde_json::to_string_pretty(&head).unwrap()).unwrap();
        assert_eq!(format_of(root).unwrap(), 1);
        assert!(upgrade_note(root).is_some_and(|n| n.contains("format 2")));

        let q = load(root).unwrap();
        // Same products (ids from their numbers), same device parts, pages in format 1's order
        // with sparse keys.
        assert_eq!(q.parts, p.parts);
        assert_eq!(q.devices, p.devices);
        assert_eq!(q.terminals, p.terminals, "terminals from the drawings' designations, same ids");
        // Node drawings show nodes again, now with the ids of their stand-in devices; every
        // other drawing is as it was, and the potentials are the same.
        let devices_only = |p: &Project| -> Vec<ecad_model::Function> {
            p.functions.values().filter(|f| f.node.is_none()).cloned().collect()
        };
        assert_eq!(devices_only(&q), devices_only(&p));
        assert_eq!(q.nodes.len(), p.nodes.len());
        let names = |p: &Project| {
            let mut n: Vec<Vec<String>> = p.potentials().into_iter().map(|p| p.names).collect();
            n.sort();
            n
        };
        assert_eq!(names(&q), names(&p));
        let names: Vec<_> = q.pages_in_order().iter().map(|pg| pg.name.clone()).collect();
        let mut want: Vec<_> = p.pages_in_order().iter().map(|pg| pg.name.clone()).collect();
        want.reverse();
        assert_eq!(names, want);
        assert!(q.pages.values().all(|pg| pg.order % PAGE_STEP == 0 && pg.order > 0));
        // Converting twice gives the same project.
        assert_eq!(load(root).unwrap(), q);
        save(&q, root).unwrap();
        assert_eq!(format_of(root).unwrap(), 2);
        assert_eq!(upgrade_note(root), None);
        assert!(!root.join("parts.json").exists(), "format 1's catalogue file is replaced");
        assert_eq!(load(root).unwrap(), q);
    }

    /// Two revisions of one project: `a`, then `b` with a page, its devices and wires gone
    /// and a device added, so a mix of their files would not load as either.
    fn two_revisions() -> (Project, Project) {
        let a = ecad_model::sample::with_plc().unwrap();
        let mut b = a.clone();
        let last = b.pages_in_order().last().unwrap().id;
        let on_last: Vec<_> = b.functions_on(last).map(|f| (f.id, f.device)).collect();
        let mut cmds: Vec<Command> = on_last.iter().map(|(f, _)| Command::Unplace(*f)).collect();
        cmds.push(Command::RemovePage(last));
        b.apply(Command::Batch(cmds)).unwrap();
        let orphans: Vec<_> = on_last.iter().map(|(_, d)| *d).filter(|d| b.functions_of(*d).next().is_none()).collect();
        for d in orphans {
            if b.devices.contains_key(&d) {
                b.apply(Command::RemoveDevice(d)).unwrap();
            }
        }
        b.apply(Command::AddDevice(ecad_model::Device::new(ecad_model::DeviceId::new(), "-K77", "added"))).unwrap();
        // And an underlay behind the first page: its file is part of the same save.
        let first = b.pages_in_order()[0].id;
        let shapes = ecad_model::underlay::UnderlayShapes {
            lines: vec![vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)]],
            fills: Vec::new(),
            width: 10.0,
            height: 10.0,
        };
        let u = ecad_model::underlay::Underlay {
            source: "old.dwg".into(),
            shapes: std::sync::Arc::new(shapes),
            at: Point::new(0.0, 0.0),
            scale: 1.0,
            visible: true,
            units: Default::default(),
            units_from: Default::default(),
        };
        b.apply(Command::SetUnderlay { page: first, underlay: Some(Box::new(u)) }).unwrap();
        (a, b)
    }

    #[test]
    fn a_save_stopped_at_any_step_loads_as_the_old_or_the_new_project() {
        let (a, b) = two_revisions();
        let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        save(&a, a_dir.path()).unwrap();
        save(&b, b_dir.path()).unwrap();
        let (old, new) = (load(a_dir.path()).unwrap(), load(b_dir.path()).unwrap());
        assert_ne!(old, new);
        let (mut saw_old, mut saw_new) = (false, false);
        for n in 0.. {
            let dir = tempfile::tempdir().unwrap();
            save(&a, dir.path()).unwrap();
            store::FAIL_AT.with(|f| f.set(Some(n)));
            let result = save(&b, dir.path());
            store::FAIL_AT.with(|f| f.set(None));
            if result.is_ok() {
                assert_eq!(load(dir.path()).unwrap(), new);
                assert!(n > 5, "the save took {n} steps");
                break;
            }
            // Loading finishes or undoes the save: one whole revision, never a mix.
            let back = load(dir.path()).unwrap_or_else(|e| panic!("step {n}: {e}"));
            if back == old {
                saw_old = true;
            } else {
                assert_eq!(back, new, "step {n}");
                saw_new = true;
            }
            assert!(!dir.path().join(store::JOURNAL).exists() && !store::has_staged(dir.path()).unwrap());
            // And the folder takes the next save.
            save(&b, dir.path()).unwrap();
            assert_eq!(load(dir.path()).unwrap(), new);
        }
        assert!(saw_old && saw_new, "failures before and after the journal were both tried");
    }

    #[test]
    fn a_failure_while_recovering_is_recovered_next_time() {
        let (a, b) = two_revisions();
        let dir = tempfile::tempdir().unwrap();
        save(&a, dir.path()).unwrap();
        // Stop just after the journal: the files are staged and the save has happened.
        let staged = store::project_files(dir.path()).unwrap().len();
        let mut n = 0;
        loop {
            store::FAIL_AT.with(|f| f.set(Some(n)));
            let r = save(&b, dir.path());
            store::FAIL_AT.with(|f| f.set(None));
            assert!(r.is_err(), "no step after the journal was found");
            if dir.path().join(store::JOURNAL).exists() {
                break;
            }
            n += 1;
            assert!(n < 100 + staged);
        }
        // Recovery itself fails at its first step, then succeeds.
        store::FAIL_AT.with(|f| f.set(Some(0)));
        assert!(load(dir.path()).is_err());
        store::FAIL_AT.with(|f| f.set(None));
        assert_eq!(load(dir.path()).unwrap().device_by_tag("-K77").unwrap().description, "added");
    }

    #[test]
    fn a_save_from_a_stale_copy_is_refused_and_a_clean_save_changes_nothing() {
        let (a, b) = two_revisions();
        let dir = tempfile::tempdir().unwrap();
        save(&a, dir.path()).unwrap();
        let (mine, seen) = load_checked(dir.path()).unwrap();
        let (_, theirs_seen) = load_checked(dir.path()).unwrap();
        // Someone else saves first.
        let after_theirs = save_checked(&b, dir.path(), Some(theirs_seen)).unwrap();
        let e = save_checked(&mine, dir.path(), Some(seen)).unwrap_err();
        assert!(matches!(e, ProjectError::Changed(_)), "{e}");
        assert_eq!(fingerprint(dir.path()).unwrap(), after_theirs, "the folder wasn't touched");
        assert_eq!(load(dir.path()).unwrap().device_by_tag("-K77").map(|d| d.tag.clone()), Some("-K77".into()));
        // Saving what is already there writes nothing.
        let before = std::fs::metadata(dir.path().join("devices.json")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again = save_checked(&load(dir.path()).unwrap(), dir.path(), Some(after_theirs)).unwrap();
        assert_eq!(again, after_theirs);
        assert_eq!(std::fs::metadata(dir.path().join("devices.json")).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn one_saver_at_a_time_and_a_crashed_savers_lock_is_taken_over() {
        let p = ecad_model::sample::start_circuit().unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&p, dir.path()).unwrap();
        let held = store::Lock::take(dir.path()).unwrap();
        assert!(matches!(save(&p, dir.path()), Err(ProjectError::Busy(_))));
        drop(held);
        save(&p, dir.path()).unwrap();
        // A lock file from long ago (a crash) doesn't block.
        let lock = std::fs::File::create(dir.path().join(store::LOCK)).unwrap();
        let old = std::time::SystemTime::now() - store::STALE_LOCK - std::time::Duration::from_secs(5);
        lock.set_modified(old).unwrap();
        drop(lock);
        save(&p, dir.path()).unwrap();
        assert!(!dir.path().join(store::LOCK).exists());
    }

    use ecad_model::sample::start_circuit;

    #[test]
    fn save_then_load_gives_the_same_project() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        save(&original, dir.path()).unwrap();
        assert_eq!(load(dir.path()).unwrap(), original);
        // Everything the samples use survives too: parts, terminals, PLC properties, the
        // sheet's rows, the title block and page subtitles.
        let plc = ecad_model::sample::with_plc().unwrap();
        let other = tempfile::tempdir().unwrap();
        save(&plc, other.path()).unwrap();
        assert_eq!(load(other.path()).unwrap(), plc);
        // The reference project: contact blocks, bridges, mirrored drawings, the layout page.
        let mut reference = ecad_model::reference::reference_project().unwrap();
        reference.wire_length_settings = ecad_model::WireLengthSettings { spare_per_end: 200.0, round_to: 100.0 };
        let third = tempfile::tempdir().unwrap();
        save(&reference, third.path()).unwrap();
        assert_eq!(load(third.path()).unwrap(), reference);
        // Saving again writes identical bytes (stable order, no timestamps).
        let first = std::fs::read(dir.path().join("devices.json")).unwrap();
        save(&load(dir.path()).unwrap(), dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("devices.json")).unwrap(), first);
    }

    /// The content hash and the derived connectivity (terminal and node ids, the netlist) come
    /// back the same after a save and load.
    #[test]
    fn content_hash_and_netlist_survive_a_save_and_load() {
        use ecad_model::sample;
        for p in [
            sample::start_circuit(),
            sample::with_potentials(),
            sample::with_terminals(),
            sample::with_plc(),
            sample::with_power(),
            ecad_model::reference::reference_project(),
        ] {
            let p = p.unwrap();
            let dir = tempfile::tempdir().unwrap();
            save(&p, dir.path()).unwrap();
            let back = load(dir.path()).unwrap();
            assert_eq!(back.content_hash().unwrap(), p.content_hash().unwrap(), "{}", p.name);
            assert_eq!(back.connectivity(), p.connectivity(), "{}", p.name);
            assert_eq!(back.netlist(), p.netlist(), "{}", p.name);
        }
    }

    #[test]
    fn wire_length_settings_out_of_range_are_rejected_on_load() {
        let dir = tempfile::tempdir().unwrap();
        save(&start_circuit().unwrap(), dir.path()).unwrap();
        let path = dir.path().join("project.json");
        let mut head: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for bad in [
            serde_json::json!({"spare_per_end": -500.0, "round_to": 50.0}),
            serde_json::json!({"spare_per_end": 150.0, "round_to": -1.0}),
            serde_json::json!({"spare_per_end": 1e300, "round_to": 1e-300}),
        ] {
            head["wire_lengths"] = bad;
            std::fs::write(&path, serde_json::to_vec(&head).unwrap()).unwrap();
            let e = load(dir.path()).unwrap_err().to_string();
            assert!(e.contains("project.json") && e.contains("spare length"), "{e}");
        }
        head["wire_lengths"] = serde_json::json!({"spare_per_end": 0.0, "round_to": 0.0});
        std::fs::write(&path, serde_json::to_vec(&head).unwrap()).unwrap();
        assert_eq!(load(dir.path()).unwrap().wire_length_settings.round_to, 0.0);
    }

    #[test]
    fn an_underlay_is_saved_in_its_own_file_and_goes_when_removed() {
        use ecad_model::underlay::{Underlay, UnderlayShapes};
        let mut p = start_circuit().unwrap();
        let page = p.pages_in_order()[0].id;
        let shapes = UnderlayShapes {
            lines: vec![vec![Point::new(0.0, 0.0), Point::new(12.5, 0.25)]],
            fills: vec![[Point::new(0.0, 0.0), Point::new(1.0, 0.0), Point::new(0.0, 1.0)]],
            width: 12.5,
            height: 1.0,
        };
        let u = Underlay {
            source: "old.dxf".into(),
            shapes: std::sync::Arc::new(shapes),
            at: Point::new(5.0, 6.0),
            scale: 2.0,
            visible: false,
            units: ecad_model::underlay::Units::Inches,
            units_from: ecad_model::underlay::UnitsFrom::File,
        };
        p.apply(Command::SetUnderlay { page, underlay: Some(Box::new(u.clone())) }).unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&p, dir.path()).unwrap();
        let file = dir.path().join("underlays").join(format!("{}.json", page.0));
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("{\"source\":\"old.dxf\""));
        assert_eq!(load(dir.path()).unwrap().underlays.get(&page), Some(&u));
        // The content hash covers the underlay and survives the round trip.
        let with = p.content_hash().unwrap();
        assert_eq!(load(dir.path()).unwrap().content_hash().unwrap(), with);
        assert_ne!(with, start_circuit().unwrap().content_hash().unwrap());
        p.apply(Command::SetUnderlay { page, underlay: None }).unwrap();
        save(&p, dir.path()).unwrap();
        assert!(!file.exists());
        assert!(load(dir.path()).unwrap().underlays.is_empty());
        // A hand-edited file breaking the rules is refused with its name.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bad = r#"{"source":"x","at":[0,0],"scale":0,"visible":true,"width":1,"height":1,"lines":[],"fills":[]}"#;
        std::fs::write(&file, bad).unwrap();
        let e = load(dir.path()).unwrap_err().to_string();
        assert!(e.contains("underlays") && e.contains("scale above 0"), "{e}");
        // A file too big is refused before it is read whole (a sparse file: no disk used).
        let f = std::fs::File::create(&file).unwrap();
        f.set_len(MAX_UNDERLAY_FILE as u64 + 1).unwrap();
        drop(f);
        assert!(matches!(load(dir.path()), Err(ProjectError::TooBig { .. })));
        // And the fingerprint reads it in pieces without trouble.
        fingerprint(dir.path()).unwrap();
    }

    #[test]
    fn a_bounded_read_takes_up_to_the_limit_and_no_more() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, [7u8; 100]).unwrap();
        assert_eq!(read_bounded(&path, 100).unwrap().len(), 100);
        assert!(matches!(read_bounded(&path, 99), Err(ProjectError::TooBig { .. })));
        assert!(matches!(read_bounded(&dir.path().join("missing"), 10), Err(ProjectError::Io { .. })));
    }

    #[test]
    fn removed_pages_lose_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = start_circuit().unwrap();
        save(&p, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("pages")).unwrap().count(), 1);
        let ids: Vec<_> = p.functions.keys().copied().collect();
        for f in ids {
            p.apply(Command::Unplace(f)).unwrap();
        }
        p.pages.clear();
        save(&p, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("pages")).unwrap().count(), 0);
    }

    #[test]
    fn a_broken_file_is_rejected_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        save(&start_circuit().unwrap(), dir.path()).unwrap();
        let devices = dir.path().join("devices.json");
        let text = std::fs::read_to_string(&devices).unwrap().replace("\"-S1\"", "\"-K1\"");
        std::fs::write(&devices, text).unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("devices.json") && err.contains("-K1"), "{err}");
    }

    #[test]
    fn projects_saved_before_sheets_get_one_around_their_drawing() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        save(&original, dir.path()).unwrap();
        let head = dir.path().join("project.json");
        let mut json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&head).unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("sheet");
        std::fs::write(&head, json.to_string()).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.sheet, original.sheet, "the same placement the sample itself uses");
    }

    #[test]
    fn the_catalogue_is_saved_and_required_in_format_2() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        assert_eq!(original.parts.len(), 3);
        save(&original, dir.path()).unwrap();
        assert!(dir.path().join("products.json").exists());
        assert_eq!(load(dir.path()).unwrap(), original);
        // A format 2 project with no products still requires its canonical empty file.
        let mut old = original.clone();
        for d in old.devices.values_mut() {
            d.parts.clear();
        }
        old.parts.clear();
        save(&old, dir.path()).unwrap();
        std::fs::remove_file(dir.path().join("products.json")).unwrap();
        assert!(load(dir.path()).is_err(), "format 2 requires its product file, even when empty");
    }

    #[test]
    fn symbol_ids_must_be_safe_file_names() {
        assert_eq!(symbol_file_name("iec.coil").unwrap(), "iec.coil.json");
        assert!(symbol_file_name("../evil").is_err());
        assert!(symbol_file_name("").is_err());
    }
}
