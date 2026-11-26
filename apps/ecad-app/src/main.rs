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
const ERROR: Color32 = Color32::from_rgb(240, 90, 90);
const WARNING: Color32 = Color32::from_rgb(230, 180, 60);

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
    /// Whether the checks panel is open.
    show_checks: bool,
    /// Filter text of the symbol library.
    symbol_filter: String,
    /// The cables window, its edit buffers by cable, and the new-cable row.
    show_cables: bool,
    cable_edits: std::collections::BTreeMap<String, CableEdit>,
    new_cable: CableEdit,
    black_box: BoxFields,
    /// Whether the project settings window is open, and its buffers.
    show_project: bool,
    project_fields: ProjectFields,
    /// Edit buffers for catalogue rows: number → (manufacturer, description).
    part_edits: std::collections::BTreeMap<String, (String, String, String)>,
    /// The "new part" row: number, manufacturer, description.
    new_part: (String, String, String, String),
    title: String,
    close_sent: bool,
}

/// Edit buffers for the current page's number and content.
#[derive(Default)]
struct PageFields {
    key: Option<String>,
    name: String,
    description: String,
    subtitle: String,
}

/// Edit buffer of one cable row.
#[derive(Clone, Default, PartialEq)]
struct CableEdit {
    tag: String,
    kind: String,
    cores: u32,
    shielded: bool,
}

/// The "New black box" window: terminal names as comma-separated lists.
struct BoxFields {
    open: bool,
    description: String,
    prefix: String,
    top: String,
    bottom: String,
    left: String,
    right: String,
}

impl Default for BoxFields {
    fn default() -> Self {
        BoxFields {
            open: false,
            description: String::new(),
            prefix: "-A".into(),
            top: String::new(),
            bottom: String::new(),
            left: String::new(),
            right: String::new(),
        }
    }
}

fn names(list: &str) -> Vec<String> {
    list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// Edit buffers of the project settings window; refilled when the project changes underneath.
#[derive(Default)]
struct ProjectFields {
    key: Option<String>,
    name: String,
    info: ecad_editor::ProjectInfo,
    naming: ecad_editor::Naming,
    /// Applied with a button, so dragging a number doesn't make one undo step per value.
    grid: ecad_editor::SheetGrid,
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
    /// A wire's cross-section, colour, cable and core.
    cross_section: String,
    colour: String,
    cable: String,
    core: String,
    /// A PLC channel's address, symbol and comment.
    address: String,
    plc_symbol: String,
    comment: String,
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
                if ui.button("Project settings…").on_hover_text("Title block, revisions, sheet grid").clicked() {
                    ui.close();
                    self.show_project = true;
                }
                ui.separator();
                let tip = "All pages in one PDF, plus DXF, DWG and PNG per page";
                if ui.button("Export drawings…").on_hover_text(tip).clicked() {
                    ui.close();
                    if let Some(path) = pick_folder("Choose a folder for the drawings") {
                        self.send(UiEvent::ExportDrawings { path });
                    }
                }
                let tip = "Wire list, BOM, terminal diagram and PLC I/O list as CSV";
                if ui.button("Export reports…").on_hover_text(tip).clicked() {
                    ui.close();
                    if let Some(path) = pick_folder("Choose a folder for the reports") {
                        self.send(UiEvent::ExportReports { path });
                    }
                }
            });
            if ui.selectable_label(self.show_parts, "Parts").on_hover_text("The parts catalogue").clicked() {
                self.show_parts = !self.show_parts;
            }
            if ui.selectable_label(self.show_cables, "Cables").on_hover_text("Cables and their types").clicked() {
                self.show_cables = !self.show_cables;
            }
            let errors = frame.checks.iter().filter(|c| c.severity == "error").count();
            let warnings = frame.checks.len() - errors;
            let label = match (errors, warnings) {
                (0, 0) => egui::RichText::new("Checks ✔"),
                (0, w) => egui::RichText::new(format!("Checks: {w} ⚠")).color(WARNING),
                (e, w) => egui::RichText::new(format!("Checks: {e} ✖ {w} ⚠")).color(ERROR),
            };
            if ui
                .selectable_label(self.show_checks, label)
                .on_hover_text("Errors and warnings in the project")
                .clicked()
            {
                self.show_checks = !self.show_checks;
            }
            ui.separator();
            let mut tool = |ui: &mut egui::Ui, label: &str, id: &str, tip: &str| {
                if ui.selectable_label(frame.tool == id, label).on_hover_text(tip).clicked() {
                    self.session.handle(UiEvent::Tool { tool: id.to_owned() });
                }
            };
            tool(ui, "Select", "select", "Click to select, drag to move (Esc)");
            tool(ui, "Wire", "wire", "Click two connection points (W)");
            tool(ui, "Box", "box", "Drag a dashed location box: +A101, EXT, a connector housing");
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
            SelectionInfo::Device { tag, description, symbol, terminal, plc, .. } => {
                format!("device {tag} {description} {symbol} {terminal:?} {plc:?}")
            }
            SelectionInfo::Wire { number, from, to, cross_section, colour, cable, core, .. } => {
                format!("wire {from} {to} {number:?} {cross_section} {colour} {cable} {core}")
            }
            SelectionInfo::Box { label, devices } => format!("box {label} {devices:?}"),
        });
        if key != self.fields.key {
            self.fields = Fields { key, ..Fields::default() };
            match &frame.selection {
                Some(SelectionInfo::Device { tag, description, plc, .. }) => {
                    self.fields.tag = tag.clone();
                    self.fields.description = description.clone();
                    if let Some(plc) = plc {
                        self.fields.number = plc.channel.clone();
                        self.fields.address = plc.address.clone();
                        self.fields.plc_symbol = plc.symbol.clone();
                        self.fields.comment = plc.comment.clone();
                    }
                }
                Some(SelectionInfo::Wire { number, cross_section, colour, cable, core, .. }) => {
                    self.fields.number = number.clone().unwrap_or_default();
                    self.fields.cross_section = cross_section.clone();
                    self.fields.colour = colour.clone();
                    self.fields.cable = cable.clone();
                    self.fields.core = core.clone();
                }
                Some(SelectionInfo::Box { label, .. }) => self.fields.tag = label.clone(),
                None => {}
            }
        }
        // A field is applied when it loses focus (Tab, Enter or a click elsewhere) with a change.
        let mut events = Vec::new();
        match &frame.selection {
            Some(SelectionInfo::Device {
                tag,
                description,
                symbol,
                role,
                references,
                parts,
                terminal,
                plc,
                location,
            }) => {
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
                if !location.is_empty() {
                    ui.small(format!("In: {location}"));
                }
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
                    "plc" => {
                        if let Some(plc) = plc {
                            ui.add_space(8.0);
                            ui.strong(format!("{} channel", plc.kind));
                            ui.label("Channel number");
                            let edit = ui.text_edit_singleline(&mut self.fields.number);
                            if edit.lost_focus() && self.fields.number.trim() != plc.channel {
                                events.push(UiEvent::RenumberTerminal { number: self.fields.number.clone() });
                            }
                            let fields = [
                                (
                                    "Address",
                                    "address",
                                    &mut self.fields.address,
                                    &plc.address,
                                    "e.g. I0.0, Q1.3, IW256",
                                ),
                                (
                                    "Symbol",
                                    "symbol",
                                    &mut self.fields.plc_symbol,
                                    &plc.symbol,
                                    "The PLC program's name",
                                ),
                                ("Comment", "comment", &mut self.fields.comment, &plc.comment, ""),
                            ];
                            for (label, key, buffer, current, hint) in fields {
                                ui.label(label);
                                let edit = ui.add(egui::TextEdit::singleline(buffer).hint_text(hint));
                                if edit.lost_focus() && buffer.trim() != current {
                                    events.push(UiEvent::SetPlcProperty { key: key.into(), value: buffer.clone() });
                                }
                            }
                            ui.add_space(4.0);
                            ui.strong(format!("Card {tag}"));
                            for r in references {
                                if ui.link(&r.label).on_hover_text("Show this page").clicked() {
                                    events.push(UiEvent::ShowPage { id: r.page });
                                }
                            }
                            for item in &frame.channel_palette {
                                let label =
                                    format!("Add {}", item.description.trim_start_matches("PLC ").to_lowercase());
                                let hint = "The card's next channel and address; then click where it goes, on any page";
                                if ui.button(label).on_hover_text(hint).clicked() {
                                    events.push(UiEvent::AddContact { symbol: item.id.clone() });
                                }
                            }
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
            Some(SelectionInfo::Wire { number, from, to, potential, warning, cross_section, colour, cable, core }) => {
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
                let f = &mut self.fields;
                let mut changed = false;
                egui::Grid::new("wire-spec").num_columns(2).show(ui, |ui| {
                    ui.label("Cross-section");
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut f.cross_section).hint_text("1,5").desired_width(80.0))
                        .lost_focus();
                    ui.end_row();
                    ui.label("Colour");
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut f.colour).hint_text("BU").desired_width(80.0))
                        .lost_focus();
                    ui.end_row();
                    ui.label("Cable");
                    egui::ComboBox::from_id_salt("wire-cable")
                        .selected_text(if f.cable.is_empty() { "none" } else { &f.cable })
                        .show_ui(ui, |ui| {
                            changed |= ui.selectable_value(&mut f.cable, String::new(), "none").clicked();
                            for c in &frame.cables {
                                changed |= ui.selectable_value(&mut f.cable, c.tag.clone(), &c.tag).clicked();
                            }
                        });
                    ui.end_row();
                    if !f.cable.is_empty() {
                        ui.label("Core");
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut f.core).hint_text("1, BN…").desired_width(80.0))
                            .lost_focus();
                        ui.end_row();
                    }
                });
                let differs = f.cross_section.trim() != cross_section
                    || f.colour.trim().to_uppercase() != *colour
                    || f.cable != *cable
                    || (!f.cable.is_empty() && f.core.trim() != core);
                // A cable needs a core before the change goes in.
                if changed && differs && (f.cable.is_empty() || !f.core.trim().is_empty()) {
                    events.push(UiEvent::WireSpec {
                        cross_section: f.cross_section.clone(),
                        colour: f.colour.clone(),
                        cable: f.cable.clone(),
                        core: f.core.clone(),
                    });
                }
                if frame.cables.is_empty() {
                    ui.small("Add cables in the Cables window.");
                }
                ui.add_space(8.0);
                ui.label("Delete removes the wire.");
            }
            Some(SelectionInfo::Box { label, devices }) => {
                ui.label("Box label");
                let edit = ui.add(egui::TextEdit::singleline(&mut self.fields.tag).hint_text("+A101, EXT, -XCPR1"));
                if edit.lost_focus() && self.fields.tag.trim() != label {
                    events.push(UiEvent::LabelBox { text: self.fields.tag.clone() });
                }
                ui.small("Where the devices inside are, or what they belong to.");
                ui.add_space(6.0);
                ui.strong("Inside");
                if devices.is_empty() {
                    ui.small("No devices yet.");
                }
                ui.label(devices.join("  "));
                ui.add_space(8.0);
                ui.label("Drag the edge to move, Delete removes.");
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

    fn checks(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        let mut events = Vec::new();
        ui.horizontal(|ui| {
            ui.strong("Checks");
            ui.small("Click one to show it on its page.");
        });
        if frame.checks.is_empty() {
            ui.label("No errors or warnings.");
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (index, c) in frame.checks.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (mark, color) = if c.severity == "error" { ("✖", ERROR) } else { ("⚠", WARNING) };
                    ui.label(egui::RichText::new(mark).color(color));
                    ui.add_sized([40.0, 16.0], egui::Label::new(&c.location));
                    if ui.link(&c.message).on_hover_text(&c.rule).clicked() {
                        events.push(UiEvent::ShowCheck { index });
                    }
                });
            }
        });
        for ev in events {
            self.send(ev);
        }
    }

    /// The symbol library, grouped by category; click a symbol, then click on the page.
    fn library(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.heading("Symbols");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.symbol_filter).hint_text("Search…").desired_width(110.0));
            if ui.button("Black box…").on_hover_text("A device drawn as a box with its own terminals").clicked() {
                self.black_box.open = true;
            }
        });
        let filter = self.symbol_filter.trim().to_lowercase();
        let mut events = Vec::new();
        egui::ScrollArea::vertical().id_salt("library").show(ui, |ui| {
            let mut categories: Vec<&str> = frame.palette.iter().map(|i| i.category.as_str()).collect();
            categories.dedup();
            for cat in categories {
                let items: Vec<_> = frame
                    .palette
                    .iter()
                    .filter(|i| i.category == cat)
                    .filter(|i| {
                        filter.is_empty() || i.description.to_lowercase().contains(&filter) || i.id.contains(&filter)
                    })
                    .collect();
                if items.is_empty() {
                    continue;
                }
                let header = egui::CollapsingHeader::new(cat).default_open(false);
                let header = if filter.is_empty() { header } else { header.open(Some(true)) };
                header.show(ui, |ui| {
                    for item in items {
                        let active = frame.tool == item.id;
                        if ui
                            .selectable_label(active, &item.description)
                            .on_hover_text("Then click on the page")
                            .clicked()
                        {
                            events.push(UiEvent::Tool { tool: item.id.clone() });
                        }
                    }
                });
            }
        });
        for ev in events {
            self.send(ev);
        }
    }

    fn black_box_window(&mut self, ctx: &egui::Context) {
        let mut open = self.black_box.open;
        let mut create = None;
        egui::Window::new("New black box").open(&mut open).default_width(420.0).show(ctx, |ui| {
            let b = &mut self.black_box;
            ui.small("For devices such as inverters, safety modules or power supplies: a box with the terminals named as on the device. Names are separated by commas.");
            egui::Grid::new("black-box").num_columns(2).show(ui, |ui| {
                let fields: [(&str, &mut String, &str); 6] = [
                    ("Description", &mut b.description, "Inverter"),
                    ("Letter code", &mut b.prefix, "-A"),
                    ("Top terminals", &mut b.top, "L1, L2, L3, PE"),
                    ("Bottom terminals", &mut b.bottom, "U, V, W"),
                    ("Left terminals", &mut b.left, ""),
                    ("Right terminals", &mut b.right, "DI0, DI1, 0V"),
                ];
                for (label, value, hint) in fields {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(280.0));
                    ui.end_row();
                }
            });
            let spec = ecad_editor::BlackBox {
                description: b.description.trim().to_owned(),
                tag_prefix: b.prefix.trim().to_owned(),
                top: names(&b.top),
                bottom: names(&b.bottom),
                left: names(&b.left),
                right: names(&b.right),
            };
            let valid = spec.is_valid();
            if !valid {
                ui.small("Give it at least one terminal; names must be unique.");
            }
            if ui.add_enabled(valid, egui::Button::new("Create and place")).clicked() {
                create = Some(spec);
            }
        });
        self.black_box.open = open && create.is_none();
        if let Some(spec) = create {
            self.send(UiEvent::CreateBlackBox { spec });
        }
    }

    fn pages(&mut self, ui: &mut egui::Ui, frame: &Frame) {
        ui.heading("Pages");
        let mut events = Vec::new();
        // Room below for the page fields and the symbol library.
        egui::ScrollArea::vertical().id_salt("pages").max_height(160.0).show(ui, |ui| {
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
            let key = Some(format!("{:?} {} {} {}", current.id, current.name, current.description, current.subtitle));
            if key != self.page_fields.key {
                self.page_fields = PageFields {
                    key,
                    name: current.name.clone(),
                    description: current.description.clone(),
                    subtitle: current.subtitle.clone(),
                };
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
            ui.label("Second line").on_hover_text("Printed under the page content, e.g. in a second language");
            if ui.text_edit_singleline(&mut self.page_fields.subtitle).lost_focus()
                && self.page_fields.subtitle.trim() != current.subtitle
            {
                events.push(UiEvent::SubtitlePage { text: self.page_fields.subtitle.clone() });
            }
            ui.small("PageUp / PageDown switch pages.");
        }
        for ev in events {
            self.send(ev);
        }
    }

    fn project_window(&mut self, ctx: &egui::Context, frame: &Frame) {
        let key = Some(format!("{} {:?} {:?} {:?}", frame.project_name, frame.info, frame.naming, frame.sheet));
        if key != self.project_fields.key {
            self.project_fields = ProjectFields {
                key,
                name: frame.project_name.clone(),
                info: frame.info.clone(),
                naming: frame.naming.clone(),
                grid: frame.sheet,
            };
        }
        let mut open = self.show_project;
        let mut events = Vec::new();
        egui::Window::new("Project settings").open(&mut open).default_size([560.0, 420.0]).show(ctx, |ui| {
            let f = &mut self.project_fields;
            ui.strong("Title block");
            egui::Grid::new("project-info").num_columns(2).show(ui, |ui| {
                let fields: [(&str, &mut String); 6] = [
                    ("Project", &mut f.name),
                    ("Customer", &mut f.info.customer),
                    ("Plant / machine", &mut f.info.plant),
                    ("Diagram code", &mut f.info.code),
                    ("Designer", &mut f.info.designer),
                    ("Date", &mut f.info.date),
                ];
                for (label, value) in fields {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).desired_width(320.0));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.strong("Revisions");
            ui.small("Newest last; the title block shows the last three.");
            let mut remove = None;
            egui::Grid::new("revisions").num_columns(5).show(ui, |ui| {
                for label in ["Rev.", "Change", "Date", "By", ""] {
                    ui.small(label);
                }
                ui.end_row();
                for (i, r) in f.info.revisions.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut r.index).desired_width(32.0));
                    ui.add(egui::TextEdit::singleline(&mut r.change).desired_width(220.0));
                    ui.add(egui::TextEdit::singleline(&mut r.date).desired_width(84.0));
                    ui.add(egui::TextEdit::singleline(&mut r.by).desired_width(48.0));
                    if ui.small_button("✕").on_hover_text("Remove this revision").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                f.info.revisions.remove(i);
            }
            if ui.button("Add revision").clicked() {
                let index = f.info.revisions.last().and_then(|r| r.index.parse::<u32>().ok()).map_or(0, |n| n + 1);
                f.info.revisions.push(ecad_editor::Revision { index: index.to_string(), ..Default::default() });
            }
            ui.add_space(4.0);
            let changed = f.name.trim() != frame.project_name || f.info != frame.info;
            ui.horizontal(|ui| {
                if ui.add_enabled(changed, egui::Button::new("Apply")).clicked() {
                    events.push(UiEvent::ProjectInfo { name: f.name.clone(), info: f.info.clone() });
                }
                if ui.add_enabled(changed, egui::Button::new("Revert")).clicked() {
                    f.key = None;
                }
            });
            ui.separator();
            ui.strong("Naming");
            egui::Grid::new("naming").num_columns(2).show(ui, |ui| {
                ui.label("Device tags");
                ui.add(egui::TextEdit::singleline(&mut f.naming.tag_format).desired_width(200.0))
                    .on_hover_text("{page} page name, {letters} the symbol's letter code, {n} the next number: -{letters}{n} gives -K1, {page}{letters}{n} gives A2SB1");
                ui.end_row();
                ui.label("Wire numbers");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.naming.wire_numbering, ecad_editor::WireNumbering::PerWire, "one per wire");
                    ui.radio_value(&mut f.naming.wire_numbering, ecad_editor::WireNumbering::PerPotential, "one per potential");
                });
                ui.end_row();
                ui.label("Wire number format");
                ui.add(egui::TextEdit::singleline(&mut f.naming.wire_format).desired_width(200.0))
                    .on_hover_text("{page} page name, {n} or {n:02} the next number: W{n} gives W1, {page}{n:02} gives P201. Per potential, a named potential (L+, +A100) uses its name");
                ui.end_row();
            });
            let naming_changed = f.naming != frame.naming;
            ui.horizontal(|ui| {
                if ui.add_enabled(naming_changed, egui::Button::new("Apply naming")).clicked() {
                    events.push(UiEvent::Naming { naming: f.naming.clone() });
                }
                if ui
                    .add_enabled(!naming_changed, egui::Button::new("Renumber all wires"))
                    .on_hover_text("Number every wire again from the format, page by page (undoable)")
                    .clicked()
                {
                    events.push(UiEvent::RenumberWires);
                }
            });
            ui.small("New devices and wires follow the naming; existing tags stay as they are.");
            ui.separator();
            ui.strong("Sheet grid");
            let grid = &mut f.grid;
            ui.horizontal(|ui| {
                ui.label("Columns");
                ui.add(egui::DragValue::new(&mut grid.columns).range(1..=50));
                ui.label("numbered from");
                egui::ComboBox::from_id_salt("first-column").selected_text(grid.first_column.to_string()).show_ui(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut grid.first_column, 0, "0");
                        ui.selectable_value(&mut grid.first_column, 1, "1");
                    },
                );
                ui.label("Rows");
                ui.add(egui::DragValue::new(&mut grid.rows).range(0..=26))
                    .on_hover_text("Lettered A, B, C…; 0 for none");
            });
            let example = if grid.rows > 0 {
                "References look like 3/2F (page 3, column 2, row F)."
            } else {
                "References look like 3.2 (page 3, column 2)."
            };
            ui.small(example);
            if ui.add_enabled(*grid != frame.sheet, egui::Button::new("Apply grid")).clicked() {
                events.push(UiEvent::SheetGrid {
                    columns: grid.columns,
                    first_column: grid.first_column,
                    rows: grid.rows,
                });
            }
        });
        self.show_project = open;
        for ev in events {
            self.send(ev);
        }
    }

    fn cables_window(&mut self, ctx: &egui::Context, frame: &Frame) {
        let mut open = self.show_cables;
        let mut events = Vec::new();
        egui::Window::new("Cables").open(&mut open).default_size([520.0, 320.0]).show(ctx, |ui| {
            ui.small("Put a wire in a cable from the wire's panel: pick the cable and type the core.");
            let ids: Vec<String> = frame.cables.iter().map(|c| format!("{:?}", c.id)).collect();
            self.cable_edits.retain(|k, _| ids.contains(k));
            egui::Grid::new("cables-grid").striped(true).num_columns(6).show(ui, |ui| {
                for h in ["Tag", "Type", "Cores", "Shield", "Wires", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for c in &frame.cables {
                    let current =
                        CableEdit { tag: c.tag.clone(), kind: c.kind.clone(), cores: c.cores, shielded: c.shielded };
                    let buf = self.cable_edits.entry(format!("{:?}", c.id)).or_insert_with(|| current.clone());
                    let t = ui.add(egui::TextEdit::singleline(&mut buf.tag).desired_width(80.0));
                    let k = ui.add(egui::TextEdit::singleline(&mut buf.kind).desired_width(140.0));
                    let n = ui.add(egui::DragValue::new(&mut buf.cores).range(0..=99));
                    let s = ui.checkbox(&mut buf.shielded, "");
                    ui.label(c.used.to_string());
                    let commit = t.lost_focus() || k.lost_focus() || n.drag_stopped() || n.lost_focus() || s.changed();
                    if commit && *buf != current {
                        events.push(UiEvent::UpdateCable {
                            id: c.id,
                            tag: buf.tag.clone(),
                            kind: buf.kind.clone(),
                            cores: buf.cores,
                            shielded: buf.shielded,
                        });
                    }
                    if ui
                        .add_enabled(c.used == 0, egui::Button::new("✕").small())
                        .on_hover_text("Remove (only when no wire uses it)")
                        .clicked()
                    {
                        events.push(UiEvent::RemoveCable { id: c.id });
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let b = &mut self.new_cable;
                ui.add(egui::TextEdit::singleline(&mut b.tag).hint_text("-WM1").desired_width(80.0));
                ui.add(egui::TextEdit::singleline(&mut b.kind).hint_text("4G2,5").desired_width(140.0));
                ui.add(egui::DragValue::new(&mut b.cores).range(0..=99).prefix("cores "));
                ui.checkbox(&mut b.shielded, "shield");
                if ui.add_enabled(!b.tag.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    events.push(UiEvent::AddCable {
                        tag: b.tag.clone(),
                        kind: b.kind.clone(),
                        cores: b.cores,
                        shielded: b.shielded,
                    });
                    *b = CableEdit::default();
                }
            });
        });
        self.show_cables = open;
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
                    .on_hover_text("Columns: part number, manufacturer, description, internal code; , or ; separated")
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
                egui::Grid::new("parts-grid").striped(true).num_columns(5).show(ui, |ui| {
                    ui.strong("Part number");
                    ui.strong("Manufacturer");
                    ui.strong("Description");
                    ui.strong("Internal code");
                    ui.label("");
                    ui.end_row();
                    for part in &frame.parts {
                        let buf = self.part_edits.entry(part.number.clone()).or_insert_with(|| {
                            (part.manufacturer.clone(), part.description.clone(), part.code.clone())
                        });
                        ui.label(&part.number);
                        let m = ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut buf.0));
                        let d = ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut buf.1));
                        let c = ui.add_sized([100.0, 20.0], egui::TextEdit::singleline(&mut buf.2));
                        if (m.lost_focus() || d.lost_focus() || c.lost_focus())
                            && (buf.0 != part.manufacturer || buf.1 != part.description || buf.2 != part.code)
                        {
                            events.push(UiEvent::UpdatePart {
                                part: ecad_editor::Part {
                                    number: part.number.clone(),
                                    manufacturer: buf.0.trim().to_owned(),
                                    description: buf.1.trim().to_owned(),
                                    code: buf.2.trim().to_owned(),
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
                let (n, m, d, c) = &mut self.new_part;
                ui.add(egui::TextEdit::singleline(n).hint_text("Part number").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(m).hint_text("Manufacturer").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(d).hint_text("Description").desired_width(240.0));
                ui.add(egui::TextEdit::singleline(c).hint_text("Internal code").desired_width(90.0));
                if ui.add_enabled(!n.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    events.push(UiEvent::AddPart {
                        part: ecad_editor::Part {
                            number: n.trim().to_owned(),
                            manufacturer: m.trim().to_owned(),
                            description: d.trim().to_owned(),
                            code: c.trim().to_owned(),
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
        if self.show_checks {
            egui::Panel::bottom("checks").resizable(true).default_size(150.0).show(ui, |ui| self.checks(ui, &before));
        }
        egui::Panel::left("pages").default_size(190.0).show(ui, |ui| {
            self.pages(ui, &before);
            ui.separator();
            self.library(ui, &before);
        });
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
            if self.show_project {
                self.project_window(&ctx, &frame);
            }
            if self.show_cables {
                self.cables_window(&ctx, &frame);
            }
            if self.black_box.open {
                self.black_box_window(&ctx);
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
                show_checks: false,
                symbol_filter: String::new(),
                show_cables: false,
                cable_edits: Default::default(),
                new_cable: CableEdit::default(),
                black_box: BoxFields::default(),
                show_project: false,
                project_fields: ProjectFields::default(),
                part_edits: Default::default(),
                new_part: Default::default(),
                title: String::new(),
                close_sent: false,
            }))
        }),
    )
}
