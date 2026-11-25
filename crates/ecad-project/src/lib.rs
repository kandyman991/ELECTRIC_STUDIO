//! A project on disk is a folder of pretty-printed JSON files, so it diffs cleanly in git and
//! scripts or agents can read it:
//!
//! ```text
//! my-panel/
//!   project.json          format version, name, pages
//!   devices.json          all devices, sorted by tag
//!   symbols/<id>.json     one file per symbol used by the project
//!   pages/<page-id>.json  the functions and connections on that page
//! ```
//!
//! Loading replays everything through the model's commands, so a hand-edited file that breaks
//! a rule (duplicate tag, wire to a missing point) is rejected with the file named.

use ecad_model::{Command, Connection, Device, Function, ModelError, Page, Point, Project, Sheet, SymbolDef};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
    #[error("{path}: {source}")]
    Model { path: PathBuf, source: ModelError },
    #[error("{path}: project format {found} is newer than this program understands ({FORMAT})")]
    Format { path: PathBuf, found: u32 },
    #[error("symbol id `{0}` can't be used as a file name (letters, digits, `.`, `_`, `-` only)")]
    BadSymbolId(String),
}

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: u32,
    name: String,
    /// Missing in projects saved before M2a; such projects get a sheet around their drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sheet: Option<Sheet>,
    pages: Vec<Page>,
}

#[derive(Serialize, Deserialize, Default)]
struct PageFile {
    functions: Vec<Function>,
    connections: Vec<Connection>,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io { path: path.to_owned(), source }
}

fn model(path: &Path) -> impl FnOnce(ModelError) -> ProjectError + '_ {
    move |source| ProjectError::Model { path: path.to_owned(), source }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    serde_json::from_str(&text).map_err(|source| ProjectError::Json { path: path.to_owned(), source })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|source| ProjectError::Json { path: path.to_owned(), source })?;
    text.push('\n');
    // Write next to the target and rename, so a crash never leaves a half-written file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io(path))
}

fn symbol_file_name(id: &str) -> Result<String> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(format!("{id}.json")) } else { Err(ProjectError::BadSymbolId(id.to_owned())) }
}

/// Remove `*.json` files in `dir` that aren't in `keep`.
fn remove_stale(dir: &Path, keep: &[String]) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(io(dir))? {
        let path = entry.map_err(io(dir))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        if name.ends_with(".json") && !keep.contains(&name) {
            std::fs::remove_file(&path).map_err(io(&path))?;
        }
    }
    Ok(())
}

/// Write the project into `dir` (created if needed). Files for removed pages or symbols are
/// deleted; nothing else in the folder is touched.
pub fn save(project: &Project, dir: &Path) -> Result<()> {
    let (pages_dir, symbols_dir) = (dir.join("pages"), dir.join("symbols"));
    for d in [dir, &pages_dir, &symbols_dir] {
        std::fs::create_dir_all(d).map_err(io(d))?;
    }

    let pages: Vec<Page> = project.pages_in_order().into_iter().cloned().collect();
    let head = ProjectFile { format: FORMAT, name: project.name.clone(), sheet: Some(project.sheet), pages };
    write_json(&dir.join("project.json"), &head)?;

    let mut devices: Vec<&Device> = project.devices.values().collect();
    devices.sort_by(|a, b| a.tag.cmp(&b.tag));
    write_json(&dir.join("devices.json"), &devices)?;

    let mut symbol_files = Vec::new();
    for s in project.symbols.values() {
        let name = symbol_file_name(&s.id.0)?;
        write_json(&symbols_dir.join(&name), s)?;
        symbol_files.push(name);
    }
    remove_stale(&symbols_dir, &symbol_files)?;

    let mut page_files = Vec::new();
    for page in project.pages.keys() {
        let file = PageFile {
            functions: project.functions_on(*page).cloned().collect(),
            connections: project.connections_on(*page).cloned().collect(),
        };
        let name = format!("{}.json", page.0);
        write_json(&pages_dir.join(&name), &file)?;
        page_files.push(name);
    }
    remove_stale(&pages_dir, &page_files)
}

/// Read a project folder written by [`save`].
pub fn load(dir: &Path) -> Result<Project> {
    let path = dir.join("project.json");
    let head: ProjectFile = read_json(&path)?;
    if head.format > FORMAT {
        return Err(ProjectError::Format { path, found: head.format });
    }
    let mut project = Project::new(&head.name);
    for page in head.pages {
        project.add_page(page);
    }
    let sheet = head.sheet;

    let symbols_dir = dir.join("symbols");
    let mut symbol_paths: Vec<PathBuf> = std::fs::read_dir(&symbols_dir)
        .map_err(io(&symbols_dir))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    symbol_paths.sort();
    for p in symbol_paths {
        project.add_symbol(read_json::<SymbolDef>(&p)?);
    }

    let path = dir.join("devices.json");
    for d in read_json::<Vec<Device>>(&path)? {
        project.apply(Command::AddDevice(d)).map_err(model(&path))?;
    }

    // Place every page's functions before any connection, then connect.
    let mut pages = Vec::new();
    for id in project.pages.keys().copied().collect::<Vec<_>>() {
        let path = dir.join("pages").join(format!("{}.json", id.0));
        let file: PageFile = read_json(&path)?;
        for f in &file.functions {
            project.apply(Command::Place(f.clone())).map_err(model(&path))?;
        }
        pages.push((path, file.connections));
    }
    for (path, connections) in pages {
        for c in connections {
            project.apply(Command::Connect(c)).map_err(model(&path))?;
        }
    }
    project.sheet = match sheet {
        Some(s) => s,
        None => legacy_sheet(&project),
    };
    Ok(project)
}

/// A sheet for a project saved before sheets existed: placed around everything drawn, so no
/// page content ends up outside the frame.
fn legacy_sheet(project: &Project) -> Sheet {
    let boxes: Vec<_> = project.pages.keys().filter_map(|p| project.content_bounds(*p)).collect();
    let Some(&(mut lo, mut hi)) = boxes.first() else { return Sheet::default() };
    for (a, b) in &boxes[1..] {
        lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Point::new(hi.x.max(b.x), hi.y.max(b.y));
    }
    Sheet::around(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::sample::start_circuit;

    #[test]
    fn save_then_load_gives_the_same_project() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        save(&original, dir.path()).unwrap();
        assert_eq!(load(dir.path()).unwrap(), original);
        // Saving again writes identical bytes (stable order, no timestamps).
        let first = std::fs::read(dir.path().join("devices.json")).unwrap();
        save(&load(dir.path()).unwrap(), dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("devices.json")).unwrap(), first);
    }

    #[test]
    fn removed_pages_lose_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = start_circuit().unwrap();
        save(&p, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("pages")).unwrap().count(), 1);
        let ids: Vec<_> = p.functions.keys().copied().collect();
        for f in ids {
            p.apply(Command::Unplace(f)).unwrap();
        }
        p.pages.clear();
        save(&p, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("pages")).unwrap().count(), 0);
    }

    #[test]
    fn a_broken_file_is_rejected_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        save(&start_circuit().unwrap(), dir.path()).unwrap();
        let devices = dir.path().join("devices.json");
        let text = std::fs::read_to_string(&devices).unwrap().replace("\"-S1\"", "\"-K1\"");
        std::fs::write(&devices, text).unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("devices.json") && err.contains("-K1"), "{err}");
    }

    #[test]
    fn projects_saved_before_sheets_get_one_around_their_drawing() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        save(&original, dir.path()).unwrap();
        let head = dir.path().join("project.json");
        let mut json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&head).unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("sheet");
        std::fs::write(&head, json.to_string()).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.sheet, original.sheet, "the same placement the sample itself uses");
    }

    #[test]
    fn symbol_ids_must_be_safe_file_names() {
        assert_eq!(symbol_file_name("iec.coil").unwrap(), "iec.coil.json");
        assert!(symbol_file_name("../evil").is_err());
        assert!(symbol_file_name("").is_err());
    }
}
