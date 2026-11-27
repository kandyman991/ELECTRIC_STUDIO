//! Reading CSV, for importing parts catalogues from spreadsheets and vendor lists.
//!
//! Accepts comma- or semicolon-separated files (Excel in most European locales saves with `;`),
//! quoted fields with doubled quotes, and a header row naming the columns.

use ecad_model::Part;

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("line {line}: {message}")]
pub struct CsvError {
    pub line: usize,
    pub message: String,
}

fn err(line: usize, message: impl Into<String>) -> CsvError {
    CsvError { line, message: message.into() }
}

/// Split CSV text into records of fields, with the line each record starts on (1-based).
/// Quoted fields may contain the separator, doubled quotes and line breaks.
pub fn records(text: &str, sep: char) -> Result<Vec<(usize, Vec<String>)>, CsvError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let (mut field, mut record) = (String::new(), Vec::new());
    let (mut quoted, mut line, mut start) = (false, 1, 1);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == sep => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                out.push((start, std::mem::take(&mut record)));
                line += 1;
                start = line;
            }
            c => field.push(c),
        }
    }
    if quoted {
        return Err(err(start, "a quoted field is not closed"));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push((start, record));
    }
    // Drop blank lines.
    out.retain(|(_, r)| r.iter().any(|f| !f.trim().is_empty()));
    Ok(out)
}

/// `;` when the header has more semicolons than commas, else `,`.
pub fn detect_separator(text: &str) -> char {
    let header = text.lines().next().unwrap_or("");
    if header.matches(';').count() > header.matches(',').count() { ';' } else { ',' }
}

fn column(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| {
        let h = h.trim().to_lowercase().replace(['_', '-'], " ");
        names.contains(&h.as_str())
    })
}

/// Parts from a CSV catalogue, each with a new id. The same number from two manufacturers is two
/// parts; the same number twice from one is refused. The header must have a part-number column (`part number`,
/// `number`, `order number`, `article`…); `manufacturer`, `description`, `code`, `contacts`
/// (blocks like `13-14 21-22`) and `width`, `height`, `depth` (mm) are optional. Other columns
/// are ignored.
pub fn read_parts(text: &str) -> Result<Vec<Part>, CsvError> {
    let rows = records(text, detect_separator(text))?;
    let Some((header_line, header)) = rows.first() else { return Err(err(1, "the file is empty")) };
    let number = column(header, &["part number", "number", "order number", "article", "article number", "part"])
        .ok_or_else(|| err(*header_line, "no part number column (expected e.g. \"Part number\")"))?;
    let manufacturer = column(header, &["manufacturer", "maker", "brand", "vendor"]);
    let description = column(header, &["description", "desc", "text", "name"]);
    let code = column(header, &["code", "internal code", "article code", "item code", "codice", "codice interno"]);
    let contacts = column(header, &["contacts", "contact blocks", "auxiliary contacts", "contatti"]);
    let width = column(header, &["width", "width mm", "larghezza"]);
    let height = column(header, &["height", "height mm", "altezza"]);
    let depth = column(header, &["depth", "depth mm", "profondita", "profondità"]);
    let get =
        |r: &[String], i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_owned()).unwrap_or_default();
    // A size in mm; empty is none, a decimal comma is fine.
    let size = |r: &[String], i: Option<usize>, line: usize| -> Result<Option<f64>, CsvError> {
        let text = get(r, i);
        if text.is_empty() {
            return Ok(None);
        }
        text.replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| *v > 0.0 && v.is_finite())
            .map(Some)
            .ok_or_else(|| err(line, format!("`{text}` isn't a size in mm")))
    };

    let mut parts: Vec<Part> = Vec::new();
    for (line, r) in &rows[1..] {
        let n = get(r, Some(number));
        if n.is_empty() {
            return Err(err(*line, "the part number is empty"));
        }
        let maker = get(r, manufacturer);
        if parts.iter().any(|p| p.number == n && p.manufacturer.to_lowercase() == maker.to_lowercase()) {
            return Err(err(*line, format!("part `{n}` appears twice in the file")));
        }
        let contacts = ecad_model::parts::parse_blocks(&get(r, contacts))
            .map_err(|bad| err(*line, format!("`{bad}` isn't a contact block (write 13-14, 21-22 or 11-12-14)")))?;
        // A new id: the import matches it to a catalogue product by maker and number.
        parts.push(Part {
            id: ecad_model::ProductId::new(),
            number: n,
            manufacturer: maker,
            description: get(r, description),
            code: get(r, code),
            contacts,
            width: size(r, width, *line)?,
            height: size(r, height, *line)?,
            depth: size(r, depth, *line)?,
        });
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_fields_separators_and_line_breaks() {
        let rows = records("a,\"b,c\",\"say \"\"hi\"\"\"\r\n\n\"two\nlines\",x\n", ',').unwrap();
        assert_eq!(rows[0], (1, vec!["a".into(), "b,c".into(), "say \"hi\"".into()]));
        assert_eq!(rows[1], (3, vec!["two\nlines".into(), "x".into()]));
        assert_eq!(records("\"open", ',').unwrap_err().line, 1);
    }

    #[test]
    fn a_semicolon_excel_export_with_extra_columns_imports() {
        let text = "\u{feff}Part number;Manufacturer;Description;Price\nEX-1;Example;\"Contactor; 24 V\";12,50\n\nEX-2;;Relay;3\n";
        let parts = read_parts(text).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[0],
            Part {
                id: parts[0].id,
                manufacturer: "Example".into(),
                description: "Contactor; 24 V".into(),
                ..Part::new("EX-1")
            }
        );
        assert_ne!(parts[0].id, parts[1].id);
        assert_eq!(parts[1].manufacturer, "");
        let with = read_parts("number,contacts\nK,\"13-14 21-22\"\nR,11-12-14\n").unwrap();
        assert_eq!(with[0].contacts, ["13-14", "21-22"]);
        assert_eq!(with[1].contacts, ["11-12-14"]);
        assert_eq!(read_parts("number,contacts\nK,13-15\n").unwrap_err().line, 2);
        let sized = read_parts("number;width;height;depth\nK;45;90;\"73,5\"\n").unwrap();
        assert_eq!((sized[0].width, sized[0].height, sized[0].depth), (Some(45.0), Some(90.0), Some(73.5)));
    }

    #[test]
    fn problems_name_their_line() {
        assert_eq!(read_parts("Manufacturer,Description\nA,B\n").unwrap_err().line, 1);
        assert_eq!(read_parts("number,description\nX,a\n,b\n").unwrap_err(), err(3, "the part number is empty"));
        assert_eq!(read_parts("order_number\nX\nX\n").unwrap_err(), err(3, "part `X` appears twice in the file"));
        // The same number from two makers is two parts (D7).
        assert_eq!(read_parts("number,maker\nX,A\nX,B\n").unwrap().len(), 2);
        assert_eq!(read_parts("").unwrap_err().line, 1);
    }
}
