//! The drawing sheet every page is drawn on: its size, border, column and row grid and title
//! block. The grid gives each point an address that cross-references use: `2.4` (page 2,
//! column 4) with columns only, or `A3/2F` (page A3, column 2, row F) with rows too.

use crate::geometry::Point;
use serde::{Deserialize, Serialize};

/// Border widths in mm: wider on the left for binding, as is usual for schematics.
pub const BORDER_LEFT: f64 = 20.0;
pub const BORDER: f64 = 10.0;
/// Height of the numbered column band along the top of the drawing area.
pub const COLUMN_BAND: f64 = 6.0;
/// Height of the title block, which runs along the whole bottom of the frame.
pub const TITLE_BLOCK_HEIGHT: f64 = 24.0;
/// Width of the lettered row band in the left border.
pub const ROW_BAND: f64 = 6.0;
/// The most rows a sheet can have (A to Z).
pub const MAX_ROWS: u32 = 26;
/// The most columns a sheet can have.
pub const MAX_COLUMNS: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// Bottom-left corner of the sheet in page coordinates.
    pub origin: Point,
    pub width: f64,
    pub height: f64,
    /// Number of equal columns across the drawing area.
    pub columns: u32,
    /// The number printed on the first column: 1 (default) or 0.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub first_column: u32,
    /// Number of equal rows down the frame, lettered from A at the top; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rows: u32,
}

fn one() -> u32 {
    1
}
fn is_one(n: &u32) -> bool {
    *n == 1
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Default for Sheet {
    /// A3 landscape, ten columns.
    fn default() -> Self {
        Sheet { origin: Point::ZERO, width: 420.0, height: 297.0, columns: 10, first_column: 1, rows: 0 }
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

    /// The title block (`min`, `max`), along the bottom of the frame.
    pub fn title_block(&self) -> (Point, Point) {
        let (lo, hi) = self.frame();
        (lo, Point::new(hi.x, lo.y + TITLE_BLOCK_HEIGHT))
    }

    /// The number printed on column `n` (1-based position).
    pub fn column_label(&self, n: u32) -> u32 {
        n.saturating_sub(1) + self.first_column
    }

    /// Bottom and top edge of row `n` (1-based, A = 1 at the top). Rows run from the title
    /// block's top to the column band.
    pub fn row_span(&self, n: u32) -> (f64, f64) {
        let (lo, hi) = self.frame();
        let h = (hi.y - lo.y) / f64::from(self.rows.max(1));
        let top = hi.y - h * f64::from(n.saturating_sub(1));
        (top - h, top)
    }

    /// The letter of row `n` (1-based).
    pub fn row_label(n: u32) -> char {
        char::from(b'A' + (n.clamp(1, MAX_ROWS) - 1) as u8)
    }

    /// The row (1-based) a point falls in, or `None` without rows.
    pub fn row_of(&self, p: Point) -> Option<u32> {
        if self.rows == 0 {
            return None;
        }
        let (lo, hi) = self.frame();
        let h = (hi.y - lo.y) / f64::from(self.rows);
        let i = ((hi.y - p.y) / h).floor();
        Some(if i < 0.0 { 1 } else { (i as u32 + 1).min(self.rows) })
    }

    /// A point's address on the page: `4` (column) or `2F` (column and row).
    pub fn address(&self, p: Point) -> String {
        let column = self.column_label(self.column_of(p));
        match self.row_of(p) {
            Some(row) => format!("{column}{}", Sheet::row_label(row)),
            None => column.to_string(),
        }
    }

    /// Whether the sheet's settings are usable.
    pub fn is_valid(&self) -> bool {
        (1..=MAX_COLUMNS).contains(&self.columns) && self.rows <= MAX_ROWS && self.first_column <= 1
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
    fn rows_and_a_zero_based_grid_give_addresses_like_2f() {
        let s = Sheet { columns: 8, first_column: 0, rows: 6, ..Sheet::default() };
        let (lo, hi) = s.frame();
        assert_eq!(s.address(Point::new(lo.x + 1.0, hi.y - 1.0)), "0A");
        assert_eq!(s.address(Point::new(hi.x - 1.0, lo.y + 1.0)), "7F");
        let (left, _) = s.column_span(3);
        let (bottom, _) = s.row_span(6);
        assert_eq!(s.address(Point::new(left + 1.0, bottom + 1.0)), "2F");
        assert_eq!(Sheet::default().address(Point::new(21.0, 100.0)), "1", "no rows: just the column");
        assert!(s.is_valid());
        assert!(!Sheet { rows: 27, ..s }.is_valid());
        assert!(!Sheet { columns: 0, ..s }.is_valid());
        assert!(!Sheet { first_column: 2, ..s }.is_valid());
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
