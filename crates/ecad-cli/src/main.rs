//! `ecad`: headless commands.
//!
//! - `ecad new-sample <project-dir> [start|contacts|potentials|terminals]`: write a sample project:
//!   the start circuit (default), plus -K1's contacts on page 2, plus potentials and an
//!   interruption point across both pages
//! - `ecad check <project-dir>`: load a project and report what it contains
//! - `ecad export <project-dir> <out-dir>`: each page as DXF and PNG, plus the reports (wire
//!   list, bill of materials, devices without parts)
//! - `ecad demo <out-dir>`: `export` for the built-in sample, without a project folder

use ecad_model::Project;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  ecad new-sample <project-dir> [start|contacts|potentials|terminals]
  ecad check <project-dir>
  ecad export <project-dir> <out-dir>
  ecad demo <out-dir>";

fn load(dir: &Path) -> Result<Project, String> {
    ecad_project::load(dir).map_err(|e| e.to_string())
}

fn export(project: &Project, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, bytes: &[u8]| {
        let path = out.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
        Ok::<(), String>(())
    };
    for page in project.pages.values() {
        let stem = format!("page-{}", page.name);
        write(&format!("{stem}.dxf"), ecad_view::page_dxf(project, page.id).map_err(|e| e.to_string())?.as_bytes())?;
        write(&format!("{stem}.png"), &ecad_view::page_png(project, page.id, 1200, 1200).map_err(|e| e.to_string())?)?;
    }
    for (name, csv) in ecad_reports::all_csv(project).map_err(|e| e.to_string())? {
        write(name, csv.as_bytes())?;
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, dir, rest @ ..] if cmd == "new-sample" && rest.len() <= 1 => {
            use ecad_model::sample;
            let project = match rest.first().map(String::as_str) {
                None | Some("start") => sample::start_circuit(),
                Some("contacts") => sample::with_contacts(),
                Some("potentials") => sample::with_potentials(),
                Some("terminals") => sample::with_terminals(),
                Some(other) => return Err(format!("unknown sample `{other}`\n{USAGE}")),
            }
            .map_err(|e| e.to_string())?;
            ecad_project::save(&project, Path::new(dir)).map_err(|e| e.to_string())?;
            println!("wrote project {dir}");
            Ok(())
        }
        [cmd, dir] if cmd == "check" => {
            let p = load(Path::new(dir))?;
            println!(
                "{}: {} pages, {} devices, {} symbols placed, {} wires",
                p.name,
                p.pages.len(),
                p.devices.len(),
                p.functions.len(),
                p.connections.len()
            );
            Ok(())
        }
        [cmd, dir, out] if cmd == "export" => export(&load(Path::new(dir))?, &PathBuf::from(out)),
        [cmd, out] if cmd == "demo" => {
            export(&ecad_model::sample::start_circuit().map_err(|e| e.to_string())?, &PathBuf::from(out))
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ecad: {e}");
            ExitCode::FAILURE
        }
    }
}
