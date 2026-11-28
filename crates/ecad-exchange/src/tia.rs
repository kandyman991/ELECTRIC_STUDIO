//! The TIA Portal PLC tag table: an Excel file with a `PLC Tags` sheet, one tag per row.
//!
//! Export: every PLC channel with a usable address becomes a tag. Its name is the channel's
//! symbol, or for a channel without one a name from the card and channel (`KF1_DI3`, made
//! unique with `_2`, `_3`…); digital channels are `Bool`, analog ones `Int`; addresses are
//! written canonically with TIA Portal's `%` (`%I0.3`, `%IW256`). Columns follow TIA Portal's
//! own English export, so the file imports with *PLC tags > Import*. Tags are visible and
//! accessible from HMI but not writeable: an HMI writing to a physical input or output tag
//! is a decision for the PLC programmer, not a default.
//!
//! A channel is left out, and listed, when it has no address, an address that isn't an input
//! or output of its kind (a DI on `IW500`), or the same address as another channel however
//! written (`I0.0` and `%I00.0`): TIA Portal would get two tags at one address.
//!
//! Import: a table exported from TIA Portal (English or German column titles) is matched to
//! the channels by canonical address. Each matched channel takes the tag's name as its symbol
//! and the tag's comment, all in one undoable step; empty cells leave the channel's value
//! alone. Nothing changes if any channel's address is ambiguous. Listed, not applied: tags at
//! addresses no channel has, tags whose address doesn't fit the channel's kind, and addresses
//! the table gives twice with different names or comments. A tag named exactly as the export
//! names a channel without a symbol leaves it without one, so a genuine rename in TIA Portal to
//! exactly that name can't be told apart and is not applied either.

use crate::{ExchangeError, Result};
use calamine::{Data, Reader, Xlsx};
use ecad_model::plc_address::{canonical, fits};
use ecad_model::{COMMENT, ChannelRef, Command, FunctionId, IoKind, Project, SYMBOL};
use std::collections::{BTreeMap, BTreeSet};

/// The sheet TIA Portal reads and writes.
pub const SHEET: &str = "PLC Tags";
/// The tag table new tags go into; every PLC has it.
pub const TAG_TABLE: &str = "Default tag table";
/// The file name in an export folder.
pub const FILE_NAME: &str = "tia-plc-tags.xlsx";

/// Largest tag table file read, bytes.
pub const MAX_FILE: usize = 16 << 20;
/// Largest total unpacked size of a tag table file's parts, bytes.
pub const MAX_UNPACKED: u64 = 128 << 20;
/// Most rows read from a tag table.
pub const MAX_ROWS: usize = 200_000;

const COLUMNS: [&str; 10] = [
    "Name",
    "Path",
    "Data Type",
    "Logical Address",
    "Comment",
    "Hmi Visible",
    "Hmi Accessible",
    "Hmi Writeable",
    "Typeobject ID",
    "Version ID",
];

/// One PLC tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The tag table.
    pub path: String,
    /// `Bool` or `Int`.
    pub data_type: String,
    /// With TIA Portal's `%`: `%I0.3`.
    pub address: String,
    pub comment: String,
    pub hmi_visible: bool,
    pub hmi_accessible: bool,
    pub hmi_writeable: bool,
}

/// A channel left out of the export, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// `-KF1:3`.
    pub channel: String,
    pub reason: String,
}

/// A PLC channel with its card's tag and its address in canonical form.
struct Channel {
    card: String,
    channel: ChannelRef,
    address: Option<String>,
}

impl Channel {
    fn at(&self) -> String {
        format!("{}:{}", self.card, self.channel.channel)
    }
}

/// Every PLC channel, card by card (cards by tag), with its canonical address.
fn channels(project: &Project) -> Vec<Channel> {
    let mut cards: Vec<(String, Vec<ChannelRef>)> = project
        .devices
        .values()
        .map(|d| (d.tag.clone(), project.plc_channels_of(d.id)))
        .filter(|(_, channels)| !channels.is_empty())
        .collect();
    cards.sort_by(|a, b| a.0.cmp(&b.0));
    cards
        .into_iter()
        .flat_map(|(card, cs)| {
            cs.into_iter().map(move |c| Channel { card: card.clone(), address: canonical(&c.address), channel: c })
        })
        .collect()
}

/// Canonical addresses more than one channel has, with those channels (`-KF1:0`).
fn shared_addresses(channels: &[Channel]) -> BTreeMap<String, Vec<String>> {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in channels {
        if let Some(a) = &c.address {
            by.entry(a.clone()).or_default().push(c.at());
        }
    }
    by.retain(|_, v| v.len() > 1);
    by
}

/// The name a channel without a symbol gets from its card and channel: letters, digits and
/// `_` only (`-KF1`, DI 3 → `KF1_DI3`).
fn placeholder(c: &Channel) -> String {
    let card: String = c.card.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect();
    format!("{}_{}{}", card.trim_matches('_'), c.channel.kind.short(), c.channel.channel)
}

/// The names the export gives channels without a symbol, by function: the placeholder, with
/// `_2`, `_3`… if a symbol or an earlier made-up name has it.
fn made_up_names(channels: &[Channel]) -> BTreeMap<FunctionId, String> {
    let symbols: BTreeSet<&str> = channels.iter().map(|c| c.channel.symbol.as_str()).collect();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = BTreeMap::new();
    for c in channels.iter().filter(|c| c.channel.symbol.is_empty()) {
        let base = placeholder(c);
        let mut name = base.clone();
        let mut n = 2;
        while symbols.contains(name.as_str()) || taken.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        taken.insert(name.clone());
        out.insert(c.channel.function, name);
    }
    out
}

/// The tags for TIA Portal, card by card, and the channels that can't be tags.
pub fn tags(project: &Project) -> (Vec<Tag>, Vec<Skipped>) {
    let channels = channels(project);
    let shared = shared_addresses(&channels);
    let made_up = made_up_names(&channels);
    let mut tags = Vec::new();
    let mut skipped = Vec::new();
    for c in &channels {
        let reason = match &c.address {
            _ if c.channel.address.trim().is_empty() => Some("no address".to_owned()),
            None => Some(format!("address {} isn't an input or output", c.channel.address)),
            Some(a) if !fits(c.channel.kind, a) => {
                Some(format!("address {} doesn't fit a {} channel", c.channel.address, c.channel.kind.short()))
            }
            Some(a) => shared.get(a).map(|on| format!("address {a} is also on {}", others(on, &c.at()))),
        };
        if let Some(reason) = reason {
            skipped.push(Skipped { channel: c.at(), reason });
            continue;
        }
        let (Some(address), name) = (&c.address, made_up.get(&c.channel.function).unwrap_or(&c.channel.symbol)) else {
            continue;
        };
        let analog = matches!(c.channel.kind, IoKind::AnalogIn | IoKind::AnalogOut);
        tags.push(Tag {
            name: name.clone(),
            path: TAG_TABLE.into(),
            data_type: if analog { "Int" } else { "Bool" }.into(),
            address: format!("%{address}"),
            comment: c.channel.comment.clone(),
            hmi_visible: true,
            hmi_accessible: true,
            hmi_writeable: false,
        });
    }
    (tags, skipped)
}

/// The channels in `on` besides `me`, comma separated.
fn others(on: &[String], me: &str) -> String {
    on.iter().filter(|c| c.as_str() != me).cloned().collect::<Vec<_>>().join(", ")
}

/// The tag table as an Excel file for TIA Portal. With no tags it is the header alone, so a
/// file written over an older one never keeps tags that are gone.
pub fn to_xlsx(tags: &[Tag]) -> Result<Vec<u8>> {
    let err = |e: rust_xlsxwriter::XlsxError| ExchangeError::Write(e.to_string());
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name(SHEET).map_err(err)?;
    for (col, title) in (0u16..).zip(COLUMNS) {
        sheet.write_string(0, col, title).map_err(err)?;
    }
    let flag = |b: bool| if b { "True" } else { "False" };
    for (row, tag) in (1u32..).zip(tags) {
        let cells = [
            tag.name.as_str(),
            &tag.path,
            &tag.data_type,
            &tag.address,
            &tag.comment,
            flag(tag.hmi_visible),
            flag(tag.hmi_accessible),
            flag(tag.hmi_writeable),
        ];
        for (col, value) in (0u16..).zip(cells) {
            sheet.write_string(row, col, value).map_err(err)?;
        }
    }
    sheet.autofit();
    book.save_to_buffer().map_err(err)
}

/// What [`export_to`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    /// Tags written.
    pub tags: usize,
    /// Channels left out, and why.
    pub skipped: Vec<Skipped>,
    /// The file, if one was written.
    pub file: Option<std::path::PathBuf>,
    /// A tag table from an earlier export was removed: the project has no PLC channels now.
    pub removed: bool,
}

/// Write the tag table into an export folder. With PLC channels the file is always written,
/// with only the header if none can be a tag, so an older file never stays behind with tags
/// that are gone; without any, an older file is removed.
pub fn export_to(project: &Project, dir: &std::path::Path) -> Result<Export> {
    let path = dir.join(FILE_NAME);
    let (tags, skipped) = tags(project);
    let io = |e: std::io::Error| ExchangeError::Write(format!("{}: {e}", path.display()));
    if tags.is_empty() && skipped.is_empty() {
        let removed = match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io(e)),
        };
        return Ok(Export { tags: 0, skipped, file: None, removed });
    }
    std::fs::write(&path, to_xlsx(&tags)?).map_err(io)?;
    Ok(Export { tags: tags.len(), skipped, file: Some(path), removed: false })
}

/// A tag table as read: its tags and the rows left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub tags: Vec<Tag>,
    /// Rows without a name or without an address (`row 7: no address`).
    pub ignored: Vec<String>,
}

/// Refuse files too big to be a tag table, before unpacking anything.
fn check_size(bytes: &[u8]) -> Result<()> {
    check_size_within(bytes, MAX_FILE, MAX_UNPACKED)
}

fn check_size_within(bytes: &[u8], max_file: usize, max_unpacked: u64) -> Result<()> {
    if bytes.len() > max_file {
        return Err(ExchangeError::Read(format!("the file is over {} MB", max_file >> 20)));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ExchangeError::Read(e.to_string()))?;
    let mut unpacked = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|e| ExchangeError::Read(e.to_string()))?;
        unpacked = unpacked.saturating_add(entry.size());
    }
    if unpacked > max_unpacked {
        return Err(ExchangeError::Read(format!("the file unpacks to over {} MB", max_unpacked >> 20)));
    }
    Ok(())
}

/// The tags of a TIA Portal tag table (`PLC Tags` or `PLC-Variablen` sheet, or the first
/// sheet), by their English or German column titles.
pub fn read_xlsx(bytes: &[u8]) -> Result<Table> {
    check_size(bytes)?;
    let err = |e: calamine::XlsxError| ExchangeError::Read(e.to_string());
    let mut book: Xlsx<_> = Xlsx::new(std::io::Cursor::new(bytes)).map_err(err)?;
    let names = book.sheet_names();
    let sheet = names
        .iter()
        .find(|n| n.as_str() == SHEET || n.as_str() == "PLC-Variablen")
        .or(names.first())
        .cloned()
        .ok_or_else(|| ExchangeError::Read("no sheets".into()))?;
    let range = book.worksheet_range(&sheet).map_err(err)?;
    if range.height() > MAX_ROWS + 1 {
        return Err(ExchangeError::Read(format!("over {MAX_ROWS} rows")));
    }
    let mut rows = range.rows();
    let header: Vec<String> =
        rows.next().map(|r| r.iter().map(|c| c.to_string().trim().to_lowercase()).collect()).unwrap_or_default();
    let column = |titles: &[&str], what: &'static str| {
        header.iter().position(|h| titles.contains(&h.as_str())).ok_or(ExchangeError::MissingColumn(what))
    };
    let name = column(&["name"], "Name")?;
    let address = column(&["logical address", "logische adresse", "address", "adresse"], "Logical Address")?;
    let optional = |titles: &[&str], what| column(titles, what).ok();
    let comment = optional(&["comment", "kommentar"], "Comment");
    let path = optional(&["path", "pfad"], "Path");
    let data_type = optional(&["data type", "datentyp"], "Data Type");
    let visible = optional(&["hmi visible", "von hmi sichtbar"], "Hmi Visible");
    let accessible = optional(&["hmi accessible", "von hmi erreichbar"], "Hmi Accessible");
    let writeable = optional(&["hmi writeable", "von hmi beschreibbar"], "Hmi Writeable");
    let cell = |row: &[Data], i: Option<usize>| {
        i.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_owned()).unwrap_or_default()
    };
    let flag = |row: &[Data], i: Option<usize>| {
        let v = cell(row, i).to_lowercase();
        v == "true" || v == "wahr"
    };
    let mut table = Table::default();
    // Row numbers as Excel shows them: the header is row 1.
    for (n, row) in (2usize..).zip(rows) {
        let tag = Tag {
            name: cell(row, Some(name)),
            path: cell(row, path),
            data_type: cell(row, data_type),
            address: cell(row, Some(address)),
            comment: cell(row, comment),
            hmi_visible: flag(row, visible),
            hmi_accessible: flag(row, accessible),
            hmi_writeable: flag(row, writeable),
        };
        match (tag.name.is_empty(), tag.address.is_empty()) {
            (true, true) => {}
            (true, false) => table.ignored.push(format!("row {n}: no name ({})", tag.address)),
            (false, true) => table.ignored.push(format!("row {n}: no address ({})", tag.name)),
            (false, false) => table.tags.push(tag),
        }
    }
    Ok(table)
}

type Properties = BTreeMap<String, String>;

/// What an import changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Import {
    /// One step setting the matched channels' symbols and comments; `None` if nothing changes.
    pub command: Option<Command>,
    /// Channels whose symbol or comment changes.
    pub updated: usize,
    /// Channels found that already match.
    pub unchanged: usize,
    /// Tags at input or output addresses no channel has (`%I4.0 Spare_In`).
    pub unknown: Vec<String>,
    /// Tags at a channel's address that isn't that channel's kind
    /// (`%IW500 Level: -KF1:1 is a DI channel`).
    pub mismatched: Vec<String>,
    /// Tags that aren't inputs or outputs (memory, data blocks): not for the schematic.
    pub other: usize,
    /// Rows repeating an earlier row's address, name and comment: the same tag twice.
    pub duplicates: Vec<String>,
    /// Addresses the table gives more than once with different names or comments: none of
    /// those rows is applied (`%I0.0: Start, Start_Button`).
    pub conflicts: Vec<String>,
}

/// Match a TIA Portal tag table to the project's PLC channels by address.
///
/// Fails, changing nothing, if two channels share an address however written: which one a tag
/// belongs to can't be told.
pub fn import(project: &Project, tags: &[Tag]) -> Result<Import> {
    let channels = channels(project);
    let shared = shared_addresses(&channels);
    if !shared.is_empty() {
        let list = shared.iter().map(|(a, on)| format!("{a} on {}", on.join(", "))).collect();
        return Err(ExchangeError::SharedAddresses(list));
    }
    let made_up = made_up_names(&channels);
    let by_address: BTreeMap<&str, &Channel> =
        channels.iter().filter_map(|c| c.address.as_deref().map(|a| (a, c))).collect();
    let mut out = Import::default();
    // Every row by address first, so a conflict anywhere in the table is seen before any row
    // at that address is used.
    let mut rows: BTreeMap<String, Vec<&Tag>> = BTreeMap::new();
    for tag in tags {
        match canonical(&tag.address) {
            Some(a) => rows.entry(a).or_default().push(tag),
            None => out.other += 1,
        }
    }
    let mut changes: Vec<(FunctionId, Properties, Properties)> = Vec::new();
    for (address, rows) in &rows {
        let tag = rows[0];
        let differs = |t: &&Tag| t.name != tag.name || t.comment != tag.comment;
        if rows.iter().any(differs) {
            let names: Vec<&str> = rows.iter().map(|t| t.name.as_str()).collect();
            out.conflicts.push(format!("%{address}: {}", names.join(", ")));
            continue;
        }
        out.duplicates.extend(rows[1..].iter().map(|t| format!("{} {}", t.address, t.name)));
        let Some(c) = by_address.get(address.as_str()) else {
            out.unknown.push(format!("{} {}", tag.address, tag.name));
            continue;
        };
        if !fits(c.channel.kind, address) {
            out.mismatched.push(format!(
                "{} {}: {} is a {} channel",
                tag.address,
                tag.name,
                c.at(),
                c.channel.kind.short()
            ));
            continue;
        }
        // The export's own made-up name coming back isn't a symbol.
        let made_up_back = made_up.get(&c.channel.function) == Some(&tag.name);
        let old = project.properties_of(project.function(c.channel.function)?);
        let mut new = old.clone();
        if !made_up_back {
            new.insert(SYMBOL.into(), tag.name.clone());
        }
        if !tag.comment.is_empty() {
            new.insert(COMMENT.into(), tag.comment.clone());
        }
        if new == old {
            out.unchanged += 1;
        } else {
            changes.push((c.channel.function, old, new));
        }
    }
    out.updated = changes.len();
    if !changes.is_empty() {
        // Symbols are unique, and an import may swap two of them: first take the changed
        // channels' symbols away, then give each its new one.
        let mut steps: Vec<Command> = changes
            .iter()
            .map(|(id, old, _)| {
                let mut cleared = old.clone();
                cleared.remove(SYMBOL);
                Command::SetProperties { id: *id, properties: cleared }
            })
            .collect();
        steps.extend(changes.into_iter().map(|(id, _, new)| Command::SetProperties { id, properties: new }));
        // Checked here, so a clash with a channel the table doesn't mention is an error now.
        project.clone().apply(Command::Batch(steps.clone()))?;
        out.command = Some(Command::Batch(steps));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::sample::with_plc;
    use ecad_model::{ADDRESS, Function};

    fn plc_channels(p: &Project) -> Vec<ChannelRef> {
        let kf1 = p.devices.values().find(|d| d.tag == "-KF1").unwrap().id;
        p.plc_channels_of(kf1)
    }

    /// Set a channel's properties, past the model's checks (as a hand-edited or older project
    /// file could have them).
    fn force(p: &mut Project, f: FunctionId, props: &[(&str, &str)]) {
        // A channel's address, symbol and comment are on its terminal (M10).
        let terminals: Vec<_> = p.function(f).unwrap().terminals.values().copied().collect();
        let channel = terminals.iter().find_map(|t| p.terminals.get(t).and_then(|t| t.channel.as_ref()).map(|_| *t));
        for (k, v) in props {
            match (channel, *k) {
                (Some(t), "address") => {
                    p.terminals.get_mut(&t).unwrap().channel.as_mut().unwrap().address = (*v).into()
                }
                (Some(t), "symbol") => p.terminals.get_mut(&t).unwrap().channel.as_mut().unwrap().symbol = (*v).into(),
                (Some(t), "comment") => {
                    p.terminals.get_mut(&t).unwrap().channel.as_mut().unwrap().comment = (*v).into()
                }
                _ => {
                    let func: &mut Function = p.functions.get_mut(&f).unwrap();
                    func.properties.insert((*k).into(), (*v).into());
                }
            }
        }
    }

    fn set(p: &mut Project, f: FunctionId, props: &[(&str, &str)]) {
        let mut properties = p.properties_of(p.function(f).unwrap());
        for (k, v) in props {
            if v.is_empty() {
                properties.remove(*k);
            } else {
                properties.insert((*k).into(), (*v).into());
            }
        }
        p.apply(Command::SetProperties { id: f, properties }).unwrap();
    }

    fn tag(name: &str, address: &str) -> Tag {
        Tag {
            name: name.into(),
            path: TAG_TABLE.into(),
            data_type: "Bool".into(),
            address: address.into(),
            comment: String::new(),
            hmi_visible: true,
            hmi_accessible: true,
            hmi_writeable: false,
        }
    }

    fn import_of(p: &Project, tags: &[Tag]) -> Command {
        import(p, tags).unwrap().command.unwrap()
    }

    #[test]
    fn the_sample_channels_become_tia_tags_not_writeable_from_hmi() {
        let p = with_plc().unwrap();
        let (tags, skipped) = tags(&p);
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(tags[0].address, "%I0.0");
        for t in &tags {
            assert!(t.address.starts_with('%') && t.path == TAG_TABLE, "{t:?}");
            assert!(t.data_type == "Bool" || t.data_type == "Int");
            assert!(t.hmi_visible && t.hmi_accessible && !t.hmi_writeable);
        }
        let names: BTreeSet<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), tags.len(), "names are unique");
    }

    #[test]
    fn the_excel_file_reads_back_and_imports_as_no_change() {
        let p = with_plc().unwrap();
        let (tags, _) = tags(&p);
        let bytes = to_xlsx(&tags).unwrap();
        assert_eq!(&bytes[..2], b"PK", "an xlsx is a zip");
        let back = read_xlsx(&bytes).unwrap();
        assert_eq!(back.tags, tags);
        assert!(back.ignored.is_empty());
        let import = import(&p, &back.tags).unwrap();
        assert_eq!((import.updated, import.command.is_none()), (0, true), "{import:?}");
        assert_eq!(import.unchanged, tags.len());
        // No tags: the header alone.
        let empty = read_xlsx(&to_xlsx(&[]).unwrap()).unwrap();
        assert_eq!(empty, Table::default());
    }

    #[test]
    fn symbols_and_comments_renamed_in_tia_come_back_by_address() {
        let mut p = with_plc().unwrap();
        let (mut tags, _) = tags(&p);
        let (first, second) = (tags[0].clone(), tags[1].clone());
        // Swap two names (symbols are unique, so this needs the two-step import), add a
        // comment, write an address differently, and add tags the schematic doesn't have.
        tags[0].name = second.name.clone();
        tags[0].address = "%i 00.0".into();
        tags[1].name = first.name.clone();
        tags[1].comment = "Renamed in TIA".into();
        tags.push(tag("Spare", "%I9.0"));
        tags.push(tag("Memory", "%M10.0"));
        tags.push(tags[1].clone());
        let import = import(&p, &tags).unwrap();
        assert_eq!(import.updated, 2, "{import:?}");
        assert_eq!(import.unknown, ["%I9.0 Spare"]);
        assert_eq!(import.other, 1);
        assert_eq!(import.duplicates, [format!("{} {}", second.address, first.name)]);
        assert!(import.conflicts.is_empty() && import.mismatched.is_empty());
        p.apply(import.command.unwrap()).unwrap();
        let (after, _) = super::tags(&p);
        let at = |a: &str| after.iter().find(|t| t.address == a).unwrap().clone();
        assert_eq!(at(&first.address).name, second.name);
        assert_eq!(at(&second.address).name, first.name);
        assert_eq!(at(&second.address).comment, "Renamed in TIA");
        // An empty comment in TIA leaves ours.
        assert_eq!(at(&first.address).comment, first.comment);
    }

    #[test]
    fn rows_giving_one_address_different_names_are_not_applied() {
        let p = with_plc().unwrap();
        let (tags, _) = tags(&p);
        let mut table = tags.clone();
        table[0].name = "Renamed".into();
        table.push(Tag { name: "Other_Name".into(), address: "I00.0".into(), ..tags[0].clone() });
        let import = import(&p, &table).unwrap();
        assert_eq!(import.conflicts, ["%I0.0: Renamed, Other_Name"]);
        assert_eq!(import.unchanged, tags.len() - 1);
        assert!(import.command.is_none());
    }

    #[test]
    fn one_address_on_two_channels_stops_export_of_both_and_any_import() {
        let mut p = with_plc().unwrap();
        let cs = plc_channels(&p);
        force(&mut p, cs[1].function, &[(ADDRESS, "%I00.0")]);
        let (tags, skipped) = tags(&p);
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped.iter().all(|s| s.reason.starts_with("address I0.0 is also on -KF1:")), "{skipped:?}");
        assert!(tags.iter().all(|t| t.address != "%I0.0"));
        let before = p.clone();
        let e = import(&p, &[tag("Renamed", "%I0.1")]).unwrap_err();
        let expected = format!("two channels share an address: I0.0 on -KF1:{}, -KF1:{}", cs[0].channel, cs[1].channel);
        assert_eq!(e.to_string(), expected);
        assert_eq!(p, before);
    }

    #[test]
    fn a_tag_at_a_channel_of_another_kind_is_listed_not_applied() {
        let mut p = with_plc().unwrap();
        let cs = plc_channels(&p);
        let di = cs.iter().find(|c| c.kind == IoKind::DigitalIn).unwrap();
        let ai = cs.iter().find(|c| c.kind == IoKind::AnalogIn).unwrap();
        // A DI on IW500 and an AI on I50.0: neither goes out…
        force(&mut p, di.function, &[(ADDRESS, "IW500")]);
        force(&mut p, ai.function, &[(ADDRESS, "I50.0")]);
        let (_, skipped) = tags(&p);
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped.iter().all(|s| s.reason.contains("doesn't fit")));
        // …and tags at those addresses don't come in.
        let before = p.clone();
        let import = import(&p, &[tag("Level", "%IW500"), tag("Switch", "%I50.0")]).unwrap();
        assert_eq!(
            import.mismatched,
            [
                format!("%I50.0 Switch: -KF1:{} is a AI channel", ai.channel),
                format!("%IW500 Level: -KF1:{} is a DI channel", di.channel)
            ]
        );
        assert!(import.command.is_none());
        assert_eq!(p, before);
    }

    #[test]
    fn made_up_names_come_back_unset_but_renames_sharing_their_start_apply() {
        let mut p = with_plc().unwrap();
        let cs = plc_channels(&p);
        let (a, b) = (cs[0].clone(), cs[1].clone());
        set(&mut p, a.function, &[(SYMBOL, "")]);
        // A real symbol taking `a`'s made-up name pushes it to `_2`.
        let base = format!("KF1_{}{}", a.kind.short(), a.channel);
        set(&mut p, b.function, &[(SYMBOL, &base)]);
        let (tags, _) = tags(&p);
        let name_at = |addr: &str| tags.iter().find(|t| t.address == format!("%{addr}")).unwrap().name.clone();
        assert_eq!(name_at(&a.address), format!("{base}_2"));
        assert_eq!(name_at(&b.address), base);
        // The made-up name coming back leaves `a` without a symbol.
        let import = import(&p, &tags).unwrap();
        assert!(import.command.is_none(), "{import:?}");
        // A rename that only starts like it is a real symbol.
        let renamed = format!("{base}_Start");
        let mut table = tags.clone();
        table.iter_mut().find(|t| t.name == format!("{base}_2")).unwrap().name = renamed.clone();
        p.apply(import_of(&p, &table)).unwrap();
        assert_eq!(p.property(p.function(a.function).unwrap(), SYMBOL), renamed);
    }

    #[test]
    fn channels_without_a_usable_address_are_skipped() {
        let mut p = with_plc().unwrap();
        let cs = plc_channels(&p);
        set(&mut p, cs[0].function, &[(ADDRESS, "")]);
        force(&mut p, cs[1].function, &[(ADDRESS, "MW10")]);
        let (_, skipped) = tags(&p);
        assert_eq!(skipped[0], Skipped { channel: format!("-KF1:{}", cs[0].channel), reason: "no address".into() });
        assert_eq!(skipped[1].reason, "address MW10 isn't an input or output");
    }

    #[test]
    fn a_german_table_reads_with_its_flags_and_rows_without_name_or_address_are_listed() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name("PLC-Variablen").unwrap();
        let header = ["Name", "Pfad", "Datentyp", "Logische Adresse", "Kommentar", "Von HMI beschreibbar"];
        let rows = [
            ["Start", "Standard-Variablentabelle", "Bool", "%I0.0", "Taster", "Wahr"],
            ["", "", "Bool", "%I0.1", "", ""],
            ["Lonely", "", "Bool", "", "", ""],
        ];
        for (r, row) in (0u32..).zip(std::iter::once(header).chain(rows)) {
            for (col, value) in (0u16..).zip(row) {
                sheet.write_string(r, col, value).unwrap();
            }
        }
        let table = read_xlsx(&book.save_to_buffer().unwrap()).unwrap();
        assert_eq!(table.tags.len(), 1);
        let t = &table.tags[0];
        assert_eq!((t.name.as_str(), t.address.as_str(), t.comment.as_str()), ("Start", "%I0.0", "Taster"));
        assert!(t.hmi_writeable);
        assert_eq!(table.ignored, ["row 3: no name (%I0.1)", "row 4: no address (Lonely)"]);
    }

    #[test]
    fn exporting_again_replaces_or_removes_the_older_table() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = with_plc().unwrap();
        let first = export_to(&p, dir.path()).unwrap();
        assert!(first.tags > 0 && first.file.is_some());
        // No usable addresses any more: the file is the header alone, and says why.
        for c in plc_channels(&p) {
            set(&mut p, c.function, &[(ADDRESS, "")]);
        }
        let second = export_to(&p, dir.path()).unwrap();
        assert_eq!((second.tags, second.skipped.len()), (0, plc_channels(&p).len()));
        assert!(read_xlsx(&std::fs::read(dir.path().join(FILE_NAME)).unwrap()).unwrap().tags.is_empty());
        // No PLC at all: the older file goes.
        let third = export_to(&ecad_model::sample::start_circuit().unwrap(), dir.path()).unwrap();
        assert!(third.removed && third.file.is_none());
        assert!(!dir.path().join(FILE_NAME).exists());
        assert!(!export_to(&ecad_model::sample::start_circuit().unwrap(), dir.path()).unwrap().removed);
    }

    #[test]
    fn files_that_arent_tag_tables_or_are_too_big_are_refused() {
        let mut book = rust_xlsxwriter::Workbook::new();
        book.add_worksheet().write_string(0, 0, "Name").unwrap();
        let e = read_xlsx(&book.save_to_buffer().unwrap()).unwrap_err();
        assert!(matches!(e, ExchangeError::MissingColumn("Logical Address")), "{e}");
        assert!(matches!(read_xlsx(b"not a spreadsheet"), Err(ExchangeError::Read(_))));
        let huge = vec![0u8; MAX_FILE + 1];
        assert!(read_xlsx(&huge).unwrap_err().to_string().contains("over 16 MB"));
        // A file whose parts unpack to more than the limit (here 2 MB of 1).
        let mut bomb = Vec::new();
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bomb));
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
            zip.write_all(&vec![b' '; 2 << 20]).unwrap();
            zip.finish().unwrap();
        }
        let e = check_size_within(&bomb, MAX_FILE, 1 << 20).unwrap_err();
        assert!(e.to_string().contains("unpacks to over 1 MB"), "{e}");
        assert!(check_size_within(&bomb, MAX_FILE, 4 << 20).is_ok());
    }
}
