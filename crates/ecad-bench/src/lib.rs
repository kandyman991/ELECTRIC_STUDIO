//! Benchmark harness: representative projects, the workloads run on them, and what a run
//! measures.
//!
//! Every run reports **deterministic counts** (what the workload produced: devices, nets,
//! findings, the content hash...) that must be identical on every machine, and **measurements**
//! (wall time per iteration, peak resident memory) that are only comparable on one machine
//! under recorded conditions. CI checks the counts on every PR and runs the smoke size as a
//! performance smoke test; baselines for time and memory come from the controlled run
//! (`bench/run-controlled.sh`). See `bench/README.md`.

use ecad_model::underlay::{Underlay, UnderlayShapes, Units, UnitsFrom};
use ecad_model::{
    ADDRESS, BoxId, CableCore, CableId, Command, ConnectionId, DeviceId, FunctionId, PageId, PanelItemId,
    PanelItemKind, Point, Project, SYMBOL,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// Errors are reported as text: the harness only prints them.
pub type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The representative projects.
pub const PROJECTS: [&str; 3] = ["conveyor", "reference", "synthetic"];

/// The workloads, each run in a process of its own.
pub const WORKLOADS: [&str; 8] = ["open", "save", "edit", "check", "netlist", "hash", "render", "pdf"];

/// How big a run is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    /// Small enough for every PR.
    Smoke,
    /// The controlled run: the large synthetic project, more iterations.
    Full,
}

impl Size {
    pub fn parse(s: &str) -> Result<Size> {
        match s {
            "smoke" => Ok(Size::Smoke),
            "full" => Ok(Size::Full),
            _ => Err(format!("unknown size `{s}` (smoke or full)")),
        }
    }

    /// Copies of the conveyor in the synthetic project: about 50 devices each.
    pub fn copies(self) -> usize {
        match self {
            Size::Smoke => 4,
            Size::Full => 40,
        }
    }

    /// Points in the synthetic project's generated underlay.
    pub fn underlay_points(self) -> usize {
        match self {
            Size::Smoke => 20_000,
            Size::Full => 400_000,
        }
    }

    pub fn iterations(self) -> usize {
        match self {
            Size::Smoke => 3,
            Size::Full => 10,
        }
    }
}

/// Build a representative project. `underlay` replaces the synthetic project's generated
/// underlay with a real drawing (a local DXF or DWG; never committed).
pub fn project(name: &str, size: Size, underlay: Option<&Path>) -> Result<Project> {
    match name {
        "conveyor" => ecad_model::conveyor::conveyor().map_err(err),
        "reference" => ecad_model::reference::reference_project().map_err(err),
        "synthetic" => {
            let mut p = synthetic(size.copies())?;
            let shapes = match underlay {
                Some(path) => {
                    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ecad_view::underlay::import(&bytes, &name).map_err(err)?.shapes
                }
                None => generated_underlay(size.underlay_points()),
            };
            put_underlay(&mut p, shapes)?;
            Ok(p)
        }
        _ => Err(format!("unknown project `{name}` ({})", PROJECTS.join(", "))),
    }
}

/// The conveyor `copies` times over, as one project: a line of identical machines. Copy `k`
/// (from 1) gets new ids, tags prefixed `=M<k+1>`, its pages numbered `<k+1>.<page>`, its wire
/// numbers prefixed, its PLC addresses moved up by two bytes per copy and its PLC symbols
/// suffixed. Built through commands, so it is as valid as the conveyor.
pub fn synthetic(copies: usize) -> Result<Project> {
    let base = ecad_model::conveyor::conveyor().map_err(err)?;
    let mut p = base.clone();
    for k in 1..copies {
        for cmd in copy_commands(&base, k)? {
            p.apply(cmd).map_err(err)?;
        }
    }
    Ok(p)
}

const NAMESPACE: Uuid = Uuid::from_u128(0x6563_6164_2d62_656e_6368_2d63_6f70_7900);

fn remap(k: usize, id: Uuid) -> Uuid {
    Uuid::new_v5(&NAMESPACE, format!("{k}/{id}").as_bytes())
}

/// `I0.3` moved up by `bytes` bytes: `I8.3`.
fn shift_address(address: &str, bytes: u32) -> String {
    let (area, rest) = address.split_at(address.find(|c: char| c.is_ascii_digit()).unwrap_or(address.len()));
    match rest.split_once('.').and_then(|(b, bit)| b.parse::<u32>().ok().map(|b| (b, bit))) {
        Some((byte, bit)) => format!("{area}{}.{bit}", byte + bytes),
        None => address.to_owned(),
    }
}

fn copy_commands(base: &Project, k: usize) -> Result<Vec<Command>> {
    let machine = format!("=M{}", k + 1);
    let page_id = |id: PageId| PageId(remap(k, id.0));
    let device_id = |id: DeviceId| DeviceId(remap(k, id.0));
    let cable_id = |id: CableId| CableId(remap(k, id.0));
    let pages_per_copy = u32::try_from(base.pages.len()).map_err(err)?;
    let k32 = u32::try_from(k).map_err(err)?;
    let mut out = Vec::new();
    for page in base.pages.values() {
        let mut page = page.clone();
        page.id = page_id(page.id);
        page.name = format!("{}.{}", k + 1, page.name);
        page.order += k32 * pages_per_copy;
        out.push(Command::AddPage(page));
    }
    for d in base.devices.values() {
        let mut d = d.clone();
        d.id = device_id(d.id);
        d.tag = format!("{machine}{}", d.tag);
        out.push(Command::AddDevice(d));
    }
    for f in base.functions.values() {
        let mut f = f.clone();
        f.id = FunctionId(remap(k, f.id.0));
        f.device = device_id(f.device);
        f.page = page_id(f.page);
        if let Some(a) = f.properties.get_mut(ADDRESS) {
            *a = shift_address(a, 2 * k32);
        }
        if let Some(s) = f.properties.get_mut(SYMBOL).filter(|s| !s.is_empty()) {
            *s = format!("{s}_M{}", k + 1);
        }
        out.push(Command::Place(f));
    }
    for c in base.cables.values() {
        let mut c = c.clone();
        c.id = cable_id(c.id);
        c.tag = format!("{machine}{}", c.tag);
        out.push(Command::AddCable(c));
    }
    for c in base.connections.values() {
        let mut c = c.clone();
        c.id = ConnectionId(remap(k, c.id.0));
        c.from.function = FunctionId(remap(k, c.from.function.0));
        c.to.function = FunctionId(remap(k, c.to.function.0));
        c.wire_number = c.wire_number.map(|n| format!("M{}.{n}", k + 1));
        c.spec.cable = c.spec.cable.map(|cc| CableCore { cable: cable_id(cc.cable), core: cc.core });
        out.push(Command::Connect(c));
    }
    for b in base.boxes.values() {
        let mut b = b.clone();
        b.id = BoxId(remap(k, b.id.0));
        b.page = page_id(b.page);
        out.push(Command::AddBox(b));
    }
    for (page, panel) in &base.panels {
        out.push(Command::SetPanel { page: page_id(*page), panel: Some(*panel) });
    }
    for item in base.panel_items.values() {
        let mut item = item.clone();
        item.id = PanelItemId(remap(k, item.id.0));
        item.page = page_id(item.page);
        if let PanelItemKind::Device { device } = &mut item.kind {
            *device = device_id(*device);
        }
        out.push(Command::AddPanelItem(item));
    }
    Ok(out)
}

/// A deterministic drawing of about `points` points: a building plan of rooms, walls and
/// cable routes, the kind of DWG a retrofit starts from.
pub fn generated_underlay(points: usize) -> UnderlayShapes {
    let (width, height) = (400.0, 270.0);
    let mut seed: u64 = 0x5eed;
    let mut next = move || {
        // xorshift64*: deterministic on every platform.
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        (seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut lines = Vec::new();
    let mut total = 0;
    while total < points {
        // A closed room outline, then a route of a few segments.
        let (x, y) = (next() * (width - 20.0), next() * (height - 20.0));
        let (w, h) = (2.0 + next() * 18.0, 2.0 + next() * 18.0);
        lines.push(vec![
            Point::new(x, y),
            Point::new(x + w, y),
            Point::new(x + w, y + h),
            Point::new(x, y + h),
            Point::new(x, y),
        ]);
        let mut route = vec![Point::new(x + w / 2.0, y + h / 2.0)];
        for _ in 0..5 {
            let last = route[route.len() - 1];
            let step = (next() - 0.5) * 30.0;
            let p = if route.len() % 2 == 0 {
                Point::new(last.x, (last.y + step).clamp(0.0, height))
            } else {
                Point::new((last.x + step).clamp(0.0, width), last.y)
            };
            route.push(p);
        }
        total += 5 + route.len();
        lines.push(route);
    }
    UnderlayShapes { lines, fills: Vec::new(), width, height }
}

/// Put `shapes` behind the first page, placed as an import places it.
fn put_underlay(p: &mut Project, shapes: UnderlayShapes) -> Result<()> {
    let page = p.pages_in_order().first().map(|pg| pg.id).ok_or("no pages")?;
    let (lo, hi) = p.sheet.frame();
    let (at, scale, _) = Underlay::placement(&shapes, Point::new(0.0, 0.0), Units::Millimetres, lo, hi);
    let underlay = Underlay {
        source: "generated".into(),
        shapes: Arc::new(shapes),
        at,
        scale,
        visible: true,
        units: Units::Millimetres,
        units_from: UnitsFrom::Assumed,
    };
    p.apply(Command::SetUnderlay { page, underlay: Some(Box::new(underlay)) }).map(drop).map_err(err)
}

/// The deterministic description of a project.
pub fn project_counts(p: &Project) -> Result<BTreeMap<String, serde_json::Value>> {
    let c = p.connectivity();
    let n = p.netlist();
    let underlay_points: usize = p.underlays.values().map(|u| u.shapes.points()).sum();
    Ok([
        ("pages", p.pages.len().into()),
        ("devices", p.devices.len().into()),
        ("drawings", p.functions.len().into()),
        ("wires", p.connections.len().into()),
        ("terminals", c.terminals.len().into()),
        ("nodes", c.nodes.len().into()),
        ("nets", n.nets.len().into()),
        ("underlayPoints", underlay_points.into()),
        ("contentHash", p.content_hash().map_err(err)?.into()),
    ]
    .into_iter()
    .map(|(k, v): (&str, serde_json::Value)| (k.to_owned(), v))
    .collect())
}

/// One workload's result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub project: String,
    pub workload: String,
    pub size: Size,
    /// The project's deterministic description, then what the workload produced.
    pub counts: BTreeMap<String, serde_json::Value>,
    pub iterations: usize,
    /// Wall time of each iteration, ns. The first is process-first, after setup; later iterations are warm.
    pub times_ns: Vec<u64>,
    /// Peak resident memory while building the project, KiB (Linux; `null` elsewhere).
    pub setup_peak_kib: Option<u64>,
    /// Peak resident memory during the iterations, KiB, after resetting the peak (Linux).
    pub peak_kib: Option<u64>,
}

/// The peak resident set size of this process, KiB (`VmHWM`), on Linux.
pub fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Reset the peak resident set size to the current one, on Linux; `false` where not possible.
fn reset_peak_rss() -> bool {
    std::fs::write("/proc/self/clear_refs", "5").is_ok()
}

/// Build the project and run one workload `iterations` times (0: build only, to measure the
/// setup alone).
pub fn run(project_name: &str, workload: &str, size: Size, iterations: usize, underlay: Option<&Path>) -> Result<Run> {
    if !WORKLOADS.contains(&workload) {
        return Err(format!("unknown workload `{workload}` ({})", WORKLOADS.join(", ")));
    }
    let mut p = project(project_name, size, underlay)?;
    let mut counts = project_counts(&p)?;
    let tmp = tempfile::tempdir().map_err(err)?;
    let dir = tmp.path().join("project");
    if workload == "open" {
        ecad_project::save(&p, &dir).map_err(err)?;
    }
    let pages: Vec<PageId> = p.pages_in_order().iter().map(|pg| pg.id).collect();
    // The edit: move one drawing back and forth, then run the checks, as the editor does.
    let moved = p.functions.values().next().map(|f| (f.id, f.position)).ok_or("no drawings")?;
    let setup_peak_kib = peak_rss_kib();
    let reset = reset_peak_rss();

    let mut times_ns = Vec::with_capacity(iterations);
    let mut produced: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for i in 0..iterations {
        let start = Instant::now();
        let out: Vec<(&str, serde_json::Value)> = match workload {
            "open" => {
                let q = ecad_project::load(&dir).map_err(err)?;
                vec![("loadedDevices", q.devices.len().into())]
            }
            "save" => {
                ecad_project::save(&p, &dir).map_err(err)?;
                vec![("saved", true.into())]
            }
            "edit" => {
                let (id, at) = moved;
                let dx = if i % 2 == 0 { 4.0 } else { 0.0 };
                p.apply(Command::Move { id, to: Point::new(at.x + dx, at.y) }).map_err(err)?;
                let findings = ecad_rules::check(&p);
                vec![("findings", findings.len().into())]
            }
            "check" => {
                let findings = ecad_rules::check(&p);
                let errors = findings.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
                vec![("findings", findings.len().into()), ("errors", errors.into())]
            }
            "netlist" => {
                let n = p.netlist();
                vec![("nets", n.nets.len().into()), ("unresolved", n.unresolved.len().into())]
            }
            "hash" => vec![("hash", p.content_hash().map_err(err)?.into())],
            "render" => {
                let mut primitives = 0;
                for page in &pages {
                    primitives += ecad_view::page_primitives(&p, *page, [0, 0, 0]).map_err(err)?.len();
                }
                vec![("primitives", primitives.into())]
            }
            "pdf" => {
                let bytes = ecad_view::project_pdf(&p).map_err(err)?;
                vec![("pdfNonEmpty", (!bytes.is_empty()).into())]
            }
            _ => Vec::new(),
        };
        times_ns.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        produced = out.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    }
    counts.extend(produced);
    Ok(Run {
        project: project_name.to_owned(),
        workload: workload.to_owned(),
        size,
        counts,
        iterations,
        times_ns,
        setup_peak_kib,
        peak_kib: if reset { peak_rss_kib() } else { None },
    })
}

/// Prepare an on-disk fixture before any filesystem cache eviction.
pub fn prepare_open(project_name: &str, size: Size, underlay: Option<&Path>, dir: &Path) -> Result<()> {
    let p = project(project_name, size, underlay)?;
    ecad_project::save(&p, dir).map_err(err)?;
    Ok(())
}

/// Measure an existing fixture without reading or writing it before the first timed load.
/// Cache eviction, when requested, must be performed by the controlled-run caller.
pub fn measure_open(project_name: &str, size: Size, dir: &Path) -> Result<Run> {
    if !PROJECTS.contains(&project_name) {
        return Err(format!("unknown project {project_name}"));
    }
    let iterations = size.iterations();
    let setup_peak_kib = peak_rss_kib();
    let reset = reset_peak_rss();
    let mut times_ns = Vec::with_capacity(iterations);
    let mut last = None;
    for _ in 0..iterations {
        drop(last.take());
        let start = Instant::now();
        let p = ecad_project::load(dir).map_err(err)?;
        times_ns.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        last = Some(p);
    }
    let peak_kib = if reset { peak_rss_kib() } else { None };
    let p = last.ok_or("no loads measured")?;
    let mut counts = project_counts(&p)?;
    counts.insert("loadedDevices".into(), p.devices.len().into());
    Ok(Run {
        project: project_name.to_owned(),
        workload: "open".into(),
        size,
        counts,
        iterations,
        times_ns,
        setup_peak_kib,
        peak_kib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_move_up_by_whole_bytes() {
        assert_eq!(shift_address("I0.3", 4), "I4.3");
        assert_eq!(shift_address("Q12.7", 2), "Q14.7");
        assert_eq!(shift_address("%I1.0", 2), "%I3.0");
        assert_eq!(shift_address("nonsense", 2), "nonsense");
    }

    #[test]
    fn the_synthetic_line_is_valid_and_deterministic() {
        let p = synthetic(3).unwrap();
        let base = ecad_model::conveyor::conveyor().unwrap();
        assert_eq!(p.devices.len(), 3 * base.devices.len());
        assert_eq!(p.connections.len(), 3 * base.connections.len());
        let errors: Vec<_> =
            ecad_rules::check(&p).into_iter().filter(|c| c.severity == ecad_rules::Severity::Error).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(p.connectivity().ambiguities.is_empty());
        assert_eq!(p.netlist().nets.len(), 3 * base.netlist().nets.len(), "copies are separate machines");
        assert_eq!(synthetic(3).unwrap().content_hash().unwrap(), p.content_hash().unwrap());
    }

    #[test]
    fn the_generated_underlay_is_valid_and_deterministic() {
        let a = generated_underlay(5_000);
        assert!(a.points() >= 5_000);
        assert!(a.is_valid());
        assert_eq!(a, generated_underlay(5_000));
    }

    #[test]
    fn prepared_open_matches_regular_open_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("project");
        assert!(measure_open("conveyor", Size::Smoke, &dir).is_err());
        prepare_open("conveyor", Size::Smoke, None, &dir).unwrap();
        let measured = measure_open("conveyor", Size::Smoke, &dir).unwrap();
        let regular = run("conveyor", "open", Size::Smoke, 1, None).unwrap();
        assert_eq!(measured.counts, regular.counts);
        assert_eq!(measured.times_ns.len(), Size::Smoke.iterations());
    }

    #[test]
    fn every_workload_runs_on_the_conveyor() {
        for w in WORKLOADS {
            let r = run("conveyor", w, Size::Smoke, 1, None).unwrap();
            assert_eq!(r.times_ns.len(), 1, "{w}");
            assert_eq!(r.counts["wires"], ecad_model::conveyor::conveyor().unwrap().connections.len());
        }
    }
}
