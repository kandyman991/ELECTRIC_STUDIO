//! `ecad library ...` and `ecad import-parts`: the components library from the command line.

use crate::Args;
use ecad_exchange::parts::{DataColumns, Field, Options, PartsList};
use ecad_model::{Part, PartsLibrary, parts_library};
use std::path::Path;
use std::process::ExitCode;

/// The options that say how to read a parts list.
pub const READING: [&str; 6] = ["--manufacturer", "--sheet", "--header-row", "--map", "--data", "--no-data"];

/// How to read a list, from the command line's options.
pub fn reading(a: &Args) -> Result<Options, String> {
    let header_row = match a.value("--header-row") {
        None => None,
        Some(n) => Some(n.parse::<usize>().ok().filter(|n| *n > 0).ok_or(format!("`{n}` isn't a row number"))?),
    };
    let mut map = Vec::new();
    for m in a.values("--map") {
        let names = Field::ALL.map(Field::name).join(", ");
        let (field, title) =
            m.split_once('=').ok_or(format!("--map `{m}`: write field=title, e.g. number=Codice ({names})"))?;
        let field = Field::from_name(field).ok_or(format!("--map: `{field}` isn't a field ({names})"))?;
        map.push((field, title.trim().to_owned()));
    }
    let data = match (a.has("--no-data"), a.value("--data")) {
        (true, Some(_)) => return Err("--data and --no-data don't go together".into()),
        (true, None) => DataColumns::None,
        (false, Some(list)) => DataColumns::Only(list.split(',').map(|t| t.trim().to_owned()).collect()),
        (false, None) => DataColumns::Rest,
    };
    Ok(Options {
        manufacturer: a.value("--manufacturer").map(str::to_owned),
        sheet: a.value("--sheet").map(str::to_owned),
        header_row,
        map,
        data,
    })
}

/// Read a list and say what was found in it.
fn read_list(file: &str, options: &Options) -> Result<PartsList, String> {
    let path = Path::new(file);
    let bytes = ecad_project::read_bounded(path, ecad_exchange::parts::MAX_FILE).map_err(|e| e.to_string())?;
    let list = ecad_exchange::parts::read_parts(&bytes, file, options).map_err(|e| format!("{file}: {e}"))?;
    let at = match &list.sheet {
        Some(s) => format!("sheet `{s}`, header in row {}", list.header_row),
        None => format!("header in line {}", list.header_row),
    };
    println!("read {} parts from {file} ({at})", list.parts.len());
    let columns: Vec<String> = list.columns.iter().map(|(t, what)| format!("{t} → {what}")).collect();
    println!("columns: {}", columns.join(", "));
    const SHOWN: usize = 20;
    for s in list.skipped.iter().take(SHOWN) {
        println!("left out {s}");
    }
    if list.skipped.len() > SHOWN {
        println!("... and {} more rows left out", list.skipped.len() - SHOWN);
    }
    Ok(list)
}

fn load(dir: &str) -> Result<(PartsLibrary, ecad_project::Fingerprint), String> {
    ecad_project::library::load_library(Path::new(dir)).map_err(|e| e.to_string())
}

pub fn new(dir: &str, name: Option<&str>) -> Result<ExitCode, String> {
    let path = Path::new(dir);
    let name = name.map(str::to_owned).unwrap_or_else(|| {
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Components".into())
    });
    ecad_project::library::create_library(path, &name).map_err(|e| e.to_string())?;
    println!("made components library `{name}` in {dir}");
    Ok(ExitCode::SUCCESS)
}

pub fn import(dir: &str, file: &str, options: &Options, dry_run: bool) -> Result<ExitCode, String> {
    let (mut library, seen) = load(dir)?;
    let list = read_list(file, options)?;
    let got = library.import(list.parts);
    for r in &got.rejected {
        println!("not imported: {r}");
    }
    let would = if dry_run { "would be " } else { "" };
    if !dry_run && (got.added > 0 || got.updated > 0) {
        ecad_project::library::save_library(&library, Path::new(dir), Some(seen)).map_err(|e| e.to_string())?;
    }
    println!(
        "{} parts {would}added, {} {would}updated, {} already up to date; the library has {}",
        got.added,
        got.updated,
        got.unchanged,
        if dry_run { "not changed".to_owned() } else { format!("{} parts", library.len()) }
    );
    Ok(ExitCode::SUCCESS)
}

fn line(p: &Part) -> String {
    format!("{}\t{}\t{}", p.number, p.manufacturer, p.description)
}

pub fn search(dir: &str, words: &[&str], limit: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let limit = match limit {
        None => 50,
        Some(n) => n.parse::<usize>().ok().filter(|n| *n > 0).ok_or(format!("`{n}` isn't a number above 0"))?,
    };
    let (library, _) = load(dir)?;
    let hits = library.search(&words.join(" "), limit);
    if json {
        println!("{}", serde_json::to_string_pretty(&hits).map_err(|e| e.to_string())?);
    } else {
        for p in &hits {
            println!("{}", line(p));
        }
        println!("{} found", hits.len());
    }
    Ok(ExitCode::SUCCESS)
}

/// The one part with this number (and manufacturer, if given).
fn one<'a>(library: &'a PartsLibrary, number: &str, maker: Option<&str>) -> Result<&'a Part, String> {
    match maker {
        Some(m) => library.get(m, number).ok_or(format!("the library has no {m} {number}")),
        None => match library.by_number(number).as_slice() {
            [] => Err(format!("the library has no part {number}")),
            [p] => Ok(p),
            many => {
                let makers: Vec<&str> = many.iter().map(|p| p.manufacturer.as_str()).collect();
                Err(format!("{number} is in the library from {}; add --manufacturer", makers.join(", ")))
            }
        },
    }
}

pub fn show(dir: &str, number: &str, maker: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let (library, _) = load(dir)?;
    let p = one(&library, number, maker)?;
    if json {
        println!("{}", serde_json::to_string_pretty(p).map_err(|e| e.to_string())?);
        return Ok(ExitCode::SUCCESS);
    }
    let size = ecad_model::parts::format_size(p);
    for (label, value) in [
        ("number", p.number.as_str()),
        ("manufacturer", &p.manufacturer),
        ("description", &p.description),
        ("code", &p.code),
        ("class", &p.class),
        ("contacts", &p.contacts.join(" ")),
        ("size", &size),
        ("datasheet", &p.datasheet),
    ] {
        if !value.is_empty() {
            println!("{label}: {value}");
        }
    }
    for d in &p.data {
        println!("  {}: {}", d.name, d.value);
    }
    Ok(ExitCode::SUCCESS)
}

fn report(t: &parts_library::Taken, dry_run: bool) {
    let would = if dry_run { "would be " } else { "" };
    for (list, what) in [(&t.added, "added"), (&t.updated, "updated"), (&t.unchanged, "already up to date")] {
        if !list.is_empty() {
            let what = if what.starts_with("already") { what.to_owned() } else { format!("{would}{what}") };
            println!("{what}: {}", list.join(", "));
        }
    }
}

/// Apply a taking or refresh to the project and save it, unless it is a dry run.
fn save_taken(
    project_dir: &str,
    mut project: ecad_model::Project,
    seen: ecad_project::Fingerprint,
    taken: &parts_library::Taken,
    dry_run: bool,
) -> Result<(), String> {
    if let (Some(command), false) = (&taken.command, dry_run) {
        project.apply(command.clone()).map_err(|e| e.to_string())?;
        ecad_project::save_checked(&project, Path::new(project_dir), Some(seen)).map_err(|e| e.to_string())?;
    }
    report(taken, dry_run);
    Ok(())
}

pub fn add(
    project_dir: &str,
    dir: &str,
    numbers: &[&str],
    maker: Option<&str>,
    dry_run: bool,
) -> Result<ExitCode, String> {
    let (library, _) = load(dir)?;
    let (project, seen) = ecad_project::load_checked(Path::new(project_dir)).map_err(|e| e.to_string())?;
    let wanted: Vec<(Option<&str>, &str)> = numbers.iter().map(|n| (maker, *n)).collect();
    let taken = parts_library::take(&project, &library, &wanted).map_err(|e| e.to_string())?;
    save_taken(project_dir, project, seen, &taken, dry_run)?;
    Ok(ExitCode::SUCCESS)
}

pub fn update(project_dir: &str, dir: &str, dry_run: bool) -> Result<ExitCode, String> {
    let (library, _) = load(dir)?;
    let (project, seen) = ecad_project::load_checked(Path::new(project_dir)).map_err(|e| e.to_string())?;
    let (taken, missing) = parts_library::refresh(&project, &library).map_err(|e| e.to_string())?;
    save_taken(project_dir, project, seen, &taken, dry_run)?;
    if !missing.is_empty() {
        println!("not in the library: {}", missing.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

/// `import-parts`: a list straight into a project's catalogue.
pub fn import_into_project(
    project_dir: &str,
    file: &str,
    options: &Options,
    dry_run: bool,
) -> Result<ExitCode, String> {
    let (project, seen) = ecad_project::load_checked(Path::new(project_dir)).map_err(|e| e.to_string())?;
    let list = read_list(file, options)?;
    let taken = parts_library::import_into(&project, list.parts).map_err(|e| e.to_string())?;
    save_taken(project_dir, project, seen, &taken, dry_run)?;
    Ok(ExitCode::SUCCESS)
}
