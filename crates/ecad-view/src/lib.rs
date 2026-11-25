//! Turns model pages into CADCraft drawings and exports them.
//!
//! This is the only crate allowed to depend on CADCraft. Its public API speaks model types and
//! bytes, so nothing above it ever sees a CADCraft type, and a CADCraft upgrade can only break
//! this crate.

use cadcraft_doc::{
    Attrib, Block, Circle, Common, Drawing, Entity, EntityKind, HAlign, Insert, Line, Space, Text, VAlign,
    color::Color, geom::Vec3,
};
use ecad_model::{Direction, Graphic, PageId, Point, Project};
use std::sync::Arc;

pub const LAYER_SYMBOLS: &str = "SYMBOLS";
pub const LAYER_WIRES: &str = "WIRES";
pub const LAYER_TAGS: &str = "TAGS";

#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error(transparent)]
    Model(#[from] ecad_model::ModelError),
    #[error("drawing: {0}")]
    Drawing(String),
}

pub type Result<T> = std::result::Result<T, ViewError>;

fn v(p: Point) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

fn text(at: Point, height: f64, value: &str) -> Text {
    Text {
        insert: v(at),
        align_pt: None,
        height,
        value: value.to_owned(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::default(),
        valign: VAlign::default(),
    }
}

/// Text anchored at `at` by the given justification (left/baseline uses the start point).
fn justified(at: Point, height: f64, value: &str, halign: HAlign, valign: VAlign) -> Text {
    let mut t = text(at, height, value);
    if (halign, valign) != (HAlign::Left, VAlign::Baseline) {
        t.align_pt = Some(v(at));
    }
    t.halign = halign;
    t.valign = valign;
    t
}

/// Gap between a connection point and its designation, in mm.
const LABEL_GAP: f64 = 1.0;
/// Gap between a symbol's box and its device tag, in mm.
const TAG_GAP: f64 = 2.5;
pub const LABEL_HEIGHT: f64 = 1.8;
pub const TAG_HEIGHT: f64 = 2.5;

/// A connection point's designation, upright whatever the symbol's rotation: inside the symbol
/// beside the point, never on the wire leaving it.
fn point_label(at: Point, leaves: Direction, name: &str) -> Text {
    let g = LABEL_GAP;
    match leaves {
        Direction::Up => justified(Point::new(at.x + g, at.y - g), LABEL_HEIGHT, name, HAlign::Left, VAlign::Top),
        Direction::Down => justified(Point::new(at.x + g, at.y + g), LABEL_HEIGHT, name, HAlign::Left, VAlign::Bottom),
        Direction::Left => justified(Point::new(at.x + g, at.y + g), LABEL_HEIGHT, name, HAlign::Left, VAlign::Bottom),
        Direction::Right => {
            justified(Point::new(at.x - g, at.y + g), LABEL_HEIGHT, name, HAlign::Right, VAlign::Bottom)
        }
    }
}

/// Where a device tag goes: right of a symbol whose wires leave up/down, above one whose wires
/// leave sideways, so it never sits on a wire.
fn tag_text(min: Point, max: Point, sideways: bool, tag: &str) -> Text {
    if sideways {
        let at = Point::new((min.x + max.x) / 2.0, max.y + TAG_GAP);
        justified(at, TAG_HEIGHT, tag, HAlign::Center, VAlign::Bottom)
    } else {
        let at = Point::new(max.x + TAG_GAP, (min.y + max.y) / 2.0);
        justified(at, TAG_HEIGHT, tag, HAlign::Left, VAlign::Middle)
    }
}

/// A wire number at the middle of the wire's longest segment: beside a vertical run, above a
/// horizontal one.
fn wire_label(route: &[Point], number: &str) -> Text {
    let len = |s: &[Point]| (s[1].x - s[0].x).abs() + (s[1].y - s[0].y).abs();
    let longest = route.windows(2).max_by(|a, b| len(a).total_cmp(&len(b)));
    let Some(seg) = longest else { return text(route.first().copied().unwrap_or(Point::ZERO), LABEL_HEIGHT, number) };
    let mid = Point::new((seg[0].x + seg[1].x) / 2.0, (seg[0].y + seg[1].y) / 2.0);
    if seg[0].x == seg[1].x {
        justified(Point::new(mid.x + LABEL_GAP, mid.y), LABEL_HEIGHT, number, HAlign::Left, VAlign::Middle)
    } else {
        justified(Point::new(mid.x, mid.y + LABEL_GAP), LABEL_HEIGHT, number, HAlign::Center, VAlign::Bottom)
    }
}

fn on(layer: &str) -> Common {
    Common { layer: layer.to_owned(), ..Common::default() }
}

/// Build the drawing for one page. Symbols become block definitions, functions become inserts
/// carrying the device tag as a `TAG` attribute, and wires are routed from the model.
pub fn page_drawing(project: &Project, page: PageId) -> Result<Drawing> {
    project.page(page)?;
    let mut d = Drawing::new_metric();
    let err = |e: cadcraft_doc::DocError| ViewError::Drawing(e.to_string());

    // Only the symbols this page uses, in a stable order.
    let mut used: Vec<_> = project.functions_on(page).map(|f| f.symbol.clone()).collect();
    used.sort();
    used.dedup();
    for id in &used {
        let sym = project.symbol(id)?;
        let mut b = Block::new(&id.0);
        b.description = sym.description.clone();
        for g in &sym.graphics {
            let kind = match g {
                Graphic::Line { a, b } => EntityKind::Line(Line { a: v(*a), b: v(*b) }),
                Graphic::Circle { center, radius } => {
                    EntityKind::Circle(Circle { center: v(*center), radius: *radius })
                }
                Graphic::Text { at, height, value } => EntityKind::Text(text(*at, *height, value)),
            };
            let h = d.new_handle();
            b.entities.push(Entity::new(h, kind));
        }
        d.blocks.insert(id.0.clone(), Arc::new(b));
    }

    for f in project.functions_on(page) {
        let device = project.device(f.device)?;
        let sym = project.symbol(&f.symbol)?;
        // Labels are placed in page coordinates, not inside the block, so they stay upright
        // and readable whatever the symbol's rotation.
        let mut sideways = false;
        for cp in &sym.connection_points {
            let (at, leaves) = project.connection_point(&ecad_model::ConnectionRef::new(f.id, &cp.name))?;
            sideways |= !leaves.is_vertical();
            let label = point_label(at, leaves, &cp.name);
            d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(label)).map_err(err)?;
        }
        let (min, max) = project.function_bounds(f.id)?.unwrap_or((f.position, f.position));
        let tag = Attrib {
            tag: "TAG".into(),
            text: tag_text(min, max, sideways, &device.tag),
            invisible: false,
            constant: false,
            prompt: "Device tag".into(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let insert = Insert {
            block: f.symbol.0.clone(),
            insert: v(f.position),
            rotation: f.rotation.degrees().to_radians(),
            attribs: vec![tag],
            ..Insert::default()
        };
        d.add(&Space::Model, on(LAYER_SYMBOLS), EntityKind::Insert(insert)).map_err(err)?;
    }

    // Colour 150 (mid blue) reads on dark screens and on white paper; pure blue (5) vanishes on dark.
    let wire = Common { color: Color::from_aci(150), ..on(LAYER_WIRES) };
    for c in project.connections_on(page) {
        let route = project.route(c)?;
        for seg in route.windows(2) {
            d.add(&Space::Model, wire.clone(), EntityKind::Line(Line { a: v(seg[0]), b: v(seg[1]) })).map_err(err)?;
        }
        if let Some(n) = &c.wire_number {
            d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(wire_label(&route, n))).map_err(err)?;
        }
    }
    Ok(d)
}

/// What a primitive draws, in page millimetres.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "points", rename_all = "lowercase")]
pub enum Shape {
    /// A connected polyline.
    Polyline(Vec<Point>),
    /// Filled triangles, three points each (text glyphs, fills).
    Triangles(Vec<Point>),
}

/// One drawable piece of a page, already resolved to a colour for the given background.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Primitive {
    pub rgb: [u8; 3],
    pub shape: Shape,
}

/// A page as plain polylines and triangles that any canvas (egui, a web canvas) can draw
/// without knowing about CADCraft. `background` decides how white/black (colour 7) resolve.
pub fn page_primitives(project: &Project, page: PageId, background: [u8; 3]) -> Result<Vec<Primitive>> {
    use cadcraft_render::{Kind, Options};
    let d = page_drawing(project, page)?;
    let list = cadcraft_render::build(&d, &Space::Model, &Options { tolerance: 0.05, ..Options::default() });
    let bg = cadcraft_doc::color::Rgb(background[0], background[1], background[2]);
    let pt = |p: &cadcraft_doc::geom::Vec2| Point::new(p.x, p.y);
    let mut out = Vec::with_capacity(list.prims.len());
    for prim in &list.prims {
        let c = prim.display_rgb(bg);
        let (start, len) = (prim.start as usize, prim.len as usize);
        let shape = match prim.kind {
            Kind::Polyline => list.verts.get(start..start + len).map(|v| Shape::Polyline(v.iter().map(pt).collect())),
            Kind::Tris => list.tris.get(start..start + len).map(|v| Shape::Triangles(v.iter().map(pt).collect())),
            // Schematic pages draw no points, rays, masks or images.
            _ => None,
        };
        if let Some(shape) = shape {
            out.push(Primitive { rgb: [c.0, c.1, c.2], shape });
        }
    }
    Ok(out)
}

/// The page as DXF text.
pub fn page_dxf(project: &Project, page: PageId) -> Result<String> {
    Ok(cadcraft_io::write_dxf(&page_drawing(project, page)?))
}

/// The page as a PNG image, fitted to its extents.
pub fn page_png(project: &Project, page: PageId, width: u32, height: u32) -> Result<Vec<u8>> {
    let d = page_drawing(project, page)?;
    cadcraft_io::png(&d, &Space::Model, width, height).map_err(|e| ViewError::Drawing(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::sample::{PAGE_1, start_circuit};

    #[test]
    fn dxf_round_trip_keeps_device_tags() {
        let p = start_circuit().unwrap();
        let back = cadcraft_io::read_dxf(page_dxf(&p, PAGE_1).unwrap().as_bytes()).unwrap();
        let mut tags: Vec<String> = back
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Insert(i) => i.attribs.first().map(|a| a.text.value.clone()),
                _ => None,
            })
            .collect();
        tags.sort();
        assert_eq!(tags, ["-K1", "-S1", "-X1:L+", "-X1:M"]);
        let wires = back.model.iter().filter(|e| e.common.layer == LAYER_WIRES).count();
        assert_eq!(wires, 3, "three straight wires in the unmoved sample");
    }

    #[test]
    fn primitives_cover_the_page_with_lines_and_text() {
        let p = start_circuit().unwrap();
        let prims = page_primitives(&p, PAGE_1, [0, 0, 0]).unwrap();
        let lines = prims.iter().filter(|p| matches!(p.shape, Shape::Polyline(_))).count();
        // 3 wires + 4 symbols' lines (11 distinct, drawn per insert) + stroked text glyphs.
        assert!(lines > 50, "wires, symbol lines and text strokes: {lines}");
        // Colour 7 resolves to white on a black background and black on a white one.
        assert!(prims.iter().any(|p| p.rgb == [255, 255, 255]));
        let light = page_primitives(&p, PAGE_1, [255, 255, 255]).unwrap();
        assert!(light.iter().any(|p| p.rgb == [0, 0, 0]));
    }
}
