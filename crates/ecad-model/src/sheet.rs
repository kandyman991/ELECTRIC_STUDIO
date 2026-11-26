//! The drawing sheet every page is drawn on: its size, border, column grid and title block.
//! Columns give each point an address such as `2.4` (page 2, column 4), which
//! cross-references use.

use crate::geometry::Point;
use serde::{Deserialize, Serialize};

/// Border widths in mm: wider on the left for binding, as is usual for schematics.
pub const BORDER_LEFT: f64 = 20.0;
pub const BORDER: f64 = 10.0;
/// Height of the numbered column band along the top of the drawing area.
pub const COLUMN_BAND: f64 = 6.0;
/// Size of the title block in the bottom-right corner.
pub const TITLE_BLOCK_WIDTH: f64 = 180.0;
pub const TITLE_BLOCK_HEIGHT: f64 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// Bottom-left corner of the sheet in page coordinates.
    pub origin: Point,
    pub width: f64,
    pub height: f64,
    /// Number of equal columns across the drawing area.
    pub columns: u32,
}

impl Default for Sheet {
    /// A3 landscape, ten columns.
    fn default() -> Self {
        Sheet { origin: Point::ZERO, width: 420.0, height: 297.0, columns: 10 }
    }
}

impl Sheet {
    /// The whole sheet (`min`, `max`).
    pub fn bounds(&self) -> (Point, Point) {
        (self.origin, Point::new(self.origin.x + self.width, self.origin.y + self.height))
    }

    /// Inside the border (`min`, `max`).
    pub fn frame(&self) -> (Point, Point) {
        let (lo, hi) = self.bounds();
        (Point::new(lo.x + BORDER_LEFT, lo.y + BORDER), Point::new(hi.x - BORDER, hi.y - BORDER))
    }

    /// The title block (`min`, `max`), in the frame's bottom-right corner.
    pub fn title_block(&self) -> (Point, Point) {
        let (lo, hi) = self.frame();
        (Point::new(hi.x - TITLE_BLOCK_WIDTH, lo.y), Point::new(hi.x, lo.y + TITLE_BLOCK_HEIGHT))
    }

    /// Left and right edge of column `n` (1-based).
    pub fn column_span(&self, n: u32) -> (f64, f64) {
        let (lo, hi) = self.frame();
        let w = (hi.x - lo.x) / f64::from(self.columns.max(1));
        let left = lo.x + w * f64::from(n.saturating_sub(1));
        (left, left + w)
    }

    /// The column (1-based) a point falls in; points left or right of the frame count as the
    /// first or last column.
    pub fn column_of(&self, p: Point) -> u32 {
        let (lo, hi) = self.frame();
        let n = self.columns.max(1);
        let w = (hi.x - lo.x) / f64::from(n);
        let i = ((p.x - lo.x) / w).floor();
        if i < 0.0 { 1 } else { (i as u32 + 1).min(n) }
    }

    /// A default-size sheet placed so that the box (`min`, `max`) sits in the middle of its
    /// drawing area, on the 4 mm grid. Used for projects saved before sheets existed.
    pub fn around(min: Point, max: Point) -> Sheet {
        let s = Sheet::default();
        let (flo, fhi) = s.frame();
        // The drawing area: inside the frame, below the column band, above the title block.
        let area_center = Point::new((flo.x + fhi.x) / 2.0, (flo.y + TITLE_BLOCK_HEIGHT + fhi.y - COLUMN_BAND) / 2.0);
        let content_center = Point::new((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
        let snap = |v: f64| (v / 4.0).round() * 4.0;
        Sheet {
            origin: Point::new(snap(content_center.x - area_center.x), snap(content_center.y - area_center.y)),
            ..s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_split_the_frame_evenly() {
        let s = Sheet::default();
        assert_eq!(s.frame(), (Point::new(20.0, 10.0), Point::new(410.0, 287.0)));
        assert_eq!(s.column_span(1), (20.0, 59.0));
        assert_eq!(s.column_of(Point::new(21.0, 100.0)), 1);
        assert_eq!(s.column_of(Point::new(60.0, 100.0)), 2);
        assert_eq!(s.column_of(Point::new(409.0, 100.0)), 10);
        assert_eq!(s.column_of(Point::new(-50.0, 0.0)), 1, "left of the frame");
        assert_eq!(s.column_of(Point::new(900.0, 0.0)), 10, "right of the frame");
    }

    #[test]
    fn a_sheet_around_content_contains_it() {
        let (min, max) = (Point::new(-5.0, -12.0), Point::new(8.0, 64.0));
        let s = Sheet::around(min, max);
        let (lo, hi) = s.frame();
        assert!(lo.x < min.x && lo.y + TITLE_BLOCK_HEIGHT < min.y && hi.x > max.x && hi.y - COLUMN_BAND > max.y);
        assert_eq!((s.origin.x % 4.0, s.origin.y % 4.0), (0.0, 0.0), "on the grid");
    }
}
