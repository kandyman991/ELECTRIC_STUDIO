#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use ecad_view::clash::{Clash, page_clashes};

fn describe(p: &ecad_model::Project, c: &Clash) -> String {
    let tag = |f| p.function(f).ok().and_then(|f| p.device(f.device).ok()).map(|d| d.tag.clone()).unwrap_or_default();
    let wire = |id| {
        p.connections
            .get(&id)
            .map(|c| {
                format!(
                    "{}({}-{})",
                    c.wire_number.clone().unwrap_or_default(),
                    tag(c.from.function),
                    tag(c.to.function)
                )
            })
            .unwrap_or_default()
    };
    let at = c.at();
    let what = match c {
        Clash::Texts { a, b, .. } => format!("text {a:?} on text {b:?}"),
        Clash::TextOnWire { text, wire: w, .. } => format!("text {text:?} on wire {}", wire(*w)),
        Clash::TextOnBoxEdge { text, .. } => format!("text {text:?} on a location box edge"),
        Clash::Wires { a, b, .. } => format!("wire {} on wire {}", wire(*a), wire(*b)),
        Clash::WireThroughSymbol { wire: w, function, .. } => format!("wire {} through {}", wire(*w), tag(*function)),
        Clash::TagOnSymbol { tag: t, function, .. } => format!("tag {t:?} on {}", tag(*function)),
    };
    format!("({:.0},{:.0}) {what}", at.x, at.y)
}

#[test]
fn the_reference_project_has_no_layout_clashes() {
    let p = ecad_model::reference::reference_project().unwrap();
    let mut all = Vec::new();
    for page in p.pages_in_order() {
        for c in page_clashes(&p, page.id).unwrap() {
            all.push(format!("{}: {}", page.name, describe(&p, &c)));
        }
    }
    assert!(all.is_empty(), "{} clashes:\n{}", all.len(), all.join("\n"));
}
