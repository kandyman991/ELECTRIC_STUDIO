//! Explicit identity-preserving construction operations. Each entry decodes one domain record
//! and constructs one named forward command. There is deliberately no arbitrary Command,
//! history-inverse, approval, automation-policy or filesystem execution endpoint.

use super::{Arg, Class, Def, Kind};
use ecad_model::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct RemovePage {
    id: PageId,
}

#[derive(Deserialize)]
struct DrawingProperties {
    id: FunctionId,
    properties: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct MirrorDrawing {
    id: FunctionId,
    to: bool,
}

#[derive(Deserialize)]
struct WireBend {
    id: ConnectionId,
    bend: Option<f64>,
}

#[derive(Deserialize)]
struct WireNumber {
    id: ConnectionId,
    number: Option<String>,
}

#[derive(Deserialize)]
struct WireSpecification {
    id: ConnectionId,
    spec: WireSpec,
}

#[derive(Deserialize)]
struct DeviceParts {
    id: DeviceId,
    parts: Vec<PartRef>,
}

#[derive(Deserialize)]
struct MountingPlate {
    page: PageId,
    panel: Panel,
}

#[derive(Deserialize)]
struct ProjectTitle {
    name: String,
    info: ProjectInfo,
}

pub(super) fn schema(kind: &str) -> Value {
    // All schemas are static, not supplied by the caller.
    serde_json::from_str(kind).unwrap_or(Value::Null)
}

/// Check structured input recursively before domain deserialization. In particular, do
/// not silently accept unknown properties that a reviewer might think were applied.
pub(super) fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let bad = || format!("invalid structured argument at {path}");
    if let Some(alternatives) = schema["anyOf"].as_array() {
        return if alternatives.iter().any(|s| validate(s, value, path).is_ok()) { Ok(()) } else { Err(bad()) };
    }
    if let Some(values) = schema["enum"].as_array()
        && !values.contains(value)
    {
        return Err(bad());
    }
    match schema["type"].as_str() {
        Some("object") => {
            let map = value.as_object().ok_or_else(bad)?;
            if let Some(required) = schema["required"].as_array() {
                for key in required.iter().filter_map(Value::as_str) {
                    if !map.contains_key(key) {
                        return Err(format!("{path}.{key} is required"));
                    }
                }
            }
            for (key, value) in map {
                let child = schema["properties"]
                    .get(key)
                    .or_else(|| schema["additionalProperties"].as_object().map(|_| &schema["additionalProperties"]));
                let child = child.ok_or_else(|| format!("unknown argument {path}.{key}"))?;
                validate(child, value, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        Some("array") => {
            let array = value.as_array().ok_or_else(bad)?;
            if array.len() > 10_000 {
                return Err(format!("{path} has too many items"));
            }
            for item in array {
                validate(&schema["items"], item, path)?;
            }
            Ok(())
        }
        Some("string") if value.is_string() => Ok(()),
        Some("number") if value.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Some("integer") if value.as_u64().is_some() => Ok(()),
        Some("boolean") if value.is_boolean() => Ok(()),
        Some("null") if value.is_null() => Ok(()),
        _ => Err(bad()),
    }
}

fn decode<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args["record"].clone()).map_err(|e| format!("invalid record: {e}"))
}

pub(super) fn command(p: &Project, name: &str, args: &Value) -> Result<Command, String> {
    Ok(match name {
        "add_page_exact" => {
            let record: Page = decode(args)?;
            if record.order == 0 || p.pages.values().any(|page| page.order == record.order) {
                return Err("page order must be a new nonzero sparse key".into());
            }
            Command::AddPage(record)
        },
        "remove_empty_page" => {
            let record: RemovePage = decode(args)?;
            if p.pages.len() <= 1 {
                return Err("the last page cannot be removed".into());
            }
            Command::RemovePage(record.id)
        },
        "add_device_exact" => {
            let record: Device = decode(args)?;
            if record.id.is_none() {
                return Err("device id cannot be nil".into());
            }
            Command::AddDevice(record)
        },
        "add_node_exact" => {
            let record: NodeEntry = decode(args)?;
            Command::AddNode(record)
        },
        "describe_node_exact" => {
            let record: NodeEntry = decode(args)?;
            let old = p.nodes.get(&record.id).ok_or("node does not exist")?;
            if old.kind != record.kind || old.name != record.name || old.scope != record.scope {
                return Err("only a node description can be changed here".into());
            }
            Command::SetNode(record)
        },
        "place_drawing_exact" => {
            let record: Function = decode(args)?;
            Command::Place(record)
        },
        "connect_drawing_exact" => {
            let record: Connection = decode(args)?;
            Command::Connect(record)
        },
        "set_drawing_properties" => {
            let record: DrawingProperties = decode(args)?;
            Command::SetProperties { id: record.id, properties: record.properties }
        },
        "mirror_drawing_exact" => {
            let record: MirrorDrawing = decode(args)?;
            Command::Mirror { id: record.id, to: record.to }
        },
        "set_wire_bend_exact" => {
            let record: WireBend = decode(args)?;
            Command::SetWireBend { id: record.id, bend: record.bend }
        },
        "set_wire_number_exact" => {
            let record: WireNumber = decode(args)?;
            Command::SetWireNumber { id: record.id, number: record.number }
        },
        "set_wire_spec_exact" => {
            let record: WireSpecification = decode(args)?;
            Command::SetWireSpec { id: record.id, spec: record.spec }
        },
        "add_cable_exact" => {
            let record: Cable = decode(args)?;
            Command::AddCable(record)
        },
        "add_location_box" => {
            let record: LocationBox = decode(args)?;
            Command::AddBox(record)
        },
        "add_symbol_definition" => {
            let record: SymbolDef = decode(args)?;
            if record.id.0.is_empty()
                || !record.id.0.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || record.id.0 == "." || record.id.0 == ".."
            {
                return Err("invalid symbol id".into());
            }
            let points: std::collections::BTreeSet<_> = record.connection_points.iter().map(|p| &p.name).collect();
            if points.len() != record.connection_points.len()
                || points.iter().any(|name| name.trim().is_empty())
                || record.bridges.iter().any(|group| group.iter().any(|name| !points.contains(name)))
            {
                return Err("symbol connection points or bridge references are invalid".into());
            }
            Command::AddSymbol(record)
        },
        "add_catalogue_part" => {
            let record: Part = decode(args)?;
            Command::AddPart(record)
        },
        "assign_device_parts" => {
            let record: DeviceParts = decode(args)?;
            if !p.device(record.id).map_err(|e| e.to_string())?.parts.is_empty() {
                return Err("existing assigned parts need explicit replacement and terminal reconciliation".into());
            }
            Command::SetDeviceParts { id: record.id, parts: record.parts }
        },
        "set_mounting_plate" => {
            let record: MountingPlate = decode(args)?;
            Command::SetPanel { page: record.page, panel: Some(record.panel) }
        },
        "add_panel_item" => {
            let record: PanelItem = decode(args)?;
            Command::AddPanelItem(record)
        },
        "set_sheet_definition" => {
            let record: Sheet = decode(args)?;
            Command::SetSheet(record)
        },
        "set_project_title" => {
            let record: ProjectTitle = decode(args)?;
            Command::SetProjectInfo { name: record.name, info: record.info }
        },
        _ => return Err(format!("unknown construction operation `{name}`")),
    })
}

pub(super) const ADD_PAGE_EXACT: Def = Def {
    name: "add_page_exact",
    title: "Add page with explicit identity",
    description: "Add a page with an explicit unique id and sparse order key.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"name":{"type":"string"},"description":{"type":"string"},"subtitle":{"type":"string"},"order":{"type":"integer","minimum":0}},"required":["id","name","order"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const REMOVE_EMPTY_PAGE: Def = Def {
    name: "remove_empty_page",
    title: "Remove empty page",
    description: "Remove an empty page by id; never remove the last page.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"}},"required":["id"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_DEVICE_EXACT: Def = Def {
    name: "add_device_exact",
    title: "Add device with explicit identity",
    description: "Add an engineering device with explicit id and tag; no existing device is changed.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"tag":{"type":"string"},"description":{"type":"string"},"parts":{"type":"array","items":{"type":"object","properties":{"product":{"type":"string","format":"uuid"},"quantity":{"type":"integer","minimum":0},"per_strip":{"type":"boolean"},"only":{"type":"string"}},"required":["product"],"additionalProperties":false}}},"required":["id","tag"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_NODE_EXACT: Def = Def {
    name: "add_node_exact",
    title: "Add electrical node",
    description: "Add a junction, potential or interruption with an explicit id and scope.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"kind":{"type":"string","enum":["junction","potential","interruption"]},"name":{"type":"string"},"scope":{"type":"string"},"description":{"type":"string"}},"required":["id","kind"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const DESCRIBE_NODE_EXACT: Def = Def {
    name: "describe_node_exact",
    title: "Describe electrical node",
    description: "Update a node description without changing its name, scope, kind or binding.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"kind":{"type":"string","enum":["junction","potential","interruption"]},"name":{"type":"string"},"scope":{"type":"string"},"description":{"type":"string"}},"required":["id","kind"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const PLACE_DRAWING_EXACT: Def = Def {
    name: "place_drawing_exact",
    title: "Place drawing with explicit bindings",
    description: "Place a drawing by symbol, page, device or node and terminal bindings, with explicit coordinates.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"device":{"type":"string","format":"uuid"},"node":{"type":"string","format":"uuid"},"symbol":{"type":"string"},"page":{"type":"string","format":"uuid"},"position":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"rotation":{"type":"string","enum":["R0","R90","R180","R270"]},"mirrored":{"type":"boolean"},"designations":{"type":"object","additionalProperties":{"type":"string"}},"terminals":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},"properties":{"type":"object","additionalProperties":{"type":"string"}}},"required":["id","symbol","page","position"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const CONNECT_DRAWING_EXACT: Def = Def {
    name: "connect_drawing_exact",
    title: "Connect drawing points",
    description: "Connect two explicit drawing ids and points, with optional wire number, specification and bend.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"from":{"type":"object","properties":{"function":{"type":"string","format":"uuid"},"point":{"type":"string"}},"required":["function","point"],"additionalProperties":false},"to":{"type":"object","properties":{"function":{"type":"string","format":"uuid"},"point":{"type":"string"}},"required":["function","point"],"additionalProperties":false},"wire_number":{"type":"string"},"cross_section":{"type":"string"},"colour":{"type":"string"},"cable":{"type":"object","properties":{"cable":{"type":"string","format":"uuid"},"core":{"type":"string"}},"required":["cable","core"],"additionalProperties":false},"bend":{"type":"number"}},"required":["id","from","to"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_DRAWING_PROPERTIES: Def = Def {
    name: "set_drawing_properties",
    title: "Set drawing properties",
    description: "Set the complete property map of a drawing, including PLC address, symbol and comment.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"properties":{"type":"object","additionalProperties":{"type":"string"}}},"required":["id","properties"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const MIRROR_DRAWING_EXACT: Def = Def {
    name: "mirror_drawing_exact",
    title: "Mirror drawing",
    description: "Set a drawing's mirrored state explicitly.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"to":{"type":"boolean"}},"required":["id","to"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_WIRE_BEND_EXACT: Def = Def {
    name: "set_wire_bend_exact",
    title: "Set wire bend",
    description: "Set an explicit wire bend in millimetres, or null for automatic.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"bend":{"anyOf":[{"type":"number"},{"type":"null"}]}},"required":["id","bend"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_WIRE_NUMBER_EXACT: Def = Def {
    name: "set_wire_number_exact",
    title: "Set wire number",
    description: "Assign or clear an explicit wire number, subject to identifier checks.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"number":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["id","number"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_WIRE_SPEC_EXACT: Def = Def {
    name: "set_wire_spec_exact",
    title: "Set wire specification",
    description: "Set the cross-section, colour and optional cable core of a wire.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"spec":{"type":"object","properties":{"cross_section":{"type":"string"},"colour":{"type":"string"},"cable":{"type":"object","properties":{"cable":{"type":"string","format":"uuid"},"core":{"type":"string"}},"required":["cable","core"],"additionalProperties":false}},"required":[],"additionalProperties":false}},"required":["id","spec"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_CABLE_EXACT: Def = Def {
    name: "add_cable_exact",
    title: "Add cable",
    description: "Add a cable with explicit identity, tag and core count.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"tag":{"type":"string"},"kind":{"type":"string"},"cores":{"type":"integer","minimum":0},"shielded":{"type":"boolean"}},"required":["id","tag"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_LOCATION_BOX: Def = Def {
    name: "add_location_box",
    title: "Add location box",
    description: "Add a labelled location box to a page.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"page":{"type":"string","format":"uuid"},"min":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"max":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"label":{"type":"string"}},"required":["id","page","min","max"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_SYMBOL_DEFINITION: Def = Def {
    name: "add_symbol_definition",
    title: "Add custom symbol definition",
    description: "Add a new custom symbol, with explicit graphics and named connection points. Does not replace an existing symbol.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string"},"description":{"type":"string"},"tag_prefix":{"type":"string"},"role":{"anyOf":[{"type":"object","properties":{"type":{"type":"string","enum":["Single"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["Main"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["Potential"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["Interruption"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["Terminal"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["Contact"]},"kind":{"type":"string"}},"required":["type","kind"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["PlcChannel"]},"kind":{"type":"string"}},"required":["type","kind"],"additionalProperties":false}]},"bridged":{"type":"boolean"},"bridges":{"type":"array","items":{"type":"array","items":{"type":"string"}}},"hide_tag":{"type":"boolean"},"hide_point_labels":{"type":"boolean"},"category":{"type":"string"},"graphics":{"type":"array","items":{"anyOf":[{"type":"object","properties":{"Line":{"type":"object","properties":{"a":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"b":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}},"required":["a","b"],"additionalProperties":false}},"required":["Line"],"additionalProperties":false},{"type":"object","properties":{"Dashed":{"type":"object","properties":{"a":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"b":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}},"required":["a","b"],"additionalProperties":false}},"required":["Dashed"],"additionalProperties":false},{"type":"object","properties":{"Circle":{"type":"object","properties":{"center":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"radius":{"type":"number"}},"required":["center","radius"],"additionalProperties":false}},"required":["Circle"],"additionalProperties":false},{"type":"object","properties":{"Text":{"type":"object","properties":{"at":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"height":{"type":"number"},"value":{"type":"string"}},"required":["at","height","value"],"additionalProperties":false}},"required":["Text"],"additionalProperties":false}]}},"connection_points":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"at":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"leaves":{"type":"string","enum":["Up","Down","Left","Right"]}},"required":["name","at","leaves"],"additionalProperties":false}}},"required":["id","description","graphics","connection_points"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_CATALOGUE_PART: Def = Def {
    name: "add_catalogue_part",
    title: "Add catalogue part",
    description: "Add a new project catalogue part with identity and technical data.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"number":{"type":"string"},"manufacturer":{"type":"string"},"description":{"type":"string"},"code":{"type":"string"},"contacts":{"type":"array","items":{"type":"string"}},"width":{"type":"number"},"height":{"type":"number"},"depth":{"type":"number"}},"required":["id","number"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ASSIGN_DEVICE_PARTS: Def = Def {
    name: "assign_device_parts",
    title: "Assign initial device parts",
    description: "Assign a device's initial catalogue references; replacing assigned parts requires explicit reconciliation.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"parts":{"type":"array","items":{"type":"object","properties":{"product":{"type":"string","format":"uuid"},"quantity":{"type":"integer","minimum":0},"per_strip":{"type":"boolean"},"only":{"type":"string"}},"required":["product"],"additionalProperties":false}}},"required":["id","parts"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_MOUNTING_PLATE: Def = Def {
    name: "set_mounting_plate",
    title: "Set mounting plate",
    description: "Set the dimensions of a page's mounting plate.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"page":{"type":"string","format":"uuid"},"panel":{"type":"object","properties":{"width":{"type":"number"},"height":{"type":"number"}},"required":["width","height"],"additionalProperties":false}},"required":["page","panel"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const ADD_PANEL_ITEM: Def = Def {
    name: "add_panel_item",
    title: "Add panel item",
    description: "Add a rail, duct or device footprint to a mounting plate.",
    class: Class::Edit,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"page":{"type":"string","format":"uuid"},"kind":{"anyOf":[{"type":"object","properties":{"type":{"type":"string","enum":["rail"]}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["duct"]},"width":{"type":"number"},"depth":{"type":"number"}},"required":["type","width","depth"],"additionalProperties":false},{"type":"object","properties":{"type":{"type":"string","enum":["device"]},"device":{"type":"string","format":"uuid"}},"required":["type","device"],"additionalProperties":false}]},"at":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"length":{"type":"number"},"vertical":{"type":"boolean"},"part":{"type":"string"}},"required":["id","page","kind","at"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_SHEET_DEFINITION: Def = Def {
    name: "set_sheet_definition",
    title: "Set sheet definition",
    description: "Set project-wide sheet dimensions and frame numbering.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"origin":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false},"width":{"type":"number"},"height":{"type":"number"},"columns":{"type":"integer","minimum":0},"first_column":{"type":"integer","minimum":0},"rows":{"type":"integer","minimum":0}},"required":["origin","width","height","columns"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};

pub(super) const SET_PROJECT_TITLE: Def = Def {
    name: "set_project_title",
    title: "Set project title and metadata",
    description: "Set project-wide title block and revision metadata.",
    class: Class::Global,
    args: &[Arg {
        name: "record",
        kind: Kind::Structured(r#"{"type":"object","properties":{"name":{"type":"string"},"info":{"type":"object","properties":{"customer":{"type":"string"},"plant":{"type":"string"},"code":{"type":"string"},"designer":{"type":"string"},"date":{"type":"string"},"revisions":{"type":"array","items":{"type":"object","properties":{"index":{"type":"string"},"change":{"type":"string"},"date":{"type":"string"},"by":{"type":"string"}},"required":[],"additionalProperties":false}}},"required":[],"additionalProperties":false}},"required":["name","info"],"additionalProperties":false}"#),
        required: true,
        description: "Typed engineering record. Ids are UUIDs; references must resolve in the proposal's project.",
    }],
    mcp: true,
};
