//! Layout clashes on a drawn page: texts on top of each other or on a wire, tags on another
//! symbol, and the model's wire clashes (wires of different potentials on top of each other,
//! wires through a symbol they don't connect to).
//!
//! Text sizes are estimated (stroke-font characters with their spacing are about 0.9 of their
//! height wide, measured on rendered pages). A label must keep [`TEXT_GAP`] clear of other
//! texts, wires and symbols that aren't its own: touching reads as overlapping on paper. The
//! label placer uses the same rules ([`texts_clash`], [`text_on_wire`], [`text_on_symbol`]).

use crate::{Result, page_drawing};
use cadcraft_doc::{EntityKind, HAlign, Text, VAlign};
use ecad_model::layout::{WireClash, segment_crosses_box};
use ecad_model::{ConnectionId, FunctionId, PageId, Point, Project};

/// Stands for "no symbol" as the owner of a text that may sit on no symbol at all.
const NO_OWNER: FunctionId = FunctionId::from_u128(0);

/// The clear space a text keeps from other texts, wires and symbols, in mm.
pub const TEXT_GAP: f64 = 0.5;

/// Whether two texts come closer than [`TEXT_GAP`].
pub(crate) fn texts_clash(a: &Rect, b: &Rect) -> bool {
    a.grown(TEXT_GAP / 2.0).overlaps(&b.grown(TEXT_GAP / 2.0))
}

/// Whether a wire segment comes closer than [`TEXT_GAP`] to a text.
pub(crate) fn text_on_wire(r: &Rect, a: Point, b: Point) -> bool {
    let g = r.grown(TEXT_GAP);
    segment_crosses_box(a, b, g.lo, g.hi)
}

/// Whether a text comes closer than [`TEXT_GAP`] to a symbol's box.
pub(crate) fn text_on_symbol(r: &Rect, symbol: &Rect) -> bool {
    r.grown(TEXT_GAP).overlaps(symbol)
}
/// Character width as a fraction of the text height.
const CHAR_WIDTH: f64 = 0.9;
/// How far above its bottom alignment point bottom-aligned text starts, as a fraction of its
/// height (measured on rendered pages).
const DESCENT: f64 = 0.25;

#[derive(Clone, Debug, PartialEq)]
pub enum Clash {
    /// Two texts overlap.
    Texts { a: String, b: String, at: Point },
    /// A text lies on a wire.
    TextOnWire { text: String, wire: ConnectionId, at: Point },
    /// A text lies on the edge of a location box.
    TextOnBoxEdge { text: String, at: Point },
    /// Two different wires run along the same line for a stretch.
    Wires { a: ConnectionId, b: ConnectionId, at: Point },
    /// A wire runs through a symbol it isn't connected to.
    WireThroughSymbol { wire: ConnectionId, function: FunctionId, at: Point },
    /// A text lies on a symbol it doesn't belong to (a tag or designation on another symbol, a
    /// wire number or cross-reference on any).
    TagOnSymbol { tag: String, function: FunctionId, at: Point },
}

impl Clash {
    pub fn at(&self) -> Point {
        match self {
            Clash::Texts { at, .. }
            | Clash::TextOnWire { at, .. }
            | Clash::TextOnBoxEdge { at, .. }
            | Clash::Wires { at, .. }
            | Clash::WireThroughSymbol { at, .. }
            | Clash::TagOnSymbol { at, .. } => *at,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub(crate) lo: Point,
    pub(crate) hi: Point,
}

impl Rect {
    pub(crate) fn grown(self, by: f64) -> Rect {
        Rect { lo: Point::new(self.lo.x - by, self.lo.y - by), hi: Point::new(self.hi.x + by, self.hi.y + by) }
    }
    fn valid(&self) -> bool {
        self.lo.x < self.hi.x && self.lo.y < self.hi.y
    }
    pub(crate) fn overlaps(&self, o: &Rect) -> bool {
        self.valid()
            && o.valid()
            && self.lo.x < o.hi.x
            && o.lo.x < self.hi.x
            && self.lo.y < o.hi.y
            && o.lo.y < self.hi.y
    }
    fn centre(&self) -> Point {
        Point::new((self.lo.x + self.hi.x) / 2.0, (self.lo.y + self.hi.y) / 2.0)
    }
}

pub(crate) fn text_rect(t: &Text) -> Option<Rect> {
    let value = t.value.trim();
    if value.is_empty() {
        return None;
    }
    let w = value.chars().count() as f64 * t.height * CHAR_WIDTH * t.width_factor;
    let h = t.height;
    let (anchor, halign, valign) = match t.align_pt {
        Some(p) => (p, t.halign, t.valign),
        None => (t.insert, HAlign::Left, VAlign::Baseline),
    };
    let x0 = match halign {
        HAlign::Center | HAlign::Middle => anchor.x - w / 2.0,
        HAlign::Right => anchor.x - w,
        _ => anchor.x,
    };
    let y0 = match valign {
        VAlign::Top => anchor.y - h,
        VAlign::Middle => anchor.y - h / 2.0,
        // Bottom-aligned text leaves room for descenders: the glyphs sit higher.
        VAlign::Bottom => anchor.y + DESCENT * h,
        _ => anchor.y,
    };
    let (lo, hi) = (Point::new(x0, y0), Point::new(x0 + w, y0 + h));
    // Turn the box about the anchor for vertical text.
    let quarter = (t.rotation / std::f64::consts::FRAC_PI_2).round() as i64;
    let turn = |p: Point| {
        let (dx, dy) = (p.x - anchor.x, p.y - anchor.y);
        match quarter.rem_euclid(4) {
            1 => Point::new(anchor.x - dy, anchor.y + dx),
            2 => Point::new(anchor.x - dx, anchor.y - dy),
            3 => Point::new(anchor.x + dy, anchor.y - dx),
            _ => p,
        }
    };
    let (a, b) = (turn(lo), turn(hi));
    Some(Rect { lo: Point::new(a.x.min(b.x), a.y.min(b.y)), hi: Point::new(a.x.max(b.x), a.y.max(b.y)) })
}

/// Every clash on a page.
pub fn page_clashes(project: &Project, page: PageId) -> Result<Vec<Clash>> {
    let d = page_drawing(project, page)?;
    let pt = |v: cadcraft_doc::geom::Vec3| Point::new(v.x, v.y);

    // Texts and the symbol each belongs to: a device tag its symbol (the insert at the same
    // place), a connection point's designation the symbol with that point, anything else none.
    let functions: Vec<&ecad_model::Function> = project.functions_on(page).collect();
    let point_owner = |t: &Text| -> FunctionId {
        let anchor = t.align_pt.unwrap_or(t.insert);
        let near = |p: Point| {
            (p.x - anchor.x).abs() <= crate::LABEL_GAP + 0.01 && (p.y - anchor.y).abs() <= crate::LABEL_GAP + 0.01
        };
        functions
            .iter()
            .find(|f| {
                project.symbol(&f.symbol).is_ok_and(|sym| {
                    sym.connection_points.iter().any(|cp| {
                        f.designation(&cp.name) == t.value
                            && project
                                .connection_point(&ecad_model::ConnectionRef::new(f.id, &cp.name))
                                .is_ok_and(|(p, _)| near(p))
                    })
                })
            })
            .map_or(NO_OWNER, |f| f.id)
    };
    let mut texts: Vec<(String, Rect, Option<FunctionId>)> = Vec::new();
    for e in d.model.iter() {
        match &e.kind {
            EntityKind::Text(t) if e.common.layer != crate::LAYER_FRAME => {
                if let Some(r) = text_rect(t) {
                    let owner = if e.common.layer == crate::LAYER_TAGS { point_owner(t) } else { NO_OWNER };
                    texts.push((t.value.clone(), r, Some(owner)));
                }
            }
            EntityKind::Insert(i) => {
                let at = pt(i.insert);
                let owner = functions.iter().find(|f| f.position == at && f.symbol.0 == i.block).map(|f| f.id);
                for a in i.attribs.iter().filter(|a| !a.invisible) {
                    if let Some(r) = text_rect(&a.text) {
                        texts.push((a.text.value.clone(), r, owner));
                    }
                }
            }
            _ => {}
        }
    }
    let wires: Vec<(ConnectionId, Vec<Point>)> =
        project.connections_on(page).map(|c| Ok((c.id, project.route(c)?))).collect::<Result<_>>()?;
    let mut symbols = Vec::new();
    for f in &functions {
        if let Some((lo, hi)) = project.function_bounds(f.id)? {
            symbols.push((f.id, Rect { lo, hi }));
        }
    }

    let mut out = Vec::new();
    for (i, (a, ra, _)) in texts.iter().enumerate() {
        for (b, rb, _) in &texts[i + 1..] {
            if texts_clash(ra, rb) {
                out.push(Clash::Texts { a: a.clone(), b: b.clone(), at: ra.centre() });
            }
        }
        for (id, route) in &wires {
            if route.windows(2).any(|s| text_on_wire(ra, s[0], s[1])) {
                out.push(Clash::TextOnWire { text: a.clone(), wire: *id, at: ra.centre() });
            }
        }
    }
    let mut edges = Vec::new();
    for b in project.boxes_on(page) {
        let (lo, hi) = (b.min, b.max);
        let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y), lo];
        edges.extend(corners.windows(2).map(|c| (c[0], c[1])));
    }
    for (text, r, _) in &texts {
        if edges.iter().any(|(a, b)| text_on_wire(r, *a, *b)) {
            out.push(Clash::TextOnBoxEdge { text: text.clone(), at: r.centre() });
        }
    }
    for (tag, r, owner) in &texts {
        let Some(owner) = owner else { continue };
        for (f, s) in &symbols {
            if f != owner && text_on_symbol(r, s) {
                out.push(Clash::TagOnSymbol { tag: tag.clone(), function: *f, at: r.centre() });
            }
        }
    }
    for c in project.wire_clashes(page) {
        out.push(match c {
            WireClash::Overlap { a, b, at } => Clash::Wires { a, b, at },
            WireClash::ThroughSymbol { wire, function, at } => Clash::WireThroughSymbol { wire, function, at },
        });
    }
    Ok(out)
}
