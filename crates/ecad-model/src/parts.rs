//! The parts catalogue and parts assigned to devices.

use serde::{Deserialize, Serialize};

/// A catalogue entry: something that can be ordered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// The product's identity (D7): never its number or maker, which are properties.
    pub id: crate::ProductId,
    /// Manufacturer's order number, e.g. `3RT2015-1BB41`. Unique per manufacturer.
    pub number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The company's own article code for the part, if it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    /// The auxiliary contacts the part brings, as printed on it: `13-14` (NO), `21-22` (NC),
    /// `11-12-14` (changeover). Contacts drawn for a device take their numbers from these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<String>,
    /// Size in mm, for the panel layout: width along the rail, height across it, depth from
    /// the plate. Parts without one are drawn one module wide on a plate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
}

impl Part {
    /// A product with only a number, with the id [`crate::ProductId::for_number`] gives it.
    pub fn new(number: &str) -> Self {
        Part {
            id: crate::ProductId::for_number(number),
            number: number.trim().to_owned(),
            manufacturer: String::new(),
            description: String::new(),
            code: String::new(),
            contacts: Vec::new(),
            width: None,
            height: None,
            depth: None,
        }
    }

    /// What makes a product unique besides its id: manufacturer (case ignored) and number.
    pub fn key(&self) -> (String, String) {
        (self.manufacturer.trim().to_lowercase(), self.number.trim().to_owned())
    }

    /// How a product is named in messages: `Example EX-CT-9`, or just the number.
    pub fn label(&self) -> String {
        if self.manufacturer.is_empty() {
            self.number.clone()
        } else {
            format!("{} {}", self.manufacturer, self.number)
        }
    }
}

/// A part's width, height and depth in mm, each optional.
pub type Size = (Option<f64>, Option<f64>, Option<f64>);

/// A part's size as written in the catalogue window: `45 x 90` or `45 x 90 x 75` (width,
/// height, depth in mm; `×` or `*` work too, and a decimal comma). Empty clears it.
pub fn parse_size(text: &str) -> Result<Size, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok((None, None, None));
    }
    let values: Vec<f64> = text
        .split(['x', 'X', '×', '*'])
        .map(|s| s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 0.0 && v.is_finite()))
        .collect::<Option<_>>()
        .ok_or_else(|| text.to_owned())?;
    match values.as_slice() {
        [w, h] => Ok((Some(*w), Some(*h), None)),
        [w, h, d] => Ok((Some(*w), Some(*h), Some(*d))),
        _ => Err(text.to_owned()),
    }
}

/// A part's size written back as `45 x 90 x 75`, or empty.
pub fn format_size(part: &Part) -> String {
    let n = |v: f64| {
        let s = format!("{v}");
        s.replace('.', ",")
    };
    match (part.width, part.height, part.depth) {
        (Some(w), Some(h), Some(d)) => format!("{} x {} x {}", n(w), n(h), n(d)),
        (Some(w), Some(h), None) => format!("{} x {}", n(w), n(h)),
        _ => String::new(),
    }
}

/// A contact block's points, e.g. `["13", "14"]` for `13-14`.
pub fn block_points(block: &str) -> Vec<String> {
    block.split('-').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()
}

/// What kind of contact a block is, from its numbers' last digits: three points are a changeover
/// (1-2-4), two ending in 1-2 or 5-6 normally closed, in 3-4 or 7-8 normally open.
pub fn block_kind(block: &str) -> Option<crate::ContactKind> {
    let points = block_points(block);
    let last = |s: &String| s.chars().last();
    match points.as_slice() {
        [_, _, _] => Some(crate::ContactKind::Changeover),
        [a, b] => match (last(a)?, last(b)?) {
            ('1', '2') | ('5', '6') => Some(crate::ContactKind::NormallyClosed),
            ('3', '4') | ('7', '8') => Some(crate::ContactKind::NormallyOpen),
            _ => None,
        },
        _ => None,
    }
}

/// Parses contact blocks written as `13-14 21-22, 11-12-14` (spaces or commas between blocks).
/// Returns the first block that isn't a valid contact as the error.
pub fn parse_blocks(text: &str) -> Result<Vec<String>, String> {
    text.split([' ', ',', ';'])
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let block = block_points(s).join("-");
            if block_kind(&block).is_some() { Ok(block) } else { Err(s.trim().to_owned()) }
        })
        .collect()
}

/// A part used by a device, e.g. a contactor plus its auxiliary block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartRef {
    /// The product, by id. Files keep its number beside it, for people reading them.
    pub product: crate::ProductId,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub quantity: u32,
    /// On a terminal strip or connector, parts normally count once per terminal; this one counts
    /// once for the whole strip (end brackets, end plates, a housing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub per_strip: bool,
    /// On a strip with terminals of more than one kind: count once per terminal drawn with this
    /// symbol only (feed-through terminals with `iec.terminal`, PE terminals with
    /// `iec.terminal_pe`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<crate::SymbolId>,
}

fn one() -> u32 {
    1
}

fn is_one(q: &u32) -> bool {
    *q == 1
}

impl PartRef {
    /// The product with the id [`crate::ProductId::for_number`] gives `number`: the products of
    /// a format 1 project and of the built-in samples.
    pub fn new(number: &str, quantity: u32) -> Self {
        PartRef::to(crate::ProductId::for_number(number), quantity)
    }

    /// `quantity` of `product`.
    pub fn to(product: crate::ProductId, quantity: u32) -> Self {
        PartRef { product, quantity, per_strip: false, only: None }
    }

    /// A part counted once for a whole strip, not per terminal.
    pub fn per_strip(number: &str, quantity: u32) -> Self {
        PartRef { per_strip: true, ..PartRef::new(number, quantity) }
    }

    /// A part counted once per terminal drawn with `symbol`.
    pub fn per_terminal_of(number: &str, quantity: u32, symbol: &str) -> Self {
        PartRef { only: Some(crate::SymbolId::new(symbol)), ..PartRef::new(number, quantity) }
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn sizes_parse_and_print() {
        assert_eq!(parse_size("45 x 90"), Ok((Some(45.0), Some(90.0), None)));
        assert_eq!(parse_size("6,4×90*87"), Ok((Some(6.4), Some(90.0), Some(87.0))));
        assert_eq!(parse_size(""), Ok((None, None, None)));
        assert!(parse_size("45").is_err() && parse_size("45 x 0").is_err() && parse_size("a x b").is_err());
        let p = Part { width: Some(6.4), height: Some(90.0), ..Part::new("X") };
        assert_eq!(format_size(&p), "6,4 x 90");
    }
}
