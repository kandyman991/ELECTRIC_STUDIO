//! Reading a DXF or DWG drawing into an underlay, and drawing an underlay behind a page.

use crate::{Primitive, Result, Shape, ViewError};
use cadcraft_doc::{Drawing, Space};
use ecad_model::geometry::Point;
use ecad_model::underlay::{MAX_POINTS, UnderlayShapes, Units};
use std::collections::BTreeMap;

/// Largest drawing file read, bytes.
pub const MAX_FILE: usize = 64 << 20;

/// A drawing read for an underlay: its shapes, where its lower-left corner was in its own
/// coordinates (for [`ecad_model::underlay::Underlay::placement`]), which space it came from
/// (`Model`, or a layout's name when model space is empty), and its units if the file says
/// (`$INSUNITS`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Imported {
    pub shapes: UnderlayShapes,
    pub origin: Point,
    pub space: String,
    pub units: Option<Units>,
}

/// How much flattening a drawing may need, in points, before it is drawn: blocks inside
/// blocks, arrays of block references and fine hatch patterns multiply, so this is counted
/// first and the drawing refused before the renderer would build it all.
const MAX_EXPANSION: u64 = 4 * MAX_POINTS as u64;
/// Blocks nested deeper than this (or in a loop) are refused.
const MAX_NESTING: usize = 32;

/// An estimate of the points flattening `space` would make: generous for curves, text and
/// hatches, so a drawing passing it can't need vastly more.
fn expansion(d: &Drawing, space: &Space, size: f64) -> std::result::Result<u64, String> {
    use cadcraft_doc::EntityKind as K;
    fn chars(s: &str) -> u64 {
        s.chars().count() as u64
    }
    // A glyph as polylines or triangles: generously.
    const PER_CHAR: u64 = 80;
    // A curve smoothed at a 20 000th of the drawing.
    const PER_CURVE: u64 = 1000;
    struct Count<'a> {
        d: &'a Drawing,
        size: f64,
        blocks: BTreeMap<String, u64>,
        open: Vec<String>,
    }
    impl Count<'_> {
        fn block(&mut self, name: &str) -> std::result::Result<u64, String> {
            if let Some(c) = self.blocks.get(name) {
                return Ok(*c);
            }
            if self.open.iter().any(|n| n == name) || self.open.len() >= MAX_NESTING {
                return Err(format!("blocks nested in a loop or over {MAX_NESTING} deep (at {name})"));
            }
            let Some(b) = self.d.blocks.get(name) else { return Ok(0) };
            self.open.push(name.to_owned());
            let mut total = 0u64;
            for e in b.entities.iter() {
                total = total.saturating_add(self.entity(&e.kind)?);
            }
            self.open.pop();
            self.blocks.insert(name.to_owned(), total);
            Ok(total)
        }

        fn entity(&mut self, kind: &cadcraft_doc::EntityKind) -> std::result::Result<u64, String> {
            Ok(match kind {
                K::Line(_) | K::Point(_) | K::Ray(_) | K::XLine(_) => 2,
                K::Circle(_) | K::Arc(_) | K::Ellipse(_) | K::Spline(_) | K::Helix(_) => PER_CURVE,
                K::LwPolyline(p) => (p.vertices.len() as u64).saturating_mul(PER_CURVE / 10).saturating_add(2),
                K::Text(t) => chars(&t.value).saturating_mul(PER_CHAR),
                K::MText(t) => chars(&t.contents).saturating_mul(PER_CHAR),
                K::Insert(i) => {
                    let copies = u64::from(i.rows.max(1)).saturating_mul(u64::from(i.cols.max(1)));
                    let attribs: u64 = i.attribs.iter().map(|a| chars(&a.text.value).saturating_mul(PER_CHAR)).sum();
                    copies.saturating_mul(self.block(&i.block)?.saturating_add(attribs))
                }
                K::Dimension(dm) => {
                    let geometry = match &dm.block {
                        Some(b) => self.block(b)?,
                        None => 0,
                    };
                    geometry.saturating_add(40 * PER_CHAR)
                }
                K::Hatch(h) => {
                    let outline: u64 =
                        h.loops.iter().map(|l| (l.vertices.len() as u64).saturating_mul(PER_CURVE / 10)).sum();
                    if h.solid {
                        outline.saturating_mul(3)
                    } else {
                        // Pattern lines across the whole drawing at their spacing: an upper bound.
                        let scale = if h.scale.is_finite() && h.scale > 0.0 { h.scale } else { 1.0 };
                        let lines: u64 = if h.pattern_lines.is_empty() {
                            // A library pattern: about one line per unit of the pattern's scale.
                            (self.size / scale).ceil() as u64
                        } else {
                            h.pattern_lines
                                .iter()
                                .map(|l| {
                                    let spacing = (l.delta.1 * scale).abs().max(1e-12);
                                    let segments = l.dashes.len().max(1) as u64;
                                    ((self.size * 1.5 / spacing).ceil().min(1e18) as u64).saturating_mul(segments)
                                })
                                .fold(0u64, u64::saturating_add)
                        };
                        outline.saturating_add(lines.saturating_mul(2))
                    }
                }
                K::Table(t) => {
                    let cells: u64 = t.cells.iter().map(|r| r.len() as u64).sum();
                    cells.saturating_mul(20 * PER_CHAR)
                }
                _ => 200,
            })
        }
    }
    let mut count = Count { d, size, blocks: BTreeMap::new(), open: Vec::new() };
    let mut total = 0u64;
    for e in d.space(space).into_iter().flat_map(|s| s.iter()) {
        total = total.saturating_add(count.entity(&e.kind)?);
        if total > MAX_EXPANSION {
            break;
        }
    }
    Ok(total)
}

/// Round to a thousandth of a unit: plenty for tracing over, and a third of the file.
fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// The space to take: model space, or if it's empty the first layout (by tab order) that
/// has something on it.
fn space_to_read(d: &Drawing) -> Option<(Space, String)> {
    if !d.extents(&Space::Model).is_empty() {
        return Some((Space::Model, "Model".into()));
    }
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    layouts.into_iter().map(|l| Space::Paper(l.name.clone())).find(|s| !d.extents(s).is_empty()).map(|s| {
        let name = match &s {
            Space::Paper(n) => n.clone(),
            Space::Model => "Model".into(),
        };
        (s, name)
    })
}

/// Read a DXF or DWG file (`name` gives the format when the content doesn't) and flatten it
/// into an underlay's shapes: every line, arc, polyline, hatch, text and block as polylines
/// and filled triangles, in the drawing's units from its lower-left corner.
pub fn import(bytes: &[u8], name: &str) -> Result<Imported> {
    if bytes.len() > MAX_FILE {
        return Err(ViewError::Drawing(format!("{name}: over {} MB", MAX_FILE >> 20)));
    }
    let d = cadcraft_io::read(bytes, name).map_err(|e| ViewError::Drawing(format!("{name}: {e}")))?;
    let (space, space_name) =
        space_to_read(&d).ok_or_else(|| ViewError::Drawing(format!("{name}: the drawing is empty")))?;
    let ext = d.extents(&space);
    let size = ext.width().max(ext.height()).max(1e-9);
    if !(ext.width().is_finite() && ext.height().is_finite())
        || ext.width() > ecad_model::underlay::MAX_SIZE
        || ext.height() > ecad_model::underlay::MAX_SIZE
    {
        return Err(ViewError::Drawing(format!(
            "{name}: the drawing is over {:e} units across",
            ecad_model::underlay::MAX_SIZE
        )));
    }
    // Before anything is flattened: a drawing that would expand past the budget is refused.
    let estimate = expansion(&d, &space, size).map_err(|e| ViewError::Drawing(format!("{name}: {e}")))?;
    if estimate > MAX_EXPANSION {
        return Err(ViewError::Drawing(format!(
            "{name}: too detailed for an underlay (blocks, arrays or hatch patterns would make over {MAX_POINTS} points); export a simpler DXF"
        )));
    }
    let units = d.header.get("INSUNITS").and_then(cadcraft_doc::HVal::as_i64).and_then(Units::from_insunits);
    let options = cadcraft_render::Options {
        // About a 20 000th of the drawing: smooth arcs without millions of points.
        tolerance: size / 20_000.0,
        min_dash: 0.0,
        text: true,
        // Every line whole: a fine pattern over a long line would be millions of dashes.
        linetypes: false,
        fill: true,
        ..cadcraft_render::Options::default()
    };
    let list = cadcraft_render::build(&d, &space, &options);
    // The pieces to keep (polylines and triangles), in order.
    let kept = || {
        list.prims.iter().filter_map(|prim| {
            let (start, len) = (prim.start as usize, prim.len as usize);
            match prim.kind {
                cadcraft_render::Kind::Polyline => list.verts.get(start..start + len).map(|v| (true, v)),
                cadcraft_render::Kind::Tris => list.tris.get(start..start + len).map(|v| (false, v)),
                _ => None,
            }
        })
    };
    // First, without copying anything: the count against the budget, and the bounds of what is
    // drawn (text and arcs can reach past the drawing's own extents; rays, which never end,
    // aren't drawn).
    let (mut lo, mut hi) = (
        cadcraft_doc::geom::Vec2::new(f64::INFINITY, f64::INFINITY),
        cadcraft_doc::geom::Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    let mut points = 0usize;
    for (_, v) in kept() {
        points = points.saturating_add(v.len());
        if points > MAX_POINTS {
            return Err(ViewError::Drawing(format!(
                "{name}: too detailed for an underlay (over {MAX_POINTS} points); export a simpler DXF"
            )));
        }
        for p in v.iter().filter(|p| p.x.is_finite() && p.y.is_finite()) {
            lo = cadcraft_doc::geom::Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = cadcraft_doc::geom::Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    if points == 0 || !(lo.x <= hi.x && lo.y <= hi.y) {
        return Err(ViewError::Drawing(format!("{name}: nothing in the drawing could be drawn")));
    }
    let (width, height) = (hi.x - lo.x, hi.y - lo.y);
    if width > ecad_model::underlay::MAX_SIZE || height > ecad_model::underlay::MAX_SIZE {
        return Err(ViewError::Drawing(format!(
            "{name}: the drawing is over {:e} units across",
            ecad_model::underlay::MAX_SIZE
        )));
    }
    // Then copied, from the lower-left corner of what is drawn.
    let origin = lo;
    let pt = |p: &cadcraft_doc::geom::Vec2| Point::new(round(p.x - origin.x), round(p.y - origin.y));
    let mut shapes =
        UnderlayShapes { lines: Vec::new(), fills: Vec::new(), width: round(width), height: round(height) };
    for (polyline, v) in kept() {
        let finite = v.iter().all(|p| p.x.is_finite() && p.y.is_finite());
        if !finite {
            continue;
        }
        if polyline {
            let mut line: Vec<Point> = v.iter().map(pt).collect();
            line.dedup();
            if line.len() >= 2 {
                shapes.lines.push(line);
            }
        } else {
            for [a, b, c] in v.as_chunks::<3>().0 {
                shapes.fills.push([pt(a), pt(b), pt(c)]);
            }
        }
    }
    if shapes.lines.is_empty() && shapes.fills.is_empty() {
        return Err(ViewError::Drawing(format!("{name}: nothing in the drawing could be drawn")));
    }
    Ok(Imported { shapes, origin: Point::new(origin.x, origin.y), space: space_name, units })
}

/// A page's underlay, if it has one shown, in a faint mix of `background` and its opposite,
/// to go under the page's own primitives.
pub(crate) fn primitives(
    project: &ecad_model::Project,
    page: ecad_model::PageId,
    background: [u8; 3],
) -> Vec<Primitive> {
    let Some(u) = project.underlays.get(&page).filter(|u| u.visible) else { return Vec::new() };
    let light = background.iter().map(|c| u32::from(*c)).sum::<u32>() > 3 * 128;
    let ink: [u8; 3] = if light { [0, 0, 0] } else { [255, 255, 255] };
    // A quarter of the way from the background to the ink.
    let mix = |b: u8, i: u8| ((u16::from(b) * 3 + u16::from(i)) / 4) as u8;
    let rgb = [mix(background[0], ink[0]), mix(background[1], ink[1]), mix(background[2], ink[2])];
    let mut out: Vec<Primitive> = u
        .shapes
        .lines
        .iter()
        .map(|l| Primitive { rgb, shape: Shape::Polyline(l.iter().map(|p| u.to_page(*p)).collect()) })
        .collect();
    if !u.shapes.fills.is_empty() {
        let tris = u.shapes.fills.iter().flat_map(|t| t.iter().map(|p| u.to_page(*p))).collect();
        out.push(Primitive { rgb, shape: Shape::Triangles(tris) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small DXF: a 200 × 100 frame at (1000, 500) with a circle and a text in it.
    fn sample_dxf() -> Vec<u8> {
        let mut d = Drawing::new_metric();
        let c = cadcraft_doc::Common::default();
        let v = |x: f64, y: f64| cadcraft_doc::geom::Vec3::new(x, y, 0.0);
        let line = |a, b| cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a, b });
        for (a, b) in [
            ((1000.0, 500.0), (1200.0, 500.0)),
            ((1200.0, 500.0), (1200.0, 600.0)),
            ((1200.0, 600.0), (1000.0, 600.0)),
            ((1000.0, 600.0), (1000.0, 500.0)),
        ] {
            d.add(&Space::Model, c.clone(), line(v(a.0, a.1), v(b.0, b.1))).unwrap();
        }
        let circle = cadcraft_doc::Circle { center: v(1050.0, 550.0), radius: 20.0 };
        d.add(&Space::Model, c.clone(), cadcraft_doc::EntityKind::Circle(circle)).unwrap();
        let text = crate::text(ecad_model::geometry::Point::new(1100.0, 540.0), 10.0, "K1");
        d.add(&Space::Model, c, cadcraft_doc::EntityKind::Text(text)).unwrap();
        cadcraft_io::write_dxf(&d).into_bytes()
    }

    #[test]
    fn a_dxf_flattens_into_lines_and_fills_from_its_corner() {
        let imported = import(&sample_dxf(), "old.dxf").unwrap();
        assert_eq!(imported.origin, Point::new(1000.0, 500.0));
        assert_eq!(imported.space, "Model");
        let s = &imported.shapes;
        assert!((s.width - 200.0).abs() < 0.01 && (s.height - 100.0).abs() < 0.01, "{} {}", s.width, s.height);
        assert!(s.lines.len() >= 5, "four sides and a circle: {}", s.lines.len());
        // Everything is within the drawing's own box, from its corner.
        for p in s.lines.iter().flatten().chain(s.fills.iter().flatten()) {
            assert!(p.x >= -0.01 && p.y >= -0.01 && p.x <= 200.01 && p.y <= 100.01, "{p:?}");
        }
        // The text is drawn too (as strokes or fills).
        let near_text = |p: &Point| p.x > 95.0 && p.x < 130.0 && p.y > 35.0 && p.y < 55.0;
        assert!(s.lines.iter().flatten().chain(s.fills.iter().flatten()).any(near_text));
        assert!(s.is_valid());
    }

    #[test]
    fn files_that_arent_drawings_are_refused() {
        assert!(import(b"hello", "notes.txt").is_err());
        assert!(
            import(b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n", "empty.dxf")
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
        assert!(import(&vec![0u8; MAX_FILE + 1], "huge.dxf").unwrap_err().to_string().contains("over 64 MB"));
    }

    #[test]
    fn an_underlay_draws_faintly_under_the_page_and_not_when_hidden() {
        use ecad_model::underlay::Underlay;
        let mut p = ecad_model::sample::start_circuit().unwrap();
        let page = ecad_model::sample::PAGE_1;
        let imported = import(&sample_dxf(), "old.dxf").unwrap();
        let u = Underlay {
            source: "old.dxf".into(),
            shapes: std::sync::Arc::new(imported.shapes),
            at: Point::new(10.0, 20.0),
            scale: 0.5,
            visible: true,
            units: Units::Millimetres,
            units_from: ecad_model::underlay::UnitsFrom::File,
        };
        p.apply(ecad_model::Command::SetUnderlay { page, underlay: Some(Box::new(u.clone())) }).unwrap();
        let white = [255, 255, 255];
        let all = crate::page_primitives(&p, page, white).unwrap();
        let plain = {
            let mut q = p.clone();
            q.underlays.clear();
            crate::page_primitives(&q, page, white).unwrap()
        };
        let extra = all.len() - plain.len();
        assert!(extra >= 5);
        // Underneath, light grey, placed by position and scale.
        assert!(all[..extra].iter().all(|x| x.rgb == [191, 191, 191]));
        let Shape::Polyline(first) = &all[0].shape else { panic!() };
        assert!(first.iter().all(|q| q.x >= 9.99 && q.x <= 110.01 && q.y >= 19.99 && q.y <= 70.01), "{first:?}");
        assert_eq!(&all[extra..], &plain[..]);
        // Dark background: a dark grey.
        assert_eq!(primitives(&p, page, [0, 0, 0])[0].rgb, [63, 63, 63]);
        // Hidden: nothing; and never in exports.
        let hidden = Underlay { visible: false, ..u };
        p.apply(ecad_model::Command::SetUnderlay { page, underlay: Some(Box::new(hidden)) }).unwrap();
        assert_eq!(crate::page_primitives(&p, page, white).unwrap().len(), plain.len());
        let mut shown = p.clone();
        shown.underlays.get_mut(&page).unwrap().visible = true;
        assert_eq!(crate::page_dxf(&shown, page).unwrap(), crate::page_dxf(&p, page).unwrap(), "exports leave it out");
    }

    fn drawing_with(build: impl FnOnce(&mut Drawing)) -> Drawing {
        let mut d = Drawing::new_metric();
        build(&mut d);
        d
    }

    fn line(d: &mut Drawing, space: &Space, a: (f64, f64), b: (f64, f64)) {
        let v = |(x, y): (f64, f64)| cadcraft_doc::geom::Vec3::new(x, y, 0.0);
        let kind = cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: v(a), b: v(b) });
        d.add(space, cadcraft_doc::Common::default(), kind).unwrap();
    }

    /// A block `name` holding `lines` short lines, or one reference to `inner`.
    fn block(d: &mut Drawing, name: &str, lines: usize, inner: Option<(&str, u32)>) {
        let mut b = cadcraft_doc::Block::new(name);
        for k in 0..lines {
            let y = k as f64;
            let kind = cadcraft_doc::EntityKind::Line(cadcraft_doc::Line {
                a: cadcraft_doc::geom::Vec3::new(0.0, y, 0.0),
                b: cadcraft_doc::geom::Vec3::new(1.0, y, 0.0),
            });
            let h = d.new_handle();
            b.entities.push(cadcraft_doc::Entity::new(h, kind));
        }
        if let Some((inner, n)) = inner {
            let insert = cadcraft_doc::Insert {
                block: inner.into(),
                rows: n,
                cols: n,
                row_spacing: 2.0,
                col_spacing: 2.0,
                ..Default::default()
            };
            let h = d.new_handle();
            b.entities.push(cadcraft_doc::Entity::new(h, cadcraft_doc::EntityKind::Insert(insert)));
        }
        d.blocks.insert(name.into(), std::sync::Arc::new(b));
    }

    fn insert(d: &mut Drawing, name: &str, n: u32) {
        let i = cadcraft_doc::Insert {
            block: name.into(),
            rows: n,
            cols: n,
            row_spacing: 50.0,
            col_spacing: 50.0,
            ..Default::default()
        };
        d.add(&Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Insert(i)).unwrap();
    }

    fn dxf(d: &Drawing) -> Vec<u8> {
        cadcraft_io::write_dxf(d).into_bytes()
    }

    #[test]
    fn text_reaching_past_the_drawings_extents_is_kept_inside_the_shapes() {
        // Text alone, at several sizes and angles: the drawn glyphs set the bounds.
        let d = drawing_with(|d| {
            for (k, h) in [2.5, 10.0, 40.0].into_iter().enumerate() {
                let mut t = crate::text(Point::new(0.0, 60.0 * k as f64), h, "Wide text, gjpqy ÄÖÜ");
                t.rotation = 0.3 * k as f64;
                d.add(&Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Text(t)).unwrap();
            }
        });
        let i = import(&dxf(&d), "text.dxf").unwrap();
        assert!(i.shapes.is_valid(), "every glyph point within the size");
        assert!(i.shapes.width > 100.0, "{}", i.shapes.width);
    }

    #[test]
    fn the_files_own_units_are_read_and_a_file_without_says_so() {
        for (code, units) in [
            (4, Some(Units::Millimetres)),
            (5, Some(Units::Centimetres)),
            (6, Some(Units::Metres)),
            (1, Some(Units::Inches)),
            (2, Some(Units::Feet)),
            (0, None),
        ] {
            let d = drawing_with(|d| {
                d.header.set_i64("INSUNITS", code);
                line(d, &Space::Model, (0.0, 0.0), (10.0, 5.0));
            });
            assert_eq!(import(&dxf(&d), "u.dxf").unwrap().units, units, "INSUNITS {code}");
        }
    }

    #[test]
    fn a_drawing_that_would_expand_too_far_is_refused_before_it_is_drawn() {
        // 200 lines in a block, its 40 × 40 array in another, and that 40 × 40 again: 64
        // million copies of a line. Counting refuses it; drawing it would take gigabytes.
        let d = drawing_with(|d| {
            block(d, "LEAF", 200, None);
            block(d, "MID", 0, Some(("LEAF", 40)));
            insert(d, "MID", 40);
        });
        let started = std::time::Instant::now();
        let e = import(&dxf(&d), "bomb.dxf").unwrap_err().to_string();
        assert!(e.contains("too detailed"), "{e}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "refused before drawing: {:?}",
            started.elapsed()
        );
        // Blocks that contain each other.
        let looped = drawing_with(|d| {
            block(d, "A", 1, Some(("B", 1)));
            block(d, "B", 1, Some(("A", 1)));
            insert(d, "A", 1);
            line(d, &Space::Model, (0.0, 0.0), (10.0, 10.0));
        });
        let mut counted = expansion(&looped, &Space::Model, 10.0);
        assert!(counted.as_ref().is_err_and(|e| e.contains("loop")), "{counted:?}");
        // A hatch with a pattern a millionth of the drawing apart.
        let fine = drawing_with(|d| {
            line(d, &Space::Model, (0.0, 0.0), (1000.0, 1000.0));
            let corner = |x: f64, y: f64| cadcraft_doc::geom::PolyVertex {
                p: cadcraft_doc::geom::Vec2::new(x, y),
                ..Default::default()
            };
            let hatch = cadcraft_doc::Hatch {
                pattern: "USER".into(),
                solid: false,
                loops: vec![cadcraft_doc::HatchLoop {
                    vertices: vec![corner(0.0, 0.0), corner(1000.0, 0.0), corner(1000.0, 1000.0), corner(0.0, 1000.0)],
                    outer: true,
                    boundary_flags: None,
                    sources: Vec::new(),
                }],
                scale: 1.0,
                angle: 0.0,
                associative: false,
                style: 0,
                elevation: 0.0,
                gradient: None,
                origin: cadcraft_doc::geom::Vec2::new(0.0, 0.0),
                background: None,
                pattern_lines: vec![cadcraft_doc::library::PatternLine {
                    angle: 0.0,
                    origin: (0.0, 0.0),
                    delta: (0.0, 0.0001),
                    dashes: Vec::new(),
                }],
            };
            d.add(&Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Hatch(hatch)).unwrap();
        });
        counted = expansion(&fine, &Space::Model, 1000.0);
        assert!(counted.unwrap() > MAX_EXPANSION);
        assert!(import(&dxf(&fine), "fine.dxf").unwrap_err().to_string().contains("too detailed"));
        // A modest array passes and is drawn.
        let modest = drawing_with(|d| {
            block(d, "LEAF", 4, None);
            insert(d, "LEAF", 10);
        });
        assert!(import(&dxf(&modest), "modest.dxf").unwrap().shapes.lines.len() >= 400);
    }

    #[test]
    fn binary_dxf_and_a_layout_only_drawing_import() {
        let d = drawing_with(|d| {
            d.header.set_i64("INSUNITS", 4);
            for (a, b) in [((0.0, 0.0), (300.0, 0.0)), ((300.0, 0.0), (300.0, 200.0))] {
                line(d, &Space::Model, a, b);
            }
        });
        let binary =
            cadcraft_io::write_with(&d, "b.dxf", &cadcraft_io::SaveOptions { binary: true, ..Default::default() })
                .unwrap();
        assert!(binary.starts_with(b"AutoCAD Binary DXF"), "a binary DXF");
        // DWG needs the worker process: tested with the `ecad` program (crates/ecad-cli/tests).
        let i = import(&binary, "b.dxf").unwrap();
        assert!((i.shapes.width - 300.0).abs() < 0.01 && (i.shapes.height - 200.0).abs() < 0.01);
        assert_eq!(i.units, Some(Units::Millimetres));
        // Nothing in model space, a drawing on the first layout.
        let paper = drawing_with(|d| {
            let layout = Space::Paper("Layout1".into());
            line(d, &layout, (10.0, 10.0), (110.0, 60.0));
        });
        let i = import(&dxf(&paper), "p.dxf").unwrap();
        assert_eq!(i.space, "Layout1");
        assert!((i.shapes.width - 100.0).abs() < 0.01);
    }
}
