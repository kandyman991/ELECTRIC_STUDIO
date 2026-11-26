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
    /// The parts catalogue window.
    show_parts: bool,
    /// Edit buffers for catalogue rows: number → (manufacturer, description).
    part_edits: std::collections::BTreeMap<String, (String, String)>,
    /// The "new part" row: number, manufacturer, description.
    new_part: (String, String, String),
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
        // Wraps onto a second row when the window is narrow or the library grows.
        ui.horizontal_wrapped(|ui| {
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
                ui.separator();
                if ui.button("Export reports…").on_hover_text("Wire list and bill of materials as CSV").clicked() {
                    ui.close();
                    if let Some(path) = pick_folder("Choose a folder for the reports") {
                        self.send(UiEvent::ExportReports { path });
                    }
                }
            });
            if ui.selectable_label(self.show_parts, "Parts").on_hover_text("The parts catalogue").clicked() {
                self.show_parts = !self.show_parts;
            }
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
            SelectionInfo::Device { tag, description, symbol, terminal, .. } => {
                format!("device {tag} {description} {symbol} {terminal:?}")
            }
            SelectionInfo::Wire { number, from, to, .. } => format!("wire {from} {to} {number:?}"),
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
            Some(SelectionInfo::Device { tag, description, symbol, role, references, parts, terminal }) => {
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
                if role != "potential" && role != "interruption" && !symbol.ends_with("junction") {
                    ui.add_space(8.0);
                    ui.strong(if role == "terminal" { "Parts (per terminal)" } else { "Parts" });
                    if parts.is_empty() {
                        ui.small("None yet: this device is missing from the BOM.");
                    }
                    for p in parts {
                        ui.horizontal(|ui| {
                            let mut q = p.quantity;
                            if ui.add(egui::DragValue::new(&mut q).range(1..=999).suffix("×")).changed() {
                                events.push(UiEvent::SetDevicePartQuantity { number: p.number.clone(), quantity: q });
                            }
                            ui.label(&p.number).on_hover_text(&p.description);
                            if ui.small_button("✕").on_hover_text("Remove from this device").clicked() {
                                events.push(UiEvent::RemoveDevicePart { number: p.number.clone() });
                            }
                        });
                    }
                    egui::ComboBox::from_id_salt("add-part").selected_text("Add part…").width(180.0).show_ui(
                        ui,
                        |ui| {
                            for part in &frame.parts {
                                let label = format!("{}  {}", part.number, part.description);
                                if ui.selectable_label(false, label).clicked() {
                                    events.push(UiEvent::AddDevicePart { number: part.number.clone() });
                                }
                            }
                            if frame.parts.is_empty() {
                                ui.label("The catalogue is empty: open Parts to add or import some.");
                            }
                        },
                    );
                }
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
                    "terminal" => {
                        ui.add_space(8.0);
                        ui.label("Terminal number");
                        if self.fields.number.is_empty() {
                            self.fields.number = terminal.clone().unwrap_or_default();
                        }
                        let edit = ui.text_edit_singleline(&mut self.fields.number);
                        if edit.lost_focus() && Some(self.fields.number.trim()) != terminal.as_deref() {
                            events.push(UiEvent::RenumberTerminal { number: self.fields.number.clone() });
                        }
                        ui.strong(format!("Strip {tag}"));
                        for r in references {
                            if ui.link(&r.label).on_hover_text("Show this page").clicked() {
                                events.push(UiEvent::ShowPage { id: r.page });
                            }
                        }
                        if ui
                            .button("Add terminal")
                            .on_hover_text("The strip's next number; then click where it goes, on any page")
                            .clicked()
                        {
                            events.push(UiEvent::AddContact { symbol: symbol.clone() });
                        }
                    }
                    "potential" | "interruption" => {
                        ui.add_space(8.0);
                        for r in references {
                            if ui.link(&r.label).on_hover_text("Show this page").clicked() {
                                events.push(UiEvent::ShowPage { id: r.page });
                            }
                        }
                        if ui
                            .button("Add another point")
                            .on_hover_text(
                                "Then click where it goes, on any page; all points of this name are connected",
                            )
                            .clicked()
                        {
                            events.push(UiEvent::AddContact { symbol: symbol.clone() });
                        }
                        let hint = if role == "potential" {
                            "The tag is the potential's name: L+, M, L1…"
                        } else {
                            "Points with the same tag are connected across pages."
                        };
                        ui.small(hint);
                    }
                    _ => {}
                }
                ui.add_space(8.0);
                ui.label("R rotates, Delete removes.");
            }
            Some(SelectionInfo::Wire { number, from, to, potential, warning }) => {
                ui.label("Wire number");
                let edit = ui.text_edit_singleline(&mut self.fields.number);
                if edit.lost_focus() && self.fields.number.trim() != number.as_deref().unwrap_or("") {
                    events.push(UiEvent::NumberWire { number: self.fields.number.clone() });
                }
                ui.add_space(4.0);
                ui.label(format!("From {from}"));
                ui.label(format!("To {to}"));
                ui.add_space(4.0);
                ui.label(format!("Potential: {}", potential.as_deref().unwrap_or("unnamed")))
                    .on_hover_text("Wires on the same potential are highlighted");
                if let Some(w) = warning {
                    ui.colored_label(Color32::from_rgb(255, 110, 90), format!("⚠ {w}"));
                }
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

    fn parts_window(&mut self, ctx: &egui::Context, frame: &Frame) {
        let mut open = self.show_parts;
        let mut events = Vec::new();
        egui::Window::new("Parts catalogue").open(&mut open).default_size([640.0, 420.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("Import CSV…")
                    .on_hover_text("Columns: part number, manufacturer, description; , or ; separated")
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file()
                {
                    events.push(UiEvent::ImportParts { path });
                }
                ui.label(format!("{} parts", frame.parts.len()));
            });
            ui.separator();
            // Keep edit buffers in step with the catalogue.
            self.part_edits.retain(|n, _| frame.parts.iter().any(|p| &p.number == n));
            egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).show(ui, |ui| {
                egui::Grid::new("parts-grid").striped(true).num_columns(4).show(ui, |ui| {
                    ui.strong("Part number");
                    ui.strong("Manufacturer");
                    ui.strong("Description");
                    ui.label("");
                    ui.end_row();
                    for part in &frame.parts {
                        let buf = self
                            .part_edits
                            .entry(part.number.clone())
                            .or_insert_with(|| (part.manufacturer.clone(), part.description.clone()));
                        ui.label(&part.number);
                        let m = ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut buf.0));
                        let d = ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut buf.1));
                        if (m.lost_focus() || d.lost_focus())
                            && (buf.0 != part.manufacturer || buf.1 != part.description)
                        {
                            events.push(UiEvent::UpdatePart {
                                part: ecad_editor::Part {
                                    number: part.number.clone(),
                                    manufacturer: buf.0.trim().to_owned(),
                                    description: buf.1.trim().to_owned(),
                                },
                            });
                        }
                        if ui.small_button("✕").on_hover_text("Remove from the catalogue").clicked() {
                            events.push(UiEvent::RemovePart { number: part.number.clone() });
                        }
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let (n, m, d) = &mut self.new_part;
                ui.add(egui::TextEdit::singleline(n).hint_text("Part number").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(m).hint_text("Manufacturer").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(d).hint_text("Description").desired_width(240.0));
                if ui.add_enabled(!n.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    events.push(UiEvent::AddPart {
                        part: ecad_editor::Part {
                            number: n.trim().to_owned(),
                            manufacturer: m.trim().to_owned(),
                            description: d.trim().to_owned(),
                        },
                    });
                    self.new_part = Default::default();
                }
            });
        });
        self.show_parts = open;
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
        // Only when nothing (a menu, a dropdown, the parts window) is on top of the canvas.
        let over_canvas = response.contains_pointer();
        ui.input(|i| {
            let Some(pos) = i.pointer.latest_pos() else { return };
            let inside = over_canvas;
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
            Overlay::Potential { points } => {
                painter.add(egui::Shape::line(points.iter().map(|p| at(*p)).collect(), Stroke::new(2.5, TARGET)));
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
            let mut frame = self.session.frame(self.revision, BG);
            if let Some(base) = frame.base.take() {
                self.base = base;
                self.revision = Some(frame.revision);
            }
            paint(ui.painter(), rect, &frame.view, &self.base, &frame.overlays);
            if self.show_parts {
                self.parts_window(&ctx, &frame);
            }
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
                show_parts: false,
                part_edits: Default::default(),
                new_part: Default::default(),
                title: String::new(),
                close_sent: false,
            }))
        }),
    )
}
