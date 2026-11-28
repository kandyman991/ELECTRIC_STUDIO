//! The committed state of a project folder (AI0, D43): the content manifest, the retained last
//! committed contents, workflow files, verification, reconciliation and path safety.
//!
//! - **The manifest**, `.ecad/state.json`, lists every engineering file (the project's `.json`
//!   files) and every workflow file (`proposals/`, `approvals/`, `changes/`) with its SHA-256
//!   and size, the project's identity, the committed engineering generation and the
//!   engineering content hash. It is staged in the save's journal ([`crate::store`]), so files
//!   and manifest always describe the same commit. It excludes itself and transaction files.
//! - **Retained contents.** Every engineering file of the committed generation is kept as an
//!   immutable object, `.ecad/objects/<sha256>`, written before the manifest that names it, so
//!   the last committed state can always be reconstructed and compared. Objects no committed
//!   manifest names are pruned after a commit.
//! - **The committed revision** ([`Revision`]) binds the project's identity, the generation and
//!   the content hash. The generation advances when the engineering content changes (an undo
//!   that brings content back to an earlier value still advances it); workflow writes (a
//!   proposal, an approval, a change record) never do.
//! - **Verification** compares the folder with its manifest: modified, missing and added
//!   engineering or workflow files, unexpected files (listed and ignored, never loaded or
//!   deleted), damaged objects and unsafe paths. It runs at opening, at preview and apply of
//!   a change set, and at saving.
//! - **Reconciliation** of outside changes: [`restore`] puts the committed files back
//!   (quarantining what was there), [`accept`] commits the folder as it is as a new generation
//!   with a change record.
//! - **Compatibility.** A folder without a manifest (saved before AI0) loads as it always did;
//!   its first trusted commit adopts it, creating the manifest at generation 1.
//!
//! What this is not: hashes detect changes, they don't establish who made them, and anyone who
//! can rewrite both the files and the manifest defeats them (section 11.8).

use crate::store;
use crate::{ProjectError, Result, io};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

/// The manifest, inside the folder.
pub const MANIFEST: &str = ".ecad/state.json";
/// Retained immutable contents, by SHA-256.
pub const OBJECTS: &str = ".ecad/objects";
/// Files moved aside by a restore, never deleted.
pub const QUARANTINE: &str = ".ecad/quarantine";
/// Workflow folders: proposals, approvals and change records.
pub const WORKFLOW: [&str; 3] = ["proposals", "approvals", "changes"];

/// The committed revision: what an approval and a preview are bound to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision {
    /// The project's identity (from its manifest), empty before it has one.
    pub project: String,
    /// The committed engineering generation, 0 before the first commit.
    pub generation: u64,
    /// The engineering content hash ([`ecad_model::Project::content_hash`]).
    pub content: String,
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}:{}", self.project, self.generation, self.content)
    }
}

impl std::str::FromStr for Revision {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let bad = || format!("`{s}` is not a revision (project@generation:content)");
        let (project, rest) = s.split_once('@').ok_or_else(bad)?;
        let (generation, content) = rest.split_once(':').ok_or_else(bad)?;
        Ok(Revision {
            project: project.into(),
            generation: generation.parse().map_err(|_| bad())?,
            content: content.into(),
        })
    }
}

/// One file of the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub project: String,
    pub generation: u64,
    pub content: String,
    /// Engineering files by path inside the folder.
    pub files: BTreeMap<String, Entry>,
    /// Workflow files by path inside the folder.
    #[serde(default)]
    pub workflow: BTreeMap<String, Entry>,
    /// Older AI0 manifests retained engineering files only.
    #[serde(default)]
    pub retains_workflow: bool,
}

impl Manifest {
    pub fn revision(&self) -> Revision {
        Revision { project: self.project.clone(), generation: self.generation, content: self.content.clone() }
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn file_sha256(path: &Path) -> Result<(String, u64)> {
    let mut h = Sha256::new();
    let mut size = 0u64;
    store::each_chunk(path, |c| {
        size += c.len() as u64;
        h.update(c);
    })?;
    Ok((h.finalize().iter().map(|b| format!("{b:02x}")).collect(), size))
}

/// Windows device names, refused as any path segment's stem on every system.
const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2",
    "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether a path from a manifest (or a restore, an archive) is a plain relative path inside
/// the folder: no absolute paths, drive or UNC prefixes, `..` or `.` segments, backslashes,
/// empty segments or device names (D43).
pub fn check_path(path: &str) -> std::result::Result<(), String> {
    let bad = |why: &str| Err(format!("unsafe path `{path}`: {why}"));
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return bad("only relative paths inside the project folder");
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return bad("no empty, `.` or `..` segments");
        }
        let stem = seg.split('.').next().unwrap_or(seg).to_ascii_lowercase();
        if DEVICES.contains(&stem.as_str()) {
            return bad("a device name");
        }
        if seg.ends_with(' ') || seg.ends_with('.') || seg.chars().any(|c| c.is_control() || "<>\"|?*".contains(c)) {
            return bad("characters not allowed in file names");
        }
    }
    if Path::new(path).components().any(|c| !matches!(c, Component::Normal(_))) {
        return bad("only plain segments");
    }
    Ok(())
}

/// [`check_path`], and on disk: no segment of it is a symbolic link or junction (which could
/// lead outside the folder).
pub fn check_on_disk(dir: &Path, path: &str) -> std::result::Result<(), String> {
    check_path(path)?;
    let mut at = dir.to_path_buf();
    for seg in path.split('/') {
        at.push(seg);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("unsafe path `{path}`: a symbolic link"));
        }
    }
    Ok(())
}

/// Paths that differ only in case, which collide on case-insensitive systems.
fn case_collisions<'a>(paths: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut seen: BTreeMap<String, &String> = BTreeMap::new();
    let mut out = Vec::new();
    for p in paths {
        if let Some(other) = seen.insert(p.to_lowercase(), p)
            && other != p
        {
            out.push(format!("`{other}` and `{p}` differ only in case"));
        }
    }
    out
}

/// Read the manifest: `None` for a folder saved before AI0. Unsafe paths are refused.
pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let path = dir.join(MANIFEST);
    check_on_disk(dir, MANIFEST).map_err(|reason| ProjectError::Canonical { path: path.clone(), reason })?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(&path)(e)),
    };
    let m: Manifest =
        serde_json::from_slice(&bytes).map_err(|source| ProjectError::Json { path: path.clone(), source })?;
    let mut problems: Vec<String> =
        m.files.keys().chain(m.workflow.keys()).filter_map(|p| check_path(p).err()).collect();
    problems.extend(case_collisions(m.files.keys().chain(m.workflow.keys())));
    if let Some(p) = m.files.keys().find(|p| is_workflow(p)) {
        problems.push(format!("`{p}` is a workflow file listed as engineering"));
    }
    for (name, entry) in m.files.iter().chain(&m.workflow) {
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            problems.push(format!("invalid object hash for {name}"));
        }
    }
    for name in m.workflow.keys().filter(|p| !is_workflow(p)) {
        problems.push(format!("`{name}` is not a workflow path"));
    }
    if !problems.is_empty() {
        return Err(ProjectError::Canonical { path, reason: problems.join("; ") });
    }
    Ok(Some(m))
}

/// The committed revision of the folder (generation 0 and no identity before its first commit).
pub fn committed(dir: &Path) -> Result<Revision> {
    Ok(read_manifest(dir)?.map(|m| m.revision()).unwrap_or(Revision {
        project: String::new(),
        generation: 0,
        content: String::new(),
    }))
}

pub fn is_workflow(path: &str) -> bool {
    WORKFLOW.iter().any(|w| path.strip_prefix(w).is_some_and(|r| r.starts_with('/')))
}

/// The workflow files in the folder, by path inside it.
pub fn workflow_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for w in WORKFLOW {
        let d = dir.join(w);
        check_on_disk(dir, w).map_err(|reason| ProjectError::Canonical { path: d.clone(), reason })?;
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&d)(e)),
        };
        for e in entries {
            let e = e.map_err(io(&d))?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map_err(io(&d))?.is_file() && name.ends_with(".json") && !name.starts_with('.') {
                out.push(format!("{w}/{name}"));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// What a folder holds compared with its manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verification {
    /// The committed revision, `None` for a folder saved before AI0.
    pub committed: Option<Revision>,
    /// Engineering or workflow files whose contents differ from the manifest.
    pub modified: Vec<String>,
    /// Files the manifest lists that are gone.
    pub missing: Vec<String>,
    /// Engineering or workflow files the manifest doesn't list.
    pub added: Vec<String>,
    /// Other files: listed and ignored, never loaded, never deleted.
    pub unexpected: Vec<String>,
    /// Retained objects that are missing or don't match their hash: the comparison with the
    /// last commit is incomplete for these files.
    pub damaged_objects: Vec<String>,
    /// Paths refused as unsafe (symbolic links, case collisions).
    pub unsafe_paths: Vec<String>,
}

impl Verification {
    /// Nothing changed outside Electric Studio, and everything committed can be reconstructed.
    pub fn clean(&self) -> bool {
        self.modified.is_empty()
            && self.missing.is_empty()
            && self.added.is_empty()
            && self.damaged_objects.is_empty()
            && self.unsafe_paths.is_empty()
    }
    /// Engineering files changed outside Electric Studio.
    pub fn engineering_changed(&self) -> bool {
        self.modified.iter().chain(&self.missing).chain(&self.added).any(|p| !is_workflow(p))
    }
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (what, list) in [
            ("modified", &self.modified),
            ("missing", &self.missing),
            ("added", &self.added),
            ("damaged retained contents", &self.damaged_objects),
            ("unsafe", &self.unsafe_paths),
            ("unexpected (ignored)", &self.unexpected),
        ] {
            if !list.is_empty() {
                parts.push(format!("{what}: {}", list.join(", ")));
            }
        }
        if parts.is_empty() { "verified: the folder matches its committed manifest".into() } else { parts.join("; ") }
    }
}

/// Reports progress and asks whether to stop, for long operations (D34).
pub trait Progress {
    /// `done` of `total` steps, now on `what`.
    fn step(&self, done: usize, total: usize, what: &str);
    /// Whether the person asked to stop.
    fn cancelled(&self) -> bool;
}

/// No progress reporting, never cancelled.
pub struct Quiet;
impl Progress for Quiet {
    fn step(&self, _: usize, _: usize, _: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Every file under `dir`, by path inside it, except the manifest's own folder, hidden
/// transaction files and hidden folders (`.git`).
fn all_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&d)(e)),
        };
        for e in entries {
            let e = e.map_err(io(&d))?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let t = e.file_type().map_err(io(&d))?;
            if t.is_dir() {
                stack.push((e.path(), rel));
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Compare the folder with its manifest (D43). With progress, cancellable: a cancelled
/// verification returns [`ProjectError::Cancelled`].
pub fn verify(dir: &Path, progress: &dyn Progress) -> Result<Verification> {
    verify_with(dir, progress, true)
}

/// [`verify`] without checking the retained objects: before a commit, which rewrites any
/// object that doesn't match its name, so a damaged one is repaired rather than reported.
pub(crate) fn verify_files(dir: &Path) -> Result<Verification> {
    verify_with(dir, &Quiet, false)
}

fn verify_with(dir: &Path, progress: &dyn Progress, objects: bool) -> Result<Verification> {
    let manifest = read_manifest(dir)?;
    let engineering: BTreeSet<String> = store::project_files(dir)?.into_iter().collect();
    let workflow: BTreeSet<String> = workflow_files(dir)?.into_iter().collect();
    let mut v = Verification { committed: manifest.as_ref().map(Manifest::revision), ..Default::default() };
    let known: BTreeSet<&String> = engineering.iter().chain(&workflow).collect();
    v.unexpected = all_files(dir)?.into_iter().filter(|p| !known.contains(p)).collect();
    v.unsafe_paths = case_collisions(known.iter().copied());
    for p in known.iter() {
        if let Err(e) = check_on_disk(dir, p) {
            v.unsafe_paths.push(e);
        }
    }
    let Some(m) = manifest else { return Ok(v) };
    let listed: BTreeMap<&String, &Entry> = m.files.iter().chain(&m.workflow).collect();
    let retained = m.files.iter().chain(m.workflow.iter().filter(|_| m.retains_workflow));
    let total = listed.len() + if objects { retained.clone().count() } else { 0 };
    let mut done = 0;
    for (path, entry) in &listed {
        if progress.cancelled() {
            return Err(ProjectError::Cancelled);
        }
        progress.step(done, total, path);
        done += 1;
        if let Err(reason) = check_on_disk(dir, path) {
            v.unsafe_paths.push(reason);
            continue;
        }
        let on_disk = dir.join(path);
        if !on_disk.exists() {
            v.missing.push((*path).clone());
            continue;
        }
        match file_sha256(&on_disk) {
            Ok((h, size)) if h == entry.sha256 && size == entry.size => {}
            _ => v.modified.push((*path).clone()),
        }
    }
    v.added = known.iter().filter(|p| !listed.contains_key(*p)).map(|p| (*p).clone()).collect();
    for (path, entry) in retained.filter(|_| objects) {
        if progress.cancelled() {
            return Err(ProjectError::Cancelled);
        }
        progress.step(done, total, path);
        done += 1;
        let object_name = format!("{OBJECTS}/{}", entry.sha256);
        let object = dir.join(&object_name);
        if let Err(reason) = check_on_disk(dir, &object_name) {
            v.unsafe_paths.push(reason);
            continue;
        }
        if !matches!(file_sha256(&object), Ok((h, size)) if h == entry.sha256 && size == entry.size) {
            v.damaged_objects.push(path.clone());
        }
    }
    progress.step(total, total, "done");
    Ok(v)
}

/// The engineering files of the last commit, from the retained objects, each checked against
/// its hash when read (D43): a missing or damaged object is an error naming its file.
pub fn committed_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let m = read_manifest(dir)?.ok_or_else(|| ProjectError::Canonical {
        path: dir.join(MANIFEST),
        reason: "no committed manifest: this folder was saved before AI0".into(),
    })?;
    let mut out = Vec::new();
    let mut damaged = Vec::new();
    for (path, entry) in &m.files {
        let object = dir.join(OBJECTS).join(&entry.sha256);
        match std::fs::read(&object) {
            Ok(bytes) if sha256(&bytes) == entry.sha256 => out.push((path.clone(), bytes)),
            _ => damaged.push(path.clone()),
        }
    }
    if !damaged.is_empty() {
        return Err(ProjectError::Canonical {
            path: dir.join(OBJECTS),
            reason: format!("retained contents missing or damaged for: {}", damaged.join(", ")),
        });
    }
    Ok(out)
}

/// The project as last committed, reconstructed from retained objects (read-only).
pub fn load_committed(dir: &Path) -> Result<ecad_model::Project> {
    let files = committed_files(dir)?;
    let tmp = tempfile::tempdir().map_err(io(&std::env::temp_dir()))?;
    for (path, bytes) in &files {
        let target = tmp.path().join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::write(&target, bytes).map_err(io(&target))?;
    }
    crate::load(tmp.path())
}

/// A commit's extra contents besides the rendered project: workflow files to add or replace,
/// and the identity and generation to give the manifest.
#[derive(Default)]
pub struct Extras {
    pub workflow: Vec<(String, Vec<u8>)>,
}

/// Write objects for `files` that aren't retained yet; synced before the manifest names them.
fn write_objects(dir: &Path, files: &[(String, Entry, &[u8])]) -> Result<()> {
    let objects = dir.join(OBJECTS);
    check_on_disk(dir, OBJECTS).map_err(|reason| ProjectError::Canonical { path: objects.clone(), reason })?;
    std::fs::create_dir_all(&objects).map_err(io(&objects))?;
    let mut written = false;
    for (_, entry, bytes) in files {
        let path = objects.join(&entry.sha256);
        let object_name = format!("{OBJECTS}/{}", entry.sha256);
        for rel in [&object_name, &format!("{object_name}.tmp")] {
            check_on_disk(dir, rel).map_err(|reason| ProjectError::Canonical { path: dir.join(rel), reason })?;
        }
        // Compare with the already verified bytes, not just the size: an equal-length
        // damaged object must be repaired. The comparison avoids a second SHA-256 pass.
        if store::same_contents(&path, bytes) {
            continue;
        }
        let tmp = objects.join(format!("{}.tmp", entry.sha256));
        store::step(&tmp)?;
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).map_err(io(&tmp))?;
            f.write_all(bytes).map_err(io(&tmp))?;
            f.sync_all().map_err(io(&tmp))?;
        }
        std::fs::rename(&tmp, &path).map_err(io(&path))?;
        written = true;
    }
    if written {
        store::sync_dir(&objects);
        store::sync_dir(&dir.join(".ecad"));
    }
    Ok(())
}

/// Remove objects no committed manifest names (never during a pending reconciliation: the
/// manifest still names the prior generation's contents then).
fn prune(dir: &Path, m: &Manifest) {
    let keep: BTreeSet<&str> = m.files.values().chain(m.workflow.values()).map(|e| e.sha256.as_str()).collect();
    if let Ok(entries) = std::fs::read_dir(dir.join(OBJECTS)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !keep.contains(name.as_str()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

fn new_identity() -> String {
    // Derived from the time and process: unique enough for a project's identity, and never
    // used as a secret.
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let h = sha256(format!("{t}-{}-{:?}", std::process::id(), std::thread::current().id()).as_bytes());
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Commit `files` (the rendered engineering files) and `extras` as one transaction, with the
/// manifest. The caller holds the [`store::Lock`] and has recovered and checked the folder.
/// `content` is the engineering content hash of what `files` render. Returns the new
/// committed revision.
/// The engineering content identity of a set of engineering files: the hash of their sorted
/// paths and hashes. Rendering is canonical, so equal engineering content gives an equal
/// digest; a committed revision's generation advances exactly when this changes.
pub(crate) fn digest<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut lines: Vec<String> = files.map(|(p, h)| format!("{p}\0{h}\n")).collect();
    lines.sort();
    format!("sha256:{}", sha256(lines.concat().as_bytes()))
}

///
/// `verified`: the caller checked, under the lock, that every engineering file on disk matches
/// the manifest; a rendered file equal byte for byte to its file on disk then keeps the
/// manifest's hash instead of being hashed again.
pub(crate) fn commit(dir: &Path, files: &[(String, Vec<u8>)], extras: &Extras, verified: bool) -> Result<Revision> {
    commit_inner(dir, files, extras, verified, false)
}

fn commit_inner(
    dir: &Path,
    files: &[(String, Vec<u8>)],
    extras: &Extras,
    verified: bool,
    reconcile: bool,
) -> Result<Revision> {
    let old = read_manifest(dir)?;
    let mut entries = Vec::new();
    for (name, bytes) in files {
        check_path(name).map_err(|reason| ProjectError::Canonical { path: dir.join(name), reason })?;
        let known = old
            .as_ref()
            .filter(|_| verified)
            .and_then(|m| m.files.get(name))
            .filter(|e| e.size == bytes.len() as u64)
            .filter(|_| store::same_contents(&dir.join(name), bytes));
        let entry = match known {
            Some(e) => e.clone(),
            None => Entry { sha256: sha256(bytes), size: bytes.len() as u64 },
        };
        entries.push((name.clone(), entry, bytes.as_slice()));
    }
    let content = digest(entries.iter().map(|(n, e, _)| (n.as_str(), e.sha256.as_str())));
    write_objects(dir, &entries)?;
    let (project, generation) = match &old {
        Some(m) if m.content == content && !reconcile => (m.project.clone(), m.generation),
        Some(m) => (m.project.clone(), m.generation + 1),
        None => (new_identity(), 1),
    };
    let mut workflow = old.as_ref().map(|m| m.workflow.clone()).unwrap_or_default();
    // Pre-AI0 projects did not have approval records. An orphaned workflow
    // without its manifest is not a trusted compatibility source.
    if old.is_none() && !workflow_files(dir)?.is_empty() {
        return Err(ProjectError::External {
            dir: dir.to_owned(),
            summary: "workflow files exist without a committed manifest".into(),
        });
    }
    for (name, bytes) in &extras.workflow {
        check_path(name).map_err(|reason| ProjectError::Canonical { path: dir.join(name), reason })?;
        if !is_workflow(name) {
            return Err(ProjectError::Canonical { path: dir.join(name), reason: "not a workflow file".into() });
        }
        workflow.insert(name.clone(), Entry { sha256: sha256(bytes), size: bytes.len() as u64 });
    }
    // Keep approval/proposal/record bytes as well as engineering bytes. Old AI0
    // manifests did not retain workflow objects: backfill only from verified live
    // bytes, never adopt a changed or missing approval as authoritative.
    for (name, entry) in &workflow {
        let object = dir.join(OBJECTS).join(&entry.sha256);
        if let Some((_, bytes)) = extras.workflow.iter().find(|(path, _)| path == name) {
            write_objects(dir, &[(name.clone(), entry.clone(), bytes.as_slice())])?;
        } else if !matches!(file_sha256(&object), Ok((h, size)) if h == entry.sha256 && size == entry.size) {
            check_on_disk(dir, name).map_err(|reason| ProjectError::Canonical { path: dir.join(name), reason })?;
            let bytes = std::fs::read(dir.join(name)).map_err(io(&dir.join(name)))?;
            if sha256(&bytes) != entry.sha256 || bytes.len() as u64 != entry.size {
                return Err(ProjectError::External { dir: dir.to_owned(), summary: format!("{name} changed") });
            }
            write_objects(dir, &[(name.clone(), entry.clone(), bytes.as_slice())])?;
        }
    }
    let manifest = Manifest {
        format: 1,
        project,
        generation,
        content,
        files: entries.iter().map(|(n, e, _)| (n.clone(), e.clone())).collect(),
        workflow,
        retains_workflow: true,
    };
    let text = serde_json::to_vec_pretty(&manifest)
        .map_err(|source| ProjectError::Json { path: dir.join(MANIFEST), source })?;
    let mut all: Vec<(String, Vec<u8>)> = files.to_vec();
    all.extend(extras.workflow.iter().cloned());
    all.push((MANIFEST.to_owned(), text));
    for w in WORKFLOW {
        let d = dir.join(w);
        std::fs::create_dir_all(&d).map_err(io(&d))?;
    }
    let ecad = dir.join(".ecad");
    std::fs::create_dir_all(&ecad).map_err(io(&ecad))?;
    if let Err(error) = store::commit(dir, &all) {
        if !reconcile || !dir.join(store::JOURNAL).exists() {
            return Err(error);
        }
        let uncertain = || ProjectError::CommitUncertain(dir.to_owned());
        store::recover(dir).map_err(|_| uncertain())?;
        if read_manifest(dir).map_err(|_| uncertain())?.as_ref() != Some(&manifest)
            || !verify(dir, &Quiet).map_err(|_| uncertain())?.clean()
        {
            return Err(uncertain());
        }
    }
    prune(dir, &manifest);
    Ok(manifest.revision())
}

/// Commit workflow files only (a proposal, an approval, a change record): the engineering files
/// and generation stay as committed. Refused while the folder has outside changes. A folder
/// saved before AI0 is adopted first (its manifest created at generation 1).
pub fn commit_workflow(dir: &Path, workflow: Vec<(String, Vec<u8>)>) -> Result<Revision> {
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    commit_workflow_locked(dir, workflow)
}

/// [`commit_workflow`] for a caller that holds the lock.
pub fn commit_workflow_locked(dir: &Path, workflow: Vec<(String, Vec<u8>)>) -> Result<Revision> {
    let v = verify(dir, &Quiet)?;
    if !v.unsafe_paths.is_empty() || (v.committed.is_some() && !v.clean()) {
        return Err(ProjectError::External { dir: dir.to_owned(), summary: v.summary() });
    }
    let manifest = read_manifest(dir)?;
    if let Some(mut manifest) = manifest.clone().filter(|m| m.retains_workflow) {
        // Verification already checked every engineering and retained file. Only
        // workflow bytes and the manifest need staging; preserve the revision.
        let mut files = workflow;
        for (name, bytes) in &files {
            check_on_disk(dir, name).map_err(|reason| ProjectError::Canonical { path: dir.join(name), reason })?;
            if !is_workflow(name) {
                return Err(ProjectError::Canonical { path: dir.join(name), reason: "not a workflow file".into() });
            }
            let entry = Entry { sha256: sha256(bytes), size: bytes.len() as u64 };
            write_objects(dir, &[(name.clone(), entry.clone(), bytes.as_slice())])?;
            manifest.workflow.insert(name.clone(), entry);
        }
        let text = serde_json::to_vec_pretty(&manifest)
            .map_err(|source| ProjectError::Json { path: dir.join(MANIFEST), source })?;
        files.push((MANIFEST.into(), text));
        if let Err(error) = store::commit_partial(dir, &files) {
            if !dir.join(store::JOURNAL).exists() {
                return Err(error);
            }
            let uncertain = || ProjectError::CommitUncertain(dir.to_owned());
            store::recover(dir).map_err(|_| uncertain())?;
            if read_manifest(dir).map_err(|_| uncertain())?.as_ref() != Some(&manifest)
                || !verify(dir, &Quiet).map_err(|_| uncertain())?.clean()
            {
                return Err(uncertain());
            }
        }
        prune(dir, &manifest);
        return Ok(manifest.revision());
    }
    let files = match manifest {
        Some(_) => committed_files(dir)?,
        // Adoption: the files as they are on disk (a format 1 folder stays format 1 until a
        // person saves it knowingly), so nothing but the manifest and contents is written.
        None => {
            crate::load(dir)?;
            let mut files = Vec::new();
            for name in store::project_files(dir)? {
                let path = dir.join(&name);
                files.push((name, std::fs::read(&path).map_err(io(&path))?));
            }
            files
        }
    };
    commit(dir, &files, &Extras { workflow }, false)
}

/// The compatibility path for a folder saved before AI0: commit its current contents as
/// generation 1 (nothing on disk changes but the manifest, the retained contents and `record`).
/// A folder that has a manifest is left alone. Returns the committed revision.
pub fn adopt(dir: &Path, record: (String, Vec<u8>)) -> Result<Revision> {
    if read_manifest(dir)?.is_some() {
        return committed(dir);
    }
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    if read_manifest(dir)?.is_some() {
        return committed(dir);
    }
    commit_workflow_locked(dir, vec![record])
}

/// Read a workflow file, checked against the manifest when it lists it (a file changed outside
/// Electric Studio is refused).
pub fn read_workflow(dir: &Path, path: &str) -> Result<Vec<u8>> {
    check_on_disk(dir, path).map_err(|reason| ProjectError::Canonical { path: dir.join(path), reason })?;
    let bytes = std::fs::read(dir.join(path)).map_err(io(&dir.join(path)))?;
    if let Some(m) = read_manifest(dir)? {
        match m.workflow.get(path) {
            Some(e) if e.sha256 == sha256(&bytes) => {}
            Some(_) => {
                return Err(ProjectError::External {
                    dir: dir.to_owned(),
                    summary: format!("{path} was changed outside Electric Studio"),
                });
            }
            None => {
                return Err(ProjectError::External {
                    dir: dir.to_owned(),
                    summary: format!("{path} is not in the committed manifest"),
                });
            }
        }
    }
    Ok(bytes)
}

/// Put the committed engineering files back (D43): files that differ, are missing or were
/// added are moved to `.ecad/quarantine/<n>/` first, never deleted or overwritten in place.
/// Returns what was restored.
pub fn restore(dir: &Path) -> Result<Vec<String>> {
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    let v = verify(dir, &Quiet)?;
    if v.committed.is_none() {
        return Err(ProjectError::Canonical {
            path: dir.join(MANIFEST),
            reason: "nothing committed to restore".into(),
        });
    }
    let mut committed = committed_files(dir)?;
    let manifest = read_manifest(dir)?.ok_or_else(|| ProjectError::Canonical {
        path: dir.join(MANIFEST),
        reason: "nothing committed to restore".into(),
    })?;
    // Prepare all workflow restoration bytes before quarantining anything. A legacy
    // project with already-damaged workflow data may lack a retained copy; fail closed.
    for path in v.modified.iter().chain(&v.missing).filter(|p| is_workflow(p)) {
        let entry = manifest.workflow.get(path).ok_or_else(|| ProjectError::Canonical {
            path: dir.join(path),
            reason: "workflow entry missing from manifest".into(),
        })?;
        let object = dir.join(OBJECTS).join(&entry.sha256);
        let bytes = std::fs::read(&object).map_err(io(&object))?;
        if sha256(&bytes) != entry.sha256 || bytes.len() as u64 != entry.size {
            return Err(ProjectError::Canonical {
                path: object,
                reason: format!("retained workflow contents damaged for {path}"),
            });
        }
        committed.push((path.clone(), bytes));
    }
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let quarantine = dir.join(QUARANTINE).join(n.to_string());
    let mut touched = Vec::new();
    for path in v.modified.iter().chain(&v.added).chain(&v.missing) {
        check_on_disk(dir, path).map_err(|reason| ProjectError::Canonical { path: dir.join(path), reason })?;
    }
    for path in v.modified.iter().chain(&v.added) {
        let target = quarantine.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::rename(dir.join(path), &target).map_err(io(&target))?;
        touched.push(path.clone());
    }
    let wanted: BTreeSet<&String> = v.modified.iter().chain(&v.missing).collect();
    let mut files = Vec::new();
    for (path, bytes) in committed {
        if wanted.contains(&path) {
            files.push((path.clone(), bytes));
            if !touched.contains(&path) {
                touched.push(path);
            }
        }
    }
    store::commit_partial(dir, &files)?;
    touched.sort();
    touched.dedup();
    Ok(touched)
}

/// Accept the folder as it is now as a new committed generation (D43), after it loaded and
/// validated: `record` is the change record (by the external, unknown actor) committed with it;
/// `rewrite` are workflow files rewritten with it (approvals invalidated by the acceptance).
pub fn accept(
    dir: &Path,
    record: (String, Vec<u8>),
    rewrite: Vec<(String, Vec<u8>)>,
) -> Result<(ecad_model::Project, Revision)> {
    let _lock = store::Lock::take(dir)?;
    store::recover(dir)?;
    accept_locked(dir, record, rewrite)
}

/// Reconcile while the caller holds the lock from the preview recheck through revocation.
pub fn accept_locked(
    dir: &Path,
    record: (String, Vec<u8>),
    rewrite: Vec<(String, Vec<u8>)>,
) -> Result<(ecad_model::Project, Revision)> {
    let project = crate::load(dir)?;
    let files = crate::render(&project)?;
    // A changed workflow file is never adopted as it is: the manifest keeps what it committed.
    let v = verify(dir, &Quiet)?;
    if !v.unsafe_paths.is_empty() {
        return Err(ProjectError::External { dir: dir.to_owned(), summary: v.summary() });
    }
    if let Some(p) = v.modified.iter().chain(&v.added).chain(&v.missing).find(|p| is_workflow(p)) {
        return Err(ProjectError::External {
            dir: dir.to_owned(),
            summary: format!("{p} (a workflow file) was changed outside Electric Studio: restore it first"),
        });
    }
    let mut workflow = rewrite;
    workflow.push(record);
    let revision = commit_inner(dir, &files, &Extras { workflow }, false, true)?;
    Ok((project, revision))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_paths_are_refused() {
        for p in ["/etc/passwd", "../x.json", "a/../b.json", "C:/x", "pages\\a.json", "pages/CON.json", "a//b", "./a"] {
            assert!(check_path(p).is_err(), "{p}");
        }
        for p in ["project.json", "pages/1234.json", "changes/abc.json"] {
            assert!(check_path(p).is_ok(), "{p}");
        }
        let names = ["pages/A.json".to_owned(), "pages/a.json".to_owned()];
        assert_eq!(case_collisions(names.iter()).len(), 1);
    }

    #[test]
    fn revisions_print_and_parse() {
        let r = Revision { project: "p1".into(), generation: 7, content: "sha256:ab".into() };
        assert_eq!(r.to_string().parse::<Revision>().unwrap(), r);
        assert!("nonsense".parse::<Revision>().is_err());
    }
}
