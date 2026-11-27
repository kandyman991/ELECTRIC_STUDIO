//! Reading outside drawings in a child process.
//!
//! A DXF or DWG from outside may be hostile: built to make the reader allocate without end or
//! crash. In-process that would end the editor with the user's unsaved work. So a program that
//! imports drawings runs the import in a copy of itself:
//!
//! - [`serve_if_asked`], first thing in `main`, turns that copy into a worker: it reads one
//!   request from standard input, imports, writes one reply to standard output and exits. It also
//!   serves CADCraft's own DWG worker, which the DWG reader needs.
//! - [`use_own_worker`], at startup, says to start such copies, for underlays and for DWG files.
//! - [`import_underlay`] imports in the worker when one is set (in-process otherwise: tests and
//!   library use), with a time limit, a cap on the reply's size, and the reply checked as
//!   untrusted input.
//!
//! A drawing that exhausts memory or crashes the reader then costs that one import. The worker
//! has no memory limit of its own: that takes per-platform system calls, which the workspace's
//! `unsafe_code = "forbid"` keeps out; the import's own budgets (file size, an estimate of the
//! flattening before it is drawn, the point count) still apply inside it.

use crate::underlay::{Imported, MAX_FILE};
use crate::{Result, ViewError};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

/// The argument that makes the program an underlay worker.
pub const WORKER_ARG: &str = "--underlay-worker";
/// How long one import may take before the worker is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Most bytes accepted as a reply: what [`ecad_model::underlay::MAX_POINTS`] points take, and
/// some.
pub const MAX_REPLY: u64 = 160 << 20;
/// Most bytes of a file name sent along.
const MAX_NAME: usize = 4096;
/// A name no file has (it starts with NUL): the worker stops dead, as a crash would, so tests
/// can check a crash costs only the import.
pub const CRASH_FOR_TEST: &str = "\0crash";

static WORKER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Serve one request and return the exit code if this process was started as a worker (for
/// underlays or for CADCraft's DWG reader); `None` to carry on as usual.
pub fn serve_if_asked() -> Option<i32> {
    if let Some(code) = cadcraft_io::dwg_worker::serve_if_asked() {
        return Some(code);
    }
    (std::env::args_os().nth(1)? == WORKER_ARG).then(|| serve(std::io::stdin().lock(), std::io::stdout().lock()))
}

/// Start copies of this program as workers from now on, for underlays and DWG files. The
/// program's `main` must call [`serve_if_asked`] first.
pub fn use_own_worker() {
    cadcraft_io::dwg_worker::use_own_worker();
    if let (Ok(program), Ok(mut w)) = (std::env::current_exe(), WORKER.lock()) {
        *w = Some(program);
    }
}

/// Use `program` (which calls [`serve_if_asked`]) as the underlay and DWG worker: for tests,
/// where the running program is a test harness.
pub fn use_worker(program: PathBuf) {
    cadcraft_io::dwg_worker::use_worker(program.clone(), vec![cadcraft_io::dwg_worker::WORKER_ARG.into()]);
    if let Ok(mut w) = WORKER.lock() {
        *w = Some(program);
    }
}

fn worker() -> Option<PathBuf> {
    WORKER.lock().ok().and_then(|w| w.clone())
}

/// [`crate::underlay::import`], in the worker if one is set.
pub fn import_underlay(bytes: &[u8], name: &str) -> Result<Imported> {
    if bytes.len() > MAX_FILE {
        return Err(ViewError::Drawing(format!("{name}: over {} MB", MAX_FILE >> 20)));
    }
    match worker() {
        Some(program) => run(&program, bytes, name),
        None => crate::underlay::import(bytes, name),
    }
}

/// A request: the name's length (4 bytes, little-endian), the name, the file.
fn request(bytes: &[u8], name: &str) -> Vec<u8> {
    let name = &name.as_bytes()[..name.len().min(MAX_NAME)];
    let mut r = Vec::with_capacity(4 + name.len() + bytes.len());
    r.extend((name.len() as u32).to_le_bytes());
    r.extend(name);
    r.extend(bytes);
    r
}

/// The worker's side: read a request, import, reply `ok` or `error` as JSON.
fn serve(mut input: impl Read, mut output: impl Write) -> i32 {
    let mut request = Vec::new();
    if input.by_ref().take((MAX_FILE + MAX_NAME + 4) as u64 + 1).read_to_end(&mut request).is_err() {
        return 2;
    }
    if parse_request(&request).is_ok_and(|(name, _)| name == CRASH_FOR_TEST) {
        std::process::abort();
    }
    // This process is the isolation CADCraft's DWG reader asks for: it may convert here.
    cadcraft_io::dwg_worker::allow_in_process();
    let reply = match parse_request(&request) {
        Ok((name, bytes)) => match crate::underlay::import(bytes, &name) {
            Ok(imported) => serde_json::json!({ "ok": imported }),
            // The message alone: the caller wraps it as its own error.
            Err(ViewError::Drawing(m)) => serde_json::json!({ "error": m }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        },
        Err(e) => serde_json::json!({ "error": e }),
    };
    match serde_json::to_writer(&mut output, &reply).and_then(|()| output.flush().map_err(serde_json::Error::io)) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

fn parse_request(r: &[u8]) -> std::result::Result<(String, &[u8]), String> {
    if r.len() > MAX_FILE + MAX_NAME + 4 {
        return Err(format!("over {} MB", MAX_FILE >> 20));
    }
    let len = r.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or("no request")?;
    if len > MAX_NAME {
        return Err("a file name too long".into());
    }
    let name = r.get(4..4 + len).ok_or("a request cut short")?;
    Ok((String::from_utf8_lossy(name).into_owned(), &r[4 + len..]))
}

/// The worker's reply, read as untrusted: well-formed JSON, an error message, or shapes the
/// model accepts.
fn parse_reply(reply: &[u8], name: &str) -> Result<Imported> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        ok: Option<Imported>,
        error: Option<String>,
    }
    let bad = |why: &str| ViewError::Drawing(format!("{name}: the import worker gave {why}"));
    let r: Reply = serde_json::from_slice(reply).map_err(|_| bad("a reply that isn't one"))?;
    match (r.ok, r.error) {
        (Some(imported), None) if imported.shapes.is_valid() => Ok(imported),
        (Some(_), None) => Err(bad("shapes out of bounds")),
        (None, Some(e)) => Err(ViewError::Drawing(e.chars().take(2000).collect())),
        _ => Err(bad("a reply that isn't one")),
    }
}

/// Start the worker, send the request, and wait for its reply within [`TIMEOUT`].
fn run(program: &PathBuf, bytes: &[u8], name: &str) -> Result<Imported> {
    let failed = |why: String| ViewError::Drawing(format!("{name}: {why}"));
    let mut child = Command::new(program)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| failed(format!("can't start the import worker: {e}")))?;
    let (Some(mut to_child), Some(from_child)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(failed("can't talk to the import worker".into()));
    };
    let request = request(bytes, name);
    // Write and read on their own threads, so a worker that stops reading can't block us.
    let writer = std::thread::spawn(move || {
        let _ = to_child.write_all(&request);
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reply = Vec::new();
        let read = from_child.take(MAX_REPLY + 1).read_to_end(&mut reply);
        let _ = tx.send(read.map(|_| reply));
    });
    let received = rx.recv_timeout(TIMEOUT);
    let _ = writer.join();
    let reply = match received {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => {
            let _ = child.kill();
            return Err(failed(format!("the import worker failed: {e}")));
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failed(format!("took over {} minutes; stopped", TIMEOUT.as_secs() / 60)));
        }
    };
    let status = child.wait().map_err(|e| failed(format!("the import worker failed: {e}")))?;
    if reply.len() as u64 > MAX_REPLY {
        return Err(failed("the import worker's reply was too big".into()));
    }
    if reply.is_empty() {
        // Out of memory, a crash: the worker ended without a word.
        return Err(failed(format!("the drawing stopped the import worker ({status}); it may be too big or damaged")));
    }
    parse_reply(&reply, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_and_bad_ones_are_refused() {
        let r = request(b"DXF BYTES", "old.dxf");
        assert_eq!(parse_request(&r).unwrap(), ("old.dxf".to_owned(), &b"DXF BYTES"[..]));
        assert!(parse_request(b"\x01").is_err());
        assert!(parse_request(&[255, 255, 0, 0]).is_err(), "a name too long");
        assert!(parse_request(&[10, 0, 0, 0, b'a']).is_err(), "cut short");
    }

    #[test]
    fn the_worker_serves_a_request_and_its_reply_is_checked() {
        let p = ecad_model::sample::start_circuit().unwrap();
        let dxf = crate::page_dxf(&p, ecad_model::sample::PAGE_1).unwrap();
        let mut out = Vec::new();
        assert_eq!(serve(&request(dxf.as_bytes(), "page.dxf")[..], &mut out), 0);
        let imported = parse_reply(&out, "page.dxf").unwrap();
        assert!(imported.shapes.lines.len() > 10);
        // An import error comes back as the error.
        let mut out = Vec::new();
        serve(&request(b"junk", "x.dxf")[..], &mut out);
        let e = parse_reply(&out, "x.dxf").unwrap_err().to_string();
        assert!(e.starts_with("drawing: x.dxf:"), "{e}");
        // A reply that isn't one, or whose shapes the model wouldn't take, is refused.
        for bad in [&b"garbage"[..], b"{}", br#"{"ok":null,"error":null}"#, br#"{"surprise":1}"#] {
            assert!(parse_reply(bad, "y.dxf").unwrap_err().to_string().contains("worker gave"), "{bad:?}");
        }
        let mut lying = serde_json::to_value(&imported).unwrap();
        lying["shapes"]["width"] = serde_json::json!(-5.0);
        let lying = serde_json::to_vec(&serde_json::json!({ "ok": lying })).unwrap();
        assert!(parse_reply(&lying, "z.dxf").unwrap_err().to_string().contains("out of bounds"));
    }
}
