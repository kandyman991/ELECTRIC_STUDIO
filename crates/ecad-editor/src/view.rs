//! Screen ↔ page transform: pan and zoom. Screen pixels have y down; page millimetres y up.

pub use ecad_model::Point;
use serde::Serialize;

pub const MIN_SCALE: f64 = 0.5;
pub const MAX_SCALE: f64 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Viewport {
    /// Page point shown at the centre of the canvas.
    pub center: Point,
    /// Pixels per millimetre.
    pub scale: f64,
    /// Canvas size in pixels.
    pub width: f64,
    pub height: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport { center: Point::ZERO, scale: 4.0, width: 800.0, height: 600.0 }
    }
}

impl Viewport {
    pub fn to_screen(&self, p: Point) -> (f64, f64) {
        (self.width / 2.0 + (p.x - self.center.x) * self.scale, self.height / 2.0 - (p.y - self.center.y) * self.scale)
    }

    pub fn to_page(&self, x: f64, y: f64) -> Point {
        Point::new(
            self.center.x + (x - self.width / 2.0) / self.scale,
            self.center.y - (y - self.height / 2.0) / self.scale,
        )
    }

    /// Move the view by a pointer drag of (`dx`, `dy`) pixels.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.center = Point::new(self.center.x - dx / self.scale, self.center.y + dy / self.scale);
    }

    /// Zoom by `factor` keeping the page point under (`x`, `y`) in place.
    pub fn zoom_at(&mut self, x: f64, y: f64, factor: f64) {
        let before = self.to_page(x, y);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_page(x, y);
        self.center = Point::new(self.center.x + before.x - after.x, self.center.y + before.y - after.y);
    }

    /// Show the box (`min`, `max`) with `margin` pixels around it.
    pub fn fit(&mut self, min: Point, max: Point, margin: f64) {
        let w = (max.x - min.x).max(1.0);
        let h = (max.y - min.y).max(1.0);
        let sx = (self.width - 2.0 * margin).max(10.0) / w;
        let sy = (self.height - 2.0 * margin).max(10.0) / h;
        self.scale = sx.min(sy).clamp(MIN_SCALE, MAX_SCALE);
        self.center = Point::new((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_page_round_trip_with_y_flipped() {
        let v = Viewport { center: Point::new(10.0, 20.0), scale: 5.0, width: 400.0, height: 300.0 };
        assert_eq!(v.to_screen(Point::new(10.0, 20.0)), (200.0, 150.0));
        assert_eq!(v.to_screen(Point::new(12.0, 22.0)), (210.0, 140.0), "page up is screen up");
        assert_eq!(v.to_page(210.0, 140.0), Point::new(12.0, 22.0));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut v = Viewport::default();
        let under = v.to_page(100.0, 50.0);
        v.zoom_at(100.0, 50.0, 2.0);
        let again = v.to_page(100.0, 50.0);
        assert!((under.x - again.x).abs() < 1e-9 && (under.y - again.y).abs() < 1e-9);
        assert_eq!(v.scale, 8.0);
    }

    #[test]
    fn fit_centres_and_scales_to_the_smaller_axis() {
        let mut v = Viewport { width: 400.0, height: 200.0, ..Viewport::default() };
        v.fit(Point::new(0.0, 0.0), Point::new(100.0, 100.0), 0.0);
        assert_eq!((v.center, v.scale), (Point::new(50.0, 50.0), 2.0));
    }
}
