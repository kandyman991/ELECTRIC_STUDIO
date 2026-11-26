//! Plane geometry in millimetres, in page coordinates (y up).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn offset(self, by: Point) -> Point {
        Point::new(self.x + by.x, self.y + by.y)
    }
}

/// Symbol rotation in quarter turns, counter-clockwise. Schematic symbols only ever sit at
/// right angles, which keeps every coordinate exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn degrees(self) -> f64 {
        match self {
            Rotation::R0 => 0.0,
            Rotation::R90 => 90.0,
            Rotation::R180 => 180.0,
            Rotation::R270 => 270.0,
        }
    }

    /// Rotate a point about the origin.
    pub fn apply(self, p: Point) -> Point {
        match self {
            Rotation::R0 => p,
            Rotation::R90 => Point::new(-p.y, p.x),
            Rotation::R180 => Point::new(-p.x, -p.y),
            Rotation::R270 => Point::new(p.y, -p.x),
        }
    }

    pub fn apply_dir(self, d: Direction) -> Direction {
        let turns = match self {
            Rotation::R0 => 0,
            Rotation::R90 => 1,
            Rotation::R180 => 2,
            Rotation::R270 => 3,
        };
        (0..turns).fold(d, |d, _| d.turn_left())
    }
}

/// The direction a wire leaves a connection point in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn turn_left(self) -> Direction {
        match self {
            Direction::Up => Direction::Left,
            Direction::Left => Direction::Down,
            Direction::Down => Direction::Right,
            Direction::Right => Direction::Up,
        }
    }

    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::Up | Direction::Down)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_exact() {
        let p = Point::new(3.0, 8.0);
        assert_eq!(Rotation::R90.apply(p), Point::new(-8.0, 3.0));
        assert_eq!(Rotation::R180.apply(p), Point::new(-3.0, -8.0));
        assert_eq!(Rotation::R270.apply(p), Point::new(8.0, -3.0));
        assert_eq!(Rotation::R90.apply_dir(Direction::Up), Direction::Left);
        assert_eq!(Rotation::R270.apply_dir(Direction::Up), Direction::Right);
    }
}
