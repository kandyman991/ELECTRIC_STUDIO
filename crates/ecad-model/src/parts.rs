//! The parts catalogue and parts assigned to devices.

use serde::{Deserialize, Serialize};

/// A catalogue entry: something that can be ordered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    /// Manufacturer's order number, unique in the catalogue, e.g. `3RT2015-1BB41`.
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
    pub number: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub quantity: u32,
    /// On a terminal strip or connector, parts normally count once per terminal; this one counts
    /// once for the whole strip (end brackets, end plates, a housing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub per_strip: bool,
}

fn one() -> u32 {
    1
}

fn is_one(q: &u32) -> bool {
    *q == 1
}

impl PartRef {
    pub fn new(number: &str, quantity: u32) -> Self {
        PartRef { number: number.to_owned(), quantity, per_strip: false }
    }

    /// A part counted once for a whole strip, not per terminal.
    pub fn per_strip(number: &str, quantity: u32) -> Self {
        PartRef { per_strip: true, ..PartRef::new(number, quantity) }
    }
}
