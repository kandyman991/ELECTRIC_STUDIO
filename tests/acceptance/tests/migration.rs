//! M10: format 1 converts to format 2 deterministically, and its ambiguous mappings are kept
//! apart and reported, never merged or reconnected (section 9, D33).
//!
//! `ambiguous/format1` is M7's `potentials` sample written by `ecad new-sample` at the M7
//! acceptance commit, then given three ambiguities (see `ambiguous/README.md`):
//! - I9, an interruption whose page 1 drawing is wired to L+ and page 2 drawing to M;
//! - M also drawn as an interruption point;
//! - junction J1 drawn a second time.
//!
//! Format 1 joined L+ and M through I9 ("Potentials L+ and M are connected").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_model::{ConnectionRef, DeviceId, Endpoint, FunctionId, NodeId, NodeKind, Project};
use ecad_project::{FindingKind, load_with_findings};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ambiguous").join("format1")
}

fn fid(n: u128) -> FunctionId {
    FunctionId::from_u128(n)
}

fn node_of(p: &Project, f: u128) -> NodeId {
    p.function(fid(f)).unwrap().node.unwrap_or_else(|| panic!("{f:x} shows no node"))
}

#[test]
fn ambiguous_mappings_are_reported_each_with_its_pages() {
    let (_, findings) = load_with_findings(&fixture()).unwrap();
    let mut got: Vec<(FindingKind, String, Vec<String>)> =
        findings.iter().map(|f| (f.kind, f.name.clone(), f.pages.clone())).collect();
    got.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        got,
        [
            (FindingKind::InterruptionJoinsPotentials, "I9".to_owned(), vec!["1".to_owned(), "2".to_owned()]),
            (FindingKind::JunctionDrawnTwice, "J1".to_owned(), vec!["1".to_owned(), "2".to_owned()]),
            (FindingKind::PotentialAndInterruption, "M".to_owned(), vec!["1".to_owned(), "2".to_owned()]),
        ]
    );
}

#[test]
fn nothing_is_merged_or_reconnected() {
    let (p, _) = load_with_findings(&fixture()).unwrap();
    // I9's drawings are two interruptions, so L+ and M are no longer joined.
    let (a, b) = (node_of(&p, 0x2a2), node_of(&p, 0x2a4));
    assert_ne!(a, b);
    let i9 = NodeId::for_device(DeviceId::from_u128(0x1a9));
    assert_eq!((a, b), (i9.split(fid(0x2a2)), i9.split(fid(0x2a4))));
    assert!(!p.nodes.contains_key(&i9));
    let netlist = p.netlist();
    assert!(netlist.nets.iter().all(|n| n.names.len() <= 1), "no net carries two potential names");
    let names: Vec<String> = netlist.nets.iter().flat_map(|n| n.names.clone()).collect();
    assert!(names.contains(&"L+".to_owned()) && names.contains(&"M".to_owned()));
    // M is a potential and, apart from it, an interruption of that name.
    let m = DeviceId::from_u128(0x104);
    assert_eq!(p.nodes[&NodeId::for_device(m)].kind, NodeKind::Potential);
    assert_eq!(node_of(&p, 0x2a5), NodeId::interruption_of(m));
    assert_eq!(p.nodes[&NodeId::interruption_of(m)].kind, NodeKind::Interruption);
    assert_eq!(p.nodes[&NodeId::interruption_of(m)].name, "M");
    // J1's two drawings are two junctions.
    assert_ne!(node_of(&p, 0x20b), node_of(&p, 0x2a6));
    // No potential, interruption or junction is a device any more.
    for tag in ["L+", "M", "I1", "I9", "J1"] {
        assert!(p.device_by_tag(tag).is_none(), "{tag}");
    }
    // Every wire is still there, every end resolved, and the rest of the network as before:
    // I1 joins its two drawings, L+ is one node on both pages.
    assert!(netlist.is_complete());
    assert_eq!(node_of(&p, 0x208), node_of(&p, 0x209));
    assert_eq!(node_of(&p, 0x201), node_of(&p, 0x207));
    let c = p.connectivity();
    assert!(c.ambiguities.is_empty(), "the conversion leaves nothing ambiguous in the model: {:?}", c.ambiguities);
    assert!(matches!(c.endpoint(&ConnectionRef::new(fid(0x2a2), "1")), Some(Endpoint::Node(_))));
}

#[test]
fn the_conversion_is_deterministic() {
    let (a, fa) = load_with_findings(&fixture()).unwrap();
    let (b, fb) = load_with_findings(&fixture()).unwrap();
    assert_eq!(a, b);
    assert_eq!(fa, fb);
    assert_eq!(a.content_hash().unwrap(), b.content_hash().unwrap());
}

#[test]
fn saved_knowingly_it_reads_back_as_converted_with_nothing_to_report() {
    let (p, _) = load_with_findings(&fixture()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ecad_project::save(&p, tmp.path()).unwrap();
    let (q, findings) = load_with_findings(tmp.path()).unwrap();
    assert!(findings.is_empty());
    assert_eq!(q, p);
    assert_eq!(q.content_hash().unwrap(), p.content_hash().unwrap());
}

#[test]
fn the_session_asks_for_a_second_save_and_the_cli_for_accept_conversion() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    copy_dir(&fixture(), &dir);
    let mut s = ecad_editor::Session::open(&dir).unwrap();
    let st = s.frame(None, [0, 0, 0]).status;
    assert!(st.contains("kept apart"), "{st}");
    s.save();
    assert!(s.frame(None, [0, 0, 0]).status.starts_with("Not saved yet"));
    assert_eq!(ecad_project::format_of(&dir).unwrap(), 1, "nothing written");
    s.save();
    assert_eq!(ecad_project::format_of(&dir).unwrap(), 2, "saved knowingly");
    assert!(ecad_project::conversion_findings(&dir).unwrap().is_empty());
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}
