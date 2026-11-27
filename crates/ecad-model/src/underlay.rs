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
/// Largest source drawing size, in its units: real drawings can have a stray object kilometres
/// off (AutoCAD's own samples do); the limit that matters is where it lands on the page.
pub const MAX_SIZE: f64 = 1e9;
/// Farthest from the page origin an underlay may reach once placed, mm (10 km).
pub const MAX_PAGE_COORD: f64 = 1e7;
/// How far a point may lie outside the drawing's declared size: shapes are rounded to a
/// thousandth of a unit.
const ROUNDING: f64 = 0.002;

/// The unit a drawing is in: from the file (DXF `$INSUNITS`), chosen on import, or, for a
/// file that doesn't say, assumed to be millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Millimetres,
    Centimetres,
    Metres,
    Inches,
    Feet,
}

impl Units {
    /// Millimetres in one unit.
    pub fn mm(self) -> f64 {
        match self {
            Units::Millimetres => 1.0,
            Units::Centimetres => 10.0,
            Units::Metres => 1000.0,
            Units::Inches => 25.4,
            Units::Feet => 304.8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Units::Millimetres => "mm",
            Units::Centimetres => "cm",
            Units::Metres => "m",
            Units::Inches => "inch",
            Units::Feet => "feet",
        }
    }

    pub const ALL: [Units; 5] = [Units::Millimetres, Units::Centimetres, Units::Metres, Units::Inches, Units::Feet];

    /// `mm`, `cm`, `m`, `in`/`inch`, `ft`/`feet`.
    pub fn parse(s: &str) -> Option<Units> {
        match s.trim().to_lowercase().as_str() {
            "mm" => Some(Units::Millimetres),
            "cm" => Some(Units::Centimetres),
            "m" => Some(Units::Metres),
            "in" | "inch" | "inches" => Some(Units::Inches),
            "ft" | "feet" | "foot" => Some(Units::Feet),
            _ => None,
        }
    }

    /// A DXF `$INSUNITS` code, if it is one of these units (0, unitless, and the rest: `None`).
    pub fn from_insunits(code: i64) -> Option<Units> {
        match code {
            1 => Some(Units::Inches),
            2 => Some(Units::Feet),
            4 => Some(Units::Millimetres),
            5 => Some(Units::Centimetres),
            6 => Some(Units::Metres),
            _ => None,
        }
    }
}

/// Where an underlay's units came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UnitsFrom {
    /// The file says so.
    File,
    /// Chosen on import.
    Chosen,
    /// The file doesn't say; taken as millimetres.
    #[default]
    Assumed,
}

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

    /// A size up to [`MAX_SIZE`], at most [`MAX_POINTS`] points, every polyline at least two
    /// points, and every point a finite number within the size.
    pub fn is_valid(&self) -> bool {
        let size_ok = |v: f64| v.is_finite() && (0.0..=MAX_SIZE).contains(&v);
        let inside = |p: &Point| {
            p.x.is_finite()
                && p.y.is_finite()
                && (-ROUNDING..=self.width + ROUNDING).contains(&p.x)
                && (-ROUNDING..=self.height + ROUNDING).contains(&p.y)
        };
        size_ok(self.width)
            && size_ok(self.height)
            && self.points() <= MAX_POINTS
            && self.lines.iter().all(|l| l.len() >= 2)
            && self.lines.iter().flatten().all(inside)
            && self.fills.iter().flatten().all(inside)
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
    /// The source's unit, so the scale can be told as the drawing's real one.
    #[serde(default)]
    pub units: Units,
    #[serde(default)]
    pub units_from: UnitsFrom,
}

impl Underlay {
    /// A point of the shapes on the page.
    pub fn to_page(&self, p: Point) -> Point {
        Point::new(self.at.x + p.x * self.scale, self.at.y + p.y * self.scale)
    }

    /// A scale above 0, its shapes valid, and placed within [`MAX_PAGE_COORD`] of the page
    /// origin, corner to corner.
    pub fn is_valid(&self) -> bool {
        let near = |v: f64| v.is_finite() && v.abs() <= MAX_PAGE_COORD;
        let far = self.to_page(Point::new(self.shapes.width, self.shapes.height));
        self.scale.is_finite()
            && self.scale > 0.0
            && near(self.at.x)
            && near(self.at.y)
            && near(far.x)
            && near(far.y)
            && self.shapes.is_valid()
    }

    /// The drawing's own scale as placed: 1.0 when one millimetre of the drawing is one on the
    /// page, whatever its units.
    pub fn real_scale(&self) -> f64 {
        self.scale / self.units.mm()
    }

    /// The scale (page mm per source unit) and position that put `shapes` inside `lo`–`hi`:
    /// at its real size where it was drawn if it fits there (`origin` is where the source's
    /// lower-left corner was, in its own units), otherwise fitted, centred, proportions kept.
    pub fn placement(shapes: &UnderlayShapes, origin: Point, units: Units, lo: Point, hi: Point) -> (Point, f64) {
        let mm = units.mm();
        let at = Point::new(origin.x * mm, origin.y * mm);
        let fits_as_drawn = at.x >= lo.x - 1e-6
            && at.y >= lo.y - 1e-6
            && at.x + shapes.width * mm <= hi.x + 1e-6
            && at.y + shapes.height * mm <= hi.y + 1e-6;
        if fits_as_drawn {
            return (at, mm);
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
        let mm = Units::Millimetres;
        assert_eq!(
            Underlay::placement(&square(100.0), Point::new(10.0, 20.0), mm, lo, hi),
            (Point::new(10.0, 20.0), 1.0)
        );
        let (at, scale) = Underlay::placement(&square(2970.0), Point::new(0.0, 0.0), mm, lo, hi);
        assert!((scale - 0.1).abs() < 1e-9);
        assert!((at.x - (420.0 - 297.0) / 2.0).abs() < 1e-9 && at.y.abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn units_decide_real_size() {
        let (lo, hi) = (Point::new(0.0, 0.0), Point::new(420.0, 297.0));
        // 10 × 10 cm drawn at (1, 2) cm: real size, 10 mm per unit, at (10, 20) mm.
        let (at, scale) = Underlay::placement(&square(10.0), Point::new(1.0, 2.0), Units::Centimetres, lo, hi);
        assert_eq!((at, scale), (Point::new(10.0, 20.0), 10.0));
        // A 4 × 4 inch square fits at real size: 25.4 mm per unit.
        assert_eq!(Underlay::placement(&square(4.0), Point::new(0.0, 0.0), Units::Inches, lo, hi).1, 25.4);
        // 2 m doesn't fit an A3 sheet: fitted, and its real scale is 1:10.
        let (_, scale) = Underlay::placement(&square(2.97), Point::new(0.0, 0.0), Units::Metres, lo, hi);
        let u = Underlay { scale, units: Units::Metres, ..underlay(square(2.97)) };
        assert!((u.real_scale() - 0.1).abs() < 1e-9, "{}", u.real_scale());
        assert_eq!(Units::from_insunits(4), Some(Units::Millimetres));
        assert_eq!(Units::from_insunits(0), None);
        assert_eq!(Units::parse("Inch"), Some(Units::Inches));
    }

    fn underlay(shapes: UnderlayShapes) -> Underlay {
        Underlay {
            source: "a.dxf".into(),
            shapes: Arc::new(shapes),
            at: Point::new(0.0, 0.0),
            scale: 1.0,
            visible: true,
            units: Units::Millimetres,
            units_from: UnitsFrom::File,
        }
    }

    #[test]
    fn malformed_or_overflowing_geometry_is_refused() {
        assert!(underlay(square(10.0)).is_valid());
        let broken: [fn(&mut UnderlayShapes); 6] = [
            |s| s.lines.push(vec![Point::new(1.0, 1.0)]),
            |s| s.lines[0][1] = Point::new(10.5, 0.0),
            |s| s.fills[0][2] = Point::new(0.0, -1.0),
            |s| s.width = f64::INFINITY,
            |s| s.width = -1.0,
            |s| s.height = MAX_SIZE * 2.0,
        ];
        for (n, change) in broken.iter().enumerate() {
            let mut s = square(10.0);
            change(&mut s);
            assert!(!underlay(s).is_valid(), "case {n}");
        }
        // Finite numbers that overflow, or land far off, once placed.
        let huge = Underlay { scale: f64::MAX / 2.0, ..underlay(square(10.0)) };
        assert!(!huge.is_valid());
        let off = Underlay { at: Point::new(MAX_PAGE_COORD, 0.0), ..underlay(square(10.0)) };
        assert!(!off.is_valid());
        let edge = Underlay { at: Point::new(MAX_PAGE_COORD - 10.0, 0.0), ..underlay(square(10.0)) };
        assert!(edge.is_valid());
    }

    #[test]
    fn an_underlay_is_set_undone_and_checked() {
        let mut p = crate::sample::start_circuit().unwrap();
        let page = crate::sample::PAGE_1;
        let u = Underlay { source: "old.dxf".into(), scale: 2.0, ..underlay(square(50.0)) };
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
        let u = underlay(square(1.0));
        p.apply(Command::SetUnderlay { page: id, underlay: Some(Box::new(u)) }).unwrap();
        assert!(matches!(p.apply(Command::RemovePage(id)), Err(ModelError::PageNotEmpty(_))));
        p.apply(Command::SetUnderlay { page: id, underlay: None }).unwrap();
        p.apply(Command::RemovePage(id)).unwrap();
    }
}
