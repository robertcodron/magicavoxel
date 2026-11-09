//! MCP tools: scene graph (`scene_manage`), layers (`layer_manage`), animation frames
//! (`animation`) and voxel shaders (`voxel_shader`).

use mv_core::{Axis, CommandOutcome, NodeId, NodeKind, SceneStructure, XsInvocation, XsMode};
use mv_core::{BUILTIN_SHADERS, MAX_GRID_DIM};
use mv_render::VoxelShader;

use crate::json::JsonValue;
use crate::session::McpSession;
use crate::tools::{prop, prop_enum, prop_ivec3, text, tool, ToolResult};

const SCENE_ACTIONS: &[&str] = &[
    "list",
    "create_object",
    "edit_object",
    "delete",
    "translate",
    "move_to",
    "rotate90",
    "move_to_ground",
    "instantiate",
    "duplicate",
    "to_duplicate",
    "union",
    "group",
    "ungroup",
    "hide",
    "show",
    "rename",
    "set_layer",
];

pub fn definitions() -> Vec<JsonValue> {
    let node_ids = JsonValue::object(vec![
        ("type", JsonValue::str("array")),
        (
            "description",
            JsonValue::str(
                "Scene node ids (integers returned by scene_manage list / create_object).",
            ),
        ),
        (
            "items",
            JsonValue::object(vec![("type", JsonValue::str("integer"))]),
        ),
    ]);
    vec![
        tool(
            "scene_manage",
            "Manage the multi-object World scene (MagicaVoxel World Editor). A scene is a tree of \
             nodes: 'shape' nodes own a voxel model (one per animation frame), 'group' nodes contain \
             other nodes. Coordinates are world voxels, Z-up (ground is z=0). Typical workflow: \
             create_object (a new empty model that becomes the edited object) -> draw into it with \
             the voxel_* tools -> create more objects / translate / rotate90 / instantiate -> \
             render_preview with mode 'world' to see the whole scene. The voxel_* tools always edit \
             the object selected by create_object (edit=true) or edit_object, in that object's local \
             grid coordinates (0..size-1). Actions: list (nodes, ids, bounds, edited shape); \
             create_object {size [x,y,z] default [20,20,20], position? [x,y,z] world offset applied \
             after the object is placed on the ground, name?, edit? default true}; edit_object {node}; \
             delete {nodes}; translate {nodes, delta [dx,dy,dz]}; move_to {node, position [x,y,z] = \
             desired world min corner}; rotate90 {nodes, axis x|y|z, times? 1..3}; move_to_ground \
             {nodes}; instantiate {node} (new object SHARING the same model: editing one edits both); \
             duplicate {node} (independent copy); to_duplicate {node} (make a shared instance \
             independent, Ctrl+T); union {nodes} (merge into one new object, the originals are \
             removed); group {nodes}; ungroup {node}; hide / show {nodes}; rename {node, name}; \
             set_layer {nodes, layer}. Each structure-changing action is one World undo step. \
             Invalid node ids make the call fail without changing anything.",
            vec![
                ("action", prop_enum("What to do (see the tool description).", SCENE_ACTIONS)),
                ("node", prop("integer", "Single target node id.")),
                ("nodes", node_ids),
                ("size", prop_ivec3("create_object: model size [x,y,z], each 1..=256 (default [20,20,20]).")),
                ("position", prop_ivec3("create_object: world offset [x,y,z]; move_to: desired world min corner [x,y,z].")),
                ("delta", prop_ivec3("translate: world offset [dx,dy,dz] in voxels.")),
                ("name", prop("string", "create_object / rename: node name.")),
                ("edit", prop("boolean", "create_object: make the new object the edited one (default true).")),
                ("axis", prop_enum("rotate90: world axis.", &["x", "y", "z"])),
                ("times", prop("integer", "rotate90: number of quarter turns, 1..3 (default 1).")),
                ("layer", prop("integer", "set_layer: layer index (see layer_manage list).")),
            ],
            &["action"],
        ),
        tool(
            "layer_manage",
            "List or modify the 8 scene layers. Hidden layers hide all objects assigned to them \
             (assign objects with scene_manage set_layer). Actions: list; set {index, name?, hidden?, \
             color? [r,g,b]}.",
            vec![
                ("action", prop_enum("list or set.", &["list", "set"])),
                ("index", prop("integer", "Layer index for set.")),
                ("name", prop("string", "New layer name.")),
                ("hidden", prop("boolean", "Hide / show the layer.")),
                (
                    "color",
                    JsonValue::object(vec![
                        ("type", JsonValue::str("array")),
                        ("description", JsonValue::str("Layer color [r,g,b], 0..255.")),
                        ("items", JsonValue::object(vec![("type", JsonValue::str("integer"))])),
                    ]),
                ),
            ],
            &["action"],
        ),
        tool(
            "animation",
            "Animation frames of the edited object (the shape selected by scene_manage \
             create_object / edit_object). Each frame is its own voxel model; voxel_* tools draw into \
             the current frame. Actions: info; set_frame {frame} (show and edit that frame; the \
             scene animation loops over the longest shape); add_frame {empty?} (insert after the \
             current frame, a copy of it unless empty=true, and switch to it); delete_frame (delete \
             the current frame, at least one stays).",
            vec![
                ("action", prop_enum("What to do.", &["info", "set_frame", "add_frame", "delete_frame"])),
                ("frame", prop("integer", "set_frame: frame index (0-based).")),
                ("empty", prop("boolean", "add_frame: insert a blank model instead of a copy.")),
            ],
            &["action"],
        ),
        tool(
            "voxel_shader",
            "Run a voxel shader (procedural generator/filter, MagicaVoxel 'xs') on the edited \
             object. Actions: list (built-in shaders plus shader files found in the shader folders \
             with their declared arguments: name/min/max/default); run {name, args? [numbers, \
             positional], iterations? 1..=360, mode? in_place (default, edit the model in place) | \
             prev (generate one new animation frame per iteration, each based on the previous \
             generated one) | cur (each based on the current frame)}. prev/cur require an edited \
             object (scene_manage create_object / edit_object).",
            vec![
                ("action", prop_enum("list or run.", &["list", "run"])),
                ("name", prop("string", "Shader name (e.g. sphere, wave, gyroid, life).")),
                (
                    "args",
                    JsonValue::object(vec![
                        ("type", JsonValue::str("array")),
                        ("description", JsonValue::str("Positional numeric shader arguments.")),
                        ("items", JsonValue::object(vec![("type", JsonValue::str("number"))])),
                    ]),
                ),
                ("iterations", prop("integer", "Number of iterations 1..=360 (default 1).")),
                ("mode", prop_enum("Run mode (default in_place).", &["in_place", "prev", "cur"])),
            ],
            &["action"],
        ),
    ]
}

pub fn dispatch(session: &mut McpSession, name: &str, args: &JsonValue) -> Option<ToolResult> {
    Some(match name {
        "scene_manage" => scene_manage(session, args),
        "layer_manage" => layer_manage(session, args),
        "animation" => animation(session, args),
        "voxel_shader" => voxel_shader(session, args),
        _ => return None,
    })
}

fn required_action(args: &JsonValue) -> Result<&str, String> {
    args.get_str("action")
        .ok_or_else(|| "Missing required string 'action'".to_string())
}

fn node_id_of(v: &JsonValue) -> Option<NodeId> {
    let n = v.as_i64()?;
    usize::try_from(n).ok()
}

/// Single node id from `key`, which must exist in the scene. The root is rejected unless
/// `allow_root`.
fn node_arg(session: &McpSession, args: &JsonValue, key: &str) -> Result<NodeId, String> {
    let v = args
        .get(key)
        .ok_or_else(|| format!("Missing required integer '{key}' (a scene node id)"))?;
    let id =
        node_id_of(v).ok_or_else(|| format!("'{key}' must be a non-negative integer node id"))?;
    check_node(session, id)?;
    Ok(id)
}

fn check_node(session: &McpSession, id: NodeId) -> Result<(), String> {
    let scene = &session.project.scene;
    if scene.node(id).is_none() {
        return Err(format!("Unknown scene node {id} (use scene_manage list)"));
    }
    if id == scene.root() {
        return Err("The root group cannot be targeted by this action".to_string());
    }
    Ok(())
}

/// `nodes` array, or a single `node` as a fallback. All ids validated.
fn nodes_arg(session: &McpSession, args: &JsonValue) -> Result<Vec<NodeId>, String> {
    let mut ids = Vec::new();
    if let Some(arr) = args.get_array("nodes") {
        for v in arr {
            ids.push(node_id_of(v).ok_or("'nodes' must contain non-negative integer node ids")?);
        }
    } else if args.get("node").is_some() {
        ids.push(node_arg(session, args, "node")?);
    }
    if ids.is_empty() {
        return Err("Missing required non-empty array 'nodes' (scene node ids)".to_string());
    }
    for &id in &ids {
        check_node(session, id)?;
    }
    ids.dedup();
    Ok(ids)
}

fn ivec3_arg(args: &JsonValue, key: &str) -> Result<glam::IVec3, String> {
    if args.get(key).is_none() {
        return Err(format!("Missing required array '{key}' [x,y,z]"));
    }
    let v = args
        .get_ivec3(key)
        .ok_or_else(|| format!("'{key}' must be an array of 3 integers [x,y,z]"))?;
    Ok(glam::IVec3::from(v))
}

fn ivec3_json(v: glam::IVec3) -> JsonValue {
    JsonValue::array(vec![
        JsonValue::int(v.x as i64),
        JsonValue::int(v.y as i64),
        JsonValue::int(v.z as i64),
    ])
}

/// Run a structure-changing closure as exactly one World undo step. On error the scene is
/// restored and no undo step is left behind.
fn undoable<T>(
    session: &mut McpSession,
    f: impl FnOnce(&mut mv_core::EditorProject) -> Result<T, String>,
) -> Result<T, String> {
    let p = &mut session.project;
    let before: SceneStructure = p.scene.structure();
    let depth = p.world_undo.len();
    p.record_world_undo();
    match f(p) {
        Ok(v) => Ok(v),
        Err(e) => {
            p.scene.restore_structure(before);
            p.world_undo.truncate(depth);
            Err(e)
        }
    }
}

fn se<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn ids_json(ids: &[NodeId]) -> JsonValue {
    JsonValue::array(ids.iter().map(|&i| JsonValue::uint(i)).collect())
}

fn scene_list(session: &McpSession) -> JsonValue {
    let p = &session.project;
    let scene = &p.scene;
    let mut nodes = Vec::new();
    for (id, n) in scene.nodes() {
        if id == scene.root() {
            continue;
        }
        let is_shape = matches!(n.kind, NodeKind::Shape { .. });
        let mut f: Vec<(&str, JsonValue)> = vec![
            ("id", JsonValue::uint(id)),
            (
                "name",
                n.name.as_deref().map_or(JsonValue::Null, JsonValue::str),
            ),
            (
                "kind",
                JsonValue::str(if is_shape { "shape" } else { "group" }),
            ),
        ];
        if is_shape {
            f.push((
                "model",
                scene
                    .shape_model(id)
                    .map_or(JsonValue::Null, JsonValue::uint),
            ));
            f.push(("frames", JsonValue::uint(n.frame_models().len())));
        } else {
            f.push(("children", ids_json(n.children())));
        }
        f.push((
            "parent",
            n.parent
                .filter(|&pa| pa != scene.root())
                .map_or(JsonValue::Null, JsonValue::uint),
        ));
        f.push(("hidden", JsonValue::bool(n.hidden)));
        f.push(("visible", JsonValue::bool(scene.is_visible(id))));
        f.push(("layer", JsonValue::uint(n.layer_id)));
        f.push((
            "world_bounds",
            scene.world_bounds(id).map_or(JsonValue::Null, |(lo, hi)| {
                JsonValue::object(vec![("min", ivec3_json(lo)), ("max", ivec3_json(hi))])
            }),
        ));
        nodes.push(JsonValue::object(f));
    }
    JsonValue::object(vec![
        (
            "edited_shape",
            p.active_shape.map_or(JsonValue::Null, JsonValue::uint),
        ),
        (
            "current_frame",
            JsonValue::uint(scene.current_frame as usize),
        ),
        ("nodes", JsonValue::array(nodes)),
    ])
}

fn scene_manage(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = required_action(args)?;
    match action {
        "list" => Ok(vec![text(scene_list(session).to_json_string())]),
        "create_object" => {
            let size = match args.get("size") {
                None => [20, 20, 20],
                Some(_) => args
                    .get_ivec3("size")
                    .ok_or("'size' must be an array of 3 integers [x,y,z]")?,
            };
            let max = MAX_GRID_DIM as i32;
            if size.iter().any(|&s| s < 1 || s > max) {
                return Err(format!(
                    "'size' components must be within 1..={max}, got {size:?}"
                ));
            }
            let offset = match args.get("position") {
                None => None,
                Some(_) => Some(ivec3_arg(args, "position")?),
            };
            let edit = args.get_bool("edit").unwrap_or(true);
            let root = session.project.scene.root();
            let id = session
                .project
                .add_new_object(root, size[0] as u32, size[1] as u32, size[2] as u32)
                .map_err(se)?;
            if let Some(d) = offset {
                session.project.scene.translate_world(id, d).map_err(se)?;
            }
            if let Some(n) = args.get_str("name") {
                if let Some(node) = session.project.scene.node_mut(id) {
                    node.name = Some(n.to_string());
                }
            }
            if edit {
                session.project.open_shape(id);
            }
            let bounds = session.project.scene.world_bounds(id);
            let mut msg = format!(
                "Created object node {id} ({}x{}x{})",
                size[0], size[1], size[2]
            );
            if let Some((lo, hi)) = bounds {
                msg += &format!(
                    ", world bounds min [{},{},{}] max [{},{},{}]",
                    lo.x, lo.y, lo.z, hi.x, hi.y, hi.z
                );
            }
            msg += if edit {
                ". It is now the edited object: voxel_* tools draw into it (local coordinates 0..size-1)."
            } else {
                ". Use scene_manage edit_object to draw into it."
            };
            Ok(vec![text(msg)])
        }
        "edit_object" => {
            let id = node_arg(session, args, "node")?;
            if !session.project.open_shape(id) {
                return Err(format!(
                    "Node {id} is not a shape (groups cannot be edited)"
                ));
            }
            let d = session.project.grid.dimensions();
            Ok(vec![text(format!(
                "Now editing node {id} (model {}x{}x{}, frame {}/{})",
                d.x,
                d.y,
                d.z,
                session.project.active_frame_index() + 1,
                session.project.active_frame_count()
            ))])
        }
        "delete" => {
            let ids = nodes_arg(session, args)?;
            undoable(session, |p| {
                for &id in &ids {
                    // A node may already be gone when an ancestor was deleted too.
                    if p.scene.node(id).is_some() {
                        p.scene.remove_node(id).map_err(se)?;
                    }
                }
                Ok(())
            })?;
            session.project.refresh_active_model();
            Ok(vec![text(format!("Deleted nodes {ids:?}"))])
        }
        "translate" => {
            let ids = nodes_arg(session, args)?;
            let d = ivec3_arg(args, "delta")?;
            undoable(session, |p| {
                for &id in &ids {
                    p.scene.translate_world(id, d).map_err(se)?;
                }
                Ok(())
            })?;
            Ok(vec![text(format!(
                "Translated nodes {ids:?} by [{},{},{}]",
                d.x, d.y, d.z
            ))])
        }
        "move_to" => {
            let id = node_arg(session, args, "node")?;
            let target = ivec3_arg(args, "position")?;
            let (lo, _) = session
                .project
                .scene
                .world_bounds(id)
                .ok_or_else(|| format!("Node {id} has no voxels, cannot compute its bounds"))?;
            let delta = target - lo;
            undoable(session, |p| p.scene.translate_world(id, delta).map_err(se))?;
            Ok(vec![text(format!(
                "Moved node {id} so its min corner is [{},{},{}]",
                target.x, target.y, target.z
            ))])
        }
        "rotate90" => {
            let ids = nodes_arg(session, args)?;
            let axis = match args.get_str("axis") {
                Some("x") | Some("X") => Axis::X,
                Some("y") | Some("Y") => Axis::Y,
                Some("z") | Some("Z") => Axis::Z,
                Some(o) => return Err(format!("'axis' must be x, y or z, got '{o}'")),
                None => return Err("Missing required 'axis' (x|y|z)".to_string()),
            };
            let times = match args.get("times") {
                None => 1,
                Some(v) => v
                    .as_i64()
                    .filter(|t| (1..=3).contains(t))
                    .ok_or("'times' must be an integer within 1..=3")?,
            };
            undoable(session, |p| {
                for &id in &ids {
                    for _ in 0..times {
                        p.scene.rotate90(id, axis).map_err(se)?;
                    }
                }
                Ok(())
            })?;
            Ok(vec![text(format!(
                "Rotated nodes {ids:?} by {} x 90 degrees",
                times
            ))])
        }
        "move_to_ground" => {
            let ids = nodes_arg(session, args)?;
            undoable(session, |p| {
                for &id in &ids {
                    p.scene.move_to_ground(id).map_err(se)?;
                }
                Ok(())
            })?;
            Ok(vec![text(format!(
                "Moved nodes {ids:?} onto the ground (z=0)"
            ))])
        }
        "instantiate" | "duplicate" => {
            let id = node_arg(session, args, "node")?;
            if session.project.scene.shape_model(id).is_none() {
                return Err(format!("Node {id} is not a shape"));
            }
            let inst = action == "instantiate";
            let new = undoable(session, |p| {
                if inst {
                    p.scene.instantiate(id).map_err(se)
                } else {
                    p.scene.duplicate(id).map_err(se)
                }
            })?;
            Ok(vec![text(format!(
                "Created node {new} as {} of node {id} at the same position (translate it to see it)",
                if inst { "a shared instance (same model)" } else { "an independent copy" }
            ))])
        }
        "to_duplicate" => {
            let id = node_arg(session, args, "node")?;
            if session.project.scene.shape_model(id).is_none() {
                return Err(format!("Node {id} is not a shape"));
            }
            let changed = undoable(session, |p| {
                p.scene.convert_reference_to_duplicate(id).map_err(se)
            })?;
            if !changed {
                // Nothing changed: drop the useless undo step.
                session.project.world_undo.pop();
            }
            session.project.refresh_active_model();
            Ok(vec![text(if changed {
                format!("Node {id} now owns an independent copy of its model")
            } else {
                format!("Node {id} already has its own model (no change)")
            })])
        }
        "union" => {
            let ids = nodes_arg(session, args)?;
            let new = undoable(session, |p| p.scene.union(&ids).map_err(se))?;
            session.project.refresh_active_model();
            Ok(vec![text(format!(
                "Merged nodes {ids:?} into new object node {new}"
            ))])
        }
        "group" => {
            let ids = nodes_arg(session, args)?;
            let g = undoable(session, |p| p.scene.group_nodes(&ids).map_err(se))?;
            Ok(vec![text(format!(
                "Grouped nodes {ids:?} into group node {g}"
            ))])
        }
        "ungroup" => {
            let id = node_arg(session, args, "node")?;
            if session.project.scene.shape_model(id).is_some() {
                return Err(format!("Node {id} is not a group"));
            }
            let freed = undoable(session, |p| p.scene.ungroup(id).map_err(se))?;
            Ok(vec![text(format!(
                "Ungrouped node {id}; freed nodes {freed:?}"
            ))])
        }
        "hide" | "show" => {
            let ids = nodes_arg(session, args)?;
            let hidden = action == "hide";
            undoable(session, |p| {
                for &id in &ids {
                    if let Some(n) = p.scene.node_mut(id) {
                        n.hidden = hidden;
                    }
                }
                Ok(())
            })?;
            Ok(vec![text(format!(
                "{} nodes {ids:?}",
                if hidden { "Hid" } else { "Showed" }
            ))])
        }
        "rename" => {
            let id = node_arg(session, args, "node")?;
            let name = args
                .get_str("name")
                .ok_or("Missing required string 'name'")?;
            undoable(session, |p| {
                if let Some(n) = p.scene.node_mut(id) {
                    n.name = Some(name.to_string());
                }
                Ok(())
            })?;
            Ok(vec![text(format!("Renamed node {id} to '{name}'"))])
        }
        "set_layer" => {
            let ids = nodes_arg(session, args)?;
            let layer = args
                .get_usize("layer")
                .ok_or("Missing required integer 'layer'")?;
            let count = session.project.scene.layers.len();
            if layer >= count {
                return Err(format!("'layer' must be within 0..{count}, got {layer}"));
            }
            undoable(session, |p| {
                for &id in &ids {
                    if let Some(n) = p.scene.node_mut(id) {
                        n.layer_id = layer;
                    }
                }
                Ok(())
            })?;
            Ok(vec![text(format!("Moved nodes {ids:?} to layer {layer}"))])
        }
        other => Err(format!(
            "Unknown scene_manage action '{other}' (expected one of: {})",
            SCENE_ACTIONS.join(", ")
        )),
    }
}

fn layers_json(session: &McpSession) -> JsonValue {
    JsonValue::array(
        session
            .project
            .scene
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
                        JsonValue::array(
                            l.color.iter().map(|&c| JsonValue::int(c as i64)).collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}

fn layer_manage(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    match required_action(args)? {
        "list" => Ok(vec![text(layers_json(session).to_json_string())]),
        "set" => {
            let index = args
                .get_usize("index")
                .ok_or("Missing required integer 'index'")?;
            let count = session.project.scene.layers.len();
            if index >= count {
                return Err(format!("'index' must be within 0..{count}, got {index}"));
            }
            let color = match args.get("color") {
                None => None,
                Some(v) => {
                    let arr = v.as_array().filter(|a| a.len() == 3);
                    let arr = arr.ok_or("'color' must be an array [r,g,b]")?;
                    let mut c = [0u8; 3];
                    for (o, v) in c.iter_mut().zip(arr) {
                        *o = v
                            .as_i64()
                            .filter(|n| (0..=255).contains(n))
                            .ok_or("'color' components must be integers within 0..=255")?
                            as u8;
                    }
                    Some(c)
                }
            };
            let name = args.get_str("name").map(str::to_string);
            let hidden = args.get_bool("hidden");
            if color.is_none() && name.is_none() && hidden.is_none() {
                return Err("Nothing to set: pass name, hidden and/or color".to_string());
            }
            undoable(session, |p| {
                let l = &mut p.scene.layers[index];
                if let Some(n) = name {
                    l.name = n;
                }
                if let Some(h) = hidden {
                    l.hidden = h;
                }
                if let Some(c) = color {
                    l.color = c;
                }
                Ok(())
            })?;
            Ok(vec![text(layers_json(session).to_json_string())])
        }
        other => Err(format!(
            "Unknown layer_manage action '{other}' (expected list or set)"
        )),
    }
}

fn animation_info(session: &McpSession) -> String {
    let p = &session.project;
    JsonValue::object(vec![
        (
            "current_frame",
            JsonValue::uint(p.scene.current_frame as usize),
        ),
        (
            "scene_animation_length",
            JsonValue::uint(p.scene.animation_length() as usize),
        ),
        (
            "edited_shape",
            p.active_shape.map_or(JsonValue::Null, JsonValue::uint),
        ),
        ("shape_frame_count", JsonValue::uint(p.active_frame_count())),
        ("shape_frame_index", JsonValue::uint(p.active_frame_index())),
    ])
    .to_json_string()
}

fn animation(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = required_action(args)?;
    if matches!(action, "add_frame" | "delete_frame") && session.project.active_shape.is_none() {
        return Err(
            "No edited object: use scene_manage create_object / edit_object first".to_string(),
        );
    }
    match action {
        "info" => Ok(vec![text(animation_info(session))]),
        "set_frame" => {
            let frame = args
                .get_u32("frame")
                .ok_or("Missing required non-negative integer 'frame'")?;
            session.project.set_frame(frame);
            Ok(vec![text(animation_info(session))])
        }
        "add_frame" => {
            let empty = args.get_bool("empty").unwrap_or(false);
            session
                .project
                .add_frame(empty)
                .ok_or("Could not add a frame to the edited object")?;
            Ok(vec![text(animation_info(session))])
        }
        "delete_frame" => {
            if !session.project.delete_frame() {
                return Err(
                    "Cannot delete the frame: an object keeps at least one frame".to_string(),
                );
            }
            Ok(vec![text(animation_info(session))])
        }
        other => Err(format!(
            "Unknown animation action '{other}' (expected info, set_frame, add_frame, delete_frame)"
        )),
    }
}

fn voxel_shader(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    match required_action(args)? {
        "list" => {
            let builtins =
                JsonValue::array(BUILTIN_SHADERS.iter().map(|s| JsonValue::str(*s)).collect());
            let mut files = Vec::new();
            for (name, path) in McpSession::discover_external_shaders() {
                let mut f = vec![("name", JsonValue::str(name.clone()))];
                match VoxelShader::load(name, &path) {
                    Ok(sh) => f.push((
                        "args",
                        JsonValue::array(
                            sh.args
                                .iter()
                                .map(|a| {
                                    JsonValue::object(vec![
                                        ("name", JsonValue::str(a.name.clone())),
                                        ("min", JsonValue::float(a.min as f64)),
                                        ("max", JsonValue::float(a.max as f64)),
                                        ("default", JsonValue::float(a.default as f64)),
                                        ("step", JsonValue::float(a.step as f64)),
                                    ])
                                })
                                .collect(),
                        ),
                    )),
                    Err(e) => f.push(("error", JsonValue::str(e.to_string()))),
                }
                files.push(JsonValue::object(f));
            }
            let out = JsonValue::object(vec![
                ("builtin", builtins),
                ("files", JsonValue::array(files)),
            ]);
            Ok(vec![text(out.to_json_string())])
        }
        "run" => {
            let name = args
                .get_str("name")
                .ok_or("Missing required string 'name'")?;
            if name.is_empty() || name.contains(char::is_whitespace) || name.starts_with('-') {
                return Err(format!("Invalid shader name '{name}'"));
            }
            let mut shader_args = Vec::new();
            if let Some(v) = args.get("args") {
                let arr = v.as_array().ok_or("'args' must be an array of numbers")?;
                for a in arr {
                    let f = a
                        .as_f64()
                        .filter(|f| f.is_finite())
                        .ok_or("'args' must contain finite numbers")?;
                    shader_args.push(f as f32);
                }
            }
            let iterations = match args.get("iterations") {
                None => 1,
                Some(v) => v
                    .as_i64()
                    .filter(|n| (1..=360).contains(n))
                    .ok_or("'iterations' must be an integer within 1..=360")?
                    as u32,
            };
            let mode = match args.get_str("mode").unwrap_or("in_place") {
                "in_place" => XsMode::InPlace,
                "prev" => XsMode::Animate {
                    from_previous: true,
                },
                "cur" => XsMode::Animate {
                    from_previous: false,
                },
                o => return Err(format!("'mode' must be in_place, prev or cur, got '{o}'")),
            };
            if matches!(mode, XsMode::Animate { .. }) && session.project.active_shape.is_none() {
                return Err(
                    "mode prev/cur generates animation frames and needs an edited object: use \
                     scene_manage create_object / edit_object first"
                        .to_string(),
                );
            }
            let inv = XsInvocation {
                shader: name.to_string(),
                args: shader_args,
                iterations,
                mode,
            };
            match session.project.run_command(&inv.to_command())? {
                CommandOutcome::Message(m) => Ok(vec![text(if m.is_empty() {
                    format!("Ran shader '{name}'")
                } else {
                    m
                })]),
                CommandOutcome::ExternalShader(inv) => {
                    let m = session.run_external_shader(&inv)?;
                    Ok(vec![text(if m.is_empty() {
                        format!("Ran shader '{name}'")
                    } else {
                        m
                    })])
                }
                CommandOutcome::ExportRequested { .. } => {
                    Err("Unexpected export request from a shader".to_string())
                }
            }
        }
        other => Err(format!(
            "Unknown voxel_shader action '{other}' (expected list or run)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::dispatch_tool_call;

    fn call(s: &mut McpSession, name: &str, json: &str) -> ToolResult {
        dispatch_tool_call(s, name, &JsonValue::parse(json).unwrap())
    }

    fn ok(s: &mut McpSession, name: &str, json: &str) -> String {
        match call(s, name, json) {
            Ok(items) => items
                .iter()
                .filter_map(|i| i.get_str("text").map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => panic!("{name} {json} failed: {e}"),
        }
    }

    fn new_obj(s: &mut McpSession, json: &str) -> NodeId {
        let msg = ok(s, "scene_manage", json);
        msg.split_whitespace().nth(3).unwrap().parse().unwrap()
    }

    fn bounds(s: &McpSession, id: NodeId) -> (glam::IVec3, glam::IVec3) {
        s.project.scene.world_bounds(id).unwrap()
    }

    #[test]
    fn definitions_have_unique_names() {
        let defs = definitions();
        let mut names: Vec<_> = defs
            .iter()
            .map(|d| d.get_str("name").unwrap().to_string())
            .collect();
        assert_eq!(names.len(), 4);
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4);
        for d in &defs {
            assert!(d.get("inputSchema").is_some());
        }
    }

    #[test]
    fn create_edit_translate_rotate_union_group_hide() {
        let mut s = McpSession::new();
        let a = new_obj(
            &mut s,
            r#"{"action":"create_object","size":[4,6,8],"name":"a"}"#,
        );
        assert_eq!(s.project.active_shape, Some(a));
        assert_eq!(s.project.grid.dimensions(), glam::UVec3::new(4, 6, 8));
        let b = new_obj(
            &mut s,
            r#"{"action":"create_object","size":[4,4,4],"position":[20,0,0],"edit":false}"#,
        );
        assert_eq!(s.project.active_shape, Some(a));
        assert_eq!(bounds(&s, b).0.x, bounds(&s, a).0.x + 20);
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"edit_object","node":{b}}}"#),
        );
        assert_eq!(s.project.active_shape, Some(b));

        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"translate","nodes":[{a}],"delta":[5,0,3]}}"#),
        );
        assert_eq!(bounds(&s, a).0.z, 3);
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"move_to_ground","nodes":[{a}]}}"#),
        );
        assert_eq!(bounds(&s, a).0.z, 0);
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"move_to","node":{a},"position":[10,10,0]}}"#),
        );
        assert_eq!(bounds(&s, a).0, glam::IVec3::new(10, 10, 0));
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"rotate90","nodes":[{a}],"axis":"z"}}"#),
        );
        let (lo, hi) = bounds(&s, a);
        assert_eq!(hi - lo + glam::IVec3::ONE, glam::IVec3::new(6, 4, 8));

        let list = ok(&mut s, "scene_manage", r#"{"action":"list"}"#);
        assert!(list.contains("\"edited_shape\"") && list.contains("\"world_bounds\""));

        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"hide","nodes":[{a}]}}"#),
        );
        assert!(!s.project.scene.is_visible(a));
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"show","nodes":[{a}]}}"#),
        );
        assert!(s.project.scene.is_visible(a));
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"rename","node":{a},"name":"x"}}"#),
        );
        assert_eq!(s.project.scene.node(a).unwrap().name.as_deref(), Some("x"));

        // Group / ungroup.
        let g = new_obj_group(
            &mut s,
            &format!(r#"{{"action":"group","nodes":[{a},{b}]}}"#),
        );
        assert_eq!(s.project.scene.node(a).unwrap().parent, Some(g));
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"ungroup","node":{g}}}"#),
        );
        assert!(s.project.scene.node(g).is_none());

        // Instances and union.
        let ins = new_obj_after(&mut s, &format!(r#"{{"action":"instantiate","node":{a}}}"#));
        assert_eq!(
            s.project.scene.shape_model(ins),
            s.project.scene.shape_model(a)
        );
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"to_duplicate","node":{ins}}}"#),
        );
        assert_ne!(
            s.project.scene.shape_model(ins),
            s.project.scene.shape_model(a)
        );
        // Voxels are needed for union.
        ok(
            &mut s,
            "scene_manage",
            &format!(r#"{{"action":"edit_object","node":{a}}}"#),
        );
        s.project.grid.set(1, 1, 1, 3);
        let before = s.project.scene.nodes().count();
        let u = new_obj_after(&mut s, &format!(r#"{{"action":"union","nodes":[{a}]}}"#));
        assert!(s.project.scene.node(a).is_none());
        assert!(s.project.scene.node(u).is_some());
        assert_eq!(s.project.scene.nodes().count(), before);
    }

    fn new_obj_group(s: &mut McpSession, json: &str) -> NodeId {
        let msg = ok(s, "scene_manage", json);
        msg.rsplit(' ').next().unwrap().parse().unwrap()
    }

    fn new_obj_after(s: &mut McpSession, json: &str) -> NodeId {
        let msg = ok(s, "scene_manage", json);
        let w: Vec<_> = msg.split_whitespace().collect();
        let pos = if w[0] == "Merged" {
            w.iter().position(|x| *x == "node").unwrap()
        } else {
            2
        };
        let tok = if w[0] == "Merged" {
            w[w.len() - 1]
        } else {
            w[pos]
        };
        tok.parse().unwrap()
    }

    #[test]
    fn one_world_undo_step_per_action() {
        let mut s = McpSession::new();
        let a = new_obj(&mut s, r#"{"action":"create_object","size":[4,4,4]}"#);
        assert_eq!(s.project.world_undo.len(), 1);
        let before = s.project.scene.structure();
        for json in [
            format!(r#"{{"action":"translate","nodes":[{a}],"delta":[3,0,0]}}"#),
            format!(r#"{{"action":"rotate90","nodes":[{a}],"axis":"x","times":2}}"#),
            format!(r#"{{"action":"hide","nodes":[{a}]}}"#),
            format!(r#"{{"action":"set_layer","nodes":[{a}],"layer":2}}"#),
            format!(r#"{{"action":"instantiate","node":{a}}}"#),
            format!(r#"{{"action":"delete","nodes":[{a}]}}"#),
        ] {
            let n = s.project.world_undo.len();
            let snap = s.project.scene.structure();
            ok(&mut s, "scene_manage", &json);
            assert_eq!(s.project.world_undo.len(), n + 1, "{json}");
            assert!(s.project.world_undo());
            assert!(s.project.scene.structure() == snap, "{json}");
            s.project.world_redo();
            s.project.world_undo();
        }
        assert!(s.project.scene.structure() == before);
    }

    #[test]
    fn errors_leave_scene_unchanged() {
        let mut s = McpSession::new();
        let a = new_obj(&mut s, r#"{"action":"create_object"}"#);
        let snap = s.project.scene.structure();
        let depth = s.project.world_undo.len();
        for json in [
            format!(r#"{{"action":"translate","nodes":[{a},999],"delta":[1,0,0]}}"#),
            r#"{"action":"delete","nodes":[999]}"#.to_string(),
            r#"{"action":"delete","nodes":[0]}"#.to_string(),
            r#"{"action":"edit_object","node":999}"#.to_string(),
            r#"{"action":"edit_object","node":-1}"#.to_string(),
            format!(r#"{{"action":"rotate90","nodes":[{a}],"axis":"w"}}"#),
            format!(r#"{{"action":"rotate90","nodes":[{a}],"axis":"x","times":9}}"#),
            format!(r#"{{"action":"translate","nodes":[{a}],"delta":[1,2]}}"#),
            format!(r#"{{"action":"set_layer","nodes":[{a}],"layer":99}}"#),
            r#"{"action":"create_object","size":[0,5,5]}"#.to_string(),
            r#"{"action":"create_object","size":[999,5,5]}"#.to_string(),
            r#"{"action":"bogus"}"#.to_string(),
            r#"{}"#.to_string(),
            format!(r#"{{"action":"ungroup","node":{a}}}"#),
        ] {
            assert!(call(&mut s, "scene_manage", &json).is_err(), "{json}");
        }
        assert!(s.project.scene.structure() == snap);
        assert_eq!(s.project.world_undo.len(), depth);
    }

    #[test]
    fn layers() {
        let mut s = McpSession::new();
        assert!(ok(&mut s, "layer_manage", r#"{"action":"list"}"#).contains("\"index\":7"));
        ok(
            &mut s,
            "layer_manage",
            r#"{"action":"set","index":2,"name":"trees","hidden":true,"color":[1,2,3]}"#,
        );
        let l = &s.project.scene.layers[2];
        assert_eq!(
            (l.name.as_str(), l.hidden, l.color),
            ("trees", true, [1, 2, 3])
        );
        assert_eq!(s.project.world_undo.len(), 1);
        assert!(s.project.world_undo());
        assert!(!s.project.scene.layers[2].hidden);
        assert!(call(
            &mut s,
            "layer_manage",
            r#"{"action":"set","index":50,"hidden":true}"#
        )
        .is_err());
        assert!(call(&mut s, "layer_manage", r#"{"action":"set","index":1}"#).is_err());
        assert!(call(
            &mut s,
            "layer_manage",
            r#"{"action":"set","index":1,"color":[1,2,300]}"#
        )
        .is_err());
    }

    #[test]
    fn animation_frames() {
        let mut s = McpSession::new();
        s.project.active_shape = None;
        assert!(call(&mut s, "animation", r#"{"action":"add_frame"}"#).is_err());
        new_obj(&mut s, r#"{"action":"create_object","size":[4,4,4]}"#);
        assert!(call(&mut s, "animation", r#"{"action":"delete_frame"}"#).is_err());
        ok(
            &mut s,
            "animation",
            r#"{"action":"add_frame","empty":true}"#,
        );
        assert_eq!(s.project.active_frame_count(), 2);
        assert_eq!(s.project.active_frame_index(), 1);
        ok(&mut s, "animation", r#"{"action":"add_frame"}"#);
        assert_eq!(s.project.active_frame_count(), 3);
        let info = ok(&mut s, "animation", r#"{"action":"set_frame","frame":0}"#);
        assert!(
            info.contains("\"shape_frame_count\":3") && info.contains("\"shape_frame_index\":0")
        );
        ok(&mut s, "animation", r#"{"action":"delete_frame"}"#);
        assert_eq!(s.project.active_frame_count(), 2);
        assert!(call(&mut s, "animation", r#"{"action":"set_frame"}"#).is_err());
    }

    #[test]
    fn shader_list_and_run() {
        let mut s = McpSession::new();
        let list = ok(&mut s, "voxel_shader", r#"{"action":"list"}"#);
        assert!(list.contains("sphere") && list.contains("\"files\""));
        ok(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"sphere"}"#,
        );
        assert!(s.project.grid.count_solid() > 0);
        s.project.active_shape = None;
        assert!(call(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"sphere","mode":"prev"}"#
        )
        .is_err());
        assert!(call(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"nope_shader"}"#
        )
        .is_err());
        assert!(call(&mut s, "voxel_shader", r#"{"action":"run","name":"a b"}"#).is_err());
        assert!(call(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"sphere","iterations":0}"#
        )
        .is_err());
        new_obj(&mut s, r#"{"action":"create_object","size":[8,8,8]}"#);
        ok(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"sphere","iterations":2,"mode":"cur"}"#,
        );
        assert!(s.project.active_frame_count() >= 2);
    }
}
