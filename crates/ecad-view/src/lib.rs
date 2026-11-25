//! Turns model pages into CADCraft drawings and exports them.
//!
//! This is the only crate allowed to depend on CADCraft. Its public API speaks model types and
//! bytes, so nothing above it ever sees a CADCraft type, and a CADCraft upgrade can only break
//! this crate.

use cadcraft_doc::{
    Attrib, Block, Circle, Common, Drawing, Entity, EntityKind, HAlign, Insert, Line, Space, Text,
    VAlign, color::Color, geom::Vec3,
};
use ecad_model::{Graphic, PageId, Point, Project};
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

fn on(layer: &str) -> Common {
    Common {
        layer: layer.to_owned(),
        ..Common::default()
    }
}

/// Build the drawing for one page. Symbols become block definitions, functions become inserts
/// carrying the device tag as a `TAG` attribute, and wires are routed from the model.
pub fn page_drawing(project: &Project, page: PageId) -> Result<Drawing> {
    project.page(page)?;
    let mut d = Drawing::new_metric();
    let err = |e: cadcraft_doc::DocError| ViewError::Drawing(e.to_string());

    // Only the symbols this page uses, in a stable order.
    let mut used: Vec<_> = project
        .functions_on(page)
        .map(|f| f.symbol.clone())
        .collect();
    used.sort();
    used.dedup();
    for id in &used {
        let sym = project.symbol(id)?;
        let mut b = Block::new(&id.0);
        b.description = sym.description.clone();
        for g in &sym.graphics {
            let kind = match g {
                Graphic::Line { a, b } => EntityKind::Line(Line { a: v(*a), b: v(*b) }),
                Graphic::Circle { center, radius } => EntityKind::Circle(Circle {
                    center: v(*center),
                    radius: *radius,
                }),
                Graphic::Text { at, height, value } => EntityKind::Text(text(*at, *height, value)),
            };
            let h = d.new_handle();
            b.entities.push(Entity::new(h, kind));
        }
        for cp in &sym.connection_points {
            let h = d.new_handle();
            let label = text(Point::new(cp.at.x + 1.0, cp.at.y - 2.5), 1.8, &cp.name);
            b.entities.push(Entity::new(h, EntityKind::Text(label)));
        }
        d.blocks.insert(id.0.clone(), Arc::new(b));
    }

    for f in project.functions_on(page) {
        let device = project.device(f.device)?;
        let tag = Attrib {
            tag: "TAG".into(),
            text: text(
                Point::new(f.position.x + 7.0, f.position.y),
                2.5,
                &device.tag,
            ),
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
        d.add(&Space::Model, on(LAYER_SYMBOLS), EntityKind::Insert(insert))
            .map_err(err)?;
    }

    let wire = Common {
        color: Color::from_aci(5),
        ..on(LAYER_WIRES)
    };
    for c in project.connections_on(page) {
        let route = project.route(c)?;
        for seg in route.windows(2) {
            d.add(
                &Space::Model,
                wire.clone(),
                EntityKind::Line(Line {
                    a: v(seg[0]),
                    b: v(seg[1]),
                }),
            )
            .map_err(err)?;
        }
        if let Some(n) = &c.wire_number {
            let mid = route[route.len() / 2];
            let label = text(Point::new(mid.x + 1.0, mid.y + 1.0), 1.8, n);
            d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(label))
                .map_err(err)?;
        }
    }
    Ok(d)
}

/// The page as DXF text.
pub fn page_dxf(project: &Project, page: PageId) -> Result<String> {
    Ok(cadcraft_io::write_dxf(&page_drawing(project, page)?))
}

/// The page as a PNG image, fitted to its extents.
pub fn page_png(project: &Project, page: PageId, width: u32, height: u32) -> Result<Vec<u8>> {
    let d = page_drawing(project, page)?;
    cadcraft_io::png(&d, &Space::Model, width, height)
        .map_err(|e| ViewError::Drawing(e.to_string()))
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
        let wires = back
            .model
            .iter()
            .filter(|e| e.common.layer == LAYER_WIRES)
            .count();
        assert_eq!(wires, 3, "three straight wires in the unmoved sample");
    }
}
