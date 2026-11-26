//! Estimated wire lengths from the panel layout: from a device's footprint straight to the
//! nearest cable duct, along the ducts by the shortest way, and out to the other device, plus a
//! spare length at each end, rounded up.
//!
//! A wire end on a junction, a potential point or a page arrow isn't a real place. When exactly
//! one other device on its potential is on the plate, the end counts from there and the length
//! is marked inferred; otherwise the wire has no estimate. Neither has a wire with an end on a
//! device that isn't on a plate (a motor, a door button).

use crate::geometry::Point;
use crate::panel::PanelItemKind;
use crate::project::{ConnectionId, DeviceId, PageId, Project};
use crate::symbol::Role;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How wire lengths are estimated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireLengthSettings {
    /// Added at each end for stripping and routing inside the device, mm.
    pub spare_per_end: f64,
    /// Lengths are rounded up to a multiple of this, mm (0: not rounded).
    pub round_to: f64,
}

impl Default for WireLengthSettings {
    fn default() -> Self {
        WireLengthSettings { spare_per_end: 150.0, round_to: 50.0 }
    }
}

impl WireLengthSettings {
    /// The largest spare length and rounding step, mm.
    pub const MAX: f64 = 10_000.0;

    /// Spare 0 to [`MAX`](Self::MAX) mm; rounding 0 (off) or 1 to [`MAX`](Self::MAX) mm.
    pub fn is_valid(&self) -> bool {
        (0.0..=Self::MAX).contains(&self.spare_per_end)
            && (self.round_to == 0.0 || (1.0..=Self::MAX).contains(&self.round_to))
    }
}

/// A wire's estimated length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireLength {
    /// mm, spare lengths included, rounded up.
    pub mm: f64,
    /// Measured straight across (no ducts join the two ends), not along ducts.
    pub direct: bool,
    /// An end is a junction, potential point or arrow, placed at the one device it can stand
    /// for: the wire's real route may differ.
    pub inferred: bool,
}

/// Where a device sits: its layout page and the middle of its footprint.
type Place = (PageId, Point);

/// A wire end: on a placed device, or on a junction, potential point or arrow (its device),
/// which takes its place from the potential.
type End = Result<(DeviceId, Place), DeviceId>;

/// The ducts of one plate as a network along their centre lines.
struct Ducts {
    /// Each duct: its centre line and its footprint (lower-left, upper-right).
    ducts: Vec<Duct>,
    /// Points on the centre lines where one duct passes into another: (duct, point).
    joins: Vec<(usize, Point)>,
    /// Passages between joined ducts: indices into `joins`, and the length across.
    passages: Vec<(usize, usize, f64)>,
}

struct Duct {
    from: Point,
    to: Point,
    lo: Point,
    hi: Point,
}

/// Manhattan distance: wires run across and along, not slanted.
fn manhattan(a: Point, b: Point) -> f64 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

/// The point of the across or upright segment `a`–`b` nearest to `p`.
fn project_on(p: Point, a: Point, b: Point) -> Point {
    if a.x == b.x {
        Point::new(a.x, p.y.clamp(a.y.min(b.y), a.y.max(b.y)))
    } else {
        Point::new(p.x.clamp(a.x.min(b.x), a.x.max(b.x)), a.y)
    }
}

/// Ducts closer than this count as touching, mm.
const TOUCH: f64 = 0.5;

impl Duct {
    /// The point of the centre line nearest to `p`.
    fn nearest(&self, p: Point) -> Point {
        project_on(p, self.from, self.to)
    }
}

impl Ducts {
    fn of(project: &Project, page: PageId) -> Ducts {
        let ducts: Vec<Duct> = project
            .panel_items_on(page)
            .into_iter()
            .filter(|i| matches!(i.kind, PanelItemKind::Duct { .. }))
            .map(|i| {
                let (lo, hi) = project.panel_item_rect(i);
                let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
                let (from, to) = if i.vertical {
                    (Point::new(mid.x, lo.y), Point::new(mid.x, hi.y))
                } else {
                    (Point::new(lo.x, mid.y), Point::new(hi.x, mid.y))
                };
                Duct { from, to, lo, hi }
            })
            .collect();
        Ducts::new(ducts)
    }

    /// Two ducts are joined where their footprints overlap or touch: a wire passes from one
    /// centre line to the other through the middle of the shared area.
    fn new(ducts: Vec<Duct>) -> Ducts {
        let mut joins = Vec::new();
        let mut passages = Vec::new();
        for (i, p) in ducts.iter().enumerate() {
            for (j, q) in ducts.iter().enumerate().skip(i + 1) {
                let lo = Point::new(p.lo.x.max(q.lo.x), p.lo.y.max(q.lo.y));
                let hi = Point::new(p.hi.x.min(q.hi.x), p.hi.y.min(q.hi.y));
                if lo.x > hi.x + TOUCH || lo.y > hi.y + TOUCH {
                    continue;
                }
                let shared = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
                let (a, b) = (p.nearest(shared), q.nearest(shared));
                joins.push((i, a));
                joins.push((j, b));
                passages.push((joins.len() - 2, joins.len() - 1, manhattan(a, b)));
            }
        }
        Ducts { ducts, joins, passages }
    }

    /// The shortest way from `a` to `b` through the ducts: from `a` straight to its nearest
    /// duct, along joined ducts, and out of the duct nearest to `b`. Wires don't cross the open
    /// plate to reach a farther duct, so if the nearest ducts aren't joined this is `None` (and
    /// the wire is measured straight), even when some other duct would connect them. Ducts at
    /// the same distance (a corner) are all ways in.
    fn route(&self, a: Point, b: Point) -> Option<f64> {
        if self.ducts.is_empty() {
            return None;
        }
        // Nodes: the two ends, every join, and where each end enters each duct.
        let mut nodes: Vec<(Option<usize>, Point)> = vec![(None, a), (None, b)];
        nodes.extend(self.joins.iter().map(|(k, p)| (Some(*k), *p)));
        let offset = 2;
        let mut links: Vec<(usize, usize, f64)> =
            self.passages.iter().map(|(x, y, w)| (x + offset, y + offset, *w)).collect();
        for end in 0..2 {
            let p = nodes[end].1;
            let drops: Vec<(usize, Point, f64)> = self
                .ducts
                .iter()
                .enumerate()
                .map(|(k, d)| {
                    let q = d.nearest(p);
                    (k, q, manhattan(p, q))
                })
                .collect();
            let nearest = drops.iter().map(|d| d.2).fold(f64::INFINITY, f64::min);
            for (k, q, drop) in drops.into_iter().filter(|d| d.2 <= nearest + TOUCH) {
                nodes.push((Some(k), q));
                links.push((end, nodes.len() - 1, drop));
            }
        }
        // Along each duct, its nodes in order are joined.
        for k in 0..self.ducts.len() {
            let mut on: Vec<(usize, Point)> =
                nodes.iter().enumerate().filter(|(_, n)| n.0 == Some(k)).map(|(i, n)| (i, n.1)).collect();
            on.sort_by(|x, y| (x.1.x + x.1.y).total_cmp(&(y.1.x + y.1.y)));
            for w in on.windows(2) {
                links.push((w[0].0, w[1].0, manhattan(w[0].1, w[1].1)));
            }
        }
        // The two ends link only to duct nodes, never to each other.
        let mut dist = vec![f64::INFINITY; nodes.len()];
        let mut done = vec![false; nodes.len()];
        dist[0] = 0.0;
        loop {
            let next = (0..nodes.len())
                .filter(|i| !done[*i] && dist[*i].is_finite())
                .min_by(|x, y| dist[*x].total_cmp(&dist[*y]));
            let Some(u) = next else { break };
            if u == 1 {
                return Some(dist[1]);
            }
            done[u] = true;
            for (x, y, w) in &links {
                let v = if *x == u {
                    *y
                } else if *y == u {
                    *x
                } else {
                    continue;
                };
                if dist[u] + w < dist[v] {
                    dist[v] = dist[u] + w;
                }
            }
        }
        None
    }
}

/// A wire end on the plate: where it is, and whether that was taken from the potential.
struct Located {
    page: PageId,
    at: Point,
    inferred: bool,
}

impl Project {
    /// The middle of a device's footprint and its page, if it is on a plate.
    fn placed_at(&self, device: DeviceId) -> Option<Place> {
        let item = self.panel_item_of(device)?;
        let (lo, hi) = self.panel_item_rect(item);
        Some((item.page, Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)))
    }

    /// Estimated lengths of the wires whose two ends can be found on one plate.
    pub fn wire_lengths(&self) -> BTreeMap<ConnectionId, WireLength> {
        let mut out = BTreeMap::new();
        if self.panel_items.is_empty() {
            return out;
        }
        let potentials = self.potentials();
        let mut ducts: BTreeMap<PageId, Ducts> = BTreeMap::new();
        let settings = self.wire_length_settings;
        for c in self.connections.values() {
            let pot = potentials.iter().find(|p| p.connections.contains(&c.id));
            let (Some(a), Some(b)) = (self.wire_end(&c.from), self.wire_end(&c.to)) else { continue };
            let (Some(a), Some(b)) = (self.locate(&a, &b, pot), self.locate(&b, &a, pot)) else { continue };
            if a.page != b.page {
                continue;
            }
            let network = ducts.entry(a.page).or_insert_with(|| Ducts::of(self, a.page));
            let (raw, direct) = match network.route(a.at, b.at) {
                Some(mm) => (mm, false),
                None => (manhattan(a.at, b.at), true),
            };
            let mm = raw + 2.0 * settings.spare_per_end;
            let mm = if settings.round_to > 0.0 { (mm / settings.round_to).ceil() * settings.round_to } else { mm };
            if !mm.is_finite() {
                continue;
            }
            out.insert(c.id, WireLength { mm, direct, inferred: a.inferred || b.inferred });
        }
        out
    }

    /// A wire end: its device placed on a plate, or a junction, potential point or arrow (its
    /// device), which isn't a place of its own. `None` for a device that isn't on a plate.
    fn wire_end(&self, r: &crate::ConnectionRef) -> Option<End> {
        let f = self.function(r.function).ok()?;
        match self.role(f.id).ok()? {
            Role::Potential | Role::Interruption => Some(Err(f.device)),
            _ if self.symbol(&f.symbol).is_ok_and(|s| s.bridged && s.connection_points.len() > 2) => {
                Some(Err(f.device))
            }
            _ => self.placed_at(f.device).map(|at| Ok((f.device, at))),
        }
    }

    /// Where an end is. A junction, potential point or arrow takes the place of the one placed
    /// device on its potential, besides the other end's, on the other end's plate; with none or
    /// several to choose from, or the other end not on a plate either, the place is unknown
    /// rather than guessed.
    fn locate(&self, this: &End, other: &End, pot: Option<&crate::potential::Potential>) -> Option<Located> {
        match this {
            Ok((_, (page, at))) => Some(Located { page: *page, at: *at, inferred: false }),
            Err(_) => {
                let Ok((other_device, (page, _))) = other else { return None };
                let mut candidates: Vec<(DeviceId, Point)> = pot?
                    .points
                    .iter()
                    .filter_map(|r| self.function(r.function).ok().map(|f| f.device))
                    .filter(|d| d != other_device)
                    .filter_map(|d| self.placed_at(d).filter(|(p, _)| p == page).map(|(_, at)| (d, at)))
                    .collect();
                candidates.sort_by_key(|(d, _)| *d);
                candidates.dedup_by_key(|(d, _)| *d);
                match candidates.as_slice() {
                    [(_, at)] => Some(Located { page: *page, at: *at, inferred: true }),
                    _ => None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{Panel, PanelItem};
    use crate::project::{Command, PanelItemId};

    /// A duct by its footprint, centre line along its long side.
    fn duct(lo: (f64, f64), hi: (f64, f64)) -> Duct {
        let (lo, hi) = (Point::new(lo.0, lo.1), Point::new(hi.0, hi.1));
        let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let (from, to) = if hi.y - lo.y > hi.x - lo.x {
            (Point::new(mid.x, lo.y), Point::new(mid.x, hi.y))
        } else {
            (Point::new(lo.x, mid.y), Point::new(hi.x, mid.y))
        };
        Duct { from, to, lo, hi }
    }

    fn route(ducts: Vec<Duct>, a: (f64, f64), b: (f64, f64)) -> Option<f64> {
        Ducts::new(ducts).route(Point::new(a.0, a.1), Point::new(b.0, b.1))
    }

    #[test]
    fn a_path_runs_along_joined_ducts_by_the_shortest_way() {
        // An L of 60 mm ducts: across at y=100, up at x=400, overlapping at the corner.
        let l = || vec![duct((0.0, 70.0), (430.0, 130.0)), duct((370.0, 70.0), (430.0, 500.0))];
        // Down 60 to the duct, 300 along, 200 up, 70 out.
        assert_eq!(route(l(), (100.0, 40.0), (470.0, 300.0)), Some(630.0));
        // A cross: 60 in, 150 along, through the crossing, 150 up, 60 out.
        let cross = vec![duct((0.0, -30.0), (400.0, 30.0)), duct((170.0, -200.0), (230.0, 200.0))];
        assert_eq!(route(cross, (50.0, -60.0), (260.0, 150.0)), Some(420.0));
        // A T: the upright duct stands on the top edge of the other one; 30 across the joint.
        let t = vec![duct((0.0, -30.0), (400.0, 30.0)), duct((170.0, 30.0), (230.0, 300.0))];
        assert_eq!(route(t, (50.0, -60.0), (260.0, 150.0)), Some(60.0 + 150.0 + 30.0 + 120.0 + 60.0));
        // Two ducts end to end, touching.
        let line = vec![duct((0.0, -30.0), (100.0, 30.0)), duct((100.0, -30.0), (200.0, 30.0))];
        assert_eq!(route(line, (50.0, 40.0), (150.0, 40.0)), Some(40.0 + 100.0 + 40.0));
    }

    #[test]
    fn ducts_with_a_gap_between_them_are_not_joined() {
        // End to end with 30 mm between: wide enough to reach across by their widths, but apart.
        let gap = vec![duct((0.0, -30.0), (100.0, 30.0)), duct((130.0, -30.0), (230.0, 30.0))];
        assert_eq!(route(gap, (50.0, 40.0), (180.0, 40.0)), None);
        // An upright duct ending 10 mm short of an across one.
        let near = vec![duct((0.0, -30.0), (200.0, 30.0)), duct((100.0, 40.0), (160.0, 300.0))];
        assert_eq!(route(near, (50.0, -50.0), (200.0, 200.0)), None);
        // Unequal widths crossing at the centre lines' level but beside each other: apart.
        let beside = vec![duct((0.0, -40.0), (200.0, 40.0)), duct((205.0, -300.0), (235.0, 300.0))];
        assert_eq!(route(beside, (50.0, -50.0), (260.0, 200.0)), None);
        // No ducts at all.
        assert_eq!(route(Vec::new(), (0.0, 0.0), (10.0, 0.0)), None);
    }

    #[test]
    fn the_reference_wires_between_cabinet_devices_get_lengths() {
        let p = crate::reference::reference_project().unwrap();
        let lengths = p.wire_lengths();
        let wire = |n: &str, from: &str| {
            p.connections
                .values()
                .find(|c| c.wire_number.as_deref() == Some(n) && p.describe(&c.from).is_ok_and(|d| d.0 == from))
                .map(|c| c.id)
        };
        // The main switch's L1 output to the distribution block: both on the plate.
        let p201 = wire("P201", "P2QS1").expect("P2QS1 to P2WB1");
        let l = lengths[&p201];
        assert!(!l.direct && !l.inferred && l.mm >= 300.0 && l.mm % 50.0 == 0.0, "{l:?}");
        // A motor isn't on the plate: its wires have no estimate.
        assert!(
            p.connections
                .values()
                .filter(|c| p.describe(&c.to).is_ok_and(|d| d.0 == "M3"))
                .all(|c| !lengths.contains_key(&c.id))
        );
        assert!(lengths.len() > 50, "{}", lengths.len());
    }

    /// [`crate::sample::with_potentials`] with a plate and the given devices on it, in a row.
    fn placed(tags: &[&str]) -> Project {
        let mut p = crate::sample::with_potentials().unwrap();
        let page = crate::sample::PAGE_1;
        p.apply(Command::SetPanel { page, panel: Some(Panel { width: 1000.0, height: 1000.0 }) }).unwrap();
        for (k, tag) in tags.iter().enumerate() {
            let device = p.devices.values().find(|d| d.tag == *tag).unwrap().id;
            let item = PanelItem {
                id: PanelItemId::new(),
                page,
                kind: PanelItemKind::Device { device },
                at: Point::new(100.0 + 200.0 * k as f64, 100.0),
                length: 0.0,
                vertical: false,
                part: String::new(),
            };
            p.apply(Command::AddPanelItem(item)).unwrap();
        }
        p
    }

    fn by_number(p: &Project, n: &str) -> Option<WireLength> {
        let id = p.connections.values().find(|c| c.wire_number.as_deref() == Some(n))?.id;
        p.wire_lengths().get(&id).copied()
    }

    #[test]
    fn a_junction_stands_for_its_one_placed_device_and_otherwise_for_nothing() {
        // J1 joins -K1:A2 (W3), -K2:A2 (W7) and M (W8). With -K1 and -K2 on the plate, W3's
        // junction end can only be -K2, and W7's only -K1: inferred lengths.
        let p = placed(&["-K1", "-K2"]);
        let w3 = by_number(&p, "W3").unwrap();
        assert!(w3.inferred && w3.direct, "{w3:?}");
        assert_eq!(w3.mm, 200.0 + 2.0 * 150.0);
        assert!(by_number(&p, "W7").unwrap().inferred);
        // W8 runs from the junction to the potential point M: neither end is a place.
        assert_eq!(by_number(&p, "W8"), None);
        // With M on the plate too, W3's junction end could be -K2 or M: unknown, not guessed.
        let p = placed(&["-K1", "-K2", "M"]);
        assert_eq!(by_number(&p, "W3"), None);
        assert_eq!(by_number(&p, "W7"), None);
        // Wires between two placed devices are known exactly.
        let p = placed(&["-S1", "-K1"]);
        let w2 = by_number(&p, "W2").unwrap();
        assert!(!w2.inferred && w2.direct);
    }

    #[test]
    fn without_ducts_lengths_are_measured_straight_and_settings_apply() {
        let mut p = placed(&["-S1", "-K1"]);
        let id = p.connections.values().find(|c| c.wire_number.as_deref() == Some("W2")).unwrap().id;
        // Middles 200 apart across; spare 150 at each end.
        assert_eq!(p.wire_lengths()[&id].mm, 200.0 + 2.0 * 150.0);
        p.apply(Command::SetWireLengthSettings(WireLengthSettings { spare_per_end: 0.0, round_to: 0.0 })).unwrap();
        assert_eq!(p.wire_lengths()[&id].mm, 200.0);
        p.apply(Command::SetWireLengthSettings(WireLengthSettings { spare_per_end: 25.0, round_to: 1.0 })).unwrap();
        assert_eq!(p.wire_lengths()[&id].mm, 250.0);
    }

    #[test]
    fn settings_out_of_range_are_rejected() {
        let ok = |spare_per_end, round_to| WireLengthSettings { spare_per_end, round_to }.is_valid();
        assert!(
            ok(0.0, 0.0) && ok(150.0, 50.0) && ok(WireLengthSettings::MAX, 1.0) && ok(0.0, WireLengthSettings::MAX)
        );
        for (spare, round) in [
            (-1.0, 50.0),
            (150.0, -1.0),
            (f64::NAN, 50.0),
            (150.0, f64::INFINITY),
            (WireLengthSettings::MAX + 1.0, 50.0),
            (150.0, 0.5),
            (150.0, 1e-300),
        ] {
            assert!(!ok(spare, round), "{spare} {round}");
        }
        let mut p = placed(&[]);
        assert!(
            p.apply(Command::SetWireLengthSettings(WireLengthSettings { spare_per_end: -1.0, round_to: 0.0 })).is_err()
        );
    }
}
