//! An MCP (Model Context Protocol) server for one Electric Studio project.
//!
//! It speaks JSON-RPC 2.0 over stdin and stdout, one message per line, as MCP's stdio
//! transport does. Read tools describe the project (pages, devices, wires, checks, reports,
//! the symbol library). With `--write`, edit tools change it through the editor's commands,
//! one undo step each; changes stay in memory until the `save` tool writes the project folder.
//! Without `--write` the edit tools aren't offered at all.

mod tools;

use ecad_editor::Editor;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// MCP versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
This server holds one Electric Studio electrical schematic project. Devices have tags like -K1; \
a connection point is written TAG:POINT as drawn (-K1:A1, -X1:3). Pages are named as in the \
project (list them with project_overview). Start with project_overview and run_checks. Edit tools, \
when offered, change the project in memory with one undo step each; nothing reaches the project \
folder until you call save. Positions are mm on the page, snapped to a 4 mm grid.";

/// One project, open for an MCP client.
pub struct Server {
    dir: PathBuf,
    editor: Editor,
    writable: bool,
}

/// A JSON-RPC error: code and message.
struct RpcError(i64, String);

const INVALID_PARAMS: i64 = -32602;

impl Server {
    /// Open the project in `dir`. With `writable` false the edit tools aren't offered.
    pub fn open(dir: &Path, writable: bool) -> Result<Server, String> {
        let project = ecad_project::load(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let page = project.pages_in_order().first().map(|p| p.id).ok_or("the project has no pages")?;
        let editor = Editor::new(project, page).map_err(|e| e.to_string())?;
        Ok(Server { dir: dir.to_owned(), editor, writable })
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
            Err(e) => error_reply(Value::Null, RpcError(-32700, format!("parse error: {e}"))),
        };
        Some(reply.to_string())
    }

    /// Answer one JSON-RPC message: `None` for a notification or a response.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        if message.is_array() {
            return Some(error_reply(Value::Null, RpcError(-32600, "batches aren't supported".into())));
        }
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to something we never send, or not a request at all.
            return id.map(|id| error_reply(id, RpcError(-32600, "not a request".into())));
        };
        // Notifications (no id) get no answer: `notifications/initialized`, cancellations.
        let id = id?;
        let params = message.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list(self.writable) })),
            "tools/call" => self.call(&params),
            other => Err(RpcError(-32601, format!("unknown method `{other}`"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => error_reply(id, e),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "electric-studio", "title": "Electric Studio", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(Value::as_str).ok_or(RpcError(INVALID_PARAMS, "no tool name".into()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        if !tools::list(self.writable).iter().any(|t| t["name"] == name) {
            let why = if tools::list(true).iter().any(|t| t["name"] == name) {
                format!("`{name}` edits the project; start the server with --write to allow it")
            } else {
                format!("unknown tool `{name}`")
            };
            return Err(RpcError(INVALID_PARAMS, why));
        }
        // A tool's own failure is a result the model can read and act on, not a protocol error.
        Ok(match tools::call(self, name, &args) {
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
}

fn error_reply(id: Value, RpcError(code, message): RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
