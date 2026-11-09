//! MCP tools: voxel drawing on the edited model, palette and material management.

use glam::{IVec3, Vec3};
use mv_core::{Material, MaterialType, Rgba, VoxelGrid};

use crate::json::JsonValue;
use crate::primitives::{
    draw_box, draw_cylinder, draw_ellipsoid, draw_line, execute_batch_op, parse_axis,
};
use crate::session::McpSession;
use crate::tools::{prop, prop_enum, prop_ivec3, prop_vec3, text, tool, ToolResult};

const COORD_LIMIT: i32 = 100_000;
const GET_CAP: usize = 5000;

const AXES_NOTE: &str = "Axes are Z-up: X right, Y depth, Z up (ground z = 0). Coordinates outside the canvas are clipped. Color 0 erases; colors are palette indices 1..=255 (default: the selected palette color).";

pub fn definitions() -> Vec<JsonValue> {
    let color = || {
        prop(
            "integer",
            "Palette index 0..=255 (0 erases). Default: selected palette color",
        )
    };
    let hollow = || prop("boolean", "Only draw the shell (default false)");
    let wall = || prop("integer", "Shell thickness when hollow (default 1)");
    let d = |s: &str| format!("{s} {AXES_NOTE} One undo step.");
    vec![
        tool(
            "voxel_set",
            &d("Set individual voxels in the edited model."),
            vec![
                (
                    "voxels",
                    prop("array", "Array of [x,y,z], [x,y,z,color] or {x,y,z,color}"),
                ),
                ("color", color()),
            ],
            &["voxels"],
        ),
        tool(
            "voxel_box",
            &d("Draw an axis-aligned box between two inclusive corners."),
            vec![
                ("min", prop_ivec3("Corner [x,y,z]")),
                ("max", prop_ivec3("Opposite corner [x,y,z]")),
                ("color", color()),
                ("hollow", hollow()),
                ("wall_thickness", wall()),
            ],
            &["min", "max"],
        ),
        tool(
            "voxel_sphere",
            &d("Draw a sphere or ellipsoid."),
            vec![
                ("center", prop_vec3("Center [x,y,z]")),
                ("radius", prop("number", "Radius in voxels (sphere)")),
                ("radii", prop_vec3("Radii [rx,ry,rz] (ellipsoid), overrides radius")),
                ("color", color()),
                ("hollow", hollow()),
                ("wall_thickness", wall()),
            ],
            &["center"],
        ),
        tool(
            "voxel_cylinder",
            &d("Draw a cylinder, cone or frustum from a base cap extending `height` voxels along `axis`."),
            vec![
                ("center", prop_vec3("Center [x,y,z] of the base cap (lowest coordinate along axis)")),
                ("axis", prop_enum("Axis of the cylinder (default z = vertical)", &["x", "y", "z"])),
                ("height", prop("integer", "Length along the axis in voxels")),
                ("radius", prop("number", "Base radius")),
                ("radius_top", prop("number", "Top radius (default = radius; 0 makes a cone)")),
                ("color", color()),
                ("hollow", hollow()),
            ],
            &["center", "height", "radius"],
        ),
        tool(
            "voxel_line",
            &d("Draw a 3D line segment, optionally thick."),
            vec![
                ("start", prop_ivec3("Start [x,y,z]")),
                ("end", prop_ivec3("End [x,y,z]")),
                ("color", color()),
                ("radius", prop("integer", "Thickness radius (0 = one voxel wide, default 0)")),
            ],
            &["start", "end"],
        ),
        tool(
            "voxel_batch",
            &d("Run many drawing operations atomically (all or nothing, one undo step); preferred for building objects. `operations` is an array of objects, each with an `op` field and optional `color` (palette index, default selected) and `hollow`. Ops: \
{op:'set', voxels:[[x,y,z,color?],...]} or {op:'set', x,y,z}; \
{op:'box', min:[x,y,z], max:[x,y,z], hollow?, wall_thickness?}; \
{op:'sphere'|'ellipsoid', center:[x,y,z], radius? or radii:[rx,ry,rz], hollow?, wall_thickness?}; \
{op:'cylinder'|'cone', center:[x,y,z] (base cap), axis:'x'|'y'|'z' (default z), height, radius (or radius_bottom), radius_top? (cone defaults to 0), hollow?}; \
{op:'pyramid', min, max, axis? (apex direction, default z), hollow?}; \
{op:'torus', center, axis?, major_radius, tube_radius}; \
{op:'line', start:[x,y,z], end:[x,y,z], radius?}; \
{op:'replace_color', from_color, to_color, min?, max?}; \
{op:'clear'}. Operations are applied in order, later ones overwrite earlier ones."),
            vec![("operations", prop("array", "Array of operation objects (see description)"))],
            &["operations"],
        ),
        tool(
            "voxel_get",
            "Read non-empty voxels of the edited model in a region as JSON [[x,y,z,color],...] (capped at 5000 entries) plus the total count. Z-up axes; the region defaults to the whole canvas and is clipped to it.",
            vec![
                ("min", prop_ivec3("Region min corner (inclusive)")),
                ("max", prop_ivec3("Region max corner (inclusive)")),
            ],
            &[],
        ),
        tool(
            "voxel_clear",
            "Remove every voxel of the edited model (canvas size is kept). One undo step.",
            vec![],
            &[],
        ),
        tool(
            "palette_manage",
            "Inspect or edit the 255-color palette (indices 1..=255; 0 is empty). `get` returns one entry or all used/customized entries; `set` changes one color (index+color) or several (colors: [{index,color}]) as one undo step; `select` chooses the default drawing color. Colors are \"#RRGGBB\", \"#RRGGBBAA\", [r,g,b] or [r,g,b,a].",
            vec![
                ("action", prop_enum("get, set or select", &["get", "set", "select"])),
                ("index", prop("integer", "Palette index 1..=255")),
                ("color", prop("string", "Color: \"#RRGGBB\", \"#RRGGBBAA\" (or an [r,g,b(,a)] array)")),
                ("colors", prop("array", "Batch set: array of {index, color}")),
            ],
            &["action"],
        ),
        tool(
            "material_manage",
            "Get, set or reset the PBR material attached to a palette index (1..=255). `type` is diffuse|metal|glass|emit|cloud|blend; other fields are 0..1 floats (ior and flux have wider ranges). Used by the path tracer and exports. set/reset are one undo step.",
            vec![
                ("action", prop_enum("get, set or reset", &["get", "set", "reset"])),
                ("index", prop("integer", "Palette index 1..=255")),
                ("type", prop_enum("Material type", &["diffuse", "metal", "glass", "emit", "cloud", "blend"])),
                ("rough", prop("number", "Roughness")),
                ("spec", prop("number", "Specular")),
                ("ior", prop("number", "Index of refraction (glass)")),
                ("metal", prop("number", "Metalness")),
                ("emit", prop("number", "Emission amount")),
                ("flux", prop("number", "Emission power")),
                ("alpha", prop("number", "Transparency")),
                ("density", prop("number", "Density (cloud)")),
            ],
            &["action", "index"],
        ),
    ]
}

pub fn dispatch(session: &mut McpSession, name: &str, args: &JsonValue) -> Option<ToolResult> {
    Some(match name {
        "voxel_set" => voxel_set(session, args),
        "voxel_box" => voxel_box(session, args),
        "voxel_sphere" => voxel_sphere(session, args),
        "voxel_cylinder" => voxel_cylinder(session, args),
        "voxel_line" => voxel_line(session, args),
        "voxel_batch" => voxel_batch(session, args),
        "voxel_get" => voxel_get(session, args),
        "voxel_clear" => with_grid(session, |g| {
            g.clear();
            Ok("Model cleared.".to_string())
        }),
        "palette_manage" => palette_manage(session, args),
        "material_manage" => material_manage(session, args),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run `f` on a clone of the edited grid; on success record one undo step and commit.
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

fn color_arg(args: &JsonValue, default: u8) -> Result<u8, String> {
    match args.get("color") {
        None | Some(JsonValue::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|c| *c <= 255)
            .map(|c| c as u8)
            .ok_or_else(|| "'color' must be an integer palette index 0..=255".to_string()),
    }
}

fn ivec3_arg(args: &JsonValue, key: &str) -> Result<IVec3, String> {
    let v = args
        .get_ivec3(key)
        .ok_or_else(|| format!("'{key}' must be an array of 3 integers [x,y,z]"))?;
    if v.iter().any(|c| c.abs() > COORD_LIMIT) {
        return Err(format!(
            "'{key}' coordinates must be within +/-{COORD_LIMIT}"
        ));
    }
    Ok(IVec3::from_array(v))
}

fn vec3_arg(args: &JsonValue, key: &str) -> Result<Vec3, String> {
    let v = args
        .get_vec3(key)
        .ok_or_else(|| format!("'{key}' must be an array of 3 numbers [x,y,z]"))?;
    if v.iter()
        .any(|c| !c.is_finite() || c.abs() > COORD_LIMIT as f32)
    {
        return Err(format!(
            "'{key}' coordinates must be finite and within +/-{COORD_LIMIT}"
        ));
    }
    Ok(Vec3::from_array(v))
}

fn num_arg(args: &JsonValue, key: &str, default: f32, max: f32) -> Result<f32, String> {
    match args.get(key) {
        None | Some(JsonValue::Null) => Ok(default),
        Some(v) => {
            let n = v
                .as_f64()
                .ok_or_else(|| format!("'{key}' must be a number"))? as f32;
            if !n.is_finite() || n < 0.0 || n > max {
                return Err(format!("'{key}' must be between 0 and {max}"));
            }
            Ok(n)
        }
    }
}

fn voxels_msg(n: usize) -> String {
    format!("{n} voxel(s) written inside the canvas.")
}

// ---------------------------------------------------------------------------
// Drawing tools
// ---------------------------------------------------------------------------

fn voxel_set(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let voxels = args
        .get_array("voxels")
        .ok_or_else(|| "'voxels' must be an array".to_string())?;
    let color = color_arg(args, default_color(session))?;
    let mut parsed: Vec<(i32, i32, i32, u8)> = Vec::with_capacity(voxels.len());
    for (i, v) in voxels.iter().enumerate() {
        let bad = || format!("voxels[{i}] must be [x,y,z], [x,y,z,color] or {{x,y,z,color}}");
        let int = |j: &JsonValue| -> Result<i32, String> {
            j.as_i64()
                .filter(|n| n.abs() <= COORD_LIMIT as i64)
                .map(|n| n as i32)
                .ok_or_else(bad)
        };
        let col = |j: &JsonValue| -> Result<u8, String> {
            j.as_u64()
                .filter(|c| *c <= 255)
                .map(|c| c as u8)
                .ok_or_else(bad)
        };
        let item = if let Some(a) = v.as_array() {
            if a.len() < 3 || a.len() > 4 {
                return Err(bad());
            }
            (
                int(&a[0])?,
                int(&a[1])?,
                int(&a[2])?,
                a.get(3).map(col).transpose()?.unwrap_or(color),
            )
        } else if matches!(v, JsonValue::Object(_)) {
            (
                int(v.get("x").ok_or_else(bad)?)?,
                int(v.get("y").ok_or_else(bad)?)?,
                int(v.get("z").ok_or_else(bad)?)?,
                v.get("color").map(col).transpose()?.unwrap_or(color),
            )
        } else {
            return Err(bad());
        };
        parsed.push(item);
    }
    with_grid(session, |g| {
        let mut n = 0;
        for (x, y, z, c) in parsed {
            if g.in_bounds(x, y, z) {
                g.set_i32(x, y, z, c);
                n += 1;
            }
        }
        Ok(format!(
            "{n} of {} voxel(s) set (the rest were outside the canvas).",
            voxels.len()
        ))
    })
}

fn voxel_box(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let (a, b) = (ivec3_arg(args, "min")?, ivec3_arg(args, "max")?);
    let color = color_arg(args, default_color(session))?;
    let hollow = args.get_bool("hollow").unwrap_or(false);
    let wall = args.get_i32("wall_thickness").unwrap_or(1).max(1);
    let (lo, hi) = (a.min(b), a.max(b));
    if hollow && (hi - lo).max_element() > 1024 {
        return Err("Hollow boxes are limited to 1024 voxels per side".into());
    }
    with_grid(session, |g| {
        let (lo, hi) = if hollow {
            (lo, hi)
        } else {
            // Solid boxes can be clipped to the canvas without changing the result.
            let max = IVec3::new(
                g.size_x() as i32 - 1,
                g.size_y() as i32 - 1,
                g.size_z() as i32 - 1,
            );
            (lo.max(IVec3::ZERO), hi.min(max))
        };
        Ok(voxels_msg(draw_box(g, lo, hi, color, hollow, wall)))
    })
}

fn voxel_sphere(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let center = vec3_arg(args, "center")?;
    let radii = if args.get("radii").is_some() {
        let r = vec3_arg(args, "radii")?;
        if r.min_element() < 0.0 || r.max_element() > 512.0 {
            return Err("'radii' must be between 0 and 512".into());
        }
        r
    } else {
        Vec3::splat(num_arg(args, "radius", 4.0, 512.0)?)
    };
    let color = color_arg(args, default_color(session))?;
    let hollow = args.get_bool("hollow").unwrap_or(false);
    let wall = num_arg(args, "wall_thickness", 1.0, 512.0)?;
    with_grid(session, |g| {
        Ok(voxels_msg(draw_ellipsoid(
            g, center, radii, color, hollow, wall,
        )))
    })
}

fn voxel_cylinder(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let center = vec3_arg(args, "center")?;
    let axis = parse_axis(args.get_str("axis"));
    if let Some(a) = args.get_str("axis") {
        if !["x", "y", "z"].contains(&a.to_ascii_lowercase().as_str()) {
            return Err("'axis' must be x, y or z".into());
        }
    }
    let height = args
        .get_i32("height")
        .filter(|h| (1..=512).contains(h))
        .ok_or_else(|| "'height' must be an integer between 1 and 512".to_string())?;
    let radius = num_arg(args, "radius", f32::NAN, 256.0)?;
    if args.get("radius").is_none() {
        return Err("'radius' is required".into());
    }
    let top = num_arg(args, "radius_top", radius, 256.0)?;
    let color = color_arg(args, default_color(session))?;
    let hollow = args.get_bool("hollow").unwrap_or(false);
    with_grid(session, |g| {
        Ok(voxels_msg(draw_cylinder(
            g, center, axis, height, radius, top, color, hollow,
        )))
    })
}

fn voxel_line(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let (start, end) = (ivec3_arg(args, "start")?, ivec3_arg(args, "end")?);
    let color = color_arg(args, default_color(session))?;
    let radius = args.get_i32("radius").unwrap_or(0);
    if !(0..=32).contains(&radius) {
        return Err("'radius' must be between 0 and 32".into());
    }
    with_grid(session, |g| {
        Ok(voxels_msg(draw_line(g, start, end, color, radius)))
    })
}

fn voxel_batch(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let ops = args
        .get_array("operations")
        .ok_or_else(|| "'operations' must be an array of operation objects".to_string())?;
    if ops.is_empty() {
        return Err("'operations' is empty".into());
    }
    let color = default_color(session);
    with_grid(session, |g| {
        let mut total = 0;
        for (i, op) in ops.iter().enumerate() {
            total += execute_batch_op(g, op, color)
                .map_err(|e| format!("operations[{i}]: {e} (no changes were applied)"))?;
        }
        Ok(format!(
            "{} operation(s) applied, {total} voxel(s) written.",
            ops.len()
        ))
    })
}

fn voxel_get(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let g = &session.project.grid;
    let max_c = IVec3::new(
        g.size_x() as i32 - 1,
        g.size_y() as i32 - 1,
        g.size_z() as i32 - 1,
    );
    let (a, b) = (
        if args.get("min").is_some() {
            ivec3_arg(args, "min")?
        } else {
            IVec3::ZERO
        },
        if args.get("max").is_some() {
            ivec3_arg(args, "max")?
        } else {
            max_c
        },
    );
    let lo = a.min(b).max(IVec3::ZERO);
    let hi = a.max(b).min(max_c);
    let mut out = Vec::new();
    let mut total = 0usize;
    for z in lo.z..=hi.z {
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let c = g.get(x as u32, y as u32, z as u32);
                if c != 0 {
                    total += 1;
                    if out.len() < GET_CAP {
                        out.push(JsonValue::array(vec![
                            JsonValue::int(x as i64),
                            JsonValue::int(y as i64),
                            JsonValue::int(z as i64),
                            JsonValue::int(c as i64),
                        ]));
                    }
                }
            }
        }
    }
    let mut s = JsonValue::array(out).to_json_string();
    s.push_str(&format!("\ntotal non-empty voxels in region: {total}"));
    if total > GET_CAP {
        s.push_str(&format!(
            " (truncated to the first {GET_CAP}; narrow min/max to see the rest)"
        ));
    }
    Ok(vec![text(s)])
}

// ---------------------------------------------------------------------------
// Palette & materials
// ---------------------------------------------------------------------------

fn parse_color(v: &JsonValue) -> Result<Rgba, String> {
    let bad = || "color must be \"#RRGGBB\", \"#RRGGBBAA\", [r,g,b] or [r,g,b,a]".to_string();
    if let Some(s) = v.as_str() {
        let h = s.trim().trim_start_matches('#');
        if !(h.len() == 6 || h.len() == 8) || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        let a = if h.len() == 8 { b(6) } else { 255 };
        return Ok(Rgba::new(b(0), b(2), b(4), a));
    }
    if let Some(arr) = v.as_array() {
        if arr.len() == 3 || arr.len() == 4 {
            let mut c = [255u8; 4];
            for (i, x) in arr.iter().enumerate() {
                c[i] = x.as_u64().filter(|n| *n <= 255).ok_or_else(bad)? as u8;
            }
            return Ok(Rgba::new(c[0], c[1], c[2], c[3]));
        }
    }
    Err(bad())
}

fn hex(c: Rgba) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

fn index_arg(v: Option<&JsonValue>) -> Result<usize, String> {
    v.and_then(JsonValue::as_u64)
        .filter(|i| (1..=255).contains(i))
        .map(|i| i as usize)
        .ok_or_else(|| "'index' must be an integer between 1 and 255".to_string())
}

fn palette_manage(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args.get_str("action").unwrap_or("");
    match action {
        "get" => {
            let p = &session.project;
            let mut used = [0usize; 256];
            for &c in p.grid.raw_data() {
                used[c as usize] += 1;
            }
            let default = mv_core::Palette::default();
            let entry = |i: usize| {
                JsonValue::object(vec![
                    ("index", JsonValue::uint(i)),
                    ("color", JsonValue::str(hex(p.palette.colors[i]))),
                    ("used_voxels", JsonValue::uint(used[i])),
                ])
            };
            let body = if args.get("index").is_some() {
                entry(index_arg(args.get("index"))?)
            } else {
                let list = (1..=255)
                    .filter(|&i| used[i] > 0 || p.palette.colors[i] != default.colors[i])
                    .map(entry)
                    .collect();
                JsonValue::object(vec![
                    ("selected", JsonValue::int(p.palette.selected as i64)),
                    ("entries_used_or_customized", JsonValue::array(list)),
                ])
            };
            Ok(vec![text(body.to_json_string())])
        }
        "set" => {
            let mut changes: Vec<(usize, Rgba)> = Vec::new();
            if let Some(list) = args.get_array("colors") {
                for (i, e) in list.iter().enumerate() {
                    let idx = index_arg(e.get("index")).map_err(|m| format!("colors[{i}]: {m}"))?;
                    let col = parse_color(
                        e.get("color")
                            .ok_or_else(|| format!("colors[{i}]: missing 'color'"))?,
                    )
                    .map_err(|m| format!("colors[{i}]: {m}"))?;
                    changes.push((idx, col));
                }
            }
            if args.get("index").is_some() || args.get("color").is_some() {
                let idx = index_arg(args.get("index"))?;
                let col = parse_color(
                    args.get("color")
                        .ok_or_else(|| "missing 'color'".to_string())?,
                )?;
                changes.push((idx, col));
            }
            if changes.is_empty() {
                return Err("'set' needs index+color or a non-empty 'colors' array".into());
            }
            session.project.record_undo();
            for (i, c) in &changes {
                session.project.palette.colors[*i] = *c;
            }
            session.project.dirty = true;
            Ok(vec![text(format!(
                "{} palette color(s) set.",
                changes.len()
            ))])
        }
        "select" => {
            let idx = index_arg(args.get("index"))?;
            session.project.palette.selected = idx as u8;
            Ok(vec![text(format!("Selected palette color {idx}."))])
        }
        _ => Err("'action' must be get, set or select".into()),
    }
}

fn kind_name(k: MaterialType) -> &'static str {
    match k {
        MaterialType::Diffuse => "diffuse",
        MaterialType::Metal => "metal",
        MaterialType::Glass => "glass",
        MaterialType::Emit => "emit",
        MaterialType::Blend => "blend",
        MaterialType::Cloud => "cloud",
    }
}

fn material_json(m: &Material) -> JsonValue {
    let f = |v: f32| JsonValue::float(v as f64);
    JsonValue::object(vec![
        ("index", JsonValue::int(m.id as i64)),
        ("type", JsonValue::str(kind_name(m.kind))),
        ("rough", f(m.rough)),
        ("spec", f(m.spec)),
        ("ior", f(m.ior)),
        ("metal", f(m.metal)),
        ("emit", f(m.emit)),
        ("flux", f(m.flux)),
        ("alpha", f(m.alpha)),
        ("density", f(m.density)),
        ("att", f(m.att)),
        ("weight", f(m.weight)),
        ("ldr", f(m.ldr)),
    ])
}

fn material_manage(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let idx = index_arg(args.get("index"))?;
    match args.get_str("action").unwrap_or("") {
        "get" => Ok(vec![text(
            material_json(&session.project.materials[idx]).to_json_string(),
        )]),
        "reset" => {
            session.project.record_undo();
            session.project.materials[idx] = Material {
                id: idx as u8,
                ..Material::default()
            };
            session.project.dirty = true;
            Ok(vec![text(format!(
                "Material {idx} reset to diffuse defaults."
            ))])
        }
        "set" => {
            let mut m = session.project.materials[idx];
            m.id = idx as u8;
            if let Some(t) = args.get("type") {
                m.kind = match t.as_str().map(str::to_ascii_lowercase).as_deref() {
                    Some("diffuse") => MaterialType::Diffuse,
                    Some("metal") => MaterialType::Metal,
                    Some("glass") => MaterialType::Glass,
                    Some("emit") => MaterialType::Emit,
                    Some("cloud") => MaterialType::Cloud,
                    Some("blend") => MaterialType::Blend,
                    _ => {
                        return Err(
                            "'type' must be diffuse, metal, glass, emit, cloud or blend".into()
                        )
                    }
                };
            }
            let mut changed = args.get("type").is_some();
            let fields: [(&str, &mut f32); 7] = [
                ("rough", &mut m.rough),
                ("spec", &mut m.spec),
                ("ior", &mut m.ior),
                ("metal", &mut m.metal),
                ("emit", &mut m.emit),
                ("flux", &mut m.flux),
                ("alpha", &mut m.alpha),
            ];
            for (k, slot) in fields {
                if args.get(k).is_some() {
                    *slot = num_arg(args, k, 0.0, 1000.0)?;
                    changed = true;
                }
            }
            if args.get("density").is_some() {
                m.density = num_arg(args, "density", 0.0, 1000.0)?;
                changed = true;
            }
            if !changed {
                return Err("'set' needs at least one of type, rough, spec, ior, metal, emit, flux, alpha, density".into());
            }
            session.project.record_undo();
            session.project.materials[idx] = m;
            session.project.dirty = true;
            Ok(vec![text(material_json(&m).to_json_string())])
        }
        _ => Err("'action' must be get, set or reset".into()),
    }
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

    fn session() -> McpSession {
        McpSession::with_project(mv_core::EditorProject::new_empty(16, 16, 16))
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
        assert_eq!(n, 10);
    }

    #[test]
    fn single_shape_tools_are_one_undo_step() {
        let cases = [
            (
                "voxel_set",
                r#"{"voxels":[[1,1,1],[2,2,2,5],{"x":3,"y":3,"z":3,"color":6},[99,0,0]]}"#,
            ),
            (
                "voxel_box",
                r#"{"min":[0,0,0],"max":[3,3,3],"color":2,"hollow":true}"#,
            ),
            ("voxel_sphere", r#"{"center":[8,8,8],"radius":4,"color":3}"#),
            ("voxel_sphere", r#"{"center":[8,8,8],"radii":[5,3,2]}"#),
            (
                "voxel_cylinder",
                r#"{"center":[8,8,0],"axis":"z","height":6,"radius":3,"radius_top":0}"#,
            ),
            (
                "voxel_line",
                r#"{"start":[0,0,0],"end":[10,10,10],"radius":1}"#,
            ),
        ];
        for (name, json) in cases {
            let mut s = session();
            call(&mut s, name, json).unwrap();
            assert!(s.project.grid.count_solid() > 0, "{name}");
            assert!(s.project.undo(), "{name}");
            assert_eq!(s.project.grid.count_solid(), 0, "{name} undo");
            assert!(!s.project.undo(), "{name} exactly one step");
        }
    }

    #[test]
    fn voxel_set_erase_and_clip() {
        let mut s = session();
        let t = first_text(call(
            &mut s,
            "voxel_set",
            r#"{"voxels":[[1,1,1],[99,0,0]],"color":4}"#,
        ));
        assert!(t.contains("1 of 2"));
        assert_eq!(s.project.grid.get(1, 1, 1), 4);
        call(&mut s, "voxel_set", r#"{"voxels":[[1,1,1,0]]}"#).unwrap();
        assert_eq!(s.project.grid.get(1, 1, 1), 0);
        assert!(call(&mut s, "voxel_set", r#"{"voxels":[[1,1]]}"#).is_err());
        assert!(call(&mut s, "voxel_set", r#"{"voxels":"x"}"#).is_err());
        assert!(call(&mut s, "voxel_box", r#"{"min":[0,0]}"#).is_err());
    }

    #[test]
    fn batch_is_atomic() {
        let mut s = session();
        let ok = r#"{"operations":[{"op":"box","min":[0,0,0],"max":[2,2,2],"color":3},{"op":"sphere","center":[8,8,8],"radius":3}]}"#;
        call(&mut s, "voxel_batch", ok).unwrap();
        let solid = s.project.grid.count_solid();
        assert!(solid > 27);
        assert!(s.project.undo());
        assert_eq!(s.project.grid.count_solid(), 0);
        assert!(!s.project.undo());
        s.project.redo();

        let bad = r#"{"operations":[{"op":"box","min":[5,5,5],"max":[6,6,6]},{"op":"bogus"}]}"#;
        let err = call(&mut s, "voxel_batch", bad).unwrap_err();
        assert!(err.contains("operations[1]"));
        assert_eq!(s.project.grid.count_solid(), solid);
        // no undo step added: the one undo reverts the first successful batch only
        assert!(s.project.undo());
        assert_eq!(s.project.grid.count_solid(), 0);
        assert!(call(&mut s, "voxel_batch", r#"{"operations":[]}"#).is_err());
    }

    #[test]
    fn get_and_clear() {
        let mut s = session();
        call(
            &mut s,
            "voxel_box",
            r#"{"min":[0,0,0],"max":[1,1,1],"color":7}"#,
        )
        .unwrap();
        let t = first_text(call(
            &mut s,
            "voxel_get",
            r#"{"min":[0,0,0],"max":[0,0,1]}"#,
        ));
        assert!(t.contains("[0,0,0,7]") && t.contains("total non-empty voxels in region: 2"));
        let all = first_text(call(&mut s, "voxel_get", "{}"));
        assert!(all.contains("region: 8"));
        call(&mut s, "voxel_clear", "{}").unwrap();
        assert_eq!(s.project.grid.count_solid(), 0);
        s.project.undo();
        assert_eq!(s.project.grid.count_solid(), 8);
    }

    #[test]
    fn get_truncates() {
        let mut s = McpSession::with_project(mv_core::EditorProject::new_empty(32, 32, 32));
        call(
            &mut s,
            "voxel_box",
            r#"{"min":[0,0,0],"max":[31,31,31],"color":1}"#,
        )
        .unwrap();
        let t = first_text(call(&mut s, "voxel_get", "{}"));
        assert!(t.contains("truncated"));
    }

    #[test]
    fn palette_set_select_get_undo() {
        let mut s = session();
        call(
            &mut s,
            "palette_manage",
            r##"{"action":"set","index":5,"color":"#FF8000"}"##,
        )
        .unwrap();
        let c = s.project.palette.colors[5];
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 0, 255));
        call(&mut s, "palette_manage", r#"{"action":"set","colors":[{"index":6,"color":[1,2,3]},{"index":7,"color":[1,2,3,4]}]}"#).unwrap();
        assert_eq!(s.project.palette.colors[7].a, 4);
        s.project.undo();
        assert_ne!(s.project.palette.colors[6], Rgba::new(1, 2, 3, 255));
        assert_eq!(s.project.palette.colors[5], c);
        call(&mut s, "palette_manage", r#"{"action":"select","index":9}"#).unwrap();
        assert_eq!(s.project.palette.selected, 9);
        call(&mut s, "voxel_set", r#"{"voxels":[[0,0,0]]}"#).unwrap();
        assert_eq!(s.project.grid.get(0, 0, 0), 9);
        let t = first_text(call(
            &mut s,
            "palette_manage",
            r#"{"action":"get","index":5}"#,
        ));
        assert!(t.contains("#FF8000"));
        assert!(call(&mut s, "palette_manage", "{\"action\":\"get\"}").is_ok());
        assert!(call(
            &mut s,
            "palette_manage",
            r##"{"action":"set","index":0,"color":"#000000"}"##
        )
        .is_err());
        assert!(call(
            &mut s,
            "palette_manage",
            r#"{"action":"set","index":1,"color":"red"}"#
        )
        .is_err());
        assert!(call(
            &mut s,
            "palette_manage",
            r#"{"action":"select","index":256}"#
        )
        .is_err());
    }

    #[test]
    fn material_set_reset_undo() {
        let mut s = session();
        call(
            &mut s,
            "material_manage",
            r#"{"action":"set","index":3,"type":"metal","rough":0.2,"metal":0.9}"#,
        )
        .unwrap();
        let m = s.project.materials[3];
        assert_eq!((m.id, m.kind), (3, MaterialType::Metal));
        assert!((m.metal - 0.9).abs() < 1e-6);
        let t = first_text(call(
            &mut s,
            "material_manage",
            r#"{"action":"get","index":3}"#,
        ));
        assert!(t.contains("metal"));
        call(&mut s, "material_manage", r#"{"action":"reset","index":3}"#).unwrap();
        assert_eq!(s.project.materials[3].kind, MaterialType::Diffuse);
        assert_eq!(s.project.materials[3].id, 3);
        s.project.undo();
        assert_eq!(s.project.materials[3].kind, MaterialType::Metal);
        assert!(call(
            &mut s,
            "material_manage",
            r#"{"action":"set","index":3,"type":"wood"}"#
        )
        .is_err());
        assert!(call(&mut s, "material_manage", r#"{"action":"set","index":3}"#).is_err());
    }
}
