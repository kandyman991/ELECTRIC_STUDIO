//! The server as an MCP client sees it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_mcp::Server;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn project(sample: ecad_model::Project) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ecad_project::save(&sample, dir.path()).unwrap();
    dir
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// Call a tool: its structured result, or its text; and whether it failed.
fn call(server: &mut Server, name: &str, args: Value) -> (Value, bool) {
    let reply = server.handle(&request(9, "tools/call", json!({ "name": name, "arguments": args }))).unwrap();
    let result = &reply["result"];
    let failed = result["isError"].as_bool().unwrap();
    let value = result.get("structuredContent").cloned().unwrap_or_else(|| result["content"][0]["text"].clone());
    (value, failed)
}

#[test]
fn a_client_talks_to_the_binary_over_stdio() {
    let dir = project(ecad_model::sample::with_plc().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ecad-mcp"))
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut send = |message: Value| writeln!(stdin, "{message}").unwrap();
    let mut read = || {
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    send(request(
        1,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }),
    ));
    let init = read();
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "electric-studio");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // A notification gets no answer: the next line answers the next request.
    send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    send(request(2, "tools/list", json!({})));
    let list = read();
    assert_eq!(list["id"], 2);
    let names: Vec<&str> =
        list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"run_checks") && !names.contains(&"connect"), "read only by default: {names:?}");
    send(request(3, "tools/call", json!({ "name": "report", "arguments": { "name": "plc-io-list" } })));
    let report = read();
    let csv = report["result"]["content"][0]["text"].as_str().unwrap();
    assert!(csv.starts_with("card,channel,kind,address"), "{csv}");
    // A line that isn't JSON gets a parse error, and the server carries on.
    writeln!(stdin, "{{not json").unwrap();
    let bad = read();
    assert_eq!(bad["error"]["code"], -32700);
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn the_read_tools_describe_the_project() {
    let dir = project(ecad_model::reference::reference_project().unwrap());
    let mut s = Server::open(dir.path(), false).unwrap();
    let (o, failed) = call(&mut s, "project_overview", json!({}));
    assert!(!failed);
    assert_eq!(o["errors"], 0);
    assert_eq!(o["can_edit"], false);
    assert!(o["pages"].as_array().unwrap().len() > 3);
    let (d, _) = call(&mut s, "device_details", json!({ "tag": "P2QS1" }));
    let points = d["drawings"][0]["points"].as_array().unwrap();
    assert!(points.iter().any(|p| p["wire"]["to"].as_str().is_some_and(|t| t.starts_with("P2WB1:"))), "{points:?}");
    let (w, _) = call(&mut s, "list_wires", json!({ "device": "P2QS1" }));
    assert!(
        w["wires"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["from"].as_str().unwrap().starts_with("P2QS1:")
                || r["to"].as_str().unwrap().starts_with("P2QS1:"))
    );
    let (sym, _) = call(&mut s, "list_symbols", json!({ "search": "coil" }));
    assert!(sym["symbols"].as_array().unwrap().iter().any(|x| x["id"] == "iec.coil"));
    let (devices, _) = call(&mut s, "list_devices", json!({ "page": "#1" }));
    assert!(devices["devices"].is_array());
    let (c, _) = call(&mut s, "run_checks", json!({}));
    assert_eq!(c["errors"], 0);
    // Mistakes come back as tool errors the model can read.
    let (e, failed) = call(&mut s, "device_details", json!({ "tag": "-NOPE" }));
    assert!(failed && e == "no device `-NOPE`", "{e}");
    let (e, failed) = call(&mut s, "report", json!({ "name": "motors" }));
    assert!(failed && e == "unknown report `motors`");
}

#[test]
fn edits_need_write_and_are_saved_only_on_save() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut ro = Server::open(dir.path(), false).unwrap();
    let reply = ro.handle(&request(4, "tools/call", json!({ "name": "connect", "arguments": {} }))).unwrap();
    assert_eq!(reply["error"]["code"], -32602);
    assert!(reply["error"]["message"].as_str().unwrap().contains("--write"));

    let mut s = Server::open(dir.path(), true).unwrap();
    let (placed, failed) =
        call(&mut s, "place_symbol", json!({ "symbol": "iec.coil", "page": "#1", "x": 200, "y": 100 }));
    assert!(!failed, "{placed}");
    let tag = placed["tag"].as_str().unwrap().to_owned();
    assert_eq!(placed["unsaved_changes"], true);
    let (second, failed) =
        call(&mut s, "place_symbol", json!({ "symbol": "iec.coil", "page": "#1", "x": 240, "y": 100 }));
    assert!(!failed);
    let other = second["tag"].as_str().unwrap().to_owned();
    let (wired, failed) = call(&mut s, "connect", json!({ "from": format!("{tag}:A2"), "to": format!("{other}:A1") }));
    assert!(!failed, "{wired}");
    assert!(wired["wire"].is_string());
    let (e, failed) = call(&mut s, "connect", json!({ "from": format!("{tag}:A2"), "to": format!("{other}:A2") }));
    assert!(failed && e.as_str().unwrap().contains("already"), "{e}");
    // Not on disk yet.
    let on_disk = ecad_project::load(dir.path()).unwrap().devices.len();
    assert_eq!(on_disk, ecad_model::sample::start_circuit().unwrap().devices.len());
    let (undone, _) = call(&mut s, "undo", json!({}));
    assert!(undone["undid"].as_str().unwrap().starts_with("Draw wire"));
    call(&mut s, "redo", json!({}));
    call(&mut s, "edit_device", json!({ "tag": tag, "new_tag": "-K10", "description": "Added over MCP" }));
    let (saved, failed) = call(&mut s, "save", json!({}));
    assert!(!failed, "{saved}");
    let back = ecad_project::load(dir.path()).unwrap();
    assert_eq!(back.device_by_tag("-K10").unwrap().description, "Added over MCP");
    let (o, _) = call(&mut s, "project_overview", json!({}));
    assert_eq!(o["unsaved_changes"], false);
}

#[test]
fn plc_channels_and_devices_change_by_name() {
    let dir = project(ecad_model::sample::with_plc().unwrap());
    let mut s = Server::open(dir.path(), true).unwrap();
    let (_, failed) = call(&mut s, "set_plc_channel", json!({ "card": "-KF1", "channel": "0", "symbol": "Door_Shut" }));
    assert!(!failed);
    let (e, failed) = call(&mut s, "set_plc_channel", json!({ "card": "-KF1", "channel": "1", "address": "I0.0" }));
    assert!(failed, "duplicate address refused: {e}");
    let (_, failed) = call(&mut s, "delete_device", json!({ "tag": "-KF1" }));
    assert!(!failed);
    let (e, failed) = call(&mut s, "device_details", json!({ "tag": "-KF1" }));
    assert!(failed, "{e}");
}

#[test]
fn the_protocol_edges_are_answered() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = Server::open(dir.path(), false).unwrap();
    // An unknown version gets the newest.
    let init = s.handle(&request(1, "initialize", json!({ "protocolVersion": "1999-01-01" }))).unwrap();
    assert_eq!(init["result"]["protocolVersion"], ecad_mcp::PROTOCOL_VERSIONS[0]);
    assert_eq!(s.handle(&request(2, "ping", json!({}))).unwrap()["result"], json!({}));
    assert_eq!(s.handle(&request(3, "resources/list", json!({}))).unwrap()["error"]["code"], -32601);
    assert_eq!(s.handle(&request(4, "tools/call", json!({ "name": "nope" }))).unwrap()["error"]["code"], -32602);
    assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled" })).is_none());
    let parse: Value = serde_json::from_str(&s.handle_line("{oops").unwrap()).unwrap();
    assert_eq!(parse["error"]["code"], -32700);
    assert!(s.handle_line("   ").is_none());
    assert_eq!(s.handle(&json!([1, 2])).unwrap()["error"]["code"], -32600);
    // Every tool has a schema the client can use.
    let list = s.handle(&request(5, "tools/list", json!({}))).unwrap();
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert_eq!(t["annotations"]["readOnlyHint"], true);
    }
}
