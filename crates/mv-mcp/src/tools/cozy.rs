//! MCP tools: procedural cozy tools (`smart_brush`, `architecture_generate`,
//! `light_manage`, `gizmo_transform`).

use glam::{IVec3, Vec3};
use mv_core::architecture::{
    build_chimney, generate_roof, punch_door, punch_window, RoofType,
};
use mv_core::biome::{carve_pond, generate_fluffy_tree, grow_ivy, paint_path};
use mv_core::light::{spawn_lantern, PointLight};
use mv_core::spline::{carve_arch, generate_fence, generate_spline_wall};
use mv_core::voxel::{Axis, VoxelGrid};
use mv_core::NodeId;

use crate::json::JsonValue;
use crate::session::McpSession;
use crate::tools::{prop, prop_enum, prop_ivec3, prop_vec3, text, tool, ToolResult};

const COORD_LIMIT: i32 = 100_000;

pub fn definitions() -> Vec<JsonValue> {
    vec![
        tool(
            "smart_brush",
            "Procedural spline and organic biome brushes for cozy world building. \
             Actions: \
             - curve_wall: rasterize continuous walls along 3D spline control points (thickness, height, crenelated, color). \
             - smart_arch: carve a vaulted archway with an outer stone frame (center, width, height, depth, color). \
             - fence: wooden post-and-rail fence along 3D spline points (points, height, color). \
             - paint_path: organic cobblestone and dirt pathway along 3D spline points (points, width, stone_color, dirt_color). \
             - grow_ivy: procedural climbing ivy foliage on vertical walls (voxels, count, color). \
             - pond: natural sloping water basin with air clearing above (center, radius, depth, color). \
             - tree: stylized fluffy tree with wooden trunk and overlapping foliage lobes (base, height, radius, wood_color, foliage_color).",
            vec![
                (
                    "action",
                    prop_enum(
                        "Smart brush action to execute",
                        &[
                            "curve_wall",
                            "smart_arch",
                            "fence",
                            "paint_path",
                            "grow_ivy",
                            "pond",
                            "tree",
                        ],
                    ),
                ),
                (
                    "points",
                    prop(
                        "array",
                        "Array of 3D control points [[x,y,z], ...] for curve_wall, fence, or paint_path",
                    ),
                ),
                (
                    "voxels",
                    prop(
                        "array",
                        "Array of seed voxels [[x,y,z], ...] for grow_ivy",
                    ),
                ),
                (
                    "center",
                    prop_ivec3("Center coordinate [x,y,z] for smart_arch or pond"),
                ),
                (
                    "base",
                    prop_ivec3("Ground base coordinate [x,y,z] for tree"),
                ),
                ("thickness", prop("integer", "Wall thickness in voxels for curve_wall (default 1)")),
                ("height", prop("integer", "Height in voxels for curve_wall, smart_arch, fence, or tree")),
                ("width", prop("integer", "Width / span in voxels for smart_arch or paint_path")),
                ("depth", prop("integer", "Depth in voxels for smart_arch or pond")),
                ("radius", prop("integer", "Radius in voxels for pond or tree foliage")),
                ("crenelated", prop("boolean", "Whether curve_wall includes battlements at the top")),
                ("count", prop("integer", "Maximum number of ivy leaves to grow")),
                ("color", prop("integer", "Palette index (0..=255, default: selected color)")),
                ("stone_color", prop("integer", "Palette index for path stones (default: color or selected)")),
                ("dirt_color", prop("integer", "Palette index for path dirt edges (default: 45)")),
                ("wood_color", prop("integer", "Palette index for tree trunk or fence timber (default: 78)")),
                ("foliage_color", prop("integer", "Palette index for tree foliage (default: color or selected)")),
            ],
            &["action"],
        ),
        tool(
            "architecture_generate",
            "Procedural architectural element generator for buildings and structures. \
             Actions: \
             - roof: generate procedural roofs over a bounding box (min, max, roof_type: gabled/hipped/mansard/conical, color, overhang). \
             - window: carve window cavity, outer frame, recessed glass pane, and optional exterior shutters (min, max, frame_color, glass_color, shutters). \
             - door: carve doorway with timber/stone chambranle and wooden door panel (origin, width, height, frame_color, door_color). \
             - chimney: brick chimney stack with hollow central flue and decorative chimney pot (origin, height, width, color).",
            vec![
                (
                    "action",
                    prop_enum(
                        "Architecture action to execute",
                        &["roof", "window", "door", "chimney"],
                    ),
                ),
                ("min", prop_ivec3("Lower corner [x,y,z] for roof or window")),
                ("max", prop_ivec3("Upper corner [x,y,z] for roof or window")),
                ("origin", prop_ivec3("Origin corner [x,y,z] for door or chimney")),
                (
                    "roof_type",
                    prop_enum(
                        "Type of roof to generate",
                        &["gabled", "hipped", "mansard", "conical"],
                    ),
                ),
                ("overhang", prop("integer", "Eave overhang extension in voxels (default 1)")),
                ("width", prop("integer", "Width in voxels for door or chimney")),
                ("height", prop("integer", "Height in voxels for door or chimney")),
                ("shutters", prop("boolean", "Whether to place exterior window shutters")),
                ("color", prop("integer", "Main palette index (0..=255, default: selected color)")),
                ("frame_color", prop("integer", "Palette index for frame / chambranle (default: color or selected)")),
                ("glass_color", prop("integer", "Palette index for glass pane (default: 79)")),
                ("door_color", prop("integer", "Palette index for door panel (default: 78)")),
                ("brick_color", prop("integer", "Palette index for brick masonry (default: color or selected)")),
            ],
            &["action"],
        ),
        tool(
            "light_manage",
            "Manage point lights and atmospheric illumination props in the scene. \
             Actions: \
             - add: add an omnidirectional point light (pos [x,y,z], radius, color [r,g,b], intensity). \
             - remove: remove a point light by index (idx). \
             - clear: remove all point lights from the scene. \
             - list: list all point lights in the scene. \
             - spawn_lantern: place a stylized voxel lantern prop on a wall, ceiling, or ground (pos, wall_normal, lantern_color, light_color, add_light).",
            vec![
                (
                    "action",
                    prop_enum(
                        "Light action to execute",
                        &["add", "remove", "clear", "list", "spawn_lantern"],
                    ),
                ),
                ("pos", prop_vec3("3D position [x,y,z] for light or lantern")),
                ("radius", prop("number", "Light radius in voxels (default 10.0)")),
                ("color", prop("array", "Linear RGB color [r,g,b] in [0.0, 1.0] (default [1.0, 0.78, 0.45])")),
                ("intensity", prop("number", "Luminous intensity multiplier (default 1.0)")),
                ("idx", prop("integer", "Point light index to remove")),
                ("wall_normal", prop_ivec3("Surface normal [x,y,z] for lantern attachment (default [0,0,1])")),
                ("lantern_color", prop("integer", "Palette index for lantern iron frame (default: 240)")),
                ("light_color", prop("integer", "Palette index for glowing lantern core (default: 19)")),
                ("add_light", prop("boolean", "Also add a point light at lantern position (default true)")),
            ],
            &["action"],
        ),
        tool(
            "gizmo_transform",
            "Transformations and extrusion for interactive building. \
             Actions: \
             - extrude: extrude outer solid faces or active selection along an axis (axis: x|y|z, amount). \
             - translate: move the active selection or scene nodes with optional grid snapping (axis?, delta: int or [dx,dy,dz], snap?, node?). \
             - snap_config: configure grid snapping settings (step?, enabled?).",
            vec![
                (
                    "action",
                    prop_enum(
                        "Transformation action to execute",
                        &["extrude", "translate", "snap_config"],
                    ),
                ),
                ("axis", prop_enum("Axis for extrusion or translation", &["x", "y", "z"])),
                ("amount", prop("integer", "Extrusion distance in voxels (positive or negative)")),
                (
                    "delta",
                    prop(
                        "integer",
                        "Translation delta (single integer if axis given, or array [dx,dy,dz])",
                    ),
                ),
                ("snap", prop("integer", "Grid snap interval in voxels (e.g. 1, 2, 4, 8)")),
                ("node", prop("integer", "Scene node ID to translate in world space (optional)")),
                ("step", prop("integer", "Snap step in voxels for snap_config")),
                ("enabled", prop("boolean", "Enable/disable snapping in snap_config")),
            ],
            &["action"],
        ),
    ]
}

pub fn dispatch(session: &mut McpSession, name: &str, args: &JsonValue) -> Option<ToolResult> {
    Some(match name {
        "smart_brush" => smart_brush(session, args),
        "architecture_generate" => architecture_generate(session, args),
        "light_manage" => light_manage(session, args),
        "gizmo_transform" => gizmo_transform(session, args),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn with_grid(
    session: &mut McpSession,
    f: impl FnOnce(&mut VoxelGrid) -> Result<String, String>,
) -> ToolResult {
    let mut grid = session.project.grid.clone();
    let msg = f(&mut grid)?;
    session.project.record_undo();
    session.project.grid = grid;
    session.project.dirty = true;
    Ok(vec![text(msg)])
}

fn default_color(session: &McpSession) -> u8 {
    session.project.palette.selected.max(1)
}

fn color_arg(args: &JsonValue, key: &str, default: u8) -> Result<u8, String> {
    match args.get(key) {
        None | Some(JsonValue::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|c| *c <= 255)
            .map(|c| c as u8)
            .ok_or_else(|| format!("'{key}' must be an integer palette index 0..=255")),
    }
}

fn ivec3_arg(args: &JsonValue, key: &str) -> Result<IVec3, String> {
    let v = args
        .get_ivec3(key)
        .ok_or_else(|| format!("'{key}' must be an array of 3 integers [x,y,z]"))?;
    if v.iter().any(|c| c.abs() > COORD_LIMIT) {
        return Err(format!("'{key}' coordinates must be within +/-{COORD_LIMIT}"));
    }
    Ok(IVec3::from_array(v))
}

fn vec3_arg(args: &JsonValue, key: &str) -> Result<Vec3, String> {
    let v = args
        .get_vec3(key)
        .ok_or_else(|| format!("'{key}' must be an array of 3 numbers [x,y,z]"))?;
    if v.iter().any(|c| !c.is_finite() || c.abs() > COORD_LIMIT as f32) {
        return Err(format!("'{key}' coordinates must be finite and within +/-{COORD_LIMIT}"));
    }
    Ok(Vec3::from_array(v))
}

fn parse_vec3_points(args: &JsonValue, key: &str) -> Result<Vec<Vec3>, String> {
    let arr = args
        .get_array(key)
        .ok_or_else(|| format!("Missing required array argument '{key}'"))?;
    let mut points = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        if let Some(coords) = item.as_array() {
            if coords.len() < 3 {
                return Err(format!("Point #{i} in '{key}' must have at least 3 coordinates [x, y, z]"));
            }
            let x = coords[0].as_f64().ok_or_else(|| format!("Invalid x in point #{i}"))? as f32;
            let y = coords[1].as_f64().ok_or_else(|| format!("Invalid y in point #{i}"))? as f32;
            let z = coords[2].as_f64().ok_or_else(|| format!("Invalid z in point #{i}"))? as f32;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(format!("Coordinates in point #{i} must be finite"));
            }
            points.push(Vec3::new(x, y, z));
        } else {
            return Err(format!("Item #{i} in '{key}' must be an array [x,y,z]"));
        }
    }
    Ok(points)
}

fn parse_ivec3_points(args: &JsonValue, key: &str) -> Result<Vec<IVec3>, String> {
    let arr = args
        .get_array(key)
        .ok_or_else(|| format!("Missing required array argument '{key}'"))?;
    let mut points = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        if let Some(coords) = item.as_array() {
            if coords.len() < 3 {
                return Err(format!("Point #{i} in '{key}' must have at least 3 coordinates [x, y, z]"));
            }
            let x = coords[0].as_i64().ok_or_else(|| format!("Invalid x in point #{i}"))? as i32;
            let y = coords[1].as_i64().ok_or_else(|| format!("Invalid y in point #{i}"))? as i32;
            let z = coords[2].as_i64().ok_or_else(|| format!("Invalid z in point #{i}"))? as i32;
            points.push(IVec3::new(x, y, z));
        } else {
            return Err(format!("Item #{i} in '{key}' must be an array [x,y,z]"));
        }
    }
    Ok(points)
}

fn parse_axis(s: &str) -> Result<Axis, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "z" => Ok(Axis::Z),
        _ => Err(format!("Unknown axis '{s}' (expected 'x', 'y' or 'z')")),
    }
}

// ---------------------------------------------------------------------------
// Tool: smart_brush
// ---------------------------------------------------------------------------

fn smart_brush(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or_else(|| "Missing required argument 'action'".to_string())?;

    let def_col = default_color(session);

    match action {
        "curve_wall" => {
            let points = parse_vec3_points(args, "points")?;
            if points.len() < 2 {
                return Err("'curve_wall' requires at least 2 control points in 'points'".to_string());
            }
            let thickness = args.get_u32("thickness").unwrap_or(1);
            let height = args.get_u32("height").unwrap_or(4);
            let crenelated = args.get_bool("crenelated").unwrap_or(false);
            let color = color_arg(args, "color", def_col)?;

            with_grid(session, |grid| {
                generate_spline_wall(grid, &points, thickness, height, crenelated, color);
                Ok(format!(
                    "Generated spline wall along {} points (thickness: {thickness}, height: {height}, crenelated: {crenelated}).",
                    points.len()
                ))
            })
        }
        "smart_arch" => {
            let center = ivec3_arg(args, "center")?;
            let width = args
                .get_u32("width")
                .ok_or_else(|| "Missing required argument 'width'".to_string())?;
            let height = args
                .get_u32("height")
                .ok_or_else(|| "Missing required argument 'height'".to_string())?;
            let depth = args.get_u32("depth").unwrap_or(1);
            let color = color_arg(args, "color", def_col)?;

            with_grid(session, |grid| {
                carve_arch(grid, center, width, height, depth, color);
                Ok(format!(
                    "Carved smart arch at [{}, {}, {}] (width: {width}, height: {height}, depth: {depth}).",
                    center.x, center.y, center.z
                ))
            })
        }
        "fence" => {
            let points = parse_vec3_points(args, "points")?;
            if points.len() < 2 {
                return Err("'fence' requires at least 2 control points in 'points'".to_string());
            }
            let height = args.get_u32("height").unwrap_or(2);
            let wood_color = color_arg(args, "wood_color", color_arg(args, "color", def_col)?)?;

            with_grid(session, |grid| {
                generate_fence(grid, &points, height, wood_color);
                Ok(format!(
                    "Generated wooden fence along {} points (height: {height}).",
                    points.len()
                ))
            })
        }
        "paint_path" => {
            let points = parse_vec3_points(args, "points")?;
            if points.is_empty() {
                return Err("'paint_path' requires at least 1 point in 'points'".to_string());
            }
            let width = args.get_u32("width").unwrap_or(3);
            let stone_color = color_arg(args, "stone_color", color_arg(args, "color", def_col)?)?;
            let dirt_color = color_arg(args, "dirt_color", 45)?;

            with_grid(session, |grid| {
                paint_path(grid, &points, width, stone_color, dirt_color);
                Ok(format!(
                    "Painted organic pathway along {} points (width: {width}).",
                    points.len()
                ))
            })
        }
        "grow_ivy" => {
            let seed_voxels = if args.get("voxels").is_some() {
                parse_ivec3_points(args, "voxels")?
            } else if args.get("points").is_some() {
                parse_ivec3_points(args, "points")?
            } else {
                return Err("Missing required array 'voxels' for 'grow_ivy'".to_string());
            };
            if seed_voxels.is_empty() {
                return Err("'grow_ivy' requires at least 1 seed voxel".to_string());
            }
            let count = args.get_u32("count").unwrap_or(40);
            let leaf_color = color_arg(args, "color", def_col)?;

            with_grid(session, |grid| {
                grow_ivy(grid, &seed_voxels, count, leaf_color);
                Ok(format!(
                    "Grew ivy foliage from {} seed points (max count: {count}).",
                    seed_voxels.len()
                ))
            })
        }
        "pond" => {
            let center = ivec3_arg(args, "center")?;
            let radius = args
                .get_u32("radius")
                .ok_or_else(|| "Missing required argument 'radius'".to_string())?;
            let depth = args
                .get_u32("depth")
                .ok_or_else(|| "Missing required argument 'depth'".to_string())?;
            let water_color = color_arg(args, "color", def_col)?;

            with_grid(session, |grid| {
                carve_pond(grid, center, radius, depth, water_color);
                Ok(format!(
                    "Carved pond at [{}, {}, {}] (radius: {radius}, depth: {depth}).",
                    center.x, center.y, center.z
                ))
            })
        }
        "tree" => {
            let base = ivec3_arg(args, "base")?;
            let trunk_height = args.get_u32("height").unwrap_or(6);
            let crown_radius = args.get_u32("radius").unwrap_or(4);
            let wood_color = color_arg(args, "wood_color", 78)?;
            let foliage_color =
                color_arg(args, "foliage_color", color_arg(args, "color", def_col)?)?;

            with_grid(session, |grid| {
                generate_fluffy_tree(
                    grid,
                    base,
                    trunk_height,
                    crown_radius,
                    wood_color,
                    foliage_color,
                );
                Ok(format!(
                    "Generated fluffy tree at [{}, {}, {}] (trunk height: {trunk_height}, crown radius: {crown_radius}).",
                    base.x, base.y, base.z
                ))
            })
        }
        other => Err(format!(
            "Unknown action '{other}' for 'smart_brush' (expected curve_wall, smart_arch, fence, paint_path, grow_ivy, pond, tree)"
        )),
    }
}

// ---------------------------------------------------------------------------
// Tool: architecture_generate
// ---------------------------------------------------------------------------

fn architecture_generate(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or_else(|| "Missing required argument 'action'".to_string())?;

    let def_col = default_color(session);

    match action {
        "roof" => {
            let min = ivec3_arg(args, "min")?;
            let max = ivec3_arg(args, "max")?;
            let roof_type_str = args
                .get_str("roof_type")
                .or_else(|| args.get_str("type"))
                .unwrap_or("gabled");
            let roof_type = RoofType::from_str(roof_type_str).ok_or_else(|| {
                format!(
                    "Invalid roof_type '{roof_type_str}' (expected gabled, hipped, mansard, or conical)"
                )
            })?;
            let color = color_arg(args, "color", def_col)?;
            let overhang = args.get_u32("overhang").unwrap_or(1);

            with_grid(session, |grid| {
                generate_roof(grid, min, max, roof_type, color, overhang);
                Ok(format!(
                    "Generated {:?} roof over [{min}] to [{max}] (overhang: {overhang}).",
                    roof_type
                ))
            })
        }
        "window" => {
            let min = ivec3_arg(args, "min")?;
            let max = ivec3_arg(args, "max")?;
            let frame_color = color_arg(args, "frame_color", color_arg(args, "color", def_col)?)?;
            let glass_color = color_arg(args, "glass_color", 79)?;
            let shutters = args.get_bool("shutters").unwrap_or(false);

            with_grid(session, |grid| {
                punch_window(grid, min, max, frame_color, glass_color, shutters);
                Ok(format!(
                    "Punched window aperture from [{min}] to [{max}] (shutters: {shutters})."
                ))
            })
        }
        "door" => {
            let origin = ivec3_arg(args, "origin")?;
            let (width, height) = if let Some(dims) = args.get_array("dimensions") {
                if dims.len() >= 2 {
                    let w = dims[0].as_u64().unwrap_or(3) as u32;
                    let h = dims[1].as_u64().unwrap_or(5) as u32;
                    (w, h)
                } else {
                    (3, 5)
                }
            } else {
                let w = args.get_u32("width").unwrap_or(3);
                let h = args.get_u32("height").unwrap_or(5);
                (w, h)
            };
            let frame_color = color_arg(args, "frame_color", color_arg(args, "color", def_col)?)?;
            let door_color = color_arg(args, "door_color", 78)?;

            with_grid(session, |grid| {
                punch_door(grid, origin, width, height, frame_color, door_color);
                Ok(format!(
                    "Punched doorway at [{origin}] (width: {width}, height: {height})."
                ))
            })
        }
        "chimney" => {
            let origin = ivec3_arg(args, "origin")?;
            let height = args.get_u32("height").unwrap_or(8);
            let width = args.get_u32("width").unwrap_or(3);
            let brick_color = color_arg(args, "brick_color", color_arg(args, "color", def_col)?)?;

            with_grid(session, |grid| {
                build_chimney(grid, origin, height, width, brick_color);
                Ok(format!(
                    "Built chimney stack at [{origin}] (height: {height}, width: {width})."
                ))
            })
        }
        other => Err(format!(
            "Unknown action '{other}' for 'architecture_generate' (expected roof, window, door, chimney)"
        )),
    }
}

// ---------------------------------------------------------------------------
// Tool: light_manage
// ---------------------------------------------------------------------------

fn light_manage(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or_else(|| "Missing required argument 'action'".to_string())?;

    match action {
        "add" => {
            let pos = if let Ok(p) = vec3_arg(args, "pos") {
                p
            } else {
                vec3_arg(args, "position")?
            };
            let radius = args.get_f32("radius").unwrap_or(10.0);
            let intensity = args.get_f32("intensity").unwrap_or(1.0);

            let color = if let Some(arr) = args.get_array("color") {
                if arr.len() >= 3 {
                    let r = arr[0].as_f64().unwrap_or(1.0) as f32;
                    let g = arr[1].as_f64().unwrap_or(0.78) as f32;
                    let b = arr[2].as_f64().unwrap_or(0.45) as f32;
                    [r, g, b]
                } else {
                    [1.0, 0.78, 0.45]
                }
            } else {
                [1.0, 0.78, 0.45]
            };

            let light = PointLight::new(pos, radius, color, intensity);
            let idx = session.project.add_light(light);
            Ok(vec![text(format!(
                "Added point light #{idx} at [{}, {}, {}] (radius: {radius:.1}, intensity: {intensity:.1}).",
                pos.x, pos.y, pos.z
            ))])
        }
        "remove" => {
            let idx = args
                .get_usize("idx")
                .or_else(|| args.get_usize("index"))
                .ok_or_else(|| "Missing required integer argument 'idx'".to_string())?;

            if let Some(removed) = session.project.remove_light(idx) {
                Ok(vec![text(format!(
                    "Removed point light #{idx} at [{:.1}, {:.1}, {:.1}].",
                    removed.position.x, removed.position.y, removed.position.z
                ))])
            } else {
                Err(format!(
                    "Light index #{idx} not found (scene contains {} lights)",
                    session.project.lights.len()
                ))
            }
        }
        "clear" => {
            let count = session.project.lights.len();
            session.project.clear_lights();
            Ok(vec![text(format!("Cleared {count} point light(s)."))])
        }
        "list" => {
            let lights = &session.project.lights;
            if lights.is_empty() {
                return Ok(vec![text("No point lights in scene.")]);
            }
            let mut desc = format!("Scene contains {} point light(s):\n", lights.len());
            for (i, l) in lights.iter().enumerate() {
                desc.push_str(&format!(
                    "  #{i}: pos=[{:.2}, {:.2}, {:.2}], r={:.1}, i={:.2}, rgb=[{:.2}, {:.2}, {:.2}]\n",
                    l.position.x, l.position.y, l.position.z, l.radius, l.intensity,
                    l.color[0], l.color[1], l.color[2]
                ));
            }
            Ok(vec![text(desc.trim_end().to_string())])
        }
        "spawn_lantern" => {
            let pos = if let Ok(p) = ivec3_arg(args, "pos") {
                p
            } else {
                ivec3_arg(args, "position")?
            };
            let wall_normal = ivec3_arg(args, "wall_normal").unwrap_or(IVec3::new(0, 0, 1));
            let lantern_color = color_arg(args, "lantern_color", 240)?;
            let light_color = color_arg(args, "light_color", 19)?;
            let add_point_light = args.get_bool("add_light").unwrap_or(true);

            with_grid(session, |grid| {
                spawn_lantern(grid, pos, wall_normal, lantern_color, light_color);
                Ok(format!(
                    "Spawned lantern prop at [{pos}] with normal [{wall_normal}]."
                ))
            })?;

            if add_point_light {
                let light_pos = pos.as_vec3() + wall_normal.as_vec3() * 0.5;
                let idx = session
                    .project
                    .add_light(PointLight::warm(light_pos, 8.0, 1.2));
                Ok(vec![text(format!(
                    "Spawned lantern prop at [{pos}] with wall_normal [{wall_normal}] and added warm point light #{idx}."
                ))])
            } else {
                Ok(vec![text(format!(
                    "Spawned lantern prop at [{pos}] with wall_normal [{wall_normal}]."
                ))])
            }
        }
        other => Err(format!(
            "Unknown action '{other}' for 'light_manage' (expected add, remove, clear, list, spawn_lantern)"
        )),
    }
}

// ---------------------------------------------------------------------------
// Tool: gizmo_transform
// ---------------------------------------------------------------------------

fn gizmo_transform(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or_else(|| "Missing required argument 'action'".to_string())?;

    match action {
        "extrude" => {
            let axis_str = args
                .get_str("axis")
                .ok_or_else(|| "Missing required argument 'axis' (x|y|z)".to_string())?;
            let axis = parse_axis(axis_str)?;
            let amount = args
                .get_i32("amount")
                .ok_or_else(|| "Missing required integer argument 'amount'".to_string())?;

            if amount == 0 {
                return Ok(vec![text("Extrusion amount is 0 (no-op).")]);
            }

            session.project.record_undo();

            if session.project.selection.any_selected() {
                let dir = match axis {
                    Axis::X => IVec3::new(amount.signum(), 0, 0),
                    Axis::Y => IVec3::new(0, amount.signum(), 0),
                    Axis::Z => IVec3::new(0, 0, amount.signum()),
                };
                let steps = amount.abs();
                let mut moved = 0;
                for step in 1..=steps {
                    for z in 0..session.project.grid.size_z() {
                        for y in 0..session.project.grid.size_y() {
                            for x in 0..session.project.grid.size_x() {
                                if session.project.selection.get(x, y, z) {
                                    let col = session.project.grid.get(x, y, z);
                                    if col != 0 {
                                        let target =
                                            IVec3::new(x as i32, y as i32, z as i32) + dir * step;
                                        if session.project.grid.in_bounds(
                                            target.x, target.y, target.z,
                                        ) {
                                            session.project.grid.set_i32(
                                                target.x, target.y, target.z, col,
                                            );
                                            session.project.selection.set(
                                                target.x as u32,
                                                target.y as u32,
                                                target.z as u32,
                                                true,
                                            );
                                            moved += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                session.project.dirty = true;
                Ok(vec![text(format!(
                    "Extruded selection along {:?} by {} ({} voxels affected).",
                    axis, amount, moved
                ))])
            } else {
                session.project.grid.extrude(axis, amount);
                session.project.dirty = true;
                Ok(vec![text(format!(
                    "Extruded outer face along {:?} by {}.",
                    axis, amount
                ))])
            }
        }
        "translate" => {
            let snap_val = if let Some(s) = args.get_u32("snap") {
                Some(s)
            } else if session.snap_enabled {
                Some(session.snap_step)
            } else {
                None
            };

            let delta = if let Some(axis_str) = args.get_str("axis") {
                let axis = parse_axis(axis_str)?;
                let d = args
                    .get_i32("delta")
                    .ok_or_else(|| "Missing integer argument 'delta'".to_string())?;
                match axis {
                    Axis::X => IVec3::new(d, 0, 0),
                    Axis::Y => IVec3::new(0, d, 0),
                    Axis::Z => IVec3::new(0, 0, d),
                }
            } else if let Some(arr) = args.get_array("delta") {
                if arr.len() < 3 {
                    return Err("'delta' array must have 3 integers [dx, dy, dz]".to_string());
                }
                let dx = arr[0].as_i64().ok_or_else(|| "Invalid dx")? as i32;
                let dy = arr[1].as_i64().ok_or_else(|| "Invalid dy")? as i32;
                let dz = arr[2].as_i64().ok_or_else(|| "Invalid dz")? as i32;
                IVec3::new(dx, dy, dz)
            } else {
                return Err("Missing 'delta' (integer along 'axis', or [dx, dy, dz])".to_string());
            };

            // Apply grid snapping if requested
            let snapped_delta = match snap_val {
                Some(s) if s > 1 => {
                    let step = s as i32;
                    let snap_coord = |v: i32| -> i32 {
                        let rem = v.rem_euclid(step);
                        if rem >= (step + 1) / 2 {
                            v + (step - rem)
                        } else {
                            v - rem
                        }
                    };
                    IVec3::new(
                        snap_coord(delta.x),
                        snap_coord(delta.y),
                        snap_coord(delta.z),
                    )
                }
                _ => delta,
            };

            if snapped_delta == IVec3::ZERO {
                return Ok(vec![text(
                    "Translation delta is zero (or snapped to 0).",
                )]);
            }

            // Check if translating specific scene node(s)
            let target_node: Option<NodeId> = args
                .get("node")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);

            if let Some(node_id) = target_node {
                let p = &mut session.project;
                let before = p.scene.structure();
                let depth = p.world_undo.len();
                p.record_world_undo();
                match p.scene.translate_world(node_id, snapped_delta) {
                    Ok(()) => {
                        p.dirty = true;
                        Ok(vec![text(format!(
                            "Translated scene node #{node_id} by [{}, {}, {}].",
                            snapped_delta.x, snapped_delta.y, snapped_delta.z
                        ))])
                    }
                    Err(e) => {
                        p.scene.restore_structure(before);
                        p.world_undo.truncate(depth);
                        Err(format!("Cannot translate node #{node_id}: {e}"))
                    }
                }
            } else if session.project.selection.any_selected() {
                // Translate selection within model grid
                let p = &mut session.project;
                let sx = p.grid.size_x() as i32;
                let sy = p.grid.size_y() as i32;
                let sz = p.grid.size_z() as i32;

                let mut voxels = Vec::new();
                for z in 0..sz {
                    for y in 0..sy {
                        for x in 0..sx {
                            if p.selection.get(x as u32, y as u32, z as u32) {
                                let c = p.grid.get(x as u32, y as u32, z as u32);
                                if c != 0 {
                                    voxels.push((IVec3::new(x, y, z), c));
                                }
                            }
                        }
                    }
                }

                p.record_undo();

                for &(pos, _) in &voxels {
                    p.grid.set_i32(pos.x, pos.y, pos.z, 0);
                    p.selection.set(pos.x as u32, pos.y as u32, pos.z as u32, false);
                }

                let mut moved = 0;
                for (pos, col) in voxels {
                    let dest = pos + snapped_delta;
                    if dest.x >= 0
                        && dest.x < sx
                        && dest.y >= 0
                        && dest.y < sy
                        && dest.z >= 0
                        && dest.z < sz
                    {
                        p.grid.set_i32(dest.x, dest.y, dest.z, col);
                        p.selection
                            .set(dest.x as u32, dest.y as u32, dest.z as u32, true);
                        moved += 1;
                    }
                }

                p.dirty = true;
                Ok(vec![text(format!(
                    "Translated selection by [{}, {}, {}] ({} voxels moved).",
                    snapped_delta.x, snapped_delta.y, snapped_delta.z, moved
                ))])
            } else if let Some(active_node) = session.project.active_shape {
                // If in world mode / active shape present, translate active scene shape
                let p = &mut session.project;
                let before = p.scene.structure();
                let depth = p.world_undo.len();
                p.record_world_undo();
                match p.scene.translate_world(active_node, snapped_delta) {
                    Ok(()) => {
                        p.dirty = true;
                        Ok(vec![text(format!(
                            "Translated active scene node #{active_node} by [{}, {}, {}].",
                            snapped_delta.x, snapped_delta.y, snapped_delta.z
                        ))])
                    }
                    Err(e) => {
                        p.scene.restore_structure(before);
                        p.world_undo.truncate(depth);
                        Err(format!("Cannot translate active shape: {e}"))
                    }
                }
            } else {
                // Loop shift model grid
                session.project.record_undo();
                session.project.grid.loop_shift(snapped_delta);
                session.project.dirty = true;
                Ok(vec![text(format!(
                    "Shifted model grid by [{}, {}, {}].",
                    snapped_delta.x, snapped_delta.y, snapped_delta.z
                ))])
            }
        }
        "snap_config" => {
            if let Some(step) = args.get_u32("step") {
                session.snap_step = step.clamp(1, 256);
            }
            if let Some(en) = args.get_bool("enabled") {
                session.snap_enabled = en;
            }
            let res = JsonValue::object(vec![
                ("snap_step", JsonValue::uint(session.snap_step as usize)),
                ("snap_enabled", JsonValue::bool(session.snap_enabled)),
            ]);
            Ok(vec![text(res.to_json_string())])
        }
        other => Err(format!(
            "Unknown action '{other}' for 'gizmo_transform' (expected extrude, translate, snap_config)"
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definitions_are_well_formed() {
        let defs = definitions();
        assert_eq!(defs.len(), 4);
        let names: Vec<&str> = defs
            .iter()
            .map(|d| d.get_str("name").expect("Tool has name"))
            .collect();
        assert!(names.contains(&"smart_brush"));
        assert!(names.contains(&"architecture_generate"));
        assert!(names.contains(&"light_manage"));
        assert!(names.contains(&"gizmo_transform"));
    }

    #[test]
    fn test_smart_brush_actions_and_undo() {
        let mut session = McpSession::new();
        assert_eq!(session.project.grid.count_solid(), 0);

        // 1. curve_wall
        let wall_args = JsonValue::object(vec![
            ("action", JsonValue::str("curve_wall")),
            (
                "points",
                JsonValue::array(vec![
                    JsonValue::array(vec![JsonValue::int(5), JsonValue::int(5), JsonValue::int(0)]),
                    JsonValue::array(vec![JsonValue::int(15), JsonValue::int(5), JsonValue::int(0)]),
                    JsonValue::array(vec![JsonValue::int(25), JsonValue::int(15), JsonValue::int(0)]),
                ]),
            ),
            ("thickness", JsonValue::int(2)),
            ("height", JsonValue::int(5)),
            ("color", JsonValue::int(7)),
        ]);
        let res = smart_brush(&mut session, &wall_args).expect("curve_wall failed");
        assert!(session.project.grid.count_solid() > 0);
        assert!(res[0].get_str("text").unwrap().contains("spline wall"));

        // Test Undo
        assert!(session.project.undo());
        assert_eq!(session.project.grid.count_solid(), 0);
        assert!(session.project.redo());
        assert!(session.project.grid.count_solid() > 0);

        // 2. pond
        let pond_args = JsonValue::object(vec![
            ("action", JsonValue::str("pond")),
            ("center", JsonValue::array(vec![JsonValue::int(20), JsonValue::int(20), JsonValue::int(5)])),
            ("radius", JsonValue::int(4)),
            ("depth", JsonValue::int(3)),
            ("color", JsonValue::int(12)),
        ]);
        smart_brush(&mut session, &pond_args).expect("pond failed");

        // 3. tree
        let tree_args = JsonValue::object(vec![
            ("action", JsonValue::str("tree")),
            ("base", JsonValue::array(vec![JsonValue::int(30), JsonValue::int(30), JsonValue::int(0)])),
            ("height", JsonValue::int(5)),
            ("radius", JsonValue::int(3)),
            ("wood_color", JsonValue::int(78)),
            ("foliage_color", JsonValue::int(105)),
        ]);
        smart_brush(&mut session, &tree_args).expect("tree failed");

        // 4. fence
        let fence_args = JsonValue::object(vec![
            ("action", JsonValue::str("fence")),
            (
                "points",
                JsonValue::array(vec![
                    JsonValue::array(vec![JsonValue::int(0), JsonValue::int(0), JsonValue::int(0)]),
                    JsonValue::array(vec![JsonValue::int(0), JsonValue::int(10), JsonValue::int(0)]),
                ]),
            ),
            ("height", JsonValue::int(2)),
            ("wood_color", JsonValue::int(78)),
        ]);
        smart_brush(&mut session, &fence_args).expect("fence failed");

        // 5. smart_arch
        let arch_args = JsonValue::object(vec![
            ("action", JsonValue::str("smart_arch")),
            ("center", JsonValue::array(vec![JsonValue::int(15), JsonValue::int(5), JsonValue::int(0)])),
            ("width", JsonValue::int(6)),
            ("height", JsonValue::int(6)),
            ("depth", JsonValue::int(2)),
            ("color", JsonValue::int(20)),
        ]);
        smart_brush(&mut session, &arch_args).expect("smart_arch failed");

        // 6. paint_path
        let path_args = JsonValue::object(vec![
            ("action", JsonValue::str("paint_path")),
            (
                "points",
                JsonValue::array(vec![
                    JsonValue::array(vec![JsonValue::int(0), JsonValue::int(2), JsonValue::int(0)]),
                    JsonValue::array(vec![JsonValue::int(10), JsonValue::int(2), JsonValue::int(0)]),
                ]),
            ),
            ("width", JsonValue::int(3)),
            ("stone_color", JsonValue::int(15)),
            ("dirt_color", JsonValue::int(45)),
        ]);
        smart_brush(&mut session, &path_args).expect("paint_path failed");

        // 7. grow_ivy
        let ivy_args = JsonValue::object(vec![
            ("action", JsonValue::str("grow_ivy")),
            (
                "voxels",
                JsonValue::array(vec![
                    JsonValue::array(vec![JsonValue::int(6), JsonValue::int(5), JsonValue::int(1)]),
                ]),
            ),
            ("count", JsonValue::int(15)),
            ("color", JsonValue::int(105)),
        ]);
        smart_brush(&mut session, &ivy_args).expect("grow_ivy failed");

        // Error handling & atomic execution
        let bad_args = JsonValue::object(vec![
            ("action", JsonValue::str("curve_wall")),
            ("points", JsonValue::array(vec![])), // empty points
        ]);
        let grid_count_before = session.project.grid.count_solid();
        assert!(smart_brush(&mut session, &bad_args).is_err());
        assert_eq!(session.project.grid.count_solid(), grid_count_before);

        let unknown_action = JsonValue::object(vec![
            ("action", JsonValue::str("nonexistent")),
        ]);
        assert!(smart_brush(&mut session, &unknown_action).is_err());
    }

    #[test]
    fn test_architecture_generate_and_undo() {
        let mut session = McpSession::new();

        // 1. roof (test different roof types: gabled, hipped, mansard, conical)
        for (rtype, z_base) in [
            ("gabled", 0),
            ("hipped", 8),
            ("mansard", 16),
            ("conical", 24),
        ] {
            let roof_args = JsonValue::object(vec![
                ("action", JsonValue::str("roof")),
                ("min", JsonValue::array(vec![JsonValue::int(2), JsonValue::int(2), JsonValue::int(z_base)])),
                ("max", JsonValue::array(vec![JsonValue::int(10), JsonValue::int(10), JsonValue::int(z_base + 6)])),
                ("roof_type", JsonValue::str(rtype)),
                ("color", JsonValue::int(45)),
                ("overhang", JsonValue::int(1)),
            ]);
            let res = architecture_generate(&mut session, &roof_args).expect("roof failed");
            assert!(res[0].get_str("text").unwrap().contains("roof"));
        }
        assert!(session.project.grid.count_solid() > 0);

        // 2. window
        let win_args = JsonValue::object(vec![
            ("action", JsonValue::str("window")),
            ("min", JsonValue::array(vec![JsonValue::int(10), JsonValue::int(5), JsonValue::int(2)])),
            ("max", JsonValue::array(vec![JsonValue::int(15), JsonValue::int(5), JsonValue::int(6)])),
            ("frame_color", JsonValue::int(80)),
            ("glass_color", JsonValue::int(79)),
            ("shutters", JsonValue::bool(true)),
        ]);
        architecture_generate(&mut session, &win_args).expect("window failed");

        // 3. door
        let door_args = JsonValue::object(vec![
            ("action", JsonValue::str("door")),
            ("origin", JsonValue::array(vec![JsonValue::int(8), JsonValue::int(5), JsonValue::int(0)])),
            ("dimensions", JsonValue::array(vec![JsonValue::int(3), JsonValue::int(5)])),
            ("frame_color", JsonValue::int(80)),
            ("door_color", JsonValue::int(78)),
        ]);
        architecture_generate(&mut session, &door_args).expect("door failed");

        // 4. chimney
        let chim_args = JsonValue::object(vec![
            ("action", JsonValue::str("chimney")),
            ("origin", JsonValue::array(vec![JsonValue::int(12), JsonValue::int(12), JsonValue::int(14)])),
            ("height", JsonValue::int(6)),
            ("width", JsonValue::int(3)),
            ("brick_color", JsonValue::int(145)),
        ]);
        architecture_generate(&mut session, &chim_args).expect("chimney failed");

        // Error handling & atomic execution
        let bad_roof = JsonValue::object(vec![
            ("action", JsonValue::str("roof")),
            ("min", JsonValue::array(vec![JsonValue::int(2), JsonValue::int(2), JsonValue::int(0)])),
            ("max", JsonValue::array(vec![JsonValue::int(10), JsonValue::int(10), JsonValue::int(6)])),
            ("roof_type", JsonValue::str("invalid_profile")),
        ]);
        let count_before = session.project.grid.count_solid();
        assert!(architecture_generate(&mut session, &bad_roof).is_err());
        assert_eq!(session.project.grid.count_solid(), count_before);
    }

    #[test]
    fn test_light_manage_lifecycle() {
        let mut session = McpSession::new();
        assert_eq!(session.project.lights.len(), 0);

        // Add light
        let add_args = JsonValue::object(vec![
            ("action", JsonValue::str("add")),
            ("pos", JsonValue::array(vec![JsonValue::float(10.0), JsonValue::float(10.0), JsonValue::float(5.0)])),
            ("radius", JsonValue::float(12.5)),
            ("intensity", JsonValue::float(1.5)),
        ]);
        let res = light_manage(&mut session, &add_args).expect("add light failed");
        assert_eq!(session.project.lights.len(), 1);
        assert!(res[0].get_str("text").unwrap().contains("#0"));

        // List lights
        let list_args = JsonValue::object(vec![("action", JsonValue::str("list"))]);
        let list_res = light_manage(&mut session, &list_args).expect("list failed");
        assert!(list_res[0].get_str("text").unwrap().contains("Scene contains 1"));

        // Spawn lantern prop
        let lantern_args = JsonValue::object(vec![
            ("action", JsonValue::str("spawn_lantern")),
            ("pos", JsonValue::array(vec![JsonValue::int(5), JsonValue::int(5), JsonValue::int(5)])),
            ("wall_normal", JsonValue::array(vec![JsonValue::int(0), JsonValue::int(1), JsonValue::int(0)])),
            ("add_light", JsonValue::bool(true)),
        ]);
        light_manage(&mut session, &lantern_args).expect("spawn lantern failed");
        assert_eq!(session.project.lights.len(), 2);
        assert!(session.project.grid.count_solid() > 0);

        // Remove light #0
        let rem_args = JsonValue::object(vec![
            ("action", JsonValue::str("remove")),
            ("idx", JsonValue::int(0)),
        ]);
        light_manage(&mut session, &rem_args).expect("remove failed");
        assert_eq!(session.project.lights.len(), 1);

        // Clear all
        let clear_args = JsonValue::object(vec![("action", JsonValue::str("clear"))]);
        light_manage(&mut session, &clear_args).expect("clear failed");
        assert_eq!(session.project.lights.len(), 0);
    }

    #[test]
    fn test_gizmo_transform_extrude_and_translate() {
        let mut session = McpSession::new();
        // Place a small 2x2 cube
        session.project.grid.set(5, 5, 5, 7);
        session.project.grid.set(5, 6, 5, 7);
        session.project.grid.set(6, 5, 5, 7);
        session.project.grid.set(6, 6, 5, 7);
        let initial_count = session.project.grid.count_solid();
        assert_eq!(initial_count, 4);

        // Extrude along Z by +2
        let extrude_args = JsonValue::object(vec![
            ("action", JsonValue::str("extrude")),
            ("axis", JsonValue::str("z")),
            ("amount", JsonValue::int(2)),
        ]);
        gizmo_transform(&mut session, &extrude_args).expect("extrude failed");
        assert_eq!(session.project.grid.count_solid(), 12);

        // Test undo
        assert!(session.project.undo());
        assert_eq!(session.project.grid.count_solid(), initial_count);

        // Snap config test
        let snap_cfg = JsonValue::object(vec![
            ("action", JsonValue::str("snap_config")),
            ("step", JsonValue::int(4)),
            ("enabled", JsonValue::bool(true)),
        ]);
        let cfg_res = gizmo_transform(&mut session, &snap_cfg).expect("snap_config failed");
        assert!(cfg_res[0].get_str("text").unwrap().contains("\"snap_step\":4"));
        assert_eq!(session.snap_step, 4);
        assert!(session.snap_enabled);

        // Translate with snapping: delta 3 along X should snap to 4
        let trans_snap = JsonValue::object(vec![
            ("action", JsonValue::str("translate")),
            ("axis", JsonValue::str("x")),
            ("delta", JsonValue::int(3)),
            ("snap", JsonValue::int(4)),
        ]);
        let trans_res = gizmo_transform(&mut session, &trans_snap).expect("translate snap failed");
        assert!(trans_res[0].get_str("text").unwrap().contains("[4, 0, 0]"));

        // Translate with selection
        session.project.selection.set(9, 5, 5, true);
        let trans_sel = JsonValue::object(vec![
            ("action", JsonValue::str("translate")),
            ("delta", JsonValue::array(vec![JsonValue::int(2), JsonValue::int(0), JsonValue::int(0)])),
        ]);
        let sel_res = gizmo_transform(&mut session, &trans_sel).expect("translate selection failed");
        assert!(sel_res[0].get_str("text").unwrap().contains("Translated selection"));

        // Extrude with selection
        let extrude_sel = JsonValue::object(vec![
            ("action", JsonValue::str("extrude")),
            ("axis", JsonValue::str("y")),
            ("amount", JsonValue::int(2)),
        ]);
        let ext_sel_res = gizmo_transform(&mut session, &extrude_sel).expect("extrude selection failed");
        assert!(ext_sel_res[0].get_str("text").unwrap().contains("Extruded selection"));

        // Error handling & atomic execution
        let bad_extrude = JsonValue::object(vec![
            ("action", JsonValue::str("extrude")),
            ("axis", JsonValue::str("invalid_axis")),
            ("amount", JsonValue::int(5)),
        ]);
        assert!(gizmo_transform(&mut session, &bad_extrude).is_err());

        let bad_trans = JsonValue::object(vec![
            ("action", JsonValue::str("translate")),
            ("delta", JsonValue::array(vec![JsonValue::int(1)])), // too short
        ]);
        assert!(gizmo_transform(&mut session, &bad_trans).is_err());
    }
}
