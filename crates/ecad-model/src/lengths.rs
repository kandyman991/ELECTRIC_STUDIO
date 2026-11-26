//! Estimated wire lengths from the panel layout: from a device's footprint straight to the
//! nearest cable duct, along the ducts by the shortest way, and out to the other device, plus a
//! spare length at each end, rounded up.
//!
//! A wire end on a junction, a potential point or a page arrow isn't a real place: it counts
//! from the nearest placed device on the same potential. A wire with an end on a device that
//! isn't on a plate (a motor, a door button) has no estimate.

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
    pub fn is_valid(&self) -> bool {
        self.spare_per_end >= 0.0 && self.spare_per_end.is_finite() && self.round_to >= 0.0 && self.round_to.is_finite()
    }
}

/// A wire's estimated length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireLength {
    /// mm, spare lengths included, rounded up.
    pub mm: f64,
    /// Measured straight across (no ducts join the two ends), not along ducts.
    pub direct: bool,
}

/// Where a device sits: its layout page and the middle of its footprint.
type Place = (PageId, Point);

/// A wire end: on a placed device, or on a junction, potential point or arrow (its device),
/// which takes its place from the potential.
type End = Result<(DeviceId, Place), DeviceId>;

/// The ducts of one plate as a network along their centre lines.
struct Ducts {
    /// Centre lines: (from, to, half width).
    lines: Vec<(Point, Point, f64)>,
}

/// Manhattan distance: wires run across and along, not slanted.
fn manhattan(a: Point, b: Point) -> f64 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

/// The point of segment `a`–`b` nearest to `p`.
fn project_on(p: Point, a: Point, b: Point) -> Point {
    if a.x == b.x {
        Point::new(a.x, p.y.clamp(a.y.min(b.y), a.y.max(b.y)))
    } else {
        Point::new(p.x.clamp(a.x.min(b.x), a.x.max(b.x)), a.y)
    }
}

impl Ducts {
    fn of(project: &Project, page: PageId) -> Ducts {
        let lines = project
            .panel_items_on(page)
            .into_iter()
            .filter_map(|i| {
                let PanelItemKind::Duct { width, .. } = i.kind else { return None };
                let (lo, hi) = project.panel_item_rect(i);
                let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
                Some(if i.vertical {
                    (Point::new(mid.x, lo.y), Point::new(mid.x, hi.y), width / 2.0)
                } else {
                    (Point::new(lo.x, mid.y), Point::new(hi.x, mid.y), width / 2.0)
                })
            })
            .collect();
        Ducts { lines }
    }

    /// Where a wire from `p` enters the ducts: (duct, entry point, distance to it).
    fn entry(&self, p: Point) -> Option<(usize, Point, f64)> {
        self.lines
            .iter()
            .enumerate()
            .map(|(k, (a, b, _))| {
                let q = project_on(p, *a, *b);
                (k, q, manhattan(p, q))
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))
    }

    /// The shortest way along the ducts from `a` on duct `da` to `b` on duct `db`, if they are
    /// joined. Ducts are joined where they cross or touch (within their half widths).
    fn along(&self, (da, a): (usize, Point), (db, b): (usize, Point)) -> Option<f64> {
        // Nodes: the two entries, every duct's ends, and every crossing of two ducts.
        let mut nodes: Vec<(usize, Point)> = vec![(da, a), (db, b)];
        for (k, (p, q, _)) in self.lines.iter().enumerate() {
            nodes.push((k, *p));
            nodes.push((k, *q));
        }
        let mut links: Vec<(usize, usize, f64)> = Vec::new();
        for (i, (p0, p1, hp)) in self.lines.iter().enumerate() {
            for (j, (q0, q1, hq)) in self.lines.iter().enumerate().skip(i + 1) {
                let reach = hp + hq + 1.0;
                // The nearest points of the two centre lines; joined if within reach.
                let candidates = [
                    project_on(*q0, *p0, *p1),
                    project_on(*q1, *p0, *p1),
                    project_on(*p0, *q0, *q1),
                    project_on(*p1, *q0, *q1),
                ];
                let crossing = if p0.y == p1.y && q0.x == q1.x {
                    Some((Point::new(q0.x, p0.y), Point::new(q0.x, p0.y)))
                } else if p0.x == p1.x && q0.y == q1.y {
                    Some((Point::new(p0.x, q0.y), Point::new(p0.x, q0.y)))
                } else {
                    None
                };
                let pairs: Vec<(Point, Point)> = crossing
                    .into_iter()
                    .map(|(c, _)| (project_on(c, *p0, *p1), project_on(c, *q0, *q1)))
                    .chain(candidates.iter().map(|c| (project_on(*c, *p0, *p1), project_on(*c, *q0, *q1))))
                    .collect();
                if let Some((pa, qa)) =
                    pairs.into_iter().min_by(|x, y| manhattan(x.0, x.1).total_cmp(&manhattan(y.0, y.1)))
                    && manhattan(pa, qa) <= reach
                {
                    nodes.push((i, pa));
                    nodes.push((j, qa));
                    links.push((nodes.len() - 2, nodes.len() - 1, manhattan(pa, qa)));
                }
            }
        }
        // Along each duct, its nodes in order are joined.
        for k in 0..self.lines.len() {
            let mut on: Vec<(usize, Point)> =
                nodes.iter().enumerate().filter(|(_, n)| n.0 == k).map(|(i, n)| (i, n.1)).collect();
            on.sort_by(|x, y| (x.1.x + x.1.y).total_cmp(&(y.1.x + y.1.y)));
            for w in on.windows(2) {
                links.push((w[0].0, w[1].0, manhattan(w[0].1, w[1].1)));
            }
        }
        // Dijkstra from node 0 to node 1 (few nodes: a plain scan is enough).
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
            for (a, b, w) in &links {
                let v = if *a == u {
                    *b
                } else if *b == u {
                    *a
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
        let placed_on = |pot: &crate::potential::Potential, other: DeviceId| -> Vec<(DeviceId, Place)> {
            let mut v: Vec<(DeviceId, Place)> = pot
                .points
                .iter()
                .filter_map(|r| self.function(r.function).ok().map(|f| f.device))
                .filter(|d| *d != other)
                .filter_map(|d| self.placed_at(d).map(|at| (d, at)))
                .collect();
            v.sort_by_key(|(d, _)| *d);
            v.dedup_by_key(|(d, _)| *d);
            v
        };
        let mut ducts: BTreeMap<PageId, Ducts> = BTreeMap::new();
        let settings = self.wire_length_settings;
        for c in self.connections.values() {
            let pot = potentials.iter().find(|p| p.connections.contains(&c.id));
            // Each end: its device's place, or for a junction, potential point or arrow the
            // nearest placed device on the potential (decided below, once the other end is known).
            let end = |r: &crate::ConnectionRef| -> Option<End> {
                let f = self.function(r.function).ok()?;
                match self.role(f.id).ok()? {
                    Role::Potential | Role::Interruption => Some(Err(f.device)),
                    _ if self.symbol(&f.symbol).is_ok_and(|s| s.bridged && s.connection_points.len() > 2) => {
                        Some(Err(f.device))
                    }
                    _ => self.placed_at(f.device).map(|at| Ok((f.device, at))),
                }
            };
            let (Some(a), Some(b)) = (end(&c.from), end(&c.to)) else { continue };
            let resolve = |this: End, other: Option<Place>, other_device: DeviceId| match this {
                Ok(x) => Some(x.1),
                Err(_) => {
                    let candidates = placed_on(pot?, other_device);
                    match other {
                        Some((page, at)) => candidates
                            .into_iter()
                            .filter(|(_, (p, _))| *p == page)
                            .min_by(|x, y| manhattan(x.1.1, at).total_cmp(&manhattan(y.1.1, at)))
                            .map(|x| x.1),
                        None => None,
                    }
                }
            };
            let device = |e: &End| match e {
                Ok((d, _)) | Err(d) => *d,
            };
            let (da, db) = (device(&a), device(&b));
            let a_at = a.as_ref().ok().map(|x| x.1);
            let b_at = b.as_ref().ok().map(|x| x.1);
            let (Some((pa, pa_at)), Some((pb, pb_at))) = (resolve(a, b_at, db), resolve(b, a_at, da)) else { continue };
            if pa != pb {
                continue;
            }
            let network = ducts.entry(pa).or_insert_with(|| Ducts::of(self, pa));
            let routed = match (network.entry(pa_at), network.entry(pb_at)) {
                (Some((ka, qa, ra)), Some((kb, qb, rb))) => {
                    network.along((ka, qa), (kb, qb)).map(|along| ra + along + rb)
                }
                _ => None,
            };
            let (raw, direct) = match routed {
                Some(mm) => (mm, false),
                None => (manhattan(pa_at, pb_at), true),
            };
            let mm = raw + 2.0 * settings.spare_per_end;
            let mm = if settings.round_to > 0.0 { (mm / settings.round_to).ceil() * settings.round_to } else { mm };
            out.insert(c.id, WireLength { mm, direct });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{Panel, PanelItem};
    use crate::project::{Command, PanelItemId};

    #[test]
    fn a_path_runs_along_joined_ducts_by_the_shortest_way() {
        // An L of ducts: across at y=100 from x=0 to 400, up at x=400 from 100 to 500.
        let d = Ducts {
            lines: vec![
                (Point::new(0.0, 100.0), Point::new(400.0, 100.0), 30.0),
                (Point::new(400.0, 100.0), Point::new(400.0, 500.0), 30.0),
            ],
        };
        let (ka, qa, ra) = d.entry(Point::new(100.0, 40.0)).unwrap();
        assert_eq!((ka, qa, ra), (0, Point::new(100.0, 100.0), 60.0));
        let (kb, qb, rb) = d.entry(Point::new(470.0, 300.0)).unwrap();
        assert_eq!((kb, qb, rb), (1, Point::new(400.0, 300.0), 70.0));
        assert_eq!(d.along((ka, qa), (kb, qb)), Some(300.0 + 200.0));
        // Ducts apart don't join.
        let apart = Ducts {
            lines: vec![
                (Point::new(0.0, 0.0), Point::new(100.0, 0.0), 20.0),
                (Point::new(0.0, 300.0), Point::new(100.0, 300.0), 20.0),
            ],
        };
        assert_eq!(apart.along((0, Point::new(50.0, 0.0)), (1, Point::new(50.0, 300.0))), None);
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
        assert!(!l.direct && l.mm >= 300.0 && l.mm % 50.0 == 0.0, "{l:?}");
        // A motor isn't on the plate: its wires have no estimate.
        assert!(
            p.connections
                .values()
                .filter(|c| p.describe(&c.to).is_ok_and(|d| d.0 == "M3"))
                .all(|c| !lengths.contains_key(&c.id))
        );
        assert!(lengths.len() > 50, "{}", lengths.len());
    }

    #[test]
    fn without_ducts_lengths_are_measured_straight_and_settings_apply() {
        let mut p = crate::sample::with_contacts().unwrap();
        let page = crate::sample::PAGE_1;
        p.apply(Command::SetPanel { page, panel: Some(Panel { width: 1000.0, height: 1000.0 }) }).unwrap();
        // Two devices placed 300 mm apart across and 100 up.
        let wired: Vec<_> = p
            .connections
            .values()
            .filter_map(|c| {
                let a = p.function(c.from.function).ok()?.device;
                let b = p.function(c.to.function).ok()?.device;
                (a != b).then_some((c.id, a, b))
            })
            .collect();
        let (id, a, b) = wired[0];
        for (d, x, y) in [(a, 100.0, 100.0), (b, 400.0, 200.0)] {
            let item = PanelItem {
                id: PanelItemId::new(),
                page,
                kind: PanelItemKind::Device { device: d },
                at: Point::new(x, y),
                length: 0.0,
                vertical: false,
                part: String::new(),
            };
            p.apply(Command::AddPanelItem(item)).unwrap();
        }
        let l = p.wire_lengths()[&id];
        assert!(l.direct);
        assert_eq!(l.mm, 300.0 + 100.0 + 2.0 * 150.0, "both one module: middles 300 across, 100 up, spare 150 each");
        p.apply(Command::SetWireLengthSettings(WireLengthSettings { spare_per_end: 0.0, round_to: 0.0 })).unwrap();
        assert_eq!(p.wire_lengths()[&id].mm, 400.0);
        assert!(
            p.apply(Command::SetWireLengthSettings(WireLengthSettings { spare_per_end: -1.0, round_to: 0.0 })).is_err()
        );
    }
}
