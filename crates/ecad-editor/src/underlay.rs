//! A drawing behind the shown page, to redraw over: import, show or hide, place, remove.

use crate::Editor;
use ecad_model::geometry::Point;
use ecad_model::underlay::{Underlay, Units, UnitsFrom};
use ecad_model::{Command, PageId};
use std::sync::Arc;

impl Editor {
    /// Put a DXF or DWG file behind `page`: read with a size limit, then as
    /// [`Editor::import_underlay`]. A file that fails leaves the page as it was.
    pub fn import_underlay_file(
        &mut self,
        page: PageId,
        path: &std::path::Path,
        units: Option<Units>,
    ) -> Result<String, String> {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = ecad_project::read_bounded(path, ecad_view::underlay::MAX_FILE).map_err(|e| e.to_string())?;
        self.import_underlay(page, &bytes, &name, units)
    }

    /// Put a DXF or DWG drawing behind `page`, replacing any there. It is read in a separate
    /// worker process when the program set one up (`ecad_view::worker`), so a hostile file
    /// can't take the editor down. `units` overrides the file's own (`$INSUNITS`); without
    /// either it is taken as millimetres, and the message says so. It is placed at its real
    /// size where it was drawn if that is on the sheet, otherwise fitted to the sheet. A file
    /// that fails leaves the page as it was. Says how it was placed.
    pub fn import_underlay(
        &mut self,
        page: PageId,
        bytes: &[u8],
        name: &str,
        units: Option<Units>,
    ) -> Result<String, String> {
        self.project.page(page).map_err(|e| e.to_string())?;
        let imported = ecad_view::worker::import_underlay(bytes, name).map_err(|e| e.to_string())?;
        let (units, units_from) = match (units, imported.units) {
            (Some(u), _) => (u, UnitsFrom::Chosen),
            (None, Some(u)) => (u, UnitsFrom::File),
            (None, None) => (Units::Millimetres, UnitsFrom::Assumed),
        };
        let sheet = self.project.sheet;
        let (lo, hi) = (sheet.origin, Point::new(sheet.origin.x + sheet.width, sheet.origin.y + sheet.height));
        let (at, scale) = Underlay::placement(&imported.shapes, imported.origin, units, lo, hi);
        let points = imported.shapes.points();
        let underlay = Underlay {
            source: name.to_owned(),
            shapes: Arc::new(imported.shapes),
            at,
            scale,
            visible: true,
            units,
            units_from,
        };
        let real = underlay.real_scale();
        let label = format!("Underlay {name}");
        self.execute(&label, Command::SetUnderlay { page, underlay: Some(Box::new(underlay)) })
            .map_err(|e| e.to_string())?;
        let how = if (real - 1.0).abs() < 1e-9 {
            "1:1 as drawn".to_owned()
        } else {
            format!("fitted to the sheet at {}", ratio(real))
        };
        let from = if imported.space == "Model" { String::new() } else { format!(" (layout {})", imported.space) };
        let unit = match units_from {
            UnitsFrom::File => format!("in {}", units.name()),
            UnitsFrom::Chosen => format!("in {} as chosen", units.name()),
            UnitsFrom::Assumed => {
                "units not given in the file, taken as mm (import again choosing its units if not)".to_owned()
            }
        };
        Ok(format!("Underlay {name}{from}: {points} points, {unit}, {how}"))
    }

    /// Show or hide `page`'s underlay.
    pub fn show_underlay(&mut self, page: PageId, visible: bool) -> Result<(), String> {
        self.change_underlay(page, if visible { "Show underlay" } else { "Hide underlay" }, |u| u.visible = visible)
    }

    /// Place `page`'s underlay: its lower-left corner at `at` mm, `scale` page mm per unit.
    pub fn place_underlay(&mut self, page: PageId, at: Point, scale: f64) -> Result<(), String> {
        self.change_underlay(page, "Place underlay", |u| {
            u.at = at;
            u.scale = scale;
        })
    }

    /// Take `page`'s underlay away.
    pub fn remove_underlay(&mut self, page: PageId) -> Result<(), String> {
        if !self.project.underlays.contains_key(&page) {
            return Err("this page has no underlay".into());
        }
        self.execute("Remove underlay", Command::SetUnderlay { page, underlay: None }).map_err(|e| e.to_string())
    }

    fn change_underlay(&mut self, page: PageId, label: &str, change: impl FnOnce(&mut Underlay)) -> Result<(), String> {
        let mut u = self.project.underlays.get(&page).cloned().ok_or("this page has no underlay")?;
        let before = u.clone();
        change(&mut u);
        if u == before {
            return Ok(());
        }
        // The shapes are shared, not copied: only the placement changes.
        self.execute(label, Command::SetUnderlay { page, underlay: Some(Box::new(u)) }).map_err(|e| e.to_string())
    }
}

/// A scale as a drawing ratio: 0.1 → `1:10`, 2 → `2:1`, 0.37 → `1:2.7`.
pub fn ratio(scale: f64) -> String {
    let short = |v: f64| {
        let r = (v * 10.0).round() / 10.0;
        if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
    };
    if scale >= 1.0 { format!("{}:1", short(scale)) } else { format!("1:{}", short(1.0 / scale)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        let p = ecad_model::sample::start_circuit().unwrap();
        Editor::new(p, ecad_model::sample::PAGE_1).unwrap()
    }

    /// One of Electric Studio's own pages as a DXF: a real schematic with blocks and text.
    fn a_schematic_dxf() -> Vec<u8> {
        let p = ecad_model::reference::reference_project().unwrap();
        let page = p.pages_in_order()[2].id;
        ecad_view::page_dxf(&p, page).unwrap().into_bytes()
    }

    #[test]
    fn a_dxf_goes_behind_the_page_and_comes_off_with_undo() {
        let mut e = editor();
        let page = e.page();
        let base = e.base([255, 255, 255]).unwrap().len();
        let message = e.import_underlay(page, &a_schematic_dxf(), "old-sheet.dxf", None).unwrap();
        // Our own A3 page lies on the sheet as drawn.
        assert!(
            message.starts_with("Underlay old-sheet.dxf: ") && message.ends_with("in mm, 1:1 as drawn"),
            "{message}"
        );
        assert!(e.base([255, 255, 255]).unwrap().len() > base + 100);
        assert!(!e.checks().iter().any(|c| c.message.contains("underlay")), "not checked");
        e.show_underlay(page, false).unwrap();
        assert_eq!(e.base([255, 255, 255]).unwrap().len(), base);
        e.show_underlay(page, true).unwrap();
        e.place_underlay(page, Point::new(5.0, 5.0), 0.5).unwrap();
        assert_eq!(e.project().underlays[&page].scale, 0.5);
        assert!(e.place_underlay(page, Point::new(5.0, 5.0), 0.0).is_err());
        e.remove_underlay(page).unwrap();
        assert!(e.remove_underlay(page).is_err());
        for _ in 0..5 {
            e.undo().unwrap();
        }
        assert!(e.project().underlays.is_empty(), "undone back to before the import");
    }

    #[test]
    fn a_drawing_bigger_than_the_sheet_is_fitted() {
        // A 10× drawing: the page drawn in cm-as-mm, say.
        let mut d = ecad_model::sample::start_circuit().unwrap();
        let mut big = d.clone();
        big.sheet.width *= 10.0;
        big.sheet.height *= 10.0;
        d.sheet = big.sheet;
        let dxf = ecad_view::page_dxf(&big, ecad_model::sample::PAGE_1).unwrap();
        let mut e = editor();
        let page = e.page();
        let message = e.import_underlay(page, dxf.as_bytes(), "big.dxf", None).unwrap();
        assert!(message.contains("fitted to the sheet at 1:"), "{message}");
        assert!(e.import_underlay(page, b"not a drawing", "x.dxf", None).is_err());
    }

    #[test]
    fn units_given_chosen_or_assumed_decide_the_size() {
        let mut e = editor();
        let page = e.page();
        // Our own DXF says mm; chosen as cm it is ten times the size, so fitted.
        let dxf = a_schematic_dxf();
        let m = e.import_underlay(page, &dxf, "a.dxf", Some(Units::Centimetres)).unwrap();
        assert!(m.contains("in cm as chosen") && m.contains("fitted to the sheet at 1:"), "{m}");
        let u = &e.project().underlays[&page];
        assert_eq!((u.units, u.units_from), (Units::Centimetres, UnitsFrom::Chosen));
        // A file that doesn't say: taken as mm, and the message says so.
        let text =
            String::from_utf8(dxf).unwrap().replacen("$INSUNITS\r\n 70\r\n4\r\n", "$INSUNITS\r\n 70\r\n0\r\n", 1);
        assert!(text.contains("$INSUNITS\r\n 70\r\n0\r\n"), "the header was changed");
        let m = e.import_underlay(page, text.as_bytes(), "b.dxf", None).unwrap();
        assert!(m.contains("units not given in the file, taken as mm"), "{m}");
        assert_eq!(e.project().underlays[&page].units_from, UnitsFrom::Assumed);
    }

    #[test]
    fn a_file_too_big_or_broken_leaves_the_page_as_it_was() {
        let mut e = editor();
        let page = e.page();
        e.import_underlay(page, &a_schematic_dxf(), "kept.dxf", None).unwrap();
        let before = e.project().clone();
        let dir = tempfile::tempdir().unwrap();
        // A sparse file just over the limit: refused before it is read.
        let huge = dir.path().join("huge.dxf");
        std::fs::File::create(&huge).unwrap().set_len(ecad_view::underlay::MAX_FILE as u64 + 1).unwrap();
        let err = e.import_underlay_file(page, &huge, None).unwrap_err();
        assert!(err.contains("too big"), "{err}");
        let junk = dir.path().join("junk.dxf");
        std::fs::write(&junk, "not a drawing").unwrap();
        assert!(e.import_underlay_file(page, &junk, None).is_err());
        assert_eq!(e.project(), &before);
        assert_eq!(e.undo_label(), Some("Underlay kept.dxf"));
    }

    #[test]
    fn scales_read_as_ratios() {
        assert_eq!(ratio(1.0), "1:1");
        assert_eq!(ratio(0.1), "1:10");
        assert_eq!(ratio(2.0), "2:1");
        assert_eq!(ratio(1.0 / 2.7), "1:2.7");
    }
}
