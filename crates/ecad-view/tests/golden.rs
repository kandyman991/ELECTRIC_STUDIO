//! Golden-file tests: the drawing output for known projects must not change unnoticed, in
//! particular when the pinned CADCraft commit is upgraded.
//!
//! DXF is compared byte for byte. PNG is compared pixel by pixel with a small tolerance, since
//! anti-aliasing may differ slightly between platforms.
//!
//! To accept intended changes: `ECAD_UPDATE_GOLDEN=1 cargo test -p ecad-view --test golden`,
//! then review the changed files in the diff.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::sample::{K1_COIL, PAGE_1, start_circuit};
use ecad_model::{Command, Point, Project, Rotation};
use std::path::PathBuf;

const PNG_SIZE: u32 = 600;
/// Share of pixels allowed to differ by more than `CHANNEL_TOLERANCE` in any channel.
const MAX_DIFFERING: f64 = 0.005;
const CHANNEL_TOLERANCE: u8 = 48;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name)
}

fn updating() -> bool {
    std::env::var_os("ECAD_UPDATE_GOLDEN").is_some()
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn check(project: &Project, stem: &str) {
    check_page(project, PAGE_1, stem);
}

fn check_page(project: &Project, page: ecad_model::PageId, stem: &str) {
    let dxf = ecad_view::page_dxf(project, page).unwrap();
    let png = ecad_view::page_png(project, page, PNG_SIZE, PNG_SIZE).unwrap();
    let (dxf_path, png_path) = (golden(&format!("{stem}.dxf")), golden(&format!("{stem}.png")));
    if updating() {
        std::fs::write(&dxf_path, &dxf).unwrap();
        std::fs::write(&png_path, &png).unwrap();
        return;
    }

    let want = std::fs::read_to_string(&dxf_path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with ECAD_UPDATE_GOLDEN=1)", dxf_path.display()));
    if dxf != want {
        let line = dxf.lines().zip(want.lines()).position(|(a, b)| a != b).map_or(0, |i| i + 1);
        panic!("{stem}.dxf differs from the golden file, first at line {line}");
    }

    let (w1, h1, got) = decode(&png);
    let (w2, h2, exp) = decode(&std::fs::read(&png_path).unwrap());
    assert_eq!((w1, h1), (w2, h2), "{stem}.png size changed");
    let differing = got
        .chunks(4)
        .zip(exp.chunks(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE))
        .count();
    let share = differing as f64 / f64::from(w1 * h1);
    assert!(share <= MAX_DIFFERING, "{stem}.png: {:.2}% of pixels differ", share * 100.0);
}

#[test]
fn start_circuit_page() {
    check(&start_circuit().unwrap(), "start_circuit");
}

#[test]
fn start_circuit_with_moved_and_rotated_coil() {
    let mut p = start_circuit().unwrap();
    p.move_function(K1_COIL, Point::new(40.0, 16.0)).unwrap();
    p.apply(Command::Rotate { id: K1_COIL, to: Rotation::R90 }).unwrap();
    check(&p, "start_circuit_moved");
}

#[test]
fn cross_references_on_both_pages() {
    let p = ecad_model::sample::with_contacts().unwrap();
    check(&p, "with_contacts_page1");
    check_page(&p, ecad_model::sample::PAGE_2, "with_contacts_page2");
}

#[test]
fn potentials_across_both_pages() {
    let p = ecad_model::sample::with_potentials().unwrap();
    check(&p, "with_potentials_page1");
    check_page(&p, ecad_model::sample::PAGE_2, "with_potentials_page2");
}

#[test]
fn terminal_strip() {
    check(&ecad_model::sample::with_terminals().unwrap(), "with_terminals_page1");
}

#[test]
fn plc_channels() {
    let p = ecad_model::sample::with_plc().unwrap();
    let page3 = p.pages_in_order()[2].id;
    check_page(&p, page3, "with_plc_page3");
}

#[test]
fn dxf_output_is_deterministic() {
    let p = start_circuit().unwrap();
    assert_eq!(ecad_view::page_dxf(&p, PAGE_1).unwrap(), ecad_view::page_dxf(&p, PAGE_1).unwrap());
}
