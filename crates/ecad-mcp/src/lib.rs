//! An MCP (Model Context Protocol) server for one Electric Studio project.
//!
//! It speaks JSON-RPC 2.0 over stdin and stdout, one message per line, as MCP's stdio
//! transport does. Its tools are generated from the engineering registry
//! (`ecad_editor::registry`): queries read the project as last committed; operations only
//! **prepare** change sets, stored as proposals in the project folder (`proposals/`) with their
//! preview. Nothing an MCP client does changes the project (D18, D32): a person reviews and
//! approves the exact proposal in the app, which applies it as one transaction. There is no
//! write mode (`--write` was removed in AI0).
//!
//! The server labels everything it prepares as the connected assistant's: the actor is
//! assigned here, from the client's own `initialize` information, recorded as a claim (D31).
//!
//! Every message is checked before anything runs: the JSON-RPC envelope (`jsonrpc`, `id`,
//! `method`, `params`), the MCP lifecycle (`initialize` first, once), and a tool's arguments
//! against its schema (types, required fields, no unknown fields).

mod tools;

use ecad_editor::Editor;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// MCP versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
This server holds one Electric Studio electrical schematic project. Devices have tags like -K1; \
a connection point is written TAG:POINT as drawn (-K1:A1, -X1:3). Pages are named as in the \
project (list them with project_overview). Start with project_overview and run_checks. You can't \
change the project: prepare_ tools and prepare_change prepare a change set (a proposal) with its \
preview, and a person reviews and approves it in the Electric Studio app, which applies it as one \
step. preview_change and list_proposals show where proposals stand. If a proposal is stale (the \
project changed), reload and prepare it again. Positions are mm on the page, snapped to a 4 mm grid.";

/// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Where the client is in MCP's lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Waiting for `initialize`.
    New,
    /// Answered `initialize`; the client may now call tools.
    Initialized,
}

/// One project, open for an MCP client.
pub struct Server {
    dir: PathBuf,
    editor: Editor,
    /// The committed revision loaded: what proposals are prepared against.
    revision: ecad_project::Revision,
    /// The connected assistant, as trusted code labels it (its name a claim).
    actor: ecad_editor::change::Actor,
    phase: Phase,
}

/// A JSON-RPC error: code and message.
struct RpcError(i64, String);

impl Server {
    /// Open the project in `dir`, read-only: an assistant prepares proposals, never applies.
    pub fn open(dir: &Path) -> Result<Server, String> {
        let (editor, revision) = load(dir)?;
        let actor = ecad_editor::change::Actor::Assistant {
            client: "unknown".into(),
            client_version: String::new(),
            session: ecad_editor::change::new_id(),
        };
        Ok(Server { dir: dir.to_owned(), editor, revision, actor, phase: Phase::New })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Answer one line from the client: `None` for a notification (or an empty line).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(&message)?,
            Err(e) => error_reply(Value::Null, RpcError(PARSE_ERROR, format!("parse error: {e}"))),
        };
        Some(reply.to_string())
    }

    /// Answer one JSON-RPC message: `None` for a notification or a response.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        let Some(m) = message.as_object() else {
            let what = if message.is_array() { "batches aren't supported" } else { "a request is a JSON object" };
            return Some(error_reply(Value::Null, RpcError(INVALID_REQUEST, what.into())));
        };
        // The id, if it is one: a string or an integer (MCP doesn't allow null).
        let id = match m.get("id") {
            None => None,
            Some(id @ Value::String(_)) => Some(id.clone()),
            Some(id @ Value::Number(n)) if n.is_i64() || n.is_u64() => Some(id.clone()),
            Some(_) => {
                return Some(error_reply(
                    Value::Null,
                    RpcError(INVALID_REQUEST, "`id` must be a string or an integer".into()),
                ));
            }
        };
        let reject = |id: Option<Value>, e: RpcError| id.map(|id| error_reply(id, e));
        if m.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return reject(id, RpcError(INVALID_REQUEST, "`jsonrpc` must be \"2.0\"".into()));
        }
        let Some(method) = m.get("method") else {
            // A response (`result` or `error`) to a request this server never sends: ignored.
            if m.contains_key("result") || m.contains_key("error") {
                return None;
            }
            return Some(error_reply(id.unwrap_or(Value::Null), RpcError(INVALID_REQUEST, "no `method`".into())));
        };
        let Some(method) = method.as_str() else {
            return reject(id, RpcError(INVALID_REQUEST, "`method` must be a string".into()));
        };
        let params = match m.get("params") {
            None => Map::new(),
            Some(Value::Object(p)) => p.clone(),
            Some(_) => return reject(id, RpcError(INVALID_PARAMS, "`params` must be an object".into())),
        };
        // Notifications get no answer, whatever they are: `notifications/initialized`,
        // cancellations, anything unknown.
        let id = id?;
        let result = match (method, self.phase) {
            ("ping", _) => Ok(json!({})),
            ("initialize", Phase::New) => {
                self.phase = Phase::Initialized;
                Ok(self.initialize(&params))
            }
            ("initialize", _) => Err(RpcError(INVALID_REQUEST, "already initialized".into())),
            (_, Phase::New) => Err(RpcError(INVALID_REQUEST, "not initialized: send `initialize` first".into())),
            ("tools/list", _) => Ok(json!({ "tools": tools::list() })),
            ("tools/call", _) => self.call(&params),
            (other, _) => Err(RpcError(METHOD_NOT_FOUND, format!("unknown method `{other}`"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => error_reply(id, e),
        })
    }

    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        // The client says who it is; that is recorded as its claim, never trusted as identity.
        if let ecad_editor::change::Actor::Assistant { client, client_version, .. } = &mut self.actor {
            let info = params.get("clientInfo");
            *client = info
                .and_then(|i| i.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .chars()
                .take(80)
                .collect();
            *client_version =
                info.and_then(|i| i.get("version")).and_then(Value::as_str).unwrap_or("").chars().take(40).collect();
        }
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "electric-studio", "title": "Electric Studio", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&mut self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(Value::as_str).ok_or(RpcError(INVALID_PARAMS, "no tool name".into()))?;
        let offered = tools::list();
        let Some(definition) = offered.iter().find(|t| t["name"] == name) else {
            let why = if ["save", "undo", "redo", "place_symbol", "connect", "delete_device", "edit_device"]
                .contains(&name)
            {
                format!(
                    "`{name}` would change the project, and MCP clients can't (D32): use prepare_{name} to propose it for a person to approve in the app"
                )
            } else {
                format!("unknown tool `{name}`")
            };
            return Err(RpcError(INVALID_PARAMS, why));
        };
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        // Arguments that don't fit the schema never reach the tool: the model reads why.
        let outcome = tools::validate(definition, &args).and_then(|()| tools::call(self, name, &args));
        Ok(match outcome {
            Ok(value) => {
                let text = match &value {
                    Value::String(s) => s.clone(),
                    v => serde_json::to_string_pretty(v).unwrap_or_default(),
                };
                let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": false });
                if value.is_object() {
                    result["structuredContent"] = value;
                }
                result
            }
            Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        })
    }

    /// Prepare a change set against the loaded revision and store it as a proposal.
    fn prepare(
        &mut self,
        title: &str,
        rationale: &str,
        ops: Vec<ecad_editor::change::OpCall>,
    ) -> Result<Value, String> {
        use ecad_editor::change;
        let cs =
            change::prepare(self.editor.project(), self.revision.clone(), self.actor.clone(), title, rationale, ops)?;
        let impact = change::preview(self.editor.project(), &cs, &ecad_project::state::Quiet)?;
        change::save_proposal(&self.dir, &cs)?;
        Ok(json!({
            "proposal": cs.id,
            "hash": cs.hash(),
            "base": cs.base.to_string(),
            "status": "pending: a person reviews and approves it in the Electric Studio app",
            "impact": impact,
        }))
    }

    /// Load the folder again, as last committed.
    fn reload(&mut self) -> Result<Value, String> {
        let (editor, revision) = load(&self.dir)?;
        self.editor = editor;
        self.revision = revision;
        Ok(json!({ "reloaded": self.dir.display().to_string(), "revision": self.revision.to_string() }))
    }
}

/// The project as committed, and its revision. A folder changed outside Electric Studio is
/// refused until a person reconciles it in the app (D43).
fn load(dir: &Path) -> Result<(Editor, ecad_project::Revision), String> {
    let v =
        ecad_project::state::verify(dir, &ecad_project::state::Quiet).map_err(|e| format!("{}: {e}", dir.display()))?;
    if v.committed.is_some() && !v.clean() {
        return Err(format!(
            "{}: changed outside Electric Studio ({}); a person reconciles it in the app",
            dir.display(),
            v.summary()
        ));
    }
    let (editor, _) = ecad_editor::session::load_editor_checked(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // A project saved before AI0 is adopted (its first manifest), so proposals have a base.
    let revision = ecad_editor::change::adopt(
        dir,
        ecad_editor::change::Actor::Assistant {
            client: "ecad-mcp".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            session: String::new(),
        },
    )?;
    Ok((editor, revision))
}

fn error_reply(id: Value, RpcError(code, message): RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
