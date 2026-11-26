//! `ecad`: Electric Studio without a window, for scripts and CI. See [`USAGE`].
//!
//! Exit codes: 0 done; 1 `check` found errors (or, with `--strict`, warnings); 2 the command
//! couldn't run (bad arguments, a project that doesn't load, a file that can't be written).

use ecad_model::Project;
use ecad_view::Formats;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
ecad: Electric Studio from the command line

usage:
  ecad check <project-dir> [--json] [--strict]
      Summarise the project and list its errors and warnings.
      --json    print the result as JSON instead
      --strict  fail on warnings too
  ecad export <project-dir> <out-dir> [--only <formats>]
      Write drawings.pdf (all pages), a DXF, DWG and PNG per page, and the reports as CSV.
      --only    a comma-separated subset of: pdf, dxf, dwg, png, reports
  ecad report <project-dir> <report>
      Print one report as CSV: wire-list, bom, terminal-diagram, plc-io-list,
      cable-list, table-of-contents, devices-without-parts, panel-cut-list
  ecad new-sample <project-dir> [start|contacts|potentials|terminals|plc|power|library|reference]
      Write a sample project (default: start).
  ecad demo <out-dir>
      export for the built-in sample, without a project folder.
  ecad --help | --version

exit codes: 0 done, 1 check failed, 2 the command couldn't run";

/// What `--only` can name.
const PARTS: [&str; 5] = ["pdf", "dxf", "dwg", "png", "reports"];

fn load(dir: &Path) -> Result<Project, String> {
    ecad_project::load(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// The arguments, split into positionals and `--flags` (with the value of `--only`).
struct Args {
    positional: Vec<String>,
    json: bool,
    strict: bool,
    only: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args { positional: Vec::new(), json: false, strict: false, only: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--strict" => out.strict = true,
            "--only" => out.only = Some(it.next().ok_or("--only needs a list, e.g. --only pdf,reports")?.clone()),
            flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`")),
            _ => out.positional.push(a.clone()),
        }
    }
    Ok(out)
}

/// The formats and whether to write reports, from `--only` (everything without it).
fn selection(only: Option<&str>) -> Result<(Formats, bool), String> {
    let Some(list) = only else { return Ok((Formats::ALL, true)) };
    let mut formats = Formats::NONE;
    let mut reports = false;
    for part in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part {
            "pdf" => formats.pdf = true,
            "dxf" => formats.dxf = true,
            "dwg" => formats.dwg = true,
            "png" => formats.png = true,
            "reports" => reports = true,
            other => return Err(format!("unknown format `{other}` in --only (use {})", PARTS.join(", "))),
        }
    }
    if formats == Formats::NONE && !reports {
        return Err("--only names nothing to export".into());
    }
    Ok((formats, reports))
}

fn export(project: &Project, out: &Path, formats: Formats, reports: bool) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, bytes: &[u8]| {
        let path = out.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
        Ok::<(), String>(())
    };
    for (name, bytes) in ecad_view::drawing_files(project, formats).map_err(|e| e.to_string())? {
        write(&name, &bytes)?;
    }
    if reports {
        for (name, csv) in ecad_reports::all_csv(project).map_err(|e| e.to_string())? {
            write(name, csv.as_bytes())?;
        }
    }
    Ok(())
}

fn check(project: &Project, json: bool, strict: bool) -> Result<ExitCode, String> {
    let checks = ecad_rules::check(project);
    let errors = checks.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
    let warnings = checks.len() - errors;
    if json {
        let value = serde_json::json!({
            "project": project.name,
            "pages": project.pages.len(),
            "devices": project.devices.len(),
            "symbols": project.functions.len(),
            "wires": project.connections.len(),
            "errors": errors,
            "warnings": warnings,
            "checks": checks,
        });
        println!("{}", serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?);
    } else {
        println!(
            "{}: {} pages, {} devices, {} symbols placed, {} wires",
            project.name,
            project.pages.len(),
            project.devices.len(),
            project.functions.len(),
            project.connections.len()
        );
        print!("{}", ecad_rules::summary(&checks));
        println!("{errors} errors, {warnings} warnings");
    }
    let failed = errors > 0 || (strict && warnings > 0);
    Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.first().is_some_and(|a| a == "help") {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ecad {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    let a = parse(args)?;
    let flags = |allowed: &[&str]| -> Result<(), String> {
        for (set, name) in [(a.json, "--json"), (a.strict, "--strict"), (a.only.is_some(), "--only")] {
            if set && !allowed.contains(&name) {
                return Err(format!("{name} doesn't apply to `{}`", a.positional.first().map_or("", String::as_str)));
            }
        }
        Ok(())
    };
    let p: Vec<&str> = a.positional.iter().map(String::as_str).collect();
    match p.as_slice() {
        ["check", dir] => {
            flags(&["--json", "--strict"])?;
            check(&load(Path::new(dir))?, a.json, a.strict)
        }
        ["export", dir, out] => {
            flags(&["--only"])?;
            let (formats, reports) = selection(a.only.as_deref())?;
            export(&load(Path::new(dir))?, Path::new(out), formats, reports)?;
            Ok(ExitCode::SUCCESS)
        }
        ["report", dir, name] => {
            flags(&[])?;
            let project = load(Path::new(dir))?;
            let csv = ecad_reports::csv(&project, name)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("unknown report `{name}` (use {})", ecad_reports::REPORTS.join(", ")))?;
            print!("{csv}");
            Ok(ExitCode::SUCCESS)
        }
        ["new-sample", dir, rest @ ..] if rest.len() <= 1 => {
            flags(&[])?;
            use ecad_model::sample;
            let project = match rest.first().copied() {
                None | Some("start") => sample::start_circuit(),
                Some("contacts") => sample::with_contacts(),
                Some("potentials") => sample::with_potentials(),
                Some("terminals") => sample::with_terminals(),
                Some("plc") => sample::with_plc(),
                Some("power") => sample::with_power(),
                Some("library") => sample::library_overview(),
                Some("reference") => ecad_model::reference::reference_project(),
                Some(other) => {
                    return Err(format!(
                        "unknown sample `{other}` (use start, contacts, potentials, terminals, plc, power, library, reference)"
                    ));
                }
            }
            .map_err(|e| e.to_string())?;
            ecad_project::save(&project, Path::new(dir)).map_err(|e| e.to_string())?;
            println!("wrote project {dir}");
            Ok(ExitCode::SUCCESS)
        }
        ["demo", out] => {
            flags(&[])?;
            let project = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
            export(&project, &PathBuf::from(out), Formats::ALL, true)?;
            Ok(ExitCode::SUCCESS)
        }
        [] => Err(format!("no command\n\n{USAGE}")),
        [cmd, ..] if ["check", "export", "report", "new-sample", "demo"].contains(cmd) => {
            Err(format!("wrong arguments for `{cmd}`\n\n{USAGE}"))
        }
        [cmd, ..] => Err(format!("unknown command `{cmd}`\n\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ecad: {e}");
            ExitCode::from(2)
        }
    }
}
