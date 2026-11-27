//! The components library: parts shared by every project, from every manufacturer.
//!
//! A project's catalogue ([`crate::Project::parts`]) holds the parts that project uses, so a
//! project opens on its own. The library is where they come from: parts are imported into it
//! from manufacturers' lists, [`take`]n into a project, and [`refresh`]ed when the library has
//! newer data.
//!
//! In the library a part is known by manufacturer and order number together (two makers can
//! use the same number), both compared without regard to case. In a project, as before, by
//! order number alone.

use crate::{Command, ModelError, Part, Project};
use std::collections::BTreeMap;

/// A part's place in the library: manufacturer and order number, trimmed and lowercase.
pub type Key = (String, String);

/// The key `part` is filed under.
pub fn key(manufacturer: &str, number: &str) -> Key {
    (manufacturer.trim().to_lowercase(), number.trim().to_lowercase())
}

/// The components library.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartsLibrary {
    /// What the library is called, e.g. the company's name.
    pub name: String,
    parts: BTreeMap<Key, Part>,
}

/// What an import into the library did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// Parts left out, with why.
    pub rejected: Vec<String>,
}

impl PartsLibrary {
    pub fn new(name: &str) -> Self {
        PartsLibrary { name: name.to_owned(), parts: BTreeMap::new() }
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Every part, by manufacturer then order number.
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.parts.values()
    }

    pub fn get(&self, manufacturer: &str, number: &str) -> Option<&Part> {
        self.parts.get(&key(manufacturer, number))
    }

    /// The parts with this order number, from any manufacturer.
    pub fn by_number(&self, number: &str) -> Vec<&Part> {
        let n = number.trim().to_lowercase();
        self.parts.iter().filter(|((_, k), _)| *k == n).map(|(_, p)| p).collect()
    }

    /// Put a part in, replacing the one with its manufacturer and number. The part must pass
    /// [`Part::check`].
    pub fn insert(&mut self, mut part: Part) -> Result<Option<Part>, String> {
        part.number = part.number.trim().to_owned();
        part.manufacturer = part.manufacturer.trim().to_owned();
        part.check()?;
        Ok(self.parts.insert(key(&part.manufacturer, &part.number), part))
    }

    pub fn remove(&mut self, manufacturer: &str, number: &str) -> Option<Part> {
        self.parts.remove(&key(manufacturer, number))
    }

    /// Merge parts in, as from a manufacturer's list: new parts are added; a part already
    /// there gets every field the list has ([`Part::merged_with`]), so importing a price list
    /// keeps the technical data another import brought.
    pub fn import(&mut self, parts: Vec<Part>) -> Imported {
        let mut out = Imported::default();
        for part in parts {
            let k = key(&part.manufacturer, &part.number);
            let merged = match self.parts.get(&k) {
                Some(old) => {
                    let merged = old.merged_with(&part);
                    if merged == *old {
                        out.unchanged += 1;
                        continue;
                    }
                    merged
                }
                None => part,
            };
            let label = format!("{} {}", merged.manufacturer, merged.number).trim().to_owned();
            match self.insert(merged) {
                Ok(Some(_)) => out.updated += 1,
                Ok(None) => out.added += 1,
                Err(e) => out.rejected.push(format!("{label}: {e}")),
            }
        }
        out
    }

    /// The parts matching every word of `query` (case ignored) in their order number,
    /// manufacturer, description, company code, class or technical data, best first: an exact
    /// order number, then one that starts with the query, then the rest by manufacturer and
    /// number. At most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Part> {
        let q = query.trim().to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<(u8, &Part)> = self
            .parts
            .iter()
            .filter(|(_, p)| {
                let text = haystack(p);
                words.iter().all(|w| text.contains(w))
            })
            .map(|((_, n), p)| {
                let rank = if *n == q {
                    0
                } else if !q.is_empty() && n.starts_with(&q) {
                    1
                } else {
                    2
                };
                (rank, p)
            })
            .collect();
        // Stable: within a rank, the map's manufacturer-then-number order.
        hits.sort_by_key(|(rank, _)| *rank);
        hits.into_iter().take(limit).map(|(_, p)| p).collect()
    }
}

/// Everything a search looks in, lowercase.
fn haystack(p: &Part) -> String {
    let mut s = [&p.number, &p.manufacturer, &p.description, &p.code, &p.class].map(|t| t.as_str()).join("\n");
    for d in &p.data {
        s.push('\n');
        s.push_str(&d.name);
        s.push(' ');
        s.push_str(&d.value);
    }
    s.to_lowercase()
}

/// What [`take`] or [`refresh`] would change in a project's catalogue.
#[derive(Debug, Default, PartialEq)]
pub struct Taken {
    /// The edit to apply, if anything changes.
    pub command: Option<Command>,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
}

/// The project's version of a part from the library or a list: its data laid over the
/// project's own copy, so what was only entered in the project (contact blocks, a size) stays.
/// `None` if that changes nothing; an error if the project has the number from another maker.
pub fn for_project(project: &Project, part: &Part) -> Result<Option<Part>, ModelError> {
    match project.parts.get(part.number.trim()) {
        None => Ok(Some(part.clone())),
        Some(own) => {
            let same_maker = own.manufacturer.trim().is_empty()
                || part.manufacturer.trim().is_empty()
                || own.manufacturer.trim().eq_ignore_ascii_case(part.manufacturer.trim());
            if !same_maker {
                return Err(ModelError::InvalidPart(
                    own.number.clone(),
                    format!("the project already has it from {}, not {}", own.manufacturer, part.manufacturer),
                ));
            }
            let merged = own.merged_with(part);
            Ok((merged != *own).then_some(merged))
        }
    }
}

/// Take library parts into a project's catalogue: each `(manufacturer, number)` asked for, or
/// with no manufacturer, the only part with that number. A part the project already has is
/// brought up to date.
pub fn take(project: &Project, library: &PartsLibrary, wanted: &[(Option<&str>, &str)]) -> Result<Taken, ModelError> {
    let mut out = Taken::default();
    let mut cmds = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (maker, number) in wanted {
        let part = match maker {
            Some(m) => library.get(m, number).ok_or_else(|| ModelError::UnknownPart(format!("{m} {number}")))?,
            None => match library.by_number(number).as_slice() {
                [] => return Err(ModelError::UnknownPart((*number).to_owned())),
                [one] => *one,
                many => {
                    let makers: Vec<&str> = many.iter().map(|p| p.manufacturer.as_str()).collect();
                    return Err(ModelError::InvalidPart(
                        (*number).to_owned(),
                        format!("the library has it from {}; say which", makers.join(", ")),
                    ));
                }
            },
        };
        if !seen.insert(part.number.trim().to_lowercase()) {
            continue;
        }
        let number = part.number.clone();
        match for_project(project, part)? {
            Some(p) if project.parts.contains_key(&p.number) => {
                out.updated.push(number);
                cmds.push(Command::UpdatePart(p));
            }
            Some(p) => {
                out.added.push(number);
                cmds.push(Command::AddPart(p));
            }
            None => out.unchanged.push(number),
        }
    }
    out.command = (!cmds.is_empty()).then_some(Command::Batch(cmds));
    Ok(out)
}

/// Put a list's parts into a project's catalogue: new ones added, ones it has brought up to
/// date ([`for_project`]). The catalogue knows a part by number alone, so a list with one
/// number from two makers is refused.
pub fn import_into(project: &Project, parts: Vec<Part>) -> Result<Taken, ModelError> {
    let mut out = Taken::default();
    let mut cmds = Vec::new();
    let mut seen = BTreeMap::new();
    for part in parts {
        let number = part.number.trim().to_owned();
        if let Some(maker) = seen.insert(number.to_lowercase(), part.manufacturer.clone()) {
            return Err(ModelError::InvalidPart(
                number,
                format!("the list has it from both {maker} and {}", part.manufacturer),
            ));
        }
        match for_project(project, &part)? {
            Some(p) if project.parts.contains_key(&p.number) => {
                out.updated.push(number);
                cmds.push(Command::UpdatePart(p));
            }
            Some(p) => {
                out.added.push(number);
                cmds.push(Command::AddPart(p));
            }
            None => out.unchanged.push(number),
        }
    }
    out.command = (!cmds.is_empty()).then_some(Command::Batch(cmds));
    Ok(out)
}

/// Bring every part of a project's catalogue up to date from the library. Parts the library
/// doesn't have, or has from more than one manufacturer while the project names none, are
/// left alone and listed in the second value.
pub fn refresh(project: &Project, library: &PartsLibrary) -> Result<(Taken, Vec<String>), ModelError> {
    let mut wanted = Vec::new();
    let mut missing = Vec::new();
    for own in project.parts.values() {
        let found = if own.manufacturer.trim().is_empty() {
            match library.by_number(&own.number).as_slice() {
                [one] => Some(*one),
                _ => None,
            }
        } else {
            library.get(&own.manufacturer, &own.number)
        };
        match found {
            Some(p) => wanted.push((Some(p.manufacturer.as_str()), own.number.as_str())),
            None => missing.push(own.number.clone()),
        }
    }
    Ok((take(project, library, &wanted)?, missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Datum;

    fn part(maker: &str, number: &str, description: &str) -> Part {
        Part { description: description.into(), ..Part::new(number, maker) }
    }

    fn library() -> PartsLibrary {
        let mut lib = PartsLibrary::new("Test");
        let mut k = part("Example", "EX-CT-9", "Contactor 4 kW 24 V DC");
        k.data = vec![Datum::new("Rated current AC-3", "9 A")];
        let got = lib.import(vec![
            k,
            part("Example", "EX-CT-12", "Contactor 5.5 kW"),
            part("Other", "EX-CT-9", "Not the same contactor"),
            part("Other", "MCB-C16", "Circuit breaker C16"),
        ]);
        assert_eq!((got.added, got.updated, got.rejected.len()), (4, 0, 0));
        lib
    }

    #[test]
    fn parts_are_known_by_maker_and_number() {
        let mut lib = library();
        assert_eq!(lib.len(), 4);
        assert_eq!(lib.get("EXAMPLE", "ex-ct-9").map(|p| p.description.as_str()), Some("Contactor 4 kW 24 V DC"));
        assert_eq!(lib.by_number("EX-CT-9").len(), 2);
        // A price list for one maker updates only that maker's part and keeps its data.
        let got = lib.import(vec![
            part("example", "EX-CT-9 ", "Contactor 4 kW"),
            part("Example", "EX-CT-12", ""),
            Part::new("", "Example"),
        ]);
        assert_eq!((got.added, got.updated, got.unchanged, got.rejected.len()), (0, 1, 1, 1));
        let k = lib.get("Example", "EX-CT-9").unwrap();
        assert_eq!((k.description.as_str(), k.datum("rated current ac-3")), ("Contactor 4 kW", Some("9 A")));
        assert_eq!(k.manufacturer, "Example");
        assert_eq!(lib.get("Other", "EX-CT-9").unwrap().description, "Not the same contactor");
    }

    #[test]
    fn search_finds_every_word_best_first() {
        let lib = library();
        let numbers = |q: &str| lib.search(q, 10).iter().map(|p| p.number.clone()).collect::<Vec<_>>();
        assert_eq!(numbers("contactor kw"), ["EX-CT-12", "EX-CT-9"]);
        assert_eq!(numbers("ex-ct-9"), ["EX-CT-9", "EX-CT-9"]);
        assert_eq!(numbers("ex-ct"), ["EX-CT-12", "EX-CT-9", "EX-CT-9"]);
        assert_eq!(numbers("rated current"), ["EX-CT-9"], "technical data is searched too");
        assert_eq!(numbers("other breaker"), ["MCB-C16"]);
        assert!(numbers("nothing like it").is_empty());
        assert_eq!(lib.search("", 2).len(), 2);
    }

    #[test]
    fn parts_go_into_a_project_and_come_up_to_date() {
        let mut lib = library();
        let mut p = Project::default();
        let ambiguous = take(&p, &lib, &[(None, "EX-CT-9")]).unwrap_err();
        assert!(ambiguous.to_string().contains("Example, Other"), "{ambiguous}");
        let t = take(&p, &lib, &[(Some("Example"), "EX-CT-9"), (None, "MCB-C16"), (None, "mcb-c16")]).unwrap();
        assert_eq!((t.added.len(), t.updated.len()), (2, 0));
        p.apply(t.command.unwrap()).unwrap();
        assert_eq!(p.parts["EX-CT-9"].datum("Rated current AC-3"), Some("9 A"));

        // Contact blocks entered in the project survive an update from the library.
        let mut own = p.parts["EX-CT-9"].clone();
        own.contacts = vec!["13-14".into()];
        p.apply(Command::UpdatePart(own)).unwrap();
        lib.import(vec![part("Example", "EX-CT-9", "Contactor 4 kW, new text")]);
        let (t, missing) = refresh(&p, &lib).unwrap();
        assert_eq!(
            (t.updated.as_slice(), t.unchanged.as_slice()),
            (&["EX-CT-9".to_owned()][..], &["MCB-C16".to_owned()][..])
        );
        assert!(missing.is_empty());
        p.apply(t.command.unwrap()).unwrap();
        let k = &p.parts["EX-CT-9"];
        assert_eq!((k.description.as_str(), k.contacts.len()), ("Contactor 4 kW, new text", 1));
        assert_eq!(refresh(&p, &lib).unwrap().0.command, None);

        // The same number from another maker doesn't replace the project's part.
        assert!(take(&p, &lib, &[(Some("Other"), "EX-CT-9")]).is_err());
        p.apply(Command::AddPart(Part::new("HAND-1", ""))).unwrap();
        assert_eq!(refresh(&p, &lib).unwrap().1, ["HAND-1"]);
    }

    #[test]
    fn a_list_goes_straight_into_a_project() {
        let mut p = Project::default();
        let t = import_into(&p, vec![part("M", "A", "one"), part("M", "B", "two")]).unwrap();
        assert_eq!(t.added, ["A", "B"]);
        p.apply(t.command.unwrap()).unwrap();
        let t = import_into(&p, vec![part("", "A", "one"), part("M", "B", "two, new")]).unwrap();
        assert_eq!((t.unchanged.as_slice(), t.updated.as_slice()), (&["A".to_owned()][..], &["B".to_owned()][..]));
        assert!(import_into(&p, vec![part("M", "C", ""), part("N", "c", "")]).is_err());
        assert!(import_into(&p, vec![part("Other", "A", "")]).is_err());
    }
}
