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
}

/// A part used by a device, e.g. a contactor plus its auxiliary block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartRef {
    pub number: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub quantity: u32,
}

fn one() -> u32 {
    1
}

fn is_one(q: &u32) -> bool {
    *q == 1
}

impl PartRef {
    pub fn new(number: &str, quantity: u32) -> Self {
        PartRef { number: number.to_owned(), quantity }
    }
}
