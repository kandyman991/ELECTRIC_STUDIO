//! Wire layout on a page: wires of different potentials running on top of each other, wires
//! through symbols they don't connect to, and moving a wire's middle run (its bend) to avoid
//! both.

use crate::geometry::Point;
use crate::project::{Command, ConnectionId, FunctionId, PageId, Project};
use std::collections::BTreeMap;

/// Grid step a bend moves by, in mm.
pub const BEND_STEP: f64 = 4.0;
/// How far a bend may move from halfway, in steps.
const MAX_STEPS: i32 = 12;
/// Symbol boxes grow by this much before a wire counts as passing through, in mm: a wire along
/// a symbol's edge reads as part of it.
const EDGE: f64 = 0.1;
/// Wires of different potentials on one line must keep at least this far apart, in mm, unless a
/// symbol sits between them: closer, they read as one wire.
pub const LINE_GAP: f64 = 16.5;
/// Parallel wires closer than this, side by side, read as one, in mm (the grid is 4 mm).
pub const MIN_SPACING: f64 = 3.0;

#[derive(Clone, Debug, PartialEq)]
pub enum WireClash {
    /// Two wires run along the same line for a stretch or side by side too close, or two wires
    /// of different potentials line up end to end with only a short gap between them.
    Overlap { a: ConnectionId, b: ConnectionId, at: Point },
    /// A wire runs through a symbol it isn't connected to.
    ThroughSymbol { wire: ConnectionId, function: FunctionId, at: Point },
}

/// Where two routes run on top of each other for more than a millimetre, if they do.
pub fn shared_run(a: &[Point], b: &[Point]) -> Option<Point> {
    for s in a.windows(2) {
        for t in b.windows(2) {
            let horizontal = s[0].y == s[1].y && t[0].y == t[1].y && s[0].y == t[0].y && s[0].x != s[1].x;
            let vertical = s[0].x == s[1].x && t[0].x == t[1].x && s[0].x == t[0].x && s[0].y != s[1].y;
            let (s0, s1, t0, t1) = if horizontal {
                (s[0].x, s[1].x, t[0].x, t[1].x)
            } else if vertical {
                (s[0].y, s[1].y, t[0].y, t[1].y)
            } else {
                continue;
            };
            let lo = s0.min(s1).max(t0.min(t1));
            let hi = s0.max(s1).min(t0.max(t1));
            if hi - lo > 1.0 {
                let m = (lo + hi) / 2.0;
                return Some(if horizontal { Point::new(m, s[0].y) } else { Point::new(s[0].x, m) });
            }
        }
    }
    None
}

/// Where two routes lie on one line with less than `gap` between them (or overlapping), if
/// they do: the middle of the gap or of the shared stretch.
pub fn lined_up(a: &[Point], b: &[Point], gap: f64) -> Option<Point> {
    for s in a.windows(2) {
        for t in b.windows(2) {
            let horizontal =
                s[0].y == s[1].y && t[0].y == t[1].y && s[0].y == t[0].y && s[0].x != s[1].x && t[0].x != t[1].x;
            let vertical =
                s[0].x == s[1].x && t[0].x == t[1].x && s[0].x == t[0].x && s[0].y != s[1].y && t[0].y != t[1].y;
            let (s0, s1, t0, t1) = if horizontal {
                (s[0].x, s[1].x, t[0].x, t[1].x)
            } else if vertical {
                (s[0].y, s[1].y, t[0].y, t[1].y)
            } else {
                continue;
            };
            let lo = s0.min(s1).max(t0.min(t1));
            let hi = s0.max(s1).min(t0.max(t1));
            // `hi - lo` is the shared length, or minus the gap.
            if hi - lo > -gap {
                let m = (lo + hi) / 2.0;
                return Some(if horizontal { Point::new(m, s[0].y) } else { Point::new(s[0].x, m) });
            }
        }
    }
    None
}

/// Where two routes run side by side closer than `spacing` (but not on one line) for more than
/// a millimetre, if they do.
pub fn too_close(a: &[Point], b: &[Point], spacing: f64) -> Option<Point> {
    for s in a.windows(2) {
        for t in b.windows(2) {
            let horizontal = s[0].y == s[1].y && t[0].y == t[1].y && s[0].x != s[1].x && t[0].x != t[1].x;
            let vertical = s[0].x == s[1].x && t[0].x == t[1].x && s[0].y != s[1].y && t[0].y != t[1].y;
            let (d, s0, s1, t0, t1) = if horizontal {
                ((s[0].y - t[0].y).abs(), s[0].x, s[1].x, t[0].x, t[1].x)
            } else if vertical {
                ((s[0].x - t[0].x).abs(), s[0].y, s[1].y, t[0].y, t[1].y)
            } else {
                continue;
            };
            let lo = s0.min(s1).max(t0.min(t1));
            let hi = s0.max(s1).min(t0.max(t1));
            if d > 0.0 && d < spacing && hi - lo > 1.0 {
                let m = (lo + hi) / 2.0;
                return Some(if horizontal { Point::new(m, s[0].y) } else { Point::new(s[0].x, m) });
            }
        }
    }
    None
}

/// Whether an axis-parallel segment passes through the inside of a box (touching its edge
/// doesn't count).
pub fn segment_crosses_box(a: Point, b: Point, lo: Point, hi: Point) -> bool {
    if lo.x >= hi.x || lo.y >= hi.y {
        return false;
    }
    if a.x == b.x {
        lo.x < a.x && a.x < hi.x && a.y.min(b.y) < hi.y && lo.y < a.y.max(b.y)
    } else if a.y == b.y {
        lo.y < a.y && a.y < hi.y && a.x.min(b.x) < hi.x && lo.x < a.x.max(b.x)
    } else {
        false
    }
}

/// What the layout checks need about a page, computed once.
struct PageWires {
    /// Each wire: id, its two functions, its potential (index), its route.
    wires: Vec<(ConnectionId, FunctionId, FunctionId, usize, Vec<Point>)>,
    /// Each symbol's box, grown by [`EDGE`].
    symbols: Vec<(FunctionId, Point, Point)>,
}

impl PageWires {
    /// Where two routes clash: any shared stretch, and for different potentials also lining up
    /// end to end closer than [`LINE_GAP`] with no symbol in between.
    fn pair_clash(&self, pa: usize, ra: &[Point], pb: usize, rb: &[Point]) -> Option<Point> {
        // Even on one potential a shared stretch hides which wire goes where.
        if let Some(at) = shared_run(ra, rb).or_else(|| too_close(ra, rb, MIN_SPACING)) {
            return Some(at);
        }
        if pa == pb {
            return None;
        }
        let at = lined_up(ra, rb, LINE_GAP)?;
        let bridged =
            self.symbols.iter().any(|(_, lo, hi)| lo.x <= at.x && at.x <= hi.x && lo.y <= at.y && at.y <= hi.y);
        (!bridged).then_some(at)
    }
}

impl Project {
    fn page_wires(&self, page: PageId) -> PageWires {
        let mut potential: BTreeMap<ConnectionId, usize> = BTreeMap::new();
        for (i, p) in self.potentials().iter().enumerate() {
            for c in &p.connections {
                potential.insert(*c, i);
            }
        }
        let mut next = usize::MAX;
        let wires = self
            .connections_on(page)
            .filter_map(|c| {
                let route = self.route(c).ok()?;
                // A wire on no potential (shouldn't happen) is its own.
                let pot = potential.get(&c.id).copied().unwrap_or_else(|| {
                    next -= 1;
                    next
                });
                Some((c.id, c.from.function, c.to.function, pot, route))
            })
            .collect();
        let symbols = self
            .functions_on(page)
            .filter_map(|f| {
                let (lo, hi) = self.function_bounds(f.id).ok()??;
                Some((f.id, Point::new(lo.x - EDGE, lo.y - EDGE), Point::new(hi.x + EDGE, hi.y + EDGE)))
            })
            .collect();
        PageWires { wires, symbols }
    }

    /// Wires on a page that run on top of another wire, line up with one of another potential,
    /// or run through or along a symbol.
    pub fn wire_clashes(&self, page: PageId) -> Vec<WireClash> {
        let pw = self.page_wires(page);
        let mut out = Vec::new();
        for (i, (a, _, _, pa, ra)) in pw.wires.iter().enumerate() {
            for (b, _, _, pb, rb) in &pw.wires[i + 1..] {
                if let Some(at) = pw.pair_clash(*pa, ra, *pb, rb) {
                    out.push(WireClash::Overlap { a: *a, b: *b, at });
                }
            }
        }
        for (id, from, to, _, route) in &pw.wires {
            for (f, lo, hi) in &pw.symbols {
                if f == from || f == to {
                    continue;
                }
                if let Some(seg) = route.windows(2).find(|s| segment_crosses_box(s[0], s[1], *lo, *hi)) {
                    out.push(WireClash::ThroughSymbol { wire: *id, function: *f, at: seg[0] });
                }
            }
        }
        out
    }

    /// A bend for the wire that keeps it off wires of other potentials and out of symbols, if
    /// its current route has such a clash and a nearby bend has fewer; `None` when the route is
    /// fine as it is or nothing better is found. The middle run moves in [`BEND_STEP`]s from
    /// halfway, preferring small moves and staying between the wire's ends.
    pub fn free_bend(&self, id: ConnectionId) -> Option<f64> {
        let c = self.connections.get(&id)?;
        let page = self.function(c.from.function).ok()?.page;
        let pw = self.page_wires(page);
        let (_, from, to, pot, current) = pw.wires.iter().find(|w| w.0 == id)?;
        let clashes = |route: &[Point]| {
            let wires =
                pw.wires.iter().filter(|w| w.0 != id && pw.pair_clash(*pot, route, w.3, &w.4).is_some()).count();
            let symbols = pw
                .symbols
                .iter()
                .filter(|(f, lo, hi)| {
                    f != from && f != to && route.windows(2).any(|s| segment_crosses_box(s[0], s[1], *lo, *hi))
                })
                .count();
            wires + symbols
        };
        let now = clashes(current);
        if now == 0 {
            return None;
        }
        let (a, da) = self.connection_point(&c.from).ok()?;
        let (b, db) = self.connection_point(&c.to).ok()?;
        let run = crate::project::middle_run(a, da, b, db)?;
        let (half, (lo, hi)) = (run.at, run.between);
        let mut best: Option<(usize, bool, i32, f64)> = None;
        for k in 1..=MAX_STEPS {
            for sign in [1, -1] {
                let bend = half + f64::from(sign * k) * BEND_STEP;
                let route = crate::project::route_bent(a, da, b, db, Some(bend));
                let n = clashes(&route);
                let outside = bend <= lo || bend >= hi;
                let key = (n, outside, k, bend);
                if n < now && best.is_none_or(|b| (key.0, key.1, key.2) < (b.0, b.1, b.2)) {
                    best = Some(key);
                }
            }
        }
        best.map(|b| b.3)
    }

    /// Commands that give every clashing wire on a page a better bend, in order, each wire seeing
    /// the bends given before it.
    pub fn tidy_wires(&self, page: PageId) -> Vec<Command> {
        let mut scratch = self.clone();
        let mut out = Vec::new();
        let ids: Vec<ConnectionId> = self.connections_on(page).map(|c| c.id).collect();
        for id in ids {
            if let Some(bend) = scratch.free_bend(id) {
                let cmd = Command::SetWireBend { id, bend: Some(bend) };
                if scratch.apply(cmd.clone()).is_ok() {
                    out.push(cmd);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_runs_need_more_than_a_millimetre() {
        let a = [Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
        let b = [Point::new(9.5, 0.0), Point::new(20.0, 0.0)];
        let c = [Point::new(5.0, 0.0), Point::new(20.0, 0.0)];
        assert_eq!(shared_run(&a, &b), None);
        assert_eq!(shared_run(&a, &c), Some(Point::new(7.5, 0.0)));
    }

    #[test]
    fn wires_lined_up_end_to_end_or_side_by_side_are_found() {
        let a = [Point::new(0.0, 0.0), Point::new(0.0, 10.0)];
        // On the same line, 12 mm further up: lined up within 16.5 mm, not within 4.
        let b = [Point::new(0.0, 22.0), Point::new(0.0, 40.0)];
        assert_eq!(lined_up(&a, &b, LINE_GAP), Some(Point::new(0.0, 16.0)));
        assert_eq!(lined_up(&a, &b, 4.0), None);
        // Side by side 2 mm apart is too close; 4 mm (one grid step) is fine.
        let c = [Point::new(2.0, 2.0), Point::new(2.0, 8.0)];
        let d = [Point::new(4.0, 2.0), Point::new(4.0, 8.0)];
        assert_eq!(too_close(&a, &c, MIN_SPACING), Some(Point::new(0.0, 5.0)));
        assert_eq!(too_close(&a, &d, MIN_SPACING), None);
    }

    #[test]
    fn a_segment_on_a_box_edge_does_not_cross_it() {
        let (lo, hi) = (Point::new(0.0, 0.0), Point::new(4.0, 4.0));
        assert!(!segment_crosses_box(Point::new(0.0, -5.0), Point::new(0.0, 9.0), lo, hi));
        assert!(segment_crosses_box(Point::new(2.0, -5.0), Point::new(2.0, 9.0), lo, hi));
        assert!(!segment_crosses_box(Point::new(2.0, 5.0), Point::new(2.0, 9.0), lo, hi));
    }
}
