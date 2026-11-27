//! `ecad-mcp <project-dir> [--write]`: an MCP server over stdio for one project. See the
//! crate docs.

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
ecad-mcp: an MCP server for one Electric Studio project, over stdin/stdout

usage:
  ecad-mcp <project-dir> [--write]
      Serve the project. Read tools only, unless --write: then edit tools too, with undo;
      changes reach the folder only when the client calls `save`.
  ecad-mcp --help | --version";

fn main() -> ExitCode {
    // A copy of this program started to read an outside DWG does that and nothing else.
    if let Some(code) = ecad_view::worker::serve_if_asked() {
        return ExitCode::from(u8::try_from(code).unwrap_or(2));
    }
    ecad_view::worker::use_own_worker();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ecad-mcp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let writable = args.iter().any(|a| a == "--write");
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let (Some(dir), true) = (dirs.first(), dirs.len() == 1 && args.len() == dirs.len() + usize::from(writable)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut server = match ecad_mcp::Server::open(Path::new(dir), writable) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ecad-mcp: {e}");
            return ExitCode::from(2);
        }
    };
    // stdout carries only protocol messages; anything for a person goes to stderr.
    eprintln!("ecad-mcp: serving {dir}{}", if writable { " (edits allowed)" } else { " (read only)" });
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if let Some(reply) = server.handle_line(&line)
            && (writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
    if server.editor().is_dirty() {
        eprintln!("ecad-mcp: closed with unsaved changes; the project folder is as it was last saved");
    }
    ExitCode::SUCCESS
}
