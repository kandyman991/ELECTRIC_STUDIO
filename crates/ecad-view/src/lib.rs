//! Turns model pages into CADCraft drawings and exports them.
//!
//! This is the only crate allowed to depend on CADCraft. Its public API speaks model types and
//! bytes, so nothing above it ever sees a CADCraft type, and a CADCraft upgrade can only break
//! this crate.

use cadcraft_doc::{
    Attrib, Block, Circle, Common, Drawing, Entity, EntityKind, HAlign, Insert, Line, Space, Text, VAlign,
    color::Color, geom::Vec3,
};
use ecad_model::{Direction, Graphic, PageId, Point, Project, Role};
use std::sync::Arc;

pub const LAYER_SYMBOLS: &str = "SYMBOLS";
pub const LAYER_WIRES: &str = "WIRES";
pub const LAYER_TAGS: &str = "TAGS";
pub const LAYER_FRAME: &str = "FRAME";
pub const LAYER_XREF: &str = "XREF";
/// Text height of cross-reference lines.
pub const XREF_HEIGHT: f64 = 1.8;

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

/// Cross-reference text for a function: under a coil, one line per contact
/// (`13-14 NO 2.4`); at a contact of a device that has a coil, where that coil is (`/1.3`).
/// Each entry is (line index, text).
fn cross_reference_lines(project: &Project, f: ecad_model::FunctionId, role: Role) -> Result<Vec<(usize, String)>> {
    let device = project.function(f)?.device;
    Ok(match role {
        Role::Main => project
            .contacts_of(device)
            .iter()
            .enumerate()
            .map(|(i, c)| (i, format!("{} {} {}", c.terminals, c.kind.short(), c.location)))
            .collect(),
        Role::Contact { .. } => match project.main_function(device) {
            Some(main) => vec![(0, format!("/{}", project.function_location(main.id)?))],
            None => Vec::new(),
        },
        // An interruption point shows where its partners are.
        Role::Interruption => project
            .functions_of(device)
            .filter(|g| g.id != f)
            .filter_map(|g| project.function_location(g.id).ok())
            .enumerate()
            .map(|(i, at)| (i, format!("/{at}")))
            .collect(),
        // A PLC channel shows its address and symbol: `I0.0 Door_Closed`.
        Role::PlcChannel { .. } => {
            let g = project.function(f)?;
            let line = [g.property(ecad_model::ADDRESS), g.property(ecad_model::SYMBOL)]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if line.is_empty() { Vec::new() } else { vec![(0, line)] }
        }
        Role::Single | Role::Potential | Role::Terminal => Vec::new(),
    })
}

/// Where cross-reference lines go: under the tag, which sits right of an upright symbol and
/// above a sideways one (so the lines go below a sideways symbol instead, clear of the tag).
fn line_text(min: Point, max: Point, sideways: bool, (i, s): &(usize, String)) -> Text {
    let step = XREF_HEIGHT * 1.5;
    let at = if sideways {
        Point::new(min.x, min.y - TAG_GAP - step * *i as f64)
    } else {
        Point::new(max.x + TAG_GAP, (min.y + max.y) / 2.0 - TAG_HEIGHT - step * *i as f64)
    };
    justified(at, XREF_HEIGHT, s, HAlign::Left, VAlign::Top)
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
    draw_sheet(&mut d, project, page)?;

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
            if !sym.hide_point_labels {
                let label = point_label(at, leaves, f.designation(&cp.name));
                d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(label)).map_err(err)?;
            }
        }
        let (min, max) = project.function_bounds(f.id)?.unwrap_or((f.position, f.position));
        for line in cross_reference_lines(project, f.id, sym.role)? {
            d.add(&Space::Model, on(LAYER_XREF), EntityKind::Text(line_text(min, max, sideways, &line)))
                .map_err(err)?;
        }
        let tag = Attrib {
            tag: "TAG".into(),
            // A terminal or PLC channel is labelled with its device and number: -X2:3, -KF1:0.
            text: tag_text(
                min,
                max,
                sideways,
                &match project.drawing_number(f.id) {
                    Some(n) => format!("{}:{n}", device.tag),
                    None => device.tag.clone(),
                },
            ),
            // Kept in the DXF even when not shown (junctions), so the drawing stays complete.
            invisible: sym.hide_tag,
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

fn add_line(d: &mut Drawing, common: &Common, a: Point, b: Point) -> Result<()> {
    d.add(&Space::Model, common.clone(), EntityKind::Line(Line { a: v(a), b: v(b) }))
        .map(|_| ())
        .map_err(|e| ViewError::Drawing(e.to_string()))
}

fn add_rect(d: &mut Drawing, common: &Common, lo: Point, hi: Point) -> Result<()> {
    let (a, b, c, e) = (lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y));
    for (p, q) in [(a, b), (b, c), (c, e), (e, a)] {
        add_line(d, common, p, q)?;
    }
    Ok(())
}

fn add_text(d: &mut Drawing, common: &Common, t: Text) -> Result<()> {
    d.add(&Space::Model, common.clone(), EntityKind::Text(t)).map(|_| ()).map_err(|e| ViewError::Drawing(e.to_string()))
}

/// The sheet: outer edge, frame, numbered column band and the title block.
fn draw_sheet(d: &mut Drawing, project: &Project, page: PageId) -> Result<()> {
    use ecad_model::sheet::{COLUMN_BAND, TITLE_BLOCK_HEIGHT};
    let sheet = &project.sheet;
    // Colour 8 (grey) reads on dark screens and prints light on paper; text stays colour 7.
    let lines = Common { color: Color::from_aci(8), ..on(LAYER_FRAME) };
    let words = on(LAYER_FRAME);
    let (s_lo, s_hi) = sheet.bounds();
    add_rect(d, &lines, s_lo, s_hi)?;
    let (lo, hi) = sheet.frame();
    add_rect(d, &lines, lo, hi)?;

    let band = hi.y - COLUMN_BAND;
    add_line(d, &lines, Point::new(lo.x, band), Point::new(hi.x, band))?;
    for n in 1..=sheet.columns {
        let (left, right) = sheet.column_span(n);
        if n > 1 {
            add_line(d, &lines, Point::new(left, band), Point::new(left, hi.y))?;
        }
        let at = Point::new((left + right) / 2.0, band + COLUMN_BAND / 2.0);
        add_text(d, &words, justified(at, 2.5, &n.to_string(), HAlign::Center, VAlign::Middle))?;
    }

    let this = project.page(page)?;
    let (t_lo, t_hi) = sheet.title_block();
    add_rect(d, &lines, t_lo, t_hi)?;
    let split_x = t_hi.x - 40.0;
    let mid_y = t_lo.y + TITLE_BLOCK_HEIGHT / 2.0;
    add_line(d, &lines, Point::new(split_x, t_lo.y), Point::new(split_x, t_hi.y))?;
    add_line(d, &lines, Point::new(t_lo.x, mid_y), Point::new(split_x, mid_y))?;
    let caption = |x: f64, y: f64, s: &str| justified(Point::new(x, y), 1.8, s, HAlign::Left, VAlign::Top);
    let value = |x: f64, y: f64, s: &str| text(Point::new(x, y), 3.5, s);
    add_text(d, &words, caption(t_lo.x + 2.0, t_hi.y - 1.5, "Project"))?;
    add_text(d, &words, value(t_lo.x + 2.0, mid_y + 3.0, &project.name))?;
    add_text(d, &words, caption(t_lo.x + 2.0, mid_y - 1.5, "Page content"))?;
    add_text(d, &words, value(t_lo.x + 2.0, t_lo.y + 3.0, &this.description))?;
    add_text(d, &words, caption(split_x + 2.0, t_hi.y - 1.5, "Page"))?;
    let page_at = Point::new((split_x + t_hi.x) / 2.0, mid_y + 1.0);
    add_text(d, &words, justified(page_at, 7.0, &this.name, HAlign::Center, VAlign::Middle))?;
    let index = project.pages_in_order().iter().position(|p| p.id == page).map_or(0, |i| i + 1);
    let count = format!("{index} of {}", project.pages.len());
    add_text(
        d,
        &words,
        justified(Point::new((split_x + t_hi.x) / 2.0, t_lo.y + 2.0), 2.5, &count, HAlign::Center, VAlign::Bottom),
    )?;
    Ok(())
}

/// The page as DXF text.
pub fn page_dxf(project: &Project, page: PageId) -> Result<String> {
    Ok(cadcraft_io::write_dxf(&page_drawing(project, page)?))
}

/// The page as a DWG file (AutoCAD 2004 format or later, as CADCraft chooses).
pub fn page_dwg(project: &Project, page: PageId) -> Result<Vec<u8>> {
    // The DWG writer runs third-party code that should be isolated from untrusted input; this
    // input is a drawing this process just built, which is the case the in-process path is for.
    cadcraft_io::dwg_worker::allow_in_process();
    cadcraft_io::write(&page_drawing(project, page)?, "page.dwg").map_err(|e| ViewError::Drawing(e.to_string()))
}

/// Gap between pages laid side by side in [`project_pdf`]'s working drawing, in mm.
const PDF_PAGE_GAP: f64 = 50.0;

/// The whole project as one PDF: every page in order, one A3 landscape sheet each, at 1:1
/// (the sheet is 420 × 297 mm). Drawing layers become PDF layers. Text is drawn as outlines
/// (CADCraft's stroke font), so it prints exactly as on screen but can't be searched yet.
pub fn project_pdf(project: &Project) -> Result<Vec<u8>> {
    use cadcraft_doc::geom::{Bounds2, Mat3, Vec2};
    use cadcraft_io::pdf::PdfOptions;
    let pages = project.pages_in_order();
    // CADCraft plots sheets of one drawing, so the pages are laid side by side in one drawing
    // and each PDF page plots its window.
    let mut d = Drawing::new_metric();
    let mut items = Vec::new();
    let (lo, hi) = project.sheet.bounds();
    let step = (hi.x - lo.x) + PDF_PAGE_GAP;
    let err = |e: cadcraft_doc::DocError| ViewError::Drawing(e.to_string());
    for (i, page) in pages.iter().enumerate() {
        let dx = step * i as f64;
        let part = page_drawing(project, page.id)?;
        for (name, block) in &part.blocks {
            d.blocks.entry(name.clone()).or_insert_with(|| block.clone());
        }
        for layer in &part.layers {
            if !d.layers.iter().any(|l| l.name == layer.name) {
                d.layers.push(layer.clone());
            }
        }
        let shift = Mat3::translate(Vec2::new(dx, 0.0));
        for e in part.model.iter() {
            let mut kind = e.kind.clone();
            kind.transform(&shift);
            d.add(&Space::Model, e.common.clone(), kind).map_err(err)?;
        }
        let window = Bounds2::new(Vec2::new(lo.x + dx, lo.y), Vec2::new(hi.x + dx, hi.y));
        let sheet = PdfOptions {
            paper_mm: Some((project.sheet.height, project.sheet.width)),
            landscape: Some(true),
            fit: Some(true),
            margins: Some([0.0; 4]),
            window: Some(window),
            title: page.name.clone(),
            ..PdfOptions::default()
        };
        items.push((Space::Model, sheet));
    }
    let file = PdfOptions { title: project.name.clone(), compress: true, ..PdfOptions::default() };
    cadcraft_io::pdf::pdf_items(&d, &items, &file).map_err(|e| ViewError::Drawing(e.to_string()))
}

/// Safe file-name stem for a page: its number, with anything but letters, digits, `-`, `_` and
/// `.` replaced by `_` (page `3/a` → `page-3_a`).
pub fn page_file_stem(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    format!("page-{clean}")
}

/// Which drawing files [`drawing_files`] writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formats {
    pub pdf: bool,
    pub dxf: bool,
    pub dwg: bool,
    pub png: bool,
}

impl Formats {
    pub const ALL: Formats = Formats { pdf: true, dxf: true, dwg: true, png: true };
    pub const NONE: Formats = Formats { pdf: false, dxf: false, dwg: false, png: false };
}

/// Drawing outputs, as (file name, contents): `drawings.pdf` with all pages, plus a DXF, a DWG
/// and a PNG preview per page (`page-1.dxf`…), as far as `formats` asks for them.
pub fn drawing_files(project: &Project, formats: Formats) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    if formats.pdf {
        files.push(("drawings.pdf".to_owned(), project_pdf(project)?));
    }
    for page in project.pages_in_order() {
        let stem = page_file_stem(&page.name);
        if formats.dxf {
            files.push((format!("{stem}.dxf"), page_dxf(project, page.id)?.into_bytes()));
        }
        if formats.dwg {
            files.push((format!("{stem}.dwg"), page_dwg(project, page.id)?));
        }
        if formats.png {
            files.push((format!("{stem}.png"), page_png(project, page.id, 1200, 1200)?));
        }
    }
    Ok(files)
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
        assert_eq!(tags, ["-K1", "-S1", "L+", "M"]);
        let wires = back.model.iter().filter(|e| e.common.layer == LAYER_WIRES).count();
        assert_eq!(wires, 3, "three straight wires in the unmoved sample");
    }

    #[test]
    fn dwg_round_trip_keeps_device_tags() {
        let p = start_circuit().unwrap();
        let back = cadcraft_io::read(&page_dwg(&p, PAGE_1).unwrap(), "page.dwg").unwrap();
        let mut tags: Vec<String> = back
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Insert(i) => i.attribs.first().map(|a| a.text.value.clone()),
                _ => None,
            })
            .collect();
        tags.sort();
        assert_eq!(tags, ["-K1", "-S1", "L+", "M"]);
    }

    #[test]
    fn the_project_pdf_has_one_a3_page_per_page_in_order() {
        use cadcraft_io::pdf_read::{PdfImportOptions, read};
        let p = ecad_model::sample::with_plc().unwrap();
        let pdf = project_pdf(&p).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        for page in 1..=3 {
            let (d, pages) = read(&pdf, &PdfImportOptions { page, ..PdfImportOptions::default() }).unwrap();
            assert_eq!(pages, 3);
            // The sheet's outer edge fills the A3 page (420 x 297 mm) at 1:1.
            let b = d.extents(&Space::Model);
            let size = b.max - b.min;
            assert!((size.x - 420.0).abs() < 1.0 && (size.y - 297.0).abs() < 1.0, "page {page}: {b:?}");
        }
        assert_eq!(project_pdf(&p).unwrap(), pdf, "same project, same bytes");
    }

    #[test]
    fn drawing_files_cover_every_page_with_safe_names() {
        assert_eq!(page_file_stem("3/a b"), "page-3_a_b");
        let p = ecad_model::sample::with_contacts().unwrap();
        let names: Vec<String> = drawing_files(&p, Formats::ALL).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["drawings.pdf", "page-1.dxf", "page-1.dwg", "page-1.png", "page-2.dxf", "page-2.dwg", "page-2.png"]
        );
    }

    fn texts_on(project: &Project, page: PageId, layer: &str) -> Vec<String> {
        let d = page_drawing(project, page).unwrap();
        let mut v: Vec<String> = d
            .model
            .iter()
            .filter(|e| e.common.layer == layer)
            .filter_map(|e| match &e.kind {
                EntityKind::Text(t) => Some(t.value.clone()),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn coils_list_their_contacts_and_contacts_point_back() {
        use ecad_model::sample::{K1_COIL, K1_NC, K1_NO, PAGE_2, with_contacts};
        let p = with_contacts().unwrap();
        let col = |f| p.sheet.column_of(p.function(f).unwrap().position);
        assert_eq!(
            texts_on(&p, PAGE_1, LAYER_XREF),
            [format!("13-14 NO 2.{}", col(K1_NO)), format!("21-22 NC 2.{}", col(K1_NC))]
        );
        let back = format!("/1.{}", col(K1_COIL));
        assert_eq!(texts_on(&p, PAGE_2, LAYER_XREF), [back.clone(), back]);
        // The contacts show their own terminal numbers.
        let labels = texts_on(&p, PAGE_2, LAYER_TAGS);
        assert!(["13", "14", "21", "22"].iter().all(|d| labels.contains(&d.to_string())), "{labels:?}");
    }

    #[test]
    fn interruption_points_show_their_partner_and_junctions_hide_their_tag() {
        use ecad_model::FunctionId;
        use ecad_model::sample::{PAGE_2, with_potentials};
        let p = with_potentials().unwrap();
        let loc = |f: u128| p.function_location(FunctionId::from_u128(f)).unwrap();
        // I1 on page 2 (0x208) points to page 1 (0x209) and back.
        assert!(texts_on(&p, PAGE_2, LAYER_XREF).contains(&format!("/{}", loc(0x209))));
        assert!(texts_on(&p, PAGE_1, LAYER_XREF).contains(&format!("/{}", loc(0x208))));
        let d = page_drawing(&p, PAGE_1).unwrap();
        let junction_tag = d.model.iter().find_map(|e| match &e.kind {
            EntityKind::Insert(i) if i.block == "iec.junction" => {
                i.attribs.first().map(|a| (a.text.value.clone(), a.invisible))
            }
            _ => None,
        });
        assert_eq!(junction_tag, Some(("J1".to_owned(), true)));
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
