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

/// A server past `initialize`, as a client would have it.
fn open(dir: &std::path::Path, writable: bool) -> Server {
    let mut s = Server::open(dir, writable).unwrap();
    let init = s.handle(&request(0, "initialize", json!({ "protocolVersion": "2025-06-18" }))).unwrap();
    assert!(init["result"].is_object(), "{init}");
    assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
    s
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
    let mut s = open(dir.path(), false);
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
    assert!(failed && e.as_str().unwrap().starts_with("`name` must be one of: wire-list"), "{e}");
}

#[test]
fn edits_need_write_and_are_saved_only_on_save() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut ro = open(dir.path(), false);
    let reply = ro.handle(&request(4, "tools/call", json!({ "name": "connect", "arguments": {} }))).unwrap();
    assert_eq!(reply["error"]["code"], -32602);
    assert!(reply["error"]["message"].as_str().unwrap().contains("--write"));

    let mut s = open(dir.path(), true);
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
    let mut s = open(dir.path(), true);
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
    // Nothing but ping before initialize.
    assert_eq!(s.handle(&request(1, "ping", json!({}))).unwrap()["result"], json!({}));
    let early = s.handle(&request(2, "tools/list", json!({}))).unwrap();
    assert_eq!(early["error"]["code"], -32600, "{early}");
    // An unknown version gets the newest; initialize happens once.
    let init = s.handle(&request(3, "initialize", json!({ "protocolVersion": "1999-01-01" }))).unwrap();
    assert_eq!(init["result"]["protocolVersion"], ecad_mcp::PROTOCOL_VERSIONS[0]);
    assert_eq!(s.handle(&request(4, "initialize", json!({}))).unwrap()["error"]["code"], -32600);
    assert_eq!(s.handle(&request(5, "resources/list", json!({}))).unwrap()["error"]["code"], -32601);
    assert_eq!(s.handle(&request(6, "tools/call", json!({ "name": "nope" }))).unwrap()["error"]["code"], -32602);
    // Notifications are never answered, known or not, well formed or not.
    assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled" })).is_none());
    assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "save" } })).is_none());
    // Responses to requests the server never sent are ignored.
    assert!(s.handle(&json!({ "jsonrpc": "2.0", "id": 7, "result": {} })).is_none());
    let parse: Value = serde_json::from_str(&s.handle_line("{oops").unwrap()).unwrap();
    assert_eq!(parse["error"]["code"], -32700);
    assert!(s.handle_line("   ").is_none());
    // Envelopes that aren't JSON-RPC 2.0 requests.
    let code = |s: &mut Server, m: Value| s.handle(&m).map(|r| (r["error"]["code"].clone(), r["id"].clone()));
    assert_eq!(code(&mut s, json!([1, 2])), Some((json!(-32600), Value::Null)));
    assert_eq!(code(&mut s, json!("tools/list")), Some((json!(-32600), Value::Null)));
    assert_eq!(code(&mut s, json!({ "id": 8, "method": "ping" })), Some((json!(-32600), json!(8))), "no jsonrpc");
    assert_eq!(code(&mut s, json!({ "jsonrpc": "1.0", "id": 9, "method": "ping" })), Some((json!(-32600), json!(9))));
    for bad in [json!(null), json!(1.5), json!(true), json!({ "a": 1 }), json!([1])] {
        let reply = code(&mut s, json!({ "jsonrpc": "2.0", "id": bad, "method": "ping" }));
        assert_eq!(reply, Some((json!(-32600), Value::Null)), "id {bad}");
    }
    assert_eq!(code(&mut s, json!({ "jsonrpc": "2.0", "id": "s", "method": 5 })), Some((json!(-32600), json!("s"))));
    assert_eq!(code(&mut s, json!({ "jsonrpc": "2.0", "id": 10 })), Some((json!(-32600), json!(10))));
    let params = json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list", "params": [1] });
    assert_eq!(code(&mut s, params), Some((json!(-32602), json!(11))));
    // A string id is answered with the same id.
    assert_eq!(s.handle(&json!({ "jsonrpc": "2.0", "id": "abc", "method": "ping" })).unwrap()["id"], "abc");
}

#[test]
fn every_tool_declares_its_hints() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = open(dir.path(), true);
    let list = s.handle(&request(5, "tools/list", json!({}))).unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    let hint = |name: &str, key: &str| {
        let t = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name}"));
        t["annotations"][key].as_bool().unwrap()
    };
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{t}");
    }
    for read in
        ["project_overview", "list_devices", "device_details", "list_wires", "run_checks", "report", "list_symbols"]
    {
        assert!(hint(read, "readOnlyHint") && !hint(read, "destructiveHint"), "{read}");
    }
    for adds in ["place_symbol", "connect"] {
        assert!(!hint(adds, "readOnlyHint") && !hint(adds, "destructiveHint"), "{adds}");
    }
    for changes in ["delete_wire", "delete_device", "edit_device", "set_plc_channel", "undo", "redo", "save", "reload"]
    {
        assert!(!hint(changes, "readOnlyHint") && hint(changes, "destructiveHint"), "{changes}");
    }
}

#[test]
fn arguments_that_dont_fit_the_schema_change_nothing() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = open(dir.path(), true);
    let before = s.editor().project().clone();
    let on_disk = ecad_project::fingerprint(dir.path()).unwrap();
    let place = |extra: Value| {
        let mut args = json!({ "symbol": "iec.coil", "page": "#1", "x": 200, "y": 100 });
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        args
    };
    let cases = [
        ("place_symbol", place(json!({ "device": 5 })), "`device` must be a string"),
        ("place_symbol", place(json!({ "x": "200" })), "`x` must be a number"),
        ("place_symbol", place(json!({ "colour": "red" })), "unknown argument `colour`"),
        ("place_symbol", json!({ "symbol": "iec.coil", "page": "#1", "x": 1 }), "`y` is needed"),
        ("connect", json!({ "from": "-S1:13", "to": null }), "`to` must be a string"),
        ("report", json!({ "name": "motors" }), "`name` must be one of"),
        ("save", json!({ "force": true }), "unknown argument `force` (arguments: none)"),
        ("project_overview", json!([1]), "the arguments must be an object"),
    ];
    for (tool, args, why) in cases {
        let (e, failed) = call(&mut s, tool, args.clone());
        assert!(failed && e.as_str().unwrap().contains(why), "{tool} {args}: {e}");
    }
    assert_eq!(s.editor().project(), &before);
    assert_eq!(s.editor().undo_label(), None);
    assert_eq!(ecad_project::fingerprint(dir.path()).unwrap(), on_disk);
}

#[test]
fn a_save_over_someone_elses_is_refused_until_reload() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut a = open(dir.path(), true);
    let mut b = open(dir.path(), true);
    // Nothing to save: nothing written.
    let on_disk = ecad_project::fingerprint(dir.path()).unwrap();
    let (r, failed) = call(&mut a, "save", json!({}));
    assert!(!failed && r["saved"] == false, "{r}");
    assert_eq!(ecad_project::fingerprint(dir.path()).unwrap(), on_disk);
    // A saves an edit.
    call(&mut a, "edit_device", json!({ "tag": "-K1", "description": "from A" }));
    let (r, failed) = call(&mut a, "save", json!({}));
    assert!(!failed && r["saved"] == true, "{r}");
    // B, still holding the old project, edits and tries to save: refused, the folder keeps A's.
    call(&mut b, "edit_device", json!({ "tag": "-S1", "description": "from B" }));
    let (e, failed) = call(&mut b, "save", json!({}));
    assert!(failed && e.as_str().unwrap().contains("changed on disk"), "{e}");
    let saved = ecad_project::load(dir.path()).unwrap();
    assert_eq!(saved.device_by_tag("-K1").unwrap().description, "from A");
    assert_eq!(saved.device_by_tag("-S1").unwrap().description, "Start", "as before B");
    // B reloads (dropping its edit), redoes it, and saves both.
    let (r, _) = call(&mut b, "reload", json!({}));
    assert_eq!(r["dropped_unsaved_changes"], true);
    call(&mut b, "edit_device", json!({ "tag": "-S1", "description": "from B" }));
    let (_, failed) = call(&mut b, "save", json!({}));
    assert!(!failed);
    let saved = ecad_project::load(dir.path()).unwrap();
    assert_eq!(saved.device_by_tag("-K1").unwrap().description, "from A");
    assert_eq!(saved.device_by_tag("-S1").unwrap().description, "from B");
    // The desktop editor is held to the same rule.
    let mut desk = ecad_editor::session::Session::open(dir.path()).unwrap();
    call(&mut a, "reload", json!({}));
    call(&mut a, "edit_device", json!({ "tag": "-K1", "description": "A again" }));
    call(&mut a, "save", json!({}));
    desk.handle(ecad_editor::session::UiEvent::Rename { tag: "-K1".into() });
    desk.handle(ecad_editor::session::UiEvent::Save);
    assert_eq!(ecad_project::load(dir.path()).unwrap().device_by_tag("-K1").unwrap().description, "A again");
}
