//! The TIA Portal PLC tag table: an Excel file with a `PLC Tags` sheet, one tag per row.
//!
//! Export: every PLC channel with an address becomes a tag. Its name is the channel's symbol,
//! or for a channel without one a name from the card and channel (`KF1_DI3`); digital
//! channels are `Bool`, analog ones `Int`; addresses get TIA Portal's `%` (`%I0.3`, `%IW256`).
//! Columns follow TIA Portal's own English export, so the file imports with
//! *PLC tags > Import*.
//!
//! Import: a table exported from TIA Portal (English or German column titles) is matched to
//! the channels by address. Each matched channel takes the tag's name as its symbol and the
//! tag's comment, all in one undoable step; empty cells leave the channel's value alone, and
//! tags at addresses no channel has (memory, data blocks, unused inputs) are listed, not added.

use crate::{ExchangeError, Result};
use calamine::{Data, Reader, Xlsx};
use ecad_model::{COMMENT, ChannelRef, Command, FunctionId, IoKind, Project, SYMBOL};
use std::collections::{BTreeMap, BTreeSet};

/// The sheet TIA Portal reads and writes.
pub const SHEET: &str = "PLC Tags";
/// The tag table new tags go into; every PLC has it.
pub const TAG_TABLE: &str = "Default tag table";
/// The file name in an export folder.
pub const FILE_NAME: &str = "tia-plc-tags.xlsx";

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
}

/// A channel left out of the export, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// `-KF1:3`.
    pub channel: String,
    pub reason: String,
}

/// An address in the model's form (`I0.3`, `IW256`), upper case, without TIA Portal's `%`.
fn plain(address: &str) -> String {
    address.trim().trim_start_matches('%').replace(' ', "").to_uppercase()
}

/// Whether `address` (plain) is the right kind for the channel: `I`/`Q` bits for digital,
/// `IW`/`QW` words for analog.
fn fits(kind: IoKind, address: &str) -> bool {
    let bit = |rest: &str| {
        rest.split_once('.')
            .is_some_and(|(byte, bit)| byte.parse::<u32>().is_ok() && bit.parse::<u32>().is_ok_and(|b| b < 8))
    };
    let word = |rest: &str| rest.parse::<u32>().is_ok();
    match kind {
        IoKind::DigitalIn => address.strip_prefix('I').is_some_and(bit),
        IoKind::DigitalOut => address.strip_prefix('Q').is_some_and(bit),
        IoKind::AnalogIn => address.strip_prefix("IW").is_some_and(word),
        IoKind::AnalogOut => address.strip_prefix("QW").is_some_and(word),
    }
}

/// The name a channel without a symbol is exported under: card tag and channel, letters,
/// digits and `_` only (`-KF1`, DI 3 → `KF1_DI3`).
fn placeholder(card: &str, channel: &ChannelRef) -> String {
    let card: String = card.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let card = card.trim_matches('_');
    format!("{card}_{}{}", channel.kind.short(), channel.channel)
}

/// Every PLC card with its tag, by tag, and its channels in order.
fn cards(project: &Project) -> Vec<(String, Vec<ChannelRef>)> {
    let mut out: Vec<(String, Vec<ChannelRef>)> = project
        .devices
        .values()
        .map(|d| (d.tag.clone(), project.plc_channels_of(d.id)))
        .filter(|(_, channels)| !channels.is_empty())
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The tags for TIA Portal, card by card, and the channels that can't be tags.
pub fn tags(project: &Project) -> (Vec<Tag>, Vec<Skipped>) {
    let mut tags = Vec::new();
    let mut skipped = Vec::new();
    let symbols: BTreeSet<String> =
        cards(project).iter().flat_map(|(_, cs)| cs.iter().map(|c| c.symbol.clone())).collect();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for (card, channels) in cards(project) {
        for c in channels {
            let at = format!("{card}:{}", c.channel);
            let address = plain(&c.address);
            if address.is_empty() {
                skipped.push(Skipped { channel: at, reason: "no address".into() });
                continue;
            }
            if !fits(c.kind, &address) {
                let reason = format!("address {} doesn't fit a {} channel", c.address, c.kind.short());
                skipped.push(Skipped { channel: at, reason });
                continue;
            }
            let mut name = if c.symbol.is_empty() { placeholder(&card, &c) } else { c.symbol.clone() };
            // A made-up name must not take a real symbol or another made-up name.
            if c.symbol.is_empty() {
                let base = name.clone();
                let mut n = 2;
                while symbols.contains(&name) || names.contains(&name) {
                    name = format!("{base}_{n}");
                    n += 1;
                }
            }
            names.insert(name.clone());
            let analog = matches!(c.kind, IoKind::AnalogIn | IoKind::AnalogOut);
            tags.push(Tag {
                name,
                path: TAG_TABLE.into(),
                data_type: if analog { "Int" } else { "Bool" }.into(),
                address: format!("%{address}"),
                comment: c.comment.clone(),
            });
        }
    }
    (tags, skipped)
}

/// The tag table as an Excel file for TIA Portal.
pub fn to_xlsx(tags: &[Tag]) -> Result<Vec<u8>> {
    let err = |e: rust_xlsxwriter::XlsxError| ExchangeError::Write(e.to_string());
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name(SHEET).map_err(err)?;
    for (col, title) in (0u16..).zip(COLUMNS) {
        sheet.write_string(0, col, title).map_err(err)?;
    }
    for (row, tag) in (1u32..).zip(tags) {
        let cells = [&tag.name, &tag.path, &tag.data_type, &tag.address, &tag.comment];
        for (col, value) in (0u16..).zip(cells) {
            sheet.write_string(row, col, value.as_str()).map_err(err)?;
        }
        for col in 5..8 {
            sheet.write_string(row, col, "True").map_err(err)?;
        }
    }
    sheet.autofit();
    book.save_to_buffer().map_err(err)
}

/// The tags of a TIA Portal tag table (`PLC Tags` sheet, or the first sheet), by their
/// English or German column titles. Rows without a name or address are left out.
pub fn read_xlsx(bytes: &[u8]) -> Result<Vec<Tag>> {
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
    let mut rows = range.rows();
    let header: Vec<String> =
        rows.next().map(|r| r.iter().map(|c| c.to_string().trim().to_lowercase()).collect()).unwrap_or_default();
    let column = |titles: &[&str], what: &'static str| {
        header.iter().position(|h| titles.contains(&h.as_str())).ok_or(ExchangeError::MissingColumn(what))
    };
    let name = column(&["name"], "Name")?;
    let address = column(&["logical address", "logische adresse", "address", "adresse"], "Logical Address")?;
    let comment = column(&["comment", "kommentar"], "Comment").ok();
    let path = column(&["path", "pfad"], "Path").ok();
    let data_type = column(&["data type", "datentyp"], "Data Type").ok();
    let cell = |row: &[Data], i: Option<usize>| {
        i.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_owned()).unwrap_or_default()
    };
    Ok(rows
        .map(|row| Tag {
            name: cell(row, Some(name)),
            path: cell(row, path),
            data_type: cell(row, data_type),
            address: cell(row, Some(address)),
            comment: cell(row, comment),
        })
        .filter(|t| !t.name.is_empty() && !t.address.is_empty())
        .collect())
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
    /// Tags that aren't inputs or outputs (memory, data blocks): not for the schematic.
    pub other: usize,
    /// Tags whose address a tag earlier in the table already had.
    pub duplicates: Vec<String>,
}

/// Match a TIA Portal tag table to the project's PLC channels by address.
pub fn import(project: &Project, tags: &[Tag]) -> Result<Import> {
    let mut by_address: BTreeMap<String, (String, ChannelRef)> = BTreeMap::new();
    for (card, channels) in cards(project) {
        for c in channels {
            let a = plain(&c.address);
            if !a.is_empty() {
                by_address.insert(a, (card.clone(), c));
            }
        }
    }
    let mut out = Import::default();
    let mut seen = BTreeSet::new();
    // (function, old properties, new properties)
    let mut changes: Vec<(FunctionId, Properties, Properties)> = Vec::new();
    for tag in tags {
        let address = plain(&tag.address);
        let kinds = [IoKind::DigitalIn, IoKind::DigitalOut, IoKind::AnalogIn, IoKind::AnalogOut];
        if !kinds.iter().any(|k| fits(*k, &address)) {
            out.other += 1;
            continue;
        }
        if !seen.insert(address.clone()) {
            out.duplicates.push(format!("{} {}", tag.address, tag.name));
            continue;
        }
        let Some((card, channel)) = by_address.get(&address) else {
            out.unknown.push(format!("{} {}", tag.address, tag.name));
            continue;
        };
        // Our own made-up name coming back isn't a symbol.
        let symbol =
            if channel.symbol.is_empty() && tag.name.starts_with(&placeholder(card, channel)) { "" } else { &tag.name };
        let old = project.function(channel.function)?.properties.clone();
        let mut new = old.clone();
        if !symbol.is_empty() {
            new.insert(SYMBOL.into(), symbol.to_owned());
        }
        if !tag.comment.is_empty() {
            new.insert(COMMENT.into(), tag.comment.clone());
        }
        if new == old {
            out.unchanged += 1;
        } else {
            changes.push((channel.function, old, new));
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
    use ecad_model::ADDRESS;
    use ecad_model::sample::with_plc;

    fn channel_properties(p: &Project, card: &str, channel: &str) -> (String, String, String) {
        let d = p.devices.values().find(|d| d.tag == card).unwrap();
        let c = p.plc_channels_of(d.id).into_iter().find(|c| c.channel == channel).unwrap();
        (c.address, c.symbol, c.comment)
    }

    #[test]
    fn the_sample_channels_become_tia_tags() {
        let p = with_plc().unwrap();
        let (tags, skipped) = tags(&p);
        assert!(skipped.is_empty(), "{skipped:?}");
        assert!(!tags.is_empty());
        for t in &tags {
            assert!(t.address.starts_with('%'), "{t:?}");
            assert_eq!(t.path, TAG_TABLE);
            assert!(t.data_type == "Bool" || t.data_type == "Int");
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
        assert_eq!(back, tags);
        let import = import(&p, &back).unwrap();
        assert_eq!((import.updated, import.command.is_none()), (0, true), "{import:?}");
        assert_eq!(import.unchanged, tags.len());
    }

    #[test]
    fn symbols_and_comments_renamed_in_tia_come_back_by_address() {
        let mut p = with_plc().unwrap();
        let (mut tags, _) = tags(&p);
        let first = tags[0].clone();
        let second = tags[1].clone();
        // Swap two names (symbols are unique, so this needs the two-step import), add a
        // comment, and add tags the schematic doesn't have.
        tags[0].name = second.name.clone();
        tags[1].name = first.name.clone();
        tags[1].comment = "Renamed in TIA".into();
        tags.push(Tag { name: "Spare".into(), address: "%I9.0".into(), ..first.clone() });
        tags.push(Tag { name: "Memory".into(), address: "%M10.0".into(), ..first.clone() });
        tags.push(Tag { name: "Again".into(), ..first.clone() });
        let import = import(&p, &tags).unwrap();
        assert_eq!(import.updated, 2);
        assert_eq!(import.unknown, ["%I9.0 Spare"]);
        assert_eq!(import.other, 1);
        assert_eq!(import.duplicates, [format!("{} Again", first.address)]);
        p.apply(import.command.unwrap()).unwrap();
        let (tags_after, _) = super::tags(&p);
        let at = |a: &str| tags_after.iter().find(|t| t.address == a).unwrap().clone();
        assert_eq!(at(&first.address).name, second.name);
        assert_eq!(at(&second.address).name, first.name);
        assert_eq!(at(&second.address).comment, "Renamed in TIA");
        // An empty comment in TIA leaves ours.
        assert_eq!(at(&first.address).comment, first.comment);
    }

    #[test]
    fn channels_without_a_usable_address_are_skipped_and_unnamed_ones_get_a_name() {
        let mut p = with_plc().unwrap();
        let card = p.devices.values().find(|d| !p.plc_channels_of(d.id).is_empty()).unwrap();
        let tag = card.tag.clone();
        let channels = p.plc_channels_of(card.id);
        let (a, b) = (&channels[0], &channels[1]);
        let set = |p: &mut Project, f, props: &[(&str, &str)]| {
            let properties = props.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            p.apply(Command::SetProperties { id: f, properties }).unwrap();
        };
        set(&mut p, a.function, &[]);
        let wrong = if matches!(b.kind, IoKind::DigitalIn | IoKind::DigitalOut) { "IW500" } else { "I50.0" };
        set(&mut p, b.function, &[(ADDRESS, wrong)]);
        let (_, skipped) = tags(&p);
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert_eq!(skipped[0], Skipped { channel: format!("{tag}:{}", a.channel), reason: "no address".into() });
        assert!(skipped[1].reason.contains("doesn't fit"), "{skipped:?}");
        // Give a third channel no symbol: it's named from card and channel, and that name
        // coming back from TIA doesn't become its symbol.
        let c = &channels[2];
        set(&mut p, c.function, &[(ADDRESS, &c.address)]);
        let (tags, _) = super::tags(&p);
        let made_up = tags.iter().find(|t| plain(&t.address) == plain(&c.address)).unwrap();
        assert_eq!(made_up.name, placeholder(&tag, c));
        assert!(made_up.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
        let import = import(&p, &tags).unwrap();
        assert_eq!(import.updated, 0, "{import:?}");
        assert_eq!(channel_properties(&p, &tag, &c.channel).1, "");
    }

    #[test]
    fn a_german_table_reads_and_a_table_without_addresses_is_refused() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name("PLC-Variablen").unwrap();
        for (col, title) in (0u16..).zip(["Name", "Pfad", "Datentyp", "Logische Adresse", "Kommentar"]) {
            sheet.write_string(0, col, title).unwrap();
        }
        for (col, value) in (0u16..).zip(["Start", "Standard-Variablentabelle", "Bool", "%I0.0", "Taster"]) {
            sheet.write_string(1, col, value).unwrap();
        }
        let tags = read_xlsx(&book.save_to_buffer().unwrap()).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(
            (tags[0].name.as_str(), tags[0].address.as_str(), tags[0].comment.as_str()),
            ("Start", "%I0.0", "Taster")
        );

        let mut book = rust_xlsxwriter::Workbook::new();
        book.add_worksheet().write_string(0, 0, "Name").unwrap();
        let e = read_xlsx(&book.save_to_buffer().unwrap()).unwrap_err();
        assert!(matches!(e, ExchangeError::MissingColumn("Logical Address")), "{e}");
        assert!(matches!(read_xlsx(b"not a spreadsheet"), Err(ExchangeError::Read(_))));
    }
}
