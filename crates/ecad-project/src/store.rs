//! Saving a project folder as one step, and one saver at a time.
//!
//! A project is several files, and a save may change, add and delete many of them. To never
//! leave a mix of old and new files (an old page naming a device the new `devices.json` no
//! longer has), a save goes:
//!
//! 1. every new file is written beside its target as `<name>.ecad-new`, and flushed to disk;
//! 2. a journal, `.ecad-save.json`, listing the files to move into place and those to delete,
//!    is written to a temporary name and renamed: that rename is the moment the save happens;
//! 3. the files are moved into place and stale ones deleted, then the journal is removed.
//!
//! [`recover`] runs before every load and save. A journal means step 2 finished: the save is
//! completed (every staged file is whole, so this can be repeated after another failure). No
//! journal means it didn't: staged files are deleted and the folder is the project before the
//! save. Either way it loads as one complete revision.
//!
//! `.ecad-lock`, created exclusively for the length of a save, keeps two programs (the editor
//! and the MCP server) from saving the same folder at once; one left behind by a crash is
//! taken over after [`STALE_LOCK`].

use crate::{ProjectError, Result, io};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The journal of a save in progress.
pub const JOURNAL: &str = ".ecad-save.json";
/// Held while a program saves the folder.
pub const LOCK: &str = ".ecad-lock";
/// A new file waiting to replace its target.
pub const STAGED: &str = ".ecad-new";
/// A lock older than this is left over from a crash.
pub const STALE_LOCK: Duration = Duration::from_secs(120);

/// The folders that hold project files, besides the project folder itself.
const SUBDIRS: [&str; 3] = ["pages", "symbols", "underlays"];
/// Every folder a save stages files in: the project's, the manifest's and the workflow's.
const STAGE_DIRS: [&str; 7] = ["pages", "symbols", "underlays", ".ecad", "proposals", "approvals", "changes"];

#[derive(Serialize, Deserialize)]
struct Journal {
    /// Files to move from `<name>.ecad-new` to `<name>`, by path inside the folder.
    replace: Vec<String>,
    /// Stale files to delete.
    delete: Vec<String>,
}

#[cfg(test)]
thread_local! {
    /// For tests: fail the n-th file system change from now (0 = the next one).
    pub(crate) static FAIL_AT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Before each change to the folder: in tests, where an injected failure happens.
pub(crate) fn step(path: &Path) -> Result<()> {
    #[cfg(test)]
    {
        let fail = FAIL_AT.with(|f| match f.get() {
            Some(0) => {
                f.set(None);
                true
            }
            Some(n) => {
                f.set(Some(n - 1));
                false
            }
            None => false,
        });
        if fail {
            return Err(io(path)(std::io::Error::other("injected failure")));
        }
    }
    let _ = path;
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    step(path)?;
    let mut f = std::fs::File::create(path).map_err(io(path))?;
    f.write_all(bytes).map_err(io(path))?;
    f.sync_all().map_err(io(path))
}

/// Make renames in `dir` durable (Unix; Windows has no directory handles to flush).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    let _ = dir;
}

fn rename(from: &Path, to: &Path) -> Result<()> {
    step(to)?;
    std::fs::rename(from, to).map_err(io(to))
}

fn remove(path: &Path) -> Result<()> {
    step(path)?;
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(path)(e)),
        _ => Ok(()),
    }
}

/// Read a file in 64 KB pieces, never holding it whole.
pub(crate) fn each_chunk(path: &Path, mut f: impl FnMut(&[u8])) -> Result<()> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(io(path))?;
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = file.read(&mut buf).map_err(io(path))?;
        if n == 0 {
            return Ok(());
        }
        f(&buf[..n]);
    }
}

/// Whether the file at `path` holds exactly `bytes`, compared in pieces.
fn same_contents(path: &Path, bytes: &[u8]) -> bool {
    if std::fs::metadata(path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        return false;
    }
    let mut at = 0;
    let mut same = true;
    let read = each_chunk(path, |chunk| {
        let end = at + chunk.len();
        same &= bytes.get(at..end) == Some(chunk);
        at = end;
    });
    read.is_ok() && same && at == bytes.len()
}

/// The names of files in `dir` (one level), `None` if it doesn't exist.
fn names_in(dir: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(dir)(e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io(dir))?;
        if entry.file_type().map_err(io(dir))?.is_file() {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(out)
}

/// The project's files, by path inside the folder: the `.json` files at the top and in
/// `pages/` and `symbols/` (not hidden ones like the journal).
/// Top-level files of earlier formats that a save of this format replaces, and so removes:
/// format 1's `parts.json` (now `products.json`).
pub const RETIRED: [&str; 1] = ["parts.json"];

pub fn project_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for sub in std::iter::once("").chain(SUBDIRS) {
        for name in names_in(&dir.join(sub))? {
            if name.ends_with(".json") && !name.starts_with('.') {
                out.push(if sub.is_empty() { name } else { format!("{sub}/{name}") });
            }
        }
    }
    Ok(out)
}

/// Staged files left by a save that stopped part way.
fn staged_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for sub in std::iter::once("").chain(STAGE_DIRS) {
        for name in names_in(&dir.join(sub))? {
            if name.ends_with(STAGED) || name == format!("{JOURNAL}.tmp") {
                out.push(if sub.is_empty() { name } else { format!("{sub}/{name}") });
            }
        }
    }
    Ok(out)
}

/// Whether a save left staged files behind.
pub fn has_staged(dir: &Path) -> Result<bool> {
    Ok(!staged_files(dir)?.is_empty())
}

fn staged(name: &str) -> String {
    format!("{name}{STAGED}")
}

/// Write `files` (path inside the folder, contents) as the project, deleting other project
/// files in `pages/` and `symbols/`. The caller holds the [`Lock`].
pub(crate) fn commit(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    let keep: std::collections::BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let delete: Vec<String> = project_files(dir)?
        .into_iter()
        .filter(|n| {
            (SUBDIRS.iter().any(|s| n.starts_with(&format!("{s}/"))) || RETIRED.contains(&n.as_str()))
                && !keep.contains(n.as_str())
        })
        .collect();
    // Unchanged files needn't move: fewer steps, and the same revision either way.
    let mut replace = Vec::new();
    for (name, bytes) in files {
        let target = dir.join(name);
        if same_contents(&target, bytes) {
            continue;
        }
        write_synced(&dir.join(staged(name)), bytes)?;
        replace.push(name.clone());
    }
    if replace.is_empty() && delete.is_empty() {
        return Ok(());
    }
    let journal = Journal { replace, delete };
    let text =
        serde_json::to_vec_pretty(&journal).map_err(|source| ProjectError::Json { path: dir.join(JOURNAL), source })?;
    let tmp = dir.join(format!("{JOURNAL}.tmp"));
    write_synced(&tmp, &text)?;
    // The save happens here.
    rename(&tmp, &dir.join(JOURNAL))?;
    sync_dir(dir);
    apply(dir, &journal)
}

/// Write `files` (path inside the folder, contents) as one step, deleting nothing: a restore
/// of single files. The caller holds the [`Lock`].
pub(crate) fn commit_partial(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    let mut replace = Vec::new();
    for (name, bytes) in files {
        if let Some(parent) = dir.join(name).parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        write_synced(&dir.join(staged(name)), bytes)?;
        replace.push(name.clone());
    }
    if replace.is_empty() {
        return Ok(());
    }
    let journal = Journal { replace, delete: Vec::new() };
    let text =
        serde_json::to_vec_pretty(&journal).map_err(|source| ProjectError::Json { path: dir.join(JOURNAL), source })?;
    let tmp = dir.join(format!("{JOURNAL}.tmp"));
    write_synced(&tmp, &text)?;
    rename(&tmp, &dir.join(JOURNAL))?;
    sync_dir(dir);
    apply(dir, &journal)
}

/// Carry out a written journal; safe to repeat.
fn apply(dir: &Path, journal: &Journal) -> Result<()> {
    for name in &journal.replace {
        let from = dir.join(staged(name));
        if from.exists() {
            rename(&from, &dir.join(name))?;
        }
    }
    for name in &journal.delete {
        remove(&dir.join(name))?;
    }
    for sub in STAGE_DIRS {
        sync_dir(&dir.join(sub));
    }
    remove(&dir.join(JOURNAL))?;
    sync_dir(dir);
    Ok(())
}

/// Finish a save that got as far as its journal, or undo one that didn't. The caller holds
/// the [`Lock`].
pub fn recover(dir: &Path) -> Result<()> {
    let path = dir.join(JOURNAL);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let journal: Journal =
                serde_json::from_slice(&bytes).map_err(|source| ProjectError::Json { path: path.clone(), source })?;
            apply(dir, &journal)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(&path)(e)),
    }
    for name in staged_files(dir)? {
        remove(&dir.join(name))?;
    }
    Ok(())
}

/// The folder's save lock, released when dropped.
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    /// Take the lock, or fail with [`ProjectError::Busy`] while another program holds it.
    pub fn take(dir: &Path) -> Result<Lock> {
        let path = dir.join(LOCK);
        for attempt in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = writeln!(f, "{}", std::process::id());
                    return Ok(Lock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
                    if age.is_some_and(|a| a > STALE_LOCK) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    return Err(ProjectError::Busy(dir.to_owned()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(ProjectError::Busy(dir.to_owned()));
                }
                Err(e) => return Err(io(&path)(e)),
            }
        }
        Err(ProjectError::Busy(dir.to_owned()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
