//! `ecad`: headless commands.
//!
//! - `ecad new-sample <project-dir>`: write the sample project as a project folder
//! - `ecad check <project-dir>`: load a project and report what it contains
//! - `ecad export <project-dir> <out-dir>`: each page as DXF and PNG, plus the wire list
//! - `ecad demo <out-dir>`: `export` for the built-in sample, without a project folder

use ecad_model::Project;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  ecad new-sample <project-dir>
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
    let rows = ecad_reports::wire_list(project).map_err(|e| e.to_string())?;
    write("wire-list.csv", ecad_reports::wire_list_csv(&rows).as_bytes())
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, dir] if cmd == "new-sample" => {
            let project = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
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
