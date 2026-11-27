//! `ecad-bench`: run the benchmark workloads, compare deterministic counts with a baseline, and
//! count instructions under Valgrind. See `bench/README.md`.

use ecad_bench::{PROJECTS, Size, WORKLOADS};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
ecad-bench: Electric Studio's benchmark harness

usage:
  ecad-bench run <project> <workload> [--size smoke|full] [--iterations N] [--underlay <file>]
      Build the project and run one workload in this process; print the result as JSON.
      --iterations 0 builds the project only (to subtract the setup).
  ecad-bench suite [--size smoke|full] [--projects a,b] [--workloads a,b] [--underlay <file>]
                   [--out <file>]
      Run every workload on every project, each in a process of its own, and write the
      results with the machine's conditions as JSON (to stdout without --out).
  ecad-bench compare <results.json> <baseline.json>
      Fail if any deterministic count differs from the baseline; report times and peak memory
      against it when both runs come from the same machine.
  ecad-bench instructions <baseline.json> [--update] [--project <name>] [--tolerance <percent>]
      Count each workload's instructions under Valgrind (cachegrind), setup subtracted, and
      warn about any that moved more than the tolerance (default 10) from the baseline.

projects:  conveyor, reference, synthetic (the conveyor replicated; an underlay on page 1)
workloads: open, save, edit, check, netlist, hash, render, pdf
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ecad-bench: {e}");
            ExitCode::from(2)
        }
    }
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn list<'a>(args: &'a [String], name: &str, all: &[&'a str]) -> Vec<&'a str> {
    match option(args, name) {
        Some(v) => v.split(',').collect(),
        None => all.to_vec(),
    }
}

fn size(args: &[String]) -> Result<Size, String> {
    Size::parse(option(args, "--size").unwrap_or("smoke"))
}

fn dispatch(args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("run") => {
            let (Some(project), Some(workload)) = (args.get(1), args.get(2)) else {
                return Err(format!("run needs a project and a workload\n\n{USAGE}"));
            };
            let size = size(args)?;
            let iterations = match option(args, "--iterations") {
                Some(n) => n.parse().map_err(|_| format!("bad --iterations `{n}`"))?,
                None => size.iterations(),
            };
            let underlay = option(args, "--underlay").map(Path::new);
            let r = ecad_bench::run(project, workload, size, iterations, underlay)?;
            println!("{}", serde_json::to_string(&r).map_err(|e| e.to_string())?);
            Ok(ExitCode::SUCCESS)
        }
        Some("suite") => suite(args),
        Some("compare") => {
            let (Some(results), Some(baseline)) = (args.get(1), args.get(2)) else {
                return Err(format!("compare needs two files\n\n{USAGE}"));
            };
            compare(&read_json(Path::new(results))?, &read_json(Path::new(baseline))?)
        }
        Some("instructions") => instructions(args),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

fn first_line(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_owned())
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

/// The conditions a run was made under, as far as this machine tells.
fn machine() -> Value {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
        s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_owned())
    });
    let on_ac = std::fs::read_dir("/sys/class/power_supply").ok().map(|dir| {
        dir.filter_map(|e| e.ok())
            .filter(|e| read_trim(&format!("{}/type", e.path().display())).as_deref() == Some("Mains"))
            .any(|e| read_trim(&format!("{}/online", e.path().display())).as_deref() == Some("1"))
    });
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": cpu,
        "cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "kernel": read_trim("/proc/sys/kernel/osrelease"),
        "governor": read_trim("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "onAc": on_ac,
        "loadavg": read_trim("/proc/loadavg"),
        "rustc": first_line("rustc", &["--version"]),
        "commit": std::env::var("GITHUB_SHA").ok().or_else(|| first_line("git", &["rev-parse", "HEAD"])),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "label": std::env::var("ECAD_BENCH_MACHINE").ok(),
    })
}

fn suite(args: &[String]) -> Result<ExitCode, String> {
    let size = size(args)?;
    let projects = list(args, "--projects", &PROJECTS);
    let workloads = list(args, "--workloads", &WORKLOADS);
    let underlay = option(args, "--underlay");
    let mut results = Vec::new();
    for project in &projects {
        for workload in &workloads {
            let mut cmd = Command::new(exe()?);
            cmd.args(["run", project, workload, "--size", option(args, "--size").unwrap_or("smoke")]);
            if let Some(u) = underlay {
                cmd.args(["--underlay", u]);
            }
            let out = cmd.output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("{project} {workload}: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
            let r: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("{project} {workload}: {e}"))?;
            let times: Vec<u64> = r["timesNs"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            eprintln!(
                "{project:>10} {workload:<8} first {:>9.2} ms  median {:>9.2} ms  peak {:>8} KiB",
                times.first().copied().unwrap_or(0) as f64 / 1e6,
                median(&times) as f64 / 1e6,
                r["peakKib"].as_u64().map_or("-".into(), |k| k.to_string()),
            );
            results.push(r);
        }
    }
    let doc = json!({ "size": size, "machine": machine(), "results": results });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    match option(args, "--out") {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn median(times: &[u64]) -> u64 {
    let mut t = times.to_vec();
    t.sort_unstable();
    t.get(t.len() / 2).copied().unwrap_or(0)
}

fn key(r: &Value) -> String {
    format!("{} {}", r["project"].as_str().unwrap_or("?"), r["workload"].as_str().unwrap_or("?"))
}

/// Validate the complete result set before comparing any measurements. A partial suite must
/// never satisfy a full baseline, and malformed or duplicate rows must never disappear in a map.
fn validated_rows(doc: &Value) -> Result<std::collections::BTreeMap<String, &Value>, String> {
    let size = Size::parse(doc["size"].as_str().ok_or("missing suite size")?)?;
    let rows = doc["results"].as_array().filter(|r| !r.is_empty()).ok_or("missing or empty results")?;
    if !doc["machine"].is_object() {
        return Err("missing machine metadata".into());
    }
    let mut indexed = std::collections::BTreeMap::new();
    for r in rows {
        let project = r["project"].as_str().ok_or("missing project")?;
        let workload = r["workload"].as_str().ok_or("missing workload")?;
        if !PROJECTS.contains(&project) || !WORKLOADS.contains(&workload) {
            return Err(format!("unknown benchmark {project} {workload}"));
        }
        let k = key(r);
        if r["size"] != doc["size"] || r["iterations"].as_u64() != Some(size.iterations() as u64) {
            return Err(format!("{k}: wrong size or iteration count"));
        }
        let times = r["timesNs"].as_array().ok_or_else(|| format!("{k}: missing timings"))?;
        if times.len() != size.iterations() || times.iter().any(|t| t.as_u64().is_none()) {
            return Err(format!("{k}: incomplete or invalid timings"));
        }
        for field in ["setupPeakKib", "peakKib"] {
            if r.get(field).is_none_or(|v| !v.is_null() && v.as_u64().is_none()) {
                return Err(format!("{k}: invalid {field}"));
            }
        }
        let counts = r["counts"].as_object().ok_or_else(|| format!("{k}: missing counts"))?;
        let mut numeric = vec!["pages", "devices", "drawings", "wires", "terminals", "nodes", "nets", "underlayPoints"];
        match workload {
            "open" => numeric.push("loadedDevices"),
            "edit" => numeric.push("findings"),
            "check" => numeric.extend(["findings", "errors"]),
            "netlist" => numeric.push("unresolved"),
            "render" => numeric.push("primitives"),
            _ => {}
        }
        for field in numeric.iter().copied() {
            if counts.get(field).and_then(Value::as_u64).is_none() {
                return Err(format!("{k}: missing or invalid count {field}"));
            }
        }
        let hash_ok = |field: &str| {
            counts.get(field).and_then(Value::as_str).is_some_and(|s| {
                s.strip_prefix("sha256:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            })
        };
        let valid_extra = match workload {
            "save" => counts.get("saved").and_then(Value::as_bool) == Some(true),
            "pdf" => counts.get("pdfNonEmpty").and_then(Value::as_bool) == Some(true),
            "hash" => hash_ok("hash"),
            _ => true,
        };
        let extra = usize::from(matches!(workload, "save" | "pdf" | "hash"));
        if !hash_ok("contentHash") || !valid_extra || counts.len() != numeric.len() + 1 + extra {
            return Err(format!("{k}: invalid count schema"));
        }
        if indexed.insert(k.clone(), r).is_some() {
            return Err(format!("duplicate benchmark {k}"));
        }
    }
    Ok(indexed)
}

fn compare(results: &Value, baseline: &Value) -> Result<ExitCode, String> {
    let base = validated_rows(baseline)?;
    let actual = validated_rows(results)?;
    if baseline["size"] != results["size"] || base.keys().ne(actual.keys()) {
        return Err("benchmark suite size or workload set differs from baseline".into());
    }
    let same_machine = baseline["machine"]["label"].is_string()
        && ["label", "profile", "os", "arch", "cpu", "cores", "kernel", "governor", "onAc", "rustc"]
            .iter()
            .all(|field| baseline["machine"][*field] == results["machine"][*field]);
    let mut mismatches = 0;
    for (k, r) in actual {
        let b = base.get(&k).ok_or_else(|| format!("{k}: missing baseline"))?;
        if r["counts"] != b["counts"] {
            mismatches += 1;
            println!("{k}: counts differ\n  got  {}\n  want {}", r["counts"], b["counts"]);
        }
        if same_machine && r["counts"] == b["counts"] {
            let t = |v: &Value| {
                median(&v["timesNs"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect::<Vec<_>>())
            };
            let (now, then) = (t(r), t(b));
            let mem = |v: &Value| v["peakKib"].as_u64();
            println!(
                "{k}: median {:.2} ms ({:+.1}%), peak {} KiB (baseline {})",
                now as f64 / 1e6,
                if then > 0 { (now as f64 / then as f64 - 1.0) * 100.0 } else { 0.0 },
                mem(r).map_or("-".into(), |m| m.to_string()),
                mem(b).map_or("-".into(), |m| m.to_string()),
            );
        }
    }
    if !same_machine {
        println!("(times and memory not compared: machine conditions or toolchain differ)");
    }
    if mismatches > 0 {
        println!("{mismatches} workload(s) with different counts");
        return Ok(ExitCode::FAILURE);
    }
    println!("counts match the baseline");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod compare_tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn comparison_rejects_missing_extra_duplicate_and_malformed_results() {
        let baseline: Value = serde_json::from_str(include_str!("../../../bench/baseline-smoke.json")).unwrap();
        assert_eq!(compare(&baseline, &baseline).unwrap(), ExitCode::SUCCESS);
        let mut empty = baseline.clone();
        empty["results"] = json!([]);
        assert!(compare(&empty, &baseline).is_err());
        let mut missing = baseline.clone();
        missing["results"].as_array_mut().unwrap().pop();
        assert!(compare(&missing, &baseline).is_err());
        let mut duplicate = baseline.clone();
        let row = duplicate["results"][0].clone();
        duplicate["results"].as_array_mut().unwrap().push(row);
        assert!(compare(&duplicate, &baseline).is_err());
        let mut unknown = baseline.clone();
        unknown["results"][0]["workload"] = json!("unknown");
        assert!(compare(&unknown, &baseline).is_err());
        assert!(compare(&baseline, &missing).is_err());
        let mut malformed = baseline.clone();
        malformed["results"][0]["timesNs"] = json!([1]);
        assert!(compare(&malformed, &baseline).is_err());
        malformed = baseline.clone();
        malformed["results"][0]["counts"] = json!({});
        assert!(compare(&malformed, &baseline).is_err());
        let mut changed = baseline.clone();
        changed["results"][0]["counts"]["devices"] = json!(999);
        assert_eq!(compare(&changed, &baseline).unwrap(), ExitCode::FAILURE);
    }
}

/// Instructions executed by `run <project> <workload> --iterations <n>` under cachegrind.
fn instructions_of(project: &str, workload: &str, iterations: usize) -> Result<u64, String> {
    let exe = exe()?;
    let out = Command::new("valgrind")
        .args(["--tool=cachegrind", "--cache-sim=no", "--cachegrind-out-file=/dev/null"])
        .arg(&exe)
        .args(["run", project, workload, "--size", "smoke", "--iterations", &iterations.to_string()])
        .output()
        .map_err(|e| format!("valgrind: {e}"))?;
    if !out.status.success() {
        return Err(format!("{project} {workload}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr.lines().find(|l| l.contains("I refs:")).ok_or("no `I refs` in valgrind's output")?;
    let digits: String = line.split("I refs:").nth(1).unwrap_or("").chars().filter(char::is_ascii_digit).collect();
    digits.parse().map_err(|_| format!("can't read `{line}`"))
}

fn instructions(args: &[String]) -> Result<ExitCode, String> {
    let Some(path) = args.get(1).filter(|a| !a.starts_with("--")) else {
        return Err(format!("instructions needs a baseline file\n\n{USAGE}"));
    };
    let project = option(args, "--project").unwrap_or("conveyor");
    let tolerance: f64 = option(args, "--tolerance").map_or(Ok(10.0), str::parse).map_err(|_| "bad --tolerance")?;
    let update = args.iter().any(|a| a == "--update");
    let github = std::env::var_os("GITHUB_ACTIONS").is_some();
    let baseline = if update { Value::Null } else { read_json(Path::new(path))? };
    let mut counted = serde_json::Map::new();
    let mut moved = 0;
    for workload in WORKLOADS {
        // Each workload's own setup (the open workload saves the project first) subtracted.
        let setup = instructions_of(project, workload, 0)?;
        let own = instructions_of(project, workload, 1)?.saturating_sub(setup);
        counted.insert(workload.to_owned(), own.into());
        match baseline["workloads"][workload].as_u64() {
            Some(then) if then > 0 => {
                let change = (own as f64 / then as f64 - 1.0) * 100.0;
                println!("{project} {workload:<8} {own:>14} instructions ({change:+.1}%)");
                if change.abs() > tolerance {
                    moved += 1;
                    if github {
                        println!("::warning::{project} {workload}: {change:+.1}% instructions against the baseline");
                    }
                }
            }
            _ => println!("{project} {workload:<8} {own:>14} instructions"),
        }
    }
    if update {
        let doc = json!({
            "project": project,
            "workloads": counted,
            "rustc": first_line("rustc", &["--version"]),
            "note": "Instructions per workload under cachegrind, setup subtracted, release build. Informative: a new toolchain moves them too.",
        });
        std::fs::write(path, serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n")
            .map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    } else if moved > 0 {
        println!("{moved} workload(s) moved more than {tolerance}% (a warning, not a failure)");
    }
    Ok(ExitCode::SUCCESS)
}
