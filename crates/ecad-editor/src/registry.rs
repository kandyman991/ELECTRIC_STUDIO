//! The engineering interface's one registry (AI0, D16): every query and operation with its name,
//! typed arguments, JSON schema and class, shared by the app, the CLI and MCP. MCP's tool list
//! is generated from it; nothing defines an operation anywhere else.
//!
//! - **Queries** read: bounded results with stable ids beside tags.
//! - **Operations** change, and never directly: [`lower`] runs one on a scratch copy of the
//!   project and returns the commands it made. Those go into a change set
//!   ([`crate::change`]), which is previewed, approved and applied as one transaction, or, for
//!   trusted automation within the project's scope, applied with `--apply`.
//! - **Classes** decide who may apply what (D18, D42): ordinary edits, numbering and imports may
//!   be in the automation scope; global operations (renumbering) and safety-related changes
//!   never are, and need a reviewed change set.

use crate::Editor;
use crate::script::{find_device, find_page};
use ecad_model::policy::AutomationClass;
use ecad_model::{Command, Project};
use serde_json::{Value, json};

/// What an entry does, for authority (who may apply it) and for clients' hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Class {
    /// Reads only.
    Query,
    /// An ordinary engineering change.
    Edit,
    /// Reserving, retiring and reclaiming identifiers.
    Numbering,
    /// Bringing in outside data (a tag table, a drawing).
    Import,
    /// A global operation (renumbering): never in an automation scope; reviewed change set only.
    Global,
    /// Changes a safety classification: a person's act, never an assistant's weakening one.
    Safety,
}

impl Class {
    /// The automation class trusted automation needs for `--apply` (none for global and
    /// safety operations, which never qualify).
    pub fn automation(self) -> Option<AutomationClass> {
        match self {
            Class::Edit => Some(AutomationClass::Edit),
            Class::Numbering => Some(AutomationClass::Numbering),
            Class::Import => Some(AutomationClass::Import),
            Class::Query | Class::Global | Class::Safety => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    OneOf(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub description: &'static str,
}

const fn text(name: &'static str, description: &'static str) -> Arg {
    Arg { name, kind: Kind::Text, required: true, description }
}
const fn opt(name: &'static str, description: &'static str) -> Arg {
    Arg { name, kind: Kind::Text, required: false, description }
}
const fn number(name: &'static str, description: &'static str) -> Arg {
    Arg { name, kind: Kind::Number, required: true, description }
}

/// One query or operation.
#[derive(Clone, Copy, Debug)]
pub struct Def {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub class: Class,
    pub args: &'static [Arg],
    /// Offered to MCP clients (imports that read local files are not: CLI and app only).
    pub mcp: bool,
}

const IDENTIFIERS: &str = "Identifiers separated by spaces: tag:-K10, tag:-K10..-K19, cable:-W9, wire:W5, terminal:-X3:7..9, address:I1.0..I1.3, core:-W1:3, page:3a";

/// The registry.
pub const DEFS: &[Def] = &[
    // Queries.
    Def {
        name: "project_overview",
        title: "Project overview",
        description: "The project's name, pages, numbers of devices, drawings and wires, and how many check errors and warnings it has. Start here.",
        class: Class::Query,
        args: &[],
        mcp: true,
    },
    Def {
        name: "list_devices",
        title: "List devices",
        description: "Every device (tag, description, products, drawings) and node, or only those on one page. Ids are given beside tags.",
        class: Class::Query,
        args: &[opt("page", "A page name, or #n for the n-th page")],
        mcp: true,
    },
    Def {
        name: "device_details",
        title: "Device details",
        description: "One device: its products, drawings, properties and connection points with their wires.",
        class: Class::Query,
        args: &[text("tag", "The device's tag, like -K1")],
        mcp: true,
    },
    Def {
        name: "list_wires",
        title: "List wires",
        description: "The wire list: number, potential, both ends, cross-section, colour, cable and core, drawn or not. Optionally only one device's.",
        class: Class::Query,
        args: &[opt("device", "Only the wires at this device, like -K1")],
        mcp: true,
    },
    Def {
        name: "run_checks",
        title: "Run checks",
        description: "The project's checks: errors and warnings with their rules and locations.",
        class: Class::Query,
        args: &[],
        mcp: true,
    },
    Def {
        name: "release_check",
        title: "Release check",
        description: "Whether the project can be released: the blocking reasons of the shared release-validation policy. Passing means software consistency, not electrical or functional safety certification.",
        class: Class::Query,
        args: &[],
        mcp: true,
    },
    Def {
        name: "report",
        title: "Report",
        description: "A report as CSV text: bom, wire-list, terminal-diagram, cable-list, plc-io-list, netlist.",
        class: Class::Query,
        args: &[Arg {
            name: "name",
            kind: Kind::OneOf(&ecad_reports::REPORTS),
            required: true,
            description: "The report",
        }],
        mcp: true,
    },
    Def {
        name: "list_symbols",
        title: "List symbols",
        description: "Symbols in the project's library (id, description, category, tag prefix, points), optionally matching a word.",
        class: Class::Query,
        args: &[opt("search", "A word to look for, like coil or terminal")],
        mcp: true,
    },
    Def {
        name: "numbering",
        title: "Numbering",
        description: "The numbering policies, page suffix style, capacities, and reserved and retired identifiers with their notes.",
        class: Class::Query,
        args: &[],
        mcp: true,
    },
    // Operations.
    Def {
        name: "renumber_devices",
        title: "Renumber devices",
        description: "Number every device tag <prefix><n> again per prefix, in drawing order, skipping reserved and retired tags. A global operation: ids, wires and drawings stay; only tags change.",
        class: Class::Global,
        args: &[],
        mcp: true,
    },
    Def {
        name: "renumber_wires",
        title: "Renumber wires",
        description: "Number every wire again from the wire format, page by page. A global operation.",
        class: Class::Global,
        args: &[],
        mcp: true,
    },
    Def {
        name: "renumber_pages",
        title: "Renumber pages",
        description: "Number the pages again in page order; suffixed pages follow their page. A global operation.",
        class: Class::Global,
        args: &[],
        mcp: true,
    },
    Def {
        name: "renumber_terminals",
        title: "Renumber strip",
        description: "Number a strip's drawn terminals 1, 2, 3… in drawing order; undrawn groups keep theirs. A global operation.",
        class: Class::Global,
        args: &[text("strip", "The strip's tag, like -X1")],
        mcp: true,
    },
    Def {
        name: "place_symbol",
        title: "Place symbol",
        description: "Place a symbol on a page at x, y mm (snapped to the 4 mm grid). Without `device` it is a new device with the project's next tag; with `device` another drawing of that device.",
        class: Class::Edit,
        args: &[
            text("symbol", "A symbol id from list_symbols, like iec.coil"),
            text("page", "A page name, or #n for the n-th page"),
            number("x", "mm from the left of the page"),
            number("y", "mm from the bottom of the page"),
            opt("device", "An existing device to add this drawing to, like -K1"),
        ],
        mcp: true,
    },
    Def {
        name: "connect",
        title: "Connect",
        description: "Draw a wire between two free connection points, TAG:POINT as drawn (-S1:14 to -K1:A1).",
        class: Class::Edit,
        args: &[text("from", "One end, like -S1:14"), text("to", "The other end, like -K1:A1")],
        mcp: true,
    },
    Def {
        name: "add_wire",
        title: "Add wire (not drawn)",
        description: "Add a wire between the terminals or nodes two points show, without drawing it.",
        class: Class::Edit,
        args: &[text("from", "One end"), text("to", "The other end"), opt("number", "Its wire number, if any")],
        mcp: true,
    },
    Def {
        name: "delete_wire",
        title: "Delete wire",
        description: "Delete the wire at a connection point; its number is retired.",
        class: Class::Edit,
        args: &[text("at", "A connection point the wire is on, like -K1:A1")],
        mcp: true,
    },
    Def {
        name: "delete_device",
        title: "Delete device",
        description: "Delete a device: its drawings, their wires and its layout place. Its tag and freed wire numbers are retired; nothing is renumbered.",
        class: Class::Edit,
        args: &[text("tag", "The device's tag, like -K1")],
        mcp: true,
    },
    Def {
        name: "edit_device",
        title: "Edit device",
        description: "Give a device a new tag and/or description.",
        class: Class::Edit,
        args: &[
            text("tag", "The device's tag now"),
            opt("new_tag", "Its new tag"),
            opt("description", "Its new description"),
        ],
        mcp: true,
    },
    Def {
        name: "set_plc_channel",
        title: "Set PLC channel",
        description: "Set a PLC channel's address, symbol and comment. Leave a field out to keep it, \"\" clears it.",
        class: Class::Edit,
        args: &[
            text("card", "The PLC card's tag, like -A1"),
            text("channel", "The channel number drawn on the card"),
            opt("address", "Like I0.3, Q1.0, IW256"),
            opt("symbol", "The PLC tag name"),
            opt("comment", "The comment"),
        ],
        mcp: true,
    },
    Def {
        name: "add_page",
        title: "Add page",
        description: "Insert a page after another (or at the end), named by the page policy (after 3 it is 3a): no other page changes.",
        class: Class::Edit,
        args: &[opt("after", "The page to insert after; at the end without it")],
        mcp: true,
    },
    Def {
        name: "reserve",
        title: "Reserve identifiers",
        description: "Keep identifiers or resources free for a planned extension.",
        class: Class::Numbering,
        args: &[text("identifiers", IDENTIFIERS), opt("note", "Why, for what")],
        mcp: true,
    },
    Def {
        name: "unreserve",
        title: "Unreserve identifiers",
        description: "Release reserved identifiers.",
        class: Class::Numbering,
        args: &[text("identifiers", IDENTIFIERS)],
        mcp: true,
    },
    Def {
        name: "retire",
        title: "Retire identifiers",
        description: "Retire identifiers: never allocated again automatically.",
        class: Class::Numbering,
        args: &[text("identifiers", IDENTIFIERS), opt("note", "Why")],
        mcp: true,
    },
    Def {
        name: "reclaim",
        title: "Reclaim identifiers",
        description: "Allow retired identifiers to be used again.",
        class: Class::Numbering,
        args: &[text("identifiers", IDENTIFIERS)],
        mcp: true,
    },
    Def {
        name: "mark_safety_related",
        title: "Mark safety-related",
        description: "Mark a device safety-related (related: yes) or unmark it (no). Unmarking is a person's act: an assistant's proposal can't weaken a safety classification.",
        class: Class::Safety,
        args: &[
            text("tag", "The device's tag"),
            Arg { name: "related", kind: Kind::OneOf(&["yes", "no"]), required: true, description: "yes or no" },
        ],
        mcp: true,
    },
    Def {
        name: "import_tia",
        title: "Import TIA tag table",
        description: "Update PLC channels' symbols and comments from a TIA Portal tag table (.xlsx).",
        class: Class::Import,
        args: &[text("file", "The .xlsx file")],
        mcp: false,
    },
    Def {
        name: "import_dxf",
        title: "Import underlay",
        description: "Show a DXF or DWG drawing behind a page.",
        class: Class::Import,
        args: &[text("page", "The page"), text("file", "The drawing"), opt("units", "mm, cm, m, inch or feet")],
        mcp: false,
    },
    Def {
        name: "remove_underlay",
        title: "Remove underlay",
        description: "Remove the drawing shown behind a page.",
        class: Class::Import,
        args: &[text("page", "The page")],
        mcp: false,
    },
];

pub fn find(name: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.name == name)
}

/// The JSON schema of an entry's arguments (MCP's `inputSchema`).
pub fn schema(def: &Def) -> Value {
    let mut properties = serde_json::Map::new();
    for a in def.args {
        let p = match a.kind {
            Kind::Text => json!({ "type": "string", "description": a.description }),
            Kind::Number => json!({ "type": "number", "description": a.description }),
            Kind::OneOf(options) => json!({ "type": "string", "enum": options, "description": a.description }),
        };
        properties.insert(a.name.to_owned(), p);
    }
    let required: Vec<&str> = def.args.iter().filter(|a| a.required).map(|a| a.name).collect();
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

/// Check arguments against an entry: an object, no unknown fields, every required one, each of
/// its type and in its list.
pub fn validate(def: &Def, args: &Value) -> Result<(), String> {
    let Some(map) = args.as_object() else { return Err("the arguments must be an object".into()) };
    for (key, value) in map {
        let Some(a) = def.args.iter().find(|a| a.name == key) else {
            let known: Vec<String> = def.args.iter().map(|a| format!("`{}`", a.name)).collect();
            let known = if known.is_empty() { "none".to_owned() } else { known.join(", ") };
            return Err(format!("unknown argument `{key}` (arguments: {known})"));
        };
        match a.kind {
            Kind::Text if !value.is_string() => return Err(format!("`{key}` must be a string")),
            Kind::Number if !value.as_f64().is_some_and(f64::is_finite) => {
                return Err(format!("`{key}` must be a number"));
            }
            Kind::OneOf(options) if !value.as_str().is_some_and(|v| options.contains(&v)) => {
                return Err(format!("`{key}` must be one of: {}", options.join(", ")));
            }
            _ => {}
        }
    }
    if let Some(a) = def.args.iter().find(|a| a.required && !map.contains_key(a.name)) {
        return Err(format!("`{}` is needed", a.name));
    }
    Ok(())
}

fn s<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn req<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    s(args, key).ok_or_else(|| format!("`{key}` is needed"))
}
fn num(args: &Value, key: &str) -> Result<f64, String> {
    args.get(key).and_then(Value::as_f64).ok_or_else(|| format!("`{key}` must be a number"))
}

/// Run a query.
pub fn query(p: &Project, name: &str, args: &Value) -> Result<Value, String> {
    let def = find(name).ok_or_else(|| format!("unknown query `{name}`"))?;
    if def.class != Class::Query {
        return Err(format!("`{name}` is an operation: prepare it as a change set"));
    }
    validate(def, args)?;
    match name {
        "project_overview" => Ok(queries::overview(p)),
        "list_devices" => queries::list_devices(p, s(args, "page")),
        "device_details" => queries::device_details(p, req(args, "tag")?),
        "list_wires" => queries::list_wires(p, s(args, "device")),
        "run_checks" => Ok(queries::run_checks(p)),
        "release_check" => Ok(queries::release(p)),
        "report" => {
            let name = req(args, "name")?;
            let csv = ecad_reports::csv(p, name).map_err(|e| e.to_string())?;
            csv.map(Value::String).ok_or_else(|| format!("unknown report `{name}`"))
        }
        "list_symbols" => Ok(queries::list_symbols(p, s(args, "search"))),
        "numbering" => Ok(queries::numbering(p)),
        other => Err(format!("unknown query `{other}`")),
    }
}

/// An operation lowered to commands: what it made, against the project it ran on.
#[derive(Clone, Debug, PartialEq)]
pub struct Lowered {
    pub label: String,
    pub commands: Vec<Command>,
    /// What it reports back (a new tag, a wire number).
    pub result: Value,
}

/// Run an operation on a scratch copy of `p` and return the commands it made; `p` itself is
/// never changed.
pub fn lower(p: &Project, name: &str, args: &Value) -> Result<Lowered, String> {
    let def = find(name).ok_or_else(|| format!("unknown operation `{name}`"))?;
    if def.class == Class::Query {
        return Err(format!("`{name}` is a query"));
    }
    validate(def, args)?;
    let page = p.pages_in_order().first().map(|pg| pg.id).ok_or("the project has no pages")?;
    let mut e = Editor::new(p.clone(), page).map_err(|e| e.to_string())?;
    e.record_commands();
    let result = run(&mut e, name, args)?;
    let commands = e.take_recorded();
    if commands.is_empty() {
        return Err(format!("{}: nothing to change", def.title));
    }
    Ok(Lowered { label: def.title.to_owned(), commands, result })
}

fn renumber(e: &mut Editor, scope: ecad_model::renumber::Scope) -> Result<Value, String> {
    let r = e.preview_renumbering(scope).map_err(|e| e.to_string())?;
    if r.changes.is_empty() {
        return Ok(json!({ "changes": [] }));
    }
    let changes: Vec<Value> = r.changes.iter().map(|c| json!({ "what": c.what, "from": c.from, "to": c.to })).collect();
    e.apply_renumbering(&r).map_err(|e| e.to_string())?;
    Ok(json!({ "changes": changes }))
}

fn identifiers(p: &Project, specs: &str) -> Result<Vec<ecad_model::Identifier>, String> {
    let mut out = Vec::new();
    for spec in specs.split_whitespace() {
        out.extend(p.parse_identifiers(spec)?);
    }
    if out.is_empty() {
        return Err("no identifiers given".into());
    }
    Ok(out)
}

fn run(e: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    use ecad_model::renumber::Scope;
    let model = |r: Result<(), ecad_model::ModelError>| r.map_err(|e| e.to_string());
    match name {
        "renumber_devices" => renumber(e, Scope::Devices),
        "renumber_wires" => renumber(e, Scope::Wires),
        "renumber_pages" => renumber(e, Scope::Pages),
        "renumber_terminals" => {
            let strip = find_device(e.project(), req(args, "strip")?)?;
            renumber(e, Scope::Terminals(strip))
        }
        "place_symbol" => {
            let page = find_page(e.project(), req(args, "page")?)?;
            let f = e.script_place(req(args, "symbol")?, page, num(args, "x")?, num(args, "y")?, s(args, "device"))?;
            let p = e.project();
            let func = p.function(f).map_err(|e| e.to_string())?;
            Ok(json!({ "tag": p.label_of(func), "drawing": f.0.to_string() }))
        }
        "connect" => {
            let id = e.script_connect(req(args, "from")?, req(args, "to")?)?;
            Ok(
                json!({ "wire": e.project().connection(id).ok().and_then(|c| c.wire_number.clone()), "id": id.0.to_string() }),
            )
        }
        "add_wire" => {
            let id = e.script_add_wire(req(args, "from")?, req(args, "to")?, s(args, "number"))?;
            Ok(json!({ "wire": id.0.to_string(), "drawn": false }))
        }
        "delete_wire" => Ok(json!({ "deleted_wire": e.script_disconnect(req(args, "at")?)? })),
        "delete_device" => {
            e.script_delete_device(req(args, "tag")?)?;
            Ok(json!({ "deleted": req(args, "tag")? }))
        }
        "edit_device" => {
            let tag = req(args, "tag")?;
            e.script_edit_device(tag, s(args, "new_tag"), s(args, "description"))?;
            Ok(json!({ "tag": s(args, "new_tag").unwrap_or(tag) }))
        }
        "set_plc_channel" => {
            let (card, channel) = (req(args, "card")?, req(args, "channel")?);
            e.script_set_plc_channel(card, channel, s(args, "address"), s(args, "symbol"), s(args, "comment"))?;
            Ok(json!({ "channel": format!("{card}:{channel}") }))
        }
        "add_page" => {
            let after = match s(args, "after") {
                Some(name) => Some(find_page(e.project(), name)?),
                None => e.project().pages_in_order().last().map(|pg| pg.id),
            };
            let cmds = e.project().insert_page(after, ecad_model::Page::new("", 0));
            let name = match cmds.first() {
                Some(Command::AddPage(pg)) => pg.name.clone(),
                _ => String::new(),
            };
            model(e.run(&format!("Add page {name}"), cmds))?;
            Ok(json!({ "page": name }))
        }
        "reserve" | "retire" => {
            let ids = identifiers(e.project(), req(args, "identifiers")?)?;
            let note = s(args, "note").unwrap_or_default();
            let records = ids.iter().map(|id| ecad_model::Record::new(id.clone(), note)).collect();
            let cmd = if name == "reserve" { Command::Reserve(records) } else { Command::Retire(records) };
            model(e.run(name, vec![cmd]))?;
            Ok(json!({ name: ids.iter().map(|id| e.project().describe_identifier(id)).collect::<Vec<_>>() }))
        }
        "unreserve" | "reclaim" => {
            let ids = identifiers(e.project(), req(args, "identifiers")?)?;
            let labels: Vec<String> = ids.iter().map(|id| e.project().describe_identifier(id)).collect();
            let cmd = if name == "unreserve" { Command::Unreserve(ids) } else { Command::Reclaim(ids) };
            model(e.run(name, vec![cmd]))?;
            Ok(json!({ name: labels }))
        }
        "mark_safety_related" => {
            let device = find_device(e.project(), req(args, "tag")?)?;
            let related = req(args, "related")? == "yes";
            model(e.run("Mark safety-related", vec![Command::SetSafetyRelated { device, related }]))?;
            Ok(json!({ "tag": req(args, "tag")?, "safety_related": related }))
        }
        "import_tia" => {
            let file = req(args, "file")?;
            let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let table = ecad_exchange::tia::read_xlsx(&bytes).map_err(|e| format!("{file}: {e}"))?;
            let import = ecad_exchange::tia::import(e.project(), &table.tags).map_err(|e| e.to_string())?;
            let mut notes = Vec::new();
            for (list, what) in [
                (&table.ignored, "left out"),
                (&import.unknown, "no channel at"),
                (&import.mismatched, "another kind of channel, not applied:"),
                (&import.conflicts, "different names or comments for one address, not applied:"),
                (&import.duplicates, "the same tag twice:"),
            ] {
                notes.extend(list.iter().map(|t| format!("{what} {t}")));
            }
            if let Some(cmd) = import.command {
                model(e.run("Import TIA tag table", vec![cmd]))?;
            }
            Ok(
                json!({ "updated": import.updated, "unchanged": import.unchanged, "other": import.other, "notes": notes }),
            )
        }
        "import_dxf" => {
            let page = find_page(e.project(), req(args, "page")?)?;
            let units = match s(args, "units") {
                None => None,
                Some(u) => Some(
                    ecad_model::underlay::Units::parse(u)
                        .ok_or_else(|| format!("unknown unit `{u}` (use mm, cm, m, inch or feet)"))?,
                ),
            };
            let message = e.import_underlay_file(page, std::path::Path::new(req(args, "file")?), units)?;
            Ok(json!({ "message": message }))
        }
        "remove_underlay" => {
            let page = find_page(e.project(), req(args, "page")?)?;
            e.remove_underlay(page)?;
            Ok(json!({ "removed": true }))
        }
        other => Err(format!("unknown operation `{other}`")),
    }
}

mod queries {
    use crate::script::{find_device, find_page};
    use ecad_model::Project;
    use serde_json::{Value, json};

    pub fn overview(p: &Project) -> Value {
        let checks = ecad_rules::check(p);
        let errors = checks.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
        let pages: Vec<Value> = p
            .pages_in_order()
            .iter()
            .enumerate()
            .map(|(n, page)| json!({ "number": n + 1, "id": page.id.0.to_string(), "name": page.name, "description": page.description }))
            .collect();
        json!({
            "project": p.name,
            "pages": pages,
            "devices": p.devices.len(),
            "symbols_placed": p.functions.len(),
            "wires": p.connections.len(),
            "errors": errors,
            "warnings": checks.len() - errors,
            "safety_related": p.safety_related.iter().filter_map(|d| p.device(*d).ok().map(|d| d.tag.clone())).collect::<Vec<_>>(),
        })
    }

    pub fn numbering(p: &Project) -> Value {
        use ecad_model::numbering::Kind;
        let n = &p.numbering;
        let entries = |m: &std::collections::BTreeMap<ecad_model::Identifier, String>| -> Vec<Value> {
            m.iter().map(|(id, note)| json!({ "identifier": p.describe_identifier(id), "note": note })).collect()
        };
        let policies: serde_json::Map<String, Value> = Kind::ALL
            .iter()
            .map(|k| (k.label().to_owned(), serde_json::to_value(n.policies.strategy(*k)).unwrap_or_default()))
            .collect();
        let capacities: Vec<Value> = n
            .capacities
            .iter()
            .map(|(d, c)| json!({ "device": p.device(*d).map(|d| d.tag.clone()).unwrap_or_default(), "capacity": c }))
            .collect();
        json!({
            "policies": policies,
            "page_suffix": n.policies.page_suffix,
            "capacities": capacities,
            "reserved": entries(&n.reserved),
            "retired": entries(&n.retired),
        })
    }

    fn page_name(p: &Project, page: ecad_model::PageId) -> String {
        p.page(page).map(|pg| pg.name.clone()).unwrap_or_default()
    }

    pub fn list_devices(p: &Project, page: Option<&str>) -> Result<Value, String> {
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
                let parts: Vec<Value> = d.parts.iter().map(|r| part_json(p, r)).collect();
                json!({ "id": d.id.0.to_string(), "tag": d.tag, "description": d.description, "parts": parts, "drawings": drawings, "safety_related": p.safety_related.contains(&d.id) })
            })
            .collect();
        let nodes: Vec<Value> = p
            .nodes
            .values()
            .filter(|n| only.is_none_or(|pg| p.drawings_of_node(n.id).any(|f| f.page == pg)))
            .map(|n| {
                let drawings: Vec<Value> = p
                    .drawings_of_node(n.id)
                    .map(|f| json!({ "symbol": f.symbol.0, "page": page_name(p, f.page), "x": f.position.x, "y": f.position.y }))
                    .collect();
                json!({ "id": n.id.0.to_string(), "kind": n.kind.label(), "name": n.name, "scope": n.scope, "description": n.description, "drawings": drawings })
            })
            .collect();
        Ok(json!({ "devices": list, "nodes": nodes }))
    }

    fn part_json(p: &Project, r: &ecad_model::PartRef) -> Value {
        let part = p.parts.get(&r.product);
        json!({
            "product": r.product.0.to_string(),
            "number": part.map(|x| x.number.as_str()),
            "manufacturer": part.map(|x| x.manufacturer.as_str()),
            "quantity": r.quantity,
        })
    }

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
                json!({ "point": p.designation(f, &cp.name), "wire": wire })
            })
            .collect()
    }

    pub fn device_details(p: &Project, tag: &str) -> Result<Value, String> {
        if p.device_by_tag(tag).is_none() && p.functions.values().any(|f| f.node.is_some() && p.label_of(f) == tag) {
            let drawings: Vec<Value> = p
                .functions
                .values()
                .filter(|f| f.node.is_some() && p.label_of(f) == tag)
                .map(|f| json!({ "symbol": f.symbol.0, "page": page_name(p, f.page), "x": f.position.x, "y": f.position.y, "points": points_of(p, f) }))
                .collect();
            return Ok(json!({ "tag": tag, "node": true, "drawings": drawings }));
        }
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
                    "properties": p.properties_of(f),
                    "points": points_of(p, f),
                })
            })
            .collect();
        let parts: Vec<Value> = d.parts.iter().map(|r| part_json(p, r)).collect();
        Ok(
            json!({ "id": d.id.0.to_string(), "tag": d.tag, "description": d.description, "parts": parts, "drawings": drawings, "safety_related": p.safety_related.contains(&id) }),
        )
    }

    pub fn list_wires(p: &Project, device: Option<&str>) -> Result<Value, String> {
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
                    "drawn": r.route != "not drawn",
                })
            })
            .collect();
        Ok(json!({ "wires": wires }))
    }

    pub fn run_checks(p: &Project) -> Value {
        let checks = ecad_rules::check(p);
        let errors = checks.iter().filter(|c| c.severity == ecad_rules::Severity::Error).count();
        let list: Vec<Value> = checks
            .iter()
            .map(|c| json!({ "rule": c.rule, "severity": c.severity, "message": c.message, "location": c.location }))
            .collect();
        json!({ "errors": errors, "warnings": checks.len() - errors, "checks": list })
    }

    pub fn release(p: &Project) -> Value {
        let r = ecad_rules::release::validate(p);
        json!({
            "releasable": r.releasable(),
            "blocking": r.blocking.iter().map(|b| json!({ "rule": b.rule, "message": b.message })).collect::<Vec<_>>(),
            "note": ecad_rules::release::NOTE,
        })
    }

    pub fn list_symbols(p: &Project, search: Option<&str>) -> Value {
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
}
