//! The editing controller, independent of any UI toolkit.
//!
//! A shell (egui, Tauri, tests) feeds it pointer events in page millimetres and key presses,
//! and draws what it returns: the page's base primitives from [`Editor::base`] (rebuilt when
//! [`Editor::revision`] changes) plus the overlays from [`Editor::overlays`]. All behaviour —
//! tools, hit-testing, snapping, undo — lives here, so it is tested without a window and any
//! shell gets the same editor.

use ecad_model::{
    Command, Connection, ConnectionId, ConnectionRef, Device, DeviceId, Direction, Function, FunctionId, History,
    ModelError, Page, PageId, Point, Potential, Project, Rotation, SymbolId, route_between,
};
pub use ecad_model::{Naming, Part, PartRef, ProjectInfo, Revision, WireNumbering};
pub use ecad_view::{Primitive, Shape};
pub use session::{
    Button, Choice, DevicePart, Frame, KeyName, PageInfo, PaletteItem, SelectionInfo, Session, SheetGrid, UiEvent,
};
pub use view::Viewport;

pub mod session;
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selected {
    Function(FunctionId),
    Connection(ConnectionId),
}

/// What is under the pointer, in priority order: connection points, then symbols, then wires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Point(ConnectionRef),
    Function(FunctionId),
    Connection(ConnectionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Delete,
    /// Rotate the selected symbol a quarter turn counter-clockwise.
    Rotate,
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

    pub fn remove_part(&mut self, number: &str) -> Result<(), ModelError> {
        self.execute(&format!("Remove part {number}"), Command::RemovePart(number.to_owned()))
    }

    /// Add new parts and update existing ones (by part number) in one undoable step. Returns
    /// how many were added and how many changed.
    pub fn import_parts(&mut self, parts: Vec<Part>) -> Result<(usize, usize), ModelError> {
        let (mut added, mut updated) = (0, 0);
        let mut cmds = Vec::new();
        for part in parts {
            match self.project.parts.get(&part.number) {
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
        // Devices drawn only on this page go with it.
        let mut devices: Vec<DeviceId> = on_page.iter().map(|f| f.device).collect();
        devices.sort();
        devices.dedup();
        for d in devices {
            if self.project.functions.values().all(|f| f.device != d || f.page == page) {
                cmds.push(Command::RemoveDevice(d));
            }
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

    /// Set how new tags and wire numbers are formed.
    pub fn set_naming(&mut self, naming: ecad_model::Naming) -> Result<(), ModelError> {
        if naming == self.project.naming {
            return Ok(());
        }
        self.execute("Naming", Command::SetNaming(naming))
    }

    /// Number every wire again from the project's wire format, page by page.
    pub fn renumber_all_wires(&mut self) -> Result<(), ModelError> {
        let cmds = self.project.renumber_all_wires();
        self.execute("Renumber all wires", Command::Batch(cmds))
    }

    pub fn pointer_move(&mut self, p: Point) {
        self.cursor = p;
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
        match self.tool.clone() {
            Tool::Select => match hit {
                Some(Hit::Function(f)) | Some(Hit::Point(ConnectionRef { function: f, .. })) => {
                    self.selection = Some(Selected::Function(f));
                    if let Ok(func) = self.project.function(f) {
                        let grab = Point::new(p.x - func.position.x, p.y - func.position.y);
                        self.drag = Some(Drag { function: f, from: func.position, grab, moved: false });
                    }
                }
                Some(Hit::Connection(c)) => self.selection = Some(Selected::Connection(c)),
                None => self.selection = None,
            },
            Tool::Place(symbol) => self.place(&symbol, snap(p, self.grid)),
            Tool::PlaceFor { symbol, device } => self.place_for(&symbol, device, snap(p, self.grid)),
            Tool::Wire => self.wire_click(hit),
        }
    }

    pub fn pointer_up(&mut self, p: Point) {
        self.cursor = p;
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
                if self.wire_start.take().is_none() {
                    self.selection = None;
                    self.tool = Tool::Select;
                }
            }
            Key::Delete => match self.selection.take() {
                Some(Selected::Function(f)) => {
                    let label = format!("Delete {}", self.tag_of(f));
                    let device = self.project.function(f).map(|f| f.device).ok();
                    let mut cmds = vec![Command::Unplace(f)];
                    // Drop the device too when this was its only drawing.
                    if let Some(d) = device
                        && self.project.functions.values().filter(|x| x.device == d).count() == 1
                    {
                        cmds.push(Command::RemoveDevice(d));
                    }
                    let r = self.execute(&label, Command::Batch(cmds));
                    self.report(r);
                }
                Some(Selected::Connection(c)) => {
                    let label = format!("Delete wire {}", self.wire_name(c));
                    let r = self.execute(&label, Command::Disconnect(c));
                    self.report(r);
                }
                None => {}
            },
            Key::Rotate => {
                if let Some(Selected::Function(f)) = self.selection
                    && let Ok(func) = self.project.function(f)
                {
                    let to = next_rotation(func.rotation);
                    let label = format!("Rotate {}", self.tag_of(f));
                    let r = self.execute(&label, Command::Rotate { id: f, to });
                    self.report(r);
                }
            }
            Key::Undo => match self.history.undo(&mut self.project) {
                Ok(Some(label)) => self.changed(format!("Undid: {label}")),
                Ok(None) => self.status = "Nothing to undo".into(),
                Err(e) => self.status = e.to_string(),
            },
            Key::Redo => match self.history.redo(&mut self.project) {
                Ok(Some(label)) => self.changed(format!("Redid: {label}")),
                Ok(None) => self.status = "Nothing to redo".into(),
                Err(e) => self.status = e.to_string(),
            },
        }
        self.forget_missing();
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
        self.select(Some(match check.target {
            ecad_rules::Target::Function(f) => Selected::Function(f),
            ecad_rules::Target::Connection(c) => Selected::Connection(c),
        }));
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
            _ => {}
        }
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
            None => {}
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
                    ecad_model::Graphic::Line { a, b } => Some((a.offset(at), b.offset(at))),
                    _ => None,
                })
                .collect();
            out.push(Overlay::Ghost { lines });
        }
        out
    }

    // ---- internals ----

    fn hit(&self, p: Point) -> Option<Hit> {
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
        None
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
        if let Err(e) = self.project.symbol(symbol) {
            return self.status = e.to_string();
        }
        // The project's tag format: -K1, or A2SB1 with the page in front.
        let tag = self.project.new_tag(symbol, self.page);
        let device = DeviceId::new();
        let function = FunctionId::new();
        let cmd = Command::Batch(vec![
            Command::AddDevice(Device::new(device, &tag, "")),
            Command::Place(Function {
                id: function,
                device,
                symbol: symbol.clone(),
                page: self.page,
                position: at,
                rotation: Rotation::R0,
                // A new terminal strip starts at terminal 1; contacts keep their own numbers.
                designations: self.project.next_designations(device, symbol),
                // A PLC channel gets the next free address.
                properties: self.project.new_drawing_properties(symbol),
            }),
        ]);
        let result = self.execute(&format!("Place {tag}"), cmd);
        if result.is_ok() {
            self.selection = Some(Selected::Function(function));
        }
        self.report(result);
    }

    fn place_for(&mut self, symbol: &SymbolId, device: DeviceId, at: Point) {
        let tag = match self.project.device(device) {
            Ok(d) => d.tag.clone(),
            Err(e) => return self.status = e.to_string(),
        };
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
            id: function,
            device,
            symbol: symbol.clone(),
            page: self.page,
            position: at,
            rotation: Rotation::R0,
            designations,
            properties: self.project.new_drawing_properties(symbol),
        });
        let result = self.execute(&format!("Place {tag}{what}"), cmd);
        if result.is_ok() {
            self.tool = Tool::Select;
            self.selection = Some(Selected::Function(function));
        }
        self.report(result);
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

    /// Make the selected drawing part of the device tagged `tag` (e.g. turn a stray contact
    /// into a contact of -K1). It takes that device's next contact numbers; its old device is
    /// removed when nothing else draws it.
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
        let mut cmds = vec![Command::Reassign { id: f, device: target, designations }];
        if self.project.functions_of(func.device).count() == 1 {
            cmds.push(Command::RemoveDevice(func.device));
        }
        self.execute(&format!("Move {old_tag} to {tag}"), Command::Batch(cmds))
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
                // Per potential, the wire takes its potential's number, and a wire that joins
                // two potentials brings the other one's wires to the same number.
                let (number, renumber) = self.project.number_for_new_wire(&start, &point);
                let mut cmds: Vec<Command> = renumber
                    .into_iter()
                    .map(|id| Command::SetWireNumber { id, number: Some(number.clone()) })
                    .collect();
                cmds.push(Command::Connect(Connection {
                    id: ConnectionId::new(),
                    from: start,
                    to: point,
                    wire_number: Some(number.clone()),
                }));
                let result = self.execute(&format!("Draw wire {number}"), Command::Batch(cmds));
                self.report(result);
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
        };
        if self.selection.as_ref().is_some_and(|s| !exists(s)) {
            self.selection = None;
        }
        self.hover = None;
    }
}

#[cfg(test)]
mod tests;
