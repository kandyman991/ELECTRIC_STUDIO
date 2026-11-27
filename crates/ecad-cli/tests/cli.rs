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
    let mut tags = ecad_exchange::tia::read_xlsx(&bytes).unwrap().tags;
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
    let o = ecad(&["import-dxf", &start, "#1", dxf.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("1:1 as drawn"), "{}", text(&o));
    let underlays = Path::new(&start).join("underlays");
    assert_eq!(std::fs::read_dir(&underlays).unwrap().count(), 1);
    // The check doesn't see it; the export doesn't draw it.
    assert!(ecad(&["check", &start]).status.success());
    let o = ecad(&["import-dxf", &start, "Z9", dxf.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no page `Z9`"), "{}", text(&o));
    assert!(ecad(&["remove-underlay", &start, "#1"]).status.success());
    assert_eq!(std::fs::read_dir(&underlays).unwrap().count(), 0);
    assert_eq!(ecad(&["remove-underlay", &start, "#1"]).status.code(), Some(2));
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
    let o = ecad(&["import-dxf", &start, "#1", dwg.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("in mm, 1:1 as drawn"), "{}", text(&o));
    // Chosen units win over the file's.
    let o = ecad(&["import-dxf", &start, "#1", dwg.to_str().unwrap(), "--units", "inch"]);
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
fn a_components_library_from_a_price_list_to_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("components");
    let lib = lib.to_str().unwrap();
    let o = ecad(&["library", "new", lib, "--name", "Workshop"]);
    assert!(o.status.success(), "{}", text(&o));

    // A maker's price list: title rows, Italian titles, no manufacturer column.
    let list = dir.path().join("listino.csv");
    std::fs::write(
        &list,
        "Listino 2026;;;\nCodice articolo;Descrizione;Corrente nominale;Prezzo\n\
         EX-CT-9;Contattore 4 kW 24 V DC;9 A;20,50\nEX-CT-12;Contattore 5,5 kW;12 A;25\n;Gruppo relè;;\n",
    )
    .unwrap();
    let list = list.to_str().unwrap();
    let o = ecad(&["library", "import", lib, list, "--manufacturer", "Example", "--dry-run"]);
    assert!(text(&o).contains("2 parts would be added"), "{}", text(&o));
    let o = ecad(&["library", "import", lib, list, "--manufacturer", "Example", "--no-data", "--data", "x"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let o = ecad(&["library", "import", lib, list, "--manufacturer", "Example"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(
        t.contains("header in line 2") && t.contains("Codice articolo → number") && t.contains("Prezzo → data"),
        "{t}"
    );
    assert!(t.contains("left out row 5: no order number"), "{t}");
    assert!(t.contains("2 parts added, 0 updated") && t.contains("the library has 2 parts"), "{t}");
    assert!(dir.path().join("components/parts/example.json").is_file());

    let o = ecad(&["library", "search", lib, "contattore", "9"]);
    assert_eq!(text(&o), "EX-CT-9\tExample\tContattore 4 kW 24 V DC\n1 found\n");
    let o = ecad(&["library", "show", lib, "ex-ct-9"]);
    assert!(text(&o).contains("  Corrente nominale: 9 A"), "{}", text(&o));
    let o = ecad(&["library", "show", lib, "EX-CT-9", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["data"][1]["value"], "20,50");

    // Into a project, then the price changes and the project catches up.
    let project = sample(dir.path(), "start");
    let o = ecad(&["library", "add", &project, lib, "EX-CT-9"]);
    assert!(text(&o).contains("added: EX-CT-9"), "{}", text(&o));
    let catalogue = std::fs::read_to_string(Path::new(&project).join("parts.json")).unwrap();
    assert!(catalogue.contains("Corrente nominale"), "{catalogue}");
    let newer = dir.path().join("newer.xlsx");
    let mut book = rust_xlsxwriter::Workbook::new();
    let s = book.add_worksheet();
    for (c, t) in ["Bestellnummer", "Hersteller", "Preis"].iter().enumerate() {
        s.write(0, c as u16, *t).unwrap();
    }
    s.write(1, 0, "EX-CT-9").unwrap();
    s.write(1, 1, "example").unwrap();
    s.write(1, 2, 21.75).unwrap();
    book.save(&newer).unwrap();
    let o = ecad(&["library", "import", lib, newer.to_str().unwrap()]);
    assert!(text(&o).contains("0 parts added, 1 updated"), "{}", text(&o));
    let o = ecad(&["library", "update", &project, lib, "--dry-run"]);
    assert!(text(&o).contains("would be updated: EX-CT-9"), "{}", text(&o));
    let o = ecad(&["library", "update", &project, lib]);
    assert!(text(&o).contains("updated: EX-CT-9") && text(&o).contains("not in the library: EX-"), "{}", text(&o));
    let catalogue = std::fs::read_to_string(Path::new(&project).join("parts.json")).unwrap();
    assert!(catalogue.contains("21.75") && catalogue.contains("Corrente nominale"), "{catalogue}");

    // A list straight into a project, and arguments that are wrong.
    let o = ecad(&["import-parts", &project, list, "--map", "number=Codice articolo", "--data", "Prezzo"]);
    assert!(text(&o).contains("added: EX-CT-12"), "{}", text(&o));
    for bad in [
        vec!["library", "import", lib, list, "--map", "nothing=Codice"],
        vec!["library", "import", lib, list, "--map", "number"],
        vec!["library", "search", lib, "x", "--limit", "0"],
        vec!["library", "show", lib, "NOPE"],
        vec!["library", "add", &project, lib, "NOPE"],
        vec!["library", "search", lib, "x", "--dry-run"],
        vec!["library", "frobnicate", lib],
        vec!["library", "new", &project],
    ] {
        let o = ecad(&bad);
        assert_eq!(o.status.code(), Some(2), "{bad:?}: {}", text(&o));
    }
}
