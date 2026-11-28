//! `ecad`: Electric Studio without a window, for scripts and CI. See [`USAGE`].
//!
//! Exit codes: 0 done; 1 `check` found errors (or, with `--strict`, warnings); 2 the command
//! couldn't run (bad arguments, a project that doesn't load, a file that can't be written).

use ecad_model::{Command, Project};
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
      Write drawings.pdf (all pages), a DXF, DWG and PNG per page, the reports as CSV,
      and the PLC channels as a TIA Portal tag table (tia-plc-tags.xlsx).
      --only    a comma-separated subset of: pdf, dxf, dwg, png, reports
  ecad report <project-dir> <report>
      Print one report as CSV: wire-list, bom, terminal-diagram, plc-io-list,
      cable-list, table-of-contents, devices-without-parts, panel-cut-list,
      wire-totals, netlist
  ecad import-tia <project-dir> <tags.xlsx>
      Take PLC symbols and comments from a TIA Portal PLC tags export, matched by
      address, and save the project. Tags at other addresses are listed, not added.
  ecad import-dxf <project-dir> <page> <drawing.dxf|.dwg> [--units mm|cm|m|inch|feet]
      Put an existing drawing behind a page (its name, or #n) to redraw over: at real
      size where it was drawn if that is on the sheet, otherwise fitted. Its units come
      from the file (or --units; mm if neither). Replaces one already there. The file is
      read in a separate worker process.
  ecad remove-underlay <project-dir> <page>
      Take a page's underlay away.
  ecad new-sample <project-dir> [start|contacts|potentials|terminals|plc|power|library|reference|conveyor]
      Write a sample project (default: start).
  ecad numbering <project-dir> [--json]
      List the numbering policies, capacities, reserved and retired identifiers.
  ecad reserve <project-dir> <identifier>... [--note <text>]
  ecad unreserve <project-dir> <identifier>...
  ecad retire <project-dir> <identifier>... [--note <text>]
  ecad reclaim <project-dir> <identifier>...
      Keep identifiers free for an extension, release them, retire them, or allow a retired
      one to be used again. Identifiers: tag:-K10, tag:-K10..-K19, cable:-W9, wire:W5,
      terminal:-X3:7..9, address:I1.0..I1.3, core:-W1:3, page:3a.
  ecad renumber <project-dir> wires|devices|pages|terminals:<strip> [--apply] [--json]
      Preview renumbering (nothing changes); --apply renumbers, as one step. Ids, wires and
      drawings stay; reserved and retired identifiers are skipped.
  ecad demo <out-dir>
      export for the built-in sample, without a project folder.
  ecad --help | --version

Commands that save a project write format 2. Saving a format 1 project whose conversion
found ambiguous mappings (kept apart, nothing merged) needs --accept-conversion.

exit codes: 0 done, 1 check failed, 2 the command couldn't run";

/// What `--only` can name.
const PARTS: [&str; 5] = ["pdf", "dxf", "dwg", "png", "reports"];

/// Say on stderr when a save is about to upgrade an older project's format.
/// Before a save: say when it upgrades an older project's format, and refuse to save a
/// format 1 conversion that found ambiguous mappings unless `--accept-conversion` says to
/// save them as converted (kept apart, nothing merged).
fn before_save(dir: &str, accept: bool) -> Result<(), String> {
    let findings = ecad_project::conversion_findings(Path::new(dir)).map_err(|e| e.to_string())?;
    if !findings.is_empty() && !accept {
        let list: Vec<String> = findings.iter().map(ToString::to_string).collect();
        return Err(format!(
            "{dir}: converting from format 1 kept these apart without merging them: {}; resolve them in the app, or pass --accept-conversion to save them so",
            list.join("; ")
        ));
    }
    if let Some(note) = ecad_project::upgrade_note(Path::new(dir)) {
        eprintln!("note: {dir}: {note}");
    }
    Ok(())
}

/// `ecad numbering`: the project's numbering state.
fn numbering(p: &Project, json: bool) -> Result<ExitCode, String> {
    use ecad_model::numbering::Kind;
    let n = &p.numbering;
    let list = |m: &std::collections::BTreeMap<ecad_model::Identifier, String>| -> Vec<(String, String)> {
        m.iter().map(|(id, note)| (p.describe_identifier(id), note.clone())).collect()
    };
    let capacities: Vec<(String, u32)> =
        n.capacities.iter().map(|(d, c)| (p.device(*d).map(|d| d.tag.clone()).unwrap_or_default(), *c)).collect();
    if json {
        let policies: serde_json::Map<String, serde_json::Value> = Kind::ALL
            .iter()
            .map(|k| (k.label().to_owned(), serde_json::to_value(n.policies.strategy(*k)).unwrap_or_default()))
            .collect();
        let entries = |v: Vec<(String, String)>| -> Vec<serde_json::Value> {
            v.into_iter().map(|(id, note)| serde_json::json!({ "identifier": id, "note": note })).collect()
        };
        println!(
            "{}",
            serde_json::json!({
                "policies": policies,
                "pageSuffix": n.policies.page_suffix,
                "capacities": capacities.iter().map(|(t, c)| serde_json::json!({ "device": t, "capacity": c })).collect::<Vec<_>>(),
                "reserved": entries(list(&n.reserved)),
                "retired": entries(list(&n.retired)),
            })
        );
        return Ok(ExitCode::SUCCESS);
    }
    for k in Kind::ALL {
        println!("{}: {:?}", k.label(), n.policies.strategy(k));
    }
    println!("inserted pages: {:?}", n.policies.page_suffix);
    for (t, c) in capacities {
        println!("capacity {t}: {c}");
    }
    for (what, m) in [("reserved", &n.reserved), ("retired", &n.retired)] {
        for (id, note) in list(m) {
            if note.is_empty() { println!("{what} {id}") } else { println!("{what} {id} ({note})") }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn load(dir: &Path) -> Result<Project, String> {
    ecad_project::load(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// The arguments, split into positionals and `--flags` (with the value of `--only`).
struct Args {
    positional: Vec<String>,
    json: bool,
    strict: bool,
    only: Option<String>,
    units: Option<String>,
    accept_conversion: bool,
    apply: bool,
    note: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        json: false,
        strict: false,
        only: None,
        units: None,
        accept_conversion: false,
        apply: false,
        note: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--strict" => out.strict = true,
            "--accept-conversion" => out.accept_conversion = true,
            "--apply" => out.apply = true,
            "--note" => out.note = Some(it.next().ok_or("--note needs a text")?.clone()),
            "--only" => out.only = Some(it.next().ok_or("--only needs a list, e.g. --only pdf,reports")?.clone()),
            "--units" => out.units = Some(it.next().ok_or("--units needs a unit: mm, cm, m, inch or feet")?.clone()),
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
        let tia = ecad_exchange::tia::export_to(project, out).map_err(|e| e.to_string())?;
        if let Some(file) = &tia.file {
            println!("wrote {}", file.display());
        }
        for s in &tia.skipped {
            println!("not in the TIA tag table: {} ({})", s.channel, s.reason);
        }
        if tia.removed {
            println!("removed the older {}: the project has no PLC channels", ecad_exchange::tia::FILE_NAME);
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
        for (set, name) in [
            (a.json, "--json"),
            (a.strict, "--strict"),
            (a.only.is_some(), "--only"),
            (a.units.is_some(), "--units"),
            (a.apply, "--apply"),
            (a.note.is_some(), "--note"),
        ] {
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
        ["import-tia", dir, file] => {
            flags(&[])?;
            let mut project = load(Path::new(dir))?;
            let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let table = ecad_exchange::tia::read_xlsx(&bytes).map_err(|e| format!("{file}: {e}"))?;
            let import = ecad_exchange::tia::import(&project, &table.tags).map_err(|e| e.to_string())?;
            for (list, what) in [
                (&table.ignored, "left out"),
                (&import.unknown, "no channel at"),
                (&import.mismatched, "another kind of channel, not applied:"),
                (&import.conflicts, "different names or comments for one address, not applied:"),
                (&import.duplicates, "the same tag twice:"),
            ] {
                for t in list {
                    println!("{what} {t}");
                }
            }
            if let Some(command) = import.command {
                project.apply(command).map_err(|e| e.to_string())?;
                before_save(dir, a.accept_conversion)?;
                ecad_project::save(&project, Path::new(dir)).map_err(|e| e.to_string())?;
            }
            println!(
                "updated {} PLC channels, {} already matched, {} tags not inputs or outputs",
                import.updated, import.unchanged, import.other
            );
            Ok(ExitCode::SUCCESS)
        }
        ["import-dxf", dir, page, file] => {
            flags(&["--units"])?;
            let units = match a.units.as_deref() {
                None => None,
                Some(u) => Some(
                    ecad_model::underlay::Units::parse(u)
                        .ok_or_else(|| format!("unknown unit `{u}` (use mm, cm, m, inch or feet)"))?,
                ),
            };
            let (project, seen) = ecad_project::load_checked(Path::new(dir)).map_err(|e| e.to_string())?;
            let page = ecad_editor::script::find_page(&project, page)?;
            let mut editor = ecad_editor::Editor::new(project, page).map_err(|e| e.to_string())?;
            let message = editor.import_underlay_file(page, Path::new(file), units)?;
            before_save(dir, a.accept_conversion)?;
            ecad_project::save_checked(editor.project(), Path::new(dir), Some(seen)).map_err(|e| e.to_string())?;
            println!("{message}");
            Ok(ExitCode::SUCCESS)
        }
        ["remove-underlay", dir, page] => {
            flags(&[])?;
            let (project, seen) = ecad_project::load_checked(Path::new(dir)).map_err(|e| e.to_string())?;
            let page = ecad_editor::script::find_page(&project, page)?;
            let mut editor = ecad_editor::Editor::new(project, page).map_err(|e| e.to_string())?;
            editor.remove_underlay(page)?;
            before_save(dir, a.accept_conversion)?;
            ecad_project::save_checked(editor.project(), Path::new(dir), Some(seen)).map_err(|e| e.to_string())?;
            println!("removed the underlay");
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
                Some("conveyor") => ecad_model::conveyor::conveyor(),
                Some(other) => {
                    return Err(format!(
                        "unknown sample `{other}` (use start, contacts, potentials, terminals, plc, power, library, reference, conveyor)"
                    ));
                }
            }
            .map_err(|e| e.to_string())?;
            ecad_project::save(&project, Path::new(dir)).map_err(|e| e.to_string())?;
            println!("wrote project {dir}");
            Ok(ExitCode::SUCCESS)
        }
        ["numbering", dir] => {
            flags(&["--json"])?;
            let project = load(Path::new(dir))?;
            numbering(&project, a.json)
        }
        [cmd @ ("reserve" | "unreserve" | "retire" | "reclaim"), dir, specs @ ..] if !specs.is_empty() => {
            flags(if matches!(*cmd, "reserve" | "retire") { &["--note"] } else { &[] })?;
            let (mut project, seen) = ecad_project::load_checked(Path::new(dir)).map_err(|e| e.to_string())?;
            let mut ids = Vec::new();
            for spec in specs {
                ids.extend(project.parse_identifiers(spec)?);
            }
            let note = a.note.clone().unwrap_or_default();
            let records = || ids.iter().map(|id| ecad_model::Record::new(id.clone(), &note)).collect();
            let command = match *cmd {
                "reserve" => Command::Reserve(records()),
                "retire" => Command::Retire(records()),
                "unreserve" => Command::Unreserve(ids.clone()),
                _ => Command::Reclaim(ids.clone()),
            };
            project.apply(command).map_err(|e| e.to_string())?;
            before_save(dir, a.accept_conversion)?;
            ecad_project::save_checked(&project, Path::new(dir), Some(seen)).map_err(|e| e.to_string())?;
            let done = if *cmd == "reclaim" { "reclaimed".to_owned() } else { format!("{cmd}d") };
            println!("{done} {}", ids.iter().map(|id| project.describe_identifier(id)).collect::<Vec<_>>().join(", "));
            Ok(ExitCode::SUCCESS)
        }
        ["renumber", dir, what] => {
            flags(&["--apply", "--json"])?;
            let (mut project, seen) = ecad_project::load_checked(Path::new(dir)).map_err(|e| e.to_string())?;
            let scope = match *what {
                "wires" => ecad_model::renumber::Scope::Wires,
                "devices" => ecad_model::renumber::Scope::Devices,
                "pages" => ecad_model::renumber::Scope::Pages,
                other => match other.strip_prefix("terminals:") {
                    Some(tag) => {
                        ecad_model::renumber::Scope::Terminals(ecad_editor::script::find_device(&project, tag)?)
                    }
                    None => return Err(format!("renumber wires, devices, pages or terminals:<strip>, not `{other}`")),
                },
            };
            let r = project.preview_renumbering(scope).map_err(|e| e.to_string())?;
            if a.json {
                let changes: Vec<serde_json::Value> = r
                    .changes
                    .iter()
                    .map(|c| serde_json::json!({ "what": c.what, "from": c.from, "to": c.to }))
                    .collect();
                println!("{}", serde_json::json!({ "changes": changes, "applied": a.apply }));
            } else {
                for c in &r.changes {
                    println!("{}: {} -> {}", c.what, c.from, c.to);
                }
            }
            if a.apply && !r.changes.is_empty() {
                project.apply_renumbering(&r).map_err(|e| e.to_string())?;
                before_save(dir, a.accept_conversion)?;
                ecad_project::save_checked(&project, Path::new(dir), Some(seen)).map_err(|e| e.to_string())?;
                eprintln!("renumbered: {} changes", r.changes.len());
            } else if !a.json {
                eprintln!("{} changes previewed; nothing changed (add --apply to renumber)", r.changes.len());
            }
            Ok(ExitCode::SUCCESS)
        }
        ["demo", out] => {
            flags(&[])?;
            let project = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
            export(&project, &PathBuf::from(out), Formats::ALL, true)?;
            Ok(ExitCode::SUCCESS)
        }
        [] => Err(format!("no command\n\n{USAGE}")),
        [cmd, ..]
            if [
                "check",
                "export",
                "report",
                "import-tia",
                "import-dxf",
                "remove-underlay",
                "new-sample",
                "demo",
                "numbering",
                "reserve",
                "unreserve",
                "retire",
                "reclaim",
                "renumber",
            ]
            .contains(cmd) =>
        {
            Err(format!("wrong arguments for `{cmd}`\n\n{USAGE}"))
        }
        [cmd, ..] => Err(format!("unknown command `{cmd}`\n\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    // A copy of this program started to read an outside drawing does that and nothing else.
    if let Some(code) = ecad_view::worker::serve_if_asked() {
        return ExitCode::from(u8::try_from(code).unwrap_or(2));
    }
    ecad_view::worker::use_own_worker();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ecad: {e}");
            ExitCode::from(2)
        }
    }
}
