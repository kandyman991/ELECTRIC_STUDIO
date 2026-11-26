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

pub mod clash;
mod place;

use place::{Placer, Side, TagSize};

pub const LAYER_SYMBOLS: &str = "SYMBOLS";
pub const LAYER_WIRES: &str = "WIRES";
pub const LAYER_TAGS: &str = "TAGS";
pub const LAYER_FRAME: &str = "FRAME";
pub const LAYER_XREF: &str = "XREF";
pub const LAYER_BOXES: &str = "BOXES";
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
pub(crate) fn justified(at: Point, height: f64, value: &str, halign: HAlign, valign: VAlign) -> Text {
    let mut t = text(at, height, value);
    if (halign, valign) != (HAlign::Left, VAlign::Baseline) {
        t.align_pt = Some(v(at));
    }
    t.halign = halign;
    t.valign = valign;
    t
}

/// Gap between a connection point and its designation, in mm.
pub(crate) const LABEL_GAP: f64 = 1.0;
/// Gap between a symbol's box and its device tag, in mm.
pub(crate) const TAG_GAP: f64 = 2.5;
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

/// Whether a terminal or channel has one of the same device just left of it on the same page,
/// 8 mm away at the same height: then its label leaves out the strip's name.
fn continues_row(project: &Project, f: &ecad_model::Function) -> bool {
    project.functions_of(f.device).any(|g| {
        g.id != f.id
            && g.page == f.page
            && (g.position.y - f.position.y).abs() < 0.01
            && (f.position.x - g.position.x - 8.0).abs() < 0.01
    })
}

/// Which side of a symbol its tag and cross-references go on first: the first of right, left
/// and above that no wire leaves from. (They move elsewhere if that side is crowded.)
fn tag_side(leaving: &[Direction]) -> Side {
    if !leaving.contains(&Direction::Right) {
        Side::Right
    } else if !leaving.contains(&Direction::Left) {
        Side::Left
    } else {
        Side::Above
    }
}

/// Cross-reference text for a function: under a coil, one line per contact
/// (`13-14 NO 2.4`); at a contact of a device that has a coil, where that coil is (`/1.3`).
fn cross_reference_lines(project: &Project, f: ecad_model::FunctionId, role: Role) -> Result<Vec<String>> {
    let device = project.function(f)?.device;
    Ok(match role {
        // Drawn as a contact table instead.
        Role::Main => Vec::new(),
        Role::Contact { .. } => match project.main_function(device) {
            Some(main) => vec![project.reference(main.id)?],
            None => Vec::new(),
        },
        // An interruption point shows where its potential goes on: the next page that has a
        // partner (or, from the last such page, the first), not every page that uses it.
        Role::Interruption => {
            let order: Vec<PageId> = project.pages_in_order().iter().map(|p| p.id).collect();
            let rank = |page: PageId| order.iter().position(|p| *p == page).unwrap_or(usize::MAX);
            let here = rank(project.function(f)?.page);
            let mut others: Vec<&ecad_model::Function> =
                project.functions_of(device).filter(|g| g.id != f && rank(g.page) != here).collect();
            others.sort_by_key(|g| rank(g.page));
            let next = others.iter().find(|g| rank(g.page) > here).or_else(|| others.first());
            next.and_then(|g| project.reference(g.id).ok()).map(|r| vec![r]).unwrap_or_default()
        }
        // A PLC channel shows its address and symbol: `I0.0 Door_Closed`.
        Role::PlcChannel { .. } => {
            let g = project.function(f)?;
            let line = [g.property(ecad_model::ADDRESS), g.property(ecad_model::SYMBOL)]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if line.is_empty() { Vec::new() } else { vec![line] }
        }
        Role::Single | Role::Potential | Role::Terminal => Vec::new(),
    })
}

/// The contact table under a coil or breaker: one row per pole of each of its contacts, as in
/// `14 _/_ 13  2/4C`: the designations around a small NO or NC contact, then where it is drawn.
/// `top` is the top-left corner of the first row.
fn contact_table(project: &Project, main: ecad_model::FunctionId, top: Point) -> Result<ContactTable> {
    let device = project.function(main)?.device;
    let step = XREF_HEIGHT * 1.5;
    let mut table = ContactTable::default();
    let mut row = 0.0;
    for c in project.contacts_of(device) {
        for (first, second) in &c.poles {
            let y = top.y - step * row - XREF_HEIGHT / 2.0;
            let x = top.x;
            let mid = |at: f64, s: &str, h: HAlign| justified(Point::new(at, y), XREF_HEIGHT, s, h, VAlign::Middle);
            table.texts.push(mid(x + 3.5, second, HAlign::Right));
            let glyph = x + 4.2;
            let base = y - 0.6;
            table.lines.extend([
                (Point::new(glyph, base), Point::new(glyph + 1.0, base)),
                (Point::new(glyph + 2.0, base), Point::new(glyph + 3.0, base)),
            ]);
            match c.kind {
                // The blade crosses under the fixed contact and a hook closes it.
                ecad_model::ContactKind::NormallyClosed => table.lines.extend([
                    (Point::new(glyph + 1.0, base), Point::new(glyph + 2.4, base - 0.7)),
                    (Point::new(glyph + 2.0, base), Point::new(glyph + 2.0, base - 0.9)),
                ]),
                _ => table.lines.push((Point::new(glyph + 1.0, base), Point::new(glyph + 2.3, base + 1.1))),
            }
            table.texts.push(mid(x + 7.8, first, HAlign::Left));
            table.texts.push(mid(x + 13.0, &c.location, HAlign::Left));
            row += 1.0;
        }
    }
    Ok(table)
}

/// A coil's contact table: its texts and the lines of its little contact symbols.
#[derive(Default)]
struct ContactTable {
    texts: Vec<Text>,
    lines: Vec<(Point, Point)>,
}

/// Width of a contact table, in mm (to the end of a typical location like `12/3C`).
const TABLE_WIDTH: f64 = 22.0;

/// Top-left corners to try for a coil's contact table: under the tag on its preferred side, on
/// the other side, then further and further down under the symbol (below a rail if need be, as
/// printed drawings do in dense columns).
fn table_corners(min: Point, max: Point, preferred: Side, floor: f64) -> Vec<Point> {
    let corner = |side| {
        let c = table_corner(min, max, side);
        Point::new(c.x, c.y)
    };
    let mut out = vec![corner(preferred)];
    for side in [Side::Right, Side::Left] {
        if side != preferred {
            out.push(corner(side));
        }
    }
    let x = (min.x + max.x) / 2.0 - TABLE_WIDTH / 2.0;
    let mut y = min.y - TAG_GAP;
    while y > floor {
        out.push(Point::new(x, y));
        y -= 4.0;
    }
    out
}

/// The top-left corner of a coil's contact table: where the first cross-reference line would go
/// under its tag on the preferred side (below a sideways symbol).
fn table_corner(min: Point, max: Point, side: Side) -> Vec3 {
    let under_tag = (min.y + max.y) / 2.0 - TAG_HEIGHT;
    match side {
        Side::Right => Vec3::new(max.x + TAG_GAP, under_tag, 0.0),
        // The table reads left to right; start it far enough left to end at the symbol.
        Side::Left => Vec3::new(min.x - TAG_GAP - 24.0, under_tag, 0.0),
        _ => Vec3::new(min.x, min.y - TAG_GAP, 0.0),
    }
}

/// A dashed line as separate dashes (1.5 mm on, 1 mm off), so it looks the same in every
/// output without depending on linetype support.
fn dashes(a: Point, b: Point) -> Vec<(Point, Point)> {
    const ON: f64 = 1.5;
    const PERIOD: f64 = 2.5;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if len <= f64::EPSILON {
        return Vec::new();
    }
    let at = |t: f64| Point::new(a.x + dx * t / len, a.y + dy * t / len);
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < len {
        out.push((at(t), at((t + ON).min(len))));
        t += PERIOD;
    }
    out
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
            let kinds = match g {
                Graphic::Line { a, b } => vec![EntityKind::Line(Line { a: v(*a), b: v(*b) })],
                Graphic::Dashed { a, b } => {
                    dashes(*a, *b).into_iter().map(|(a, b)| EntityKind::Line(Line { a: v(a), b: v(b) })).collect()
                }
                Graphic::Circle { center, radius } => {
                    vec![EntityKind::Circle(Circle { center: v(*center), radius: *radius })]
                }
                Graphic::Text { at, height, value } => vec![EntityKind::Text(text(*at, *height, value))],
            };
            for kind in kinds {
                let h = d.new_handle();
                b.entities.push(Entity::new(h, kind));
            }
        }
        d.blocks.insert(id.0.clone(), Arc::new(b));
    }

    // First everything with a fixed place: symbols' point labels, contact tables, wires, cables
    // and location boxes. Then the tags, cross-references and wire numbers, each where it
    // doesn't sit on any of those or on each other.
    struct Tagged {
        id: ecad_model::FunctionId,
        symbol: String,
        position: Point,
        rotation: f64,
        bounds: (Point, Point),
        preferred: Side,
        tag: String,
        size: TagSize,
        /// A terminal or PLC channel, labelled with its number.
        numbered: bool,
        lines: Vec<String>,
        hidden: bool,
        table: bool,
    }
    let mut tagged = Vec::new();
    let mut placer = {
        use ecad_model::sheet::COLUMN_BAND;
        let (flo, fhi) = project.sheet.frame();
        let (_, tb) = project.sheet.title_block();
        Placer::new((Point::new(flo.x, tb.y), Point::new(fhi.x, fhi.y - COLUMN_BAND)))
    };
    for f in project.functions_on(page) {
        let device = project.device(f.device)?;
        let sym = project.symbol(&f.symbol)?;
        // Labels are placed in page coordinates, not inside the block, so they stay upright
        // and readable whatever the symbol's rotation.
        let (min, max) = project.function_bounds(f.id)?.unwrap_or((f.position, f.position));
        placer.add_symbol(f.id, min, max);
        // The tag keeps clear of the symbol and of its point labels, which may stick out.
        let (mut tag_min, mut tag_max) = (min, max);
        let mut leaving = Vec::new();
        for cp in &sym.connection_points {
            let (at, leaves) = project.connection_point(&ecad_model::ConnectionRef::new(f.id, &cp.name))?;
            leaving.push(leaves);
            if !sym.hide_point_labels {
                let label = point_label(at, leaves, f.designation(&cp.name));
                if let Some(r) = clash::text_rect(&label) {
                    tag_min = Point::new(tag_min.x.min(r.lo.x), tag_min.y.min(r.lo.y));
                    tag_max = Point::new(tag_max.x.max(r.hi.x), tag_max.y.max(r.hi.y));
                }
                d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(label)).map_err(err)?;
            }
        }
        let side = tag_side(&leaving);
        let lines = if sym.role == Role::Main { Vec::new() } else { cross_reference_lines(project, f.id, sym.role)? };
        tagged.push(Tagged {
            id: f.id,
            symbol: f.symbol.0.clone(),
            position: f.position,
            rotation: f.rotation.degrees().to_radians(),
            bounds: (tag_min, tag_max),
            preferred: side,
            // A terminal or PLC channel is labelled with its device and number: -X2:3, -KF1:0.
            tag: match project.drawing_number(f.id) {
                // In a row of terminals of one strip, only the first names the strip.
                Some(n) if continues_row(project, f) => format!(":{n}"),
                Some(n) => format!("{}:{n}", device.tag),
                None => device.tag.clone(),
            },
            numbered: project.drawing_number(f.id).is_some(),
            size: if project.drawing_number(f.id).is_some() && continues_row(project, f) {
                TagSize::COMPACT
            } else {
                TagSize::NORMAL
            },
            lines,
            hidden: sym.hide_tag,
            table: sym.role == Role::Main,
        });
    }

    // Colour 150 (mid blue) reads on dark screens and on white paper; pure blue (5) vanishes on dark.
    let wire = Common { color: Color::from_aci(150), ..on(LAYER_WIRES) };
    let mut numbered = Vec::new();
    for c in project.connections_on(page) {
        let route = project.route(c)?;
        for seg in route.windows(2) {
            d.add(&Space::Model, wire.clone(), EntityKind::Line(Line { a: v(seg[0]), b: v(seg[1]) })).map_err(err)?;
            placer.add_wire(seg[0], seg[1]);
        }
        // Cross-section and colour go just after the number: `2,5 BU`.
        let spec = [c.spec.cross_section.as_str(), c.spec.colour.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if c.wire_number.is_some() || !spec.is_empty() {
            numbered.push((route, c.wire_number.clone(), spec));
        }
    }
    draw_cables(&mut d, project, page)?;
    draw_boxes(&mut d, project, page)?;
    for e in d.model.iter() {
        if let EntityKind::Text(t) = &e.kind {
            placer.add_text(t);
        }
    }
    for b in project.boxes_on(page) {
        let (lo, hi) = (b.min, b.max);
        let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y), lo];
        for side in corners.windows(2) {
            placer.add_edge(side[0], side[1]);
        }
    }
    for b in project.boxes_on(page).filter(|b| !b.label.is_empty()) {
        for t in placer.choose(box_label_candidates(b.min, b.max, &b.label), None) {
            d.add(&Space::Model, on(LAYER_BOXES), EntityKind::Text(t)).map_err(err)?;
        }
    }

    // Contact tables first: they are big and have few places to go.
    let floor = placer.floor();
    for t in tagged.iter().filter(|t| t.table) {
        let (min, max) = t.bounds;
        let mut tables = Vec::new();
        for corner in table_corners(min, max, t.preferred, floor) {
            tables.push(contact_table(project, t.id, corner)?);
        }
        let candidates: Vec<Vec<Text>> = tables.iter().map(|tb| tb.texts.clone()).collect();
        if let Some(i) = placer.choose_index(&candidates, Some(t.id))
            && let Some(table) = tables.into_iter().nth(i)
        {
            let layer = on(LAYER_XREF);
            for text in table.texts {
                add_text(&mut d, &layer, text)?;
            }
            for (a, b) in table.lines {
                add_line(&mut d, &layer, a, b)?;
            }
        }
    }
    for t in tagged {
        let (min, max) = t.bounds;
        let texts = if t.hidden {
            // Not shown, but kept in the DXF so the drawing stays complete.
            place::tag_block(min, max, t.preferred, t.size, &t.tag, &[])
        } else {
            let mut sides = vec![t.preferred];
            sides.extend(Side::ALL.iter().filter(|s| **s != t.preferred));
            // A terminal's or pin's label may shrink to the compact size where the normal one
            // finds no clear place.
            let mut sizes = vec![t.size];
            if t.numbered && t.size.height > TagSize::COMPACT.height {
                sizes.push(TagSize::COMPACT);
            }
            let candidates = sizes
                .iter()
                .flat_map(|size| sides.iter().map(|side| place::tag_block(min, max, *side, *size, &t.tag, &t.lines)))
                .collect();
            placer.choose(candidates, Some(t.id))
        };
        let mut texts = texts.into_iter();
        let Some(tag_text) = texts.next() else { continue };
        for line in texts {
            d.add(&Space::Model, on(LAYER_XREF), EntityKind::Text(line)).map_err(err)?;
        }
        let tag = Attrib {
            tag: "TAG".into(),
            text: tag_text,
            invisible: t.hidden,
            constant: false,
            prompt: "Device tag".into(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let insert = Insert {
            block: t.symbol,
            insert: v(t.position),
            rotation: t.rotation,
            attribs: vec![tag],
            ..Insert::default()
        };
        d.add(&Space::Model, on(LAYER_SYMBOLS), EntityKind::Insert(insert)).map_err(err)?;
    }

    // First every wire that has a clear place for its labels. Then the rest: a wire whose number
    // and spec another wire of the page already shows goes without (typically the short stretch
    // from an arrow or terminal to the next junction); the others take the least crowded place.
    let mut shown: Vec<(Option<String>, String)> = Vec::new();
    let mut crowded = Vec::new();
    for (route, number, spec) in numbered {
        let mut candidates = place::wire_candidates(&route, number.as_deref(), &spec);
        if candidates.is_empty() {
            // A wire of no length: label it where it is.
            let at = route.first().copied().unwrap_or(Point::ZERO);
            candidates.push(place::wire_labels(at, at, 0.5, false, number.as_deref(), &spec));
        }
        let texts = placer.choose_clear(candidates.clone(), None);
        if texts.is_empty() {
            crowded.push((candidates, number, spec));
            continue;
        }
        shown.push((number, spec));
        for t in texts {
            d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(t)).map_err(err)?;
        }
    }
    for (candidates, number, spec) in crowded {
        let key = (number, spec);
        if shown.contains(&key) {
            continue;
        }
        shown.push(key);
        for t in placer.choose(candidates, None) {
            d.add(&Space::Model, on(LAYER_TAGS), EntityKind::Text(t)).map_err(err)?;
        }
    }
    Ok(d)
}

/// Location boxes: a dashed rectangle with its label above the top-left corner.
fn draw_boxes(d: &mut Drawing, project: &Project, page: PageId) -> Result<()> {
    let layer = on(LAYER_BOXES);
    for b in project.boxes_on(page) {
        let (lo, hi) = (b.min, b.max);
        let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y), lo];
        for side in corners.windows(2) {
            for (a, z) in dashes(side[0], side[1]) {
                add_line(d, &layer, a, z)?;
            }
        }
    }
    Ok(())
}

/// Places for a location box's label: above its top-left corner, above its top-right corner,
/// inside along the top, below the bottom-left corner.
fn box_label_candidates(lo: Point, hi: Point, label: &str) -> Vec<Vec<Text>> {
    let t = |at: Point, h: HAlign, v: VAlign| vec![justified(at, TAG_HEIGHT, label, h, v)];
    let top = hi.y - LABEL_GAP;
    vec![
        // Above the box, at either end; inside its top corners; beside it at the top; below.
        t(Point::new(lo.x, hi.y + TAG_GAP), HAlign::Left, VAlign::Bottom),
        t(Point::new(hi.x, hi.y + TAG_GAP), HAlign::Right, VAlign::Bottom),
        t(Point::new(lo.x + LABEL_GAP, top), HAlign::Left, VAlign::Top),
        t(Point::new(hi.x - LABEL_GAP, top), HAlign::Right, VAlign::Top),
        t(Point::new(lo.x - LABEL_GAP, top), HAlign::Right, VAlign::Top),
        t(Point::new(hi.x + LABEL_GAP, top), HAlign::Left, VAlign::Top),
        t(Point::new(lo.x - LABEL_GAP, lo.y + LABEL_GAP), HAlign::Right, VAlign::Bottom),
        t(Point::new(hi.x + LABEL_GAP, lo.y + LABEL_GAP), HAlign::Left, VAlign::Bottom),
        t(Point::new(lo.x, lo.y - TAG_GAP), HAlign::Left, VAlign::Top),
    ]
}

/// Where a wire is marked as running in a cable: a third of the way along its longest segment.
fn cable_mark(route: &[Point]) -> Option<(Point, bool)> {
    let len = |s: &[Point]| (s[1].x - s[0].x).abs() + (s[1].y - s[0].y).abs();
    let seg = route.windows(2).max_by(|a, b| len(a).total_cmp(&len(b)))?;
    let at = Point::new(seg[0].x + (seg[1].x - seg[0].x) / 3.0, seg[0].y + (seg[1].y - seg[0].y) / 3.0);
    Some((at, seg[0].x == seg[1].x))
}

/// Cables: a short slash across each wire that runs in one, a dashed line joining the slashes
/// of the same cable on the page, and the cable's tag and type beside the first slash
/// (`-WM10A 4x2,5 SCH`).
fn draw_cables(d: &mut Drawing, project: &Project, page: PageId) -> Result<()> {
    let layer = on(LAYER_WIRES);
    for cable in project.cables.values() {
        let mut marks: Vec<(Point, bool)> = project
            .connections_on(page)
            .filter(|c| c.spec.cable.as_ref().is_some_and(|k| k.cable == cable.id))
            .filter_map(|c| project.route(c).ok().and_then(|r| cable_mark(&r)))
            .collect();
        if marks.is_empty() {
            continue;
        }
        marks.sort_by(|a, b| (a.0.x, -a.0.y).partial_cmp(&(b.0.x, -b.0.y)).unwrap_or(std::cmp::Ordering::Equal));
        for (at, vertical) in &marks {
            let (dx, dy) = if *vertical { (1.2, 1.2) } else { (1.2, -1.2) };
            add_line(d, &layer, Point::new(at.x - dx, at.y - dy), Point::new(at.x + dx, at.y + dy))?;
        }
        for w in marks.windows(2) {
            for (a, b) in dashes(w[0].0, w[1].0) {
                add_line(d, &layer, a, b)?;
            }
        }
        let mut label = cable.tag.clone();
        if !cable.kind.is_empty() {
            label = format!("{label} {}", cable.kind);
        }
        if cable.shielded {
            label.push_str(" SCH");
        }
        let (first, vertical) = marks[0];
        let at = if vertical { Point::new(first.x - 2.5, first.y) } else { Point::new(first.x, first.y - 2.5) };
        let (h, v) = if vertical { (HAlign::Right, VAlign::Middle) } else { (HAlign::Left, VAlign::Top) };
        add_text(d, &on(LAYER_TAGS), justified(at, LABEL_HEIGHT, &label, h, v))?;
    }
    Ok(())
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

/// The sheet: outer edge, frame, numbered column band, lettered rows (if any) and the title
/// block along the bottom.
fn draw_sheet(d: &mut Drawing, project: &Project, page: PageId) -> Result<()> {
    use ecad_model::sheet::{COLUMN_BAND, ROW_BAND, Sheet, TITLE_BLOCK_HEIGHT};
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
        let label = sheet.column_label(n).to_string();
        add_text(d, &words, justified(at, 2.5, &label, HAlign::Center, VAlign::Middle))?;
    }
    // Rows are lettered in the left border, outside the frame, so they never cover the drawing.
    if sheet.rows > 0 {
        let x = lo.x - ROW_BAND;
        add_line(d, &lines, Point::new(x, lo.y), Point::new(x, hi.y))?;
        for n in 1..=sheet.rows {
            let (bottom, top) = sheet.row_span(n);
            if n > 1 {
                add_line(d, &lines, Point::new(x, top), Point::new(lo.x, top))?;
            }
            let at = Point::new(x + ROW_BAND / 2.0, (bottom + top) / 2.0);
            let label = Sheet::row_label(n).to_string();
            add_text(d, &words, justified(at, 2.5, &label, HAlign::Center, VAlign::Middle))?;
        }
    }
    title_block(d, project, page, &lines, &words, TITLE_BLOCK_HEIGHT)
}

/// The title block: revisions on the left, then diagram code, customer and designer, plant and
/// date, the page title in two lines, and the page number with the next page.
fn title_block(
    d: &mut Drawing,
    project: &Project,
    page: PageId,
    lines: &Common,
    words: &Common,
    height: f64,
) -> Result<()> {
    let this = project.page(page)?;
    let info = &project.info;
    let (t_lo, t_hi) = project.sheet.title_block();
    add_rect(d, lines, t_lo, t_hi)?;
    let caption = |x: f64, y: f64, s: &str| justified(Point::new(x, y), 1.6, s, HAlign::Left, VAlign::Top);
    // Text shrinks to fit its cell (stroke-font characters with their spacing are about 0.85 of
    // their height wide, measured on rendered pages).
    let value = |x: f64, y: f64, h: f64, s: &str, width: f64| {
        let fit = width / (s.chars().count().max(1) as f64 * 0.85);
        justified(Point::new(x, y), h.min(fit), s, HAlign::Left, VAlign::Bottom)
    };
    // Column edges, left to right; the title takes what is left.
    let width = t_hi.x - t_lo.x;
    let rev_w = 90.0;
    let page_w = 34.0;
    let fixed = [rev_w, 46.0, 60.0, 56.0];
    let title_w = (width - fixed.iter().sum::<f64>() - page_w).max(40.0);
    let mut x = t_lo.x;
    let mut edges = Vec::new();
    for w in fixed.iter().chain([title_w, page_w].iter()) {
        edges.push(x);
        x += w;
    }
    for &e in &edges[1..] {
        add_line(d, lines, Point::new(e, t_lo.y), Point::new(e, t_hi.y))?;
    }
    let mid = t_lo.y + height / 2.0;

    // Revisions: the last three, newest at the top, under a header row.
    let row_h = height / 4.0;
    let rev_cols = [0.0, 10.0, 56.0, 76.0];
    for i in 1..4 {
        let y = t_lo.y + row_h * f64::from(i);
        add_line(d, lines, Point::new(edges[0], y), Point::new(edges[1], y))?;
    }
    for &c in &rev_cols[1..] {
        add_line(d, lines, Point::new(edges[0] + c, t_lo.y), Point::new(edges[0] + c, t_hi.y))?;
    }
    let header_y = t_hi.y - row_h / 2.0;
    for (c, label) in rev_cols.iter().zip(["Rev.", "Change", "Date", "By"]) {
        let at = Point::new(edges[0] + c + 1.0, header_y);
        add_text(d, words, justified(at, 1.6, label, HAlign::Left, VAlign::Middle))?;
    }
    for (i, r) in info.revisions.iter().rev().take(3).enumerate() {
        let y = t_hi.y - row_h * (i as f64 + 1.5);
        for (c, v) in rev_cols.iter().zip([&r.index, &r.change, &r.date, &r.by]) {
            add_text(d, words, justified(Point::new(edges[0] + c + 1.0, y), 2.0, v, HAlign::Left, VAlign::Middle))?;
        }
    }

    // Two stacked cells per column: (caption, value) above and below the middle line.
    type Cell<'a> = (f64, f64, [(&'a str, &'a str); 2]);
    let cells: [Cell; 3] = [
        (edges[1], edges[2], [("Diagram code", &info.code), ("Project", &project.name)]),
        (edges[2], edges[3], [("Customer", &info.customer), ("Designer", &info.designer)]),
        (edges[3], edges[4], [("Plant", &info.plant), ("Date", &info.date)]),
    ];
    for (left, right, pair) in cells {
        add_line(d, lines, Point::new(left, mid), Point::new(right, mid))?;
        for (i, (label, text)) in pair.iter().enumerate() {
            let top = if i == 0 { t_hi.y } else { mid };
            add_text(d, words, caption(left + 1.5, top - 1.0, label))?;
            add_text(d, words, value(left + 1.5, top - height / 2.0 + 1.5, 2.8, text, right - left - 3.0))?;
        }
    }

    // The page title: description over subtitle.
    add_text(d, words, caption(edges[4] + 1.5, t_hi.y - 1.0, "Title"))?;
    add_text(d, words, value(edges[4] + 3.0, mid + 0.5, 3.5, &this.description, edges[5] - edges[4] - 6.0))?;
    add_text(d, words, value(edges[4] + 3.0, t_lo.y + 3.0, 3.0, &this.subtitle, edges[5] - edges[4] - 6.0))?;

    // Page number and the next page.
    let (left, right) = (edges[5], t_hi.x);
    add_line(d, lines, Point::new(left, mid), Point::new(right, mid))?;
    add_text(d, words, caption(left + 1.5, t_hi.y - 1.0, "Page"))?;
    let page_at = Point::new((left + right) / 2.0 + 3.0, mid + height / 4.0 - 0.5);
    add_text(d, words, justified(page_at, 5.0, &this.name, HAlign::Center, VAlign::Middle))?;
    let order = project.pages_in_order();
    let index = order.iter().position(|p| p.id == page);
    let next = index.and_then(|i| order.get(i + 1)).map_or(String::new(), |p| p.name.clone());
    add_text(d, words, caption(left + 1.5, mid - 1.0, "Next"))?;
    add_text(d, words, value(left + 12.0, t_lo.y + 5.5, 2.8, &next, right - left - 14.0))?;
    let count = format!("{} of {}", index.map_or(0, |i| i + 1), project.pages.len());
    add_text(d, words, justified(Point::new(right - 1.5, t_lo.y + 1.2), 1.8, &count, HAlign::Right, VAlign::Bottom))?;
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
        // The coil's contact table: each contact's numbers and where it is.
        let mut expected = vec![
            "13".to_owned(),
            "14".into(),
            "21".into(),
            "22".into(),
            format!("2.{}", col(K1_NO)),
            format!("2.{}", col(K1_NC)),
        ];
        expected.sort();
        assert_eq!(texts_on(&p, PAGE_1, LAYER_XREF), expected);
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
