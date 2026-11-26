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
    let mut tags = ecad_exchange::tia::read_xlsx(&bytes).unwrap();
    let door = tags.iter_mut().find(|t| t.address == "%I0.0").unwrap();
    assert_eq!(door.name, "Door_Closed");
    door.name = "Guard_Door_Closed".into();
    tags.push(ecad_exchange::tia::Tag { name: "Spare".into(), address: "%I7.7".into(), ..tags[0].clone() });
    let file = dir.path().join("from-tia.xlsx");
    std::fs::write(&file, ecad_exchange::tia::to_xlsx(&tags).unwrap()).unwrap();
    let o = ecad(&["import-tia", &plc, file.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("no channel at %I7.7 Spare"), "{}", text(&o));
    assert!(text(&o).contains("updated 1 PLC channels"), "{}", text(&o));
    let csv = String::from_utf8(ecad(&["report", &plc, "plc-io-list"]).stdout).unwrap();
    assert!(csv.contains("-KF1,0,DI,I0.0,Guard_Door_Closed,"), "{csv}");
    // A file that isn't a tag table fails without touching the project.
    let o = ecad(&["import-tia", &plc, out.join("bom.csv").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
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
