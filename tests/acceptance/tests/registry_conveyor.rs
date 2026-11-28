//! Full AI0 acceptance: replay every forward step of the real conveyor builder through
//! named registry operations, then submit that identical sequence via MCP as one proposal.
//! No fixture-only tool, raw command input or direct application authority is exposed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test-only file

use ecad_editor::change::{self, ApprovalState, RecordKind, Status};
use ecad_editor::registry;
use ecad_editor::session::{Session, UiEvent};
use ecad_model::{Command, Page, PageId, Project};
use ecad_project::state::{self, Quiet};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn record(name: &str, value: Value) -> Value {
    json!({ "op": name, "args": { "record": value } })
}

/// Translate the fixture's forward steps to the public operation vocabulary. Every
/// unmatched step fails the test; this is not a production command-deserialization path.
fn operations(command: &Command, target: &Project, out: &mut Vec<Value>) {
    let op = match command {
        Command::AddPage(page) => record("add_page_exact", json!(page)),
        Command::AddDevice(device) => record("add_device_exact", json!(device)),
        Command::AddNode(node) => record("add_node_exact", json!(node)),
        Command::SetNode(node) => record("describe_node_exact", json!(node)),
        Command::Place(drawing) => record("place_drawing_exact", json!(drawing)),
        Command::Connect(wire) => record("connect_drawing_exact", json!(wire)),
        Command::SetDescription { id, description } => {
            json!({ "op": "edit_device", "args": { "tag": target.device(*id).unwrap().tag, "description": description } })
        }
        Command::SetProperties { id, properties } => {
            record("set_drawing_properties", json!({ "id": id, "properties": properties }))
        }
        Command::Mirror { id, to } => record("mirror_drawing_exact", json!({ "id": id, "to": to })),
        Command::SetWireBend { id, bend } => record("set_wire_bend_exact", json!({ "id": id, "bend": bend })),
        Command::SetWireNumber { id, number } => {
            record("set_wire_number_exact", json!({ "id": id, "number": number }))
        }
        Command::SetWireSpec { id, spec } => record("set_wire_spec_exact", json!({ "id": id, "spec": spec })),
        Command::AddCable(cable) => record("add_cable_exact", json!(cable)),
        Command::AddBox(location) => record("add_location_box", json!(location)),
        Command::AddSymbol(symbol) => record("add_symbol_definition", json!(symbol)),
        Command::AddPart(part) => record("add_catalogue_part", json!(part)),
        Command::SetDeviceParts { id, parts } => record("assign_device_parts", json!({ "id": id, "parts": parts })),
        Command::SetPanel { page, panel: Some(panel) } => {
            record("set_mounting_plate", json!({ "page": page, "panel": panel }))
        }
        Command::AddPanelItem(item) => record("add_panel_item", json!(item)),
        Command::SetSheet(sheet) => record("set_sheet_definition", json!(sheet)),
        Command::SetProjectInfo { name, info } => record("set_project_title", json!({ "name": name, "info": info })),
        Command::SetSafetyRelated { device, related } => {
            let tag = &target.device(*device).unwrap().tag;
            let related = if *related { "yes" } else { "no" };
            json!({ "op": "mark_safety_related", "args": { "tag": tag, "related": related } })
        }
        Command::Batch(commands) => {
            for step in commands {
                operations(step, target, out);
            }
            return;
        }
        other => panic!("conveyor step lacks a typed registry operation: {other:?}"),
    };
    out.push(op);
}

fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    ecad_project::store::project_files(dir)
        .unwrap()
        .into_iter()
        .map(|name| {
            let bytes = std::fs::read(dir.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

fn call(server: &mut ecad_mcp::Server, method: &str, params: Value) -> Value {
    server.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap()
}

fn tool(server: &mut ecad_mcp::Server, name: &str, arguments: Value) -> Value {
    call(server, "tools/call", json!({ "name": name, "arguments": arguments }))
}

#[test]
fn full_conveyor_registry_and_assistant_build_match_the_reference_byte_for_byte() {
    let (target, steps) = ecad_model::conveyor::conveyor_steps().unwrap();
    assert_eq!(target, ecad_model::conveyor::conveyor().unwrap());
    assert!(steps.len() > 40);

    // Begin with only the built-in IEC library and an empty starter page (the editor needs
    // a page to open). All actual conveyor pages, including page 1, are built by the registry.
    let mut seed = Project::new("Belt conveyor");
    for symbol in ecad_model::library::iec() {
        seed.add_symbol(symbol);
    }
    let starter = PageId::from_u128(0x99);
    seed.apply(Command::AddPage(Page {
        id: starter,
        name: "0".into(),
        description: String::new(),
        subtitle: String::new(),
        order: ecad_model::PAGE_STEP * 10,
    }))
    .unwrap();
    let mut ops = Vec::new();
    for step in &steps {
        operations(step, &target, &mut ops);
    }
    ops.push(record("remove_empty_page", json!({ "id": starter })));

    // Independently replay the same operations through the public typed registry.
    let mut replay = seed.clone();
    for (index, op) in ops.iter().enumerate() {
        let name = op["op"].as_str().unwrap();
        let def = registry::find(name).unwrap();
        assert!(def.mcp);
        let lowered = registry::lower(&replay, name, &op["args"])
            .unwrap_or_else(|e| panic!("step {index} {name}: {e}; {op}"));
        replay.apply(Command::Batch(lowered.commands)).unwrap();
    }
    assert_eq!(replay, target, "registry replay must preserve the fixture's complete model");

    let tmp = tempfile::tempdir().unwrap();
    let expected = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    ecad_project::save(&seed, dir).unwrap();
    ecad_project::save(&target, expected.path()).unwrap();
    let original_files = files(dir);
    let before = state::committed(dir).unwrap();
    let mut server = ecad_mcp::Server::open(dir).unwrap();
    let init = call(
        &mut server,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "conveyor-client", "version": "1" } }),
    );
    assert!(init["result"].is_object());
    let tools = call(&mut server, "tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    for op in &ops {
        let name = format!("prepare_{}", op["op"].as_str().unwrap());
        assert!(tools.iter().any(|tool| tool["name"] == name), "{name} missing from MCP");
    }
    let response = tool(
        &mut server,
        "prepare_change",
        json!({ "title": "Build complete conveyor", "rationale": "Acceptance fixture", "operations": ops }),
    );
    let result = &response["result"];
    assert_eq!(result["isError"], false, "{response}");
    let payload = result["structuredContent"].as_object().unwrap();
    let id = payload["proposal"].as_str().unwrap().to_owned();
    assert_eq!(payload["impact"]["checks"]["new_errors"].as_array().unwrap().len(), 0, "{response}");
    let proposal = change::read_proposal(dir, &id).unwrap();
    assert_eq!(proposal.operations.len(), ops.len());
    assert!(proposal.actor.is_assistant());
    assert_eq!(files(dir), original_files);
    assert_eq!(state::committed(dir).unwrap(), before);
    assert!(change::approvals(dir).unwrap().is_empty());
    assert!(change::issue_cli_approval(dir, &id, &proposal.hash(), change::Confirmations { safety: true, global: true }, change::Actor::person()).is_err());
    assert_eq!(files(dir), original_files);

    let mut s = Session::open(dir).unwrap();
    s.handle(UiEvent::ReviewProposal { id: id.clone() });
    let review = s.frame(None, [0, 0, 0]).review.unwrap();
    assert!(review.assistant && !review.cli && review.needs_safety_confirmation && review.needs_global_confirmation);
    assert!(review.safety_direct.iter().any(|d| d.contains("-K3")));
    assert!(review.pages.iter().any(|name| name == "L1"));
    assert!(review.other.iter().any(|what| what.contains("products")));
    assert!(review.other.iter().any(|what| what.contains("panel layout")));
    for (safety, global) in [(false, true), (true, false)] {
        s.handle(UiEvent::ApproveProposal { confirm_safety: safety, confirm_global: global });
        assert!(s.frame(None, [0, 0, 0]).status.starts_with("Not applied"));
        assert_eq!(files(dir), original_files);
        assert_eq!(state::committed(dir).unwrap(), before);
    }
    s.handle(UiEvent::ApproveProposal { confirm_safety: true, confirm_global: true });
    let status = s.frame(None, [0, 0, 0]).status;
    assert!(status.starts_with("Applied"), "{status}");
    assert_eq!(s.editor().project(), &target);
    assert_eq!(ecad_project::load(dir).unwrap(), target);
    assert_eq!(files(dir), files(expected.path()), "canonical files and all graphical page files");
    let applied = state::committed(dir).unwrap();
    assert_eq!(applied.generation, before.generation + 1);
    let approvals = change::approvals(dir).unwrap();
    let consumed = approvals.iter().find(|approval| approval.proposal == id).unwrap();
    assert!(matches!(consumed.state, ApprovalState::Consumed { .. }));
    let records = change::records(dir).unwrap();
    assert_eq!(records.iter().filter(|r| r.kind == RecordKind::ChangeSet).count(), 1);
    assert!(state::verify(dir, &Quiet).unwrap().clean());

    s.handle(UiEvent::Key { key: ecad_editor::KeyName::Undo });
    assert_eq!(s.editor().project(), &seed);
    assert_eq!(s.editor().undo_label(), None);
    s.handle(UiEvent::Save);
    assert_eq!(files(dir), original_files);
    let undone = state::committed(dir).unwrap();
    assert_eq!(undone.generation, applied.generation + 1);
    assert_eq!(undone.content, before.content);
    assert!(state::verify(dir, &Quiet).unwrap().clean());
    s.handle(UiEvent::ReviewProposal { id });
    let reviewed = s.frame(None, [0, 0, 0]).review.unwrap();
    assert_eq!(reviewed.status, Status::Applied);
    assert!(reviewed.stale.is_some());
}

#[test]
fn construction_records_do_not_accept_raw_commands_or_unknown_nested_fields() {
    let p = ecad_model::sample::start_circuit().unwrap();
    let invalid = json!({ "record": {
        "id": PageId::new(),
        "name": "2",
        "order": 50000,
        "command": { "SetAutomation": {} }
    } });
    assert!(registry::lower(&p, "add_page_exact", &invalid).is_err());
    let invalid = json!({ "record": {
        "id": ecad_model::FunctionId::new(),
        "symbol": "iec.coil",
        "page": p.pages.keys().next().unwrap(),
        "position": { "x": 20, "y": 20, "hidden_command": "SetAutomation" }
    } });
    assert!(registry::lower(&p, "place_drawing_exact", &invalid).is_err());
    assert!(registry::find("apply_command").is_none());
    assert!(registry::find("set_automation").is_none());
}
