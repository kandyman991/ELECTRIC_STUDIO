//! The parts catalogue and parts assigned to devices.

use serde::{Deserialize, Serialize};

/// A catalogue entry: something that can be ordered.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// Size in mm, for the panel layout: width along the rail, height across it, depth from
    /// the plate. Parts without one are drawn one module wide on a plate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
    /// The part's product class, as the manufacturer classifies it: an ETIM or ECLASS class
    /// (`EC000066`, `27-37-10-03`) or a class name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// Technical data, in the manufacturer's order: rated current, coil voltage, power...
    /// Each value is text with its unit, as the drawing and the data sheet show it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<Datum>,
    /// Where the data sheet is: a web address or a file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub datasheet: String,
}

/// One line of a part's technical data, e.g. `Rated operational current AC-3` = `9 A`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datum {
    pub name: String,
    pub value: String,
}

impl Datum {
    pub fn new(name: &str, value: &str) -> Self {
        Datum { name: name.to_owned(), value: value.to_owned() }
    }
}

/// At most this many lines of technical data per part.
pub const MAX_DATA: usize = 500;
/// At most this many characters in any one text of a part (a description, a datum's value).
pub const MAX_TEXT: usize = 2000;

impl Part {
    /// A part with just its order number and manufacturer.
    pub fn new(number: &str, manufacturer: &str) -> Self {
        Part { number: number.to_owned(), manufacturer: manufacturer.to_owned(), ..Part::default() }
    }

    /// The value of one line of technical data, by its name (case ignored).
    pub fn datum(&self, name: &str) -> Option<&str> {
        self.data.iter().find(|d| d.name.eq_ignore_ascii_case(name)).map(|d| d.value.as_str())
    }

    /// Whether the part can go into a catalogue: a number, texts within [`MAX_TEXT`], at most
    /// [`MAX_DATA`] lines of data with unique, non-empty names, and sizes above zero. The error
    /// says what is wrong.
    pub fn check(&self) -> Result<(), String> {
        if self.number.trim().is_empty() {
            return Err("the part number is empty".into());
        }
        let texts = [&self.number, &self.manufacturer, &self.description, &self.code, &self.class, &self.datasheet];
        if texts.iter().any(|t| t.chars().count() > MAX_TEXT) {
            return Err(format!("a text is over {MAX_TEXT} characters"));
        }
        if self.data.len() > MAX_DATA {
            return Err(format!("over {MAX_DATA} lines of technical data"));
        }
        let mut names = std::collections::BTreeSet::new();
        for d in &self.data {
            let name = d.name.trim();
            if name.is_empty() {
                return Err("a line of technical data has no name".into());
            }
            if d.name.chars().count() > MAX_TEXT || d.value.chars().count() > MAX_TEXT {
                return Err(format!("technical data `{name}` is over {MAX_TEXT} characters"));
            }
            if !names.insert(name.to_lowercase()) {
                return Err(format!("technical data `{name}` appears twice"));
            }
        }
        if [self.width, self.height, self.depth].iter().flatten().any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err("a size must be above zero".into());
        }
        Ok(())
    }

    /// `newer` laid over this part: every field `newer` has replaces this part's, the ones it
    /// leaves empty are kept (a price list without contact blocks doesn't wipe them), and its
    /// technical data replaces lines of the same name and adds the others after them. The
    /// manufacturer is only filled in, never respelled: both name the same maker.
    pub fn merged_with(&self, newer: &Part) -> Part {
        let text = |new: &String, old: &String| if new.trim().is_empty() { old.clone() } else { new.clone() };
        let mut data = self.data.clone();
        for d in &newer.data {
            match data.iter_mut().find(|o| o.name.eq_ignore_ascii_case(&d.name)) {
                Some(o) => o.value = d.value.clone(),
                None => data.push(d.clone()),
            }
        }
        let sized = newer.width.is_some() || newer.height.is_some();
        Part {
            number: self.number.clone(),
            manufacturer: text(&self.manufacturer, &newer.manufacturer),
            description: text(&newer.description, &self.description),
            code: text(&newer.code, &self.code),
            contacts: if newer.contacts.is_empty() { self.contacts.clone() } else { newer.contacts.clone() },
            width: if sized { newer.width } else { self.width },
            height: if sized { newer.height } else { self.height },
            depth: if sized { newer.depth } else { self.depth },
            class: text(&newer.class, &self.class),
            data,
            datasheet: text(&newer.datasheet, &self.datasheet),
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
    pub number: String,
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
    pub fn new(number: &str, quantity: u32) -> Self {
        PartRef { number: number.to_owned(), quantity, per_strip: false, only: None }
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
        let p = Part { width: Some(6.4), height: Some(90.0), ..Part::new("X", "") };
        assert_eq!(format_size(&p), "6,4 x 90");
    }

    #[test]
    fn a_part_checks_its_data() {
        let mut p = Part::new("X", "Example");
        p.data = vec![Datum::new("Rated current", "9 A"), Datum::new("Coil voltage", "24 V DC")];
        assert_eq!(p.check(), Ok(()));
        assert_eq!(p.datum("rated CURRENT"), Some("9 A"));
        p.data.push(Datum::new("rated current", "12 A"));
        assert_eq!(p.check().unwrap_err(), "technical data `rated current` appears twice");
        p.data = vec![Datum::new(" ", "1")];
        assert!(p.check().is_err());
        p.data = vec![Datum::new("a", &"x".repeat(MAX_TEXT + 1))];
        assert!(p.check().is_err());
        assert!(Part::new(" ", "").check().is_err());
        assert!(Part { width: Some(0.0), ..Part::new("X", "") }.check().is_err());
        assert!(Part { height: Some(f64::NAN), ..Part::new("X", "") }.check().is_err());
    }

    #[test]
    fn a_newer_part_keeps_what_it_leaves_out() {
        let old = Part {
            description: "Contactor".into(),
            contacts: vec!["13-14".into()],
            width: Some(45.0),
            height: Some(90.0),
            data: vec![Datum::new("Coil voltage", "24 V DC"), Datum::new("Rated current", "9 A")],
            ..Part::new("X", "Example")
        };
        let price_list = Part {
            description: "Contactor 4 kW".into(),
            data: vec![Datum::new("rated current", "12 A"), Datum::new("Price", "20")],
            ..Part::new("X", "")
        };
        let m = old.merged_with(&price_list);
        assert_eq!((m.manufacturer.as_str(), m.description.as_str()), ("Example", "Contactor 4 kW"));
        assert_eq!((m.contacts.len(), m.width), (1, Some(45.0)));
        assert_eq!(
            m.data,
            [Datum::new("Coil voltage", "24 V DC"), Datum::new("Rated current", "12 A"), Datum::new("Price", "20")]
        );
        assert_eq!(old.merged_with(&Part::new("X", "")), old);
    }
}
