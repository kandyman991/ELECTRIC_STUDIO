//! The `ecad` binary as a script or CI job would run it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use std::path::Path;
use std::process::{Command, Output};

fn ecad(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ecad")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// A person allows trusted automation classes in the app (the desktop session) and saves.
fn allow(dir: &str, classes: &[ecad_model::policy::AutomationClass]) {
    let mut s = ecad_editor::session::Session::open(Path::new(dir)).unwrap();
    s.handle(ecad_editor::session::UiEvent::SetAutomation { classes: classes.to_vec() });
    s.handle(ecad_editor::session::UiEvent::Save);
    assert!(s.frame(None, [0, 0, 0]).status.starts_with("Saved"), "{}", s.frame(None, [0, 0, 0]).status);
}

fn sample(dir: &Path, kind: &str) -> String {
    let p = dir.join(kind);
    let s = p.to_str().unwrap().to_owned();
    assert!(ecad(&["new-sample", &s, kind]).status.success());
    s
}

#[test]
fn check_exits_by_what_it_finds() {
    let dir = tempfile::tempdir().unwrap();
    let start = sample(dir.path(), "start");
    let o = ecad(&["check", &start]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(text(&o).ends_with("0 errors, 0 warnings\n"), "{}", text(&o));

    // Warnings pass, unless --strict.
    let plc = sample(dir.path(), "plc");
    assert_eq!(ecad(&["check", &plc]).status.code(), Some(0));
    assert_eq!(ecad(&["check", &plc, "--strict"]).status.code(), Some(1));

    let o = ecad(&["check", &plc, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((v["pages"].as_u64(), v["errors"].as_u64()), (Some(3), Some(0)));
    let first = &v["checks"][0];
    assert_eq!(first["severity"], "warning");
    assert!(first["target"]["kind"] == "function" || first["target"]["kind"] == "connection", "{first}");
}

#[test]
fn export_writes_what_only_asks_for() {
    let dir = tempfile::tempdir().unwrap();
    let plc = sample(dir.path(), "plc");
    let out = dir.path().join("out");
    let o = ecad(&["export", &plc, out.to_str().unwrap(), "--only", "pdf,reports"]);
    assert!(o.status.success(), "{}", text(&o));
    let mut names: Vec<String> =
        std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "bom.csv",
            "devices-without-parts.csv",
            "drawings.pdf",
            "plc-io-list.csv",
            "table-of-contents.csv",
            "terminal-diagram.csv",
            "tia-plc-tags.xlsx",
            "wire-list.csv"
        ]
    );
    let o = ecad(&["export", &plc, out.to_str().unwrap(), "--only", "svg"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("unknown format `svg`"), "{}", text(&o));
}

#[test]
fn tia_tags_go_out_and_renamed_symbols_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let plc = sample(dir.path(), "plc");
    let out = dir.path().join("out");
    assert!(ecad(&["export", &plc, out.to_str().unwrap(), "--only", "reports"]).status.success());
    let bytes = std::fs::read(out.join("tia-plc-tags.xlsx")).unwrap();
    let mut tags = ecad_exchange::tia::read_xlsx(&bytes).unwrap().tags;
    let door = tags.iter_mut().find(|t| t.address == "%I0.0").unwrap();
    assert_eq!(door.name, "Door_Closed");
    door.name = "Guard_Door_Closed".into();
    tags.push(ecad_exchange::tia::Tag { name: "Spare".into(), address: "%I7.7".into(), ..tags[0].clone() });
    let file = dir.path().join("from-tia.xlsx");
    std::fs::write(&file, ecad_exchange::tia::to_xlsx(&tags).unwrap()).unwrap();
    // Without --apply a dry run; outside the automation scope, refused.
    let o = ecad(&["import-tia", &plc, file.to_str().unwrap()]);
    assert!(o.status.success() && text(&o).contains("dry run"), "{}", text(&o));
    let o = ecad(&["import-tia", &plc, file.to_str().unwrap(), "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    allow(&plc, &[ecad_model::policy::AutomationClass::Import]);
    let o = ecad(&["import-tia", &plc, file.to_str().unwrap(), "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("no channel at %I7.7 Spare"), "{}", text(&o));
    assert!(text(&o).contains("updated 1 PLC channels"), "{}", text(&o));
    let csv = String::from_utf8(ecad(&["report", &plc, "plc-io-list"]).stdout).unwrap();
    assert!(csv.contains("-KF1,0,DI,I0.0,Guard_Door_Closed,"), "{csv}");
    // A file that isn't a tag table fails without touching the project.
    let o = ecad(&["import-tia", &plc, out.join("bom.csv").to_str().unwrap(), "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    // Exporting a project without PLC into the same folder removes the older table.
    let start = sample(dir.path(), "start");
    let o = ecad(&["export", &start, out.to_str().unwrap(), "--only", "reports"]);
    assert!(text(&o).contains("removed the older tia-plc-tags.xlsx"), "{}", text(&o));
    assert!(!out.join("tia-plc-tags.xlsx").exists());
}

#[test]
fn a_dxf_goes_behind_a_page_and_comes_off_again() {
    let dir = tempfile::tempdir().unwrap();
    let start = sample(dir.path(), "start");
    // A drawing to trace: one of Electric Studio's own pages, exported as DXF.
    let out = dir.path().join("out");
    assert!(ecad(&["export", &start, out.to_str().unwrap(), "--only", "dxf"]).status.success());
    let dxf = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "dxf"))
        .unwrap();
    allow(&start, &[ecad_model::policy::AutomationClass::Import]);
    let o = ecad(&["import-dxf", &start, "#1", dxf.to_str().unwrap(), "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("1:1 as drawn"), "{}", text(&o));
    let underlays = Path::new(&start).join("underlays");
    assert_eq!(std::fs::read_dir(&underlays).unwrap().count(), 1);
    // The check doesn't see it; the export doesn't draw it.
    assert!(ecad(&["check", &start]).status.success());
    let o = ecad(&["import-dxf", &start, "Z9", dxf.to_str().unwrap(), "--apply"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no page `Z9`"), "{}", text(&o));
    assert!(ecad(&["remove-underlay", &start, "#1", "--apply"]).status.success());
    assert_eq!(std::fs::read_dir(&underlays).unwrap().count(), 0);
    assert_eq!(ecad(&["remove-underlay", &start, "#1", "--apply"]).status.code(), Some(2));
}

#[test]
fn a_dwg_and_chosen_units_go_through_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let start = sample(dir.path(), "start");
    let out = dir.path().join("out");
    assert!(ecad(&["export", &start, out.to_str().unwrap(), "--only", "dwg"]).status.success());
    let dwg = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "dwg"))
        .unwrap();
    // The DWG reader runs only in a worker process: `ecad` starts a copy of itself.
    allow(&start, &[ecad_model::policy::AutomationClass::Import]);
    let o = ecad(&["import-dxf", &start, "#1", dwg.to_str().unwrap(), "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("in mm, 1:1 as drawn"), "{}", text(&o));
    // Chosen units win over the file's.
    let o = ecad(&["import-dxf", &start, "#1", dwg.to_str().unwrap(), "--units", "inch", "--apply"]);
    assert!(text(&o).contains("in inch as chosen") && text(&o).contains("shrunk"), "{}", text(&o));
    let o = ecad(&["import-dxf", &start, "#1", dwg.to_str().unwrap(), "--units", "furlong"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("unknown unit `furlong`"), "{}", text(&o));
    assert_eq!(ecad(&["check", &start, "--units", "mm"]).status.code(), Some(2), "--units is only for import-dxf");
}

#[test]
fn a_crash_while_reading_a_drawing_costs_only_that_import() {
    // The worker is this crate's `ecad` program; the test harness can't be one.
    ecad_view::worker::use_worker(env!("CARGO_BIN_EXE_ecad").into());
    let e = ecad_view::worker::import_underlay(b"anything", ecad_view::worker::CRASH_FOR_TEST).unwrap_err();
    assert!(e.to_string().contains("stopped the import worker"), "{e}");
    // This process carries on, and the next import works.
    let p = ecad_model::sample::start_circuit().unwrap();
    let dxf = ecad_view::page_dxf(&p, ecad_model::sample::PAGE_1).unwrap();
    let imported = ecad_view::worker::import_underlay(dxf.as_bytes(), "page.dxf").unwrap();
    assert!(imported.shapes.lines.len() > 10);
}

#[test]
fn report_prints_one_csv() {
    let dir = tempfile::tempdir().unwrap();
    let plc = sample(dir.path(), "plc");
    let o = ecad(&["report", &plc, "plc-io-list"]);
    assert!(o.status.success());
    let csv = String::from_utf8(o.stdout).unwrap();
    assert!(csv.starts_with("card,channel,kind,address,symbol,comment,wired_to,location\n-KF1,0,DI,I0.0"), "{csv}");
    // An empty report is its header.
    let start = sample(dir.path(), "start");
    assert_eq!(String::from_utf8(ecad(&["report", &start, "plc-io-list"]).stdout).unwrap().lines().count(), 1);
    assert_eq!(ecad(&["report", &start, "motor-list"]).status.code(), Some(2));
}

#[test]
fn usage_errors_exit_with_2() {
    assert_eq!(ecad(&[]).status.code(), Some(2));
    assert_eq!(ecad(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(ecad(&["check"]).status.code(), Some(2));
    assert_eq!(ecad(&["check", "x", "--only", "pdf"]).status.code(), Some(2));
    assert_eq!(ecad(&["check", "/no/such/project"]).status.code(), Some(2));
    let help = ecad(&["--help"]);
    assert!(help.status.success() && text(&help).contains("ecad export <project-dir> <out-dir>"));
    assert!(text(&ecad(&["--version"])).starts_with("ecad "));
}

#[test]
fn numbering_is_listed_reserved_and_renumbering_is_previewed_before_it_applies() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = sample(tmp.path(), "conveyor");
    allow(&dir, &[ecad_model::policy::AutomationClass::Numbering]);
    let o = ecad(&["reserve", &dir, "tag:-K10..-K12", "address:I3.0..I3.3", "--note", "extension", "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("reserved -K10, -K11, -K12, I3.0"), "{}", text(&o));
    let o = ecad(&["numbering", &dir, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["reserved"].as_array().unwrap().len(), 7, "{v}");
    assert_eq!(v["reserved"][0]["note"], "extension");
    // Reserving one in use is refused and saves nothing.
    let o = ecad(&["reserve", &dir, "tag:-K1", "--apply"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let o = ecad(&["unreserve", &dir, "tag:-K12", "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = ecad(&["retire", &dir, "wire:W999", "--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = ecad(&["reclaim", &dir, "wire:W999", "--apply"]);
    assert!(text(&o).contains("reclaimed wire W999"), "{}", text(&o));
    // A renumbering preview changes nothing; --apply is refused (a global operation).
    let before = std::fs::read(Path::new(&dir).join("project.json")).unwrap();
    let o = ecad(&["renumber", &dir, "pages", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["applied"], false);
    assert_eq!(std::fs::read(Path::new(&dir).join("project.json")).unwrap(), before);
    let o = ecad(&["renumber", &dir, "terminals:-NOPE"]);
    assert_eq!(o.status.code(), Some(2));
    let o = ecad(&["renumber", &dir, "sideways"]);
    assert_eq!(o.status.code(), Some(2));
}
