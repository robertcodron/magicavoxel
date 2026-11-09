//! MCP tools: `render_preview` (model / world / path-traced PNG) and `camera`.

use std::collections::HashSet;

use glam::Vec3;
use mv_render::{render_viewport_rgba, render_world_rgba, Highlight, PathTracerEngine, WorldView};

use crate::json::JsonValue;
use crate::session::McpSession;
use crate::tools::{image_png, prop, prop_enum, prop_vec3, text, tool, ToolResult};

pub fn definitions() -> Vec<JsonValue> {
    vec![
        tool(
            "render_preview",
            "Render the current work to a PNG image so you can SEE the result (call it after \
             drawing with voxel_* tools, and iterate). Workflow: scene_manage create_object -> draw \
             with voxel_* tools -> render_preview. Modes: 'model' renders only the edited object \
             (fast, shows grid and edges; camera in the object's local coordinates); 'world' \
             (default) renders the whole visible scene with fast CPU shading; 'pathtracer' renders \
             the whole scene with physically based path tracing (materials, glass, emission, \
             shadows; slower, use 'samples' to trade noise for time). The camera orbits Z-up: \
             yaw rotates around the vertical axis, pitch is the elevation in degrees (positive \
             looks from above), distance is in voxels. With auto_frame (default) the camera is \
             framed on the edited object (model) or on the visible scene (world/pathtracer) and \
             only yaw/pitch/distance overrides are applied; use auto_frame=false to keep the camera \
             set with the `camera` tool. Several calls with different yaw values (e.g. 30, 120, \
             210, 300) let you inspect every side.",
            vec![
                ("mode", prop_enum("Render mode (default world).", &["model", "world", "pathtracer"])),
                ("width", prop("integer", "Image width in pixels, 64..=1024 (default 512).")),
                ("height", prop("integer", "Image height in pixels, 64..=1024 (default 384).")),
                ("samples", prop("integer", "pathtracer only: samples per pixel 1..=1024 (default 64).")),
                ("yaw", prop("number", "Camera yaw in degrees (override).")),
                ("pitch", prop("number", "Camera pitch in degrees (override).")),
                ("distance", prop("number", "Camera distance in voxels (override).")),
                ("auto_frame", prop("boolean", "Frame the object/scene automatically (default true).")),
            ],
            &[],
        ),
        tool(
            "camera",
            "Read or set the orbit camera used by render_preview when auto_frame is false \
             (render_preview always stores the camera it used). Z-up: yaw/pitch in degrees, \
             distance in voxels, target [x,y,z] is the look-at point, fov in degrees. Actions: \
             get; set {yaw?, pitch?, distance?, target?, fov?}.",
            vec![
                ("action", prop_enum("get or set.", &["get", "set"])),
                ("yaw", prop("number", "Yaw in degrees.")),
                ("pitch", prop("number", "Pitch in degrees (clamped to -89..89).")),
                ("distance", prop("number", "Distance from the target in voxels (> 0).")),
                ("target", prop_vec3("Look-at point [x,y,z] in voxels.")),
                ("fov", prop("number", "Vertical field of view in degrees, 10..120.")),
            ],
            &["action"],
        ),
    ]
}

pub fn dispatch(session: &mut McpSession, name: &str, args: &JsonValue) -> Option<ToolResult> {
    Some(match name {
        "render_preview" => render_preview(session, args),
        "camera" => camera(session, args),
        _ => return None,
    })
}

fn num_arg(args: &JsonValue, key: &str) -> Result<Option<f32>, String> {
    match args.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite())
            .map(|f| Some(f as f32))
            .ok_or_else(|| format!("'{key}' must be a finite number")),
    }
}

fn int_arg(args: &JsonValue, key: &str, default: i64, lo: i64, hi: i64) -> Result<i64, String> {
    match args.get(key) {
        None | Some(JsonValue::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite())
            .map(|f| (f.round() as i64).clamp(lo, hi))
            .ok_or_else(|| format!("'{key}' must be a number")),
    }
}

fn camera_json(session: &McpSession) -> JsonValue {
    let c = &session.camera;
    JsonValue::object(vec![
        ("yaw", JsonValue::float(c.yaw_deg as f64)),
        ("pitch", JsonValue::float(c.pitch_deg as f64)),
        ("distance", JsonValue::float(c.distance as f64)),
        (
            "target",
            JsonValue::array(vec![
                JsonValue::float(c.target.x as f64),
                JsonValue::float(c.target.y as f64),
                JsonValue::float(c.target.z as f64),
            ]),
        ),
        ("fov", JsonValue::float(c.fov_deg as f64)),
    ])
}

fn camera(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let action = args
        .get_str("action")
        .ok_or("Missing required string 'action' (get or set)")?;
    match action {
        "get" => Ok(vec![text(camera_json(session).to_json_string())]),
        "set" => {
            let yaw = num_arg(args, "yaw")?;
            let pitch = num_arg(args, "pitch")?;
            let distance = num_arg(args, "distance")?;
            let fov = num_arg(args, "fov")?;
            let target = match args.get("target") {
                None | Some(JsonValue::Null) => None,
                Some(_) => Some(
                    args.get_vec3("target")
                        .filter(|t| t.iter().all(|c| c.is_finite()))
                        .ok_or("'target' must be an array of 3 numbers [x,y,z]")?,
                ),
            };
            if distance.is_some_and(|d| d <= 0.0) {
                return Err("'distance' must be > 0".to_string());
            }
            if yaw.is_none()
                && pitch.is_none()
                && distance.is_none()
                && fov.is_none()
                && target.is_none()
            {
                return Err(
                    "Nothing to set: pass yaw, pitch, distance, target and/or fov".to_string(),
                );
            }
            let c = &mut session.camera;
            if let Some(v) = yaw {
                c.yaw_deg = v;
            }
            if let Some(v) = pitch {
                c.pitch_deg = v.clamp(-89.0, 89.0);
            }
            if let Some(v) = distance {
                c.distance = v;
            }
            if let Some(v) = fov {
                c.fov_deg = v.clamp(10.0, 120.0);
            }
            if let Some(t) = target {
                c.target = Vec3::from(t);
            }
            Ok(vec![text(camera_json(session).to_json_string())])
        }
        other => Err(format!(
            "Unknown camera action '{other}' (expected get or set)"
        )),
    }
}

fn render_preview(session: &mut McpSession, args: &JsonValue) -> ToolResult {
    let mode = args.get_str("mode").unwrap_or("world");
    if !matches!(mode, "model" | "world" | "pathtracer") {
        return Err(format!(
            "'mode' must be model, world or pathtracer, got '{mode}'"
        ));
    }
    let width = int_arg(args, "width", 512, 64, 1024)? as usize;
    let height = int_arg(args, "height", 384, 64, 1024)? as usize;
    let samples = int_arg(args, "samples", 64, 1, 1024)? as u32;
    let yaw = num_arg(args, "yaw")?;
    let pitch = num_arg(args, "pitch")?;
    let distance = num_arg(args, "distance")?;
    if distance.is_some_and(|d| d <= 0.0) {
        return Err("'distance' must be > 0".to_string());
    }
    let auto_frame = args.get_bool("auto_frame").unwrap_or(true);

    // Camera framing.
    if auto_frame {
        if mode == "model" {
            session.camera.recenter(&session.project.grid);
        } else {
            let view = WorldView::build(&session.project, |_| (Highlight::None, false));
            if let Some((lo, hi)) = view.bounds() {
                session.camera.frame_box(lo, hi);
            }
        }
    }
    if let Some(v) = yaw {
        session.camera.yaw_deg = v;
    }
    if let Some(v) = pitch {
        session.camera.pitch_deg = v.clamp(-89.0, 89.0);
    }
    if let Some(v) = distance {
        session.camera.distance = v;
    }

    let visible_objects = {
        let scene = &session.project.scene;
        scene
            .shapes_under(scene.root())
            .into_iter()
            .filter(|&s| scene.is_visible(s))
            .count()
    };

    let mut buf: Vec<u8> = Vec::new();
    let mut done_samples = 0u32;
    match mode {
        "model" => render_viewport_rgba(
            &session.project,
            &session.camera,
            &session.display,
            &HashSet::new(),
            width,
            height,
            &mut buf,
        ),
        "world" => {
            let view = WorldView::build(&session.project, |_| (Highlight::None, false));
            render_world_rgba(
                &session.project,
                &view,
                &session.camera,
                &session.display,
                width,
                height,
                &mut buf,
            );
        }
        _ => {
            session.pt_settings.samples_per_frame = 8;
            session.pt_settings.target_samples = samples;
            session.pt_settings.paused = false;
            let engine = session.pathtracer.get_or_insert_with(PathTracerEngine::new);
            engine.reset_accumulation();
            // Each call adds at most 8 samples (1 for the first); the cap guards against
            // an engine that stops advancing.
            let max_calls = samples as usize + 2;
            for _ in 0..max_calls {
                engine.render_progressive(
                    &session.project,
                    &session.camera,
                    &session.display,
                    &session.pt_settings,
                    width,
                    height,
                    &mut buf,
                );
                if engine.current_samples >= samples {
                    break;
                }
            }
            done_samples = engine.current_samples;
        }
    }

    if buf.len() != width * height * 4 {
        return Err(format!(
            "Renderer produced {} bytes, expected {}",
            buf.len(),
            width * height * 4
        ));
    }
    let img = image::RgbaImage::from_raw(width as u32, height as u32, buf)
        .ok_or("Failed to build the output image")?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("PNG encoding failed: {e}"))?;

    let c = &session.camera;
    let mut summary = format!("Rendered mode={mode} size={width}x{height}");
    if mode == "pathtracer" {
        summary += &format!(" samples={done_samples}");
    }
    summary += &format!(
        " camera(yaw={:.1}, pitch={:.1}, distance={:.1}) visible_objects={visible_objects}",
        c.yaw_deg, c.pitch_deg, c.distance
    );
    if mode != "model" && visible_objects == 0 {
        summary += ". The scene is empty: create an object with scene_manage create_object and draw with voxel_* tools";
    }
    Ok(vec![image_png(&png), text(summary)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::dispatch_tool_call;

    fn call(s: &mut McpSession, name: &str, json: &str) -> ToolResult {
        dispatch_tool_call(s, name, &JsonValue::parse(json).unwrap())
    }

    fn b64_decode(s: &str) -> Vec<u8> {
        let val = |c: u8| match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        let mut out = Vec::new();
        let (mut acc, mut bits) = (0u32, 0);
        for c in s.bytes() {
            if let Some(v) = val(c) {
                acc = (acc << 6) | v as u32;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((acc >> bits) as u8);
                    acc &= (1 << bits) - 1;
                }
            }
        }
        out
    }

    fn decode_png(items: &[JsonValue]) -> image::DynamicImage {
        let img = items
            .iter()
            .find(|i| i.get_str("type") == Some("image"))
            .expect("image item");
        assert_eq!(img.get_str("mimeType"), Some("image/png"));
        let bytes = b64_decode(img.get_str("data").unwrap());
        assert_eq!(&bytes[1..4], b"PNG");
        image::load_from_memory(&bytes).unwrap()
    }

    fn scene_with_object() -> McpSession {
        let mut s = McpSession::new();
        call(
            &mut s,
            "scene_manage",
            r#"{"action":"create_object","size":[8,8,8]}"#,
        )
        .unwrap();
        call(
            &mut s,
            "voxel_shader",
            r#"{"action":"run","name":"sphere"}"#,
        )
        .unwrap();
        s
    }

    #[test]
    fn definitions_unique() {
        let defs = definitions();
        let mut names: Vec<_> = defs
            .iter()
            .map(|d| d.get_str("name").unwrap().to_string())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names, vec!["camera", "render_preview"]);
    }

    #[test]
    fn model_and_world_render() {
        let mut s = scene_with_object();
        let items = call(
            &mut s,
            "render_preview",
            r#"{"mode":"model","width":96,"height":80}"#,
        )
        .unwrap();
        let img = decode_png(&items);
        assert_eq!((img.width(), img.height()), (96, 80));
        assert!(items
            .iter()
            .any(|i| i.get_str("text").is_some_and(|t| t.contains("mode=model"))));

        let items = call(
            &mut s,
            "render_preview",
            r#"{"width":128,"height":100,"yaw":45,"pitch":30,"distance":50}"#,
        )
        .unwrap();
        let img = decode_png(&items);
        assert_eq!((img.width(), img.height()), (128, 100));
        assert_eq!(s.camera.yaw_deg, 45.0);
        assert_eq!(s.camera.distance, 50.0);
        let t = items.last().unwrap().get_str("text").unwrap();
        assert!(t.contains("mode=world") && t.contains("visible_objects=2"));
    }

    #[test]
    fn size_is_clamped() {
        let mut s = scene_with_object();
        let items = call(
            &mut s,
            "render_preview",
            r#"{"mode":"world","width":1,"height":2}"#,
        )
        .unwrap();
        let img = decode_png(&items);
        assert_eq!((img.width(), img.height()), (64, 64));
    }

    #[test]
    fn pathtracer_few_samples() {
        let mut s = scene_with_object();
        let items = call(
            &mut s,
            "render_preview",
            r#"{"mode":"pathtracer","width":64,"height":64,"samples":3}"#,
        )
        .unwrap();
        let img = decode_png(&items);
        assert_eq!((img.width(), img.height()), (64, 64));
        assert!(s.pathtracer.is_some());
        let t = items.last().unwrap().get_str("text").unwrap();
        assert!(t.contains("samples="), "{t}");
    }

    #[test]
    fn render_errors() {
        let mut s = McpSession::new();
        assert!(call(&mut s, "render_preview", r#"{"mode":"nope"}"#).is_err());
        assert!(call(&mut s, "render_preview", r#"{"yaw":"x"}"#).is_err());
        assert!(call(&mut s, "render_preview", r#"{"distance":-3}"#).is_err());
        // Empty scene still renders.
        assert!(call(&mut s, "render_preview", r#"{"width":64,"height":64}"#).is_ok());
    }

    #[test]
    fn camera_get_set() {
        let mut s = McpSession::new();
        let r = call(&mut s, "camera", r#"{"action":"get"}"#).unwrap();
        assert!(r[0].get_str("text").unwrap().contains("\"yaw\""));
        call(
            &mut s,
            "camera",
            r#"{"action":"set","yaw":10,"pitch":200,"distance":77,"target":[1,2,3],"fov":50}"#,
        )
        .unwrap();
        let c = &s.camera;
        assert_eq!(
            (c.yaw_deg, c.pitch_deg, c.distance, c.fov_deg),
            (10.0, 89.0, 77.0, 50.0)
        );
        assert_eq!(c.target, Vec3::new(1.0, 2.0, 3.0));
        assert!(call(&mut s, "camera", r#"{"action":"set"}"#).is_err());
        assert!(call(&mut s, "camera", r#"{"action":"set","distance":0}"#).is_err());
        assert!(call(&mut s, "camera", r#"{"action":"set","target":[1,2]}"#).is_err());
        assert!(call(&mut s, "camera", r#"{"action":"zap"}"#).is_err());
        // auto_frame=false keeps the camera.
        call(
            &mut s,
            "render_preview",
            r#"{"mode":"model","auto_frame":false,"width":64,"height":64}"#,
        )
        .unwrap();
        assert_eq!(s.camera.distance, 77.0);
    }
}
