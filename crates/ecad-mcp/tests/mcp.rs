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
fn open(dir: &std::path::Path) -> Server {
    let mut s = Server::open(dir).unwrap();
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
    assert!(names.contains(&"run_checks") && names.contains(&"prepare_connect"), "{names:?}");
    assert!(
        !names.iter().any(|n| ["connect", "save", "undo", "redo"].contains(n)),
        "no tool changes the project: {names:?}"
    );
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
    let mut s = open(dir.path());
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
    // Numbering (S0): policies and lists, and a renumbering preview that changes nothing.
    let (n, _) = call(&mut s, "numbering", json!({}));
    assert_eq!(n["policies"]["device tags"], "sequential", "{n}");
    assert!(n["reserved"].as_array().unwrap().is_empty());
    let (r, failed) = call(&mut s, "release_check", json!({}));
    assert!(!failed && r["note"].as_str().unwrap().contains("not electrical"), "{r}");
    // Mistakes come back as tool errors the model can read.
    let (e, failed) = call(&mut s, "device_details", json!({ "tag": "-NOPE" }));
    assert!(failed && e == "no device `-NOPE`", "{e}");
    let (e, failed) = call(&mut s, "report", json!({ "name": "motors" }));
    assert!(failed && e.as_str().unwrap().starts_with("`name` must be one of: wire-list"), "{e}");
}

#[test]
fn operations_are_only_prepared_never_applied() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let on_disk = ecad_project::fingerprint(dir.path()).unwrap();
    let mut s = open(dir.path());
    // Editing tools of old are gone, and say what to do instead.
    let reply = s.handle(&request(4, "tools/call", json!({ "name": "connect", "arguments": {} }))).unwrap();
    assert_eq!(reply["error"]["code"], -32602);
    assert!(reply["error"]["message"].as_str().unwrap().contains("prepare_connect"), "{reply}");
    // One change set of several operations, previewed, stored as a proposal.
    let (r, failed) = call(
        &mut s,
        "prepare_change",
        json!({
            "title": "Add a relay and wire it",
            "rationale": "test",
            "operations": [
                { "op": "place_symbol", "args": { "symbol": "iec.coil", "page": "#1", "x": 200, "y": 100 } },
                { "op": "place_symbol", "args": { "symbol": "iec.coil", "page": "#1", "x": 240, "y": 100 } },
                { "op": "edit_device", "args": { "tag": "-K1", "description": "over MCP" } },
            ],
        }),
    );
    assert!(!failed, "{r}");
    let id = r["proposal"].as_str().unwrap().to_owned();
    assert_eq!(r["impact"]["devices"].as_array().unwrap().len(), 3, "{r}");
    // Nothing changed: the engineering files are as they were.
    assert_eq!(ecad_project::fingerprint(dir.path()).unwrap(), on_disk);
    let (list, _) = call(&mut s, "list_proposals", json!({}));
    assert_eq!(list["proposals"][0]["id"], id.as_str());
    assert_eq!(list["proposals"][0]["status"], "pending");
    let (p, failed) = call(&mut s, "preview_change", json!({ "id": id }));
    assert!(!failed && p["stale"] == false, "{p}");
    // Bad operations are refused before anything is stored.
    let (e, failed) = call(
        &mut s,
        "prepare_change",
        json!({ "title": "x", "operations": [{ "op": "import_tia", "args": { "file": "/etc/passwd" } }] }),
    );
    assert!(failed && e.as_str().unwrap().contains("unknown operation"), "{e}");
    let (e, failed) = call(&mut s, "prepare_edit_device", json!({ "tag": "-NOPE" }));
    assert!(failed, "{e}");
    let (e, failed) = call(&mut s, "prepare_change", json!({ "title": "x", "operations": [] }));
    assert!(failed, "{e}");
    assert_eq!(ecad_editor::change::proposals(dir.path()).unwrap().len(), 1);
}

#[test]
fn an_assistant_cant_weaken_a_safety_classification() {
    let dir = project(ecad_model::conveyor::conveyor().unwrap());
    let mut s = open(dir.path());
    let (e, failed) = call(&mut s, "prepare_mark_safety_related", json!({ "tag": "-S0", "related": "no" }));
    assert!(failed && e.as_str().unwrap().contains("safety classification"), "{e}");
    // Deleting a safety-related device takes its marking with it: refused too.
    let (e, failed) = call(&mut s, "prepare_delete_device", json!({ "tag": "-K3" }));
    assert!(failed && e.as_str().unwrap().contains("safety classification"), "{e}");
    // Marking one is a proposal like any other.
    let (r, failed) = call(&mut s, "prepare_mark_safety_related", json!({ "tag": "-Q0", "related": "yes" }));
    assert!(!failed, "{r}");
    assert!(!r["impact"]["safety_direct"].as_array().unwrap().is_empty());
}

#[test]
fn a_stale_server_refuses_to_prepare_until_reloaded() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = open(dir.path());
    // Someone commits a change after the server loaded.
    let mut p = ecad_project::load(dir.path()).unwrap();
    let k1 = p.device_by_tag("-K1").unwrap().id;
    p.apply(ecad_model::Command::SetDescription { id: k1, description: "changed".into() }).unwrap();
    ecad_project::save(&p, dir.path()).unwrap();
    let (e, failed) = call(&mut s, "prepare_edit_device", json!({ "tag": "-K1", "new_tag": "-K9" }));
    assert!(failed && e.as_str().unwrap().contains("stale"), "{e}");
    call(&mut s, "reload", json!({}));
    let (_, failed) = call(&mut s, "prepare_edit_device", json!({ "tag": "-K1", "new_tag": "-K9" }));
    assert!(!failed);
    // A folder changed outside Electric Studio doesn't open until reconciled in the app.
    std::fs::write(dir.path().join("devices.json"), "[]").unwrap();
    assert!(Server::open(dir.path()).is_err());
}

#[test]
fn the_protocol_edges_are_answered() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = Server::open(dir.path()).unwrap();
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
fn every_tool_declares_its_hints_and_comes_from_the_registry() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = open(dir.path());
    let list = s.handle(&request(5, "tools/list", json!({}))).unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{t}");
        assert_eq!(t["annotations"]["destructiveHint"], false, "no tool changes the project: {t}");
    }
    // Generated: every query offered as itself, every operation as prepare_<name>.
    for d in ecad_editor::registry::DEFS.iter().filter(|d| d.mcp) {
        let name = if d.class == ecad_editor::registry::Class::Query {
            d.name.to_owned()
        } else {
            format!("prepare_{}", d.name)
        };
        let t = tools.iter().find(|t| t["name"] == name.as_str()).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(t["annotations"]["readOnlyHint"], d.class == ecad_editor::registry::Class::Query, "{name}");
    }
    for d in ecad_editor::registry::DEFS.iter().filter(|d| !d.mcp) {
        assert!(!tools.iter().any(|t| t["name"].as_str().unwrap().ends_with(d.name)), "{} is CLI and app only", d.name);
    }
}

#[test]
fn arguments_that_dont_fit_the_schema_change_nothing() {
    let dir = project(ecad_model::sample::start_circuit().unwrap());
    let mut s = open(dir.path());
    let on_disk = ecad_project::fingerprint(dir.path()).unwrap();
    let place = |extra: Value| {
        let mut args = json!({ "symbol": "iec.coil", "page": "#1", "x": 200, "y": 100 });
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        args
    };
    let cases = [
        ("prepare_place_symbol", place(json!({ "device": 5 })), "`device` must be a string"),
        ("prepare_place_symbol", place(json!({ "x": "200" })), "`x` must be a number"),
        ("prepare_place_symbol", place(json!({ "colour": "red" })), "unknown argument `colour`"),
        ("prepare_place_symbol", json!({ "symbol": "iec.coil", "page": "#1", "x": 1 }), "`y` is needed"),
        ("prepare_connect", json!({ "from": "-S1:13", "to": null }), "`to` must be a string"),
        ("report", json!({ "name": "motors" }), "`name` must be one of"),
        ("reload", json!({ "force": true }), "unknown argument `force`"),
        ("project_overview", json!([1]), "the arguments must be an object"),
    ];
    for (tool, args, why) in cases {
        let (e, failed) = call(&mut s, tool, args.clone());
        assert!(failed && e.as_str().unwrap().contains(why), "{tool} {args}: {e}");
    }
    assert_eq!(ecad_project::fingerprint(dir.path()).unwrap(), on_disk);
    assert!(ecad_editor::change::proposals(dir.path()).unwrap().is_empty());
}
