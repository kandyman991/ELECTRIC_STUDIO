//! Electric Studio in a Tauri window: a thin shell over `ecad_editor::Session`.
//!
//! Usage: `electric-studio-tauri [project-dir]`. Without a folder it opens (or creates) the
//! sample project in `~/ElectricStudio/Sample`.
//!
//! The web page (ui/) forwards input as `UiEvent` JSON to the one `event` command and paints
//! the `Frame` it gets back on a canvas. All behaviour lives in `ecad-editor` (AGENTS.md rule 6).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ecad_editor::{Frame, Session, UiEvent};
use std::path::PathBuf;
use std::sync::Mutex;

/// Must match the canvas background in ui/style.css.
const BG: [u8; 3] = [30, 33, 39];

fn default_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join("ElectricStudio").join("Sample")
}

/// Apply the events (none for a plain refresh) and return the frame to paint. `known` is the
/// revision whose page the web view already holds, so the page is only sent when it changed.
#[tauri::command]
fn event(state: tauri::State<'_, Mutex<Session>>, events: Vec<UiEvent>, known: Option<u64>) -> Result<Frame, String> {
    let mut session = state.lock().map_err(|_| "editor state is unavailable after an earlier error".to_owned())?;
    for ev in events {
        session.handle(ev);
    }
    Ok(session.frame(known, BG))
}

fn main() {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(default_dir);
    let session = match Session::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("electric-studio-tauri: {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let result = tauri::Builder::default()
        .manage(Mutex::new(session))
        .invoke_handler(tauri::generate_handler![event])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("electric-studio-tauri: {e}");
        std::process::exit(1);
    }
}
