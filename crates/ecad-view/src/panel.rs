//! Layout pages: the mounting plate at a standard scale, with its ducts, rails and device
//! footprints and the tags written on them.

use crate::{LAYER_PANEL, LAYER_TAGS, Result, add_line, add_rect, add_text, dash_pattern, justified, on};
use cadcraft_doc::{Drawing, HAlign, VAlign};
use ecad_model::sheet::COLUMN_BAND;
use ecad_model::{PageId, Panel, PanelItemKind, Point, Project};
use std::f64::consts::FRAC_PI_2;

/// Scales tried, largest drawing first (1:1, 1:2 …).
const SCALES: [f64; 9] = [1.0, 2.0, 2.5, 5.0, 10.0, 15.0, 20.0, 25.0, 50.0];
/// Space kept around the plate, and under it for the note, in mm on paper.
const MARGIN: f64 = 8.0;
const NOTE: f64 = 8.0;
/// Character width of the stroke font as a fraction of the text height, with its spacing.
const CHAR_WIDTH: f64 = 0.9;
/// Footprints narrower than this on paper (mm) get their tag above, with their neighbours'.
const NARROW: f64 = 1.6;

/// The scale a plate is drawn at on a page: the largest standard one that fits.
pub fn scale_for(project: &Project, panel: Panel) -> f64 {
    let (w, h) = area(project);
    SCALES.into_iter().find(|s| panel.width / s <= w && panel.height / s <= h - NOTE).unwrap_or(100.0)
}

/// Width and height of the drawing area inside the frame, above the title block.
fn area(project: &Project) -> (f64, f64) {
    let (lo, hi) = project.sheet.frame();
    let (_, tb) = project.sheet.title_block();
    (hi.x - lo.x - 2.0 * MARGIN, hi.y - COLUMN_BAND - tb.y - 2.0 * MARGIN)
}

pub(crate) fn draw(d: &mut Drawing, project: &Project, page: PageId, panel: Panel) -> Result<()> {
    let s = scale_for(project, panel);
    let (flo, fhi) = project.sheet.frame();
    let (_, tb) = project.sheet.title_block();
    let (w, h) = (panel.width / s, panel.height / s);
    // Centred in the area, the note under the plate.
    let cx = (flo.x + fhi.x) / 2.0;
    let top = fhi.y - COLUMN_BAND - MARGIN;
    let bottom = tb.y + MARGIN + NOTE;
    let origin = Point::new(cx - w / 2.0, bottom + ((top - bottom) - h) / 2.0);
    let map = |p: Point| Point::new(origin.x + p.x / s, origin.y + p.y / s);
    let layer = on(LAYER_PANEL);
    let tags = on(LAYER_TAGS);
    add_rect(d, &layer, origin, Point::new(origin.x + w, origin.y + h))?;

    // Device footprints on paper: rails are hidden behind them.
    let devices: Vec<(ecad_model::DeviceId, Point, Point)> = project
        .panel_items_on(page)
        .into_iter()
        .filter_map(|i| match i.kind {
            PanelItemKind::Device { device } => {
                let (lo, hi) = project.panel_item_rect(i);
                Some((device, map(lo), map(hi)))
            }
            _ => None,
        })
        .collect();
    let narrow = |lo: Point, hi: Point| hi.x - lo.x < NARROW;

    for item in project.panel_items_on(page) {
        let (lo, hi) = project.panel_item_rect(item);
        let (lo, hi) = (map(lo), map(hi));
        let (pw, ph) = (hi.x - lo.x, hi.y - lo.y);
        let mid = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        match &item.kind {
            PanelItemKind::Duct { width, depth } => {
                add_rect(d, &layer, lo, hi)?;
                let label = format!("{} x {}", fmt(*width), fmt(*depth));
                label_in(d, &tags, mid, pw, ph, &label, 2.0)?;
            }
            PanelItemKind::Rail => {
                // Its edges and a dash-dot centre line, where no device covers it.
                let rects: Vec<(Point, Point)> = devices.iter().map(|(_, a, b)| (*a, *b)).collect();
                let along = |p: Point| if item.vertical { p.y } else { p.x };
                let across = |p: Point| if item.vertical { p.x } else { p.y };
                let point = |a: f64, c: f64| if item.vertical { Point::new(c, a) } else { Point::new(a, c) };
                let covered: Vec<(f64, f64)> = rects
                    .iter()
                    .filter(|(a, b)| across(*a) < across(hi) && across(lo) < across(*b))
                    .map(|(a, b)| (along(*a), along(*b)))
                    .collect();
                let free = uncovered((along(lo), along(hi)), &covered);
                for (a, b) in &free {
                    for c in [across(lo), across(hi)] {
                        add_line(d, &layer, point(*a, c), point(*b, c))?;
                    }
                    for (p, q) in dash_pattern(point(*a, across(mid)), point(*b, across(mid)), 4.0, 6.0) {
                        add_line(d, &layer, p, q)?;
                    }
                }
                for end in [along(lo), along(hi)] {
                    if free.iter().any(|(a, b)| *a <= end + 0.01 && end - 0.01 <= *b) {
                        add_line(d, &layer, point(end, across(lo)), point(end, across(hi)))?;
                    }
                }
            }
            PanelItemKind::Device { device } => {
                let (_, known) = project.device_size(*device);
                if known {
                    add_rect(d, &layer, lo, hi)?;
                } else {
                    // Size unknown: a dashed outline one module wide.
                    let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y), lo];
                    for side in corners.windows(2) {
                        for (a, b) in dash_pattern(side[0], side[1], 1.0, 1.8) {
                            add_line(d, &layer, a, b)?;
                        }
                    }
                }
                // A device too narrow to write in is labelled with its neighbours, above them.
                if !narrow(lo, hi) {
                    let tag = project.device(*device).map(|d| d.tag.clone()).unwrap_or_default();
                    label_in(d, &tags, mid, pw, ph, &tag, 2.0)?;
                }
            }
        }
    }

    // Runs of narrow devices side by side share one label above them: `A3KA1 A3KA2`.
    let mut run: Vec<&(ecad_model::DeviceId, Point, Point)> = Vec::new();
    let mut sorted: Vec<&(ecad_model::DeviceId, Point, Point)> =
        devices.iter().filter(|(_, a, b)| narrow(*a, *b)).collect();
    sorted.sort_by(|a, b| (-a.1.y, a.1.x).partial_cmp(&(-b.1.y, b.1.x)).unwrap_or(std::cmp::Ordering::Equal));
    let flush = |d: &mut Drawing, run: &mut Vec<&(ecad_model::DeviceId, Point, Point)>| -> Result<()> {
        if let Some(first) = run.first() {
            let top = run.iter().map(|r| r.2.y).fold(f64::MIN, f64::max);
            let names: Vec<String> =
                run.iter().filter_map(|r| project.device(r.0).ok().map(|d| d.tag.clone())).collect();
            add_text(
                d,
                &tags,
                justified(Point::new(first.1.x, top + 0.5), 1.4, &shared_label(&names), HAlign::Left, VAlign::Bottom),
            )?;
        }
        run.clear();
        Ok(())
    };
    for dev in sorted {
        let joins = run.last().is_some_and(|last| (dev.1.x - last.2.x).abs() < 1.0 && (dev.1.y - last.1.y).abs() < 1.0);
        if !joins {
            flush(d, &mut run)?;
        }
        run.push(dev);
    }
    flush(d, &mut run)?;

    let note = format!("Plate {} x {} mm, scale 1:{}", fmt(panel.width), fmt(panel.height), fmt(s));
    add_text(d, &tags, justified(Point::new(origin.x, origin.y - 3.0), 2.5, &note, HAlign::Left, VAlign::Top))
}

/// Tags written together, their common start once: `O3KA0/1/5/6`, `A3KA1/2`; tags with no
/// common start side by side.
fn shared_label(tags: &[String]) -> String {
    let Some(first) = tags.first() else { return String::new() };
    let mut common = first.len();
    for t in &tags[1..] {
        common = common.min(first.bytes().zip(t.bytes()).take_while(|(a, b)| a == b).count());
    }
    // Cut before the numbers, so each tag keeps its whole number: O3KA|0, O3KA|1.
    while common > 0 && first.as_bytes()[common - 1].is_ascii_digit() {
        common -= 1;
    }
    if tags.len() < 2 || common == 0 || !first.is_char_boundary(common) {
        return tags.join(" ");
    }
    let rest: Vec<&str> = tags.iter().map(|t| t.get(common..).unwrap_or("")).collect();
    format!("{}{}", &first[..common], rest.join("/"))
}

/// The parts of `span` that none of `covered` covers.
fn uncovered(span: (f64, f64), covered: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut cuts: Vec<(f64, f64)> = covered.iter().map(|(a, b)| (a.min(*b), a.max(*b))).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = span.0;
    for (a, b) in cuts {
        if a > at {
            out.push((at, a.min(span.1)));
        }
        at = at.max(b);
        if at >= span.1 {
            break;
        }
    }
    if at < span.1 {
        out.push((at, span.1));
    }
    out.retain(|(a, b)| b - a > 0.05);
    out
}

/// A label centred in a box `w` × `h` (paper mm): across if it fits, else reading upwards,
/// shrunk down to 1.2 mm high if it must.
fn label_in(
    d: &mut Drawing,
    layer: &cadcraft_doc::Common,
    at: Point,
    w: f64,
    h: f64,
    text: &str,
    height: f64,
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let n = text.chars().count() as f64;
    let fits = |room: f64, other: f64| (room / (n * CHAR_WIDTH)).min(other * 0.8).min(height);
    let (across, upright) = (fits(w - 0.6, h), fits(h - 0.6, w));
    let (size, rotate) = if across >= upright.min(height) { (across, false) } else { (upright, true) };
    let size = size.max(1.2);
    let mut t = justified(at, size, text, HAlign::Center, VAlign::Middle);
    if rotate {
        t.rotation = FRAC_PI_2;
    }
    add_text(d, layer, t)
}

/// A length without needless decimals, with a decimal comma: `60`, `2,5`.
fn fmt(v: f64) -> String {
    let s = format!("{}", (v * 100.0).round() / 100.0);
    s.replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_devices_share_a_label_and_rails_hide_behind_devices() {
        let tags = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(shared_label(&tags(&["O3KA0", "O3KA1", "O3KA5", "O3KA6"])), "O3KA0/1/5/6");
        assert_eq!(shared_label(&tags(&["A3KA1", "A3KA12"])), "A3KA1/12", "the number stays whole");
        assert_eq!(shared_label(&tags(&["-K1", "X2"])), "-K1 X2");
        assert_eq!(uncovered((0.0, 100.0), &[(10.0, 20.0), (15.0, 30.0), (90.0, 120.0)]), [(0.0, 10.0), (30.0, 90.0)]);
    }
}
