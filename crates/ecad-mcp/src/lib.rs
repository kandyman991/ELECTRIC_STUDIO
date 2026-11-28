//! An MCP (Model Context Protocol) server for one Electric Studio project.
//!
//! It speaks JSON-RPC 2.0 over stdin and stdout, one message per line, as MCP's stdio
//! transport does. Read tools describe the project (pages, devices, wires, checks, reports,
//! the symbol library). With `--write`, edit tools change it through the editor's commands,
//! one undo step each; changes stay in memory until the `save` tool writes the project folder.
//! Without `--write` the edit tools aren't offered at all.
//!
//! Saving is safe beside the editor and other servers: the folder is written as one step
//! (`ecad_project::store`), one saver at a time, and a save is refused if someone else saved
//! since this server loaded or last saved (then `reload` brings their version in).
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
project (list them with project_overview). Start with project_overview and run_checks. Edit tools, \
when offered, change the project in memory with one undo step each; nothing reaches the project \
folder until you call save. If save says the project changed on disk, someone else saved it: \
reload to get their version (your unsaved changes are dropped). Positions are mm on the page, \
snapped to a 4 mm grid.";

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
    writable: bool,
    /// The folder as loaded or last saved, so a save over someone else's is refused.
    seen: Option<ecad_project::Fingerprint>,
    phase: Phase,
}

/// A JSON-RPC error: code and message.
struct RpcError(i64, String);

impl Server {
    /// Open the project in `dir`. With `writable` false the edit tools aren't offered.
    pub fn open(dir: &Path, writable: bool) -> Result<Server, String> {
        let (editor, seen) = load(dir)?;
        Ok(Server { dir: dir.to_owned(), editor, writable, seen, phase: Phase::New })
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
            ("tools/list", _) => Ok(json!({ "tools": tools::list(self.writable) })),
            ("tools/call", _) => self.call(&params),
            (other, _) => Err(RpcError(METHOD_NOT_FOUND, format!("unknown method `{other}`"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => error_reply(id, e),
        })
    }

    fn initialize(&self, params: &Map<String, Value>) -> Value {
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
        let offered = tools::list(self.writable);
        let Some(definition) = offered.iter().find(|t| t["name"] == name) else {
            let why = if tools::list(true).iter().any(|t| t["name"] == name) {
                format!("`{name}` edits the project; start the server with --write to allow it")
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

    /// Save, unless there is nothing to save or someone else saved since this server loaded
    /// or last saved.
    fn save(&mut self) -> Result<Value, String> {
        if !self.editor.is_dirty() {
            return Ok(json!({ "saved": false, "reason": "no unsaved changes" }));
        }
        // A format 1 conversion with ambiguous mappings is saved by a person, in the app.
        let findings = ecad_project::conversion_findings(&self.dir).map_err(|e| e.to_string())?;
        if !findings.is_empty() {
            let list: Vec<String> = findings.iter().map(ToString::to_string).collect();
            return Err(format!(
                "not saved: converting this format 1 project kept these apart without merging them, and a person must decide: {}",
                list.join("; ")
            ));
        }
        match ecad_project::save_checked(self.editor.project(), &self.dir, self.seen) {
            Ok(seen) => {
                self.seen = Some(seen);
                self.editor.mark_saved();
                Ok(json!({ "saved": true, "folder": self.dir.display().to_string() }))
            }
            Err(e @ ecad_project::ProjectError::Changed(_)) => Err(format!(
                "{e}. Call reload to get the saved version (this drops your unsaved changes), then redo them."
            )),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Load the folder again, dropping unsaved changes.
    fn reload(&mut self) -> Result<Value, String> {
        let dropped = self.editor.is_dirty();
        let (editor, seen) = load(&self.dir)?;
        self.editor = editor;
        self.seen = seen;
        Ok(json!({ "reloaded": self.dir.display().to_string(), "dropped_unsaved_changes": dropped }))
    }
}

/// The project for editing, and the folder's fingerprint as loaded.
fn load(dir: &Path) -> Result<(Editor, Option<ecad_project::Fingerprint>), String> {
    let (editor, seen) =
        ecad_editor::session::load_editor_checked(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok((editor, Some(seen)))
}

fn error_reply(id: Value, RpcError(code, message): RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
