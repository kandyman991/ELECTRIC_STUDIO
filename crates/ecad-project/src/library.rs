//! The components library on disk: a folder of pretty-printed JSON, shared between projects
//! (on a network share, or kept in git like a project).
//!
//! ```text
//! components/
//!   library.json              format version and name
//!   parts/<manufacturer>.json that manufacturer's parts, sorted by order number
//! ```
//!
//! One file per manufacturer keeps each file a reasonable size and each change to one maker
//! in one file. Saving is the same all-or-nothing step as a project's ([`crate::store`]), with
//! the same lock and the same check against someone else's save in between.

use crate::{Fingerprint, ProjectError, Result, fingerprint, io, read_bounded, render_json, store};
use ecad_model::{Part, PartsLibrary};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file that makes a folder a library.
pub const LIBRARY_FILE: &str = "library.json";
pub const LIBRARY_FORMAT: u32 = 1;
/// Largest manufacturer file read back: tens of thousands of parts with full technical data.
pub const MAX_PARTS_FILE: usize = 256 << 20;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    format: u32,
    #[serde(default)]
    name: String,
}

/// The file a manufacturer's parts go in: `parts/<name>.json`, the name in lowercase letters
/// and digits with `-` for anything else; `parts/no-manufacturer.json` for parts without one.
pub fn parts_file(manufacturer: &str) -> String {
    let mut slug = String::new();
    for c in manufacturer.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "no-manufacturer" } else { slug };
    // Long enough for any real name, short enough for any file system.
    let slug: String = slug.chars().take(80).collect();
    format!("parts/{slug}.json")
}

/// Whether `dir` is a components library.
pub fn is_library(dir: &Path) -> bool {
    dir.join(LIBRARY_FILE).is_file()
}

/// Make a new, empty library in `dir` (created if needed). A folder that is already a library
/// or a project is refused.
pub fn create_library(dir: &Path, name: &str) -> Result<Fingerprint> {
    if is_library(dir) {
        return Err(ProjectError::NotALibrary(dir.to_owned(), "it is a library already"));
    }
    if dir.join("project.json").exists() {
        return Err(ProjectError::NotALibrary(dir.to_owned(), "it is a project folder"));
    }
    save_library(&PartsLibrary::new(name), dir, None)
}

/// Every file of a library, as (path inside the folder, contents).
fn render(library: &PartsLibrary) -> Result<Vec<(String, Vec<u8>)>> {
    let head = LibraryFile { format: LIBRARY_FORMAT, name: library.name.clone() };
    let mut files = vec![(LIBRARY_FILE.to_owned(), render_json(Path::new(LIBRARY_FILE), &head)?)];
    let mut by_file: std::collections::BTreeMap<String, Vec<&Part>> = Default::default();
    for p in library.parts() {
        by_file.entry(parts_file(&p.manufacturer)).or_default().push(p);
    }
    for (name, mut parts) in by_file {
        parts.sort_by(|a, b| {
            (a.number.to_lowercase(), a.manufacturer.to_lowercase())
                .cmp(&(b.number.to_lowercase(), b.manufacturer.to_lowercase()))
        });
        let bytes = render_json(Path::new(&name), &parts)?;
        files.push((name, bytes));
    }
    Ok(files)
}

/// Save the library into `dir`, only if the folder is still as it was at `expected` (from
/// [`load_library`] or the last save); returns the new fingerprint. Manufacturer files no
/// longer needed are deleted; nothing else in the folder is touched.
pub fn save_library(library: &PartsLibrary, dir: &Path, expected: Option<Fingerprint>) -> Result<Fingerprint> {
    if dir.join("project.json").exists() {
        return Err(ProjectError::NotALibrary(dir.to_owned(), "it is a project folder"));
    }
    let files = render(library)?;
    for d in [dir.to_owned(), dir.join("parts")] {
        std::fs::create_dir_all(&d).map_err(io(&d))?;
    }
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    if let Some(expected) = expected
        && fingerprint(dir)? != expected
    {
        return Err(ProjectError::Changed(dir.to_owned()));
    }
    store::commit(dir, &files)?;
    fingerprint(dir)
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = read_bounded(path, MAX_PARTS_FILE)?;
    serde_json::from_slice(&bytes).map_err(|source| ProjectError::Json { path: path.to_owned(), source })
}

/// Read a library, and its fingerprint as read for a later [`save_library`]. A part file that
/// breaks a rule (a part twice, data that [`Part::check`] refuses) is rejected with the file
/// named.
pub fn load_library(dir: &Path) -> Result<(PartsLibrary, Fingerprint)> {
    if !is_library(dir) {
        return Err(ProjectError::NotALibrary(dir.to_owned(), "it has no library.json"));
    }
    if dir.join(store::JOURNAL).exists() || store::has_staged(dir)? {
        let _lock = store::Lock::take(dir)?;
        store::recover(dir)?;
    }
    let mut before = fingerprint(dir)?;
    for _ in 0..3 {
        let library = read(dir)?;
        let after = fingerprint(dir)?;
        if after == before {
            return Ok((library, after));
        }
        before = after;
    }
    Err(ProjectError::Busy(dir.to_owned()))
}

fn read(dir: &Path) -> Result<PartsLibrary> {
    let path = dir.join(LIBRARY_FILE);
    let head: LibraryFile = read_bounded_json(&path)?;
    if head.format > LIBRARY_FORMAT {
        return Err(ProjectError::Format { path, found: head.format });
    }
    let mut library = PartsLibrary::new(&head.name);
    let parts_dir = dir.join("parts");
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(&parts_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|x| x == "json")
                    && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(io(&parts_dir)(e)),
    };
    paths.sort();
    for path in paths {
        for part in read_bounded_json::<Vec<Part>>(&path)? {
            let label = format!("{} {}", part.manufacturer, part.number).trim().to_owned();
            if library.get(&part.manufacturer, &part.number).is_some() {
                return Err(ProjectError::InvalidLibrary(path, format!("{label} appears twice")));
            }
            library.insert(part).map_err(|e| ProjectError::InvalidLibrary(path.clone(), format!("{label}: {e}")))?;
        }
    }
    Ok(library)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ecad_model::Datum;

    #[test]
    fn manufacturer_file_names() {
        assert_eq!(parts_file("Phoenix Contact"), "parts/phoenix-contact.json");
        assert_eq!(parts_file("  ABB "), "parts/abb.json");
        assert_eq!(parts_file("Weidmüller"), "parts/weidm-ller.json");
        assert_eq!(parts_file("../.."), "parts/no-manufacturer.json");
        assert_eq!(parts_file(""), "parts/no-manufacturer.json");
        assert!(parts_file(&"x".repeat(500)).len() < 100);
    }

    #[test]
    fn a_library_saves_by_manufacturer_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("components");
        let start = create_library(&lib_dir, "Workshop").unwrap();
        assert!(create_library(&lib_dir, "Again").is_err());
        let (mut lib, at) = load_library(&lib_dir).unwrap();
        assert_eq!((lib.name.as_str(), lib.len(), at), ("Workshop", 0, start));

        let k = Part { data: vec![Datum::new("Rated current", "9 A")], ..Part::new("EX-CT-9", "Example") };
        lib.import(vec![
            k,
            Part::new("EX-CT-12", "Example"),
            Part::new("MCB-C16", "Other Maker"),
            Part::new("H-1", ""),
        ]);
        let at = save_library(&lib, &lib_dir, Some(at)).unwrap();
        for f in ["parts/example.json", "parts/other-maker.json", "parts/no-manufacturer.json"] {
            assert!(lib_dir.join(f).is_file(), "{f}");
        }
        let text = std::fs::read_to_string(lib_dir.join("parts/example.json")).unwrap();
        assert!(text.find("EX-CT-12").unwrap() < text.find("EX-CT-9").unwrap(), "sorted by number");
        let (back, at2) = load_library(&lib_dir).unwrap();
        assert_eq!((back, at2), (lib.clone(), at));

        // A maker whose last part goes loses its file.
        lib.remove("Other Maker", "MCB-C16");
        let at = save_library(&lib, &lib_dir, Some(at)).unwrap();
        assert!(!lib_dir.join("parts/other-maker.json").exists());

        // Someone else's save in between is refused, and leaves their save in place.
        let mut theirs = lib.clone();
        theirs.remove("", "H-1");
        save_library(&theirs, &lib_dir, None).unwrap();
        assert!(matches!(save_library(&lib, &lib_dir, Some(at)), Err(ProjectError::Changed(_))));
        assert_eq!(load_library(&lib_dir).unwrap().0, theirs);
    }

    #[test]
    fn bad_libraries_are_named() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(load_library(dir.path()), Err(ProjectError::NotALibrary(..))));
        create_library(dir.path(), "x").unwrap();
        let twice = r#"[{"number":"A","manufacturer":"M"},{"number":"a ","manufacturer":"m"}]"#;
        std::fs::write(dir.path().join("parts/m.json"), twice).unwrap();
        let e = load_library(dir.path()).unwrap_err().to_string();
        assert!(e.contains("m.json") && e.contains("appears twice"), "{e}");
        let bad = r#"[{"number":"A","data":[{"name":"x","value":"1"},{"name":"X","value":"2"}]}]"#;
        std::fs::write(dir.path().join("parts/m.json"), bad).unwrap();
        assert!(load_library(dir.path()).unwrap_err().to_string().contains("appears twice"));

        // A project folder is never made or saved as a library, nor the other way round.
        let project = dir.path().join("project");
        crate::save(&ecad_model::Project::new("p"), &project).unwrap();
        assert!(create_library(&project, "x").is_err());
        assert!(save_library(&PartsLibrary::new("x"), &project, None).is_err());
        assert!(crate::save(&ecad_model::Project::new("p"), dir.path()).is_err());
    }
}
