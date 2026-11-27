//! Reading CSV: comma-, semicolon- or tab-separated (Excel in most European locales saves
//! with `;`, and "Unicode text" with tabs), quoted fields with doubled quotes, UTF-8, UTF-16
//! or Windows-1252 text.

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("line {line}: {message}")]
pub struct CsvError {
    pub line: usize,
    pub message: String,
}

fn err(line: usize, message: impl Into<String>) -> CsvError {
    CsvError { line, message: message.into() }
}

/// The text of a CSV file: UTF-16 with a byte-order mark, UTF-8 (with or without one), or
/// else Windows-1252, which Excel uses for CSV on Western European Windows.
pub fn decode(bytes: &[u8]) -> String {
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xff, 0xfe, ..] => utf16(true),
        [0xfe, 0xff, ..] => utf16(false),
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.strip_prefix('\u{feff}').unwrap_or(s).to_owned(),
            Err(_) => bytes.iter().map(|b| windows_1252(*b)).collect(),
        },
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[usize::from(b - 0x80)],
        _ => char::from(b),
    }
}

/// Split CSV text into records of fields, with the line each record starts on (1-based).
/// Quoted fields may contain the separator, doubled quotes and line breaks. Blank lines are
/// dropped.
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
    out.retain(|(_, r)| r.iter().any(|f| !f.trim().is_empty()));
    Ok(out)
}

/// The separator the first lines use most: tab, `;` or `,` (a title line above the header
/// may have none, so a few lines are counted).
pub fn detect_separator(text: &str) -> char {
    let head: String = text.lines().take(30).collect::<Vec<_>>().join("\n");
    let count = |c: char| head.matches(c).count();
    let (tabs, semis, commas) = (count('\t'), count(';'), count(','));
    if tabs > semis && tabs > commas {
        '\t'
    } else if semis > commas {
        ';'
    } else {
        ','
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn quoted_fields_separators_and_line_breaks() {
        let rows = records("a,\"b,c\",\"say \"\"hi\"\"\"\r\n\n\"two\nlines\",x\n", ',').unwrap();
        assert_eq!(rows[0], (1, vec!["a".into(), "b,c".into(), "say \"hi\"".into()]));
        assert_eq!(rows[1], (3, vec!["two\nlines".into(), "x".into()]));
        assert_eq!(records("\"open", ',').unwrap_err().line, 1);
    }

    #[test]
    fn separators_and_encodings() {
        assert_eq!(detect_separator("Listino 2026\na;b;c\n1;2,5;3\n"), ';');
        assert_eq!(detect_separator("a\tb\tc\n"), '\t');
        assert_eq!(detect_separator("a,b\n"), ',');
        assert_eq!(decode("\u{feff}Profondità".as_bytes()), "Profondità");
        assert_eq!(decode(&[b'P', b'r', b'o', b'f', 0xe0, b' ', 0x80]), "Prof\u{e0} €");
        let utf16: Vec<u8> = [0xff, 0xfe].into_iter().chain("Höhe".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&utf16), "Höhe");
    }
}
