//! Parts lists from manufacturers and spreadsheets: CSV and Excel (`.xlsx`, `.xlsm`, `.xls`,
//! `.ods`) into [`Part`]s, for the components library or a project's catalogue.
//!
//! Columns are found by their titles in English, Italian, German or French (`Order number`,
//! `Codice articolo`, `Bestellnummer`, `Référence`...), or named with [`Options::map`] when a
//! list uses titles of its own. The header needn't be the first row: a price list's title rows
//! above it are skipped. Every other column becomes a line of technical data, titled as in the
//! list, unless [`Options::data`] says otherwise.
//!
//! A row that can't be read (no order number, a size that isn't one, a part twice) is left
//! out and listed in [`PartsList::skipped`]; the rest still import.

use crate::csv;
use crate::{ExchangeError, Result};
use calamine::{Data, Reader};
use ecad_model::{Datum, Part};

/// Largest list read: a full manufacturer price list with technical data.
pub const MAX_FILE: usize = 64 << 20;
/// What an Excel list may unpack to.
pub const MAX_UNPACKED: u64 = 512 << 20;
pub const MAX_ROWS: usize = 200_000;
pub const MAX_COLUMNS: usize = 1000;
/// How far down a header row is looked for.
pub const HEADER_SEARCH: usize = 30;

/// What a column of the list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Number,
    Manufacturer,
    Description,
    Code,
    Contacts,
    Width,
    Height,
    Depth,
    /// `45 x 90 x 75` in one column.
    Size,
    Class,
    Datasheet,
}

impl Field {
    pub const ALL: [Field; 11] = [
        Field::Number,
        Field::Manufacturer,
        Field::Description,
        Field::Code,
        Field::Contacts,
        Field::Width,
        Field::Height,
        Field::Depth,
        Field::Size,
        Field::Class,
        Field::Datasheet,
    ];

    /// The name used for it in [`Options::map`] and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Field::Number => "number",
            Field::Manufacturer => "manufacturer",
            Field::Description => "description",
            Field::Code => "code",
            Field::Contacts => "contacts",
            Field::Width => "width",
            Field::Height => "height",
            Field::Depth => "depth",
            Field::Size => "size",
            Field::Class => "class",
            Field::Datasheet => "datasheet",
        }
    }

    pub fn from_name(name: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.name() == name.trim().to_lowercase())
    }

    /// Column titles that mean this field, normalised (see [`normalise`]).
    fn titles(self) -> &'static [&'static str] {
        match self {
            Field::Number => &[
                "part number",
                "number",
                "order number",
                "order no",
                "order code",
                "ordering code",
                "article",
                "article number",
                "article no",
                "part",
                "part no",
                "product number",
                "product code",
                "catalogue number",
                "catalog number",
                "cat no",
                "codice articolo",
                "codice prodotto",
                "codice costruttore",
                "cod articolo",
                "articolo",
                "riferimento",
                "bestellnummer",
                "bestell nr",
                "best nr",
                "artikelnummer",
                "artikel nr",
                "référence",
                "reference",
                "référence commerciale",
            ],
            Field::Manufacturer => &[
                "manufacturer",
                "maker",
                "brand",
                "vendor",
                "supplier",
                "costruttore",
                "produttore",
                "marca",
                "fornitore",
                "hersteller",
                "marke",
                "fabricant",
                "marque",
            ],
            Field::Description => &[
                "description",
                "desc",
                "text",
                "name",
                "short description",
                "product description",
                "designation",
                "descrizione",
                "descrizione articolo",
                "beschreibung",
                "bezeichnung",
                "kurzbeschreibung",
                "artikelbezeichnung",
                "désignation",
                "libellé",
            ],
            Field::Code => &[
                "code",
                "internal code",
                "article code",
                "item code",
                "our code",
                "codice",
                "codice interno",
                "nostro codice",
                "interne nummer",
            ],
            Field::Contacts => &["contacts", "contact blocks", "auxiliary contacts", "contatti", "kontakte"],
            Field::Width => &["width", "width mm", "larghezza", "larghezza mm", "breite", "breite mm", "largeur"],
            Field::Height => &["height", "height mm", "altezza", "altezza mm", "höhe", "höhe mm", "hauteur"],
            Field::Depth => {
                &["depth", "depth mm", "profondita", "profondità", "profondità mm", "tiefe", "tiefe mm", "profondeur"]
            }
            Field::Size => &["size", "dimensions", "dimensioni", "abmessungen", "maße"],
            Field::Class => &[
                "class",
                "classification",
                "etim class",
                "etim",
                "eclass",
                "ecl@ss",
                "classe",
                "classe etim",
                "klasse",
                "etim klasse",
            ],
            Field::Datasheet => &[
                "datasheet",
                "data sheet",
                "datasheet url",
                "url",
                "link",
                "scheda tecnica",
                "datenblatt",
                "fiche technique",
            ],
        }
    }
}

/// Which columns besides the known ones become technical data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DataColumns {
    /// All of them (the default).
    #[default]
    Rest,
    /// None.
    None,
    /// Only these, by title.
    Only(Vec<String>),
}

/// How to read a list.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The manufacturer of parts whose row doesn't name one: a manufacturer's own price list
    /// rarely has the column.
    pub manufacturer: Option<String>,
    /// The sheet of a workbook to read; the first one that has a header, if none.
    pub sheet: Option<String>,
    /// The header's row (1 = the first, as a spreadsheet numbers them); found, if none.
    pub header_row: Option<usize>,
    /// Columns named by title, overriding the titles recognised.
    pub map: Vec<(Field, String)>,
    pub data: DataColumns,
}

/// A list read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartsList {
    pub parts: Vec<Part>,
    /// Rows left out, as `row 12: why`.
    pub skipped: Vec<String>,
    /// What each column became: (title, field name or `data` or `ignored`).
    pub columns: Vec<(String, String)>,
    /// The sheet read, for a workbook.
    pub sheet: Option<String>,
    /// The header's row.
    pub header_row: usize,
}

/// A column title as compared: lowercase, `_ - . / :` and repeated spaces as one space, and
/// a unit in brackets dropped (`Width [mm]` is `width mm`).
pub fn normalise(title: &str) -> String {
    let t: String = title
        .to_lowercase()
        .chars()
        .map(|c| if matches!(c, '_' | '-' | '.' | '/' | ':' | '(' | ')' | '[' | ']') { ' ' } else { c })
        .collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Read a list. `file_name` decides the format by its extension: `.csv`, `.txt` and `.tsv`
/// as text, `.xlsx`, `.xlsm`, `.xls`, `.xlsb` and `.ods` as a workbook.
pub fn read_parts(bytes: &[u8], file_name: &str, options: &Options) -> Result<PartsList> {
    if bytes.len() > MAX_FILE {
        return Err(ExchangeError::Parts(format!("the file is over {} MB", MAX_FILE >> 20)));
    }
    let ext =
        std::path::Path::new(file_name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "csv" | "txt" | "tsv" => {
            let text = csv::decode(bytes);
            let rows =
                csv::records(&text, csv::detect_separator(&text)).map_err(|e| ExchangeError::Parts(e.to_string()))?;
            if rows.len() > MAX_ROWS + HEADER_SEARCH {
                return Err(ExchangeError::Parts(format!("over {MAX_ROWS} rows")));
            }
            from_rows(&rows, options, None)
        }
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => read_workbook(bytes, options),
        _ => Err(ExchangeError::Parts(format!("`{file_name}`: use a .csv, .txt, .xlsx, .xlsm, .xls or .ods file"))),
    }
}

fn read_workbook(bytes: &[u8], options: &Options) -> Result<PartsList> {
    if bytes.starts_with(b"PK") {
        crate::tia::check_size_within(bytes, MAX_FILE, MAX_UNPACKED)
            .map_err(|e| ExchangeError::Parts(e.to_string()))?;
    }
    let err = |e: calamine::Error| ExchangeError::Parts(e.to_string());
    let mut book = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(bytes)).map_err(err)?;
    let names = book.sheet_names();
    let wanted: Vec<String> = match &options.sheet {
        Some(s) => {
            let found = names.iter().find(|n| n.trim().eq_ignore_ascii_case(s.trim())).cloned();
            vec![found.ok_or_else(|| {
                ExchangeError::Parts(format!("no sheet `{s}` (the sheets are: {})", names.join(", ")))
            })?]
        }
        None => names.clone(),
    };
    let mut first_error = None;
    for name in wanted {
        let range = book.worksheet_range(&name).map_err(err)?;
        if range.height() > MAX_ROWS + HEADER_SEARCH {
            return Err(ExchangeError::Parts(format!("sheet `{name}` has over {MAX_ROWS} rows")));
        }
        if range.width() > MAX_COLUMNS {
            return Err(ExchangeError::Parts(format!("sheet `{name}` has over {MAX_COLUMNS} columns")));
        }
        // Row numbers as the spreadsheet shows them, whatever the first used row is.
        let top = range.start().map_or(0, |(r, _)| r as usize);
        let left = range.start().map_or(0, |(_, c)| c as usize);
        let rows: Vec<(usize, Vec<String>)> = range
            .rows()
            .enumerate()
            .map(|(i, r)| {
                let mut cells = vec![String::new(); left];
                cells.extend(r.iter().map(cell));
                (top + i + 1, cells)
            })
            .filter(|(_, r)| r.iter().any(|c| !c.trim().is_empty()))
            .collect();
        match from_rows(&rows, options, Some(name.clone())) {
            Ok(list) => return Ok(list),
            Err(e) if options.sheet.is_none() => {
                first_error.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(first_error.unwrap_or_else(|| ExchangeError::Parts("the workbook has no sheets".into())))
}

/// A cell as text: whole numbers without `.0`, others without float noise.
fn cell(c: &Data) -> String {
    match c {
        Data::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => {
            let s = format!("{f:.9}");
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
        other => other.to_string(),
    }
    .trim()
    .to_owned()
}

/// The header's place among `rows` and each column's meaning.
struct Header {
    at: usize,
    fields: Vec<Option<Field>>,
}

fn find_header(rows: &[(usize, Vec<String>)], options: &Options) -> Result<Header> {
    let mapped = |f: Field| options.map.iter().filter(move |(m, _)| *m == f).map(|(_, t)| normalise(t));
    let field_of = |title: &str| -> Option<Field> {
        let t = normalise(title);
        if t.is_empty() {
            return None;
        }
        // A title named in the map means that field and nothing else.
        if let Some((f, _)) = options.map.iter().find(|(_, m)| normalise(m) == t) {
            return Some(*f);
        }
        Field::ALL.into_iter().find(|f| mapped(*f).next().is_none() && f.titles().contains(&t.as_str()))
    };
    let candidates: Vec<usize> = match options.header_row {
        Some(n) => {
            let i = rows.iter().position(|(line, _)| *line == n);
            vec![i.ok_or_else(|| ExchangeError::Parts(format!("row {n} is empty or past the end")))?]
        }
        None => (0..rows.len().min(HEADER_SEARCH)).collect(),
    };
    // A list with a code column but no order-number column (an Italian price list's `Codice`)
    // is the manufacturer's own list: its code is the order number.
    let code_is_number = options.map.iter().all(|(f, _)| *f != Field::Number && *f != Field::Code);
    for i in candidates {
        let mut fields: Vec<Option<Field>> = rows[i].1.iter().map(|t| field_of(t)).collect();
        if code_is_number
            && !fields.contains(&Some(Field::Number))
            && let Some(c) = fields.iter().position(|f| *f == Some(Field::Code))
        {
            fields[c] = Some(Field::Number);
        }
        if fields.contains(&Some(Field::Number)) {
            // Each field from its first column only; a second `Description` is technical data.
            let mut seen = Vec::new();
            let fields = fields
                .into_iter()
                .map(|f| match f {
                    Some(f) if seen.contains(&f) => None,
                    Some(f) => {
                        seen.push(f);
                        Some(f)
                    }
                    None => None,
                })
                .collect();
            return Ok(Header { at: i, fields });
        }
    }
    let hint = match options.map.iter().find(|(f, _)| *f == Field::Number) {
        Some((_, t)) => format!("no column titled `{t}`"),
        None => "no order-number column (a title like `Order number`, `Codice articolo` or `Bestellnummer`); \
                 name it with --map number=<title>"
            .to_owned(),
    };
    let place = match options.header_row {
        Some(n) => format!("in row {n}"),
        None => format!("in the first {HEADER_SEARCH} rows"),
    };
    Err(ExchangeError::Parts(format!("{hint} {place}")))
}

/// A size in mm: `45`, `45 mm`, `73,5`.
fn size(text: &str) -> std::result::Result<Option<f64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let n = t.trim_end_matches("mm").trim().replace(',', ".");
    n.parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0 && v.is_finite())
        .map(Some)
        .ok_or_else(|| format!("`{t}` isn't a size in mm"))
}

fn from_rows(rows: &[(usize, Vec<String>)], options: &Options, sheet: Option<String>) -> Result<PartsList> {
    if rows.is_empty() {
        return Err(ExchangeError::Parts("the list is empty".into()));
    }
    let header = find_header(rows, options)?;
    let titles = &rows[header.at].1;
    if titles.len() > MAX_COLUMNS {
        return Err(ExchangeError::Parts(format!("over {MAX_COLUMNS} columns")));
    }
    // Technical data columns, with titles made unique.
    let wanted_data = |t: &str| match &options.data {
        DataColumns::Rest => true,
        DataColumns::None => false,
        DataColumns::Only(list) => list.iter().any(|w| normalise(w) == normalise(t)),
    };
    let mut data_cols: Vec<(usize, String)> = Vec::new();
    let mut columns = Vec::new();
    for (i, t) in titles.iter().enumerate() {
        let title = t.trim().to_owned();
        if title.is_empty() {
            continue;
        }
        let what = match header.fields.get(i).copied().flatten() {
            Some(f) => f.name().to_owned(),
            None if wanted_data(&title) => {
                let mut name = title.clone();
                let mut n = 2;
                while data_cols.iter().any(|(_, d)| d.eq_ignore_ascii_case(&name)) {
                    name = format!("{title} ({n})");
                    n += 1;
                }
                data_cols.push((i, name));
                "data".to_owned()
            }
            None => "ignored".to_owned(),
        };
        columns.push((title, what));
    }
    let col = |f: Field| header.fields.iter().position(|x| *x == Some(f));
    let cols: Vec<(Field, Option<usize>)> = Field::ALL.into_iter().map(|f| (f, col(f))).collect();
    let at = |f: Field| cols.iter().find(|(g, _)| *g == f).and_then(|(_, c)| *c);

    let mut list = PartsList { columns, sheet, header_row: rows[header.at].0, ..PartsList::default() };
    let mut seen: std::collections::BTreeMap<(String, String), usize> = Default::default();
    for (line, r) in &rows[header.at + 1..] {
        let get = |f: Field| at(f).and_then(|i| r.get(i)).map(|s| s.trim().to_owned()).unwrap_or_default();
        match row_part(r, &get, &data_cols, options) {
            Ok(None) => {}
            Ok(Some(part)) => {
                let k = ecad_model::parts_library::key(&part.manufacturer, &part.number);
                if let Some(first) = seen.get(&k) {
                    list.skipped.push(format!("row {line}: {} again (first in row {first}), left out", part.number));
                    continue;
                }
                seen.insert(k, *line);
                list.parts.push(part);
            }
            Err(why) => list.skipped.push(format!("row {line}: {why}")),
        }
    }
    Ok(list)
}

/// One row as a part; `None` for a row with nothing in the known columns (a group heading).
fn row_part(
    r: &[String],
    get: &dyn Fn(Field) -> String,
    data_cols: &[(usize, String)],
    options: &Options,
) -> std::result::Result<Option<Part>, String> {
    let number = get(Field::Number);
    if number.is_empty() {
        let anything = Field::ALL.iter().filter(|f| **f != Field::Number).any(|f| !get(*f).is_empty());
        return if anything { Err("no order number, left out".into()) } else { Ok(None) };
    }
    let manufacturer = match get(Field::Manufacturer) {
        m if m.is_empty() => options.manufacturer.clone().unwrap_or_default(),
        m => m,
    };
    let contacts = ecad_model::parts::parse_blocks(&get(Field::Contacts))
        .map_err(|bad| format!("`{bad}` isn't a contact block (write 13-14, 21-22 or 11-12-14)"))?;
    let (mut width, mut height, mut depth) =
        (size(&get(Field::Width))?, size(&get(Field::Height))?, size(&get(Field::Depth))?);
    let whole = get(Field::Size);
    if !whole.is_empty() && width.is_none() && height.is_none() {
        let cleaned = whole.replace("mm", "");
        (width, height, depth) =
            ecad_model::parts::parse_size(&cleaned).map_err(|t| format!("`{t}` isn't a size like 45 x 90 x 75"))?;
    }
    let data = data_cols
        .iter()
        .filter_map(|(i, name)| {
            let v = r.get(*i).map(|s| s.trim()).unwrap_or_default();
            (!v.is_empty()).then(|| Datum::new(name, v))
        })
        .collect();
    let part = Part {
        number,
        manufacturer,
        description: get(Field::Description),
        code: get(Field::Code),
        contacts,
        width,
        height,
        depth,
        class: get(Field::Class),
        data,
        datasheet: get(Field::Datasheet),
    };
    part.check()?;
    Ok(Some(part))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn csv(text: &str, options: &Options) -> PartsList {
        read_parts(text.as_bytes(), "list.csv", options).unwrap()
    }

    #[test]
    fn a_semicolon_excel_export_with_extra_columns_imports() {
        let text = "\u{feff}Part number;Manufacturer;Description;Price\nEX-1;Example;\"Contactor; 24 V\";12,50\n\nEX-2;;Relay;3\n";
        let list = csv(text, &Options::default());
        assert_eq!(list.parts.len(), 2);
        assert_eq!(
            list.parts[0],
            Part {
                description: "Contactor; 24 V".into(),
                data: vec![Datum::new("Price", "12,50")],
                ..Part::new("EX-1", "Example")
            }
        );
        assert_eq!(list.parts[1].manufacturer, "");
        assert_eq!(list.columns[3], ("Price".to_owned(), "data".to_owned()));
        let with = csv("number,contacts\nK,\"13-14 21-22\"\nR,11-12-14\n", &Options::default());
        assert_eq!(with.parts[0].contacts, ["13-14", "21-22"]);
        assert_eq!(with.parts[1].contacts, ["11-12-14"]);
        let sized = csv("number;width [mm];height;depth\nK;45;90 mm;\"73,5\"\n", &Options::default());
        let p = &sized.parts[0];
        assert_eq!((p.width, p.height, p.depth), (Some(45.0), Some(90.0), Some(73.5)));
        let whole = csv("number;dimensioni\nK;45 x 90 x 75 mm\n", &Options::default());
        assert_eq!(whole.parts[0].depth, Some(75.0));
    }

    #[test]
    fn a_manufacturers_price_list_in_italian_with_title_rows() {
        let text = "Listino prezzi 2026;;;;\nValido dal 1 gennaio;;;;\n\
                    Codice articolo;Descrizione;Corrente nominale;Classe ETIM;Scheda tecnica\n\
                    EX-CT-9;Contattore 4 kW;9 A;EC000066;https://example.com/ct9.pdf\n\
                    ;Gruppo: relè;;;\n\
                    EX-RL-1;Relè 24 V;;EC001437;\n";
        let options = Options { manufacturer: Some("Example".into()), ..Options::default() };
        let list = csv(text, &options);
        assert_eq!(list.header_row, 3);
        assert_eq!(list.parts.len(), 2);
        let k = &list.parts[0];
        assert_eq!((k.manufacturer.as_str(), k.class.as_str()), ("Example", "EC000066"));
        assert_eq!(k.datasheet, "https://example.com/ct9.pdf");
        assert_eq!(k.data, [Datum::new("Corrente nominale", "9 A")]);
        // The group heading has text but no number: it is listed, not imported.
        assert_eq!(list.skipped, ["row 5: no order number, left out"]);
        assert!(list.parts[1].data.is_empty());
    }

    #[test]
    fn columns_by_name_and_data_chosen() {
        let text = "Art.;Testo;Prezzo;Imballo\nA1;Morsetto;0,50;50\n";
        let none = read_parts(text.as_bytes(), "x.csv", &Options::default()).unwrap_err().to_string();
        assert!(none.contains("--map number="), "{none}");
        let options = Options {
            map: vec![(Field::Number, "Art.".into()), (Field::Description, "testo".into())],
            data: DataColumns::Only(vec!["prezzo".into()]),
            ..Options::default()
        };
        let list = csv(text, &options);
        let p = &list.parts[0];
        assert_eq!((p.number.as_str(), p.description.as_str()), ("A1", "Morsetto"));
        assert_eq!(p.data, [Datum::new("Prezzo", "0,50")]);
        assert_eq!(list.columns[3], ("Imballo".to_owned(), "ignored".to_owned()));
        let bare = csv("number;Price;Price\nA;1;2\n", &Options { data: DataColumns::Rest, ..Options::default() });
        assert_eq!(bare.parts[0].data, [Datum::new("Price", "1"), Datum::new("Price (2)", "2")]);
        let off = csv("number;Price\nA;1\n", &Options { data: DataColumns::None, ..Options::default() });
        assert!(off.parts[0].data.is_empty());
        let codice = csv("Codice;Descrizione\nEX-9;Contattore\n", &Options::default());
        assert_eq!(codice.parts[0].number, "EX-9");
        assert_eq!(codice.columns[0], ("Codice".to_owned(), "number".to_owned()));
        let both = csv("Codice;Codice articolo\nC-1;EX-9\n", &Options::default());
        assert_eq!((both.parts[0].number.as_str(), both.parts[0].code.as_str()), ("EX-9", "C-1"));
        let row = Options { header_row: Some(2), ..Options::default() };
        assert_eq!(csv("Bestellnummer\nnumber\nA\n", &row).parts[0].number, "A");
    }

    #[test]
    fn bad_rows_are_listed_and_the_rest_imports() {
        let text = "number,manufacturer,description,width,contacts\n\
                    A,M,ok,,\n,M,no number,,\nA,m,again,,\nA,Other,same number other maker,,\n\
                    B,M,bad size,wide,\nC,M,bad contacts,,13-15\n";
        let list = csv(text, &Options::default());
        let numbers: Vec<&str> = list.parts.iter().map(|p| p.number.as_str()).collect();
        assert_eq!(numbers, ["A", "A"]);
        assert_eq!(list.skipped.len(), 4, "{:?}", list.skipped);
        assert!(list.skipped[1].starts_with("row 4: A again (first in row 2)"));
        assert!(list.skipped[2].contains("`wide` isn't a size"));
        assert!(read_parts(b"", "x.csv", &Options::default()).is_err());
        assert!(read_parts(b"number\nA\n", "x.pdf", &Options::default()).is_err());
    }

    #[test]
    fn an_excel_list_on_a_later_sheet_with_numbers() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let notes = book.add_worksheet();
        notes.set_name("Notes").unwrap();
        notes.write(0, 0, "Prices without VAT").unwrap();
        let s = book.add_worksheet();
        s.set_name("Articoli").unwrap();
        for (c, t) in ["", "Bestellnummer", "Bezeichnung", "Breite", "Bemessungsstrom"].iter().enumerate() {
            s.write(3, c as u16, *t).unwrap();
        }
        s.write(4, 1, "EX-MCB-C16").unwrap();
        s.write(4, 2, "Leitungsschutzschalter C16").unwrap();
        s.write(4, 3, 17.5).unwrap();
        s.write(4, 4, 16).unwrap();
        s.write(5, 1, 12345).unwrap();
        let bytes = book.save_to_buffer().unwrap();
        let options = Options { manufacturer: Some("Example".into()), ..Options::default() };
        let list = read_parts(&bytes, "list.xlsx", &options).unwrap();
        assert_eq!((list.sheet.as_deref(), list.header_row), (Some("Articoli"), 4));
        let p = &list.parts[0];
        assert_eq!((p.number.as_str(), p.width), ("EX-MCB-C16", Some(17.5)));
        assert_eq!(p.data, [Datum::new("Bemessungsstrom", "16")]);
        assert_eq!(list.parts[1].number, "12345", "a number cell reads without .0");
        let wrong = Options { sheet: Some("Missing".into()), ..Options::default() };
        assert!(read_parts(&bytes, "list.xlsx", &wrong).unwrap_err().to_string().contains("Notes, Articoli"));
        assert!(read_parts(b"PK not a zip", "list.xlsx", &Options::default()).is_err());
    }
}
