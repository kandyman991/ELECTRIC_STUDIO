//! The editing controller, independent of any UI toolkit.
//!
//! A shell (egui, Tauri, tests) feeds it pointer events in page millimetres and key presses,
//! and draws what it returns: the page's base primitives from [`Editor::base`] (rebuilt when
//! [`Editor::revision`] changes) plus the overlays from [`Editor::overlays`]. All behaviour —
//! tools, hit-testing, snapping, undo — lives here, so it is tested without a window and any
//! shell gets the same editor.

use ecad_model::{
    Command, Connection, ConnectionId, ConnectionRef, Device, DeviceId, Direction, Function, FunctionId, History,
    ModelError, PageId, Point, Project, Rotation, SymbolId, route_between,
};
pub use ecad_view::{Primitive, Shape};
pub use session::{Button, Choice, Frame, KeyName, PaletteItem, SelectionInfo, Session, UiEvent};
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
    saved_revision: u64,
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
            saved_revision: 0,
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
    /// Changes since the last [`Editor::mark_saved`].
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
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

    /// Bounding box (`min`, `max`) of everything on the page, for zoom to fit.
    pub fn extents(&self) -> Option<(Point, Point)> {
        let mut pts = Vec::new();
        for f in self.project.functions_on(self.page) {
            if let Some((lo, hi)) = self.function_box(f.id) {
                pts.extend([lo, hi]);
            }
        }
        for c in self.project.connections_on(self.page) {
            pts.extend(self.project.route(c).unwrap_or_default());
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
        }))
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
        self.execute(&label, Command::SetWireNumber { id: c, number: Some(number.to_owned()) })
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
            Tool::Wire => self.wire_click(hit),
        }
    }

    pub fn pointer_up(&mut self, p: Point) {
        self.cursor = p;
        if let Some(drag) = self.drag.take()
            && drag.moved
        {
            let tag = self.tag_of(drag.function);
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
        if let Tool::Place(symbol) = &self.tool
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
        for f in &functions {
            let Ok(sym) = self.project.symbol(&f.symbol) else { continue };
            if let Some(cp) = sym.connection_points.iter().find(|cp| near(f.to_page(cp.at))) {
                return Some(Hit::Point(ConnectionRef::new(f.id, &cp.name)));
            }
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

    fn tag_of(&self, f: FunctionId) -> String {
        self.project.function(f).and_then(|f| self.project.device(f.device)).map(|d| d.tag.clone()).unwrap_or_default()
    }

    fn wire_name(&self, c: ConnectionId) -> String {
        self.project.connection(c).ok().and_then(|c| c.wire_number.clone()).unwrap_or_default()
    }

    fn next_wire_number(&self) -> String {
        (1..)
            .map(|n| format!("W{n}"))
            .find(|w| !self.project.connections.values().any(|c| c.wire_number.as_deref() == Some(w)))
            .unwrap_or_default()
    }

    fn place(&mut self, symbol: &SymbolId, at: Point) {
        let prefix = match self.project.symbol(symbol) {
            Ok(s) if !s.tag_prefix.is_empty() => s.tag_prefix.clone(),
            Ok(_) => "-".to_owned(),
            Err(e) => return self.status = e.to_string(),
        };
        let tag = self.project.next_tag(&prefix);
        let device = DeviceId::new();
        let function = FunctionId::new();
        let cmd = Command::Batch(vec![
            Command::AddDevice(Device { id: device, tag: tag.clone(), description: String::new() }),
            Command::Place(Function {
                id: function,
                device,
                symbol: symbol.clone(),
                page: self.page,
                position: at,
                rotation: Rotation::R0,
            }),
        ]);
        let result = self.execute(&format!("Place {tag}"), cmd);
        if result.is_ok() {
            self.selection = Some(Selected::Function(function));
        }
        self.report(result);
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
                let number = self.next_wire_number();
                let cmd = Command::Connect(Connection {
                    id: ConnectionId::new(),
                    from: start,
                    to: point,
                    wire_number: Some(number.clone()),
                });
                let result = self.execute(&format!("Draw wire {number}"), cmd);
                self.report(result);
            }
        }
    }

    fn execute(&mut self, label: &str, cmd: Command) -> Result<(), ModelError> {
        self.history.execute(&mut self.project, label, cmd)?;
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
        self.status = status;
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
