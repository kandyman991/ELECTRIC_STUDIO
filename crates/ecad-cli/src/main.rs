//! `ecad`: headless commands. M0 has one: `ecad demo <out-dir>`, which writes the sample
//! project's page as DXF and PNG plus its wire list.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: ecad demo <out-dir>";

fn demo(out: PathBuf) -> Result<(), String> {
    let project = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
    let page = ecad_model::sample::PAGE_1;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, bytes: &[u8]| {
        let path = out.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
        Ok::<(), String>(())
    };
    write(
        "page-1.dxf",
        ecad_view::page_dxf(&project, page)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )?;
    write(
        "page-1.png",
        &ecad_view::page_png(&project, page, 1200, 1200).map_err(|e| e.to_string())?,
    )?;
    let rows = ecad_reports::wire_list(&project).map_err(|e| e.to_string())?;
    write(
        "wire-list.csv",
        ecad_reports::wire_list_csv(&rows).as_bytes(),
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [cmd, out] if cmd == "demo" => demo(PathBuf::from(out)),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ecad: {e}");
            ExitCode::FAILURE
        }
    }
}
