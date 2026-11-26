//! Panel layout: a page can show a mounting plate at scale instead of a schematic, with DIN
//! rails, cable ducts and the cabinet's devices drawn as their real footprints.
//!
//! Coordinates on a plate are millimetres from its lower-left corner. A device's footprint comes
//! from the sizes of its catalogue parts (see [`Project::device_size`]).

use crate::geometry::Point;
use crate::project::{DeviceId, PageId, PanelItemId, Project};
use crate::symbol::Role;
use serde::{Deserialize, Serialize};

/// Width of a TS 35 DIN rail, in mm.
pub const RAIL_WIDTH: f64 = 35.0;
/// The footprint drawn for a device whose parts have no size, in mm (one module wide).
pub const UNKNOWN_SIZE: (f64, f64) = (18.0, 45.0);

/// The mounting plate shown on a layout page.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    /// Plate width and height in mm.
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PanelItemKind {
    /// A DIN rail (TS 35).
    Rail,
    /// A cable duct, `width` across and `depth` deep (`60 x 80`).
    Duct { width: f64, depth: f64 },
    /// A device's footprint.
    Device { device: DeviceId },
}

/// Something on a mounting plate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelItem {
    pub id: PanelItemId,
    pub page: PageId,
    pub kind: PanelItemKind,
    /// Lower-left corner on the plate, mm.
    pub at: Point,
    /// Length of a rail or duct, mm (devices take theirs from their parts).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub length: f64,
    /// Runs up the plate instead of across (a device is turned a quarter).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical: bool,
    /// The catalogue part of a rail or duct, for the cut list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub part: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Project {
    /// The items on a layout page: ducts first, then rails, then devices (the order they are
    /// drawn in, each kind by position).
    pub fn panel_items_on(&self, page: PageId) -> Vec<&PanelItem> {
        let mut items: Vec<&PanelItem> = self.panel_items.values().filter(|i| i.page == page).collect();
        let rank = |i: &PanelItem| match i.kind {
            PanelItemKind::Duct { .. } => 0,
            PanelItemKind::Rail => 1,
            PanelItemKind::Device { .. } => 2,
        };
        items.sort_by(|a, b| {
            (rank(a), -a.at.y, a.at.x).partial_cmp(&(rank(b), -b.at.y, b.at.x)).unwrap_or(std::cmp::Ordering::Equal)
        });
        items
    }

    /// A device's footprint, width × height in mm, and whether it is known: the widths of its
    /// parts that have a size, each counted as the BOM counts it (a strip's terminal once per
    /// terminal), side by side, as high as the highest. Without sized parts, [`UNKNOWN_SIZE`].
    pub fn device_size(&self, device: DeviceId) -> ((f64, f64), bool) {
        let Ok(d) = self.device(device) else { return (UNKNOWN_SIZE, false) };
        let terminals: Vec<_> =
            self.functions_of(device).filter(|f| self.role(f.id).is_ok_and(|r| r == Role::Terminal)).collect();
        let (mut width, mut height) = (0.0, 0.0f64);
        for r in &d.parts {
            let Some(part) = self.parts.get(&r.number) else { continue };
            let (Some(w), Some(h)) = (part.width, part.height) else { continue };
            let per = match (&r.only, r.per_strip) {
                (_, true) => 1,
                (Some(symbol), false) => terminals.iter().filter(|f| f.symbol == *symbol).count(),
                (None, false) => terminals.len().max(1),
            };
            width += w * f64::from(r.quantity) * per as f64;
            height = height.max(h);
        }
        if width > 0.0 && height > 0.0 { ((width, height), true) } else { (UNKNOWN_SIZE, false) }
    }

    /// An item's box on its plate (`min`, `max`), in mm.
    pub fn panel_item_rect(&self, item: &PanelItem) -> (Point, Point) {
        let (along, across) = match &item.kind {
            PanelItemKind::Rail => (item.length, RAIL_WIDTH),
            PanelItemKind::Duct { width, .. } => (item.length, *width),
            PanelItemKind::Device { device } => self.device_size(*device).0,
        };
        let (w, h) = if item.vertical { (across, along) } else { (along, across) };
        (item.at, Point::new(item.at.x + w, item.at.y + h))
    }

    /// The layout item that places a device, if it is on a plate.
    pub fn panel_item_of(&self, device: DeviceId) -> Option<&PanelItem> {
        self.panel_items.values().find(|i| i.kind == PanelItemKind::Device { device })
    }

    /// Devices that belong in the cabinet's BOM (they have parts and a real symbol) but are on
    /// no plate yet, by tag. Field devices are among them until placed or left out on purpose:
    /// the list is for the layout editor, not a check.
    pub fn unplaced_devices(&self) -> Vec<DeviceId> {
        let mut out: Vec<&crate::Device> = self
            .devices
            .values()
            .filter(|d| !d.parts.is_empty() && self.needs_part(d.id) && self.panel_item_of(d.id).is_none())
            .collect();
        out.sort_by(|a, b| a.tag.cmp(&b.tag));
        out.into_iter().map(|d| d.id).collect()
    }

    /// Pairs of device footprints on a page that overlap (touching is fine).
    pub fn panel_overlaps(&self, page: PageId) -> Vec<(PanelItemId, PanelItemId)> {
        let devices: Vec<(PanelItemId, (Point, Point))> = self
            .panel_items_on(page)
            .into_iter()
            .filter(|i| matches!(i.kind, PanelItemKind::Device { .. }))
            .map(|i| (i.id, self.panel_item_rect(i)))
            .collect();
        let mut out = Vec::new();
        for (k, (a, (alo, ahi))) in devices.iter().enumerate() {
            for (b, (blo, bhi)) in &devices[k + 1..] {
                let eps = 0.01;
                if alo.x + eps < bhi.x && blo.x + eps < ahi.x && alo.y + eps < bhi.y && blo.y + eps < ahi.y {
                    out.push((*a, *b));
                }
            }
        }
        out
    }

    /// Items on a page that stick out of its plate.
    pub fn panel_off_plate(&self, page: PageId) -> Vec<PanelItemId> {
        let Some(panel) = self.panels.get(&page) else { return Vec::new() };
        self.panel_items_on(page)
            .into_iter()
            .filter(|i| {
                let (lo, hi) = self.panel_item_rect(i);
                let eps = 0.01;
                lo.x < -eps || lo.y < -eps || hi.x > panel.width + eps || hi.y > panel.height + eps
            })
            .map(|i| i.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Command, ModelError};
    use crate::{Part, PartRef};

    fn part(number: &str, size: Option<(f64, f64)>) -> Part {
        Part {
            number: number.into(),
            manufacturer: String::new(),
            description: String::new(),
            code: String::new(),
            contacts: Vec::new(),
            width: size.map(|s| s.0),
            height: size.map(|s| s.1),
            depth: None,
        }
    }

    #[test]
    fn a_layout_page_holds_rails_ducts_and_device_footprints() {
        let mut p = crate::sample::with_terminals().unwrap();
        let page = crate::sample::PAGE_1;
        let k1 = p.function(crate::sample::K1_COIL).unwrap().device;
        let strip = p.function(crate::sample::X2_1).unwrap().device;
        let item = |kind: PanelItemKind, x: f64, y: f64, length: f64| PanelItem {
            id: PanelItemId::new(),
            page,
            kind,
            at: Point::new(x, y),
            length,
            vertical: false,
            part: String::new(),
        };
        // Items need the page's plate first.
        assert!(matches!(
            p.apply(Command::AddPanelItem(item(PanelItemKind::Rail, 0.0, 0.0, 100.0))),
            Err(ModelError::NoPanel(_))
        ));
        let undo_panel =
            p.apply(Command::SetPanel { page, panel: Some(Panel { width: 300.0, height: 200.0 }) }).unwrap();
        assert!(matches!(
            p.apply(Command::AddPanelItem(item(PanelItemKind::Rail, 0.0, 0.0, 0.0))),
            Err(ModelError::InvalidPanelSize)
        ));

        // Sizes from the parts: the contactor 45 x 77; the strip's two terminals 5.1 each plus
        // end brackets once for the strip.
        for (n, size) in
            [("CT", Some((45.0, 77.0))), ("TB", Some((5.1, 60.0))), ("EB", Some((8.5, 46.0))), ("NS", None)]
        {
            p.apply(Command::AddPart(part(n, size))).unwrap();
        }
        p.apply(Command::SetDeviceParts { id: k1, parts: vec![PartRef::new("CT", 1), PartRef::new("NS", 1)] }).unwrap();
        p.apply(Command::SetDeviceParts { id: strip, parts: vec![PartRef::new("TB", 1), PartRef::per_strip("EB", 2)] })
            .unwrap();
        assert_eq!(p.device_size(k1), ((45.0, 77.0), true));
        let ((w, h), known) = p.device_size(strip);
        assert!((w - (2.0 * 5.1 + 17.0)).abs() < 1e-9 && h == 60.0 && known);

        let rail = item(PanelItemKind::Rail, 10.0, 80.0, 280.0);
        let k1_item = item(PanelItemKind::Device { device: k1 }, 20.0, 60.0, 0.0);
        let strip_item = item(PanelItemKind::Device { device: strip }, 70.0, 70.0, 0.0);
        for i in [&rail, &k1_item, &strip_item] {
            p.apply(Command::AddPanelItem(i.clone())).unwrap();
        }
        assert_eq!(p.panel_item_rect(&rail), (Point::new(10.0, 80.0), Point::new(290.0, 80.0 + RAIL_WIDTH)));
        assert!(p.panel_overlaps(page).is_empty(), "side by side");
        assert!(p.panel_off_plate(page).is_empty());
        // A device is on one plate position only.
        let twice = item(PanelItemKind::Device { device: k1 }, 200.0, 60.0, 0.0);
        assert!(matches!(p.apply(Command::AddPanelItem(twice)), Err(ModelError::AlreadyOnPanel(_))));
        // Moved onto -K1 and partly off the plate: both found; undo puts it back.
        let undo =
            p.apply(Command::UpdatePanelItem(PanelItem { at: Point::new(30.0, 170.0), ..strip_item.clone() })).unwrap();
        assert!(p.panel_overlaps(page).is_empty(), "above -K1 now");
        assert_eq!(p.panel_off_plate(page), [strip_item.id]);
        p.apply(undo).unwrap();
        p.apply(Command::UpdatePanelItem(PanelItem { at: Point::new(40.0, 60.0), ..strip_item.clone() })).unwrap();
        assert_eq!(p.panel_overlaps(page).len(), 1);

        // Placed devices aren't listed as unplaced; a placed device or a plate with items can't
        // simply go.
        assert!(!p.unplaced_devices().contains(&k1));
        assert!(matches!(p.apply(Command::SetPanel { page, panel: None }), Err(ModelError::PanelNotEmpty(_))));
        for i in [&rail, &k1_item, &strip_item] {
            p.apply(Command::RemovePanelItem(i.id)).unwrap();
        }
        p.apply(undo_panel).unwrap();
        assert!(p.panels.is_empty());
    }
}
