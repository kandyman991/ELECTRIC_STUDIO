//! The tools: what they take, and what they do.

use crate::Server;
use ecad_editor::script::{find_device, find_page};
use ecad_model::Project;
use serde_json::{Value, json};

/// What a tool does to the project, for the client's hints: declared per tool, never guessed
/// from its name. Hints describe; they don't authorise (only `--write` does).
#[derive(Clone, Copy)]
enum Effect {
    /// Only reads.
    Reads,
    /// Adds to the project and changes nothing already there.
    Adds,
    /// Changes or removes what is there (in memory, undoable).
    Changes,
    /// Overwrites or throws away saved or unsaved work: `save`, `reload`.
    Overwrites,
}

/// A tool's definition: name, description, JSON Schema of its arguments, and hints.
fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str], effect: Effect) -> Value {
    let (read_only, destructive, idempotent) = match effect {
        Effect::Reads => (true, false, true),
        Effect::Adds => (false, false, false),
        Effect::Changes => (false, true, false),
        Effect::Overwrites => (false, true, false),
    };
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false,
        },
    })
}

/// Check arguments against a tool's schema: an object, no unknown fields, every required one,
/// each of its type (and in its list, for an enum).
pub fn validate(definition: &Value, args: &Value) -> Result<(), String> {
    let schema = &definition["inputSchema"];
    let Some(args) = args.as_object() else { return Err("the arguments must be an object".into()) };
    let properties = schema["properties"].as_object().cloned().unwrap_or_default();
    for (key, value) in args {
        let Some(property) = properties.get(key) else {
            let known: Vec<&String> = properties.keys().collect();
            let known = if known.is_empty() {
                "none".to_owned()
            } else {
                known.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
            };
            return Err(format!("unknown argument `{key}` (arguments: {known})"));
        };
        let fits = match property["type"].as_str() {
            Some("string") => value.is_string(),
            Some("number") => value.as_f64().is_some_and(f64::is_finite),
            _ => true,
        };
        if !fits {
            return Err(format!("`{key}` must be a {}", property["type"].as_str().unwrap_or("value")));
        }
        if let Some(allowed) = property["enum"].as_array()
            && !allowed.contains(value)
        {
            let list: Vec<String> = allowed.iter().filter_map(Value::as_str).map(str::to_owned).collect();
            return Err(format!("`{key}` must be one of: {}", list.join(", ")));
        }
    }
    for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !args.contains_key(key) {
            return Err(format!("`{key}` is needed"));
        }
    }
    Ok(())
}

fn string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn number(description: &str) -> Value {
    json!({ "type": "number", "description": description })
}

/// The tools on offer: the read tools, and with `writable` the edit tools too.
pub fn list(writable: bool) -> Vec<Value> {
    let mut tools = vec![
        tool(
            "project_overview",
            "Project overview",
            "The project's name, pages, how many devices, symbols and wires it has, its errors and warnings, and whether there are unsaved changes.",
            json!({}),
            &[],
            Effect::Reads,
        ),
        tool(
            "list_devices",
            "List devices",
            "Every device (by tag) with its description, parts and where it is drawn. Optionally only devices drawn on one page.",
            json!({ "page": string("A page name, or #n for the n-th page") }),
            &[],
            Effect::Reads,
        ),
        tool(
            "device_details",
            "Device details",
            "One device: its drawings (symbol, page, position, properties) and each connection point with the wire on it and where that wire goes.",
            json!({ "tag": string("The device's tag, like -K1") }),
            &["tag"],
            Effect::Reads,
        ),
        tool(
            "list_wires",
            "List wires",
            "The wire list: wire number, potential, both ends as TAG:POINT, cross-section, colour, cable and estimated length. Optionally only wires at one device.",
            json!({ "device": string("Only wires with an end on this device, like -K1") }),
            &[],
            Effect::Reads,
        ),
        tool(
            "run_checks",
            "Run checks",
            "The project's checks: errors and warnings (unconnected points, duplicate tags or addresses, missing parts, layout clashes and more), each with its page location.",
            json!({}),
            &[],
            Effect::Reads,
        ),
        tool(
            "report",
            "Report",
            &format!("One report as CSV: {}.", ecad_reports::REPORTS.join(", ")),
            json!({ "name": { "type": "string", "enum": ecad_reports::REPORTS, "description": "The report" } }),
            &["name"],
            Effect::Reads,
        ),
        tool(
            "list_symbols",
            "List symbols",
            "Symbols that can be placed, with their id, description, category and connection points. Optionally filtered by a word in the id, description or category.",
            json!({ "search": string("A word to look for, like coil or terminal") }),
            &[],
            Effect::Reads,
        ),
    ];
    if writable {
        tools.extend([
            tool(
                "place_symbol",
                "Place symbol",
                "Place a symbol on a page at x, y mm (snapped to the 4 mm grid). Without `device` it is a new device with the project's next tag; with `device` it is another drawing of that device (a relay's contact, a terminal of a strip).",
                json!({
                    "symbol": string("A symbol id from list_symbols, like iec.coil"),
                    "page": string("A page name, or #n for the n-th page"),
                    "x": number("mm from the left of the page"),
                    "y": number("mm from the bottom of the page"),
                    "device": string("An existing device to add this drawing to, like -K1"),
                }),
                &["symbol", "page", "x", "y"],
                Effect::Adds,
            ),
            tool(
                "connect",
                "Connect",
                "Draw a wire between two free connection points, written TAG:POINT as drawn (-S1:14 to -K1:A1). It takes its potential's wire number.",
                json!({ "from": string("One end, like -S1:14"), "to": string("The other end, like -K1:A1") }),
                &["from", "to"],
                Effect::Adds,
            ),
            tool(
                "delete_wire",
                "Delete wire",
                "Delete the wire at a connection point.",
                json!({ "at": string("A connection point the wire is on, like -K1:A1") }),
                &["at"],
                Effect::Changes,
            ),
            tool(
                "delete_device",
                "Delete device",
                "Delete a device: all its drawings, the wires on them, and its place on a panel layout.",
                json!({ "tag": string("The device's tag, like -K1") }),
                &["tag"],
                Effect::Changes,
            ),
            tool(
                "edit_device",
                "Edit device",
                "Give a device a new tag and/or description.",
                json!({ "tag": string("The device's tag now"), "new_tag": string("Its new tag"), "description": string("Its new description") }),
                &["tag"],
                Effect::Changes,
            ),
            tool(
                "set_plc_channel",
                "Set PLC channel",
                "Set a PLC channel's address (I0.3, IW256), symbol and comment. Leave a field out to keep it, or give \"\" to clear it.",
                json!({
                    "card": string("The PLC card's tag, like -KF1"),
                    "channel": string("The channel number drawn on the card, like 3"),
                    "address": string("Like I0.3, Q1.0, IW256"),
                    "symbol": string("The PLC tag name"),
                    "comment": string("The comment"),
                }),
                &["card", "channel"],
                Effect::Changes,
            ),
            tool("undo", "Undo", "Undo the last change.", json!({}), &[], Effect::Changes),
            tool("redo", "Redo", "Redo the last undone change.", json!({}), &[], Effect::Changes),
            tool(
                "save",
                "Save",
                "Write the project to its folder, replacing what is saved there. Until then changes are only in this server. Refused if someone else saved the folder since this server loaded or last saved it; does nothing without unsaved changes.",
                json!({}),
                &[],
                Effect::Overwrites,
            ),
            tool(
                "reload",
                "Reload",
                "Load the project folder again, as last saved (by anyone). Drops this server's unsaved changes and undo history.",
                json!({}),
                &[],
                Effect::Overwrites,
            ),
        ]);
    }
    tools
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    text(args, key).ok_or_else(|| format!("`{key}` is needed"))
}

fn required_number(args: &Value, key: &str) -> Result<f64, String> {
    args.get(key).and_then(Value::as_f64).ok_or_else(|| format!("`{key}` must be a number"))
}

/// Run a tool. Its value is JSON (an object, or a string for CSV text), or a message saying
/// what went wrong.
pub fn call(server: &mut Server, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "project_overview" => Ok(overview(server)),
        "list_devices" => list_devices(server.editor.project(), text(args, "page")),
        "device_details" => device_details(server.editor.project(), required(args, "tag")?),
        "list_wires" => list_wires(server.editor.project(), text(args, "device")),
        "run_checks" => run_checks(server.editor.project()),
        "report" => {
            let name = required(args, "name")?;
            let csv = ecad_reports::csv(server.editor.project(), name).map_err(|e| e.to_string())?;
            csv.map(Value::String).ok_or_else(|| format!("unknown report `{name}`"))
        }
        "list_symbols" => Ok(list_symbols(server.editor.project(), text(args, "search"))),
        "place_symbol" => {
            let page = find_page(server.editor.project(), required(args, "page")?)?;
            let (x, y) = (required_number(args, "x")?, required_number(args, "y")?);
            let f = server.editor.script_place(required(args, "symbol")?, page, x, y, text(args, "device"))?;
            let p = server.editor.project();
            let func = p.function(f).map_err(|e| e.to_string())?;
            let tag = p.device(func.device).map_err(|e| e.to_string())?.tag.clone();
            let points = points_of(p, func);
            changed(server, json!({ "tag": tag, "x": func.position.x, "y": func.position.y, "points": points }))
        }
        "connect" => {
            let id = server.editor.script_connect(required(args, "from")?, required(args, "to")?)?;
            let wire = server.editor.project().connection(id).ok().and_then(|c| c.wire_number.clone());
            changed(server, json!({ "wire": wire }))
        }
        "delete_wire" => {
            let wire = server.editor.script_disconnect(required(args, "at")?)?;
            changed(server, json!({ "deleted_wire": wire }))
        }
        "delete_device" => {
            let tag = required(args, "tag")?;
            server.editor.script_delete_device(tag)?;
            changed(server, json!({ "deleted": tag }))
        }
        "edit_device" => {
            let tag = required(args, "tag")?;
            server.editor.script_edit_device(tag, text(args, "new_tag"), text(args, "description"))?;
            changed(server, json!({ "tag": text(args, "new_tag").unwrap_or(tag) }))
        }
        "set_plc_channel" => {
            let (card, channel) = (required(args, "card")?, required(args, "channel")?);
            let (address, symbol, comment) = (text(args, "address"), text(args, "symbol"), text(args, "comment"));
            server.editor.script_set_plc_channel(card, channel, address, symbol, comment)?;
            changed(server, json!({ "channel": format!("{card}:{channel}") }))
        }
        "undo" | "redo" => {
            let done = if name == "undo" { server.editor.undo() } else { server.editor.redo() };
            let label = done.map_err(|e| e.to_string())?;
            let key = if name == "undo" { "undid" } else { "redid" };
            changed(server, json!({ key: label }))
        }
        "save" => server.save(),
        "reload" => server.reload(),
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// An edit's result, with whether the project now has unsaved changes and what undo would do.
fn changed(server: &Server, mut value: Value) -> Result<Value, String> {
    value["unsaved_changes"] = json!(server.editor.is_dirty());
    value["undo"] = json!(server.editor.undo_label());
    Ok(value)
}

fn overview(server: &Server) -> Value {
    let p = server.editor.project();
    let checks = ecad_rules::check(p);
    let errors = checks.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
    let pages: Vec<Value> = p
        .pages_in_order()
        .iter()
        .enumerate()
        .map(|(n, page)| json!({ "number": n + 1, "name": page.name, "description": page.description }))
        .collect();
    json!({
        "project": p.name,
        "folder": server.dir.display().to_string(),
        "pages": pages,
        "devices": p.devices.len(),
        "symbols_placed": p.functions.len(),
        "wires": p.connections.len(),
        "errors": errors,
        "warnings": checks.len() - errors,
        "unsaved_changes": server.editor.is_dirty(),
        "undo": server.editor.undo_label(),
        "redo": server.editor.redo_label(),
        "can_edit": server.writable,
    })
}

fn page_name(p: &Project, page: ecad_model::PageId) -> String {
    p.page(page).map(|pg| pg.name.clone()).unwrap_or_default()
}

fn list_devices(p: &Project, page: Option<&str>) -> Result<Value, String> {
    let only = page.map(|name| find_page(p, name)).transpose()?;
    let mut devices: Vec<&ecad_model::Device> = p.devices.values().collect();
    devices.sort_by(|a, b| a.tag.cmp(&b.tag));
    let list: Vec<Value> = devices
        .into_iter()
        .filter(|d| only.is_none_or(|pg| p.functions_of(d.id).any(|f| f.page == pg)))
        .map(|d| {
            let drawings: Vec<Value> = p
                .functions_of(d.id)
                .map(|f| json!({ "symbol": f.symbol.0, "page": page_name(p, f.page), "x": f.position.x, "y": f.position.y }))
                .collect();
            let parts: Vec<Value> = d.parts.iter().map(|r| json!({ "number": r.number, "quantity": r.quantity })).collect();
            json!({ "tag": d.tag, "description": d.description, "parts": parts, "drawings": drawings })
        })
        .collect();
    Ok(json!({ "devices": list }))
}

/// A drawing's connection points as drawn, each with its wire and that wire's other end.
fn points_of(p: &Project, f: &ecad_model::Function) -> Vec<Value> {
    let Ok(symbol) = p.symbol(&f.symbol) else { return Vec::new() };
    symbol
        .connection_points
        .iter()
        .map(|cp| {
            let r = ecad_model::ConnectionRef::new(f.id, &cp.name);
            let wire = p.connection_at(&r).map(|c| {
                let other = if c.from == r { &c.to } else { &c.from };
                let to = p.describe(other).map(|(t, pt)| format!("{t}:{pt}")).unwrap_or_default();
                json!({ "number": c.wire_number, "to": to })
            });
            json!({ "point": f.designation(&cp.name), "wire": wire })
        })
        .collect()
}

fn device_details(p: &Project, tag: &str) -> Result<Value, String> {
    let id = find_device(p, tag)?;
    let d = p.device(id).map_err(|e| e.to_string())?;
    let drawings: Vec<Value> = p
        .functions_of(id)
        .map(|f| {
            json!({
                "symbol": f.symbol.0,
                "page": page_name(p, f.page),
                "x": f.position.x,
                "y": f.position.y,
                "properties": f.properties,
                "points": points_of(p, f),
            })
        })
        .collect();
    let parts: Vec<Value> = d.parts.iter().map(|r| json!({ "number": r.number, "quantity": r.quantity })).collect();
    Ok(json!({ "tag": d.tag, "description": d.description, "parts": parts, "drawings": drawings }))
}

fn list_wires(p: &Project, device: Option<&str>) -> Result<Value, String> {
    if let Some(tag) = device {
        find_device(p, tag)?;
    }
    let rows = ecad_reports::wire_list(p).map_err(|e| e.to_string())?;
    let wires: Vec<Value> = rows
        .iter()
        .filter(|r| device.is_none_or(|t| r.from_tag == t || r.to_tag == t))
        .map(|r| {
            json!({
                "wire": r.wire,
                "potential": r.potential,
                "from": format!("{}:{}", r.from_tag, r.from_point),
                "to": format!("{}:{}", r.to_tag, r.to_point),
                "cross_section": r.cross_section,
                "colour": r.colour,
                "cable": r.cable,
                "core": r.core,
                "length_mm": r.length,
            })
        })
        .collect();
    Ok(json!({ "wires": wires }))
}

fn run_checks(p: &Project) -> Result<Value, String> {
    let checks = ecad_rules::check(p);
    let errors = checks.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
    let list: Vec<Value> = checks
        .iter()
        .map(|c| json!({ "rule": c.rule, "severity": c.severity, "message": c.message, "location": c.location }))
        .collect();
    Ok(json!({ "errors": errors, "warnings": checks.len() - errors, "checks": list }))
}

fn list_symbols(p: &Project, search: Option<&str>) -> Value {
    let needle = search.map(str::to_lowercase);
    let list: Vec<Value> = p
        .symbols
        .values()
        .filter(|s| {
            needle.as_deref().is_none_or(|n| {
                [&s.id.0, &s.description, &s.category].iter().any(|t| t.to_lowercase().contains(n))
            })
        })
        .map(|s| {
            let points: Vec<&str> = s.connection_points.iter().map(|c| c.name.as_str()).collect();
            json!({ "id": s.id.0, "description": s.description, "category": s.category, "tag_prefix": s.tag_prefix, "points": points })
        })
        .collect();
    json!({ "symbols": list })
}
