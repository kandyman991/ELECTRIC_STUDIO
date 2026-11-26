//! Placing device tags, cross-references and wire numbers where they don't sit on a symbol, a
//! wire or another text: each label tries a list of positions, preferred first, and takes the
//! first that is clear (or the least crowded one if none is).

use crate::clash::{Rect, text_on_symbol, text_on_wire, text_rect, texts_clash};
use crate::{LABEL_GAP, LABEL_HEIGHT, TAG_GAP, TAG_HEIGHT, XREF_HEIGHT, justified};

use cadcraft_doc::{HAlign, Text, VAlign};
use ecad_model::{FunctionId, Point};
use std::f64::consts::FRAC_PI_2;

/// Everything already on the page that a label must keep off.
pub(crate) struct Placer {
    taken: Vec<Rect>,
    wires: Vec<(Point, Point)>,
    /// Location box edges.
    edges: Vec<(Point, Point)>,
    symbols: Vec<(FunctionId, Rect)>,
    /// Where labels may go: inside the frame, above the title block, below the column band.
    area: Rect,
}

impl Placer {
    pub(crate) fn new(area: (Point, Point)) -> Self {
        Placer {
            taken: Vec::new(),
            wires: Vec::new(),
            edges: Vec::new(),
            symbols: Vec::new(),
            area: Rect { lo: area.0, hi: area.1 },
        }
    }

    pub(crate) fn add_text(&mut self, t: &Text) {
        if let Some(r) = text_rect(t) {
            self.taken.push(r);
        }
    }

    pub(crate) fn add_wire(&mut self, a: Point, b: Point) {
        self.wires.push((a, b));
    }

    pub(crate) fn add_edge(&mut self, a: Point, b: Point) {
        self.edges.push((a, b));
    }

    pub(crate) fn add_symbol(&mut self, f: FunctionId, lo: Point, hi: Point) {
        self.symbols.push((f, Rect { lo, hi }));
    }

    /// How badly the texts would sit: on another text, a symbol or off the drawing area counts 4
    /// each, across a wire or a location box edge 2.
    fn cost(&self, texts: &[Text], own: Option<FunctionId>) -> usize {
        let mut n = 0;
        for t in texts {
            let Some(r) = text_rect(t) else { continue };
            if r.lo.x < self.area.lo.x || r.lo.y < self.area.lo.y || r.hi.x > self.area.hi.x || r.hi.y > self.area.hi.y
            {
                n += 4;
            }
            n += 4 * self.taken.iter().filter(|o| texts_clash(&r, o)).count();
            n += 4 * self.symbols.iter().filter(|(f, s)| Some(*f) != own && text_on_symbol(&r, s)).count();
            n += 2 * self.wires.iter().filter(|(a, b)| text_on_wire(&r, *a, *b)).count();
            n += 2 * self.edges.iter().filter(|(a, b)| text_on_wire(&r, *a, *b)).count();
        }
        n
    }

    /// The lowest a label may go: the top of the title block.
    pub(crate) fn floor(&self) -> f64 {
        self.area.lo.y
    }

    /// The first clear candidate, else the least crowded (the earliest of equals); it is then
    /// taken.
    pub(crate) fn choose(&mut self, candidates: Vec<Vec<Text>>, own: Option<FunctionId>) -> Vec<Text> {
        match self.choose_index(&candidates, own) {
            Some(i) => candidates.into_iter().nth(i).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// The first clear candidate, taken; nothing if none is clear.
    pub(crate) fn choose_clear(&mut self, candidates: Vec<Vec<Text>>, own: Option<FunctionId>) -> Vec<Text> {
        let Some(c) = candidates.into_iter().find(|c| self.cost(c, own) == 0) else { return Vec::new() };
        for t in &c {
            self.add_text(t);
        }
        c
    }

    /// Like [`Placer::choose`], returning which candidate was taken.
    pub(crate) fn choose_index(&mut self, candidates: &[Vec<Text>], own: Option<FunctionId>) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (i, c) in candidates.iter().enumerate() {
            let cost = self.cost(c, own);
            if best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, i));
                if cost == 0 {
                    break;
                }
            }
        }
        let (_, i) = best?;
        for t in &candidates[i] {
            self.add_text(t);
        }
        Some(i)
    }
}

/// Where a symbol's tag and cross-reference lines go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    Right,
    Left,
    Above,
    Below,
    /// Reading upwards right of the symbol, the lines in columns further right: for symbols with
    /// close neighbours left and right.
    RightUpright,
    /// Reading upwards above the symbol, just right of its middle (clear of a wire leaving
    /// upwards), the lines in columns to the right: for symbols in a tight row, like arrows or
    /// terminals side by side.
    AboveUpright,
    /// Reading upwards below the symbol, just right of its middle.
    BelowUpright,
}

impl Side {
    pub(crate) const ALL: [Side; 7] =
        [Side::Right, Side::Left, Side::Above, Side::Below, Side::RightUpright, Side::AboveUpright, Side::BelowUpright];
}

fn rotated(mut t: Text) -> Text {
    t.rotation = FRAC_PI_2;
    t
}

/// How big a tag is and how far from its symbol.
#[derive(Clone, Copy)]
pub(crate) struct TagSize {
    pub height: f64,
    pub gap: f64,
}

impl TagSize {
    pub(crate) const NORMAL: TagSize = TagSize { height: TAG_HEIGHT, gap: TAG_GAP };
    /// A terminal or channel continuing a row (`:2`): small and close, like a point number.
    pub(crate) const COMPACT: TagSize = TagSize { height: LABEL_HEIGHT, gap: LABEL_GAP };
}

/// The tag and the cross-reference lines under it, on one side of the box `min`-`max`.
pub(crate) fn tag_block(min: Point, max: Point, side: Side, size: TagSize, tag: &str, lines: &[String]) -> Vec<Text> {
    let TagSize { height: tag_height, gap } = size;
    let step = XREF_HEIGHT * 1.5;
    let mid_y = (min.y + max.y) / 2.0;
    let cx = (min.x + max.x) / 2.0;
    let mut out = Vec::with_capacity(lines.len() + 1);
    match side {
        Side::Right | Side::Left => {
            let (x, h) = if side == Side::Right { (max.x + gap, HAlign::Left) } else { (min.x - gap, HAlign::Right) };
            out.push(justified(Point::new(x, mid_y), tag_height, tag, h, VAlign::Middle));
            let under_tag = mid_y - tag_height;
            for (i, s) in lines.iter().enumerate() {
                out.push(justified(Point::new(x, under_tag - step * i as f64), XREF_HEIGHT, s, h, VAlign::Top));
            }
        }
        Side::Above => {
            // The tag above; the lines below the symbol, clear of the wires leaving sideways.
            out.push(justified(Point::new(cx, max.y + gap), tag_height, tag, HAlign::Center, VAlign::Bottom));
            for (i, s) in lines.iter().enumerate() {
                let y = min.y - gap - step * i as f64;
                out.push(justified(Point::new(min.x, y), XREF_HEIGHT, s, HAlign::Left, VAlign::Top));
            }
        }
        Side::Below => {
            let top = min.y - gap;
            out.push(justified(Point::new(cx, top), tag_height, tag, HAlign::Center, VAlign::Top));
            for (i, s) in lines.iter().enumerate() {
                let y = top - tag_height - 0.6 - step * i as f64;
                out.push(justified(Point::new(cx, y), XREF_HEIGHT, s, HAlign::Center, VAlign::Top));
            }
        }
        Side::RightUpright | Side::AboveUpright | Side::BelowUpright => {
            // Upright text: the baseline is on the right of the glyphs, so a column whose
            // baseline is at x covers x - height to x.
            let (left, y, h) = match side {
                Side::RightUpright => (max.x + gap, mid_y, HAlign::Center),
                Side::AboveUpright => (cx + LABEL_GAP, max.y + gap, HAlign::Left),
                _ => (cx + LABEL_GAP, min.y - gap, HAlign::Right),
            };
            let mut x = left + tag_height;
            out.push(rotated(justified(Point::new(x, y), tag_height, tag, h, VAlign::Baseline)));
            for s in lines {
                x += XREF_HEIGHT + 0.6;
                out.push(rotated(justified(Point::new(x, y), XREF_HEIGHT, s, h, VAlign::Baseline)));
            }
        }
    }
    out
}

/// A wire number (and the cross-section and colour under it) at a fraction `t` along a segment,
/// above or right of it, or (`other_side`) below or left.
pub(crate) fn wire_labels(a: Point, b: Point, t: f64, other_side: bool, number: Option<&str>, spec: &str) -> Vec<Text> {
    let at = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
    let small = LABEL_HEIGHT * 0.8;
    let mut out = Vec::new();
    if a.x == b.x {
        // Beside a vertical run: number, then spec under it.
        let (x, h) = if other_side { (at.x - LABEL_GAP, HAlign::Right) } else { (at.x + LABEL_GAP, HAlign::Left) };
        let mut y = at.y;
        if let Some(n) = number {
            out.push(justified(Point::new(x, y), LABEL_HEIGHT, n, h, VAlign::Middle));
            y -= LABEL_HEIGHT * 1.5;
        }
        if !spec.is_empty() {
            out.push(justified(Point::new(x, y), small, spec, h, VAlign::Middle));
        }
    } else if other_side {
        // Under a horizontal run: number, then spec under it.
        let mut y = at.y - LABEL_GAP;
        if let Some(n) = number {
            out.push(justified(Point::new(at.x, y), LABEL_HEIGHT, n, HAlign::Center, VAlign::Top));
            y -= LABEL_HEIGHT * 1.3;
        }
        if !spec.is_empty() {
            out.push(justified(Point::new(at.x, y), small, spec, HAlign::Center, VAlign::Top));
        }
    } else {
        // The number above the run, the spec just under it.
        if let Some(n) = number {
            out.push(justified(Point::new(at.x, at.y + LABEL_GAP), LABEL_HEIGHT, n, HAlign::Center, VAlign::Bottom));
        }
        if !spec.is_empty() {
            let below = if number.is_some() { LABEL_HEIGHT * 1.5 } else { 0.0 };
            let y = at.y + LABEL_GAP - below;
            out.push(justified(Point::new(at.x, y), small, spec, HAlign::Center, VAlign::Bottom));
        }
    }
    out
}

/// A wire number written upwards along a vertical run, beside it (left with `other_side`), the
/// cross-section and colour in the next column out: for wires too close together for
/// horizontal numbers.
pub(crate) fn upright_wire_labels(
    a: Point,
    b: Point,
    t: f64,
    other_side: bool,
    number: Option<&str>,
    spec: &str,
) -> Vec<Text> {
    let at = Point::new(a.x, a.y + (b.y - a.y) * t);
    let small = LABEL_HEIGHT * 0.8;
    // Upright text covers baseline - height to baseline across.
    let sign = if other_side { -1.0 } else { 1.0 };
    let mut edge = at.x + sign * LABEL_GAP;
    let mut out = Vec::new();
    for (value, h) in [(number.unwrap_or(""), LABEL_HEIGHT), (spec, small)] {
        if value.is_empty() {
            continue;
        }
        let baseline = if other_side { edge } else { edge + h };
        out.push(rotated(justified(Point::new(baseline, at.y), h, value, HAlign::Center, VAlign::Baseline)));
        edge += sign * (h + 0.5);
    }
    out
}

/// Candidate places for a wire's labels: along its segments, longest first, at the middle, then
/// a quarter and three quarters along, on the usual side and then the other, and along a
/// vertical run written upwards.
pub(crate) fn wire_candidates(route: &[Point], number: Option<&str>, spec: &str) -> Vec<Vec<Text>> {
    let len = |s: &[Point]| (s[1].x - s[0].x).abs() + (s[1].y - s[0].y).abs();
    // Longest first; of equal lengths the last along the route first.
    let mut segs: Vec<&[Point]> = route.windows(2).rev().filter(|s| len(s) > 0.0).collect();
    segs.sort_by(|a, b| len(b).total_cmp(&len(a)));
    let mut out = Vec::new();
    for s in segs {
        for t in [0.5, 0.25, 0.75] {
            for other in [false, true] {
                out.push(wire_labels(s[0], s[1], t, other, number, spec));
            }
            if s[0].x == s[1].x {
                for other in [false, true] {
                    out.push(upright_wire_labels(s[0], s[1], t, other, number, spec));
                }
            }
        }
    }
    out
}
