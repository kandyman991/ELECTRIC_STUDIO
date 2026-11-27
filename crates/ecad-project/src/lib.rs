//! A project on disk is a folder of pretty-printed JSON files, so it diffs cleanly in git and
//! scripts or agents can read it:
//!
//! ```text
//! my-panel/
//!   project.json          format version, name, pages
//!   devices.json          all devices, sorted by tag
//!   parts.json            the parts catalogue, sorted by part number
//!   cables.json           cables, sorted by tag
//!   symbols/<id>.json     one file per symbol used by the project
//!   pages/<page-id>.json  the functions and connections on that page
//! ```
//!
//! Loading replays everything through the model's commands, so a hand-edited file that breaks
//! a rule (duplicate tag, wire to a missing point) is rejected with the file named.
//!
//! Saving writes the whole folder as one step, one program at a time, and can refuse to save
//! over someone else's save ([`save_checked`]): see [`store`].

use ecad_model::{
    Cable, Command, Connection, Device, Function, LocationBox, ModelError, Naming, Page, Part, Point, Project,
    ProjectInfo, Sheet, SymbolDef,
};

pub mod csv;
pub mod store;
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
    #[error("{0}: the project changed on disk since it was opened (saved by another program?); not saved")]
    Changed(PathBuf),
    #[error("{0}: another program is saving this project right now; try again")]
    Busy(PathBuf),
}

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: u32,
    name: String,
    /// Missing in projects saved before M2a; such projects get a sheet around their drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sheet: Option<Sheet>,
    /// Title-block fields; missing in projects saved before G1.
    #[serde(default, skip_serializing_if = "is_default")]
    info: ProjectInfo,
    /// Tag and wire number formats; missing in projects saved before G1b.
    #[serde(default, skip_serializing_if = "is_default_naming")]
    naming: Naming,
    #[serde(default, skip_serializing_if = "is_default_wire_lengths")]
    wire_lengths: ecad_model::WireLengthSettings,
    pages: Vec<Page>,
}

fn is_default_wire_lengths(s: &ecad_model::WireLengthSettings) -> bool {
    *s == ecad_model::WireLengthSettings::default()
}

fn is_default_naming(n: &Naming) -> bool {
    *n == Naming::default()
}

fn is_default(info: &ProjectInfo) -> bool {
    *info == ProjectInfo::default()
}

#[derive(Serialize, Deserialize, Default)]
struct PageFile {
    functions: Vec<Function>,
    connections: Vec<Connection>,
    /// Location boxes; missing in projects saved before G4b.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    boxes: Vec<LocationBox>,
    /// A layout page's mounting plate and what is on it; missing before M5a.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    panel: Option<ecad_model::Panel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    panel_items: Vec<ecad_model::PanelItem>,
}

pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io { path: path.to_owned(), source }
}

fn model(path: &Path) -> impl FnOnce(ModelError) -> ProjectError + '_ {
    move |source| ProjectError::Model { path: path.to_owned(), source }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    serde_json::from_str(&text).map_err(|source| ProjectError::Json { path: path.to_owned(), source })
}

fn render_json<T: Serialize>(path: &Path, value: &T) -> Result<Vec<u8>> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|source| ProjectError::Json { path: path.to_owned(), source })?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn symbol_file_name(id: &str) -> Result<String> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(format!("{id}.json")) } else { Err(ProjectError::BadSymbolId(id.to_owned())) }
}

/// Every file of a project, as (path inside the folder, contents).
fn render(project: &Project) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    let pages: Vec<Page> = project.pages_in_order().into_iter().cloned().collect();
    let head = ProjectFile {
        format: FORMAT,
        name: project.name.clone(),
        sheet: Some(project.sheet),
        info: project.info.clone(),
        naming: project.naming.clone(),
        wire_lengths: project.wire_length_settings,
        pages,
    };
    files.push(("project.json".to_owned(), render_json(Path::new("project.json"), &head)?));

    let parts: Vec<&Part> = project.parts.values().collect();
    files.push(("parts.json".to_owned(), render_json(Path::new("parts.json"), &parts)?));

    let mut cables: Vec<&Cable> = project.cables.values().collect();
    cables.sort_by(|a, b| a.tag.cmp(&b.tag));
    files.push(("cables.json".to_owned(), render_json(Path::new("cables.json"), &cables)?));

    let mut devices: Vec<&Device> = project.devices.values().collect();
    devices.sort_by(|a, b| a.tag.cmp(&b.tag));
    files.push(("devices.json".to_owned(), render_json(Path::new("devices.json"), &devices)?));

    for s in project.symbols.values() {
        let name = format!("symbols/{}", symbol_file_name(&s.id.0)?);
        let bytes = render_json(Path::new(&name), s)?;
        files.push((name, bytes));
    }
    for page in project.pages.keys() {
        let file = PageFile {
            functions: project.functions_on(*page).cloned().collect(),
            connections: project.connections_on(*page).cloned().collect(),
            boxes: project.boxes_on(*page).cloned().collect(),
            panel: project.panels.get(page).copied(),
            panel_items: project.panel_items_on(*page).into_iter().cloned().collect(),
        };
        let name = format!("pages/{}.json", page.0);
        let bytes = render_json(Path::new(&name), &file)?;
        files.push((name, bytes));
    }
    Ok(files)
}

/// Write the project into `dir` (created if needed). Files for removed pages or symbols are
/// deleted; nothing else in the folder is touched.
///
/// The save is all or nothing: if it stops part way (a full disk, a crash), the folder loads
/// as the project before the save, or once the save's journal is written, as the project after
/// it. See [`store`].
pub fn save(project: &Project, dir: &Path) -> Result<()> {
    save_checked(project, dir, None).map(|_| ())
}

/// Save, but only if the folder is still as it was when it had `expected` (a
/// [`fingerprint`]): another program's save in between is refused with
/// [`ProjectError::Changed`], and the folder is left alone. Returns the new fingerprint.
pub fn save_checked(project: &Project, dir: &Path, expected: Option<Fingerprint>) -> Result<Fingerprint> {
    let files = render(project)?;
    for d in [dir.to_owned(), dir.join("pages"), dir.join("symbols")] {
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

/// What a project folder holds now, to tell whether someone else saved over it: a hash of
/// every project file's name and contents. An empty or missing folder has one too.
pub type Fingerprint = u64;

/// The folder's [`Fingerprint`].
pub fn fingerprint(dir: &Path) -> Result<Fingerprint> {
    use std::hash::{Hash, Hasher};
    let mut names = store::project_files(dir)?;
    names.sort();
    // The same in every run of this program (fixed keys), which is all a comparison needs.
    let mut h = std::hash::DefaultHasher::new();
    for name in names {
        let path = dir.join(&name);
        let bytes = std::fs::read(&path).map_err(io(&path))?;
        name.hash(&mut h);
        bytes.hash(&mut h);
    }
    Ok(h.finish())
}

/// Load, and the folder's fingerprint as loaded, for a later [`save_checked`]. If another
/// program saves while this loads, it loads again, so the fingerprint is that of what loaded.
pub fn load_checked(dir: &Path) -> Result<(Project, Fingerprint)> {
    let mut before = fingerprint(dir)?;
    for _ in 0..3 {
        let project = load(dir)?;
        let after = fingerprint(dir)?;
        if after == before {
            return Ok((project, after));
        }
        before = after;
    }
    Err(ProjectError::Busy(dir.to_owned()))
}

/// Read a project folder written by [`save`].
pub fn load(dir: &Path) -> Result<Project> {
    // A save that stopped part way is finished (if it got as far as its journal) or undone.
    if dir.join(store::JOURNAL).exists() || store::has_staged(dir)? {
        let _lock = store::Lock::take(dir)?;
        store::recover(dir)?;
    }
    let path = dir.join("project.json");
    let head: ProjectFile = read_json(&path)?;
    if head.format > FORMAT {
        return Err(ProjectError::Format { path, found: head.format });
    }
    let mut project = Project::new(&head.name);
    project.info = head.info;
    // Before the wires load, so per-potential numbering allows shared numbers.
    project.naming = head.naming;
    // Through the command, so a hand-edited file can't bring in settings the editor would refuse.
    project.apply(Command::SetWireLengthSettings(head.wire_lengths)).map_err(model(&path))?;
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

    // The catalogue comes before the devices that use it; projects saved before M3a have none.
    let path = dir.join("parts.json");
    if path.exists() {
        for part in read_json::<Vec<Part>>(&path)? {
            project.apply(Command::AddPart(part)).map_err(model(&path))?;
        }
    }

    // Cables before the wires that run in them; projects saved before G4 have none.
    let path = dir.join("cables.json");
    if path.exists() {
        for c in read_json::<Vec<Cable>>(&path)? {
            project.apply(Command::AddCable(c)).map_err(model(&path))?;
        }
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
        for b in &file.boxes {
            project.apply(Command::AddBox(b.clone())).map_err(model(&path))?;
        }
        if let Some(panel) = file.panel {
            project.apply(Command::SetPanel { page: id, panel: Some(panel) }).map_err(model(&path))?;
        }
        for item in &file.panel_items {
            project.apply(Command::AddPanelItem(item.clone())).map_err(model(&path))?;
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

    /// Two revisions of one project: `a`, then `b` with a page, its devices and wires gone
    /// and a device added, so a mix of their files would not load as either.
    fn two_revisions() -> (Project, Project) {
        let a = ecad_model::sample::with_plc().unwrap();
        let mut b = a.clone();
        let last = b.pages_in_order().last().unwrap().id;
        let on_last: Vec<_> = b.functions_on(last).map(|f| (f.id, f.device)).collect();
        let mut cmds: Vec<Command> = on_last.iter().map(|(f, _)| Command::Unplace(*f)).collect();
        cmds.push(Command::RemovePage(last));
        b.apply(Command::Batch(cmds)).unwrap();
        let orphans: Vec<_> = on_last.iter().map(|(_, d)| *d).filter(|d| b.functions_of(*d).next().is_none()).collect();
        for d in orphans {
            if b.devices.contains_key(&d) {
                b.apply(Command::RemoveDevice(d)).unwrap();
            }
        }
        b.apply(Command::AddDevice(ecad_model::Device::new(ecad_model::DeviceId::new(), "-K77", "added"))).unwrap();
        (a, b)
    }

    #[test]
    fn a_save_stopped_at_any_step_loads_as_the_old_or_the_new_project() {
        let (a, b) = two_revisions();
        let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        save(&a, a_dir.path()).unwrap();
        save(&b, b_dir.path()).unwrap();
        let (old, new) = (load(a_dir.path()).unwrap(), load(b_dir.path()).unwrap());
        assert_ne!(old, new);
        let (mut saw_old, mut saw_new) = (false, false);
        for n in 0.. {
            let dir = tempfile::tempdir().unwrap();
            save(&a, dir.path()).unwrap();
            store::FAIL_AT.with(|f| f.set(Some(n)));
            let result = save(&b, dir.path());
            store::FAIL_AT.with(|f| f.set(None));
            if result.is_ok() {
                assert_eq!(load(dir.path()).unwrap(), new);
                assert!(n > 5, "the save took {n} steps");
                break;
            }
            // Loading finishes or undoes the save: one whole revision, never a mix.
            let back = load(dir.path()).unwrap_or_else(|e| panic!("step {n}: {e}"));
            if back == old {
                saw_old = true;
            } else {
                assert_eq!(back, new, "step {n}");
                saw_new = true;
            }
            assert!(!dir.path().join(store::JOURNAL).exists() && !store::has_staged(dir.path()).unwrap());
            // And the folder takes the next save.
            save(&b, dir.path()).unwrap();
            assert_eq!(load(dir.path()).unwrap(), new);
        }
        assert!(saw_old && saw_new, "failures before and after the journal were both tried");
    }

    #[test]
    fn a_failure_while_recovering_is_recovered_next_time() {
        let (a, b) = two_revisions();
        let dir = tempfile::tempdir().unwrap();
        save(&a, dir.path()).unwrap();
        // Stop just after the journal: the files are staged and the save has happened.
        let staged = store::project_files(dir.path()).unwrap().len();
        let mut n = 0;
        loop {
            store::FAIL_AT.with(|f| f.set(Some(n)));
            let r = save(&b, dir.path());
            store::FAIL_AT.with(|f| f.set(None));
            assert!(r.is_err(), "no step after the journal was found");
            if dir.path().join(store::JOURNAL).exists() {
                break;
            }
            n += 1;
            assert!(n < 100 + staged);
        }
        // Recovery itself fails at its first step, then succeeds.
        store::FAIL_AT.with(|f| f.set(Some(0)));
        assert!(load(dir.path()).is_err());
        store::FAIL_AT.with(|f| f.set(None));
        assert_eq!(load(dir.path()).unwrap().device_by_tag("-K77").unwrap().description, "added");
    }

    #[test]
    fn a_save_from_a_stale_copy_is_refused_and_a_clean_save_changes_nothing() {
        let (a, b) = two_revisions();
        let dir = tempfile::tempdir().unwrap();
        save(&a, dir.path()).unwrap();
        let (mine, seen) = load_checked(dir.path()).unwrap();
        let (_, theirs_seen) = load_checked(dir.path()).unwrap();
        // Someone else saves first.
        let after_theirs = save_checked(&b, dir.path(), Some(theirs_seen)).unwrap();
        let e = save_checked(&mine, dir.path(), Some(seen)).unwrap_err();
        assert!(matches!(e, ProjectError::Changed(_)), "{e}");
        assert_eq!(fingerprint(dir.path()).unwrap(), after_theirs, "the folder wasn't touched");
        assert_eq!(load(dir.path()).unwrap().device_by_tag("-K77").map(|d| d.tag.clone()), Some("-K77".into()));
        // Saving what is already there writes nothing.
        let before = std::fs::metadata(dir.path().join("devices.json")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again = save_checked(&load(dir.path()).unwrap(), dir.path(), Some(after_theirs)).unwrap();
        assert_eq!(again, after_theirs);
        assert_eq!(std::fs::metadata(dir.path().join("devices.json")).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn one_saver_at_a_time_and_a_crashed_savers_lock_is_taken_over() {
        let p = ecad_model::sample::start_circuit().unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&p, dir.path()).unwrap();
        let held = store::Lock::take(dir.path()).unwrap();
        assert!(matches!(save(&p, dir.path()), Err(ProjectError::Busy(_))));
        drop(held);
        save(&p, dir.path()).unwrap();
        // A lock file from long ago (a crash) doesn't block.
        let lock = std::fs::File::create(dir.path().join(store::LOCK)).unwrap();
        let old = std::time::SystemTime::now() - store::STALE_LOCK - std::time::Duration::from_secs(5);
        lock.set_modified(old).unwrap();
        drop(lock);
        save(&p, dir.path()).unwrap();
        assert!(!dir.path().join(store::LOCK).exists());
    }

    use ecad_model::sample::start_circuit;

    #[test]
    fn save_then_load_gives_the_same_project() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        save(&original, dir.path()).unwrap();
        assert_eq!(load(dir.path()).unwrap(), original);
        // Everything the samples use survives too: parts, terminals, PLC properties, the
        // sheet's rows, the title block and page subtitles.
        let plc = ecad_model::sample::with_plc().unwrap();
        let other = tempfile::tempdir().unwrap();
        save(&plc, other.path()).unwrap();
        assert_eq!(load(other.path()).unwrap(), plc);
        // The reference project: contact blocks, bridges, mirrored drawings, the layout page.
        let mut reference = ecad_model::reference::reference_project().unwrap();
        reference.wire_length_settings = ecad_model::WireLengthSettings { spare_per_end: 200.0, round_to: 100.0 };
        let third = tempfile::tempdir().unwrap();
        save(&reference, third.path()).unwrap();
        assert_eq!(load(third.path()).unwrap(), reference);
        // Saving again writes identical bytes (stable order, no timestamps).
        let first = std::fs::read(dir.path().join("devices.json")).unwrap();
        save(&load(dir.path()).unwrap(), dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("devices.json")).unwrap(), first);
    }

    #[test]
    fn wire_length_settings_out_of_range_are_rejected_on_load() {
        let dir = tempfile::tempdir().unwrap();
        save(&start_circuit().unwrap(), dir.path()).unwrap();
        let path = dir.path().join("project.json");
        let mut head: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for bad in [
            serde_json::json!({"spare_per_end": -500.0, "round_to": 50.0}),
            serde_json::json!({"spare_per_end": 150.0, "round_to": -1.0}),
            serde_json::json!({"spare_per_end": 1e300, "round_to": 1e-300}),
        ] {
            head["wire_lengths"] = bad;
            std::fs::write(&path, serde_json::to_vec(&head).unwrap()).unwrap();
            let e = load(dir.path()).unwrap_err().to_string();
            assert!(e.contains("project.json") && e.contains("spare length"), "{e}");
        }
        head["wire_lengths"] = serde_json::json!({"spare_per_end": 0.0, "round_to": 0.0});
        std::fs::write(&path, serde_json::to_vec(&head).unwrap()).unwrap();
        assert_eq!(load(dir.path()).unwrap().wire_length_settings.round_to, 0.0);
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
    fn the_catalogue_is_saved_and_projects_without_one_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let original = start_circuit().unwrap();
        assert_eq!(original.parts.len(), 3);
        save(&original, dir.path()).unwrap();
        assert!(dir.path().join("parts.json").exists());
        assert_eq!(load(dir.path()).unwrap(), original);
        // A project saved before M3a: no parts.json and no parts on devices.
        let mut old = original.clone();
        for d in old.devices.values_mut() {
            d.parts.clear();
        }
        old.parts.clear();
        save(&old, dir.path()).unwrap();
        std::fs::remove_file(dir.path().join("parts.json")).unwrap();
        assert_eq!(load(dir.path()).unwrap(), old);
    }

    #[test]
    fn symbol_ids_must_be_safe_file_names() {
        assert_eq!(symbol_file_name("iec.coil").unwrap(), "iec.coil.json");
        assert!(symbol_file_name("../evil").is_err());
        assert!(symbol_file_name("").is_err());
    }
}
