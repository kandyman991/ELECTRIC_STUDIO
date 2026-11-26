//! How new devices and wires are named, as a project setting.
//!
//! Tags come from a format such as `-{letters}{n}` (`-K1`) or `{page}{letters}{n}` (`A2SB1`,
//! page A2, letter code SB). Wire numbers are either one per wire (`W1`, `W2`…) or one per
//! potential: every wire of a potential carries the potential's name (`+A100`, `L1`) or a number
//! from a format such as `{page}{n:02}` (`P201` on page P2).

use crate::project::{Command, ConnectionId, ConnectionRef, PageId, Project};
use crate::symbol::{Role, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireNumbering {
    /// Every wire its own number.
    #[default]
    PerWire,
    /// All wires of a potential share one number: its name, or one from the format.
    PerPotential,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Naming {
    /// New device tags. `{page}` the page name, `{letters}` the symbol's letter code (its tag
    /// prefix without the leading `-`), `{n}` the first free number.
    pub tag_format: String,
    pub wire_numbering: WireNumbering,
    /// New wire numbers. `{page}`, and `{n}` or `{n:02}` (zero-padded to two digits).
    pub wire_format: String,
}

impl Default for Naming {
    fn default() -> Self {
        Naming {
            tag_format: "-{letters}{n}".into(),
            wire_numbering: WireNumbering::PerWire,
            wire_format: "W{n}".into(),
        }
    }
}

impl Naming {
    /// The style of the reference project: `A2SB1`, and `P201` per potential.
    pub fn by_page() -> Self {
        Naming {
            tag_format: "{page}{letters}{n}".into(),
            wire_numbering: WireNumbering::PerPotential,
            wire_format: "{page}{n:02}".into(),
        }
    }

    /// Whether the formats can produce names: each needs `{n}` (or `{n:0W}`) once, and only
    /// known placeholders.
    pub fn is_valid(&self) -> bool {
        valid(&self.tag_format, &["page", "letters"]) && valid(&self.wire_format, &["page"])
    }
}

/// Split a format at its counter: (text before, zero-pad width, text after), with the other
/// placeholders filled in.
fn split(format: &str, page: &str, letters: &str) -> Option<(String, usize, String)> {
    let filled = format.replace("{page}", page).replace("{letters}", letters);
    let start = filled.find("{n")?;
    let end = start + filled[start..].find('}')?;
    let width = match &filled[start + 2..end] {
        "" => 0,
        spec => spec.strip_prefix(':')?.parse().ok()?,
    };
    Some((filled[..start].to_owned(), width, filled[end + 1..].to_owned()))
}

fn valid(format: &str, allowed: &[&str]) -> bool {
    let mut rest = format;
    let mut counters = 0;
    while let Some(i) = rest.find('{') {
        let Some(j) = rest[i..].find('}') else { return false };
        let name = &rest[i + 1..i + j];
        if name == "n" || name.strip_prefix("n:0").is_some_and(|w| w.parse::<u8>().is_ok_and(|w| w <= 6)) {
            counters += 1;
        } else if !allowed.contains(&name) {
            return false;
        }
        rest = &rest[i + j + 1..];
    }
    counters == 1
}

/// The first name `before{n}after`, n from 1, for which `taken` is false.
fn first_free(before: &str, width: usize, after: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..).map(|n| format!("{before}{n:0width$}{after}")).find(|s| !taken(s)).unwrap_or_default()
}

impl Project {
    /// The tag for a new device drawn with `symbol` on `page`. Potential and interruption points
    /// and junctions are names, not device tags: they keep their own prefix (`L+1`, `I1`, `J1`).
    pub fn new_tag(&self, symbol: &SymbolId, page: PageId) -> String {
        let (prefix, role) = match self.symbol(symbol) {
            Ok(s) => (s.tag_prefix.clone(), s.role),
            Err(_) => ("-".to_owned(), Role::Single),
        };
        let is_device = prefix.starts_with('-') && !role.joins_drawings();
        let page = self.page(page).map(|p| p.name.clone()).unwrap_or_default();
        let parts = if is_device { split(&self.naming.tag_format, &page, &prefix[1..]) } else { None };
        match parts {
            Some((before, width, after)) => first_free(&before, width, &after, |t| self.device_by_tag(t).is_some()),
            None => self.next_tag(if prefix.is_empty() { "-" } else { &prefix }),
        }
    }

    fn wire_number_taken(&self, number: &str) -> bool {
        self.connections.values().any(|c| c.wire_number.as_deref() == Some(number))
    }

    /// The first free number from the wire format on `page`.
    pub fn next_wire_number(&self, page: PageId) -> String {
        let page = self.page(page).map(|p| p.name.clone()).unwrap_or_default();
        match split(&self.naming.wire_format, &page, "") {
            Some((before, width, after)) => first_free(&before, width, &after, |n| self.wire_number_taken(n)),
            None => first_free("W", 0, "", |n| self.wire_number_taken(n)),
        }
    }

    /// The number for a new wire from `from` to `to`, and the wires that must take it too. Per
    /// wire: the next free number. Per potential: the name of the potential the wire joins, or
    /// the number its wires already carry (the `from` side wins), or a new one; every other wire
    /// of the joined potential is renumbered to match.
    pub fn number_for_new_wire(&self, from: &ConnectionRef, to: &ConnectionRef) -> (String, Vec<ConnectionId>) {
        let page = self.function(from.function).map(|f| f.page).unwrap_or_default();
        if self.naming.wire_numbering == WireNumbering::PerWire {
            return (self.next_wire_number(page), Vec::new());
        }
        let potentials = self.potentials();
        let find = |r: &ConnectionRef| potentials.iter().find(|p| p.points.contains(r));
        let sides: Vec<_> = [find(from), find(to)].into_iter().flatten().collect();
        let name = sides.iter().find_map(|p| p.name());
        let existing = sides.iter().find_map(|p| {
            p.connections.iter().find_map(|c| self.connection(*c).ok().and_then(|c| c.wire_number.clone()))
        });
        let number = name.or(existing).unwrap_or_else(|| self.next_wire_number(page));
        let renumber = sides
            .iter()
            .flat_map(|p| p.connections.iter())
            .filter(|c| self.connection(**c).ok().and_then(|c| c.wire_number.as_deref()) != Some(number.as_str()))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        (number, renumber)
    }

    /// Commands that give wire `c` the number `number`: per potential, every wire of its
    /// potential.
    pub fn set_wire_number_commands(&self, c: ConnectionId, number: Option<String>) -> Vec<Command> {
        let wires = match self.naming.wire_numbering {
            WireNumbering::PerWire => vec![c],
            WireNumbering::PerPotential => self.potential_of(c).map_or_else(|| vec![c], |p| p.connections),
        };
        wires.into_iter().map(|id| Command::SetWireNumber { id, number: number.clone() }).collect()
    }

    /// Commands that renumber every wire from scratch, page by page in page order and down each
    /// page column by column. Per potential, a named potential takes its name and the others
    /// a new number on the page where they start.
    pub fn renumber_all_wires(&self) -> Vec<Command> {
        let order: Vec<PageId> = self.pages_in_order().iter().map(|p| p.id).collect();
        let place = |c: ConnectionId| {
            let f = self.connection(c).ok().and_then(|c| self.function(c.from.function).ok());
            let page = f.map(|f| f.page).unwrap_or_default();
            let at = f.map_or(crate::Point::ZERO, |f| f.position);
            (order.iter().position(|p| *p == page).unwrap_or(usize::MAX), self.sheet.column_of(at), -at.y, page)
        };
        // Clear first, so new numbers can reuse old ones.
        let mut scratch = self.clone();
        let mut cmds = Vec::new();
        for id in self.connections.keys() {
            cmds.push(Command::SetWireNumber { id: *id, number: None });
            if let Some(c) = scratch.connections.get_mut(id) {
                c.wire_number = None;
            }
        }
        let mut groups: Vec<(Option<String>, Vec<ConnectionId>)> = match self.naming.wire_numbering {
            WireNumbering::PerWire => self.connections.keys().map(|c| (None, vec![*c])).collect(),
            WireNumbering::PerPotential => self
                .potentials()
                .into_iter()
                .filter(|p| !p.connections.is_empty())
                .map(|p| (p.name(), p.connections))
                .collect(),
        };
        let first = |ids: &[ConnectionId]| {
            ids.iter().map(|c| place(*c)).min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        };
        groups.sort_by(|a, b| first(&a.1).partial_cmp(&first(&b.1)).unwrap_or(std::cmp::Ordering::Equal));
        for (name, ids) in groups {
            let page = first(&ids).map(|p| p.3).unwrap_or_default();
            let number = name.unwrap_or_else(|| scratch.next_wire_number(page));
            for id in ids {
                if let Some(c) = scratch.connections.get_mut(&id) {
                    c.wire_number = Some(number.clone());
                }
                cmds.push(Command::SetWireNumber { id, number: Some(number.clone()) });
            }
        }
        cmds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_split_at_the_counter() {
        assert_eq!(split("{page}{n:02}", "P2", ""), Some(("P2".into(), 2, String::new())));
        assert_eq!(split("-{letters}{n}", "1", "KM"), Some(("-KM".into(), 0, String::new())));
        assert_eq!(split("W{n}x", "1", ""), Some(("W".into(), 0, "x".into())));
        assert_eq!(split("no counter", "1", ""), None);
        assert!(Naming::default().is_valid() && Naming::by_page().is_valid());
        assert!(!Naming { tag_format: "{page}{letters}".into(), ..Naming::default() }.is_valid(), "no counter");
        assert!(!Naming { wire_format: "{n}{n}".into(), ..Naming::default() }.is_valid(), "two counters");
        assert!(!Naming { wire_format: "{letters}{n}".into(), ..Naming::default() }.is_valid(), "no letters for wires");
        assert_eq!(first_free("P2", 2, "", |s| s == "P201"), "P202");
    }

    use crate::geometry::{Point, Rotation};
    use crate::project::{Connection, Device, DeviceId, Function, FunctionId};
    use crate::sample::{PAGE_1, start_circuit};

    fn place(p: &mut Project, symbol: &str, tag: Option<&str>, at: (f64, f64)) -> FunctionId {
        let symbol = SymbolId::new(symbol);
        let tag = tag.map_or_else(|| p.new_tag(&symbol, PAGE_1), str::to_owned);
        let device = match p.device_by_tag(&tag) {
            Some(d) => d.id,
            None => {
                let id = DeviceId::new();
                p.add_device(Device::new(id, &tag, "")).unwrap();
                id
            }
        };
        let id = FunctionId::new();
        let designations = p.next_designations(device, &symbol);
        p.place(Function {
            id,
            device,
            symbol,
            page: PAGE_1,
            position: Point::new(at.0, at.1),
            rotation: Rotation::R0,
            designations,
            properties: Default::default(),
        })
        .unwrap();
        id
    }

    fn wire(p: &mut Project, a: (FunctionId, &str), b: (FunctionId, &str)) -> String {
        let (from, to) = (ConnectionRef::new(a.0, a.1), ConnectionRef::new(b.0, b.1));
        let (number, renumber) = p.number_for_new_wire(&from, &to);
        for id in renumber {
            p.apply(Command::SetWireNumber { id, number: Some(number.clone()) }).unwrap();
        }
        p.connect(Connection {
            id: ConnectionId::new(),
            from,
            to,
            wire_number: Some(number.clone()),
            spec: Default::default(),
        })
        .unwrap();
        number
    }

    #[test]
    fn tags_follow_the_format_and_potentials_share_one_number() {
        let mut p = start_circuit().unwrap();
        assert_eq!(p.new_tag(&SymbolId::new("iec.coil"), PAGE_1), "-K2", "the default format");
        p.apply(Command::SetNaming(Naming::by_page())).unwrap();
        assert_eq!(p.new_tag(&SymbolId::new("iec.coil"), PAGE_1), "1K1");
        assert_eq!(p.new_tag(&SymbolId::new("iec.junction"), PAGE_1), "J1", "junctions keep their own names");

        // A wire from a named potential takes its name.
        let l_plus = place(&mut p, "iec.supply", Some("L+"), (100.0, 80.0));
        let k2 = place(&mut p, "iec.coil", None, (100.0, 40.0));
        assert_eq!(p.device(p.function(k2).unwrap().device).unwrap().tag, "1K1");
        assert_eq!(wire(&mut p, (l_plus, "1"), (k2, "A1")), "L+");
        // An unnamed potential gets the page's next number.
        let k3 = place(&mut p, "iec.coil", None, (140.0, 40.0));
        assert_eq!(wire(&mut p, (k2, "A2"), (k3, "A1")), "101");

        // Two numbered potentials joined by a new wire: the from side's number wins everywhere.
        let j = place(&mut p, "iec.junction", None, (140.0, 0.0));
        let k4 = place(&mut p, "iec.coil", None, (180.0, 40.0));
        let w = wire(&mut p, (k3, "A2"), (j, "1"));
        assert_eq!(w, "102");
        let j2 = place(&mut p, "iec.junction", None, (180.0, 0.0));
        p.connect(Connection {
            id: ConnectionId::new(),
            from: ConnectionRef::new(k4, "A1"),
            to: ConnectionRef::new(j2, "1"),
            wire_number: Some("X9".into()),
            spec: Default::default(),
        })
        .unwrap();
        assert_eq!(wire(&mut p, (j, "2"), (j2, "2")), "102");
        let numbers: BTreeSet<String> = p.connections.values().filter_map(|c| c.wire_number.clone()).collect();
        assert!(!numbers.contains("X9"), "X9 joined 102: {numbers:?}");

        // Renumbering a wire renumbers its whole potential.
        let any = p.connections.values().find(|c| c.wire_number.as_deref() == Some("102")).unwrap().id;
        let cmds = p.set_wire_number_commands(any, Some("150".into()));
        assert_eq!(cmds.len(), 3, "k3-j, j-j2 and j2-k4");
    }

    #[test]
    fn renumbering_all_wires_follows_the_pages() {
        let mut p = start_circuit().unwrap();
        p.apply(Command::SetNaming(Naming::by_page())).unwrap();
        let cmds = p.renumber_all_wires();
        p.apply(Command::Batch(cmds)).unwrap();
        let mut numbers: Vec<String> = p.connections.values().filter_map(|c| c.wire_number.clone()).collect();
        numbers.sort();
        assert_eq!(numbers, ["101", "L+", "M"], "S1 to K1 is the only unnamed potential");
        // Per wire, back to W1… in page order.
        p.apply(Command::SetNaming(Naming::default())).unwrap();
        p.apply(Command::Batch(p.renumber_all_wires())).unwrap();
        let mut numbers: Vec<String> = p.connections.values().filter_map(|c| c.wire_number.clone()).collect();
        numbers.sort();
        assert_eq!(numbers, ["W1", "W2", "W3"]);
    }
}
