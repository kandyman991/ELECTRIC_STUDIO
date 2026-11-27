//! Reading a DXF or DWG drawing into an underlay, and drawing an underlay behind a page.

use crate::{Primitive, Result, Shape, ViewError};
use cadcraft_doc::{Drawing, Space};
use ecad_model::geometry::Point;
use ecad_model::underlay::{MAX_POINTS, UnderlayShapes};

/// Largest drawing file read, bytes.
pub const MAX_FILE: usize = 64 << 20;

/// A drawing read for an underlay: its shapes, where its lower-left corner was in its own
/// coordinates (for [`ecad_model::underlay::Underlay::placement`]), and which space it came
/// from (`Model`, or a layout's name when model space is empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub shapes: UnderlayShapes,
    pub origin: Point,
    pub space: String,
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
    let origin = ext.min;
    let pt = |p: &cadcraft_doc::geom::Vec2| Point::new(round(p.x - origin.x), round(p.y - origin.y));
    let mut shapes =
        UnderlayShapes { lines: Vec::new(), fills: Vec::new(), width: round(ext.width()), height: round(ext.height()) };
    let mut points = 0usize;
    for prim in &list.prims {
        let (start, len) = (prim.start as usize, prim.len as usize);
        match prim.kind {
            cadcraft_render::Kind::Polyline => {
                let Some(v) = list.verts.get(start..start + len) else { continue };
                let mut line: Vec<Point> = v.iter().map(pt).collect();
                line.dedup();
                if line.len() >= 2 {
                    points += line.len();
                    shapes.lines.push(line);
                }
            }
            cadcraft_render::Kind::Tris => {
                let Some(v) = list.tris.get(start..start + len) else { continue };
                for t in v.chunks_exact(3) {
                    shapes.fills.push([pt(&t[0]), pt(&t[1]), pt(&t[2])]);
                }
                points += len;
            }
            _ => {}
        }
        if points > MAX_POINTS {
            return Err(ViewError::Drawing(format!(
                "{name}: too detailed for an underlay (over {MAX_POINTS} points); export a simpler DXF"
            )));
        }
    }
    if shapes.lines.is_empty() && shapes.fills.is_empty() {
        return Err(ViewError::Drawing(format!("{name}: nothing in the drawing could be drawn")));
    }
    Ok(Imported { shapes, origin: Point::new(origin.x, origin.y), space: space_name })
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
}
