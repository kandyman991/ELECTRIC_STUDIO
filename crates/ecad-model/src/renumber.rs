//! Explicit global renumbering (S0, D22): a separate operation, never part of another, always
//! previewed and confirmed. A [`Renumbering`] lists every change and holds the commands that
//! make them; [`Project::apply_renumbering`] applies it only to the project it was previewed
//! on, as one undo step. Canonical ids (devices, terminals, wires, pages) never change: only
//! the names people read.

use crate::numbering::{Identifier, Kind, Policies, Strategy};
use crate::project::{Command, DeviceId, FunctionId, ModelError, PageId, Project, Result};
use std::collections::{BTreeMap, BTreeSet};

/// What a renumbering covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every wire number, from the wire format, page by page.
    Wires,
    /// Every device tag `<prefix><n>`, per prefix, in drawing order.
    Devices,
    /// The terminal numbers of one strip, in drawing order. Undrawn groups keep theirs.
    Terminals(DeviceId),
    /// The page numbers, in page order; a suffixed page (`3a`) follows its page.
    Pages,
}

/// One name that changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// What it names: `-K3`, `wire -K1:A1 – -S1:14`, `page 4`, `-X1 terminal`.
    pub what: String,
    pub from: String,
    pub to: String,
}

/// A previewed renumbering: what changes, and the commands that change it.
#[derive(Clone, Debug, PartialEq)]
pub struct Renumbering {
    pub scope: Scope,
    pub changes: Vec<Change>,
    pub commands: Vec<Command>,
    /// The content hash of the project it was previewed on.
    pub base: String,
}

/// A sortable drawing position: page order, column, top to bottom, left to right.
type Place = (usize, u32, i64, i64);

impl Project {
    fn place_of(&self, f: FunctionId, pages: &BTreeMap<PageId, usize>) -> Option<Place> {
        let f = self.function(f).ok()?;
        let page = pages.get(&f.page).copied().unwrap_or(usize::MAX);
        let mm = |v: f64| (v * 1000.0).round() as i64;
        Some((page, self.sheet.column_of(f.position), -mm(f.position.y), mm(f.position.x)))
    }

    fn page_index(&self) -> BTreeMap<PageId, usize> {
        self.pages_in_order().iter().enumerate().map(|(i, p)| (p.id, i)).collect()
    }

    /// Preview renumbering `scope`: nothing changes until [`Self::apply_renumbering`].
    pub fn preview_renumbering(&self, scope: Scope) -> Result<Renumbering> {
        let (changes, commands) = match scope {
            Scope::Wires => self.wire_renumbering(),
            Scope::Devices => self.device_renumbering(),
            Scope::Terminals(strip) => self.terminal_renumbering(strip)?,
            Scope::Pages => self.page_renumbering(),
        };
        Ok(Renumbering { scope, changes, commands, base: self.content_hash()? })
    }

    /// Apply a previewed renumbering, as one transaction; refused if the project changed
    /// since the preview. Returns the command that undoes it.
    pub fn apply_renumbering(&mut self, r: &Renumbering) -> Result<Command> {
        if self.content_hash()? != r.base {
            return Err(ModelError::StalePreview);
        }
        self.apply(Command::Batch(r.commands.clone()))
    }

    fn wire_renumbering(&self) -> (Vec<Change>, Vec<Command>) {
        // From scratch: the lowest free numbers, still skipping reserved and retired ones.
        let mut scratch = self.clone();
        scratch.underlays.clear();
        let mut strategies = scratch.numbering.policies.strategies.clone();
        strategies.insert(Kind::Wire, Strategy::LowestFree);
        scratch.numbering.policies = Policies { strategies, ..scratch.numbering.policies.clone() };
        let commands = scratch.renumber_all_wires();
        let mut last: BTreeMap<crate::ConnectionId, Option<String>> = BTreeMap::new();
        for c in &commands {
            if let Command::SetWireNumber { id, number } = c {
                last.insert(*id, number.clone());
            }
        }
        let changes = last
            .into_iter()
            .filter(|(id, n)| self.wire_number(*id) != n.as_deref())
            .map(|(id, n)| Change {
                what: format!("wire {}", self.wire_ends_label(id)),
                from: self.wire_number(id).unwrap_or("").to_owned(),
                to: n.unwrap_or_default(),
            })
            .collect();
        (changes, commands)
    }

    fn wire_ends_label(&self, id: crate::ConnectionId) -> String {
        let label = |tag: String, point: String| if point.is_empty() { tag } else { format!("{tag}:{point}") };
        if let Ok(c) = self.connection(id) {
            let a = self.describe(&c.from).map(|(t, p)| label(t, p)).unwrap_or_default();
            let b = self.describe(&c.to).map(|(t, p)| label(t, p)).unwrap_or_default();
            return format!("{a} – {b}");
        }
        match self.undrawn.get(&id) {
            Some(w) => {
                let (a, b) = (self.describe_end(&w.from), self.describe_end(&w.to));
                format!("{} – {}", label(a.0, a.1), label(b.0, b.1))
            }
            None => String::new(),
        }
    }

    fn device_renumbering(&self) -> (Vec<Change>, Vec<Command>) {
        let pages = self.page_index();
        // Tags `<prefix><n>` by prefix; others keep theirs.
        let mut series: BTreeMap<String, Vec<(Option<Place>, u64, DeviceId)>> = BTreeMap::new();
        for d in self.devices.values() {
            let digits = d.tag.len() - d.tag.trim_end_matches(|c: char| c.is_ascii_digit()).len();
            let (prefix, n) = d.tag.split_at(d.tag.len() - digits);
            if digits == 0 || prefix.is_empty() {
                continue;
            }
            let Ok(n) = n.parse::<u64>() else { continue };
            let first = self.functions_of(d.id).filter_map(|f| self.place_of(f.id, &pages)).min();
            series.entry(prefix.to_owned()).or_default().push((first, n, d.id));
        }
        let mut renames = Vec::new();
        for (prefix, mut members) in series {
            // Drawn in drawing order, then undrawn by their number.
            members.sort_by_key(|(place, n, id)| (place.is_none(), *place, *n, *id));
            let ours: BTreeSet<DeviceId> = members.iter().map(|m| m.2).collect();
            let mut n = 0u64;
            for (_, _, id) in members {
                let tag = loop {
                    n += 1;
                    let t = format!("{prefix}{n}");
                    let other = self.device_by_tag(&t).is_some_and(|d| !ours.contains(&d.id));
                    if !other && self.available(&Identifier::Tag { tag: t.clone() }) {
                        break t;
                    }
                };
                let old = self.devices.get(&id).map(|d| d.tag.clone()).unwrap_or_default();
                if tag != old {
                    renames.push((id, old, tag));
                }
            }
        }
        // Through temporary names, so no two devices share a tag on the way.
        let mut commands: Vec<Command> = renames
            .iter()
            .map(|(id, _, _)| Command::RenameDevice { id: *id, tag: format!("~renumbering~{}", id.0) })
            .collect();
        commands.extend(renames.iter().map(|(id, _, tag)| Command::RenameDevice { id: *id, tag: tag.clone() }));
        let changes = renames.into_iter().map(|(_, from, to)| Change { what: from.clone(), from, to }).collect();
        (changes, commands)
    }

    fn terminal_renumbering(&self, strip: DeviceId) -> Result<(Vec<Change>, Vec<Command>)> {
        let tag = self.device(strip)?.tag.clone();
        let pages = self.page_index();
        // Each numbered group with a drawing that shows it; undrawn groups keep their numbers.
        let mut groups: BTreeMap<crate::connectivity::GroupId, (Place, FunctionId, String)> = BTreeMap::new();
        let mut fixed: BTreeSet<String> = BTreeSet::new();
        for t in self.terminals.values().filter(|t| t.device == strip && !t.point.is_empty()) {
            let shown = self
                .functions_of(strip)
                .filter(|f| f.terminals.values().any(|id| *id == t.id))
                .filter_map(|f| Some((self.place_of(f.id, &pages)?, f.id)))
                .min();
            match shown {
                Some((place, f)) => {
                    let slot = groups.entry(t.group).or_insert((place, f, t.designation.clone()));
                    if place < slot.0 {
                        *slot = (place, f, t.designation.clone());
                    }
                }
                None => {
                    fixed.insert(t.designation.clone());
                }
            }
        }
        let mut ordered: Vec<_> = groups.into_values().collect();
        ordered.sort();
        let mut renames = Vec::new();
        let mut n = 0u64;
        for (_, f, old) in ordered {
            let number = loop {
                n += 1;
                let d = n.to_string();
                if !fixed.contains(&d)
                    && self.available(&Identifier::Terminal { device: strip, designation: d.clone() })
                {
                    break d;
                }
            };
            if number != old {
                renames.push((f, old, number));
            }
        }
        let all = |f: FunctionId, d: &str| -> BTreeMap<String, String> {
            self.function(f)
                .map(|f| f.terminals.keys().map(|p| (p.clone(), d.to_owned())).collect())
                .unwrap_or_default()
        };
        let mut commands: Vec<Command> = renames
            .iter()
            .map(|(f, _, _)| Command::SetDesignations { id: *f, designations: all(*f, &format!("~{}", f.0)) })
            .collect();
        commands.extend(renames.iter().map(|(f, _, d)| Command::SetDesignations { id: *f, designations: all(*f, d) }));
        let changes =
            renames.into_iter().map(|(_, from, to)| Change { what: format!("{tag}:{from}"), from, to }).collect();
        Ok((changes, commands))
    }

    fn page_renumbering(&self) -> (Vec<Change>, Vec<Command>) {
        let order = self.pages_in_order();
        let numeric: Vec<&crate::Page> = order
            .iter()
            .copied()
            .filter(|p| !p.name.is_empty() && p.name.bytes().all(|b| b.is_ascii_digit()))
            .collect();
        let others: BTreeSet<&str> =
            order.iter().filter(|p| !numeric.iter().any(|q| q.id == p.id)).map(|p| p.name.as_str()).collect();
        let mut roots: BTreeMap<String, String> = BTreeMap::new();
        let mut n = 0u64;
        for p in &numeric {
            let name = loop {
                n += 1;
                let s = n.to_string();
                if !others.contains(s.as_str()) && self.available(&Identifier::Page { name: s.clone() }) {
                    break s;
                }
            };
            roots.insert(p.name.clone(), name);
        }
        // A suffixed page follows its page: 3a becomes 4a when 3 becomes 4.
        let mut renames = Vec::new();
        for p in &order {
            let root = &p.name[..p.name.find(|c: char| !c.is_ascii_digit()).unwrap_or(p.name.len())];
            let Some(new_root) = roots.get(root) else { continue };
            let name = format!("{new_root}{}", &p.name[root.len()..]);
            if name != p.name {
                renames.push((p.id, p.name.clone(), name));
            }
        }
        // Renamed pages may not land on a name kept by a page that isn't renamed.
        let kept: BTreeSet<&str> =
            order.iter().filter(|p| !renames.iter().any(|(id, _, _)| *id == p.id)).map(|p| p.name.as_str()).collect();
        renames
            .retain(|(_, _, to)| !kept.contains(to.as_str()) && self.available(&Identifier::Page { name: to.clone() }));
        let mut commands: Vec<Command> = renames
            .iter()
            .map(|(id, _, _)| Command::RenamePage { id: *id, name: format!("~renumbering~{}", id.0) })
            .collect();
        commands.extend(renames.iter().map(|(id, _, name)| Command::RenamePage { id: *id, name: name.clone() }));
        let changes =
            renames.into_iter().map(|(_, from, to)| Change { what: format!("page {from}"), from, to }).collect();
        (changes, commands)
    }
}
