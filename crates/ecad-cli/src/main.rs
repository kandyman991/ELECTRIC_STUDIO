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
  ecad check <project-dir> [--json] [--strict] [--release]
      Summarise the project and list its errors and warnings.
      --json    print the result as JSON instead
      --strict  fail on warnings too
  ecad export <project-dir> <out-dir> [--only <formats>] [--release]
      Write drawings.pdf (all pages), a DXF, DWG and PNG per page, the reports as CSV,
      and the PLC channels as a TIA Portal tag table (tia-plc-tags.xlsx).
      --only    a comma-separated subset of: pdf, dxf, dwg, png, reports
      --release refuse unless the release validation passes; otherwise the export is a
                draft (unreleased)
  ecad report <project-dir> <report>
      Print one report as CSV: wire-list, bom, terminal-diagram, plc-io-list,
      cable-list, table-of-contents, devices-without-parts, panel-cut-list,
      wire-totals, netlist
  ecad import-tia <project-dir> <tags.xlsx> [--apply]
      Take PLC symbols and comments from a TIA Portal PLC tags export, matched by
      address. Tags at other addresses are listed, not added.
  ecad import-dxf <project-dir> <page> <drawing.dxf|.dwg> [--units mm|cm|m|inch|feet] [--apply]
      Put an existing drawing behind a page (its name, or #n) to redraw over. The file is
      read in a separate worker process.
  ecad remove-underlay <project-dir> <page> [--apply]
      Take a page's underlay away.
  ecad new-sample <project-dir> [start|contacts|potentials|terminals|plc|power|library|reference|conveyor]
      Write a sample project (default: start).
  ecad numbering <project-dir> [--json]
      List the numbering policies, capacities, reserved and retired identifiers.
  ecad reserve <project-dir> <identifier>... [--note <text>] [--apply]
  ecad unreserve <project-dir> <identifier>... [--apply]
  ecad retire <project-dir> <identifier>... [--note <text>] [--apply]
  ecad reclaim <project-dir> <identifier>... [--apply]
      Keep identifiers free for an extension, release them, retire them, or allow a retired
      one to be used again. Identifiers: tag:-K10, tag:-K10..-K19, cable:-W9, wire:W5,
      terminal:-X3:7..9, address:I1.0..I1.3, core:-W1:3, page:3a.
  ecad renumber <project-dir> wires|devices|pages|terminals:<strip> [--json]
      Preview renumbering; nothing changes. Renumbering is a global operation: prepare it
      (`ecad prepare <dir> renumber_devices`), have it approved in the app, then apply it
      with `ecad apply`.
  ecad run <project-dir> <operation> [name=value...] [--apply] [--json]
      Run any operation of the engineering registry (`ecad operations` lists them).

  Engineering interface (AI0). Command-line use falls into three classes (D42):
  - read-only: check, export, report, numbering, renumber previews, operations, proposals,
    preview, revision, verify, and every editing command without --apply (a dry run that
    prints what would change);
  - trusted automation: --apply runs an edit, numbering or import operation only if the
    project's automation scope (set by a person in the app) allows its class, records a
    change record with the actor (OS user, host, command line, version) and saves; global
    operations (renumbering) and safety-related changes are outside any scope;
  - consequential changes: `ecad prepare` stores a change set as a proposal; a person
    reviews it in the app and approves it for the CLI; `ecad apply <dir> <id> --expect
    <revision>` applies exactly that proposal at exactly that revision, consuming the
    approval in the same transaction. AI and MCP proposals are applied in the app only.
  This is application-level enforcement, not a security boundary (D31).

  ecad operations [--json]
      List the registry: queries and operations with their classes and arguments.
  ecad prepare <project-dir> <operation> [name=value...] [--title <t>] [--rationale <r>] [--json]
      Prepare a change set and store it as a proposal (nothing else changes).
  ecad proposals <project-dir> [--json]
  ecad preview <project-dir> <proposal-id> [--json]
      List proposals; show one's preview against the project now.
  ecad apply <project-dir> <proposal-id> --expect <revision>
      Apply an approved proposal (see above). Refused, and recorded, if anything differs.
  ecad revision <project-dir>
      Print the committed revision (for --expect).
  ecad verify <project-dir> [--json]
      Compare the folder with its committed manifest: outside changes, missing, damaged
      and unexpected files. Exit 1 if it doesn't match.
  ecad check <project-dir> --release
      The shared release validation: exit 1 if anything blocks a release.
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

/// Trusted automation (D42): a dry run without `--apply`; with it, within the project's
/// automation scope, recorded and saved.
fn automate(dir: &str, op: &str, v: serde_json::Value, a: &Args, args: &[String]) -> Result<ExitCode, String> {
    if a.apply {
        before_save(dir, a.accept_conversion)?;
    }
    let done = ecad_editor::change::automate(Path::new(dir), op, &v, a.apply, ecad_editor::change::Actor::cli(args))?;
    if a.json {
        println!(
            "{}",
            serde_json::json!({ "applied": done.record.is_some(), "result": done.result, "impact": done.impact })
        );
    } else {
        if let Some(notes) = done.result.get("notes").and_then(|n| n.as_array()) {
            for n in notes.iter().filter_map(|n| n.as_str()) {
                println!("{n}");
            }
        }
        if let Some(updated) = done.result.get("updated") {
            println!(
                "updated {updated} PLC channels, {} already matched, {} tags not inputs or outputs",
                done.result["unchanged"], done.result["other"]
            );
        }
        if let Some(m) = done.result.get("message").and_then(|m| m.as_str()) {
            println!("{m}");
        }
        for verb in ["reserve", "unreserve", "retire", "reclaim"] {
            if let Some(list) = done.result.get(verb).and_then(|l| l.as_array()) {
                let names: Vec<&str> = list.iter().filter_map(|v| v.as_str()).collect();
                let done = if verb == "reclaim" { "reclaimed".to_owned() } else { format!("{verb}d") };
                println!("{done} {}", names.join(", "));
            }
        }
        println!("{}", done.impact.summary());
        match &done.record {
            Some(r) => println!("applied; change record {}", r.id),
            None => eprintln!("dry run: nothing changed (add --apply, within the project's automation scope)"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `name=value` arguments for an operation, typed by its registry entry.
fn op_args(op: &str, kv: &[&str]) -> Result<serde_json::Value, String> {
    let def = ecad_editor::registry::find(op)
        .ok_or_else(|| format!("unknown operation `{op}` (`ecad operations` lists them)"))?;
    let mut map = serde_json::Map::new();
    for item in kv {
        let (k, v) = item.split_once('=').ok_or_else(|| format!("`{item}`: write name=value"))?;
        let arg = def.args.iter().find(|x| x.name == k).ok_or_else(|| format!("`{op}` has no argument `{k}`"))?;
        let value = match arg.kind {
            ecad_editor::registry::Kind::Number => {
                serde_json::json!(v.parse::<f64>().map_err(|_| format!("`{k}` must be a number"))?)
            }
            ecad_editor::registry::Kind::Structured(_) => {
                serde_json::from_str(v).map_err(|e| format!("`{k}` must be a JSON record: {e}"))?
            }
            _ => serde_json::json!(v),
        };
        map.insert(k.to_owned(), value);
    }
    Ok(serde_json::Value::Object(map))
}

fn operations(json: bool) -> Result<ExitCode, String> {
    let defs = ecad_editor::registry::DEFS;
    if json {
        let list: Vec<serde_json::Value> = defs
            .iter()
            .map(|d| serde_json::json!({ "name": d.name, "title": d.title, "class": d.class, "mcp": d.mcp, "schema": ecad_editor::registry::schema(d) }))
            .collect();
        println!("{}", serde_json::json!({ "registry": list }));
    } else {
        for d in defs {
            let args: Vec<String> =
                d.args.iter().map(|x| if x.required { x.name.to_owned() } else { format!("[{}]", x.name) }).collect();
            println!("{:<22} {:<9} {}", d.name, format!("{:?}", d.class).to_lowercase(), args.join(" "));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn release(dir: &Path, json: bool) -> Result<ExitCode, String> {
    let project = load(dir)?;
    let r = ecad_rules::release::validate(&project);
    let revision = ecad_project::state::committed(dir).map_err(|e| e.to_string())?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "revision": revision.to_string(), "releasable": r.releasable(), "blocking": r.blocking, "note": ecad_rules::release::NOTE })
        );
    } else {
        println!("release check at {revision}: {}", r.summary());
    }
    Ok(if r.releasable() { ExitCode::SUCCESS } else { ExitCode::from(1) })
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
    expect: Option<String>,
    title: Option<String>,
    rationale: Option<String>,
    release: bool,
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
        expect: None,
        title: None,
        rationale: None,
        release: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--strict" => out.strict = true,
            "--accept-conversion" => out.accept_conversion = true,
            "--apply" => out.apply = true,
            "--note" => out.note = Some(it.next().ok_or("--note needs a text")?.clone()),
            "--expect" => {
                out.expect = Some(it.next().ok_or("--expect needs a revision (`ecad revision <dir>`)")?.clone())
            }
            "--title" => out.title = Some(it.next().ok_or("--title needs a text")?.clone()),
            "--rationale" => out.rationale = Some(it.next().ok_or("--rationale needs a text")?.clone()),
            "--release" => out.release = true,
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
            flags(&["--json", "--strict", "--release"])?;
            if a.release {
                return release(Path::new(dir), a.json);
            }
            check(&load(Path::new(dir))?, a.json, a.strict)
        }
        ["export", dir, out] => {
            flags(&["--only", "--release"])?;
            let (formats, reports) = selection(a.only.as_deref())?;
            let project = load(Path::new(dir))?;
            if a.release {
                let r = ecad_rules::release::validate(&project);
                if !r.releasable() {
                    return Err(format!("not exported for release: {}", r.summary()));
                }
                eprintln!(
                    "release export at {}: {}",
                    ecad_project::state::committed(Path::new(dir)).map_err(|e| e.to_string())?,
                    ecad_rules::release::NOTE
                );
            } else {
                eprintln!("draft export (unreleased); `--release` validates for release");
            }
            export(&project, Path::new(out), formats, reports)?;
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
            flags(&["--apply", "--json"])?;
            automate(dir, "import_tia", serde_json::json!({ "file": file }), &a, args)
        }
        ["import-dxf", dir, page, file] => {
            flags(&["--units", "--apply", "--json"])?;
            let mut v = serde_json::json!({ "page": page, "file": file });
            if let Some(u) = &a.units {
                v["units"] = serde_json::json!(u);
            }
            automate(dir, "import_dxf", v, &a, args)
        }
        ["remove-underlay", dir, page] => {
            flags(&["--apply", "--json"])?;
            automate(dir, "remove_underlay", serde_json::json!({ "page": page }), &a, args)
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
            let path = Path::new(dir);
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
            let _lock = ecad_project::store::Lock::take(path).map_err(|e| e.to_string())?;
            // Creation is not an edit operation. Never use it to replace an existing
            // project (including format 1), pending recovery, or any user's files.
            for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_name() != ecad_project::store::LOCK {
                    return Err("new-sample requires a new or empty project folder".into());
                }
            }
            ecad_project::save_locked(&project, path, Default::default()).map_err(|e| e.to_string())?;
            println!("wrote project {dir}");
            Ok(ExitCode::SUCCESS)
        }
        ["numbering", dir] => {
            flags(&["--json"])?;
            let project = load(Path::new(dir))?;
            numbering(&project, a.json)
        }
        [cmd @ ("reserve" | "unreserve" | "retire" | "reclaim"), dir, specs @ ..] if !specs.is_empty() => {
            flags(if matches!(*cmd, "reserve" | "retire") {
                &["--note", "--apply", "--json"]
            } else {
                &["--apply", "--json"]
            })?;
            let mut v = serde_json::json!({ "identifiers": specs.join(" ") });
            if let Some(n) = &a.note {
                v["note"] = serde_json::json!(n);
            }
            automate(dir, cmd, v, &a, args)
        }
        ["renumber", dir, what] => {
            flags(&["--json", "--apply"])?;
            if a.apply {
                return Err(format!(
                    "renumbering is a global operation, outside any automation scope: `ecad prepare {dir} renumber_{} …`, approve it in the app, then `ecad apply`",
                    what.split(':').next().unwrap_or(what)
                ));
            }
            let project = load(Path::new(dir))?;
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
                println!("{}", serde_json::json!({ "changes": changes, "applied": false }));
            } else {
                for c in &r.changes {
                    println!("{}: {} -> {}", c.what, c.from, c.to);
                }
                eprintln!("{} changes previewed; nothing changed", r.changes.len());
            }
            Ok(ExitCode::SUCCESS)
        }
        ["run", dir, op, kv @ ..] => {
            flags(&["--apply", "--json"])?;
            let v = op_args(op, kv)?;
            automate(dir, op, v, &a, args)
        }
        ["operations"] => {
            flags(&["--json"])?;
            operations(a.json)
        }
        ["prepare", dir, op, kv @ ..] => {
            flags(&["--title", "--rationale", "--json"])?;
            let v = op_args(op, kv)?;
            let d = Path::new(dir);
            let (project, _) = ecad_project::load_checked(d).map_err(|e| e.to_string())?;
            let base = ecad_editor::change::adopt(d, ecad_editor::change::Actor::cli(args))?;
            let call = ecad_editor::change::OpCall { op: (*op).to_owned(), args: v };
            let cs = ecad_editor::change::prepare(
                &project,
                base,
                ecad_editor::change::Actor::cli(args),
                a.title.as_deref().unwrap_or_default(),
                a.rationale.as_deref().unwrap_or_default(),
                vec![call],
            )?;
            let impact = ecad_editor::change::preview(&project, &cs, &ecad_project::state::Quiet)?;
            ecad_editor::change::save_proposal(d, &cs)?;
            if a.json {
                println!(
                    "{}",
                    serde_json::json!({ "proposal": cs.id, "hash": cs.hash(), "base": cs.base.to_string(), "impact": impact })
                );
            } else {
                println!("proposal {} ({})", cs.id, cs.title);
                println!("hash {}", cs.hash());
                println!("base {}", cs.base);
                println!("impact: {}", impact.summary());
                println!(
                    "next: review and approve it for the CLI in the app, then `ecad apply {dir} {} --expect {}`",
                    cs.id, cs.base
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ["proposals", dir] => {
            flags(&["--json"])?;
            let list = ecad_editor::change::proposals(Path::new(dir))?;
            if a.json {
                let rows: Vec<serde_json::Value> = list
                    .iter()
                    .map(|l| serde_json::json!({ "id": l.change_set.id, "title": l.change_set.title, "actor": l.change_set.actor.label(), "status": l.status, "hash": l.hash, "base": l.change_set.base.to_string() }))
                    .collect();
                println!("{}", serde_json::json!({ "proposals": rows }));
            } else {
                for l in &list {
                    println!(
                        "{} {:?} {} ({})",
                        l.change_set.id,
                        l.status,
                        l.change_set.title,
                        l.change_set.actor.label()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ["preview", dir, id] => {
            flags(&["--json"])?;
            let d = Path::new(dir);
            let cs = ecad_editor::change::read_proposal(d, id)?;
            let project = load(d)?;
            let impact = ecad_editor::change::preview(&project, &cs, &ecad_project::state::Quiet)?;
            if a.json {
                println!(
                    "{}",
                    serde_json::json!({ "proposal": cs.id, "hash": cs.hash(), "base": cs.base.to_string(), "impact": impact })
                );
            } else {
                println!("{} ({}), by {}", cs.title, cs.id, cs.actor.label());
                for i in impact.devices.iter().chain(&impact.terminals).chain(&impact.wires) {
                    println!("  {} {}: {} -> {}", i.change, i.id, i.before, i.after);
                }
                println!("impact: {}", impact.summary());
            }
            Ok(ExitCode::SUCCESS)
        }
        ["apply", dir, id] => {
            flags(&["--expect"])?;
            let expect: ecad_project::Revision = a
                .expect
                .as_deref()
                .ok_or("`ecad apply` needs --expect <revision> (`ecad revision <dir>` prints it)")?
                .parse()?;
            before_save(dir, a.accept_conversion)?;
            let record = ecad_editor::change::apply_approved(
                Path::new(dir),
                id,
                &expect,
                ecad_editor::change::Actor::cli(args),
            )?;
            println!("applied {} ({}); change record {}", record.title, id, record.id);
            println!("revision {}", ecad_project::state::committed(Path::new(dir)).map_err(|e| e.to_string())?);
            Ok(ExitCode::SUCCESS)
        }
        ["revision", dir] => {
            flags(&[])?;
            println!("{}", ecad_project::state::committed(Path::new(dir)).map_err(|e| e.to_string())?);
            Ok(ExitCode::SUCCESS)
        }
        ["verify", dir] => {
            flags(&["--json"])?;
            let v =
                ecad_project::state::verify(Path::new(dir), &ecad_project::state::Quiet).map_err(|e| e.to_string())?;
            if a.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "committed": v.committed.as_ref().map(ToString::to_string),
                        "clean": v.clean(),
                        "modified": v.modified, "missing": v.missing, "added": v.added,
                        "unexpected": v.unexpected, "damaged": v.damaged_objects, "unsafe": v.unsafe_paths,
                    })
                );
            } else {
                println!("{}", v.summary());
            }
            Ok(if v.committed.is_some() && !v.clean() { ExitCode::from(1) } else { ExitCode::SUCCESS })
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
                "run",
                "operations",
                "prepare",
                "proposals",
                "preview",
                "apply",
                "revision",
                "verify",
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
