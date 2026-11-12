//! MCP tool registry. Each submodule owns a group of tools and exposes:
//! - `definitions() -> Vec<JsonValue>`: MCP tool descriptors (`name`, `description`,
//!   `inputSchema` as a JSON Schema object);
//! - `dispatch(session, name, args) -> Option<ToolResult>`: `None` if the tool name is
//!   not handled by this module.
//!
//! A tool returns MCP content items (see [`text`] / [`image_png`]); errors are plain
//! strings reported to the client with `isError: true`.

use crate::json::JsonValue;
use crate::session::McpSession;

pub mod cozy;
pub mod project;
pub mod render;
pub mod scene;
pub mod voxel;

pub type ToolResult = Result<Vec<JsonValue>, String>;

pub use project::build_project_info_json;

pub fn tool_definitions() -> Vec<JsonValue> {
    let mut defs = Vec::new();
    defs.extend(project::definitions());
    defs.extend(voxel::definitions());
    defs.extend(scene::definitions());
    defs.extend(render::definitions());
    defs.extend(cozy::definitions());
    defs
}

pub fn dispatch_tool_call(session: &mut McpSession, name: &str, args: &JsonValue) -> ToolResult {
    project::dispatch(session, name, args)
        .or_else(|| voxel::dispatch(session, name, args))
        .or_else(|| scene::dispatch(session, name, args))
        .or_else(|| render::dispatch(session, name, args))
        .or_else(|| cozy::dispatch(session, name, args))
        .unwrap_or_else(|| Err(format!("Unknown tool '{name}'")))
}

// ---------------------------------------------------------------------------
// Helpers shared by tool modules
// ---------------------------------------------------------------------------

/// MCP text content item.
pub fn text(s: impl Into<String>) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str("text")),
        ("text", JsonValue::str(s)),
    ])
}

/// MCP image content item (PNG bytes, base64 encoded).
pub fn image_png(png_bytes: &[u8]) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str("image")),
        (
            "data",
            JsonValue::str(crate::json::base64_encode(png_bytes)),
        ),
        ("mimeType", JsonValue::str("image/png")),
    ])
}

/// MCP tool descriptor. `properties` are `(name, json_schema)` pairs.
pub fn tool(
    name: &str,
    description: &str,
    properties: Vec<(&str, JsonValue)>,
    required: &[&str],
) -> JsonValue {
    JsonValue::object(vec![
        ("name", JsonValue::str(name)),
        ("description", JsonValue::str(description)),
        (
            "inputSchema",
            JsonValue::object(vec![
                ("type", JsonValue::str("object")),
                ("properties", JsonValue::object(properties)),
                (
                    "required",
                    JsonValue::array(required.iter().map(|r| JsonValue::str(*r)).collect()),
                ),
            ]),
        ),
    ])
}

/// JSON Schema property of a simple type (`"integer"`, `"number"`, `"string"`,
/// `"boolean"`, `"object"`, `"array"`) with a description.
pub fn prop(ty: &str, description: &str) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str(ty)),
        ("description", JsonValue::str(description)),
    ])
}

/// String property restricted to `values`.
pub fn prop_enum(description: &str, values: &[&str]) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str("string")),
        ("description", JsonValue::str(description)),
        (
            "enum",
            JsonValue::array(values.iter().map(|v| JsonValue::str(*v)).collect()),
        ),
    ])
}

/// `[x, y, z]` integer array property.
pub fn prop_ivec3(description: &str) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str("array")),
        ("description", JsonValue::str(description)),
        (
            "items",
            JsonValue::object(vec![("type", JsonValue::str("integer"))]),
        ),
        ("minItems", JsonValue::int(3)),
        ("maxItems", JsonValue::int(3)),
    ])
}

/// `[x, y, z]` number array property.
pub fn prop_vec3(description: &str) -> JsonValue {
    JsonValue::object(vec![
        ("type", JsonValue::str("array")),
        ("description", JsonValue::str(description)),
        (
            "items",
            JsonValue::object(vec![("type", JsonValue::str("number"))]),
        ),
        ("minItems", JsonValue::int(3)),
        ("maxItems", JsonValue::int(3)),
    ])
}
