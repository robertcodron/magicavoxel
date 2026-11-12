//! MCP session state and JSON-RPC 2.0 protocol dispatcher (`initialize`, `ping`,
//! `tools/list`, `tools/call`, `resources/list`, `resources/read`, `prompts/list`,
//! `prompts/get`).

use std::path::PathBuf;

use mv_core::{EditorProject, XsInvocation, BUILTIN_SHADERS};
use mv_render::{
    discover_shaders, DisplaySettings, OrbitCamera, PathTracerEngine, PathTracerSettings,
    ShaderInputs, VoxelShader, VoxelShaderEngine,
};

use crate::json::JsonValue;
use crate::tools::{dispatch_tool_call, tool_definitions};

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "magicavoxel-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct McpSession {
    pub project: EditorProject,
    pub current_file: Option<PathBuf>,
    pub camera: OrbitCamera,
    pub display: DisplaySettings,
    pub pt_settings: PathTracerSettings,
    pub snap_step: u32,
    pub snap_enabled: bool,
    pub(crate) pathtracer: Option<PathTracerEngine>,
    pub(crate) shader_engine: Option<VoxelShaderEngine>,
    pub(crate) shader_seed: u64,
}

impl Default for McpSession {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSession {
    pub fn new() -> Self {
        let project = EditorProject::new_empty(40, 40, 40);
        Self::with_project(project)
    }

    pub fn with_project(project: EditorProject) -> Self {
        let mut camera = OrbitCamera::default();
        camera.recenter(&project.grid);
        Self {
            project,
            current_file: None,
            camera,
            display: DisplaySettings::default(),
            pt_settings: PathTracerSettings::default(),
            snap_step: 1,
            snap_enabled: false,
            pathtracer: None,
            shader_engine: None,
            shader_seed: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn shader_search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = std::env::var_os("MV_SHADER_DIR") {
            dirs.push(PathBuf::from(d));
        }
        dirs.push(PathBuf::from("shader"));
        dirs.push(PathBuf::from("shaders"));
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            dirs.push(exe_dir.join("shader"));
        }
        dirs
    }

    pub fn discover_external_shaders() -> Vec<(String, PathBuf)> {
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for dir in Self::shader_search_dirs()
            .into_iter()
            .filter(|d| d.is_dir())
        {
            for (name, path) in discover_shaders(&dir) {
                if !found.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    found.push((name, path));
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found
    }

    pub fn run_external_shader(&mut self, inv: &XsInvocation) -> Result<String, String> {
        let files = Self::discover_external_shaders();
        let path = files
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&inv.shader))
            .map(|(_, p)| p.clone())
            .ok_or_else(|| {
                format!(
                    "Unknown voxel shader '{}' (built-ins: {})",
                    inv.shader,
                    BUILTIN_SHADERS.join(", ")
                )
            })?;
        let shader = VoxelShader::load(&inv.shader, &path).map_err(|e| e.to_string())?;
        if self.shader_engine.is_none() {
            self.shader_engine = Some(VoxelShaderEngine::new()?);
        }

        let palette = std::array::from_fn(|i| {
            let c = self.project.palette.colors[i];
            [c.r, c.g, c.b, c.a].map(|v| v as f32 / 255.0)
        });
        let brush = &self.project.brush;
        let color = self.project.palette.selected.max(1);
        let mut base = ShaderInputs {
            args: shader.arg_values(&inv.args),
            color_index: color,
            color_sels: vec![color],
            palette,
            iter: 0,
            frame: 0,
            num_frames: 1,
            rand: [0.0; 4],
            axis: [brush.axis.x, brush.axis.y, brush.axis.z],
            mirror: [brush.mirror.x, brush.mirror.y, brush.mirror.z],
        };

        let mut seed = self.shader_seed;
        let mut next_r = || {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            let v = seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
            (v >> 40) as f32 / (1u64 << 24) as f32
        };
        let engine = self.shader_engine.as_mut().unwrap();
        let res = self.project.run_xs(inv, |grid, step| {
            base.iter = step.iter;
            base.frame = step.frame;
            base.num_frames = step.num_frames;
            base.rand = [next_r(), next_r(), next_r(), next_r()];
            *grid = engine.run(&shader, grid, &base)?;
            Ok(())
        });
        self.shader_seed = seed;
        res
    }

    /// Process a single JSON-RPC 2.0 message line and return `Some(response_json)` for requests
    /// or `None` for notifications.
    pub fn handle_jsonrpc(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let msg = match JsonValue::parse(trimmed) {
            Ok(m) => m,
            Err(e) => {
                let resp = jsonrpc_error(JsonValue::Null, -32700, &format!("Parse error: {e}"));
                return Some(resp.to_json_string());
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get_str("method").unwrap_or("");
        let params = msg
            .get("params")
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(Vec::new()));

        // Notifications have no `id`: do not send a response.
        let id = id?;

        let response = match method {
            "initialize" => {
                let client_proto = params
                    .get_str("protocolVersion")
                    .unwrap_or(MCP_PROTOCOL_VERSION);
                let result = JsonValue::object(vec![
                    ("protocolVersion", JsonValue::str(client_proto)),
                    (
                        "capabilities",
                        JsonValue::object(vec![
                            ("tools", JsonValue::object(vec![("listChanged", JsonValue::bool(false))])),
                            (
                                "resources",
                                JsonValue::object(vec![("listChanged", JsonValue::bool(false))]),
                            ),
                            (
                                "prompts",
                                JsonValue::object(vec![("listChanged", JsonValue::bool(false))]),
                            ),
                        ]),
                    ),
                    (
                        "serverInfo",
                        JsonValue::object(vec![
                            ("name", JsonValue::str(SERVER_NAME)),
                            ("version", JsonValue::str(SERVER_VERSION)),
                        ]),
                    ),
                    (
                        "instructions",
                        JsonValue::str(
                            "MagicaVoxel Rust MCP Server. Coordinate convention is Z-up: X = right (0..size_x), Y = depth/back (0..size_y), Z = height/up (0..size_z, ground is z=0). Palette index 0 is empty; indices 1..=255 are colors. Use `voxel_batch` (or `voxel_box`, `voxel_sphere`, `voxel_cylinder`, `voxel_line`, `voxel_set`) to sculpt 3D voxel objects, `palette_manage` & `material_manage` for colors and PBR materials (diffuse, metal, glass, emit, cloud), `scene_manage` for multi-object scenes, and `render_preview` to inspect your creation.",
                        ),
                    ),
                ]);
                jsonrpc_ok(id, result)
            }
            "ping" => jsonrpc_ok(id, JsonValue::Object(Vec::new())),
            "tools/list" => {
                let result =
                    JsonValue::object(vec![("tools", JsonValue::array(tool_definitions()))]);
                jsonrpc_ok(id, result)
            }
            "tools/call" => {
                let Some(tool_name) = params.get_str("name") else {
                    return Some(
                        jsonrpc_error(id, -32602, "Missing 'name' in tools/call").to_json_string(),
                    );
                };
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| JsonValue::Object(Vec::new()));
                match dispatch_tool_call(self, tool_name, &args) {
                    Ok(content_items) => {
                        let result = JsonValue::object(vec![
                            ("content", JsonValue::array(content_items)),
                            ("isError", JsonValue::bool(false)),
                        ]);
                        jsonrpc_ok(id, result)
                    }
                    Err(err_msg) => {
                        let result = JsonValue::object(vec![
                            (
                                "content",
                                JsonValue::array(vec![JsonValue::object(vec![
                                    ("type", JsonValue::str("text")),
                                    ("text", JsonValue::str(format!("Error: {err_msg}"))),
                                ])]),
                            ),
                            ("isError", JsonValue::bool(true)),
                        ]);
                        jsonrpc_ok(id, result)
                    }
                }
            }
            "resources/list" => {
                let resources = vec![
                    JsonValue::object(vec![
                        ("uri", JsonValue::str("magicavoxel://project/info")),
                        ("name", JsonValue::str("Current Project & Scene Info")),
                        (
                            "description",
                            JsonValue::str("JSON overview of active model, dimensions, scene graph, layers, and animation."),
                        ),
                        ("mimeType", JsonValue::str("application/json")),
                    ]),
                    JsonValue::object(vec![
                        ("uri", JsonValue::str("magicavoxel://project/guide")),
                        ("name", JsonValue::str("AI Voxel Modeling Guide")),
                        (
                            "description",
                            JsonValue::str("Coordinate system (Z-up), palette conventions, and best practices for AI voxel creation."),
                        ),
                        ("mimeType", JsonValue::str("text/markdown")),
                    ]),
                ];
                jsonrpc_ok(
                    id,
                    JsonValue::object(vec![("resources", JsonValue::array(resources))]),
                )
            }
            "resources/read" => {
                let uri = params.get_str("uri").unwrap_or("");
                match uri {
                    "magicavoxel://project/info" => {
                        let info = crate::tools::build_project_info_json(self);
                        let contents = vec![JsonValue::object(vec![
                            ("uri", JsonValue::str(uri)),
                            ("mimeType", JsonValue::str("application/json")),
                            ("text", JsonValue::str(info.to_json_string())),
                        ])];
                        jsonrpc_ok(
                            id,
                            JsonValue::object(vec![("contents", JsonValue::array(contents))]),
                        )
                    }
                    "magicavoxel://project/guide" => {
                        let guide = "# MagicaVoxel AI Modeling Guide\n\n\
                        - **Axes (Z-up)**: `X` = right (`0..size_x`), `Y` = depth (`0..size_y`), `Z` = up/height (`0..size_z`, ground is `z = 0`).\n\
                        - **Palette (`1..=255`)**: `0` is empty space. Define custom RGBA colors with `palette_manage` (`action: \"set\"`) and PBR materials (`metal`, `glass`, `emit`, `cloud`, `diffuse`) with `material_manage`.\n\
                        - **Fast 3D Drawing**: Use `voxel_batch` to combine `box`, `sphere`, `ellipsoid`, `cylinder`, `cone`, `pyramid`, `torus`, `line`, `set`, and `replace_color` in a single atomic call.\n\
                        - **World Scene**: Use `scene_manage` (`create_object`, `translate`, `rotate90`, `move_to_ground`, `instantiate`, `union`) to compose multi-part scenes.\n\
                        - **Visual Verification**: Call `render_preview` (`mode: \"model\"`, `\"world\"`, or `\"pathtracer\"`) to inspect a rendered PNG of your voxel art.";
                        let contents = vec![JsonValue::object(vec![
                            ("uri", JsonValue::str(uri)),
                            ("mimeType", JsonValue::str("text/markdown")),
                            ("text", JsonValue::str(guide)),
                        ])];
                        jsonrpc_ok(
                            id,
                            JsonValue::object(vec![("contents", JsonValue::array(contents))]),
                        )
                    }
                    _ => jsonrpc_error(id, -32602, &format!("Unknown resource URI: {uri}")),
                }
            }
            "prompts/list" => {
                let prompts = vec![JsonValue::object(vec![
                    ("name", JsonValue::str("create_voxel_object")),
                    (
                        "description",
                        JsonValue::str("Step-by-step workflow for creating a detailed 3D voxel object or scene."),
                    ),
                    (
                        "arguments",
                        JsonValue::array(vec![JsonValue::object(vec![
                            ("name", JsonValue::str("subject")),
                            ("description", JsonValue::str("Description of the voxel object to build")),
                            ("required", JsonValue::bool(true)),
                        ])]),
                    ),
                ])];
                jsonrpc_ok(
                    id,
                    JsonValue::object(vec![("prompts", JsonValue::array(prompts))]),
                )
            }
            "prompts/get" => {
                let subject = params
                    .get("arguments")
                    .and_then(|a| a.get_str("subject"))
                    .unwrap_or("a voxel object");
                let text = format!(
                    "Create '{subject}' in MagicaVoxel using the MCP tools:\n\
                    1. Call `project_new` with an appropriate 3D canvas size (e.g. 48x48x48 or 64x64x64; remember Z is UP and z=0 is the ground).\n\
                    2. Set up a cohesive color palette with `palette_manage` (indices 1..30) and configure PBR materials (`emit` for glowing lights, `glass` for windows/water, `metal` for metallic parts) with `material_manage`.\n\
                    3. Build the structure using `voxel_batch` (combining `box`, `sphere`, `ellipsoid`, `cylinder`, `cone`, `pyramid`, `torus`, `line`, `set`) and/or `volume_operation` (`mirror_symmetry`, `dilate`, `erode`, `flood`).\n\
                    4. Render a preview with `render_preview` to verify proportions and details.\n\
                    5. Save or export if requested with `project_save` / `project_export`."
                );
                let messages = vec![JsonValue::object(vec![
                    ("role", JsonValue::str("user")),
                    (
                        "content",
                        JsonValue::object(vec![
                            ("type", JsonValue::str("text")),
                            ("text", JsonValue::str(text)),
                        ]),
                    ),
                ])];
                jsonrpc_ok(
                    id,
                    JsonValue::object(vec![
                        ("description", JsonValue::str("Voxel creation workflow")),
                        ("messages", JsonValue::array(messages)),
                    ]),
                )
            }
            other => jsonrpc_error(id, -32601, &format!("Method not found: {other}")),
        };

        Some(response.to_json_string())
    }

    /// Convenience helper for calling a tool directly in Rust tests or embedded callers.
    pub fn call_tool(&mut self, name: &str, args: &JsonValue) -> Result<Vec<JsonValue>, String> {
        dispatch_tool_call(self, name, args)
    }
}

pub fn jsonrpc_ok(id: JsonValue, result: JsonValue) -> JsonValue {
    JsonValue::object(vec![
        ("jsonrpc", JsonValue::str("2.0")),
        ("id", id),
        ("result", result),
    ])
}

pub fn jsonrpc_error(id: JsonValue, code: i64, message: &str) -> JsonValue {
    JsonValue::object(vec![
        ("jsonrpc", JsonValue::str("2.0")),
        ("id", id),
        (
            "error",
            JsonValue::object(vec![
                ("code", JsonValue::int(code)),
                ("message", JsonValue::str(message)),
            ]),
        ),
    ])
}
