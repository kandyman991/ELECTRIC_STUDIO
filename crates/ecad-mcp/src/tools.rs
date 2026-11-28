//! The tools, generated from the engineering registry ([`ecad_editor::registry`], D16): every
//! query as a read tool, every operation offered to assistants as a `prepare_<name>` tool, and
//! the change-set tools (`prepare_change`, `preview_change`, `list_proposals`) and `reload`.
//! No tool changes the project (D32): a prepared change set is stored as a proposal for a
//! person to review and approve in the app.

use crate::Server;
use ecad_editor::change::{self, OpCall};
use ecad_editor::registry::{self, Class, Def};
use serde_json::{Value, json};

fn hints(read_only: bool) -> Value {
    json!({ "readOnlyHint": read_only, "destructiveHint": false, "idempotentHint": read_only, "openWorldHint": false })
}

fn with_proposal_fields(mut schema: Value) -> Value {
    if let Some(p) = schema["properties"].as_object_mut() {
        p.insert(
            "title".into(),
            json!({ "type": "string", "description": "What the change is, in a few words (for the reviewer)" }),
        );
        p.insert(
            "rationale".into(),
            json!({ "type": "string", "description": "Why, in your words (for the reviewer)" }),
        );
    }
    schema
}

/// The tool list.
pub fn list() -> Vec<Value> {
    let mut tools = Vec::new();
    for d in registry::DEFS.iter().filter(|d| d.mcp) {
        if d.class == Class::Query {
            tools.push(json!({
                "name": d.name, "title": d.title, "description": d.description,
                "inputSchema": registry::schema(d), "annotations": hints(true),
            }));
        } else {
            tools.push(json!({
                "name": format!("prepare_{}", d.name),
                "title": format!("Prepare: {}", d.title),
                "description": format!("{} Prepares a change set (a proposal) and its preview; nothing changes until a person approves it in the app.", d.description),
                "inputSchema": with_proposal_fields(registry::schema(d)),
                "annotations": hints(false),
            }));
        }
    }
    let ops: Vec<&str> = registry::DEFS.iter().filter(|d| d.mcp && d.class != Class::Query).map(|d| d.name).collect();
    tools.push(json!({
        "name": "prepare_change",
        "title": "Prepare a change set",
        "description": format!("Prepare one change set of several operations, reviewed and approved as one transaction (one undo step). Each operation is {{\"op\": name, \"args\": {{…}}}} with the arguments of its prepare_ tool. Operations: {}.", ops.join(", ")),
        "inputSchema": {
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "What the change is" },
                "rationale": { "type": "string", "description": "Why" },
                "operations": { "type": "array", "description": "The operations, in order", "items": { "type": "object" } },
            },
            "required": ["title", "operations"],
            "additionalProperties": false,
        },
        "annotations": hints(false),
    }));
    tools.push(json!({
        "name": "preview_change",
        "title": "Preview a proposal",
        "description": "A stored proposal's preview against the project as it is now: affected devices (old and new tags), terminals, wires, pages, checks before and after, and direct and indirect safety impact; and its status.",
        "inputSchema": { "type": "object", "properties": { "id": { "type": "string", "description": "The proposal's id" } }, "required": ["id"], "additionalProperties": false },
        "annotations": hints(true),
    }));
    tools.push(json!({
        "name": "list_proposals",
        "title": "List proposals",
        "description": "The proposals in the project folder with their status: pending, approved (for the CLI), applied or rejected.",
        "inputSchema": { "type": "object", "properties": {}, "required": [], "additionalProperties": false },
        "annotations": hints(true),
    }));
    tools.push(json!({
        "name": "reload",
        "title": "Reload",
        "description": "Load the project folder again, as last committed (after a person applied or saved changes).",
        "inputSchema": { "type": "object", "properties": {}, "required": [], "additionalProperties": false },
        "annotations": hints(true),
    }));
    tools
}

/// Check arguments against a tool's schema.
pub fn validate(definition: &Value, args: &Value) -> Result<(), String> {
    let name = definition["name"].as_str().unwrap_or_default();
    let Some(map) = args.as_object() else { return Err("the arguments must be an object".into()) };
    if let Some(def) = name.strip_prefix("prepare_").and_then(registry::find).or_else(|| registry::find(name)) {
        let mut rest = map.clone();
        rest.remove("title");
        rest.remove("rationale");
        return registry::validate(def, &Value::Object(rest));
    }
    let schema = &definition["inputSchema"];
    let properties = schema["properties"].as_object().cloned().unwrap_or_default();
    for (key, value) in map {
        let Some(property) = properties.get(key) else { return Err(format!("unknown argument `{key}`")) };
        let fits = match property["type"].as_str() {
            Some("string") => value.is_string(),
            Some("array") => value.is_array(),
            _ => true,
        };
        if !fits {
            return Err(format!("`{key}` must be a {}", property["type"].as_str().unwrap_or("value")));
        }
    }
    for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !map.contains_key(key) {
            return Err(format!("`{key}` is needed"));
        }
    }
    Ok(())
}

fn op_def(name: &str) -> Result<&'static Def, String> {
    registry::find(name)
        .filter(|d| d.mcp && d.class != Class::Query)
        .ok_or_else(|| format!("unknown operation `{name}`"))
}

/// Run a tool.
pub fn call(server: &mut Server, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "reload" => server.reload(),
        "list_proposals" => {
            let list = change::proposals(&server.dir)?;
            let rows: Vec<Value> = list
                .iter()
                .map(|l| json!({ "id": l.change_set.id, "title": l.change_set.title, "actor": l.change_set.actor.label(), "status": l.status, "hash": l.hash, "base": l.change_set.base.to_string() }))
                .collect();
            Ok(json!({ "proposals": rows }))
        }
        "preview_change" => {
            let id = args["id"].as_str().unwrap_or_default();
            let cs = change::read_proposal(&server.dir, id)?;
            let impact = change::preview(server.editor.project(), &cs, &ecad_project::state::Quiet)?;
            let (a, r) = (change::approvals(&server.dir)?, change::records(&server.dir)?);
            Ok(
                json!({ "id": cs.id, "hash": cs.hash(), "status": change::status(&a, &r, &cs.id), "base": cs.base.to_string(), "current": server.revision.to_string(), "stale": cs.base != server.revision, "impact": impact }),
            )
        }
        "prepare_change" => {
            let ops: Vec<OpCall> = args["operations"]
                .as_array()
                .ok_or("`operations` must be an array")?
                .iter()
                .map(|o| {
                    let op = o["op"].as_str().ok_or("each operation needs `op`")?;
                    let def = op_def(op)?;
                    let call_args = o.get("args").cloned().unwrap_or(json!({}));
                    registry::validate(def, &call_args).map_err(|e| format!("{op}: {e}"))?;
                    Ok(OpCall { op: op.to_owned(), args: call_args })
                })
                .collect::<Result<_, String>>()?;
            server.prepare(
                args["title"].as_str().unwrap_or_default(),
                args["rationale"].as_str().unwrap_or_default(),
                ops,
            )
        }
        other => {
            if let Some(op) = other.strip_prefix("prepare_") {
                op_def(op)?;
                let mut call_args = args.clone();
                let (title, rationale) = match call_args.as_object_mut() {
                    Some(m) => (
                        m.remove("title").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default(),
                        m.remove("rationale").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default(),
                    ),
                    None => (String::new(), String::new()),
                };
                return server.prepare(&title, &rationale, vec![OpCall { op: op.to_owned(), args: call_args }]);
            }
            let mut value = registry::query(server.editor.project(), other, args)?;
            if other == "project_overview" {
                value["folder"] = json!(server.dir.display().to_string());
                value["revision"] = json!(server.revision.to_string());
                value["can_edit"] = json!(false);
                value["how_to_change"] =
                    json!("prepare a change set with a prepare_ tool; a person reviews and approves it in the app");
            }
            Ok(value)
        }
    }
}
