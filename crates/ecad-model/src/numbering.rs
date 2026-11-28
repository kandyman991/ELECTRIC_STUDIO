//! Stable numbering (S0, D22–D25): engineering identifiers are persistent assignments.
//!
//! - New objects get an available identifier without touching existing ones; nothing is ever
//!   renumbered automatically (D22). Renumbering is a separate, explicit, previewed operation
//!   ([`crate::renumber`]).
//! - A **policy** per kind: sequential by default (the next value after the highest ever
//!   allocated, retired ones included), or the lowest free value.
//! - **Bounded resources** (PLC channels on a card, terminals on a strip of known size, cores
//!   of a cable) take the first free slot that isn't used, reserved or retired, and report
//!   exhaustion instead of adding anything.
//! - **Reserved** identifiers and resources are kept free for a planned extension: allocation
//!   skips them and using one needs it unreserved first (an explicit choice).
//! - **Retired** identifiers (D23) are those of removed objects: never allocated again
//!   automatically; using one again needs an explicit [`Command::Reclaim`].
//!
//! Stored in `numbering.json`. One implementation serves the editor, the CLI and MCP: they all
//! allocate through these methods and the commands below.

use crate::project::{CableId, Command, DeviceId, ModelError, PageId, Project, Result};
use crate::symbol::IoKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// An engineering identifier or resource assignment, by what it identifies.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Identifier {
    /// A device tag, `-K10`.
    Tag { tag: String },
    /// A cable tag, `-W3`.
    Cable { tag: String },
    /// A wire number, `W12`, `105`.
    Wire { number: String },
    /// A terminal designation on a device: a strip terminal's number, a PLC channel's.
    Terminal { device: DeviceId, designation: String },
    /// A PLC address in canonical form, `I1.0`.
    Address { address: String },
    /// A core of a cable.
    Core { cable: CableId, core: String },
    /// A page name, `3a`.
    Page { name: String },
}

/// The kinds a numbering policy is set for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Tag,
    Cable,
    Wire,
    Terminal,
    Address,
    Page,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Tag, Kind::Cable, Kind::Wire, Kind::Terminal, Kind::Address, Kind::Page];
    pub fn label(self) -> &'static str {
        match self {
            Kind::Tag => "device tags",
            Kind::Cable => "cable tags",
            Kind::Wire => "wire numbers",
            Kind::Terminal => "terminal numbers",
            Kind::Address => "PLC addresses",
            Kind::Page => "page numbers",
        }
    }
}

/// How the next value of a kind is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    /// After the highest value ever allocated, retired ones included (D22).
    #[default]
    Sequential,
    /// The lowest value that is free, not reserved and not retired.
    LowestFree,
}

/// How a page inserted after another is named (D25): after `3`, `3a` or `3.1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageSuffix {
    #[default]
    Letters,
    Dotted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Policies {
    /// Kinds not listed are sequential.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub strategies: BTreeMap<Kind, Strategy>,
    #[serde(skip_serializing_if = "is_default")]
    pub page_suffix: PageSuffix,
}

impl Policies {
    pub fn strategy(&self, kind: Kind) -> Strategy {
        self.strategies.get(&kind).copied().unwrap_or_default()
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A reserved or retired identifier with a note (why, for what).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(flatten)]
    pub id: Identifier,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Record {
    pub fn new(id: Identifier, note: &str) -> Self {
        Record { id, note: note.to_owned() }
    }
}

/// The project's numbering state (`numbering.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Numbering {
    #[serde(skip_serializing_if = "is_default")]
    pub policies: Policies,
    #[serde(with = "records", skip_serializing_if = "BTreeMap::is_empty")]
    pub reserved: BTreeMap<Identifier, String>,
    #[serde(with = "records", skip_serializing_if = "BTreeMap::is_empty")]
    pub retired: BTreeMap<Identifier, String>,
    /// The number of slots of a bounded device: channels of a PLC card, terminals of a strip.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub capacities: BTreeMap<DeviceId, u32>,
}

impl Numbering {
    pub fn is_empty(&self) -> bool {
        *self == Numbering::default()
    }
    /// Reservations or a capacity that name this device (it can't be removed while they do).
    pub fn names_device(&self, device: DeviceId) -> bool {
        self.capacities.contains_key(&device)
            || self.reserved.keys().any(|id| matches!(id, Identifier::Terminal { device: d, .. } if *d == device))
    }
    pub fn names_cable(&self, cable: CableId) -> bool {
        self.reserved.keys().any(|id| matches!(id, Identifier::Core { cable: c, .. } if *c == cable))
    }
}

/// Reserved and retired lists as JSON arrays of records; a duplicate is refused (fail closed).
mod records {
    use super::{Identifier, Record};
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(m: &BTreeMap<Identifier, String>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter().map(|(id, note)| Record { id: id.clone(), note: note.clone() }))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<Identifier, String>, D::Error> {
        let mut out = BTreeMap::new();
        for r in Vec::<Record>::deserialize(d)? {
            if out.insert(r.id, r.note).is_some() {
                return Err(D::Error::custom("an identifier is listed twice"));
            }
        }
        Ok(out)
    }
}

/// Bijective base 26: 1 → a, 26 → z, 27 → aa.
fn letters(mut n: u64) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn from_letters(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_lowercase()) || s.len() > 6 {
        return None;
    }
    Some(s.bytes().fold(0, |n, b| n * 26 + u64::from(b - b'a' + 1)))
}

/// Linear position of a canonical address within its kind: digital byte·8+bit, analog the word.
fn ordinal(prefix: &str, canonical: &str) -> Option<u64> {
    let rest = canonical.strip_prefix(prefix)?;
    if prefix.ends_with('W') {
        rest.parse().ok()
    } else {
        let (byte, bit) = rest.split_once('.')?;
        Some(byte.parse::<u64>().ok()? * 8 + bit.parse::<u64>().ok().filter(|b| *b < 8)?)
    }
}

fn address_at(prefix: &str, n: u64) -> String {
    if prefix.ends_with('W') { format!("{prefix}{n}") } else { format!("{prefix}{}.{}", n / 8, n % 8) }
}

/// The leading digits of a page name: `3` of `3`, `3a`, `3.1`.
fn page_root(name: &str) -> &str {
    let end = name.find(|c: char| !c.is_ascii_digit()).unwrap_or(name.len());
    &name[..end]
}

impl Project {
    /// Whether an identifier is assigned to something in the project now.
    pub fn in_use(&self, id: &Identifier) -> bool {
        match id {
            Identifier::Tag { tag } => self.device_by_tag(tag).is_some(),
            Identifier::Cable { tag } => self.cables.values().any(|c| &c.tag == tag),
            Identifier::Wire { number } => self.wire_number_in_use(number),
            Identifier::Terminal { device, designation } => {
                self.terminals.values().any(|t| t.device == *device && &t.designation == designation)
            }
            Identifier::Address { address } => self.terminals.values().any(|t| {
                t.channel.as_ref().and_then(|c| crate::plc_address::canonical(&c.address)).as_deref()
                    == Some(address.as_str())
            }),
            Identifier::Core { cable, core } => self
                .connections
                .values()
                .map(|c| &c.spec)
                .chain(self.undrawn.values().map(|w| &w.spec))
                .any(|s| s.cable.as_ref().is_some_and(|k| k.cable == *cable && &k.core == core)),
            Identifier::Page { name } => self.pages.values().any(|p| &p.name == name),
        }
    }

    pub(crate) fn wire_number_in_use(&self, number: &str) -> bool {
        self.connections.values().any(|c| c.wire_number.as_deref() == Some(number))
            || self.undrawn.values().any(|w| w.wire_number.as_deref() == Some(number))
    }

    /// An identifier as people read it: `-K2`, `W5`, `-X1:5`, `I0.4`, `-W1 core 3`, `page 3a`.
    pub fn describe_identifier(&self, id: &Identifier) -> String {
        let tag = |d: &DeviceId| self.device(*d).map(|d| d.tag.clone()).unwrap_or_else(|_| "?".into());
        match id {
            Identifier::Tag { tag } | Identifier::Cable { tag } => tag.clone(),
            Identifier::Wire { number } => format!("wire {number}"),
            Identifier::Terminal { device, designation } => format!("{}:{designation}", tag(device)),
            Identifier::Address { address } => address.clone(),
            Identifier::Core { cable, core } => {
                let c = self.cables.get(cable).map(|c| c.tag.clone()).unwrap_or_else(|| "?".into());
                format!("{c} core {core}")
            }
            Identifier::Page { name } => format!("page {name}"),
        }
    }

    /// Neither reserved nor retired.
    pub fn available(&self, id: &Identifier) -> bool {
        !self.numbering.reserved.contains_key(id) && !self.numbering.retired.contains_key(id)
    }

    /// An explicit assignment may not take a reserved or retired identifier: unreserve or
    /// reclaim it first.
    pub(crate) fn check_available(&self, id: &Identifier) -> Result<()> {
        if self.numbering.reserved.contains_key(id) {
            return Err(ModelError::IdentifierReserved(self.describe_identifier(id)));
        }
        if self.numbering.retired.contains_key(id) {
            return Err(ModelError::IdentifierRetired(self.describe_identifier(id)));
        }
        Ok(())
    }

    /// The identifiers that removal retires, in use now: tags, cable tags, wire numbers, pages.
    /// (Bounded resources are freed, not retired.)
    pub fn retirable(&self) -> BTreeSet<Identifier> {
        let mut out = BTreeSet::new();
        out.extend(self.devices.values().map(|d| Identifier::Tag { tag: d.tag.clone() }));
        out.extend(self.cables.values().map(|c| Identifier::Cable { tag: c.tag.clone() }));
        out.extend(
            self.connections
                .values()
                .filter_map(|c| c.wire_number.clone())
                .chain(self.undrawn.values().filter_map(|w| w.wire_number.clone()))
                .map(|number| Identifier::Wire { number }),
        );
        out.extend(self.pages.values().map(|p| Identifier::Page { name: p.name.clone() }));
        out
    }

    /// A removal as one transaction that retires what it frees (D23): the identifiers no
    /// longer in use afterwards are recorded as retired, and the numbering that names a removed
    /// device or cable (its capacity, its reservations) goes with it. Removal renumbers nothing.
    pub fn removal(&self, cmds: Vec<Command>) -> Result<Vec<Command>> {
        let mut after = self.clone();
        after.underlays.clear();
        after.apply(Command::Batch(cmds.clone()))?;
        let mut out = Vec::new();
        // Device- and cable-scoped numbering first, so the removal itself isn't refused.
        let gone_devices: Vec<DeviceId> =
            self.devices.keys().filter(|d| !after.devices.contains_key(d)).copied().collect();
        let gone_cables: Vec<CableId> = self.cables.keys().filter(|c| !after.cables.contains_key(c)).copied().collect();
        let unreserve: Vec<Identifier> = self
            .numbering
            .reserved
            .keys()
            .filter(|id| match id {
                Identifier::Terminal { device, .. } => gone_devices.contains(device),
                Identifier::Core { cable, .. } => gone_cables.contains(cable),
                _ => false,
            })
            .cloned()
            .collect();
        if !unreserve.is_empty() {
            out.push(Command::Unreserve(unreserve));
        }
        for d in gone_devices.iter().filter(|d| self.numbering.capacities.contains_key(d)) {
            out.push(Command::SetCapacity { device: *d, capacity: None });
        }
        out.extend(cmds);
        let now = after.retirable();
        let retire: Vec<Record> = self
            .retirable()
            .into_iter()
            .filter(|id| !now.contains(id) && !after.numbering.retired.contains_key(id))
            .map(|id| Record::new(id, "removed"))
            .collect();
        if !retire.is_empty() {
            out.push(Command::Retire(retire));
        }
        Ok(out)
    }

    /// The next value of a series `before{n}after` (n zero-padded to `width`) by the kind's
    /// strategy, skipping values in use (`taken`), reserved and retired. `values` are the
    /// series' current values, for the highest one.
    pub(crate) fn next_in_series(
        &self,
        kind: Kind,
        (before, width, after): (&str, usize, &str),
        taken: impl Fn(&str) -> bool,
        values: impl Iterator<Item = String>,
        as_id: impl Fn(String) -> Identifier,
    ) -> String {
        let n_of = |s: &str| -> Option<u64> {
            let digits = s.strip_prefix(before)?.strip_suffix(after)?;
            (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
        };
        let start = match self.numbering.policies.strategy(kind) {
            Strategy::LowestFree => 1,
            Strategy::Sequential => {
                let retired = self.numbering.retired.keys().filter_map(|id| match (kind, id) {
                    (Kind::Tag, Identifier::Tag { tag: v })
                    | (Kind::Cable, Identifier::Cable { tag: v })
                    | (Kind::Wire, Identifier::Wire { number: v })
                    | (Kind::Page, Identifier::Page { name: v }) => Some(v.clone()),
                    _ => None,
                });
                values.chain(retired).filter_map(|v| n_of(&v)).max().map_or(1, |n| n + 1)
            }
        };
        (start..)
            .map(|n| format!("{before}{n:0width$}{after}"))
            .find(|s| !taken(s) && self.available(&as_id(s.clone())))
            .unwrap_or_default()
    }

    /// The tag for a new cable `<prefix><n>` (`-W4`) by the cable policy, skipping used,
    /// reserved and retired tags.
    pub fn new_cable_tag(&self, prefix: &str) -> String {
        self.next_in_series(
            Kind::Cable,
            (prefix, 0, ""),
            |t| self.cables.values().any(|c| c.tag == t),
            self.cables.values().map(|c| c.tag.clone()),
            |tag| Identifier::Cable { tag },
        )
    }

    /// A device's slot range for a resource: PLC channels count from 0, terminals from 1.
    fn slots(&self, device: DeviceId, from: u64) -> Option<std::ops::RangeInclusive<u64>> {
        let cap = u64::from(*self.numbering.capacities.get(&device)?);
        Some(from..=cap + from - 1)
    }

    /// A strip's next terminal number: on a strip of known size the first free, unreserved,
    /// unretired slot (none left: exhaustion); otherwise by the terminal policy.
    pub fn allocate_terminal_number(&self, strip: DeviceId) -> Result<String> {
        let used: BTreeSet<&str> =
            self.terminals.values().filter(|t| t.device == strip).map(|t| t.designation.as_str()).collect();
        let id = |designation: String| Identifier::Terminal { device: strip, designation };
        if let Some(slots) = self.slots(strip, 1) {
            return slots
                .map(|n| n.to_string())
                .find(|n| !used.contains(n.as_str()) && self.available(&id(n.clone())))
                .ok_or_else(|| ModelError::Exhausted(self.exhausted(strip, "terminal")));
        }
        let retired = self.numbering.retired.keys().filter_map(|i| match i {
            Identifier::Terminal { device, designation } if *device == strip => Some(designation.clone()),
            _ => None,
        });
        let values: Vec<String> = used.iter().map(|s| (*s).to_owned()).chain(retired).collect();
        Ok(self.next_in_series(Kind::Terminal, ("", 0, ""), |n| used.contains(n), values.into_iter(), id))
    }

    fn exhausted(&self, device: DeviceId, what: &str) -> String {
        let tag = self.device(device).map(|d| d.tag.clone()).unwrap_or_default();
        format!("{tag} has no free {what} left")
    }

    /// A card's next channel number: the first free, unreserved, unretired slot from 0 (within
    /// its capacity if known; none left: exhaustion).
    pub fn allocate_channel(&self, card: DeviceId) -> Result<String> {
        let used: BTreeSet<&str> = self
            .terminals
            .values()
            .filter(|t| t.device == card && t.channel.is_some())
            .map(|t| t.designation.as_str())
            .collect();
        let free = |n: &String| {
            !used.contains(n.as_str()) && self.available(&Identifier::Terminal { device: card, designation: n.clone() })
        };
        match self.slots(card, 0) {
            Some(slots) => slots
                .map(|n| n.to_string())
                .find(free)
                .ok_or_else(|| ModelError::Exhausted(self.exhausted(card, "channel"))),
            None => Ok((0u64..).map(|n| n.to_string()).find(free).unwrap_or_default()),
        }
    }

    /// A cable's next core: the first of 1…cores not used, reserved or retired. Cores named
    /// otherwise (`BN`, `GNYE`) count against the cable's size too; none left: exhaustion.
    pub fn allocate_core(&self, cable: CableId) -> Result<String> {
        let c = self.cables.get(&cable).ok_or(ModelError::NoCable(cable))?;
        let mut held: BTreeSet<&str> = self
            .connections
            .values()
            .map(|w| &w.spec)
            .chain(self.undrawn.values().map(|w| &w.spec))
            .filter_map(|s| s.cable.as_ref().filter(|k| k.cable == cable).map(|k| k.core.as_str()))
            .collect();
        held.extend(self.numbering.reserved.keys().chain(self.numbering.retired.keys()).filter_map(|id| match id {
            Identifier::Core { cable: k, core } if *k == cable => Some(core.as_str()),
            _ => None,
        }));
        let exhausted = || ModelError::Exhausted(format!("{} has no free core left", c.tag));
        if held.len() >= c.cores as usize {
            return Err(exhausted());
        }
        (1..=u64::from(c.cores)).map(|n| n.to_string()).find(|n| !held.contains(n.as_str())).ok_or_else(exhausted)
    }

    /// The next PLC address of a kind by the address policy, skipping used, reserved and
    /// retired addresses: Siemens-style `I0.0`…`I0.7`, `I1.0` for digital, `IW256`, `IW258`
    /// (words in steps of 2) for analog. Analog has no start of its own: empty until one is
    /// used.
    pub fn next_address(&self, kind: IoKind) -> String {
        let prefix = match kind {
            IoKind::DigitalIn => "I",
            IoKind::DigitalOut => "Q",
            IoKind::AnalogIn => "IW",
            IoKind::AnalogOut => "QW",
        };
        let step = if prefix.ends_with('W') { 2 } else { 1 };
        let used: BTreeSet<u64> = self
            .terminals
            .values()
            .filter_map(|t| ordinal(prefix, &crate::plc_address::canonical(&t.channel.as_ref()?.address)?))
            .collect();
        let retired = self.numbering.retired.keys().filter_map(|id| match id {
            Identifier::Address { address } => ordinal(prefix, address),
            _ => None,
        });
        let start = match (self.numbering.policies.strategy(Kind::Address), step) {
            (Strategy::LowestFree, 1) => Some(0),
            (Strategy::LowestFree, _) => used.iter().min().copied(),
            (Strategy::Sequential, _) => used.iter().copied().chain(retired).max().map(|n| n + step),
        };
        let Some(start) = start.or((step == 1).then_some(0)) else { return String::new() };
        (0..)
            .map(|i| start + i * step)
            .find(|n| !used.contains(n) && self.available(&Identifier::Address { address: address_at(prefix, *n) }))
            .map(|n| address_at(prefix, n))
            .unwrap_or_default()
    }

    /// The name for a page inserted after `after` (D25): appended after the last page, the
    /// next page number; inserted between pages, the previous page's number with the next
    /// suffix (`3a`, `3b`, or `3.1`, `3.2`). Never renames another page.
    pub fn new_page_name(&self, after: Option<PageId>) -> String {
        let order = self.pages_in_order();
        let last = order.last().map(|p| p.id);
        let taken = |n: &str| self.pages.values().any(|p| p.name == n);
        let page_id = |name: String| Identifier::Page { name };
        let names: Vec<String> = self.pages.values().map(|p| p.name.clone()).collect();
        let prev = after.and_then(|a| self.pages.get(&a));
        match prev {
            Some(p) if Some(p.id) != last && !page_root(&p.name).is_empty() => {
                let root = page_root(&p.name);
                let retired: Vec<String> = self
                    .numbering
                    .retired
                    .keys()
                    .filter_map(|id| match id {
                        Identifier::Page { name } => Some(name.clone()),
                        _ => None,
                    })
                    .collect();
                match self.numbering.policies.page_suffix {
                    PageSuffix::Dotted => {
                        self.next_in_series(Kind::Page, (&format!("{root}."), 0, ""), taken, names.into_iter(), page_id)
                    }
                    PageSuffix::Letters => {
                        let suffix = |n: &str| n.strip_prefix(root).and_then(from_letters);
                        let start = match self.numbering.policies.strategy(Kind::Page) {
                            Strategy::LowestFree => 1,
                            Strategy::Sequential => {
                                names.iter().chain(&retired).filter_map(|n| suffix(n)).max().map_or(1, |n| n + 1)
                            }
                        };
                        (start..)
                            .map(|n| format!("{root}{}", letters(n)))
                            .find(|n| !taken(n) && self.available(&page_id(n.clone())))
                            .unwrap_or_default()
                    }
                }
            }
            _ => self.next_in_series(Kind::Page, ("", 0, ""), taken, names.into_iter(), page_id),
        }
    }

    /// The commands that insert `page` after `after` (or at the end): its name by
    /// [`Self::new_page_name`] if empty, and a sort key between its neighbours, so no other
    /// page's key, name or file changes (D25). Only when two neighbours have no key between
    /// them does the order get new keys, as few as [`crate::sparse_keys`] needs.
    pub fn insert_page(&self, after: Option<PageId>, mut page: Page) -> Vec<Command> {
        if page.name.trim().is_empty() {
            page.name = self.new_page_name(after);
        }
        let order: Vec<&Page> = self.pages_in_order();
        let at = after.and_then(|a| order.iter().position(|p| p.id == a)).map_or(order.len(), |i| i + 1);
        let lower = if at == 0 { 0 } else { order[at - 1].order };
        let key = match order.get(at) {
            None => Some(lower.saturating_add(crate::PAGE_STEP)),
            Some(next) if next.order > lower + 1 => Some(lower + (next.order - lower) / 2),
            Some(_) => None,
        };
        match key {
            Some(k) => {
                page.order = k;
                vec![Command::AddPage(page)]
            }
            None => {
                let id = page.id;
                let mut ids: Vec<PageId> = order.iter().map(|p| p.id).collect();
                ids.insert(at, id);
                vec![Command::AddPage(page), Command::ReorderPages(ids)]
            }
        }
    }

    /// Read identifiers as written on the command line, with ranges of up to 1000:
    /// `tag:-K10`, `tag:-K10..-K19` (or `..19`), `cable:-W9`, `wire:W5..W9`,
    /// `terminal:-X3:7..9`, `address:I1.0..I1.3`, `core:-W1:3`, `page:3a`.
    pub fn parse_identifiers(&self, spec: &str) -> std::result::Result<Vec<Identifier>, String> {
        let (kind, rest) = spec.split_once(':').ok_or_else(|| format!("`{spec}`: write kind:value, e.g. tag:-K10"))?;
        let expand = |value: &str| -> std::result::Result<Vec<String>, String> {
            let Some((a, b)) = value.split_once("..") else { return Ok(vec![value.to_owned()]) };
            let split = |s: &str| {
                let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
                (s[..s.len() - digits].to_owned(), s[s.len() - digits..].to_owned())
            };
            let (pa, na) = split(a);
            let (pb, nb) = split(b);
            if na.is_empty() || nb.is_empty() || !(pb.is_empty() || pb == pa) {
                return Err(format!("`{value}`: a range runs between two numbers of one series"));
            }
            let (x, y): (u64, u64) = (na.parse().map_err(|_| "bad number")?, nb.parse().map_err(|_| "bad number")?);
            if y < x || y - x >= 1000 {
                return Err(format!("`{value}`: a range runs upwards, up to 1000 values"));
            }
            Ok((x..=y).map(|n| format!("{pa}{n:0w$}", w = if na.starts_with('0') { na.len() } else { 0 })).collect())
        };
        let device = |t: &str| self.device_by_tag(t).map(|d| d.id).ok_or_else(|| format!("no device {t}"));
        let cable =
            |t: &str| self.cables.values().find(|c| c.tag == t).map(|c| c.id).ok_or_else(|| format!("no cable {t}"));
        let owned = |s: &str| -> std::result::Result<(String, String), String> {
            // `-X3:7`: the owner up to the last colon.
            s.rsplit_once(':')
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .ok_or_else(|| format!("`{s}`: write owner:number"))
        };
        Ok(match kind {
            "tag" => expand(rest)?.into_iter().map(|tag| Identifier::Tag { tag }).collect(),
            "cable" => expand(rest)?.into_iter().map(|tag| Identifier::Cable { tag }).collect(),
            "wire" => expand(rest)?.into_iter().map(|number| Identifier::Wire { number }).collect(),
            "page" => expand(rest)?.into_iter().map(|name| Identifier::Page { name }).collect(),
            "terminal" => {
                let (owner, n) = owned(rest)?;
                let device = device(&owner)?;
                expand(&n)?.into_iter().map(|designation| Identifier::Terminal { device, designation }).collect()
            }
            "core" => {
                let (owner, n) = owned(rest)?;
                let cable = cable(&owner)?;
                expand(&n)?.into_iter().map(|core| Identifier::Core { cable, core }).collect()
            }
            "address" => {
                let canon =
                    |a: &str| crate::plc_address::canonical(a).ok_or_else(|| format!("`{a}` is not a PLC address"));
                match rest.split_once("..") {
                    None => vec![Identifier::Address { address: canon(rest)? }],
                    Some((a, b)) => {
                        let (a, b) = (canon(a)?, canon(b)?);
                        let prefix: String = a.chars().take_while(char::is_ascii_alphabetic).collect();
                        let (Some(x), Some(y)) = (ordinal(&prefix, &a), ordinal(&prefix, &b)) else {
                            return Err(format!("`{rest}`: both ends of one kind"));
                        };
                        let step = if prefix.ends_with('W') { 2 } else { 1 };
                        if y < x || (y - x) / step >= 1000 {
                            return Err(format!("`{rest}`: a range runs upwards, up to 1000 values"));
                        }
                        (x..=y)
                            .step_by(step as usize)
                            .map(|n| Identifier::Address { address: address_at(&prefix, n) })
                            .collect()
                    }
                }
            }
            other => return Err(format!("unknown kind `{other}` (tag, cable, wire, terminal, address, core, page)")),
        })
    }

    /// Whether an identifier names something that exists and fits it, for reserving.
    fn check_identifier(&self, id: &Identifier) -> Result<()> {
        let bad = || ModelError::BadIdentifier(self.describe_identifier(id));
        let blank = |s: &str| s.trim().is_empty() || s.trim() != s;
        match id {
            Identifier::Tag { tag: s }
            | Identifier::Cable { tag: s }
            | Identifier::Wire { number: s }
            | Identifier::Page { name: s } => {
                if blank(s) {
                    return Err(bad());
                }
            }
            Identifier::Terminal { device, designation } => {
                self.device(*device)?;
                if blank(designation) {
                    return Err(bad());
                }
                if let (Some(cap), Ok(n)) = (self.numbering.capacities.get(device), designation.parse::<u64>())
                    && n > u64::from(*cap)
                {
                    return Err(bad());
                }
            }
            Identifier::Address { address } => {
                if crate::plc_address::canonical(address).as_deref() != Some(address.as_str()) {
                    return Err(bad());
                }
            }
            Identifier::Core { cable, core } => {
                let c = self.cables.get(cable).ok_or(ModelError::NoCable(*cable))?;
                if blank(core) || core.parse::<u64>().is_ok_and(|n| n == 0 || n > u64::from(c.cores)) {
                    return Err(bad());
                }
            }
        }
        Ok(())
    }

    /// The numbering commands, applied by [`Project::apply`].
    pub(crate) fn apply_numbering(&mut self, cmd: Command) -> Result<Command> {
        match cmd {
            Command::Reserve(records) => {
                let mut seen = BTreeSet::new();
                for r in &records {
                    self.check_identifier(&r.id)?;
                    if !seen.insert(&r.id) || self.numbering.reserved.contains_key(&r.id) {
                        return Err(ModelError::AlreadyReserved(self.describe_identifier(&r.id)));
                    }
                    if self.numbering.retired.contains_key(&r.id) {
                        return Err(ModelError::IdentifierRetired(self.describe_identifier(&r.id)));
                    }
                    if self.in_use(&r.id) {
                        return Err(ModelError::IdentifierInUse(self.describe_identifier(&r.id)));
                    }
                }
                let ids = records.iter().map(|r| r.id.clone()).collect();
                self.numbering.reserved.extend(records.into_iter().map(|r| (r.id, r.note)));
                Ok(Command::Unreserve(ids))
            }
            Command::Unreserve(ids) => {
                if let Some(id) = ids.iter().find(|id| !self.numbering.reserved.contains_key(*id)) {
                    return Err(ModelError::NotReserved(self.describe_identifier(id)));
                }
                if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                    return Err(ModelError::BadIdentifier("listed twice".into()));
                }
                let records = ids
                    .into_iter()
                    .filter_map(|id| self.numbering.reserved.remove(&id).map(|note| Record { id, note }))
                    .collect();
                Ok(Command::Reserve(records))
            }
            Command::Retire(records) => {
                let mut seen = BTreeSet::new();
                for r in &records {
                    if !seen.insert(&r.id) || self.numbering.retired.contains_key(&r.id) {
                        return Err(ModelError::AlreadyRetired(self.describe_identifier(&r.id)));
                    }
                    if self.numbering.reserved.contains_key(&r.id) {
                        return Err(ModelError::IdentifierReserved(self.describe_identifier(&r.id)));
                    }
                    if self.in_use(&r.id) {
                        return Err(ModelError::IdentifierInUse(self.describe_identifier(&r.id)));
                    }
                    if matches!(&r.id, Identifier::Address { address } if crate::plc_address::canonical(address).as_deref() != Some(address.as_str()))
                    {
                        return Err(ModelError::BadIdentifier(self.describe_identifier(&r.id)));
                    }
                }
                let ids = records.iter().map(|r| r.id.clone()).collect();
                self.numbering.retired.extend(records.into_iter().map(|r| (r.id, r.note)));
                Ok(Command::Reclaim(ids))
            }
            Command::Reclaim(ids) => {
                if let Some(id) = ids.iter().find(|id| !self.numbering.retired.contains_key(*id)) {
                    return Err(ModelError::NotRetired(self.describe_identifier(id)));
                }
                if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                    return Err(ModelError::BadIdentifier("listed twice".into()));
                }
                let records = ids
                    .into_iter()
                    .filter_map(|id| self.numbering.retired.remove(&id).map(|note| Record { id, note }))
                    .collect();
                Ok(Command::Retire(records))
            }
            Command::SetNumberingPolicies(policies) => {
                Ok(Command::SetNumberingPolicies(std::mem::replace(&mut self.numbering.policies, policies)))
            }
            Command::SetCapacity { device, capacity } => {
                let tag = self.device(device)?.tag.clone();
                if let Some(cap) = capacity {
                    let highest = self
                        .terminals
                        .values()
                        .filter(|t| t.device == device)
                        .filter_map(|t| {
                            t.designation.parse::<u64>().ok().map(|n| if t.channel.is_some() { n + 1 } else { n })
                        })
                        .chain(self.numbering.reserved.keys().filter_map(|id| match id {
                            Identifier::Terminal { device: d, designation } if *d == device => designation.parse().ok(),
                            _ => None,
                        }))
                        .max()
                        .unwrap_or(0);
                    if cap == 0 || highest > u64::from(cap) {
                        return Err(ModelError::BadCapacity(tag));
                    }
                }
                let old = match capacity {
                    Some(c) => self.numbering.capacities.insert(device, c),
                    None => self.numbering.capacities.remove(&device),
                };
                Ok(Command::SetCapacity { device, capacity: old })
            }
            _ => Err(ModelError::BadIdentifier("not a numbering command".into())),
        }
    }
}

use crate::project::Page;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes_and_addresses_count_as_expected() {
        assert_eq!((letters(1), letters(26), letters(27)), ("a".into(), "z".into(), "aa".into()));
        assert_eq!(from_letters("aa"), Some(27));
        assert_eq!(from_letters("A"), None);
        assert_eq!(page_root("12b"), "12");
        assert_eq!(ordinal("I", "I1.3"), Some(11));
        assert_eq!(address_at("I", 11), "I1.3");
        assert_eq!(address_at("IW", 258), "IW258");
        assert_eq!(ordinal("I", "IW256"), None);
    }

    #[test]
    fn records_round_trip_and_refuse_duplicates() {
        let mut n = Numbering::default();
        n.reserved.insert(Identifier::Tag { tag: "-K10".into() }, "future section".into());
        n.retired.insert(Identifier::Address { address: "I0.4".into() }, String::new());
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(serde_json::from_str::<Numbering>(&json).unwrap(), n);
        let dup = r#"{"reserved":[{"kind":"tag","tag":"-K1"},{"kind":"tag","tag":"-K1"}]}"#;
        assert!(serde_json::from_str::<Numbering>(dup).is_err());
        assert_eq!(serde_json::to_string(&Numbering::default()).unwrap(), "{}");
    }
}
