//! A whole editing session as the UI sees it: the editor, the viewport, the project folder,
//! input in screen pixels, and the decisions around it (open, new, close, unsaved changes).
//! The UI only forwards [`UiEvent`]s, paints the [`Frame`] it gets back, and shows the
//! [`Frame::prompt`] when there is one. Both types are serialisable, for a future MCP server.

use crate::view::Viewport;
use crate::{Editor, Key, Overlay, Primitive, Selected, Tool};
use ecad_model::{Page, PageId, Part, PartRef, Point, Project, Role, SymbolId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hit tolerance in screen pixels; converted to millimetres at the current zoom.
const HIT_PX: f64 = 7.0;
/// Margin around the page when fitting it to the window.
const FIT_MARGIN_PX: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Primary,
    /// Middle button drags pan the view.
    Middle,
}

/// The answer to an unsaved-changes prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Save,
    Discard,
    Cancel,
}

/// Input from the UI, in screen pixels relative to the canvas's top-left corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    Resize {
        width: f64,
        height: f64,
    },
    PointerMove {
        x: f64,
        y: f64,
    },
    PointerDown {
        x: f64,
        y: f64,
        button: Button,
    },
    PointerUp {
        x: f64,
        y: f64,
        button: Button,
    },
    /// Mouse wheel: positive `delta` zooms in.
    Wheel {
        x: f64,
        y: f64,
        delta: f64,
    },
    Key {
        key: KeyName,
    },
    /// `select`, `wire`, or a symbol id to place.
    Tool {
        tool: String,
    },
    /// New tag for the selected symbol's device.
    Rename {
        tag: String,
    },
    /// New description for the selected symbol's device.
    Describe {
        text: String,
    },
    /// The selected box's label.
    LabelBox {
        text: String,
    },
    /// The selected wire's cross-section, colour, and cable (by tag, empty for none) and core.
    WireSpec {
        cross_section: String,
        colour: String,
        cable: String,
        core: String,
    },
    /// A new cable.
    AddCable {
        tag: String,
        kind: String,
        cores: u32,
        shielded: bool,
    },
    UpdateCable {
        id: ecad_model::CableId,
        tag: String,
        kind: String,
        cores: u32,
        shielded: bool,
    },
    /// Remove a cable no wire runs in.
    RemoveCable {
        id: ecad_model::CableId,
    },
    /// New number for the selected wire; blank clears it.
    NumberWire {
        number: String,
    },
    Save,
    Fit,
    /// Open the project in this folder (asks first when there are unsaved changes).
    Open {
        path: PathBuf,
    },
    /// Create an empty project in this folder and open it.
    New {
        path: PathBuf,
    },
    /// The window wants to close (asks first when there are unsaved changes).
    CloseRequested,
    /// The answer to [`Frame::prompt`].
    Confirm {
        choice: Choice,
    },
    /// Show this page.
    ShowPage {
        id: PageId,
    },
    /// Show the page of [`Frame::checks`]`[index]` and select what it points at.
    ShowCheck {
        index: usize,
    },
    /// Add a page after the current one and show it.
    AddPage,
    /// Delete the current page and everything on it (undoable).
    DeletePage,
    RenamePage {
        name: String,
    },
    DescribePage {
        text: String,
    },
    /// The current page title's second line (e.g. in a second language).
    SubtitlePage {
        text: String,
    },
    /// The project name and title-block fields, as the project settings show them.
    ProjectInfo {
        name: String,
        info: ecad_model::ProjectInfo,
    },
    /// Make a black-box symbol (a box with named terminals) and pick it for placing.
    CreateBlackBox {
        spec: ecad_model::library::BlackBox,
    },
    /// How new tags and wire numbers are formed.
    Naming {
        naming: ecad_model::Naming,
    },
    /// Number every wire again from the wire format.
    RenumberWires,
    /// Move the page's wires off each other and out of symbols where a nearby route is clear.
    TidyWires,
    /// Put the selected wire's middle run back halfway.
    StraightenWire,
    /// The sheet's columns (numbered from `first_column`, 0 or 1) and lettered rows (0: none).
    SheetGrid {
        columns: u32,
        first_column: u32,
        rows: u32,
    },
    /// Move the current page earlier (negative) or later (positive).
    MovePage {
        delta: i32,
    },
    /// Place one more contact of the selected symbol's device, drawn with this symbol.
    AddContact {
        symbol: String,
    },
    /// Make the selected symbol part of the device with this tag.
    AssignTo {
        tag: String,
    },
    /// Add a part to the catalogue.
    AddPart {
        part: Part,
    },
    /// Replace the catalogue entry with this part's number.
    UpdatePart {
        part: Part,
    },
    /// A part's size, as typed: `45 x 90` or `45 x 90 x 75` (mm), empty to clear.
    PartSize {
        number: String,
        size: String,
    },
    /// A part's contact blocks, as typed: `13-14 21-22` or `11-12-14`.
    PartContacts {
        number: String,
        contacts: String,
    },
    RemovePart {
        number: String,
    },
    /// Import a CSV catalogue: new part numbers are added, known ones updated.
    ImportParts {
        path: PathBuf,
    },
    /// Give the selected symbol's device one of this part (or one more).
    AddDevicePart {
        number: String,
    },
    RemoveDevicePart {
        number: String,
    },
    SetDevicePartQuantity {
        number: String,
        quantity: u32,
    },
    /// On a terminal strip, how a part counts: `terminal` once per terminal, `strip` once for
    /// the strip (end plates, brackets), or a terminal symbol's id once per terminal drawn with
    /// it (PE terminals with `iec.terminal_pe`). See [`DevicePart::count`].
    SetDevicePartCount {
        number: String,
        count: String,
    },
    /// Write every report as CSV into this folder.
    ExportReports {
        path: PathBuf,
    },
    /// Write the drawings (one PDF with every page, plus DXF, DWG and PNG per page) into a folder.
    ExportDrawings {
        path: PathBuf,
    },
    /// Give the selected terminal or PLC channel another number.
    RenumberTerminal {
        number: String,
    },
    /// Set the selected PLC channel's `address`, `symbol` or `comment`; blank clears it.
    SetPlcProperty {
        key: String,
        value: String,
    },
}

/// Keys the UI maps from its own key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyName {
    Escape,
    Delete,
    Rotate,
    Mirror,
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteItem {
    pub id: String,
    pub description: String,
    /// The library group, e.g. "Contacts".
    pub category: String,
}

/// One finding of the checks, for the checks panel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckInfo {
    /// `error` or `warning`.
    pub severity: String,
    pub rule: String,
    pub message: String,
    /// Page and column, e.g. `2.4`.
    pub location: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageInfo {
    pub id: PageId,
    pub name: String,
    pub description: String,
    pub subtitle: String,
}

/// The sheet's grid, for the project settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct SheetGrid {
    pub columns: u32,
    pub first_column: u32,
    pub rows: u32,
}

/// What is selected, with the fields the side panel shows and edits.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SelectionInfo {
    Device {
        tag: String,
        description: String,
        symbol: String,
        /// `main` (a coil), `contact`, or `single`.
        role: String,
        /// For a coil: its contacts. For a contact: its coil. Each can be shown with
        /// [`UiEvent::ShowPage`].
        references: Vec<Reference>,
        /// Catalogue parts the device is built from.
        parts: Vec<DevicePart>,
        /// For a terminal strip: the kinds of terminal drawn on it, as (symbol id, description),
        /// for counting a part per terminal of one kind.
        terminal_kinds: Vec<(String, String)>,
        /// For a terminal, its number on the strip.
        terminal: Option<String>,
        /// For a PLC channel: its kind, number, address, symbol and comment.
        plc: Option<PlcInfo>,
        /// The labels of the boxes around this drawing, outermost first (`+A101 -XCPR1`).
        location: String,
    },
    Wire {
        number: Option<String>,
        from: String,
        to: String,
        /// The potential's name (`L+`), if it has one.
        potential: Option<String>,
        /// Set when differently named potentials are connected on this wire's potential.
        warning: Option<String>,
        cross_section: String,
        colour: String,
        /// The cable's tag and the core, empty when the wire isn't in a cable.
        cable: String,
        core: String,
        /// Whether its middle run was moved (it can be straightened).
        bent: bool,
    },
    /// A location box: its label and the devices drawn inside it.
    Box { label: String, devices: Vec<String> },
}

/// A cable, for the cables window and the wire panel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CableInfo {
    pub id: ecad_model::CableId,
    pub tag: String,
    pub kind: String,
    pub cores: u32,
    pub shielded: bool,
    /// How many wires run in it.
    pub used: usize,
}

/// The selected PLC channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlcInfo {
    /// `DI`, `DO`, `AI` or `AO`.
    pub kind: String,
    pub channel: String,
    pub address: String,
    pub symbol: String,
    pub comment: String,
}

/// A part of the selected device, with its catalogue description.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DevicePart {
    pub number: String,
    pub quantity: u32,
    /// On a strip: `terminal`, `strip` or a terminal symbol's id (see
    /// [`UiEvent::SetDevicePartCount`]).
    pub count: String,
    pub description: String,
}

/// A cross-reference the side panel lists, e.g. `13-14 NO 2.7`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reference {
    pub label: String,
    pub page: PageId,
}

/// Everything the UI needs to paint one frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub revision: u64,
    /// The page in page millimetres; `None` when the UI already has this revision.
    pub base: Option<Vec<Primitive>>,
    pub overlays: Vec<Overlay>,
    pub view: Viewport,
    pub status: String,
    /// `select`, `wire`, or the symbol id being placed.
    pub tool: String,
    pub selection: Option<SelectionInfo>,
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub dirty: bool,
    pub title: String,
    pub project_dir: String,
    pub palette: Vec<PaletteItem>,
    /// Contact symbols, offered for adding contacts to the selected device.
    pub contact_palette: Vec<PaletteItem>,
    /// PLC channel symbols, offered for adding channels to the selected card.
    pub channel_palette: Vec<PaletteItem>,
    /// The parts catalogue, by part number.
    pub parts: Vec<Part>,
    /// All pages in order, and the one shown.
    pub pages: Vec<PageInfo>,
    pub cables: Vec<CableInfo>,
    /// The project name as the settings edit it (the title adds a `*` when unsaved).
    pub project_name: String,
    pub info: ecad_model::ProjectInfo,
    pub naming: ecad_model::Naming,
    pub sheet: SheetGrid,
    /// The project's checks, errors first; updated on every change.
    pub checks: Vec<CheckInfo>,
    pub page: PageId,
    /// An unsaved-changes question waiting for [`UiEvent::Confirm`].
    pub prompt: Option<String>,
    /// The window may close now.
    pub close: bool,
}

/// An action waiting for the unsaved-changes answer.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Open(PathBuf),
    New(PathBuf),
    Close,
}

#[derive(Debug)]
pub struct Session {
    editor: Editor,
    view: Viewport,
    dir: PathBuf,
    panning: Option<(f64, f64)>,
    fitted: bool,
    message: Option<String>,
    pending: Option<Pending>,
    closing: bool,
}

fn first_page_editor(project: Project) -> Result<Editor, String> {
    let page = *project.pages.keys().next().ok_or("the project has no pages")?;
    Editor::new(project, page).map_err(|e| e.to_string())
}

fn load_editor(dir: &Path) -> Result<Editor, String> {
    let mut project = ecad_project::load(dir).map_err(|e| e.to_string())?;
    // Projects made before a library symbol existed get it; the project's own copies win.
    for s in ecad_model::library::iec() {
        if !project.symbols.contains_key(&s.id) {
            project.add_symbol(s);
        }
    }
    first_page_editor(project)
}

/// An empty project with one page and the starter symbols, named after its folder.
fn blank_project(dir: &Path) -> Project {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("New project");
    let mut p = Project::new(name);
    for s in ecad_model::library::iec() {
        p.add_symbol(s);
    }
    p.add_page(Page::new("1", 0));
    p
}

impl Session {
    /// Open the project in `dir`, or create the sample project there when the folder has none.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let editor = if dir.join("project.json").exists() {
            load_editor(dir)?
        } else {
            let p = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
            ecad_project::save(&p, dir).map_err(|e| e.to_string())?;
            first_page_editor(p)?
        };
        Ok(Session {
            editor,
            view: Viewport::default(),
            dir: dir.to_owned(),
            panning: None,
            fitted: false,
            message: None,
            pending: None,
            closing: false,
        })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }
    pub fn view(&self) -> &Viewport {
        &self.view
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    /// Whether the window may close without asking.
    pub fn can_close(&self) -> bool {
        self.closing || !self.editor.is_dirty()
    }

    fn page_point(&mut self, x: f64, y: f64) -> Point {
        self.editor.hit_tolerance = HIT_PX / self.view.scale;
        self.view.to_page(x, y)
    }

    fn fit(&mut self) {
        match self.editor.extents() {
            Some((lo, hi)) => self.view.fit(lo, hi, FIT_MARGIN_PX),
            None => self.view.center = Point::ZERO,
        }
    }

    fn report<E: std::fmt::Display>(&mut self, r: Result<(), E>) {
        if let Err(e) = r {
            self.message = Some(e.to_string());
        }
    }

    pub fn handle(&mut self, ev: UiEvent) {
        self.message = None;
        // While a question is open, only the answer and view changes are accepted.
        if self.pending.is_some()
            && !matches!(ev, UiEvent::Confirm { .. } | UiEvent::Resize { .. } | UiEvent::CloseRequested)
        {
            return;
        }
        match ev {
            UiEvent::Resize { width, height } => {
                self.view.width = width.max(1.0);
                self.view.height = height.max(1.0);
                if !self.fitted {
                    self.fit();
                    self.fitted = true;
                }
            }
            UiEvent::PointerMove { x, y } => {
                if let Some((px, py)) = self.panning {
                    self.view.pan(x - px, y - py);
                    self.panning = Some((x, y));
                } else {
                    let p = self.page_point(x, y);
                    self.editor.pointer_move(p);
                }
            }
            UiEvent::PointerDown { x, y, button: Button::Middle } => self.panning = Some((x, y)),
            UiEvent::PointerUp { button: Button::Middle, .. } => self.panning = None,
            UiEvent::PointerDown { x, y, button: Button::Primary } => {
                let p = self.page_point(x, y);
                self.editor.pointer_down(p);
            }
            UiEvent::PointerUp { x, y, button: Button::Primary } => {
                let p = self.page_point(x, y);
                self.editor.pointer_up(p);
            }
            UiEvent::Wheel { x, y, delta } => self.view.zoom_at(x, y, 1.15_f64.powf(delta.clamp(-10.0, 10.0))),
            UiEvent::Key { key } => self.editor.key(match key {
                KeyName::Escape => Key::Escape,
                KeyName::Delete => Key::Delete,
                KeyName::Rotate => Key::Rotate,
                KeyName::Mirror => Key::Mirror,
                KeyName::Undo => Key::Undo,
                KeyName::Redo => Key::Redo,
            }),
            UiEvent::Tool { tool } => self.editor.set_tool(match tool.as_str() {
                "select" => Tool::Select,
                "wire" => Tool::Wire,
                "box" => Tool::Box,
                symbol => Tool::Place(SymbolId::new(symbol)),
            }),
            UiEvent::Rename { tag } => {
                let r = self.editor.rename_selected(tag.trim());
                self.report(r);
            }
            UiEvent::Describe { text } => {
                let r = self.editor.describe_selected(&text);
                self.report(r);
            }
            UiEvent::LabelBox { text } => {
                let r = self.editor.label_selected_box(&text);
                self.report(r);
            }
            UiEvent::WireSpec { cross_section, colour, cable, core } => {
                let r = self.editor.set_selected_wire_spec(&cross_section, &colour, &cable, &core);
                self.report(r);
            }
            UiEvent::AddCable { tag, kind, cores, shielded } => {
                let cable = ecad_model::Cable { id: ecad_model::CableId::new(), tag, kind, cores, shielded };
                let r = self.editor.add_cable(cable);
                self.report(r);
            }
            UiEvent::UpdateCable { id, tag, kind, cores, shielded } => {
                let r = self.editor.update_cable(ecad_model::Cable { id, tag, kind, cores, shielded });
                self.report(r);
            }
            UiEvent::RemoveCable { id } => {
                let r = self.editor.remove_cable(id);
                self.report(r);
            }
            UiEvent::NumberWire { number } => {
                let r = self.editor.number_selected_wire(&number);
                self.report(r);
            }
            UiEvent::Save => self.save(),
            UiEvent::Fit => self.fit(),
            UiEvent::Open { path } => self.request(Pending::Open(path)),
            UiEvent::New { path } => self.request(Pending::New(path)),
            UiEvent::CloseRequested => {
                if self.pending.is_none() {
                    self.request(Pending::Close);
                }
            }
            UiEvent::ShowCheck { index } => {
                let checks = self.editor.checks();
                if let Some(check) = checks.get(index) {
                    self.message = Some(match self.editor.show_check(check) {
                        Ok(()) => format!("{} {}", check.location, check.message),
                        Err(e) => e.to_string(),
                    });
                }
            }
            UiEvent::ShowPage { id } => {
                let r = self.editor.set_page(id);
                self.report(r);
            }
            UiEvent::AddPage => {
                let r = self.editor.add_page();
                self.report(r);
            }
            UiEvent::DeletePage => {
                let r = self.editor.delete_page();
                self.report(r);
            }
            UiEvent::RenamePage { name } => {
                let r = self.editor.rename_page(&name);
                self.report(r);
            }
            UiEvent::DescribePage { text } => {
                let r = self.editor.describe_page(&text);
                self.report(r);
            }
            UiEvent::SubtitlePage { text } => {
                let r = self.editor.subtitle_page(&text);
                self.report(r);
            }
            UiEvent::ProjectInfo { name, info } => {
                let r = self.editor.set_project_info(&name, info);
                self.report(r);
            }
            UiEvent::CreateBlackBox { spec } => {
                let r = self.editor.create_black_box(&spec);
                self.report(r);
            }
            UiEvent::Naming { naming } => {
                let r = self.editor.set_naming(naming);
                self.report(r);
            }
            UiEvent::TidyWires => match self.editor.tidy_wires() {
                Ok(0) => self.message = Some("No wire needed moving".into()),
                Ok(1) => self.message = Some("Moved 1 wire".into()),
                Ok(n) => self.message = Some(format!("Moved {n} wires")),
                Err(e) => self.report(Err(e)),
            },
            UiEvent::StraightenWire => {
                let r = self.editor.straighten_selected_wire();
                self.report(r);
            }
            UiEvent::RenumberWires => {
                let r = self.editor.renumber_all_wires();
                self.report(r);
                if self.message.is_none() {
                    self.message = Some(format!("Renumbered {} wires", self.editor.project().connections.len()));
                }
            }
            UiEvent::SheetGrid { columns, first_column, rows } => {
                let r = self.editor.set_sheet_grid(columns, first_column, rows);
                self.report(r);
            }
            UiEvent::MovePage { delta } => {
                let r = self.editor.move_page(delta);
                self.report(r);
            }
            UiEvent::AddContact { symbol } => match self.selected_device() {
                Some(device) => {
                    self.editor.set_tool(Tool::PlaceFor { symbol: SymbolId::new(&symbol), device });
                    self.message = Some("Click where the contact goes; switch pages first if needed".into());
                }
                None => self.message = Some("Select the device first".into()),
            },
            UiEvent::AssignTo { tag } => {
                let r = self.editor.reassign_selected(&tag);
                self.report(r);
            }
            UiEvent::AddPart { part } => {
                let r = self.editor.add_part(part);
                self.report(r);
            }
            UiEvent::UpdatePart { part } => {
                let r = self.editor.update_part(part);
                self.report(r);
            }
            UiEvent::PartSize { number, size } => match ecad_model::parts::parse_size(&size) {
                Err(bad) => {
                    self.message = Some(format!("`{bad}` isn't a size: write width x height, e.g. 45 x 90 (mm)"))
                }
                Ok((width, height, depth)) => {
                    if let Some(mut part) = self.editor.project().parts.get(&number).cloned()
                        && (part.width, part.height, part.depth) != (width, height, depth)
                    {
                        (part.width, part.height, part.depth) = (width, height, depth);
                        let r = self.editor.update_part(part);
                        self.report(r);
                    }
                }
            },
            UiEvent::PartContacts { number, contacts } => match ecad_model::parts::parse_blocks(&contacts) {
                Err(bad) => {
                    self.message =
                        Some(format!("`{bad}` isn't a contact: write 13-14 (NO), 21-22 (NC) or 11-12-14 (CO)"));
                }
                Ok(blocks) => {
                    if let Some(mut part) = self.editor.project().parts.get(&number).cloned()
                        && part.contacts != blocks
                    {
                        part.contacts = blocks;
                        let r = self.editor.update_part(part);
                        self.report(r);
                    }
                }
            },
            UiEvent::RemovePart { number } => {
                let r = self.editor.remove_part(&number);
                self.report(r);
            }
            UiEvent::ImportParts { path } => self.import_parts(&path),
            UiEvent::AddDevicePart { number } => {
                self.edit_device_parts(|parts| match parts.iter_mut().find(|p| p.number == number) {
                    Some(p) => p.quantity += 1,
                    None => parts.push(PartRef::new(&number, 1)),
                })
            }
            UiEvent::RemoveDevicePart { number } => {
                self.edit_device_parts(|parts| parts.retain(|p| p.number != number))
            }
            UiEvent::SetDevicePartQuantity { number, quantity } => self.edit_device_parts(|parts| {
                if let Some(p) = parts.iter_mut().find(|p| p.number == number) {
                    p.quantity = quantity;
                }
            }),
            UiEvent::SetDevicePartCount { number, count } => self.edit_device_parts(|parts| {
                if let Some(p) = parts.iter_mut().find(|p| p.number == number) {
                    (p.per_strip, p.only) = match count.as_str() {
                        "terminal" => (false, None),
                        "strip" => (true, None),
                        symbol => (false, Some(ecad_model::SymbolId::new(symbol))),
                    };
                }
            }),
            UiEvent::ExportReports { path } => self.export_reports(&path),
            UiEvent::ExportDrawings { path } => self.export_drawings(&path),
            UiEvent::SetPlcProperty { key, value } => {
                let r = self.editor.set_selected_property(&key, &value);
                self.report(r);
            }
            UiEvent::RenumberTerminal { number } => {
                let r = self.editor.renumber_selected(&number);
                self.report(r);
            }
            UiEvent::Confirm { choice } => {
                let Some(action) = self.pending.take() else { return };
                match choice {
                    Choice::Cancel => {}
                    Choice::Discard => self.perform(action),
                    Choice::Save => {
                        self.save();
                        // A failed save leaves the changes in place: don't throw them away.
                        if !self.editor.is_dirty() {
                            self.perform(action);
                        }
                    }
                }
            }
        }
    }

    fn request(&mut self, action: Pending) {
        if self.editor.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        let result = match action {
            Pending::Close => {
                self.closing = true;
                Ok(())
            }
            Pending::Open(path) => load_editor(&path).map(|e| self.replace(e, path)),
            Pending::New(path) => {
                if path.join("project.json").exists() {
                    Err(format!("{} already contains a project; use Open", path.display()))
                } else {
                    let p = blank_project(&path);
                    ecad_project::save(&p, &path)
                        .map_err(|e| e.to_string())
                        .and_then(|()| first_page_editor(p))
                        .map(|e| self.replace(e, path))
                }
            }
        };
        self.report(result);
    }

    fn replace(&mut self, editor: Editor, dir: PathBuf) {
        self.message = Some(format!("Opened {}", dir.display()));
        self.editor = editor;
        self.dir = dir;
        self.panning = None;
        self.fit();
    }

    pub fn save(&mut self) {
        self.message = Some(match ecad_project::save(self.editor.project(), &self.dir) {
            Ok(()) => {
                self.editor.mark_saved();
                format!("Saved to {}", self.dir.display())
            }
            Err(e) => format!("Save failed: {e}"),
        });
    }

    fn edit_device_parts(&mut self, change: impl FnOnce(&mut Vec<PartRef>)) {
        let Some(device) = self.selected_device() else { return };
        let Ok(d) = self.editor.project().device(device) else { return };
        let mut parts = d.parts.clone();
        change(&mut parts);
        if parts != d.parts {
            let r = self.editor.set_selected_parts(parts);
            self.report(r);
        }
    }

    fn import_parts(&mut self, path: &Path) {
        let result = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|text| ecad_project::csv::read_parts(&text).map_err(|e| format!("{}: {e}", path.display())))
            .and_then(|parts| self.editor.import_parts(parts).map_err(|e| e.to_string()));
        self.message = Some(match result {
            Ok((0, 0)) => "Nothing new to import: the catalogue already has these parts".into(),
            Ok((added, updated)) => format!("Imported {added} new parts, updated {updated}"),
            Err(e) => format!("Import failed: {e}"),
        });
    }

    fn export_drawings(&mut self, dir: &Path) {
        let result = ecad_view::drawing_files(self.editor.project(), ecad_view::Formats::ALL)
            .map_err(|e| e.to_string())
            .and_then(|files| {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                for (name, bytes) in &files {
                    std::fs::write(dir.join(name), bytes).map_err(|e| format!("{}: {e}", dir.join(name).display()))?;
                }
                Ok(files.len())
            });
        self.message = Some(match result {
            Ok(n) => format!("Wrote drawings.pdf and {} page files to {}", n - 1, dir.display()),
            Err(e) => format!("Export failed: {e}"),
        });
    }

    fn export_reports(&mut self, dir: &Path) {
        let result = ecad_reports::all_csv(self.editor.project()).map_err(|e| e.to_string()).and_then(|files| {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for (name, text) in &files {
                std::fs::write(dir.join(name), text).map_err(|e| format!("{}: {e}", dir.join(name).display()))?;
            }
            Ok(files.len())
        });
        let missing = ecad_reports::devices_without_parts(self.editor.project()).len();
        self.message = Some(match result {
            Ok(n) if missing > 0 => {
                format!("Wrote {n} reports to {}; {missing} devices have no part yet", dir.display())
            }
            Ok(n) => format!("Wrote {n} reports to {}", dir.display()),
            Err(e) => format!("Export failed: {e}"),
        });
    }

    fn selected_device(&self) -> Option<ecad_model::DeviceId> {
        match self.editor.selection()? {
            Selected::Function(f) => self.editor.project().function(f).ok().map(|f| f.device),
            Selected::Connection(_) | Selected::Box(_) => None,
        }
    }

    fn selection_info(&self) -> Option<SelectionInfo> {
        let project = self.editor.project();
        match self.editor.selection()? {
            Selected::Function(f) => {
                let f = project.function(f).ok()?;
                let d = project.device(f.device).ok()?;
                let role = project.role(f.id).ok()?;
                let references = match role {
                    Role::Main => project
                        .contacts_of(d.id)
                        .into_iter()
                        .map(|c| Reference {
                            label: format!("{} {} {}", c.terminals, c.kind.short(), c.location),
                            page: c.page,
                        })
                        .collect(),
                    Role::Contact { .. } => project
                        .main_function(d.id)
                        .and_then(|m| {
                            let at = project.function_location(m.id).ok()?;
                            Some(vec![Reference { label: format!("Coil at {at}"), page: m.page }])
                        })
                        .unwrap_or_default(),
                    // Potential and interruption points: the device's other drawings.
                    Role::Potential | Role::Interruption => project
                        .functions_of(d.id)
                        .filter(|g| g.id != f.id)
                        .filter_map(|g| {
                            let at = project.function_location(g.id).ok()?;
                            Some(Reference { label: format!("Also at {at}"), page: g.page })
                        })
                        .collect(),
                    // A terminal: the strip's other terminals.
                    Role::Terminal => project
                        .terminals_of(d.id)
                        .into_iter()
                        .filter(|t| t.function != f.id)
                        .map(|t| Reference { label: format!("Terminal {} at {}", t.number, t.location), page: t.page })
                        .collect(),
                    // A PLC channel: the card's other channels.
                    Role::PlcChannel { .. } => project
                        .plc_channels_of(d.id)
                        .into_iter()
                        .filter(|c| c.function != f.id)
                        .map(|c| Reference {
                            label: format!(
                                "{} {} {}at {}",
                                c.kind.short(),
                                c.channel,
                                if c.address.is_empty() { String::new() } else { format!("{} ", c.address) },
                                c.location
                            ),
                            page: c.page,
                        })
                        .collect(),
                    Role::Single => Vec::new(),
                };
                Some(SelectionInfo::Device {
                    tag: d.tag.clone(),
                    description: d.description.clone(),
                    symbol: f.symbol.0.clone(),
                    role: match role {
                        Role::Main => "main",
                        Role::Contact { .. } => "contact",
                        Role::Potential => "potential",
                        Role::Interruption => "interruption",
                        Role::Terminal => "terminal",
                        Role::PlcChannel { .. } => "plc",
                        Role::Single => "single",
                    }
                    .into(),
                    references,
                    terminal_kinds: {
                        let mut kinds: Vec<(String, String)> = project
                            .functions_of(d.id)
                            .filter(|g| matches!(project.role(g.id), Ok(Role::Terminal)))
                            .filter_map(|g| {
                                project.symbol(&g.symbol).ok().map(|s| (s.id.0.clone(), s.description.clone()))
                            })
                            .collect();
                        kinds.sort();
                        kinds.dedup();
                        kinds
                    },
                    terminal: project.terminal_number(f.id),
                    location: project.box_labels(f.id).join(" "),
                    plc: match role {
                        Role::PlcChannel { kind } => Some(PlcInfo {
                            kind: kind.short().into(),
                            channel: project.drawing_number(f.id).unwrap_or_default(),
                            address: f.property(ecad_model::ADDRESS).to_owned(),
                            symbol: f.property(ecad_model::SYMBOL).to_owned(),
                            comment: f.property(ecad_model::COMMENT).to_owned(),
                        }),
                        _ => None,
                    },
                    parts: d
                        .parts
                        .iter()
                        .map(|r| DevicePart {
                            number: r.number.clone(),
                            quantity: r.quantity,
                            count: match (&r.only, r.per_strip) {
                                (_, true) => "strip".into(),
                                (Some(s), false) => s.0.clone(),
                                (None, false) => "terminal".into(),
                            },
                            description: project
                                .parts
                                .get(&r.number)
                                .map(|p| p.description.clone())
                                .unwrap_or_default(),
                        })
                        .collect(),
                })
            }
            Selected::Box(b) => {
                let b = project.boxes.get(&b)?;
                let mut devices: Vec<String> = project
                    .functions_on(b.page)
                    .filter(|f| b.contains(f.position))
                    .filter_map(|f| project.device(f.device).ok().map(|d| d.tag.clone()))
                    .collect();
                devices.sort();
                devices.dedup();
                Some(SelectionInfo::Box { label: b.label.clone(), devices })
            }
            Selected::Connection(c) => {
                let c = project.connection(c).ok()?;
                let end = |r| project.describe(r).map(|(t, p)| format!("{t}:{p}")).unwrap_or_default();
                let potential = self.editor.potential_of(c.id);
                Some(SelectionInfo::Wire {
                    number: c.wire_number.clone(),
                    from: end(&c.from),
                    to: end(&c.to),
                    potential: potential.as_ref().and_then(|p| p.name()),
                    warning: potential.as_ref().and_then(|p| p.conflict()),
                    cross_section: c.spec.cross_section.clone(),
                    colour: c.spec.colour.clone(),
                    cable: c
                        .spec
                        .cable
                        .as_ref()
                        .and_then(|k| project.cables.get(&k.cable))
                        .map(|k| k.tag.clone())
                        .unwrap_or_default(),
                    core: c.spec.cable.as_ref().map(|k| k.core.clone()).unwrap_or_default(),
                    bent: c.bend.is_some(),
                })
            }
        }
    }

    /// The frame to paint. `known_revision` is the revision whose base the UI already holds.
    pub fn frame(&self, known_revision: Option<u64>, background: [u8; 3]) -> Frame {
        let editor = &self.editor;
        let revision = editor.scene_revision();
        let (base, base_error) = if known_revision == Some(revision) {
            (None, None)
        } else {
            match editor.base(background) {
                Ok(b) => (Some(b), None),
                Err(e) => (Some(Vec::new()), Some(e.to_string())),
            }
        };
        let project = editor.project();
        let status = base_error.or_else(|| self.message.clone()).unwrap_or_else(|| editor.status().to_owned());
        let prompt = self.pending.as_ref().map(|p| match p {
            Pending::Close => format!("Save changes to {} before closing?", project.name),
            Pending::Open(_) | Pending::New(_) => {
                format!("Save changes to {} before opening another project?", project.name)
            }
        });
        Frame {
            revision,
            base,
            overlays: editor.overlays(),
            view: self.view,
            status,
            tool: match editor.tool() {
                Tool::Select => "select".into(),
                Tool::Wire => "wire".into(),
                Tool::Box => "box".into(),
                Tool::Place(s) => s.0.clone(),
                Tool::PlaceFor { symbol, .. } => format!("contact:{}", symbol.0),
            },
            selection: self.selection_info(),
            undo: editor.undo_label().map(str::to_owned),
            redo: editor.redo_label().map(str::to_owned),
            dirty: editor.is_dirty(),
            title: format!("{}{}", project.name, if editor.is_dirty() { " *" } else { "" }),
            project_dir: self.dir.display().to_string(),
            palette: {
                let mut items: Vec<PaletteItem> = project
                    .symbols
                    .values()
                    .map(|s| PaletteItem {
                        id: s.id.0.clone(),
                        description: s.description.clone(),
                        category: if s.category.is_empty() { "Other".into() } else { s.category.clone() },
                    })
                    .collect();
                items.sort_by(|a, b| (&a.category, &a.description).cmp(&(&b.category, &b.description)));
                items
            },
            parts: project.parts.values().cloned().collect(),
            contact_palette: project
                .symbols
                .values()
                // Plain contacts for coils and breakers; operator and sensor contacts are
                // devices of their own.
                .filter(|s| {
                    matches!(s.role, Role::Contact { .. }) && (s.category == "Contacts" || s.category.is_empty())
                })
                .map(|s| PaletteItem {
                    id: s.id.0.clone(),
                    description: s.description.clone(),
                    category: s.category.clone(),
                })
                .collect(),
            channel_palette: project
                .symbols
                .values()
                .filter(|s| matches!(s.role, Role::PlcChannel { .. }))
                .map(|s| PaletteItem {
                    id: s.id.0.clone(),
                    description: s.description.clone(),
                    category: s.category.clone(),
                })
                .collect(),
            pages: project
                .pages_in_order()
                .into_iter()
                .map(|p| PageInfo {
                    id: p.id,
                    name: p.name.clone(),
                    description: p.description.clone(),
                    subtitle: p.subtitle.clone(),
                })
                .collect(),
            page: editor.page(),
            cables: {
                let mut cables: Vec<CableInfo> = project
                    .cables
                    .values()
                    .map(|c| CableInfo {
                        id: c.id,
                        tag: c.tag.clone(),
                        kind: c.kind.clone(),
                        cores: c.cores,
                        shielded: c.shielded,
                        used: project
                            .connections
                            .values()
                            .filter(|w| w.spec.cable.as_ref().is_some_and(|k| k.cable == c.id))
                            .count(),
                    })
                    .collect();
                cables.sort_by(|a, b| a.tag.cmp(&b.tag));
                cables
            },
            project_name: project.name.clone(),
            info: project.info.clone(),
            naming: project.naming.clone(),
            sheet: SheetGrid {
                columns: project.sheet.columns,
                first_column: project.sheet.first_column,
                rows: project.sheet.rows,
            },
            checks: editor
                .checks()
                .into_iter()
                .map(|c| CheckInfo {
                    severity: c.severity.as_str().into(),
                    rule: c.rule.into(),
                    message: c.message,
                    location: c.location,
                })
                .collect(),
            prompt,
            close: self.closing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::open(dir.path()).unwrap();
        s.handle(UiEvent::Resize { width: 800.0, height: 600.0 });
        (dir, s)
    }

    fn place_coil(s: &mut Session) {
        s.handle(UiEvent::Tool { tool: "iec.coil".into() });
        let (x, y) = s.view().to_screen(Point::new(40.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
    }

    #[test]
    fn a_new_folder_gets_the_sample_and_the_view_fits_it() {
        let (dir, s) = session();
        assert!(dir.path().join("project.json").exists());
        let (lo, hi) = s.editor().extents().unwrap();
        let (x0, y0) = s.view().to_screen(lo);
        let (x1, y1) = s.view().to_screen(hi);
        assert!(x0 >= 0.0 && x1 <= 800.0 && y1 >= 0.0 && y0 <= 600.0, "whole page visible");
    }

    #[test]
    fn frames_carry_the_base_only_when_it_changed() {
        let (_dir, mut s) = session();
        let f = s.frame(None, [0, 0, 0]);
        assert!(f.base.as_ref().is_some_and(|b| !b.is_empty()));
        assert!(s.frame(Some(f.revision), [0, 0, 0]).base.is_none());
        place_coil(&mut s);
        let g = s.frame(Some(f.revision), [0, 0, 0]);
        assert!(g.base.is_some() && g.dirty && g.title.ends_with('*'));
        assert!(matches!(&g.selection, Some(SelectionInfo::Device { tag, .. }) if tag == "-K2"));
    }

    #[test]
    fn save_writes_the_folder_and_clears_dirty() {
        let (dir, mut s) = session();
        place_coil(&mut s);
        s.handle(UiEvent::Save);
        assert!(!s.frame(None, [0, 0, 0]).dirty);
        assert!(ecad_project::load(dir.path()).unwrap().device_by_tag("-K2").is_some());
    }

    #[test]
    fn middle_drag_pans_and_wheel_zooms_without_editing() {
        let (_dir, mut s) = session();
        let before = *s.view();
        s.handle(UiEvent::PointerDown { x: 100.0, y: 100.0, button: Button::Middle });
        s.handle(UiEvent::PointerMove { x: 140.0, y: 100.0 });
        s.handle(UiEvent::PointerUp { x: 140.0, y: 100.0, button: Button::Middle });
        assert!((s.view().center.x - (before.center.x - 40.0 / before.scale)).abs() < 1e-9);
        s.handle(UiEvent::Wheel { x: 400.0, y: 300.0, delta: 1.0 });
        assert!(s.view().scale > before.scale);
        assert!(!s.editor().is_dirty());
    }

    #[test]
    fn closing_a_clean_project_needs_no_question() {
        let (_dir, mut s) = session();
        assert!(s.can_close());
        s.handle(UiEvent::CloseRequested);
        assert!(s.frame(None, [0, 0, 0]).close);
    }

    #[test]
    fn closing_with_changes_asks_and_each_answer_does_what_it_says() {
        let (dir, mut s) = session();
        place_coil(&mut s);
        assert!(!s.can_close());

        s.handle(UiEvent::CloseRequested);
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.prompt.as_deref(), Some("Save changes to Sample: start circuit before closing?"));
        assert!(!f.close);
        // Other input is ignored while the question is open.
        s.handle(UiEvent::Key { key: KeyName::Undo });
        assert!(s.editor().is_dirty());

        s.handle(UiEvent::Confirm { choice: Choice::Cancel });
        let f = s.frame(None, [0, 0, 0]);
        assert!(f.prompt.is_none() && !f.close && f.dirty);

        s.handle(UiEvent::CloseRequested);
        s.handle(UiEvent::Confirm { choice: Choice::Save });
        assert!(s.frame(None, [0, 0, 0]).close);
        assert!(ecad_project::load(dir.path()).unwrap().device_by_tag("-K2").is_some());
    }

    #[test]
    fn discarding_on_close_leaves_the_folder_as_it_was() {
        let (dir, mut s) = session();
        place_coil(&mut s);
        s.handle(UiEvent::CloseRequested);
        s.handle(UiEvent::Confirm { choice: Choice::Discard });
        assert!(s.can_close() && s.frame(None, [0, 0, 0]).close);
        assert!(ecad_project::load(dir.path()).unwrap().device_by_tag("-K2").is_none());
    }

    #[test]
    fn new_creates_an_empty_project_and_open_switches_back() {
        let (first, mut s) = session();
        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("Panel A");
        s.handle(UiEvent::New { path: path.clone() });
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.title, "Panel A");
        assert_eq!(s.editor().project().functions.len(), 0);
        assert_eq!(f.palette.len(), ecad_model::library::iec().len(), "starter symbols are available");
        assert!(path.join("project.json").exists());

        // New into a folder that already has a project is refused.
        s.handle(UiEvent::New { path: first.path().to_owned() });
        assert!(s.frame(None, [0, 0, 0]).status.contains("already contains a project"));

        s.handle(UiEvent::Open { path: first.path().to_owned() });
        assert_eq!(s.editor().project().name, "Sample: start circuit");
        assert_eq!(s.dir(), first.path());
    }

    #[test]
    fn opening_with_unsaved_changes_asks_first() {
        let (_first, mut s) = session();
        let other = tempfile::tempdir().unwrap();
        s.handle(UiEvent::New { path: other.path().join("B") });
        place_coil(&mut s);
        s.handle(UiEvent::Open { path: other.path().join("missing") });
        assert!(s.frame(None, [0, 0, 0]).prompt.is_some());
        s.handle(UiEvent::Confirm { choice: Choice::Discard });
        let f = s.frame(None, [0, 0, 0]);
        assert!(f.status.contains("project.json"), "a failed open says why: {}", f.status);
        assert_eq!(s.editor().project().name, "B", "and keeps the current project open");
    }

    #[test]
    fn the_side_panel_shows_and_edits_the_selection() {
        let (_dir, mut s) = session();
        let (x, y) = s.view().to_screen(Point::new(0.2, 26.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        assert_eq!(
            s.frame(None, [0, 0, 0]).selection,
            Some(SelectionInfo::Wire {
                number: Some("W2".into()),
                from: "-S1:14".into(),
                to: "-K1:A1".into(),
                potential: None,
                warning: None,
                cross_section: String::new(),
                colour: String::new(),
                cable: String::new(),
                core: String::new(),
                bent: false,
            })
        );
        s.handle(UiEvent::TidyWires);
        assert_eq!(s.frame(None, [0, 0, 0]).status, "No wire needed moving");
        s.handle(UiEvent::NumberWire { number: "W1".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "wire number `W1` is already used");
        s.handle(UiEvent::NumberWire { number: "102".into() });
        assert!(
            matches!(s.frame(None, [0, 0, 0]).selection, Some(SelectionInfo::Wire { number: Some(n), .. }) if n == "102")
        );
    }

    #[test]
    fn pages_are_added_shown_renamed_moved_and_deleted() {
        let (_dir, mut s) = session();
        let first = s.frame(None, [0, 0, 0]);
        let page1 = first.page;
        s.handle(UiEvent::AddPage);
        let f = s.frame(Some(first.revision), [0, 0, 0]);
        assert_eq!(f.pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert_ne!(f.page, page1, "the new page is shown");
        assert!(f.base.is_some(), "switching pages redraws");
        assert_eq!(s.editor().project().functions_on(f.page).count(), 0);

        s.handle(UiEvent::RenamePage { name: "1".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "page `1` already exists");
        s.handle(UiEvent::RenamePage { name: "2a".into() });
        s.handle(UiEvent::DescribePage { text: "Motor".into() });
        s.handle(UiEvent::MovePage { delta: -1 });
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["2a", "1"]);
        assert_eq!(f.pages[0].description, "Motor");

        // Place something on page 2a, then delete the page: it all goes, and undo brings it back.
        place_coil(&mut s);
        s.handle(UiEvent::DeletePage);
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.pages.len(), 1);
        assert_eq!(f.page, page1);
        assert!(s.editor().project().device_by_tag("-K2").is_none());
        s.handle(UiEvent::Key { key: KeyName::Undo });
        assert_eq!(s.editor().project().pages.len(), 2);
        assert!(s.editor().project().device_by_tag("-K2").is_some());

        // Undo everything; then the last page can't be deleted.
        for _ in 0..5 {
            s.handle(UiEvent::Key { key: KeyName::Undo });
        }
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.pages.len(), 1);
        assert_eq!(f.page, page1, "undoing 'Add page' while it was shown went back to page 1");
        assert!(!f.dirty, "back where we started");
        s.handle(UiEvent::DeletePage);
        assert_eq!(s.frame(None, [0, 0, 0]).status, "A project needs at least one page");
    }

    #[test]
    fn switching_pages_does_not_count_as_a_change() {
        let (_dir, mut s) = session();
        s.handle(UiEvent::AddPage);
        s.handle(UiEvent::Save);
        let pages = s.frame(None, [0, 0, 0]).pages;
        s.handle(UiEvent::ShowPage { id: pages[0].id });
        s.handle(UiEvent::ShowPage { id: pages[1].id });
        assert!(!s.frame(None, [0, 0, 0]).dirty);
    }

    #[test]
    fn a_coil_lists_its_contacts_and_a_contact_its_coil() {
        let dir = tempfile::tempdir().unwrap();
        ecad_project::save(&ecad_model::sample::with_contacts().unwrap(), dir.path()).unwrap();
        let mut s = Session::open(dir.path()).unwrap();
        s.handle(UiEvent::Resize { width: 800.0, height: 600.0 });
        let click = |s: &mut Session, p: Point| {
            let (x, y) = s.view().to_screen(p);
            s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
            s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        };
        click(&mut s, Point::new(2.0, 12.0)); // the coil -K1
        let f = s.frame(None, [0, 0, 0]);
        let Some(SelectionInfo::Device { role, references, .. }) = f.selection else { panic!("coil selected") };
        assert_eq!(role, "main");
        let labels: Vec<&str> = references.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels.len(), 2);
        assert!(labels[0].starts_with("13-14 NO 2.") && labels[1].starts_with("21-22 NC 2."), "{labels:?}");
        assert_eq!(f.contact_palette.len(), 3, "NO, NC and 3-pole main contacts");

        // Add a third contact on page 2 from the side panel.
        s.handle(UiEvent::AddContact { symbol: "iec.contact_no".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).tool, "contact:iec.contact_no");
        s.handle(UiEvent::ShowPage { id: references[0].page });
        click(&mut s, Point::new(0.0, 60.0));
        let f = s.frame(None, [0, 0, 0]);
        let Some(SelectionInfo::Device { role, references, tag, .. }) = f.selection else { panic!("contact selected") };
        assert_eq!((role.as_str(), tag.as_str()), ("contact", "-K1"));
        assert!(references[0].label.starts_with("Coil at 1."), "{references:?}");
    }

    #[test]
    fn wires_show_their_potential_and_points_list_their_other_drawings() {
        let dir = tempfile::tempdir().unwrap();
        ecad_project::save(&ecad_model::sample::with_potentials().unwrap(), dir.path()).unwrap();
        let mut s = Session::open(dir.path()).unwrap();
        s.handle(UiEvent::Resize { width: 800.0, height: 600.0 });
        let click = |s: &mut Session, p: Point| {
            let (x, y) = s.view().to_screen(p);
            s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
            s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        };
        click(&mut s, Point::new(0.2, 56.0)); // W1, from L+
        let Some(SelectionInfo::Wire { potential, warning, .. }) = s.frame(None, [0, 0, 0]).selection else {
            panic!("wire selected")
        };
        assert_eq!((potential.as_deref(), warning), (Some("L+"), None));

        click(&mut s, Point::new(0.0, 64.0)); // the L+ point itself
        let Some(SelectionInfo::Device { role, references, .. }) = s.frame(None, [0, 0, 0]).selection else {
            panic!("point selected")
        };
        assert_eq!(role, "potential");
        assert_eq!(references.len(), 1, "L+ is also drawn on page 2");
        assert!(references[0].label.starts_with("Also at 2."), "{references:?}");
    }

    #[test]
    fn parts_are_imported_assigned_and_exported() {
        let (dir, mut s) = session();
        let csv = dir.path().join("vendor.csv");
        std::fs::write(&csv, "Part number;Manufacturer;Description\nEX-PB-GRN;Example;Pushbutton, green, 1 NO\nEX-LAMP;Example;Pilot lamp\n")
            .unwrap();
        s.handle(UiEvent::ImportParts { path: csv.clone() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "Imported 1 new parts, updated 0");
        assert_eq!(s.frame(None, [0, 0, 0]).parts.len(), 4);
        s.handle(UiEvent::ImportParts { path: csv });
        assert!(s.frame(None, [0, 0, 0]).status.starts_with("Nothing new"));

        // Select -K1 and give it two auxiliary blocks.
        let (x, y) = s.view().to_screen(Point::new(2.0, 12.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        s.handle(UiEvent::AddDevicePart { number: "EX-AUX-1NO1NC".into() });
        s.handle(UiEvent::AddDevicePart { number: "EX-AUX-1NO1NC".into() });
        let Some(SelectionInfo::Device { parts, .. }) = s.frame(None, [0, 0, 0]).selection else { panic!() };
        let summary: Vec<(String, u32)> = parts.iter().map(|p| (p.number.clone(), p.quantity)).collect();
        assert_eq!(summary, [("EX-CT-24DC".to_owned(), 1), ("EX-AUX-1NO1NC".to_owned(), 2)]);
        s.handle(UiEvent::SetDevicePartQuantity { number: "EX-AUX-1NO1NC".into(), quantity: 1 });
        s.handle(UiEvent::RemovePart { number: "EX-AUX-1NO1NC".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "part `EX-AUX-1NO1NC` is used by -K1");

        let out = dir.path().join("reports");
        s.handle(UiEvent::ExportReports { path: out.clone() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, format!("Wrote 3 reports to {}", out.display()));
        let bom = std::fs::read_to_string(out.join("bom.csv")).unwrap();
        assert!(bom.contains("EX-AUX-1NO1NC,Example,Auxiliary contact block 1 NO + 1 NC,1,-K1"), "{bom}");
    }

    #[test]
    fn drawings_are_exported_as_pdf_dxf_dwg_and_png() {
        let (dir, mut s) = session();
        let out = dir.path().join("drawings");
        s.handle(UiEvent::ExportDrawings { path: out.clone() });
        assert_eq!(
            s.frame(None, [0, 0, 0]).status,
            format!("Wrote drawings.pdf and 3 page files to {}", out.display())
        );
        for name in ["drawings.pdf", "page-1.dxf", "page-1.dwg", "page-1.png"] {
            assert!(std::fs::metadata(out.join(name)).is_ok_and(|m| m.len() > 1000), "{name}");
        }
    }

    #[test]
    fn terminals_are_numbered_per_strip_and_renumbered() {
        let (_dir, mut s) = session();
        let click = |s: &mut Session, p: Point| {
            let (x, y) = s.view().to_screen(p);
            s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
            s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        };
        // A new strip from the toolbar starts at terminal 1.
        s.handle(UiEvent::Tool { tool: "iec.terminal".into() });
        click(&mut s, Point::new(40.0, 40.0));
        let device = |s: &Session| match s.frame(None, [0, 0, 0]).selection {
            Some(SelectionInfo::Device { tag, terminal, references, role, .. }) => (tag, terminal, references, role),
            other => panic!("{other:?}"),
        };
        let (tag, terminal, _, role) = device(&s);
        assert_eq!((tag.as_str(), terminal.as_deref(), role.as_str()), ("-X1", Some("1"), "terminal"));
        // "Add terminal" from the side panel gives the next number.
        s.handle(UiEvent::AddContact { symbol: "iec.terminal".into() });
        click(&mut s, Point::new(60.0, 40.0));
        let (_, terminal, references, _) = device(&s);
        assert_eq!(terminal.as_deref(), Some("2"));
        assert!(references[0].label.starts_with("Terminal 1 at 1."), "{references:?}");
        // Numbers are unique on the strip.
        s.handle(UiEvent::RenumberTerminal { number: "1".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "-X1:1 already exists");
        s.handle(UiEvent::RenumberTerminal { number: "10".into() });
        assert_eq!(device(&s).1.as_deref(), Some("10"));
        assert_eq!(s.editor().undo_label(), Some("Renumber -X1:2 to 10"));
    }

    #[test]
    fn plc_channels_get_the_next_number_and_address_and_edit_their_properties() {
        let (_dir, mut s) = session();
        let click = |s: &mut Session, p: Point| {
            let (x, y) = s.view().to_screen(p);
            s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
            s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        };
        let plc = |s: &Session| match s.frame(None, [0, 0, 0]).selection {
            Some(SelectionInfo::Device { tag, role, plc: Some(plc), references, .. }) => (tag, role, plc, references),
            other => panic!("{other:?}"),
        };
        assert_eq!(s.frame(None, [0, 0, 0]).channel_palette.len(), 4);
        s.handle(UiEvent::Tool { tool: "iec.plc_di".into() });
        click(&mut s, Point::new(40.0, 40.0));
        let (tag, role, info, _) = plc(&s);
        assert_eq!((tag.as_str(), role.as_str()), ("-KF1", "plc"));
        assert_eq!((info.kind.as_str(), info.channel.as_str(), info.address.as_str()), ("DI", "0", "I0.0"));
        // "Add digital input" from the side panel: the card's next channel and address.
        s.handle(UiEvent::AddContact { symbol: "iec.plc_di".into() });
        click(&mut s, Point::new(80.0, 40.0));
        let (_, _, info, references) = plc(&s);
        assert_eq!((info.channel.as_str(), info.address.as_str()), ("1", "I0.1"));
        assert_eq!(s.editor().undo_label(), Some("Place -KF1:1"));
        assert!(references[0].label.starts_with("DI 0 I0.0 at 1."), "{references:?}");
        // Symbol and comment are set from the panel; an address in use is refused.
        s.handle(UiEvent::SetPlcProperty { key: "symbol".into(), value: " Door_Closed ".into() });
        assert_eq!(plc(&s).2.symbol, "Door_Closed");
        assert_eq!(s.editor().undo_label(), Some("Set symbol of -KF1:1"));
        s.handle(UiEvent::SetPlcProperty { key: "address".into(), value: "I0.0".into() });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "PLC address I0.0 is already used by -KF1:0");
        assert_eq!(plc(&s).2.address, "I0.1");
        s.handle(UiEvent::RenumberTerminal { number: "7".into() });
        assert_eq!(plc(&s).2.channel, "7");
    }

    #[test]
    fn checks_follow_every_change_and_lead_to_the_problem() {
        let (_dir, mut s) = session();
        let before = s.frame(None, [0, 0, 0]).checks.len();
        s.handle(UiEvent::Tool { tool: "iec.coil".into() });
        let (x, y) = s.view().to_screen(Point::new(120.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        s.handle(UiEvent::Key { key: KeyName::Escape });
        s.handle(UiEvent::PointerDown { x: 1.0, y: 1.0, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x: 1.0, y: 1.0, button: Button::Primary });
        let f = s.frame(None, [0, 0, 0]);
        let new: Vec<&str> = f.checks.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(f.checks.len(), before + 3, "{new:?}");
        let index = f.checks.iter().position(|c| c.message == "-K2:A1 is not connected").unwrap();
        assert_eq!(f.checks[index].severity, "warning");
        assert!(f.selection.is_none());
        s.handle(UiEvent::ShowCheck { index });
        let f = s.frame(None, [0, 0, 0]);
        assert!(matches!(f.selection, Some(SelectionInfo::Device { ref tag, .. }) if tag == "-K2"));
        assert!(f.status.ends_with("-K2:A1 is not connected"), "{}", f.status);
        s.handle(UiEvent::Key { key: KeyName::Undo });
        assert_eq!(s.frame(None, [0, 0, 0]).checks.len(), before);
    }

    #[test]
    fn project_settings_page_subtitles_and_the_sheet_grid_are_undoable() {
        let (_dir, mut s) = session();
        let mut info = s.frame(None, [0, 0, 0]).info;
        info.customer = "Example Foods".into();
        info.revisions.push(ecad_model::Revision {
            index: "0".into(),
            change: "First issue".into(),
            ..Default::default()
        });
        s.handle(UiEvent::ProjectInfo { name: "Panel A".into(), info: info.clone() });
        s.handle(UiEvent::SubtitlePage { text: "Circuito di avviamento".into() });
        s.handle(UiEvent::SheetGrid { columns: 8, first_column: 0, rows: 6 });
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!((f.project_name.as_str(), &f.info), ("Panel A", &info));
        assert_eq!(f.pages[0].subtitle, "Circuito di avviamento");
        assert_eq!((f.sheet.columns, f.sheet.first_column, f.sheet.rows), (8, 0, 6));
        assert_eq!(s.editor().undo_label(), Some("Sheet columns and rows"));
        s.handle(UiEvent::SheetGrid { columns: 8, first_column: 0, rows: 30 });
        assert!(s.frame(None, [0, 0, 0]).status.contains("up to 26 rows"));
        for _ in 0..3 {
            s.handle(UiEvent::Key { key: KeyName::Undo });
        }
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!((f.info.customer.as_str(), f.sheet.rows, f.pages[0].subtitle.as_str()), ("", 0, ""));
    }

    #[test]
    fn naming_by_page_tags_new_devices_and_numbers_wires_per_potential() {
        let (_dir, mut s) = session();
        s.handle(UiEvent::Naming { naming: ecad_model::Naming::by_page() });
        assert_eq!(s.frame(None, [0, 0, 0]).naming, ecad_model::Naming::by_page());
        s.handle(UiEvent::RenumberWires);
        assert_eq!(s.frame(None, [0, 0, 0]).status, "Renumbered 3 wires");
        let mut numbers: Vec<String> =
            s.editor().project().connections.values().filter_map(|c| c.wire_number.clone()).collect();
        numbers.sort();
        assert_eq!(numbers, ["101", "L+", "M"]);
        s.handle(UiEvent::Tool { tool: "iec.coil".into() });
        let (x, y) = s.view().to_screen(Point::new(120.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        assert!(
            matches!(s.frame(None, [0, 0, 0]).selection, Some(SelectionInfo::Device { ref tag, .. }) if tag == "1K1")
        );
        s.handle(UiEvent::Naming {
            naming: ecad_model::Naming { tag_format: "{page}".into(), ..ecad_model::Naming::by_page() },
        });
        assert!(s.frame(None, [0, 0, 0]).status.contains("needs one {n}"), "{}", s.frame(None, [0, 0, 0]).status);
    }

    #[test]
    fn a_black_box_is_made_placed_and_undone() {
        let (_dir, mut s) = session();
        s.handle(UiEvent::CreateBlackBox { spec: ecad_model::library::BlackBox::default() });
        assert!(s.frame(None, [0, 0, 0]).status.contains("at least one terminal"));
        let spec = ecad_model::library::BlackBox {
            description: "Safety module".into(),
            tag_prefix: "-KF".into(),
            top: vec!["A1".into(), "S11".into(), "S12".into()],
            bottom: vec!["A2".into(), "13".into(), "14".into()],
            ..Default::default()
        };
        s.handle(UiEvent::CreateBlackBox { spec });
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.tool, "box.1");
        assert!(f.palette.iter().any(|i| i.id == "box.1" && i.category == "Black boxes"));
        let (x, y) = s.view().to_screen(Point::new(120.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        assert!(
            matches!(s.frame(None, [0, 0, 0]).selection, Some(SelectionInfo::Device { ref tag, .. }) if tag == "-KF1")
        );
        // Undo the placing, then the symbol itself.
        s.handle(UiEvent::Key { key: KeyName::Undo });
        s.handle(UiEvent::Key { key: KeyName::Undo });
        assert!(!s.frame(None, [0, 0, 0]).palette.iter().any(|i| i.id == "box.1"));
    }

    #[test]
    fn wires_get_a_cross_section_colour_and_cable_core() {
        let (_dir, mut s) = session();
        s.handle(UiEvent::AddCable { tag: "-W1".into(), kind: "3G1,5".into(), cores: 3, shielded: false });
        assert_eq!(s.frame(None, [0, 0, 0]).cables.len(), 1);
        // Select W2 (-S1 to -K1) and put it in the cable.
        let (x, y) = s.view().to_screen(Point::new(0.0, 26.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        let spec = |cable: &str, core: &str| UiEvent::WireSpec {
            cross_section: "1,5".into(),
            colour: "bu".into(),
            cable: cable.into(),
            core: core.into(),
        };
        s.handle(spec("-W9", "1"));
        assert_eq!(s.frame(None, [0, 0, 0]).status, "there is no cable `-W9`");
        s.handle(spec("-W1", "1"));
        let f = s.frame(None, [0, 0, 0]);
        let Some(SelectionInfo::Wire { cross_section, colour, cable, core, .. }) = f.selection else { panic!("wire") };
        assert_eq!((cross_section.as_str(), colour.as_str(), cable.as_str(), core.as_str()), ("1,5", "BU", "-W1", "1"));
        assert_eq!(f.cables[0].used, 1);
        let id = f.cables[0].id;
        s.handle(UiEvent::RemoveCable { id });
        assert_eq!(s.frame(None, [0, 0, 0]).status, "cable -W1 still has wires in it");
        s.handle(UiEvent::UpdateCable { id, tag: "-W2".into(), kind: "3G1,5".into(), cores: 3, shielded: true });
        assert_eq!(s.frame(None, [0, 0, 0]).cables[0].tag, "-W2");
    }

    #[test]
    fn a_box_is_drawn_labelled_moved_and_deleted() {
        let (_dir, mut s) = session();
        s.handle(UiEvent::Tool { tool: "box".into() });
        let drag = |s: &mut Session, a: Point, b: Point| {
            let (x, y) = s.view().to_screen(a);
            s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
            let (x, y) = s.view().to_screen(b);
            s.handle(UiEvent::PointerMove { x, y });
            s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        };
        // Around -K1 (at 0, 12).
        drag(&mut s, Point::new(-12.0, 0.0), Point::new(12.0, 24.0));
        let f = s.frame(None, [0, 0, 0]);
        assert_eq!(f.tool, "select");
        let Some(SelectionInfo::Box { devices, .. }) = f.selection else { panic!("box selected") };
        assert_eq!(devices, ["-K1"]);
        s.handle(UiEvent::LabelBox { text: "+A1".into() });
        assert!(
            matches!(s.frame(None, [0, 0, 0]).selection, Some(SelectionInfo::Box { ref label, .. }) if label == "+A1")
        );
        // -K1 is in +A1 now.
        let k1 = s.editor().project().functions.values().find(|f| f.symbol.0 == "iec.coil").unwrap().id;
        assert_eq!(s.editor().project().box_labels(k1), ["+A1"]);
        // Drag its left edge 8 mm to the left: the box moves, one undo step.
        drag(&mut s, Point::new(-12.0, 8.0), Point::new(-20.0, 8.0));
        let b = s.editor().project().boxes.values().next().unwrap().clone();
        assert_eq!((b.min.x, b.max.x), (-20.0, 4.0));
        assert_eq!(s.editor().undo_label(), Some("Move box +A1"));
        s.handle(UiEvent::Key { key: KeyName::Delete });
        assert!(s.editor().project().boxes.is_empty());
    }

    #[test]
    fn events_round_trip_as_json() {
        let ev: UiEvent = serde_json::from_str(r#"{"type":"pointerDown","x":1,"y":2,"button":"primary"}"#).unwrap();
        assert_eq!(ev, UiEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Primary });
        let ev: UiEvent = serde_json::from_str(r#"{"type":"confirm","choice":"discard"}"#).unwrap();
        assert_eq!(ev, UiEvent::Confirm { choice: Choice::Discard });
        let (_dir, s) = session();
        let json = serde_json::to_value(s.frame(None, [0, 0, 0])).unwrap();
        assert!(json["base"].as_array().is_some_and(|b| !b.is_empty()));
        assert_eq!(json["tool"], "select");
    }
}
