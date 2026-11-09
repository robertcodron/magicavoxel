//! MCP tools: project lifecycle (new/open/save/export/info), console commands and undo/redo.

use std::path::{Path, PathBuf};

use mv_core::{CommandOutcome, EditorProject, NodeKind};
use mv_format::GltfOptions;

use crate::json::JsonValue;
use crate::session::McpSession;
use crate::tools::{prop, prop_enum, text, tool, ToolResult};

pub fn definitions() -> Vec<JsonValue> {
    vec![
        tool(
            "project_new",
            "Replace the current project with a new empty one. Canvas axes are MagicaVoxel Z-up: X = right, Y = depth, Z = up (ground is z = 0). Each axis is 1..=256 voxels (default 40). Unsaved work in the current project is discarded.",
            vec![
                ("size_x", prop("integer", "Canvas width along X (1..=256, default 40)")),
                ("size_y", prop("integer", "Canvas depth along Y (1..=256, default 40)")),
                ("size_z", prop("integer", "Canvas height along Z, up (1..=256, default 40)")),
                ("name", prop("string", "Optional project name")),
            ],
            &[],
        ),
        tool(
            "project_open",
            "Load a MagicaVoxel .vox file from disk, replacing the current project (scene, palette, materials).",
            vec![("path", prop("string", "Path of the .vox file"))],
            &["path"],
        ),
        tool(
            "project_save",
            "Save the project as a MagicaVoxel .vox file. Uses `path` if given, otherwise the file the project was opened/saved from.",
            vec![("path", prop("string", "Destination .vox path (optional if already saved/opened)"))],
            &[],
        ),
        tool(
            "project_export",
            "Export to another format. vox saves the project; glb/gltf (3D mesh, scale = voxel size), obj (greedy-meshed) and ply (point cloud) export the whole visible scene; slice_png exports the edited model as a stacked PNG slice strip.",
            vec![
                (
                    "format",
                    prop_enum(
                        "Export format",
                        &["vox", "glb", "gltf", "obj", "ply", "slice_png"],
                    ),
                ),
                ("path", prop("string", "Destination file path")),
                ("scale", prop("number", "glTF/GLB voxel size in meters (default 1.0)")),
            ],
            &["format", "path"],
        ),
        tool(
            "project_info",
            "Get a JSON overview of the project: edited model size and voxel count, palette selection, scene nodes (with world bounds), layers and animation state.",
            vec![],
            &[],
        ),
        tool(
            "console_command",
            "Run any MagicaVoxel console command; this gives access to every console command (size, flip, rot, mir, dil, ero, flood, noise, fill, zero, inv, scale, repeat, loop, xs <shader>, pal ..., and more) as documented in MagicaVoxel's console reference. Operates on the edited model. Example: `size 20 20 20`, `mir x`, `xs wave`.",
            vec![("command", prop("string", "Console command line, e.g. \"dil 1\""))],
            &["command"],
        ),
        tool(
            "history",
            "Undo or redo. scope `model` (default) steps the voxel/palette/material history of the edited model; scope `world` steps the scene-level history (object moves, creation, deletion).",
            vec![
                ("action", prop_enum("undo or redo", &["undo", "redo"])),
                ("scope", prop_enum("model (default) or world", &["model", "world"])),
            ],
            &["action"],
        ),
    ]
}

pub fn dispatch(session: &mut McpSession, name: &str, args: &JsonValue) -> Option<ToolResult> {
    Some(match name {
        "project_new" => project_new(session, args),
        "project_open" => project_open(session, args),
        "project_save" => project_save(session, args),
        "project_export" => project_export(session, args),
        "project_info" => Ok(vec![text(
            build_project_info_json(session).to_json_string(),
        )]),
        "console_command" => console_command(session, args),
        "history" => history(session, args),
        _ => return None,
    })
}

fn dim_arg(args: &JsonValue, key: &str) -> Result<u32, String> {
    match args.get(key) {
        None | Some(JsonValue::Null) => Ok(40),
        Some(v) => {
            let n = v
                .as_i64()
                .ok_or_else(|| format!("'{key}' must be an integer"))?;
            Ok(n.clamp(1, 256) as u32)
        }
    }
}

fn path_arg<'a>(args: &'a JsonValue, key: &str) -> Result<&'a str, String> {
    match args.get_str(key) {
        Some(p) if !p.trim().is_empty() => Ok(p),
        _ => Err(format!("Missing required string argument '{key}'")),
    }
}

fn project_new(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let (sx, sy, sz) = (
        dim_arg(args, "size_x")?,
        dim_arg(args, "size_y")?,
        dim_arg(args, "size_z")?,
    );
    let mut project = EditorProject::new_empty(sx, sy, sz);
    if let Some(name) = args.get_str("name") {
        project.name = name.to_string();
    }
    session.project = project;
    session.current_file = None;
    session.camera.recenter(&session.project.grid);
    Ok(vec![text(format!(
        "New project created: {sx}x{sy}x{sz} (X right, Y depth, Z up; ground z=0)."
    ))])
}

fn project_open(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let path = path_arg(args, "path")?;
    let project =
        mv_format::load_vox_file(path).map_err(|e| format!("Cannot open '{path}': {e}"))?;
    session.project = project;
    session.current_file = Some(PathBuf::from(path));
    session.camera.recenter(&session.project.grid);
    let d = session.project.grid.dimensions();
    Ok(vec![text(format!(
        "Opened '{path}': edited model {}x{}x{}, {} model(s) in scene.",
        d.x,
        d.y,
        d.z,
        session.project.scene.models.len()
    ))])
}

fn project_save(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let path: PathBuf = match args.get_str("path").filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => session
            .current_file
            .clone()
            .ok_or_else(|| "No 'path' given and the project has no file yet".to_string())?,
    };
    mv_format::save_vox_file(&session.project, &path)
        .map_err(|e| format!("Cannot save '{}': {e}", path.display()))?;
    session.project.dirty = false;
    let msg = format!("Saved project to '{}'.", path.display());
    session.current_file = Some(path);
    Ok(vec![text(msg)])
}

fn project_export(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let format = args
        .get_str("format")
        .ok_or_else(|| "Missing required argument 'format'".to_string())?
        .to_ascii_lowercase();
    let mut path = PathBuf::from(path_arg(args, "path")?);
    let scale = args.get_f32("scale").unwrap_or(1.0);
    if !(scale.is_finite() && scale > 0.0) {
        return Err("'scale' must be a positive number".into());
    }
    let is_glb = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    let p = &session.project;
    let detail = match format.as_str() {
        "vox" => {
            mv_format::save_vox_file(p, &path).map_err(|e| e.to_string())?;
            String::new()
        }
        "glb" | "gltf" => {
            if format == "glb" && !is_glb(&path) {
                path.set_extension("glb");
            } else if format == "gltf" && is_glb(&path) {
                path.set_extension("gltf");
            }
            let opts = GltfOptions {
                scale,
                include_hidden: false,
            };
            mv_format::export_gltf(p, &path, &opts).map_err(|e| e.to_string())?;
            String::new()
        }
        "obj" => {
            let n = mv_format::export_obj_greedy(p, &path).map_err(|e| e.to_string())?;
            format!(" ({n} quads)")
        }
        "ply" => {
            let n = mv_format::export_ply_point_cloud(p, &path).map_err(|e| e.to_string())?;
            format!(" ({n} points)")
        }
        "slice_png" => {
            mv_format::export_slice_png(p, &path).map_err(|e| e.to_string())?;
            String::new()
        }
        other => {
            return Err(format!(
                "Unknown format '{other}' (expected vox, glb, gltf, obj, ply, slice_png)"
            ))
        }
    };
    Ok(vec![text(format!(
        "Exported {format} to '{}'{detail}.",
        path.display()
    ))])
}

fn console_command(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let command = args
        .get_str("command")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "Missing required string argument 'command'".to_string())?;
    match session.project.run_command(command)? {
        CommandOutcome::Message(m) => Ok(vec![text(if m.is_empty() { "OK".to_string() } else { m })]),
        CommandOutcome::ExternalShader(inv) => {
            let msg = session.run_external_shader(&inv)?;
            Ok(vec![text(if msg.is_empty() { "OK".to_string() } else { msg })])
        }
        CommandOutcome::ExportRequested { format, .. } => Err(format!(
            "Console export ('{format}') is not available over MCP; use the `project_export` tool instead"
        )),
    }
}

fn history(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or_else(|| "Missing required argument 'action' (undo or redo)".to_string())?;
    let scope = args.get_str("scope").unwrap_or("model");
    let p = &mut session.project;
    let done = match (action, scope) {
        ("undo", "model") => p.undo(),
        ("redo", "model") => p.redo(),
        ("undo", "world") => p.world_undo(),
        ("redo", "world") => p.world_redo(),
        _ => return Err("'action' must be undo|redo and 'scope' model|world".into()),
    };
    Ok(vec![text(if done {
        format!("{action} ({scope}) done.")
    } else {
        format!("Nothing to {action} ({scope}).")
    })])
}

/// JSON overview of the session (resource `magicavoxel://project/info`).
pub fn build_project_info_json(session: &McpSession) -> JsonValue {
    let p = &session.project;
    let d = p.grid.dimensions();
    let scene = &p.scene;

    let nodes: Vec<JsonValue> = scene
        .nodes()
        .map(|(id, n)| {
            let mut f: Vec<(&str, JsonValue)> = vec![
                ("id", JsonValue::uint(id)),
                (
                    "name",
                    n.name
                        .as_deref()
                        .map(JsonValue::str)
                        .unwrap_or(JsonValue::Null),
                ),
                (
                    "kind",
                    JsonValue::str(match n.kind {
                        NodeKind::Group { .. } => "group",
                        NodeKind::Shape { .. } => "shape",
                    }),
                ),
                (
                    "model",
                    scene
                        .shape_model(id)
                        .map(JsonValue::uint)
                        .unwrap_or(JsonValue::Null),
                ),
                (
                    "parent",
                    n.parent.map(JsonValue::uint).unwrap_or(JsonValue::Null),
                ),
                ("hidden", JsonValue::bool(n.hidden)),
                ("layer", JsonValue::uint(n.layer_id)),
            ];
            if let Some((lo, hi)) = scene.world_bounds(id) {
                let v = |a: glam::IVec3| {
                    JsonValue::array(vec![
                        JsonValue::int(a.x as i64),
                        JsonValue::int(a.y as i64),
                        JsonValue::int(a.z as i64),
                    ])
                };
                f.push(("world_min", v(lo)));
                f.push(("world_max", v(hi)));
            }
            JsonValue::object(f)
        })
        .collect();

    let layers: Vec<JsonValue> = scene
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            JsonValue::object(vec![
                ("index", JsonValue::uint(i)),
                ("name", JsonValue::str(l.name.clone())),
                ("hidden", JsonValue::bool(l.hidden)),
                (
                    "color",
                    JsonValue::array(l.color.iter().map(|c| JsonValue::int(*c as i64)).collect()),
                ),
            ])
        })
        .collect();

    JsonValue::object(vec![
        ("name", JsonValue::str(p.name.clone())),
        (
            "file",
            session
                .current_file
                .as_ref()
                .map(|f| JsonValue::str(f.display().to_string()))
                .unwrap_or(JsonValue::Null),
        ),
        (
            "edited_model",
            JsonValue::object(vec![
                (
                    "size",
                    JsonValue::array(vec![
                        JsonValue::uint(d.x as usize),
                        JsonValue::uint(d.y as usize),
                        JsonValue::uint(d.z as usize),
                    ]),
                ),
                ("solid_voxels", JsonValue::uint(p.grid.count_solid())),
                ("active_model", JsonValue::uint(p.active_model)),
                (
                    "active_shape",
                    p.active_shape
                        .map(JsonValue::uint)
                        .unwrap_or(JsonValue::Null),
                ),
            ]),
        ),
        (
            "palette_selected",
            JsonValue::int(p.palette.selected as i64),
        ),
        ("model_count", JsonValue::uint(scene.models.len())),
        ("nodes", JsonValue::array(nodes)),
        ("layers", JsonValue::array(layers)),
        (
            "animation",
            JsonValue::object(vec![
                ("current_frame", JsonValue::int(scene.current_frame as i64)),
                ("length", JsonValue::int(scene.animation_length() as i64)),
                (
                    "active_shape_frame_count",
                    JsonValue::uint(p.active_frame_count()),
                ),
                (
                    "active_shape_frame_index",
                    JsonValue::uint(p.active_frame_index()),
                ),
            ]),
        ),
        ("dirty", JsonValue::bool(p.dirty)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::dispatch_tool_call;

    fn call(s: &mut McpSession, name: &str, json: &str) -> ToolResult {
        dispatch_tool_call(s, name, &JsonValue::parse(json).unwrap())
    }

    fn first_text(r: ToolResult) -> String {
        r.unwrap()[0].get_str("text").unwrap().to_string()
    }

    #[test]
    fn definitions_are_well_formed() {
        let defs = definitions();
        let mut names: Vec<&str> = defs.iter().map(|d| d.get_str("name").unwrap()).collect();
        assert!(defs.iter().all(|d| d.get("inputSchema").is_some()));
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
        assert_eq!(n, 7);
    }

    #[test]
    fn new_info_and_console() {
        let mut s = McpSession::new();
        call(
            &mut s,
            "project_new",
            r#"{"size_x":10,"size_y":12,"size_z":500,"name":"t"}"#,
        )
        .unwrap();
        let d = s.project.grid.dimensions();
        assert_eq!((d.x, d.y, d.z), (10, 12, 256));
        let info = JsonValue::parse(&first_text(call(&mut s, "project_info", "{}"))).unwrap();
        assert_eq!(info.get_str("name"), Some("t"));
        assert!(info.get_array("nodes").is_some());
        call(&mut s, "console_command", r#"{"command":"size 20 20 20"}"#).unwrap();
        assert_eq!(s.project.grid.size_x(), 20);
        call(&mut s, "console_command", r#"{"command":"full"}"#).unwrap();
        assert_eq!(s.project.grid.count_solid(), 8000);
        assert!(call(&mut s, "console_command", r#"{"command":"  "}"#).is_err());
    }

    #[test]
    fn history_undo_redo() {
        let mut s = McpSession::new();
        s.project.record_undo();
        s.project.grid.set(1, 1, 1, 3);
        call(&mut s, "history", r#"{"action":"undo"}"#).unwrap();
        assert_eq!(s.project.grid.get(1, 1, 1), 0);
        call(&mut s, "history", r#"{"action":"redo","scope":"model"}"#).unwrap();
        assert_eq!(s.project.grid.get(1, 1, 1), 3);
        assert!(call(&mut s, "history", r#"{"action":"bogus"}"#).is_err());
        call(&mut s, "history", r#"{"action":"undo","scope":"world"}"#).unwrap();
    }

    #[test]
    fn save_open_export_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mvmcp_proj_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vox = dir.join("a.vox");
        let mut s = McpSession::new();
        s.project.grid.set(2, 3, 4, 5);
        assert!(call(&mut s, "project_save", "{}").is_err());
        let args = format!(r#"{{"path":"{}"}}"#, vox.display());
        call(&mut s, "project_save", &args).unwrap();
        assert_eq!(s.current_file.as_deref(), Some(vox.as_path()));
        assert!(!s.project.dirty);
        call(&mut s, "project_save", "{}").unwrap();

        let mut s2 = McpSession::new();
        call(&mut s2, "project_open", &args).unwrap();
        assert_eq!(s2.project.grid.get(2, 3, 4), 5);
        assert!(call(&mut s2, "project_open", r#"{"path":"/nonexistent/x.vox"}"#).is_err());

        for (fmt, file) in [
            ("glb", "m.glb"),
            ("gltf", "m.gltf"),
            ("obj", "m.obj"),
            ("ply", "m.ply"),
            ("slice_png", "m.png"),
            ("vox", "b.vox"),
        ] {
            let p = dir.join(file);
            let a = format!(
                r#"{{"format":"{fmt}","path":"{}","scale":0.5}}"#,
                p.display()
            );
            call(&mut s2, "project_export", &a).unwrap();
            assert!(p.exists(), "{fmt}");
        }
        assert!(call(
            &mut s2,
            "project_export",
            r#"{"format":"x","path":"/tmp/x"}"#
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
