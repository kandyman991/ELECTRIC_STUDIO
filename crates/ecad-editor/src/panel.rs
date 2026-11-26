//! Editing a layout page: selecting and dragging rails, ducts and device footprints (devices snap
//! onto rails), placing devices that aren't on a plate yet, and drawing rails and ducts.
//!
//! The pointer arrives in paper millimetres like everywhere else; the page's [`PanelFrame`]
//! turns it into plate millimetres.

use crate::{Editor, Hit, Overlay, Selected, Tool};
use ecad_model::panel::{PANEL_GRID, PanelFrame, RAIL_WIDTH};
use ecad_model::{Command, DeviceId, ModelError, Page, PageId, Panel, PanelItem, PanelItemId, PanelItemKind, Point};

/// Width of a duct drawn with the Duct tool, mm (changed in the side panel afterwards).
pub const DUCT_WIDTH: f64 = 60.0;
const DUCT_DEPTH: f64 = 80.0;
/// The shortest rail or duct the tools draw, mm.
const MIN_LENGTH: f64 = 20.0;

fn snap(p: Point) -> Point {
    Point::new((p.x / PANEL_GRID).round() * PANEL_GRID, (p.y / PANEL_GRID).round() * PANEL_GRID)
}

impl Editor {
    pub(crate) fn panel_frame(&self) -> Option<PanelFrame> {
        self.project.panel_frame(self.page)
    }

    /// Whether the page shown is a layout page.
    pub fn on_layout_page(&self) -> bool {
        self.project.panels.contains_key(&self.page)
    }

    /// An item's box on paper.
    pub fn panel_item_box(&self, id: PanelItemId) -> Option<(Point, Point)> {
        let frame = self.panel_frame()?;
        let item = self.project.panel_items.get(&id)?;
        let (lo, hi) = self.project.panel_item_rect(item);
        Some((frame.to_paper(lo), frame.to_paper(hi)))
    }

    /// What is under the pointer on a layout page: a device before the rail it sits on, a rail
    /// before the duct behind it.
    pub(crate) fn panel_hit(&self, p: Point) -> Option<Hit> {
        let tol = self.hit_tolerance;
        let items = self.project.panel_items_on(self.page);
        items.iter().rev().find_map(|i| {
            let (lo, hi) = self.panel_item_box(i.id)?;
            (p.x >= lo.x - tol && p.x <= hi.x + tol && p.y >= lo.y - tol && p.y <= hi.y + tol)
                .then_some(Hit::PanelItem(i.id))
        })
    }

    /// Pointer down on a layout page; `true` when handled here.
    pub(crate) fn panel_pointer_down(&mut self, p: Point, hit: &Option<Hit>) -> bool {
        let Some(frame) = self.panel_frame() else { return false };
        let at = frame.to_plate(p);
        match self.tool.clone() {
            Tool::Select => match hit {
                Some(Hit::PanelItem(id)) => {
                    self.selection = Some(Selected::PanelItem(*id));
                    if let Some(item) = self.project.panel_items.get(id) {
                        let grab = Point::new(at.x - item.at.x, at.y - item.at.y);
                        self.panel_drag = Some((item.clone(), grab, false));
                    }
                    true
                }
                _ => false,
            },
            Tool::PlaceOnPanel(device) => {
                self.place_on_panel(device, at);
                true
            }
            Tool::Rail | Tool::Duct => {
                self.panel_start = Some(snap(at));
                true
            }
            _ => false,
        }
    }

    /// Pointer moved while dragging a panel item; `true` when handled here.
    pub(crate) fn panel_pointer_move(&mut self, p: Point) -> bool {
        let Some((from, grab, _)) = self.panel_drag.clone() else { return false };
        let Some(frame) = self.panel_frame() else { return false };
        let at = frame.to_plate(p);
        let corner = Point::new(at.x - grab.x, at.y - grab.y);
        let to = match from.kind {
            PanelItemKind::Device { device } => self.project.snap_on_panel(self.page, device, corner, from.vertical),
            _ => snap(corner),
        };
        let now = self.project.panel_items.get(&from.id).map(|i| i.at);
        if now != Some(to) && self.project.apply(Command::UpdatePanelItem(PanelItem { at: to, ..from.clone() })).is_ok()
        {
            self.panel_drag = Some((from, grab, true));
            self.revision += 1;
            self.scene += 1;
        }
        true
    }

    /// Pointer up: finish a drag (one undo step) or draw the rail or duct.
    pub(crate) fn panel_pointer_up(&mut self, p: Point) {
        if let Some((from, _, moved)) = self.panel_drag.take()
            && moved
        {
            let label = format!("Move {}", self.panel_item_name(&from));
            self.forget_saved_branch();
            self.history.record(&label, Command::UpdatePanelItem(from));
        }
        let Some(start) = self.panel_start.take() else { return };
        let Some(frame) = self.panel_frame() else { return };
        let end = snap(frame.to_plate(p));
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        let vertical = dy.abs() > dx.abs();
        let length = dx.abs().max(dy.abs());
        if length < MIN_LENGTH {
            self.status = "Drag along the plate to draw it".into();
            return;
        }
        let (kind, across) = match self.tool {
            Tool::Duct => (PanelItemKind::Duct { width: DUCT_WIDTH, depth: DUCT_DEPTH }, DUCT_WIDTH),
            _ => (PanelItemKind::Rail, RAIL_WIDTH),
        };
        // The drag is the item's centre line.
        let at = if vertical {
            Point::new(start.x - across / 2.0, start.y.min(end.y))
        } else {
            Point::new(start.x.min(end.x), start.y - across / 2.0)
        };
        let item =
            PanelItem { id: PanelItemId::new(), page: self.page, kind, at, length, vertical, part: String::new() };
        let id = item.id;
        let label = format!("Draw {}", self.panel_item_name(&item));
        match self.execute(&label, Command::AddPanelItem(item)) {
            Ok(()) => self.selection = Some(Selected::PanelItem(id)),
            Err(e) => self.status = e.to_string(),
        }
    }

    /// Put a device's footprint on the plate with its middle at `at` (snapped onto a rail), select
    /// it and go back to Select.
    pub(crate) fn place_on_panel(&mut self, device: DeviceId, at: Point) {
        let (w, h) = self.project.device_size(device).0;
        let corner = Point::new(at.x - w / 2.0, at.y - h / 2.0);
        let at = self.project.snap_on_panel(self.page, device, corner, false);
        let item = PanelItem {
            id: PanelItemId::new(),
            page: self.page,
            kind: PanelItemKind::Device { device },
            at,
            length: 0.0,
            vertical: false,
            part: String::new(),
        };
        let id = item.id;
        let label = format!("Place {}", self.panel_item_name(&item));
        match self.execute(&label, Command::AddPanelItem(item)) {
            Ok(()) => {
                self.selection = Some(Selected::PanelItem(id));
                self.tool = Tool::Select;
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    /// Delete the selected panel item (a device goes back to the unplaced list).
    pub(crate) fn delete_panel_item(&mut self, id: PanelItemId) {
        let Some(item) = self.project.panel_items.get(&id) else { return };
        let label = format!("Remove {}", self.panel_item_name(item));
        let r = self.execute(&label, Command::RemovePanelItem(id));
        self.report(r);
    }

    /// Turn the selected panel item a quarter (across ↔ upright), about its middle.
    pub(crate) fn turn_panel_item(&mut self, id: PanelItemId) {
        let Some(item) = self.project.panel_items.get(&id).cloned() else { return };
        let (lo, hi) = self.project.panel_item_rect(&item);
        let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        let at = snap(Point::new(mid.x - h / 2.0, mid.y - w / 2.0));
        let label = format!("Turn {}", self.panel_item_name(&item));
        let r = self.execute(&label, Command::UpdatePanelItem(PanelItem { at, vertical: !item.vertical, ..item }));
        self.report(r);
    }

    /// Change the selected rail's or duct's length, a duct's width and depth, the part number.
    pub fn set_selected_panel_item(
        &mut self,
        length: f64,
        width: f64,
        depth: f64,
        part: &str,
    ) -> Result<(), ModelError> {
        let Some(Selected::PanelItem(id)) = self.selection else { return Ok(()) };
        let Some(item) = self.project.panel_items.get(&id).cloned() else { return Ok(()) };
        let kind = match item.kind {
            PanelItemKind::Duct { .. } => PanelItemKind::Duct { width, depth },
            ref k => k.clone(),
        };
        let length = if matches!(kind, PanelItemKind::Device { .. }) { item.length } else { length };
        let new = PanelItem { kind, length, part: part.trim().to_owned(), ..item.clone() };
        if new == item {
            return Ok(());
        }
        let label = format!("Change {}", self.panel_item_name(&item));
        self.execute(&label, Command::UpdatePanelItem(new))
    }

    /// Add a layout page after the current one, with a plate of this size, and show it.
    pub fn add_layout_page(&mut self, width: f64, height: f64) -> Result<(), ModelError> {
        let order: Vec<PageId> = self.project.pages_in_order().iter().map(|p| p.id).collect();
        let at = order.iter().position(|p| *p == self.page).map_or(order.len(), |i| i + 1);
        let name = (1..).map(|n| format!("L{n}")).find(|n| !self.project.pages.values().any(|p| &p.name == n));
        let mut page = Page::new(&name.unwrap_or_default(), 0);
        page.description = "Panel layout".into();
        let id = page.id;
        let mut new_order = order;
        new_order.insert(at, id);
        let label = format!("Add layout page {}", page.name);
        let panel = Some(Panel { width, height });
        self.execute(
            &label,
            Command::Batch(vec![
                Command::AddPage(page),
                Command::ReorderPages(new_order),
                Command::SetPanel { page: id, panel },
            ]),
        )?;
        self.set_page(id)
    }

    /// Change the current layout page's plate size.
    pub fn set_panel_size(&mut self, width: f64, height: f64) -> Result<(), ModelError> {
        if !self.on_layout_page() {
            return Ok(());
        }
        let panel = Some(Panel { width, height });
        if self.project.panels.get(&self.page).copied() == panel {
            return Ok(());
        }
        self.execute("Change plate size", Command::SetPanel { page: self.page, panel })
    }

    /// The commands that take a page's plate and items away, and the footprints of `devices`
    /// on any plate (for deleting a page with the devices drawn only there).
    pub(crate) fn clear_panel_commands(&self, page: PageId, devices: &[DeviceId]) -> Vec<Command> {
        let mut cmds: Vec<Command> = self
            .project
            .panel_items
            .values()
            .filter(|i| {
                i.page == page || matches!(i.kind, PanelItemKind::Device { device } if devices.contains(&device))
            })
            .map(|i| Command::RemovePanelItem(i.id))
            .collect();
        if self.project.panels.contains_key(&page) {
            cmds.push(Command::SetPanel { page, panel: None });
        }
        cmds
    }

    /// `-K1`, `rail`, `duct 60 x 80`.
    pub(crate) fn panel_item_name(&self, item: &PanelItem) -> String {
        match &item.kind {
            PanelItemKind::Device { device } => self.project.device(*device).map(|d| d.tag.clone()).unwrap_or_default(),
            PanelItemKind::Rail => "rail".into(),
            PanelItemKind::Duct { width, depth } => format!("duct {width} x {depth}"),
        }
    }

    /// Outlines of the hovered and selected items, the footprint about to be placed, the rail or
    /// duct being drawn.
    pub(crate) fn panel_overlays(&self, out: &mut Vec<Overlay>) {
        let Some(frame) = self.panel_frame() else { return };
        if let (Some(Hit::PanelItem(id)), Tool::Select) = (&self.hover, &self.tool)
            && let Some((min, max)) = self.panel_item_box(*id)
        {
            out.push(Overlay::Outline { min, max, selected: false });
        }
        if let Some(Selected::PanelItem(id)) = self.selection
            && let Some((min, max)) = self.panel_item_box(id)
        {
            out.push(Overlay::Outline { min, max, selected: true });
        }
        let rect = |lo: Point, hi: Point| {
            let (a, b) = (frame.to_paper(lo), frame.to_paper(hi));
            let c = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y), a];
            c.windows(2).map(|s| (s[0], s[1])).collect::<Vec<_>>()
        };
        match &self.tool {
            Tool::PlaceOnPanel(device) => {
                let (w, h) = self.project.device_size(*device).0;
                let at = frame.to_plate(self.cursor);
                let lo =
                    self.project.snap_on_panel(self.page, *device, Point::new(at.x - w / 2.0, at.y - h / 2.0), false);
                out.push(Overlay::Ghost { lines: rect(lo, Point::new(lo.x + w, lo.y + h)) });
            }
            Tool::Rail | Tool::Duct => {
                if let Some(start) = self.panel_start {
                    let end = snap(frame.to_plate(self.cursor));
                    let across = if matches!(self.tool, Tool::Duct) { DUCT_WIDTH } else { RAIL_WIDTH } / 2.0;
                    let (lo, hi) = if (end.y - start.y).abs() > (end.x - start.x).abs() {
                        (
                            Point::new(start.x - across, start.y.min(end.y)),
                            Point::new(start.x + across, start.y.max(end.y)),
                        )
                    } else {
                        (
                            Point::new(start.x.min(end.x), start.y - across),
                            Point::new(start.x.max(end.x), start.y + across),
                        )
                    };
                    out.push(Overlay::Ghost { lines: rect(lo, hi) });
                }
            }
            _ => {}
        }
    }
}
