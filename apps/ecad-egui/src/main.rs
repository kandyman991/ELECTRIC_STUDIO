//! Electric Studio in a native egui window: a thin shell over `ecad_editor::Session`.
//!
//! Usage: `electric-studio-egui [project-dir]`. Without a folder it opens (or creates) the
//! sample project in `~/ElectricStudio/Sample`.
//!
//! This shell only turns egui input into `UiEvent`s and paints the `Frame` it gets back. All
//! behaviour lives in `ecad-editor` (AGENTS.md rule 6).

use ecad_editor::{Button, Frame, KeyName, Marker, Overlay, Primitive, Session, Shape, UiEvent, Viewport};
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
    rename: String,
    renaming_for: Option<String>,
    title: String,
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
        for ev in events {
            self.send(ev);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.horizontal(|ui| {
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
        match &frame.selected_tag {
            Some(tag) => {
                if self.renaming_for.as_ref() != Some(tag) {
                    self.rename = tag.clone();
                    self.renaming_for = Some(tag.clone());
                }
                ui.label("Device tag");
                let edit = ui.text_edit_singleline(&mut self.rename);
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button("Rename").clicked()) && self.rename.trim() != tag {
                    self.session.handle(UiEvent::Rename { tag: self.rename.clone() });
                }
                ui.add_space(8.0);
                ui.label("R rotates, Delete removes.");
            }
            None => {
                self.renaming_for = None;
                ui.label("Nothing selected.");
            }
        }
        ui.add_space(16.0);
        ui.separator();
        ui.small("Middle-drag to pan, wheel to zoom.");
        ui.small(format!("Project: {}", self.session.dir().display()));
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
                for tri in pts.chunks_exact(3) {
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
        self.keys(&ctx);
        // The first frame computed only feeds the toolbars; the canvas uses a fresh one after
        // this frame's pointer events.
        let before = self.session.frame(self.revision, BG);
        egui::Panel::top("tools").show(ui, |ui| self.toolbar(ui, &before));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(if before.status.is_empty() { "Ready".to_owned() } else { before.status.clone() });
        });
        egui::Panel::right("inspector").default_size(200.0).show(ui, |ui| self.inspector(ui, &before));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = self.canvas(ui);
            let frame = self.session.frame(self.revision, BG);
            if let Some(base) = frame.base {
                self.base = base;
                self.revision = Some(frame.revision);
            }
            paint(ui.painter(), rect, &frame.view, &self.base, &frame.overlays);
            if frame.title != self.title {
                self.title = frame.title.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "{} - Electric Studio (egui)",
                    frame.title
                )));
            }
        });
    }
}

fn main() -> eframe::Result {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(default_dir);
    let session = match Session::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("electric-studio-egui: {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Electric Studio (egui)"),
        ..Default::default()
    };
    eframe::run_native(
        "Electric Studio (egui)",
        options,
        Box::new(|_cc| {
            Ok(Box::new(App {
                session,
                base: Vec::new(),
                revision: None,
                size: Vec2::ZERO,
                rename: String::new(),
                renaming_for: None,
                title: String::new(),
            }))
        }),
    )
}
