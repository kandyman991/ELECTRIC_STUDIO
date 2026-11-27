//! An underlay: an existing drawing (from a DXF or DWG file) shown faintly behind a page, to
//! redraw a schematic over it. It is reference only: nothing snaps or connects to it, it isn't
//! checked, and the page's exports leave it out.
//!
//! Its geometry is flattened once, on import, into polylines and filled triangles (text and
//! blocks included), in the source drawing's units with its lower-left corner at the origin.
//! It is placed on the page by a position and a scale. The geometry is shared, so copying a
//! project (as undo and trial edits do) doesn't copy it.

use crate::geometry::Point;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Most points an underlay may have: a large drawing flattened, not a city map.
pub const MAX_POINTS: usize = 2_000_000;

/// What an underlay draws, in source units from the source's lower-left corner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnderlayShapes {
    /// Open or closed polylines (closed ones repeat their first point).
    pub lines: Vec<Vec<Point>>,
    /// Filled triangles: filled text, solids, hatches.
    pub fills: Vec<[Point; 3]>,
    /// The source drawing's size.
    pub width: f64,
    pub height: f64,
}

impl UnderlayShapes {
    pub fn points(&self) -> usize {
        self.lines.iter().map(Vec::len).sum::<usize>() + 3 * self.fills.len()
    }

    /// Every point a finite number, within the size, and not too many.
    pub fn is_valid(&self) -> bool {
        let fits = |p: &Point| p.x.is_finite() && p.y.is_finite();
        self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
            && self.points() <= MAX_POINTS
            && self.lines.iter().flatten().all(fits)
            && self.fills.iter().flatten().all(fits)
    }
}

/// An underlay on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
    /// The file it came from (its name only), to show and to import again.
    pub source: String,
    pub shapes: Arc<UnderlayShapes>,
    /// Where the source's lower-left corner is on the page, mm.
    pub at: Point,
    /// Page mm per source unit.
    pub scale: f64,
    /// Shown behind the page, or hidden for now.
    pub visible: bool,
}

impl Underlay {
    /// A point of the shapes on the page.
    pub fn to_page(&self, p: Point) -> Point {
        Point::new(self.at.x + p.x * self.scale, self.at.y + p.y * self.scale)
    }

    pub fn is_valid(&self) -> bool {
        self.scale.is_finite()
            && self.scale > 0.0
            && self.at.x.is_finite()
            && self.at.y.is_finite()
            && self.shapes.is_valid()
    }

    /// The scale and position that fit `shapes` inside `lo`–`hi`, centred, keeping its
    /// proportions; 1:1 at its own place if it already fits there as drawn (`origin` is where
    /// the source's lower-left corner was in its own coordinates).
    pub fn placement(shapes: &UnderlayShapes, origin: Point, lo: Point, hi: Point) -> (Point, f64) {
        let fits_as_drawn = origin.x >= lo.x - 1e-6
            && origin.y >= lo.y - 1e-6
            && origin.x + shapes.width <= hi.x + 1e-6
            && origin.y + shapes.height <= hi.y + 1e-6;
        if fits_as_drawn {
            return (origin, 1.0);
        }
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        let scale = match (shapes.width > 0.0, shapes.height > 0.0) {
            (true, true) => (w / shapes.width).min(h / shapes.height),
            (true, false) => w / shapes.width,
            (false, true) => h / shapes.height,
            (false, false) => 1.0,
        };
        let at = Point::new(lo.x + (w - shapes.width * scale) / 2.0, lo.y + (h - shapes.height * scale) / 2.0);
        (at, scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, ModelError};

    fn square(size: f64) -> UnderlayShapes {
        let s = size;
        UnderlayShapes {
            lines: vec![vec![
                Point::new(0.0, 0.0),
                Point::new(s, 0.0),
                Point::new(s, s),
                Point::new(0.0, s),
                Point::new(0.0, 0.0),
            ]],
            fills: vec![[Point::new(0.0, 0.0), Point::new(1.0, 0.0), Point::new(0.0, 1.0)]],
            width: s,
            height: s,
        }
    }

    #[test]
    fn a_drawing_that_fits_stays_as_drawn_and_a_big_one_is_fitted() {
        let (lo, hi) = (Point::new(0.0, 0.0), Point::new(420.0, 297.0));
        assert_eq!(Underlay::placement(&square(100.0), Point::new(10.0, 20.0), lo, hi), (Point::new(10.0, 20.0), 1.0));
        let (at, scale) = Underlay::placement(&square(2970.0), Point::new(0.0, 0.0), lo, hi);
        assert!((scale - 0.1).abs() < 1e-9);
        assert!((at.x - (420.0 - 297.0) / 2.0).abs() < 1e-9 && at.y.abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn an_underlay_is_set_undone_and_checked() {
        let mut p = crate::sample::start_circuit().unwrap();
        let page = crate::sample::PAGE_1;
        let u = Underlay {
            source: "old.dxf".into(),
            shapes: Arc::new(square(50.0)),
            at: Point::new(0.0, 0.0),
            scale: 2.0,
            visible: true,
        };
        assert_eq!(u.to_page(Point::new(10.0, 5.0)), Point::new(20.0, 10.0));
        let undo = p.apply(Command::SetUnderlay { page, underlay: Some(Box::new(u.clone())) }).unwrap();
        assert_eq!(p.underlays.get(&page), Some(&u));
        p.apply(undo).unwrap();
        assert!(p.underlays.is_empty());
        let bad = Underlay { scale: 0.0, ..u.clone() };
        assert!(matches!(
            p.apply(Command::SetUnderlay { page, underlay: Some(Box::new(bad)) }),
            Err(ModelError::InvalidUnderlay)
        ));
        let mut nan = square(1.0);
        nan.lines[0][1].x = f64::NAN;
        let bad = Underlay { shapes: Arc::new(nan), ..u };
        assert!(p.apply(Command::SetUnderlay { page, underlay: Some(Box::new(bad)) }).is_err());
    }

    #[test]
    fn removing_a_page_with_an_underlay_is_refused() {
        let mut p = crate::sample::start_circuit().unwrap();
        let page = crate::Page::new("U", 5);
        let id = page.id;
        p.apply(Command::AddPage(page)).unwrap();
        let u = Underlay {
            source: "a.dxf".into(),
            shapes: Arc::new(square(1.0)),
            at: Point::new(0.0, 0.0),
            scale: 1.0,
            visible: true,
        };
        p.apply(Command::SetUnderlay { page: id, underlay: Some(Box::new(u)) }).unwrap();
        assert!(matches!(p.apply(Command::RemovePage(id)), Err(ModelError::PageNotEmpty(_))));
        p.apply(Command::SetUnderlay { page: id, underlay: None }).unwrap();
        p.apply(Command::RemovePage(id)).unwrap();
    }
}
