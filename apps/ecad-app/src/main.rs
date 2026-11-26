//! Electric Studio: the editor application, a thin egui shell over `ecad_editor::Session`.
//!
//! Usage: `electric-studio [project-dir]`. Without a folder it opens (or creates) the
//! sample project in `~/ElectricStudio/Sample`.
//!
//! This shell only turns egui input into `UiEvent`s and paints the `Frame` it gets back. All
//! behaviour lives in `ecad-editor` (AGENTS.md rule 6).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ecad_editor::{
    Button, Choice, Frame, KeyName, Marker, Overlay, Primitive, SelectionInfo, Session, Shape, UiEvent, Viewport,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::path::PathBuf;

const BG: [u8; 3] = [30, 33, 39];
const ACCENT: Color32 = Color32::from_rgb(64, 156, 255);
const TARGET: Color32 = Color32::from_rgb(255, 170, 40);
const START: Color32 = Color32::from_rgb(80, 220, 120);
const QUIET: Color32 = Color32::from_rgb(140, 150, 165);

fn default_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join("ElectricStudio").join("Sample")
}

struct App {
    session: Session,
    base: Vec<Primitive>,
    revision: Option<u64>,
    size: Vec2,
    /// Side-panel edit buffers, reset when the selection changes.
    fields: Fields,
    page_fields: PageFields,
    title: String,
    close_sent: bool,
}

/// Edit buffers for the current page's number and content.
#[derive(Default)]
struct PageFields {
    key: Option<String>,
    name: String,
    description: String,
}

#[derive(Default)]
struct Fields {
    /// Which selection these buffers belong to.
    key: Option<String>,
    tag: String,
    description: String,
    number: String,
    /// "Belongs to" device tag for a contact.
    owner: String,
}

/// Ask for a project folder with the system's folder picker.
fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

impl App {
    fn send(&mut self, ev: UiEvent) {
        self.session.handle(ev);
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return; // typing in the tag field
        }
        let mut events = Vec::new();
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            if i.key_pressed(egui::Key::Escape) {
                events.push(UiEvent::Key { key: KeyName::Escape });
            }
            if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
                events.push(UiEvent::Key { key: KeyName::Delete });
            }
            if i.key_pressed(egui::Key::R) && !cmd {
                events.push(UiEvent::Key { key: KeyName::Rotate });
            }
            if i.key_pressed(egui::Key::F) && !cmd {
                events.push(UiEvent::Fit);
            }
            if i.key_pressed(egui::Key::W) && !cmd {
                events.push(UiEvent::Tool { tool: "wire".into() });
            }
            if cmd && i.key_pressed(egui::Key::Z) {
                events.push(UiEvent::Key { key: if i.modifiers.shift { KeyName::Redo } else { KeyName::Undo } });
            }
            if cmd && i.key_pressed(egui::Key::Y) {
                events.push(UiEvent::Key { key: KeyName::Redo });
            }
            if cmd && i.key_pressed(egui::Key::S) {
                events.push(UiEvent::Save);
            }
        });
        let (open, new) = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(egui::Key::O), i.modifiers.command && i.key_pressed(egui::Key::N))
        });
        if open {
            self.open_dialog();
        }
        if new {
            self.new_dialog();
        }
        for ev in events {
            self.send(ev);
        }
    }

    /// A frame for the panels: everything except the page drawing, which only the canvas needs.
    fn panels_frame(&self) -> Frame {
        self.session.frame(Some(self.session.editor().scene_revision()), BG)
    }

    fn open_dialog(&mut self) {
        if let Some(path) = pick_folder("Open a project folder") {
            self.send(UiEvent::Open { path });
        }
    }

    fn new_dialog(&mut self) {
        if let Some(path) = pick_folder("Choose an empty folder for the new project") {
            self.send(UiEvent::New { path });
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New project… (Ctrl+N)").clicked() {
                    ui.close();
                    self.new_dialog();
                }
                if ui.button("Open project… (Ctrl+O)").clicked() {
                    ui.close();
                    self.open_dialog();
                }
                if ui.add_enabled(frame.dirty, egui::Button::new("Save (Ctrl+S)")).clicked() {
                    ui.close();
                    self.send(UiEvent::Save);
                }
            });
            ui.separator();
            let mut tool = |ui: &mut egui::Ui, label: &str, id: &str, tip: &str| {
                if ui.selectable_label(frame.tool == id, label).on_hover_text(tip).clicked() {
                    self.session.handle(UiEvent::Tool { tool: id.to_owned() });
                }
            };
            tool(ui, "Select", "select", "Click to select, drag to move (Esc)");
            tool(ui, "Wire", "wire", "Click two connection points (W)");
            ui.separator();
            for item in &frame.palette {
                tool(ui, &item.description, &item.id, &format!("Place {}", item.id));
            }
            ui.separator();
            let undo = frame.undo.as_deref().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Nothing to undo".into());
            if ui.add_enabled(frame.undo.is_some(), egui::Button::new("Undo")).on_hover_text(undo).clicked() {
                self.session.handle(UiEvent::Key { key: KeyName::Undo });
            }
            let redo = frame.redo.as_deref().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Nothing to redo".into());
            if ui.add_enabled(frame.redo.is_some(), egui::Button::new("Redo")).on_hover_text(redo).clicked() {
                self.session.handle(UiEvent::Key { key: KeyName::Redo });
            }
            if ui.button("Fit").on_hover_text("Zoom to the whole page (F)").clicked() {
                self.session.handle(UiEvent::Fit);
            }
            if ui.add_enabled(frame.dirty, egui::Button::new("Save")).on_hover_text("Ctrl+S").clicked() {
                self.session.handle(UiEvent::Save);
            }
        });
    }

    fn inspector(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.heading("Selection");
        let key = frame.selection.as_ref().map(|sel| match sel {
            SelectionInfo::Device { tag, description, symbol, .. } => format!("device {tag} {description} {symbol}"),
            SelectionInfo::Wire { number, from, to } => format!("wire {from} {to} {number:?}"),
        });
        if key != self.fields.key {
            self.fields = Fields { key, ..Fields::default() };
            match &frame.selection {
                Some(SelectionInfo::Device { tag, description, .. }) => {
                    self.fields.tag = tag.clone();
                    self.fields.description = description.clone();
                }
                Some(SelectionInfo::Wire { number, .. }) => self.fields.number = number.clone().unwrap_or_default(),
                None => {}
            }
        }
        // A field is applied when it loses focus (Tab, Enter or a click elsewhere) with a change.
        let mut events = Vec::new();
        match &frame.selection {
            Some(SelectionInfo::Device { tag, description, symbol, role, references }) => {
                if self.fields.owner.is_empty() {
                    self.fields.owner = tag.clone();
                }
                ui.label("Device tag");
                if ui.text_edit_singleline(&mut self.fields.tag).lost_focus() && self.fields.tag.trim() != tag {
                    events.push(UiEvent::Rename { tag: self.fields.tag.clone() });
                }
                ui.label("Description");
                let edit = ui.text_edit_singleline(&mut self.fields.description);
                if edit.lost_focus() && self.fields.description.trim() != description {
                    events.push(UiEvent::Describe { text: self.fields.description.clone() });
                }
                ui.add_space(4.0);
                ui.small(format!("Symbol: {symbol}"));
                match role.as_str() {
                    "main" => {
                        ui.add_space(8.0);
                        ui.strong("Contacts");
                        if references.is_empty() {
                            ui.small("None yet.");
                        }
                        for r in references {
                            if ui.link(&r.label).on_hover_text("Show this page").clicked() {
                                events.push(UiEvent::ShowPage { id: r.page });
                            }
                        }
                        for item in &frame.contact_palette {
                            let label = format!("Add {}", item.description.to_lowercase());
                            if ui.button(label).on_hover_text("Then click where it goes, on any page").clicked() {
                                events.push(UiEvent::AddContact { symbol: item.id.clone() });
                            }
                        }
                    }
                    "contact" => {
                        ui.add_space(8.0);
                        for r in references {
                            if ui.link(&r.label).on_hover_text("Show this page").clicked() {
                                events.push(UiEvent::ShowPage { id: r.page });
                            }
                        }
                        ui.label("Belongs to device");
                        let edit = ui.text_edit_singleline(&mut self.fields.owner);
                        if edit.lost_focus() && self.fields.owner.trim() != tag {
                            events.push(UiEvent::AssignTo { tag: self.fields.owner.clone() });
                        }
                        ui.small("Type a coil's tag, e.g. -K1, to make this one of its contacts.");
                    }
                    _ => {}
                }
                ui.add_space(8.0);
                ui.label("R rotates, Delete removes.");
            }
            Some(SelectionInfo::Wire { number, from, to }) => {
                ui.label("Wire number");
                let edit = ui.text_edit_singleline(&mut self.fields.number);
                if edit.lost_focus() && self.fields.number.trim() != number.as_deref().unwrap_or("") {
                    events.push(UiEvent::NumberWire { number: self.fields.number.clone() });
                }
                ui.add_space(4.0);
                ui.label(format!("From {from}"));
                ui.label(format!("To {to}"));
                ui.add_space(8.0);
                ui.label("Delete removes the wire.");
            }
            None => {
                ui.label("Nothing selected.");
            }
        }
        for ev in events {
            self.send(ev);
        }
        ui.add_space(16.0);
        ui.separator();
        ui.small("Middle-drag to pan, wheel to zoom, F to fit.");
        ui.small(format!("Project: {}", frame.project_dir));
    }

    fn pages(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.heading("Pages");
        let mut events = Vec::new();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 170.0).show(ui, |ui| {
            for p in &frame.pages {
                let label =
                    if p.description.is_empty() { p.name.clone() } else { format!("{}  {}", p.name, p.description) };
                if ui.selectable_label(p.id == frame.page, label).clicked() && p.id != frame.page {
                    events.push(UiEvent::ShowPage { id: p.id });
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Add").on_hover_text("New page after this one").clicked() {
                events.push(UiEvent::AddPage);
            }
            let deletable = frame.pages.len() > 1;
            if ui
                .add_enabled(deletable, egui::Button::new("Delete"))
                .on_hover_text("Delete this page and everything on it (undoable)")
                .clicked()
            {
                events.push(UiEvent::DeletePage);
            }
            if ui.button("▲").on_hover_text("Move earlier").clicked() {
                events.push(UiEvent::MovePage { delta: -1 });
            }
            if ui.button("▼").on_hover_text("Move later").clicked() {
                events.push(UiEvent::MovePage { delta: 1 });
            }
        });
        ui.separator();
        if let Some(current) = frame.pages.iter().find(|p| p.id == frame.page) {
            let key = Some(format!("{:?} {} {}", current.id, current.name, current.description));
            if key != self.page_fields.key {
                self.page_fields =
                    PageFields { key, name: current.name.clone(), description: current.description.clone() };
            }
            ui.label("Page number");
            if ui.text_edit_singleline(&mut self.page_fields.name).lost_focus()
                && self.page_fields.name.trim() != current.name
            {
                events.push(UiEvent::RenamePage { name: self.page_fields.name.clone() });
            }
            ui.label("Page content");
            if ui.text_edit_singleline(&mut self.page_fields.description).lost_focus()
                && self.page_fields.description.trim() != current.description
            {
                events.push(UiEvent::DescribePage { text: self.page_fields.description.clone() });
            }
            ui.small("PageUp / PageDown switch pages.");
        }
        for ev in events {
            self.send(ev);
        }
    }

    fn prompt(&mut self, ctx: &egui::Context, question: &str) {
        let mut answer = None;
        egui::Modal::new(egui::Id::unique("unsaved-changes")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.heading("Unsaved changes");
            ui.label(question);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Choice::Save);
                }
                if ui.button("Don't save").clicked() {
                    answer = Some(Choice::Discard);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(Choice::Cancel);
                }
            });
        });
        if let Some(choice) = answer {
            self.send(UiEvent::Confirm { choice });
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> Rect {
        let (response, _painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        if rect.size() != self.size {
            self.size = rect.size();
            self.send(UiEvent::Resize { width: f64::from(rect.width()), height: f64::from(rect.height()) });
        }
        let mut events = Vec::new();
        ui.input(|i| {
            let Some(pos) = i.pointer.latest_pos() else { return };
            let inside = rect.contains(pos);
            let (x, y) = (f64::from(pos.x - rect.min.x), f64::from(pos.y - rect.min.y));
            if i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed() {
                events.push(UiEvent::PointerMove { x, y });
            }
            for (button, egui_button) in
                [(Button::Primary, egui::PointerButton::Primary), (Button::Middle, egui::PointerButton::Middle)]
            {
                if inside && i.pointer.button_pressed(egui_button) {
                    events.push(UiEvent::PointerDown { x, y, button });
                }
                if i.pointer.button_released(egui_button) {
                    events.push(UiEvent::PointerUp { x, y, button });
                }
            }
            let scroll = i.smooth_scroll_delta.y;
            if inside && scroll != 0.0 {
                events.push(UiEvent::Wheel { x, y, delta: f64::from(scroll) / 50.0 });
            }
        });
        for ev in events {
            self.send(ev);
        }
        rect
    }
}

fn color([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn paint(painter: &egui::Painter, rect: Rect, view: &Viewport, base: &[Primitive], overlays: &[Overlay]) {
    let at = |p: ecad_editor::view::Point| {
        let (x, y) = view.to_screen(p);
        Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
    };
    painter.rect_filled(rect, 0.0, color(BG));
    for prim in base {
        match &prim.shape {
            Shape::Polyline(pts) => {
                painter.add(egui::Shape::line(pts.iter().map(|p| at(*p)).collect(), Stroke::new(1.2, color(prim.rgb))));
            }
            Shape::Triangles(pts) => {
                let mut mesh = egui::Mesh::default();
                for tri in pts.as_chunks::<3>().0 {
                    let i = mesh.vertices.len() as u32;
                    for p in tri {
                        mesh.colored_vertex(at(*p), color(prim.rgb));
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                painter.add(mesh);
            }
        }
    }
    for o in overlays {
        match o {
            Overlay::Marker { at: p, kind } => match kind {
                Marker::Open => {
                    painter.circle_stroke(at(*p), 3.5, Stroke::new(1.0, QUIET));
                }
                Marker::Target => {
                    painter.circle_filled(at(*p), 5.0, TARGET);
                }
                Marker::Start => {
                    painter.circle_filled(at(*p), 5.0, START);
                }
            },
            Overlay::Outline { min, max, selected } => {
                let r = Rect::from_two_pos(at(*min), at(*max)).expand(4.0);
                let stroke = if *selected { Stroke::new(1.5, ACCENT) } else { Stroke::new(1.0, QUIET) };
                painter.rect_stroke(r, 3.0, stroke, egui::StrokeKind::Outside);
            }
            Overlay::Wire { points, selected } => {
                let stroke = if *selected { Stroke::new(3.0, ACCENT) } else { Stroke::new(3.0, QUIET) };
                painter.add(egui::Shape::line(points.iter().map(|p| at(*p)).collect(), stroke));
            }
            Overlay::RubberBand { points } => {
                painter.add(egui::Shape::line(points.iter().map(|p| at(*p)).collect(), Stroke::new(1.5, TARGET)));
            }
            Overlay::Ghost { lines } => {
                for (a, b) in lines {
                    painter.line_segment([at(*a), at(*b)], Stroke::new(1.0, QUIET));
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Closing the window with unsaved changes asks first; the session decides.
        if ctx.input(|i| i.viewport().close_requested()) && !self.session.can_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.send(UiEvent::CloseRequested);
        }
        let now = self.panels_frame();
        if now.prompt.is_none() {
            self.keys(&ctx);
            if !ctx.egui_wants_keyboard_input() {
                let (up, down) = ctx.input(|i| (i.key_pressed(egui::Key::PageUp), i.key_pressed(egui::Key::PageDown)));
                let at = now.pages.iter().position(|p| p.id == now.page).unwrap_or(0);
                let target = if up {
                    at.checked_sub(1)
                } else if down {
                    Some(at + 1)
                } else {
                    None
                };
                if let Some(p) = target.and_then(|i| now.pages.get(i)) {
                    self.send(UiEvent::ShowPage { id: p.id });
                }
            }
        }
        // The first frame computed only feeds the toolbars; the canvas uses a fresh one after
        // this frame's pointer events.
        let before = self.panels_frame();
        egui::Panel::top("tools").show(ui, |ui| self.toolbar(ui, &before));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(if before.status.is_empty() { "Ready".to_owned() } else { before.status.clone() });
        });
        egui::Panel::left("pages").default_size(170.0).show(ui, |ui| self.pages(ui, &before));
        egui::Panel::right("inspector").default_size(200.0).show(ui, |ui| self.inspector(ui, &before));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = self.canvas(ui);
            let frame = self.session.frame(self.revision, BG);
            if let Some(base) = frame.base {
                self.base = base;
                self.revision = Some(frame.revision);
            }
            paint(ui.painter(), rect, &frame.view, &self.base, &frame.overlays);
            if let Some(question) = &frame.prompt {
                self.prompt(&ctx, question);
            }
            if frame.close && !self.close_sent {
                self.close_sent = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if frame.title != self.title {
                self.title = frame.title.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} - Electric Studio", frame.title)));
            }
        });
    }
}

fn main() -> eframe::Result {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(default_dir);
    let session = match Session::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("electric-studio: {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]).with_title("Electric Studio"),
        ..Default::default()
    };
    eframe::run_native(
        "Electric Studio",
        options,
        Box::new(|_cc| {
            Ok(Box::new(App {
                session,
                base: Vec::new(),
                revision: None,
                size: Vec2::ZERO,
                fields: Fields::default(),
                page_fields: PageFields::default(),
                title: String::new(),
                close_sent: false,
            }))
        }),
    )
}
