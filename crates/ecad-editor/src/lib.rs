//! The editing controller, independent of any UI toolkit.
//!
//! A shell (egui, Tauri, tests) feeds it pointer events in page millimetres and key presses,
//! and draws what it returns: the page's base primitives from [`Editor::base`] (rebuilt when
//! [`Editor::revision`] changes) plus the overlays from [`Editor::overlays`]. All behaviour —
//! tools, hit-testing, snapping, undo — lives here, so it is tested without a window and any
//! shell gets the same editor.

pub use ecad_model::library::BlackBox;
pub use ecad_model::parts::format_size;
use ecad_model::{
    Command, Connection, ConnectionId, ConnectionRef, Device, DeviceId, Direction, Function, FunctionId, History,
    ModelError, Page, PageId, Point, Potential, Project, Rotation, SymbolId, route_between,
};
pub use ecad_model::{Naming, Part, PartRef, ProductId, ProjectInfo, Revision, WireLengthSettings, WireNumbering};
pub use ecad_view::{Primitive, Shape};
pub use session::{
    Button, CableInfo, Choice, DevicePart, Frame, KeyName, LayoutInfo, PageInfo, PaletteItem, SelectionInfo, Session,
    SheetGrid, UiEvent, UnplacedDevice,
};
pub use view::Viewport;

pub mod script;
pub mod session;
mod underlay;
pub use underlay::ratio;
pub mod view;

/// Default snapping grid (IEC 4 mm).
pub const GRID: f64 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    Select,
    /// Click to place a new device drawn with this symbol.
    Place(SymbolId),
    /// Click to place one more drawing of an existing device, e.g. a contact of a coil. It
    /// gets the device's next contact numbers; the tool returns to Select afterwards.
    PlaceFor {
        symbol: SymbolId,
        device: DeviceId,
    },
    /// Click a connection point, then another, to draw a wire.
    Wire,
    /// Drag a rectangle to draw a location box.
    Box,
    /// On a layout page: click to put this device's footprint on the plate.
    PlaceOnPanel(ecad_model::DeviceId),
    /// On a layout page: drag along the plate to draw a DIN rail.
    Rail,
    /// On a layout page: drag along the plate to draw a cable duct.
    Duct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selected {
    Function(FunctionId),
    Connection(ConnectionId),
    Box(ecad_model::BoxId),
    /// A rail, duct or device footprint on a layout page.
    PanelItem(ecad_model::PanelItemId),
}

/// What is under the pointer, in priority order: connection points, then symbols, then wires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Point(ConnectionRef),
    Function(FunctionId),
    Connection(ConnectionId),
    /// A location box, by its edge or label.
    Box(ecad_model::BoxId),
    /// A rail, duct or device footprint on a layout page.
    PanelItem(ecad_model::PanelItemId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Delete,
    /// Rotate the selected symbol a quarter turn counter-clockwise.
    Rotate,
    /// Flip the selected symbol left to right (again to undo the flip).
    Mirror,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Marker {
    /// A connection point with no wire yet.
    Open,
    /// The point a click would act on.
    Target,
    /// The start of the wire being drawn.
    Start,
}

/// Things the shell draws on top of the page.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Overlay {
    Marker {
        at: Point,
        kind: Marker,
    },
    /// A symbol's box: hovered, or selected when `selected`.
    Outline {
        min: Point,
        max: Point,
        selected: bool,
    },
    /// A wire highlighted: hovered, or selected when `selected`.
    Wire {
        points: Vec<Point>,
        selected: bool,
    },
    /// The wire being drawn.
    RubberBand {
        points: Vec<Point>,
    },
    /// Preview of the symbol about to be placed, as line segments.
    Ghost {
        lines: Vec<(Point, Point)>,
    },
    /// A wire on the same potential as the selected one.
    Potential {
        points: Vec<Point>,
    },
}

/// A wire whose middle run is being dragged.
#[derive(Clone, Copy, Debug)]
struct BendDrag {
    wire: ConnectionId,
    /// The bend before the drag, for undo.
    from: Option<f64>,
    /// Whether the middle run is horizontal (the drag moves its y) or vertical (its x).
    horizontal: bool,
    /// Where the pointer went down: a click that barely moves only selects the wire.
    start: Point,
    moved: bool,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    function: FunctionId,
    from: Point,
    grab: Point,
    moved: bool,
}

#[derive(Debug)]
pub struct Editor {
    project: Project,
    history: History,
    page: PageId,
    tool: Tool,
    selection: Option<Selected>,
    hover: Option<Hit>,
    drag: Option<Drag>,
    bend_drag: Option<BendDrag>,
    /// A panel item being moved: as it was, and where it was grabbed (plate mm from its corner).
    panel_drag: Option<(ecad_model::PanelItem, Point, bool)>,
    /// The start of a rail or duct being drawn, plate mm.
    panel_start: Option<Point>,
    /// A box being moved: the box as it was and where it was grabbed (from its corner).
    box_drag: Option<(ecad_model::LocationBox, Point, bool)>,
    /// The first corner of a box being drawn.
    box_start: Option<Point>,
    wire_start: Option<ConnectionRef>,
    cursor: Point,
    /// Snapping grid in mm.
    pub grid: f64,
    /// How close (mm) the pointer must be to hit something; shells set it from the zoom.
    pub hit_tolerance: f64,
    revision: u64,
    /// Potentials computed for `revision`; recomputed only after a change.
    potentials: std::cell::RefCell<Option<(u64, Vec<Potential>)>>,
    /// Checks run for `revision`; rerun only after a change.
    checks: std::cell::RefCell<Option<(u64, Vec<ecad_rules::Check>)>>,
    /// Undo depth at the last save; `None` once that state can't be reached by undo/redo.
    saved_at: Option<usize>,
    /// Increases on every model change and page switch: what the drawing depends on.
    scene: u64,
    status: String,
}

fn snap(p: Point, grid: f64) -> Point {
    if grid <= 0.0 { p } else { Point::new((p.x / grid).round() * grid, (p.y / grid).round() * grid) }
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.x + t * dx, a.y + t * dy);
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

fn opposite(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    }
}

fn next_rotation(r: Rotation) -> Rotation {
    match r {
        Rotation::R0 => Rotation::R90,
        Rotation::R90 => Rotation::R180,
        Rotation::R180 => Rotation::R270,
        Rotation::R270 => Rotation::R0,
    }
}

impl Editor {
    /// Edit `page` of `project`.
    pub fn new(project: Project, page: PageId) -> Result<Self, ModelError> {
        project.page(page)?;
        Ok(Editor {
            project,
            history: History::new(),
            page,
            tool: Tool::Select,
            selection: None,
            hover: None,
            drag: None,
            bend_drag: None,
            panel_drag: None,
            panel_start: None,
            box_drag: None,
            box_start: None,
            wire_start: None,
            cursor: Point::ZERO,
            grid: GRID,
            hit_tolerance: 1.5,
            revision: 0,
            saved_at: Some(0),
            potentials: std::cell::RefCell::new(None),
            checks: std::cell::RefCell::new(None),
            scene: 0,
            status: String::new(),
        })
    }

    // ---- state for the shell ----

    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn page(&self) -> PageId {
        self.page
    }
    pub fn tool(&self) -> &Tool {
        &self.tool
    }
    pub fn selection(&self) -> Option<Selected> {
        self.selection
    }
    pub fn hover(&self) -> Option<&Hit> {
        self.hover.as_ref()
    }
    /// Increases on every model change; rebuild [`Editor::base`] when it moves.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Increases whenever [`Editor::base`] would draw something different (model changes and
    /// page switches); the UI caches the base by it.
    pub fn scene_revision(&self) -> u64 {
        self.scene
    }
    /// Changes since the last [`Editor::mark_saved`].
    /// True while the project differs from what was last saved: undoing back to the saved
    /// state makes it clean again.
    pub fn is_dirty(&self) -> bool {
        self.saved_at != Some(self.history.depth()) || self.drag.is_some_and(|d| d.moved)
    }
    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.history.depth());
    }
    /// The last message for the user (an error, or what undo/redo did).
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    /// The page drawn as plain primitives, for the given background colour.
    pub fn base(&self, background: [u8; 3]) -> Result<Vec<Primitive>, ecad_view::ViewError> {
        ecad_view::page_primitives(&self.project, self.page, background)
    }

    /// What zoom to fit shows: the whole sheet.
    pub fn extents(&self) -> Option<(Point, Point)> {
        Some(self.project.sheet.bounds())
    }

    // ---- parts ----

    pub fn add_part(&mut self, part: Part) -> Result<(), ModelError> {
        let label = format!("Add part {}", part.number.trim());
        self.execute(&label, Command::AddPart(part))
    }

    pub fn update_part(&mut self, part: Part) -> Result<(), ModelError> {
        let label = format!("Edit part {}", part.number);
        self.execute(&label, Command::UpdatePart(part))
    }

    pub fn remove_part(&mut self, product: ProductId) -> Result<(), ModelError> {
        let name = self.project.parts.get(&product).map_or_else(String::new, Part::label);
        self.execute(&format!("Remove part {name}"), Command::RemovePart(product))
    }

    /// Add new parts and update existing ones in one undoable step. An incoming part is the
    /// catalogue's product with the same manufacturer and number (keeping that product's id);
    /// one without a manufacturer is the only product with its number, if there is one. Returns
    /// how many were added and how many changed.
    pub fn import_parts(&mut self, parts: Vec<Part>) -> Result<(usize, usize), ModelError> {
        let (mut added, mut updated) = (0, 0);
        let mut cmds = Vec::new();
        for mut part in parts {
            let same = self.project.parts.values().find(|p| p.key() == part.key()).or_else(|| {
                match self.project.parts_numbered(&part.number).as_slice() {
                    [one] if part.manufacturer.trim().is_empty() => Some(*one),
                    _ => None,
                }
            });
            if let Some(existing) = same {
                part.id = existing.id;
                if part.manufacturer.trim().is_empty() {
                    part.manufacturer = existing.manufacturer.clone();
                }
            }
            // A catalogue without contact blocks or sizes doesn't wipe the ones already entered.
            if let Some(existing) = self.project.parts.get(&part.id) {
                if part.contacts.is_empty() {
                    part.contacts = existing.contacts.clone();
                }
                if part.width.is_none() && part.height.is_none() {
                    (part.width, part.height, part.depth) = (existing.width, existing.height, existing.depth);
                }
            }
            match self.project.parts.get(&part.id) {
                None => {
                    added += 1;
                    cmds.push(Command::AddPart(part));
                }
                Some(existing) if *existing != part => {
                    updated += 1;
                    cmds.push(Command::UpdatePart(part));
                }
                Some(_) => {}
            }
        }
        if !cmds.is_empty() {
            self.execute(&format!("Import {} parts", added + updated), Command::Batch(cmds))?;
        }
        Ok((added, updated))
    }

    /// Set the parts of the selected symbol's device.
    pub fn set_selected_parts(&mut self, parts: Vec<PartRef>) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let device = self.project.function(f)?.device;
        let label = format!("Parts of {}", self.tag_of(f));
        self.execute(&label, Command::SetDeviceParts { id: device, parts })
    }

    // ---- pages ----

    /// Show another page. Selection and any wire being drawn are dropped.
    pub fn set_page(&mut self, page: PageId) -> Result<(), ModelError> {
        self.project.page(page)?;
        if page != self.page {
            self.page = page;
            self.selection = None;
            self.hover = None;
            self.drag = None;
            self.wire_start = None;
            self.panel_drag = None;
            self.panel_start = None;
            if matches!(self.tool, Tool::Rail | Tool::Duct | Tool::PlaceOnPanel(_))
                != self.project.panels.contains_key(&page)
            {
                self.tool = Tool::Select;
            }
            self.scene += 1;
        }
        Ok(())
    }

    /// Add a page after the current one, named with the next free number, and show it.
    pub fn add_page(&mut self) -> Result<(), ModelError> {
        let order: Vec<PageId> = self.project.pages_in_order().iter().map(|p| p.id).collect();
        let at = order.iter().position(|p| *p == self.page).map_or(order.len(), |i| i + 1);
        let highest = self.project.pages.values().filter_map(|p| p.name.parse::<u32>().ok()).max().unwrap_or(0);
        let name = (highest + 1..).map(|n| n.to_string()).find(|n| !self.project.pages.values().any(|p| &p.name == n));
        let page = Page::new(&name.unwrap_or_default(), 0);
        let id = page.id;
        let mut new_order = order;
        new_order.insert(at, id);
        let label = format!("Add page {}", page.name);
        self.execute(&label, Command::Batch(vec![Command::AddPage(page), Command::ReorderPages(new_order)]))?;
        self.set_page(id)
    }

    /// Delete the current page with everything on it (undoable). The last page can't go.
    pub fn delete_page(&mut self) -> Result<(), ModelError> {
        let order: Vec<PageId> = self.project.pages_in_order().iter().map(|p| p.id).collect();
        if order.len() <= 1 {
            self.status = "A project needs at least one page".into();
            return Ok(());
        }
        let page = self.page;
        let name = self.project.page(page)?.name.clone();
        let mut cmds = Vec::new();
        let on_page: Vec<&Function> = self.project.functions_on(page).collect();
        for f in &on_page {
            cmds.push(Command::Unplace(f.id));
        }
        for b in self.project.boxes_on(page) {
            cmds.push(Command::RemoveBox(b.id));
        }
        // Devices drawn only on this page go with it.
        let mut devices: Vec<DeviceId> = on_page.iter().map(|f| f.device).collect();
        devices.sort();
        devices.dedup();
        let going: Vec<DeviceId> = devices
            .into_iter()
            .filter(|d| self.project.functions.values().all(|f| f.device != *d || f.page == page))
            .collect();
        // The page's plate and items, and the footprints of the devices that go with it.
        cmds.extend(self.clear_panel_commands(page, &going));
        cmds.extend(going.into_iter().map(Command::RemoveDevice));
        if self.project.underlays.contains_key(&page) {
            cmds.push(Command::SetUnderlay { page, underlay: None });
        }
        cmds.push(Command::RemovePage(page));
        let index = order.iter().position(|p| *p == page).unwrap_or(0);
        let next = if index > 0 { order[index - 1] } else { order[1] };
        self.execute(&format!("Delete page {name}"), Command::Batch(cmds))?;
        self.set_page(next)
    }

    pub fn rename_page(&mut self, name: &str) -> Result<(), ModelError> {
        let old = self.project.page(self.page)?.name.clone();
        if name.trim() == old {
            return Ok(());
        }
        let label = format!("Rename page {old} to {}", name.trim());
        self.execute(&label, Command::RenamePage { id: self.page, name: name.to_owned() })
    }

    pub fn describe_page(&mut self, description: &str) -> Result<(), ModelError> {
        let name = self.project.page(self.page)?.name.clone();
        self.execute(
            &format!("Describe page {name}"),
            Command::DescribePage { id: self.page, description: description.to_owned() },
        )
    }

    /// Set the current page title's second line.
    pub fn subtitle_page(&mut self, subtitle: &str) -> Result<(), ModelError> {
        let name = self.project.page(self.page)?.name.clone();
        self.execute(
            &format!("Page {name} subtitle"),
            Command::SetPageSubtitle { id: self.page, subtitle: subtitle.to_owned() },
        )
    }

    /// Set the project name and title-block fields.
    pub fn set_project_info(&mut self, name: &str, info: ecad_model::ProjectInfo) -> Result<(), ModelError> {
        if name.trim() == self.project.name && info == self.project.info {
            return Ok(());
        }
        self.execute("Project properties", Command::SetProjectInfo { name: name.to_owned(), info })
    }

    /// Change the sheet's columns and rows; the size and placement stay.
    pub fn set_sheet_grid(&mut self, columns: u32, first_column: u32, rows: u32) -> Result<(), ModelError> {
        let sheet = ecad_model::Sheet { columns, first_column, rows, ..self.project.sheet };
        if sheet == self.project.sheet {
            return Ok(());
        }
        let r = self.execute("Sheet columns and rows", Command::SetSheet(sheet));
        // Every page's frame changes.
        self.scene += 1;
        r
    }

    /// Move the current page `delta` places (negative: earlier).
    pub fn move_page(&mut self, delta: i32) -> Result<(), ModelError> {
        let mut order: Vec<PageId> = self.project.pages_in_order().iter().map(|p| p.id).collect();
        let Some(from) = order.iter().position(|p| *p == self.page) else { return Ok(()) };
        let to = (from as i64 + i64::from(delta)).clamp(0, order.len() as i64 - 1) as usize;
        if to == from {
            return Ok(());
        }
        let id = order.remove(from);
        order.insert(to, id);
        let name = self.project.page(self.page)?.name.clone();
        self.execute(&format!("Move page {name}"), Command::ReorderPages(order))
    }

    // ---- commands from the shell ----

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.wire_start = None;
        self.drag = None;
        self.status.clear();
    }

    pub fn select(&mut self, s: Option<Selected>) {
        self.selection = s;
    }

    /// Rename the selected symbol's device.
    pub fn rename_selected(&mut self, tag: &str) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let device = self.project.function(f)?.device;
        let old = self.project.device(device)?.tag.clone();
        let label = format!("Rename {old} to {tag}");
        self.execute(&label, Command::RenameDevice { id: device, tag: tag.to_owned() })
    }

    /// Set the selected symbol's device description.
    pub fn describe_selected(&mut self, description: &str) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let device = self.project.function(f)?.device;
        let label = format!("Describe {}", self.tag_of(f));
        self.execute(&label, Command::SetDescription { id: device, description: description.trim().to_owned() })
    }

    /// Set (or, when blank, clear) the selected wire's number.
    pub fn number_selected_wire(&mut self, number: &str) -> Result<(), ModelError> {
        let Some(Selected::Connection(c)) = self.selection else { return Ok(()) };
        let old = self.wire_name(c);
        let label = format!("Renumber wire {old} to {}", number.trim());
        // Per potential, every wire of the potential changes with it.
        let cmds = self.project.set_wire_number_commands(c, Some(number.to_owned()));
        self.execute(&label, Command::Batch(cmds))
    }

    /// Add a cable; its tag must be new.
    pub fn add_cable(&mut self, cable: ecad_model::Cable) -> Result<(), ModelError> {
        let label = format!("Add cable {}", cable.tag.trim());
        self.execute(&label, Command::AddCable(cable))
    }

    /// Change a cable's tag, type, cores or shield.
    pub fn update_cable(&mut self, cable: ecad_model::Cable) -> Result<(), ModelError> {
        if self.project.cables.get(&cable.id) == Some(&cable) {
            return Ok(());
        }
        let label = format!("Edit cable {}", cable.tag.trim());
        self.execute(&label, Command::UpdateCable(cable))
    }

    /// Remove a cable no wire runs in.
    pub fn remove_cable(&mut self, id: ecad_model::CableId) -> Result<(), ModelError> {
        let tag = self.project.cables.get(&id).map(|c| c.tag.clone()).unwrap_or_default();
        self.execute(&format!("Remove cable {tag}"), Command::RemoveCable(id))
    }

    /// Set the selected wire's cross-section and colour, and put it in `cable` (by tag; empty
    /// for none) as core `core`.
    pub fn set_selected_wire_spec(
        &mut self,
        cross_section: &str,
        colour: &str,
        cable: &str,
        core: &str,
    ) -> Result<(), ModelError> {
        let Some(Selected::Connection(c)) = self.selection else { return Ok(()) };
        let cable = cable.trim();
        let core = if cable.is_empty() {
            None
        } else {
            let id = self
                .project
                .cables
                .values()
                .find(|k| k.tag == cable)
                .map(|k| k.id)
                .ok_or_else(|| ModelError::UnknownCable(cable.to_owned()))?;
            Some(ecad_model::CableCore { cable: id, core: core.to_owned() })
        };
        let spec =
            ecad_model::WireSpec { cross_section: cross_section.to_owned(), colour: colour.to_owned(), cable: core };
        let current = &self.project.connection(c)?.spec;
        if current.cross_section == spec.cross_section.trim()
            && current.colour == spec.colour.trim().to_uppercase()
            && current.cable.as_ref().map(|k| (k.cable, k.core.as_str()))
                == spec.cable.as_ref().map(|k| (k.cable, k.core.trim()))
        {
            return Ok(());
        }
        let label = format!("Wire {} details", self.wire_name(c));
        self.execute(&label, Command::SetWireSpec { id: c, spec })
    }

    /// Make a black-box symbol from `spec` and pick it for placing.
    pub fn create_black_box(&mut self, spec: &ecad_model::library::BlackBox) -> Result<(), ModelError> {
        if !spec.is_valid() {
            return Err(ModelError::InvalidBlackBox);
        }
        let id = (1..)
            .map(|n| SymbolId::new(&format!("box.{n}")))
            .find(|id| !self.project.symbols.contains_key(id))
            .unwrap_or_else(|| SymbolId::new("box.new"));
        let symbol = ecad_model::library::black_box(&id.0, spec);
        self.execute(&format!("New black box {}", symbol.description), Command::AddSymbol(symbol))?;
        self.set_tool(Tool::Place(id));
        self.status = "Click where the black box goes".into();
        Ok(())
    }

    /// Set how new tags and wire numbers are formed.
    pub fn set_naming(&mut self, naming: ecad_model::Naming) -> Result<(), ModelError> {
        if naming == self.project.naming {
            return Ok(());
        }
        self.execute("Naming", Command::SetNaming(naming))
    }

    /// Take PLC symbols and comments from a TIA Portal tag table, matched by address, as one
    /// undoable step.
    pub fn import_tia_tags(
        &mut self,
        tags: &[ecad_exchange::tia::Tag],
    ) -> Result<ecad_exchange::tia::Import, ecad_exchange::ExchangeError> {
        let mut import = ecad_exchange::tia::import(&self.project, tags)?;
        if let Some(command) = import.command.take() {
            self.execute("Import TIA Portal tags", command)?;
        }
        Ok(import)
    }

    /// Set how wire lengths are estimated from the panel layout.
    pub fn set_wire_lengths(&mut self, settings: ecad_model::WireLengthSettings) -> Result<(), ModelError> {
        if settings == self.project.wire_length_settings {
            return Ok(());
        }
        self.execute("Wire lengths", Command::SetWireLengthSettings(settings))
    }

    /// Number every wire again from the project's wire format, page by page.
    pub fn renumber_all_wires(&mut self) -> Result<(), ModelError> {
        let cmds = self.project.renumber_all_wires();
        self.execute("Renumber all wires", Command::Batch(cmds))
    }

    /// Moves every wire on the page that runs on another wire or through a symbol to a clear
    /// route where one is near; returns how many moved.
    pub fn tidy_wires(&mut self) -> Result<usize, ModelError> {
        let cmds = self.project.tidy_wires(self.page);
        let n = cmds.len();
        if n > 0 {
            self.execute("Tidy wires", Command::Batch(cmds))?;
        }
        Ok(n)
    }

    /// Puts the selected wire's middle run back halfway.
    pub fn straighten_selected_wire(&mut self) -> Result<(), ModelError> {
        let Some(Selected::Connection(c)) = self.selection else { return Ok(()) };
        if self.project.connection(c)?.bend.is_none() {
            return Ok(());
        }
        let name = self.wire_name(c);
        self.execute(&format!("Straighten wire {name}"), Command::SetWireBend { id: c, bend: None })
    }

    /// The middle run of a wire's route, the part a bend moves (`None` for a straight wire).
    fn middle_run(&self, c: ConnectionId) -> Option<ecad_model::project::MiddleRun> {
        let wire = self.project.connection(c).ok()?;
        let (a, da) = self.project.connection_point(&wire.from).ok()?;
        let (b, db) = self.project.connection_point(&wire.to).ok()?;
        ecad_model::project::middle_run(a, da, b, db)
    }

    pub fn pointer_move(&mut self, p: Point) {
        self.cursor = p;
        if self.panel_pointer_move(p) {
            return;
        }
        if let Some((from, grab, _)) = self.box_drag.clone() {
            let min = snap(Point::new(p.x - grab.x, p.y - grab.y), self.grid);
            let (dx, dy) = (min.x - from.min.x, min.y - from.min.y);
            let moved =
                ecad_model::LocationBox { min, max: Point::new(from.max.x + dx, from.max.y + dy), ..from.clone() };
            let now = self.project.boxes.get(&from.id).map(|b| b.min);
            if now != Some(min) && self.project.apply(Command::UpdateBox(moved)).is_ok() {
                self.box_drag = Some((from, grab, true));
                self.revision += 1;
                self.scene += 1;
            }
            return;
        }
        if let Some(mut bend) = self.bend_drag {
            let travel = if bend.horizontal { p.y - bend.start.y } else { p.x - bend.start.x };
            if !bend.moved && travel.abs() < self.grid.max(1.0) / 2.0 {
                return;
            }
            let at = snap(p, self.grid);
            let to = Some(if bend.horizontal { at.y } else { at.x });
            let now = self.project.connection(bend.wire).map(|c| c.bend).ok();
            if now != Some(to) && self.project.apply(Command::SetWireBend { id: bend.wire, bend: to }).is_ok() {
                bend.moved = true;
                self.bend_drag = Some(bend);
                self.revision += 1;
                self.scene += 1;
            }
            return;
        }
        if let Some(mut drag) = self.drag {
            let to = snap(Point::new(p.x - drag.grab.x, p.y - drag.grab.y), self.grid);
            let at = self.project.function(drag.function).map(|f| f.position).ok();
            if at.is_some_and(|at| at != to) && self.project.apply(Command::Move { id: drag.function, to }).is_ok() {
                drag.moved = true;
                self.drag = Some(drag);
                self.revision += 1;
                self.scene += 1;
            }
            return;
        }
        self.hover = self.hit(p);
    }

    pub fn pointer_down(&mut self, p: Point) {
        self.cursor = p;
        self.status.clear();
        let hit = self.hit(p);
        if self.panel_pointer_down(p, &hit) {
            return;
        }
        match self.tool.clone() {
            Tool::Select => match hit {
                Some(Hit::Function(f)) | Some(Hit::Point(ConnectionRef { function: f, .. })) => {
                    self.selection = Some(Selected::Function(f));
                    if let Ok(func) = self.project.function(f) {
                        let grab = Point::new(p.x - func.position.x, p.y - func.position.y);
                        self.drag = Some(Drag { function: f, from: func.position, grab, moved: false });
                    }
                }
                Some(Hit::Connection(c)) => {
                    self.selection = Some(Selected::Connection(c));
                    // Dragging a wire moves its middle run, if it has one.
                    if let Some(run) = self.middle_run(c)
                        && let Ok(wire) = self.project.connection(c)
                    {
                        self.bend_drag = Some(BendDrag {
                            wire: c,
                            from: wire.bend,
                            horizontal: run.horizontal,
                            start: p,
                            moved: false,
                        });
                    }
                }
                Some(Hit::Box(b)) => {
                    self.selection = Some(Selected::Box(b));
                    if let Some(from) = self.project.boxes.get(&b) {
                        let grab = Point::new(p.x - from.min.x, p.y - from.min.y);
                        self.box_drag = Some((from.clone(), grab, false));
                    }
                }
                // Handled by the layout editor above.
                Some(Hit::PanelItem(_)) => {}
                None => self.selection = None,
            },
            Tool::Box => self.box_start = Some(snap(p, self.grid)),
            Tool::Place(symbol) => self.place(&symbol, snap(p, self.grid)),
            Tool::PlaceFor { symbol, device } => self.place_for(&symbol, device, snap(p, self.grid)),
            Tool::Wire => self.wire_click(hit),
            // Only on layout pages, handled above.
            Tool::PlaceOnPanel(_) | Tool::Rail | Tool::Duct => {}
        }
    }

    pub fn pointer_up(&mut self, p: Point) {
        self.cursor = p;
        self.panel_pointer_up(p);
        if let Some((from, _, moved)) = self.box_drag.take()
            && moved
        {
            self.forget_saved_branch();
            let label = if from.label.is_empty() { "Move box".to_owned() } else { format!("Move box {}", from.label) };
            self.history.record(&label, Command::UpdateBox(from));
        }
        if let Some(start) = self.box_start.take() {
            let end = snap(p, self.grid);
            let id = ecad_model::BoxId::new();
            let b = ecad_model::LocationBox { id, page: self.page, min: start, max: end, label: String::new() };
            match self.execute("Draw box", Command::AddBox(b)) {
                Ok(()) => {
                    self.selection = Some(Selected::Box(id));
                    self.tool = Tool::Select;
                    self.status = "Type the box's label in the side panel, e.g. +A101 or EXT".into();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
        if let Some(bend) = self.bend_drag.take()
            && bend.moved
        {
            let name = self.wire_name(bend.wire);
            self.forget_saved_branch();
            self.history.record(&format!("Move wire {name}"), Command::SetWireBend { id: bend.wire, bend: bend.from });
        }
        if let Some(drag) = self.drag.take()
            && drag.moved
        {
            let tag = self.tag_of(drag.function);
            self.forget_saved_branch();
            self.history.record(&format!("Move {tag}"), Command::Move { id: drag.function, to: drag.from });
        }
        self.hover = self.hit(p);
    }

    pub fn key(&mut self, key: Key) {
        self.status.clear();
        match key {
            Key::Escape => {
                if self.wire_start.take().is_none() && self.panel_start.take().is_none() {
                    self.selection = None;
                    self.tool = Tool::Select;
                }
            }
            Key::Delete => match self.selection.take() {
                Some(Selected::Function(f)) => {
                    let r = self.delete_function(f);
                    self.report(r);
                }
                Some(Selected::Connection(c)) => {
                    let r = self.delete_wire(c);
                    self.report(r);
                }
                Some(Selected::Box(b)) => {
                    let r = self.execute("Delete box", Command::RemoveBox(b));
                    self.report(r);
                }
                Some(Selected::PanelItem(id)) => self.delete_panel_item(id),
                None => {}
            },
            Key::Rotate => {
                if let Some(Selected::PanelItem(id)) = self.selection {
                    self.turn_panel_item(id);
                }
                if let Some(Selected::Function(f)) = self.selection
                    && let Ok(func) = self.project.function(f)
                {
                    let to = next_rotation(func.rotation);
                    let label = format!("Rotate {}", self.tag_of(f));
                    let r = self.execute(&label, Command::Rotate { id: f, to });
                    self.report(r);
                }
            }
            Key::Mirror => {
                if let Some(Selected::Function(f)) = self.selection
                    && let Ok(func) = self.project.function(f)
                {
                    let to = !func.mirrored;
                    let label = format!("{} {}", if to { "Mirror" } else { "Unmirror" }, self.tag_of(f));
                    let r = self.execute(&label, Command::Mirror { id: f, to });
                    self.report(r);
                }
            }
            Key::Undo => match self.undo() {
                Ok(Some(label)) => self.status = format!("Undid: {label}"),
                Ok(None) => self.status = "Nothing to undo".into(),
                Err(e) => self.status = e.to_string(),
            },
            Key::Redo => match self.redo() {
                Ok(Some(label)) => self.status = format!("Redid: {label}"),
                Ok(None) => self.status = "Nothing to redo".into(),
                Err(e) => self.status = e.to_string(),
            },
        }
        self.forget_missing();
    }

    /// Undo the last change; its label, or `None` if there was nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>, ModelError> {
        let label = self.history.undo(&mut self.project)?;
        if let Some(l) = &label {
            self.changed(format!("Undid: {l}"));
        }
        self.forget_missing();
        Ok(label)
    }

    /// Redo the last undone change; its label, or `None` if there was nothing to redo.
    pub fn redo(&mut self) -> Result<Option<String>, ModelError> {
        let label = self.history.redo(&mut self.project)?;
        if let Some(l) = &label {
            self.changed(format!("Redid: {l}"));
        }
        self.forget_missing();
        Ok(label)
    }

    /// Delete a drawing, and its device when it was the device's only drawing (with its
    /// footprint on a layout), undoably.
    pub(crate) fn delete_function(&mut self, f: FunctionId) -> Result<(), ModelError> {
        let label = format!("Delete {}", self.tag_of(f));
        let device = self.project.function(f)?.device;
        let mut cmds = vec![Command::Unplace(f)];
        if self.project.functions.values().filter(|x| x.device == device).count() == 1 {
            if let Some(item) = self.project.panel_item_of(device) {
                cmds.push(Command::RemovePanelItem(item.id));
            }
            cmds.push(Command::RemoveDevice(device));
        }
        self.execute(&label, Command::Batch(cmds))
    }

    /// Delete a wire, undoably.
    pub(crate) fn delete_wire(&mut self, c: ConnectionId) -> Result<(), ModelError> {
        let label = format!("Delete wire {}", self.wire_name(c));
        self.execute(&label, Command::Disconnect(c))
    }

    /// The project's potentials, computed once per change.
    pub fn with_potentials<R>(&self, f: impl FnOnce(&[Potential]) -> R) -> R {
        let mut cache = self.potentials.borrow_mut();
        if cache.as_ref().is_none_or(|(rev, _)| *rev != self.revision) {
            *cache = Some((self.revision, self.project.potentials()));
        }
        f(cache.as_ref().map_or(&[], |(_, p)| p.as_slice()))
    }

    /// The project's checks, run once per change.
    pub fn checks(&self) -> Vec<ecad_rules::Check> {
        let mut cache = self.checks.borrow_mut();
        if cache.as_ref().is_none_or(|(rev, _)| *rev != self.revision) {
            *cache = Some((self.revision, ecad_rules::check(&self.project)));
        }
        cache.as_ref().map(|(_, c)| c.clone()).unwrap_or_default()
    }

    /// Go to a check's page and select what it points at.
    pub fn show_check(&mut self, check: &ecad_rules::Check) -> Result<(), ModelError> {
        self.set_page(check.page)?;
        self.select(match check.target {
            ecad_rules::Target::Function(f) => Some(Selected::Function(f)),
            ecad_rules::Target::Connection(c) => Some(Selected::Connection(c)),
            // Layout items can't be selected yet (M5b): the page is shown.
            ecad_rules::Target::PanelItem(_) => None,
        });
        Ok(())
    }

    /// The potential of a wire.
    pub fn potential_of(&self, c: ConnectionId) -> Option<Potential> {
        self.with_potentials(|all| all.iter().find(|p| p.connections.contains(&c)).cloned())
    }

    // ---- what to draw on top of the page ----

    pub fn overlays(&self) -> Vec<Overlay> {
        let mut out = Vec::new();
        // Open connection points, so the user sees where wires can go.
        if matches!(self.tool, Tool::Wire) {
            for f in self.project.functions_on(self.page) {
                let Ok(sym) = self.project.symbol(&f.symbol) else { continue };
                for cp in &sym.connection_points {
                    let r = ConnectionRef::new(f.id, &cp.name);
                    if self.project.connection_at(&r).is_none() {
                        out.push(Overlay::Marker { at: f.to_page(cp.at), kind: Marker::Open });
                    }
                }
            }
        }
        match &self.hover {
            Some(Hit::Point(r)) if matches!(self.tool, Tool::Wire) => {
                if let Ok((at, _)) = self.project.connection_point(r) {
                    out.push(Overlay::Marker { at, kind: Marker::Target });
                }
            }
            Some(Hit::Function(f)) if matches!(self.tool, Tool::Select) => {
                if let Some((min, max)) = self.function_box(*f) {
                    out.push(Overlay::Outline { min, max, selected: false });
                }
            }
            Some(Hit::Connection(c)) if matches!(self.tool, Tool::Select) => {
                if let Some(points) = self.wire_points(*c) {
                    out.push(Overlay::Wire { points, selected: false });
                }
            }
            Some(Hit::Box(b)) if matches!(self.tool, Tool::Select) => {
                if let Some(b) = self.project.boxes.get(b) {
                    out.push(Overlay::Outline { min: b.min, max: b.max, selected: false });
                }
            }
            _ => {}
        }
        // The box being drawn.
        if let Some(a) = self.box_start {
            let b = snap(self.cursor, self.grid);
            let points = vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y), a];
            out.push(Overlay::RubberBand { points });
        }
        self.panel_overlays(&mut out);
        match self.selection {
            Some(Selected::Function(f)) => {
                if let Some((min, max)) = self.function_box(f) {
                    out.push(Overlay::Outline { min, max, selected: true });
                }
            }
            Some(Selected::Connection(c)) => {
                // The rest of its potential on this page, under the selected wire.
                if let Some(pot) = self.potential_of(c) {
                    for other in pot.connections.iter().filter(|o| **o != c) {
                        let on_page = self
                            .project
                            .connection(*other)
                            .is_ok_and(|w| self.project.function(w.from.function).is_ok_and(|f| f.page == self.page));
                        if on_page && let Some(points) = self.wire_points(*other) {
                            out.push(Overlay::Potential { points });
                        }
                    }
                }
                if let Some(points) = self.wire_points(c) {
                    out.push(Overlay::Wire { points, selected: true });
                }
            }
            Some(Selected::Box(b)) => {
                if let Some(b) = self.project.boxes.get(&b) {
                    out.push(Overlay::Outline { min: b.min, max: b.max, selected: true });
                }
            }
            // Drawn by panel_overlays.
            Some(Selected::PanelItem(_)) | None => {}
        }
        if let Some(start) = &self.wire_start
            && let Ok((a, da)) = self.project.connection_point(start)
        {
            out.push(Overlay::Marker { at: a, kind: Marker::Start });
            let target = match &self.hover {
                Some(Hit::Point(r)) if r != start => self.project.connection_point(r).ok(),
                _ => None,
            };
            let (b, db) = target.unwrap_or((snap(self.cursor, self.grid), opposite(da)));
            out.push(Overlay::RubberBand { points: route_between(a, da, b, db) });
        }
        if let Tool::Place(symbol) | Tool::PlaceFor { symbol, .. } = &self.tool
            && let Ok(sym) = self.project.symbol(symbol)
        {
            let at = snap(self.cursor, self.grid);
            let lines = sym
                .graphics
                .iter()
                .filter_map(|g| match g {
                    ecad_model::Graphic::Line { a, b } | ecad_model::Graphic::Dashed { a, b } => {
                        Some((a.offset(at), b.offset(at)))
                    }
                    _ => None,
                })
                .collect();
            out.push(Overlay::Ghost { lines });
        }
        out
    }

    // ---- internals ----

    fn hit(&self, p: Point) -> Option<Hit> {
        if self.on_layout_page() {
            return self.panel_hit(p);
        }
        let tol = self.hit_tolerance;
        let near = |q: Point| ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt() <= tol;
        let functions: Vec<&Function> = self.project.functions_on(self.page).collect();
        // Several points can sit on one spot (a junction's four): prefer one without a wire.
        let mut points = Vec::new();
        for f in &functions {
            let Ok(sym) = self.project.symbol(&f.symbol) else { continue };
            points.extend(
                sym.connection_points
                    .iter()
                    .filter(|cp| near(f.to_page(cp.at)))
                    .map(|cp| ConnectionRef::new(f.id, &cp.name)),
            );
        }
        if let Some(free) = points.iter().find(|r| self.project.connection_at(r).is_none()).or(points.first()) {
            return Some(Hit::Point(free.clone()));
        }
        for f in &functions {
            if let Some((lo, hi)) = self.function_box(f.id)
                && p.x >= lo.x - 0.5
                && p.x <= hi.x + 0.5
                && p.y >= lo.y - 0.5
                && p.y <= hi.y + 0.5
            {
                return Some(Hit::Function(f.id));
            }
        }
        for c in self.project.connections_on(self.page) {
            let route = self.project.route(c).unwrap_or_default();
            if route.windows(2).any(|s| dist_to_segment(p, s[0], s[1]) <= tol) {
                return Some(Hit::Connection(c.id));
            }
        }
        // A box by its edge, or its label above the top-left corner.
        for b in self.project.boxes_on(self.page) {
            let (lo, hi) = (b.min, b.max);
            let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y), lo];
            let on_edge = corners.windows(2).any(|s| dist_to_segment(p, s[0], s[1]) <= tol);
            let on_label = !b.label.is_empty()
                && p.x >= lo.x
                && p.x <= lo.x + 6.0 + 2.5 * b.label.len() as f64
                && p.y >= hi.y
                && p.y <= hi.y + 6.0;
            if on_edge || on_label {
                return Some(Hit::Box(b.id));
            }
        }
        None
    }

    /// Set the selected box's label (`+A101`, `EXT`, `-XCPR1`…).
    pub fn label_selected_box(&mut self, label: &str) -> Result<(), ModelError> {
        let Some(Selected::Box(id)) = self.selection else { return Ok(()) };
        let Some(b) = self.project.boxes.get(&id) else { return Ok(()) };
        if b.label == label.trim() {
            return Ok(());
        }
        let updated = ecad_model::LocationBox { label: label.to_owned(), ..b.clone() };
        self.execute(&format!("Label box {}", label.trim()), Command::UpdateBox(updated))
    }

    /// A function's box on the page.
    fn function_box(&self, id: FunctionId) -> Option<(Point, Point)> {
        self.project.function_bounds(id).ok().flatten()
    }

    fn wire_points(&self, id: ConnectionId) -> Option<Vec<Point>> {
        self.project.route(self.project.connection(id).ok()?).ok()
    }

    /// A new change made after undoing past the saved state throws away the redo steps that led
    /// back to it, so that state is gone for good.
    fn forget_saved_branch(&mut self) {
        if self.saved_at.is_some_and(|m| self.history.depth() < m) {
            self.saved_at = None;
        }
    }

    fn tag_of(&self, f: FunctionId) -> String {
        self.project.function(f).and_then(|f| self.project.device(f.device)).map(|d| d.tag.clone()).unwrap_or_default()
    }

    fn wire_name(&self, c: ConnectionId) -> String {
        self.project.connection(c).ok().and_then(|c| c.wire_number.clone()).unwrap_or_default()
    }

    fn place(&mut self, symbol: &SymbolId, at: Point) {
        let result = self.place_new(self.page, symbol, at);
        if let Ok(function) = result {
            self.selection = Some(Selected::Function(function));
        }
        self.report(result.map(|_| ()));
    }

    /// Place a symbol as a new device on `page` (the project's next tag), undoably.
    pub(crate) fn place_new(&mut self, page: PageId, symbol: &SymbolId, at: Point) -> Result<FunctionId, ModelError> {
        self.project.symbol(symbol)?;
        // The project's tag format: -K1, or A2SB1 with the page in front.
        let tag = self.project.new_tag(symbol, page);
        // A black box is one particular device: it starts with the box's description.
        let description = match self.project.symbol(symbol) {
            Ok(s) if s.id.0.starts_with("box.") => s.description.clone(),
            _ => String::new(),
        };
        let device = DeviceId::new();
        let function = FunctionId::new();
        let cmd = Command::Batch(vec![
            Command::AddDevice(Device::new(device, &tag, &description)),
            Command::Place(Function {
                terminals: Default::default(),
                id: function,
                device,
                symbol: symbol.clone(),
                page,
                position: at,
                rotation: Rotation::R0,
                // A new terminal strip starts at terminal 1; contacts keep their own numbers.
                designations: self.project.next_designations(device, symbol),
                // A PLC channel gets the next free address.
                properties: self.project.new_drawing_properties(symbol),
                mirrored: false,
            }),
        ]);
        self.execute(&format!("Place {tag}"), cmd)?;
        Ok(function)
    }

    fn place_for(&mut self, symbol: &SymbolId, device: DeviceId, at: Point) {
        let result = self.place_another(self.page, symbol, device, at);
        if let Ok(function) = result {
            self.tool = Tool::Select;
            self.selection = Some(Selected::Function(function));
        }
        self.report(result.map(|_| ()));
    }

    /// Place another drawing of `device` (a contact of a relay, a terminal of a strip) on
    /// `page`, undoably.
    pub(crate) fn place_another(
        &mut self,
        page: PageId,
        symbol: &SymbolId,
        device: DeviceId,
        at: Point,
    ) -> Result<FunctionId, ModelError> {
        let tag = self.project.device(device)?.tag.clone();
        let designations = self.project.next_designations(device, symbol);
        let mut terminals: Vec<&String> = designations.values().collect();
        terminals.sort();
        let numbered = self.project.symbol(symbol).is_ok_and(|s| s.role.numbered());
        let what = if numbered {
            terminals.first().map(|n| format!(":{n}")).unwrap_or_default()
        } else if terminals.is_empty() {
            String::new()
        } else {
            format!(" contact {}", terminals.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("-"))
        };
        let function = FunctionId::new();
        let cmd = Command::Place(Function {
            terminals: Default::default(),
            id: function,
            device,
            symbol: symbol.clone(),
            page,
            position: at,
            rotation: Rotation::R0,
            designations,
            properties: self.project.new_drawing_properties(symbol),
            mirrored: false,
        });
        self.execute(&format!("Place {tag}{what}"), cmd)?;
        Ok(function)
    }

    /// Set one property of the selected drawing (a PLC channel's `address`, `symbol` or
    /// `comment`); blank clears it.
    pub fn set_selected_property(&mut self, key: &str, value: &str) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let func = self.project.function(f)?;
        let value = value.trim();
        if func.property(key) == value {
            return Ok(());
        }
        let mut properties = func.properties.clone();
        properties.insert(key.to_owned(), value.to_owned());
        let n = self.project.drawing_number(f).unwrap_or_default();
        let label = format!("Set {key} of {}:{n}", self.tag_of(f));
        self.execute(&label, Command::SetProperties { id: f, properties })
    }

    /// Give the selected terminal or PLC channel another number (unique on its device).
    pub fn renumber_selected(&mut self, number: &str) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let Some(old) = self.project.drawing_number(f) else { return Ok(()) };
        let number = number.trim();
        if number == old {
            return Ok(());
        }
        let func = self.project.function(f)?;
        let sym = self.project.symbol(&func.symbol)?;
        let designations = sym.connection_points.iter().map(|cp| (cp.name.clone(), number.to_owned())).collect();
        let label = format!("Renumber {}:{old} to {number}", self.tag_of(f));
        self.execute(&label, Command::SetDesignations { id: f, designations })
    }

    /// Make the selected drawing show terminals of the device tagged `tag` (e.g. turn a stray
    /// contact into a contact of -K1). It takes that device's next contact numbers, and its
    /// wires go to those terminals: a rewiring, said so in the step's name (D2). Its old device
    /// is removed when nothing else draws it.
    pub fn reassign_selected(&mut self, tag: &str) -> Result<(), ModelError> {
        let Some(Selected::Function(f)) = self.selection else { return Ok(()) };
        let tag = tag.trim();
        let func = self.project.function(f)?.clone();
        let target =
            self.project.device_by_tag(tag).map(|d| d.id).ok_or_else(|| ModelError::UnknownTag(tag.to_owned()))?;
        if target == func.device {
            return Ok(());
        }
        let old_tag = self.project.device(func.device)?.tag.clone();
        let designations = self.project.next_designations(target, &func.symbol);
        let mut cmds = vec![Command::Reassign { id: f, device: target, designations, terminals: Default::default() }];
        if self.project.functions_of(func.device).count() == 1 {
            cmds.push(Command::RemoveDevice(func.device));
        }
        self.execute(&format!("Rewire {old_tag} to {tag}"), Command::Batch(cmds))
    }

    fn wire_click(&mut self, hit: Option<Hit>) {
        let Some(Hit::Point(point)) = hit else {
            if self.wire_start.is_none() {
                self.status = "Click a connection point to start a wire".into();
            }
            return;
        };
        if let Some(c) = self.project.connection_at(&point) {
            let (tag, pt) = self.project.describe(&point).unwrap_or_default();
            self.status = format!("{tag}:{pt} already has wire {}", c.wire_number.clone().unwrap_or_default());
            return;
        }
        match self.wire_start.take() {
            None => self.wire_start = Some(point),
            Some(start) if start == point => {}
            Some(start) => {
                let result = self.draw_wire(start, point);
                self.report(result.map(|_| ()));
            }
        }
    }

    /// Draw a wire between two free connection points, undoably: it takes its potential's
    /// number and keeps off other wires and symbols where a nearby route does.
    pub(crate) fn draw_wire(&mut self, start: ConnectionRef, point: ConnectionRef) -> Result<ConnectionId, ModelError> {
        {
            {
                // Per potential, the wire takes its potential's number, and a wire that joins
                // two potentials brings the other one's wires to the same number.
                let (number, renumber) = self.project.number_for_new_wire(&start, &point);
                let mut cmds: Vec<Command> = renumber
                    .into_iter()
                    .map(|id| Command::SetWireNumber { id, number: Some(number.clone()) })
                    .collect();
                let id = ConnectionId::new();
                cmds.push(Command::Connect(Connection {
                    bend: None,
                    id,
                    from: start,
                    to: point,
                    wire_number: Some(number.clone()),
                    spec: Default::default(),
                }));
                // The new wire keeps off other wires and symbols where a nearby route does.
                let mut trial = self.project.clone();
                if trial.apply(Command::Batch(cmds.clone())).is_ok()
                    && let Some(bend) = trial.free_bend(id)
                {
                    cmds.push(Command::SetWireBend { id, bend: Some(bend) });
                }
                self.execute(&format!("Draw wire {number}"), Command::Batch(cmds))?;
                Ok(id)
            }
        }
    }

    fn execute(&mut self, label: &str, cmd: Command) -> Result<(), ModelError> {
        let depth = self.history.depth();
        self.history.execute(&mut self.project, label, cmd)?;
        if self.saved_at.is_some_and(|m| depth < m) {
            self.saved_at = None;
        }
        self.changed(String::new());
        Ok(())
    }

    fn report(&mut self, r: Result<(), ModelError>) {
        if let Err(e) = r {
            self.status = e.to_string();
        }
    }

    fn changed(&mut self, status: String) {
        self.revision += 1;
        self.scene += 1;
        self.status = status;
        // Undo can remove the page being shown (undoing "Add page").
        if self.project.page(self.page).is_err()
            && let Some(first) = self.project.pages_in_order().first()
        {
            self.page = first.id;
        }
        self.forget_missing();
    }

    /// Clear selection and hover that point at things undo or delete removed.
    fn forget_missing(&mut self) {
        let exists = |s: &Selected| match s {
            Selected::Function(f) => self.project.function(*f).is_ok(),
            Selected::Connection(c) => self.project.connection(*c).is_ok(),
            Selected::Box(b) => self.project.boxes.contains_key(b),
            Selected::PanelItem(i) => self.project.panel_items.contains_key(i),
        };
        if self.selection.as_ref().is_some_and(|s| !exists(s)) {
            self.selection = None;
        }
        self.hover = None;
    }
}

mod panel;
pub use panel::DUCT_WIDTH;

#[cfg(test)]
mod tests;
