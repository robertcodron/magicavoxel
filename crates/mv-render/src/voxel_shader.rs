//! External voxel shaders, compatible with MagicaVoxel's `shader/*.txt` GLSL files.
//!
//! A MagicaVoxel voxel shader defines `float map(vec3 v)`, called once per voxel with
//! the voxel center `v` and returning the new palette index. Available inputs:
//! `voxel(vec3)`, `color_sel(float)`, `palette(float)`, `i_volume_size`,
//! `i_color_index`, `i_num_color_sels`, `i_iter`, `i_axis`, `i_mirror`, `i_args[16]`,
//! `iFrame`, `iNumFrames`, `iIter`, `iRand`. Arguments are declared in the header:
//!
//! ```text
//! // xs_begin
//! // arg : { name = 'Height'  var = 'm_height'  range = '1 256'  value = '1'  step = '1'  precision = '0' }
//! // xs_end
//! ```
//!
//! The source is wrapped into a GLSL 450 compute shader and run on the GPU through
//! `wgpu` (naga's GLSL front end). Global variables initialized from inputs
//! (`float t = m_threshold / 100.0;`), accepted by MagicaVoxel's drivers but not by
//! strict GLSL, are rewritten into an initialization function.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytemuck::{Pod, Zeroable};
use mv_core::VoxelGrid;

pub const MAX_SHADER_ARGS: usize = 16;

/// One `// arg : {...}` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderArg {
    pub name: String,
    /// Alias usable in the code (`var = 'm_height'`), same as `i_args[index]`.
    pub var: Option<String>,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f32,
    /// Number of decimals shown in the UI.
    pub precision: u32,
}

#[derive(Debug, Clone)]
pub struct VoxelShader {
    /// Name used by `xs <name>` (path relative to the shader folder, without `.txt`).
    pub name: String,
    pub source: String,
    pub args: Vec<ShaderArg>,
}

impl VoxelShader {
    pub fn from_source(name: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            name: name.into(),
            args: parse_args(&source),
            source,
        }
    }

    pub fn load(name: impl Into<String>, path: &Path) -> std::io::Result<Self> {
        Ok(Self::from_source(name, std::fs::read_to_string(path)?))
    }

    /// Argument values: `given` positional values first, then declared defaults.
    pub fn arg_values(&self, given: &[f32]) -> [f32; MAX_SHADER_ARGS] {
        let mut out = [0.0; MAX_SHADER_ARGS];
        for (i, a) in self.args.iter().take(MAX_SHADER_ARGS).enumerate() {
            out[i] = a.default;
        }
        for (i, v) in given.iter().take(MAX_SHADER_ARGS).enumerate() {
            out[i] = *v;
        }
        out
    }
}

/// Parse the `// arg : { key = 'value' ... }` lines of a shader header.
pub fn parse_args(source: &str) -> Vec<ShaderArg> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("//") else {
            continue;
        };
        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix("arg") else {
            continue;
        };
        let Some(body) = rest
            .trim_start()
            .strip_prefix(':')
            .map(str::trim)
            .and_then(|r| r.strip_prefix('{'))
        else {
            continue;
        };
        let body = body.trim_end().trim_end_matches('}');
        let kv = parse_key_values(body);
        let get = |k: &str| kv.get(k).map(String::as_str);
        let num = |k: &str, d: f32| get(k).and_then(|s| s.trim().parse().ok()).unwrap_or(d);
        let (min, max) = get("range")
            .map(|r| {
                let mut it = r.split_whitespace().filter_map(|p| p.parse::<f32>().ok());
                (it.next().unwrap_or(0.0), it.next().unwrap_or(100.0))
            })
            .unwrap_or((0.0, 100.0));
        out.push(ShaderArg {
            name: get("name").unwrap_or("arg").to_string(),
            var: get("var").map(str::to_string).filter(|v| is_identifier(v)),
            min,
            max,
            default: num("value", min),
            step: num("step", 1.0),
            precision: num("precision", 0.0) as u32,
        });
    }
    out
}

/// `name = 'Height'  var = 'm_height'` -> map (values in single or double quotes).
fn parse_key_values(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = body;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '\'' || *c == '"') else {
            break;
        };
        let after = &after[1..];
        let Some(end) = after.find(quote) else { break };
        map.insert(key, after[..end].to_string());
        rest = &after[end + 1..];
    }
    map
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Shader files (`*.txt`) found under `dir`, recursively, as `(name, path)` sorted by
/// name. Subfolder shaders are named `sub/name` (`xs sub/name`).
pub fn discover_shaders(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(base, &p, depth + 1, out);
            } else if p.extension().is_some_and(|x| x == "txt") {
                if let Ok(rel) = p.with_extension("").strip_prefix(base) {
                    let name = rel.to_string_lossy().replace('\\', "/");
                    out.push((name, p));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Source preparation
// ---------------------------------------------------------------------------

/// Move initializers of global variables into `void xs_init_globals()`.
/// Returns `(rewritten source, init function body)`.
fn hoist_global_initializers(src: &str) -> (String, String) {
    let mut out = String::with_capacity(src.len());
    let mut init = String::new();
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut depth = 0usize;
    let mut stmt = String::new();
    // Header of the top-level block being copied (e.g. `float map(vec3 v)`).
    let mut block_header = String::new();

    let flush_stmt = |stmt: &mut String, out: &mut String, init: &mut String| {
        let text = stmt.trim();
        if let Some((decl, assign)) = split_global_initializer(text) {
            out.push_str(&decl);
            init.push_str(&assign);
            init.push('\n');
        } else {
            out.push_str(stmt);
        }
        stmt.clear();
    };

    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();
        // Comments and preprocessor lines are copied verbatim.
        if c == '/' && next == Some('/') {
            while i < bytes.len() && bytes[i] != '\n' {
                if depth == 0 && stmt.trim().is_empty() {
                    out.push(bytes[i]);
                } else {
                    stmt.push(bytes[i]);
                }
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            let comment: String = bytes[start..i].iter().collect();
            if depth == 0 && stmt.trim().is_empty() {
                out.push_str(&comment);
            } else {
                stmt.push_str(&comment);
            }
            continue;
        }
        if c == '#' && depth == 0 && stmt.trim().is_empty() {
            while i < bytes.len() && bytes[i] != '\n' {
                out.push(bytes[i]);
                i += 1;
            }
            continue;
        }

        if depth == 0 {
            match c {
                ';' => {
                    stmt.push(c);
                    flush_stmt(&mut stmt, &mut out, &mut init);
                }
                '{' => {
                    // Function body or struct: copy the header and track nesting.
                    out.push_str(&stmt);
                    block_header = std::mem::take(&mut stmt);
                    out.push(c);
                    depth = 1;
                }
                _ => {
                    if stmt.is_empty() && c.is_whitespace() {
                        out.push(c);
                    } else {
                        stmt.push(c);
                    }
                }
            }
        } else {
            if c == '}' && depth == 1 {
                // GLSL drivers accept functions that can fall off their end; naga does
                // not. Add a default return at the end of value-returning functions.
                if let Some(ret) = default_return(&block_header) {
                    out.push_str(&ret);
                }
            }
            out.push(c);
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    out.push_str(&stmt);
    (out, init)
}

/// Fallback `return` statement for a function header (`None` for `void`, structs and
/// unknown return types).
fn default_return(header: &str) -> Option<String> {
    let header = header.trim();
    if !header.contains('(') {
        return None;
    }
    let ty = header.split_whitespace().next()?;
    let value = match ty {
        "float" => "0.0".to_string(),
        "int" => "0".to_string(),
        "uint" => "0u".to_string(),
        "bool" => "false".to_string(),
        t if t.starts_with("bvec") => format!("{t}(false)"),
        t if (t.starts_with("vec")
            || t.starts_with("ivec")
            || t.starts_with("uvec")
            || t.starts_with("mat"))
            && t.len() <= 6 =>
        {
            format!("{t}(0)")
        }
        _ => return None,
    };
    Some(format!("\n    return {value};\n"))
}

/// `float t = m_a / 100.0;` -> `("float t;", "t = m_a / 100.0;")`. `None` for anything
/// that is not a non-const global with an initializer (prototypes, structs, consts...).
fn split_global_initializer(stmt: &str) -> Option<(String, String)> {
    let body = stmt.strip_suffix(';')?.trim();
    let first = body.split_whitespace().next()?;
    if matches!(
        first,
        "const" | "uniform" | "struct" | "layout" | "precision" | "in" | "out" | "buffer"
    ) {
        return None;
    }
    // First `=` outside parentheses that is not part of `==`, `<=`, `>=`, `!=`.
    let chars: Vec<char> = body.chars().collect();
    let mut paren = 0i32;
    let mut eq = None;
    for (k, &ch) in chars.iter().enumerate() {
        match ch {
            '(' | '[' => paren += 1,
            ')' | ']' => paren -= 1,
            '=' if paren == 0 => {
                let prev = if k > 0 { chars[k - 1] } else { ' ' };
                let next = chars.get(k + 1).copied().unwrap_or(' ');
                if !matches!(prev, '=' | '<' | '>' | '!') && next != '=' {
                    eq = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let eq = eq?;
    let lhs: String = chars[..eq].iter().collect();
    let rhs: String = chars[eq + 1..].iter().collect();
    let lhs = lhs.trim();
    // Declared name: last identifier of the left side, without array brackets.
    let decl_name_part = lhs.split('[').next()?.trim();
    let name = decl_name_part.split_whitespace().last()?;
    if !is_identifier(name) || decl_name_part.split_whitespace().count() < 2 {
        return None;
    }
    Some((format!("{lhs};"), format!("    {name} = {};", rhs.trim())))
}

/// Full GLSL compute shader for a voxel shader source.
pub fn build_compute_source(shader: &VoxelShader) -> String {
    let (user, init) = hoist_global_initializers(&shader.source);
    let mut defines = String::new();
    for (i, a) in shader.args.iter().take(MAX_SHADER_ARGS).enumerate() {
        if let Some(var) = &a.var {
            defines.push_str(&format!("#define {var} (xs_args[{i}])\n"));
        }
    }
    format!(
        r#"#version 450
layout(local_size_x = 4, local_size_y = 4, local_size_z = 4) in;
layout(std430, set = 0, binding = 0) readonly buffer XsSrc {{ uint xs_src[]; }};
layout(std430, set = 0, binding = 1) buffer XsDst {{ uint xs_dst[]; }};
layout(std430, set = 0, binding = 2) readonly buffer XsParams {{
    vec4 xs_size;
    vec4 xs_misc;
    vec4 xs_rand;
    vec4 xs_axis;
    vec4 xs_mirror;
    float xs_args[16];
    float xs_sels[256];
    vec4 xs_palette[256];
}};
#define i_volume_size (xs_size.xyz)
#define i_color_index (xs_misc.x)
#define i_iter (xs_misc.y)
#define iIter (xs_misc.y)
#define iFrame (xs_misc.z)
#define iNumFrames (xs_misc.w)
#define iRand (xs_rand)
#define i_axis (xs_axis.xyz)
#define i_mirror (xs_mirror.xyz)
#define i_num_color_sels (int(xs_axis.w))
#define i_args xs_args
{defines}
float voxel(vec3 v) {{
    ivec3 p = ivec3(floor(v));
    ivec3 s = ivec3(xs_size.xyz);
    if (p.x < 0 || p.y < 0 || p.z < 0 || p.x >= s.x || p.y >= s.y || p.z >= s.z) {{
        return 0.0;
    }}
    uint i = uint(p.x + p.y * s.x + p.z * s.x * s.y);
    return float((xs_src[i >> 2u] >> ((i & 3u) * 8u)) & 255u);
}}
float color_sel(float i) {{
    int k = int(i);
    if (k < 0 || k >= i_num_color_sels) {{
        return 0.0;
    }}
    return xs_sels[k];
}}
vec4 palette(float i) {{
    return xs_palette[clamp(int(i), 0, 255)];
}}
// ---- user shader ----
{user}
// ---- end of user shader ----
void xs_init_globals() {{
{init}}}
void main() {{
    ivec3 s = ivec3(xs_size.xyz);
    ivec3 g = ivec3(gl_GlobalInvocationID);
    if (g.x >= s.x || g.y >= s.y || g.z >= s.z) {{
        return;
    }}
    xs_init_globals();
    float c = map(vec3(g) + vec3(0.5));
    uint idx = uint(g.x + g.y * s.x + g.z * s.x * s.y);
    xs_dst[idx] = uint(clamp(floor(c + 0.5), 0.0, 255.0));
}}
"#
    )
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Per-run inputs of a voxel shader.
#[derive(Debug, Clone)]
pub struct ShaderInputs {
    pub args: [f32; MAX_SHADER_ARGS],
    pub color_index: u8,
    /// Selected palette colors (`color_sel`); the brush color if empty.
    pub color_sels: Vec<u8>,
    /// RGBA in `[0, 1]` for `palette()`.
    pub palette: [[f32; 4]; 256],
    pub iter: u32,
    pub frame: u32,
    pub num_frames: u32,
    pub rand: [f32; 4],
    pub axis: [bool; 3],
    pub mirror: [bool; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuParams {
    size: [f32; 4],
    misc: [f32; 4],
    rand: [f32; 4],
    axis: [f32; 4],
    mirror: [f32; 4],
    args: [f32; 16],
    sels: [f32; 256],
    palette: [[f32; 4]; 256],
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}

/// Compiles and runs voxel shaders on the GPU. Compiled pipelines are cached by source.
pub struct VoxelShaderEngine {
    gpu: GpuContext,
    pipelines: HashMap<String, wgpu::ComputePipeline>,
}

impl VoxelShaderEngine {
    /// Create a GPU device. Fails if no adapter is available (voxel shaders need a GPU).
    pub fn new() -> Result<Self, String> {
        pollster::block_on(Self::init())
    }

    async fn init() -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no GPU adapter available: external voxel shaders need a GPU")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("mv-voxel-shader-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| format!("GPU device request failed: {e}"))?;
        // Compile errors are reported through error scopes; never panic.
        device.on_uncaptured_error(Box::new(|e| eprintln!("[voxel shader] wgpu error: {e}")));

        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xs_bgl"),
            entries: &[storage(0, true), storage(1, false), storage(2, true)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("xs_pipeline_layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        Ok(Self {
            gpu: GpuContext {
                device,
                queue,
                layout,
                pipeline_layout,
            },
            pipelines: HashMap::new(),
        })
    }

    /// Compile (or fetch from cache) the pipeline of a shader. Errors are the compiler
    /// messages, suitable for the console.
    pub fn compile(&mut self, shader: &VoxelShader) -> Result<(), String> {
        let source = build_compute_source(shader);
        if self.pipelines.contains_key(&source) {
            return Ok(());
        }
        let device = &self.gpu.device;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&shader.name),
            source: wgpu::ShaderSource::Glsl {
                shader: source.clone().into(),
                stage: wgpu::naga::ShaderStage::Compute,
                defines: Default::default(),
            },
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&shader.name),
            layout: Some(&self.gpu.pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(err) = pollster::block_on(device.pop_error_scope()) {
            return Err(format!(
                "voxel shader '{}' failed to compile: {err}",
                shader.name
            ));
        }
        self.pipelines.insert(source, pipeline);
        Ok(())
    }

    /// Run `map()` over every voxel of `grid` and return the resulting grid.
    pub fn run(
        &mut self,
        shader: &VoxelShader,
        grid: &VoxelGrid,
        inputs: &ShaderInputs,
    ) -> Result<VoxelGrid, String> {
        self.compile(shader)?;
        let pipeline = &self.pipelines[&build_compute_source(shader)];
        let GpuContext {
            device,
            queue,
            layout,
            ..
        } = &self.gpu;
        let d = grid.dimensions();
        let count = (d.x * d.y * d.z) as usize;

        let mut packed = vec![0u32; count.div_ceil(4).max(1)];
        for (i, &v) in grid.raw_data().iter().enumerate() {
            packed[i >> 2] |= (v as u32) << ((i & 3) * 8);
        }
        let mut params = GpuParams::zeroed();
        params.size = [d.x as f32, d.y as f32, d.z as f32, 0.0];
        params.misc = [
            inputs.color_index as f32,
            inputs.iter as f32,
            inputs.frame as f32,
            inputs.num_frames.max(1) as f32,
        ];
        params.rand = inputs.rand;
        let sels: Vec<u8> = if inputs.color_sels.is_empty() {
            vec![inputs.color_index]
        } else {
            inputs.color_sels.iter().take(256).copied().collect()
        };
        let b = |v: bool| v as u8 as f32;
        params.axis = [
            b(inputs.axis[0]),
            b(inputs.axis[1]),
            b(inputs.axis[2]),
            sels.len() as f32,
        ];
        params.mirror = [
            b(inputs.mirror[0]),
            b(inputs.mirror[1]),
            b(inputs.mirror[2]),
            0.0,
        ];
        params.args = inputs.args;
        for (k, &s) in sels.iter().enumerate() {
            params.sels[k] = s as f32;
        }
        params.palette = inputs.palette;

        use wgpu::util::DeviceExt;
        let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("xs_src"),
            contents: bytemuck::cast_slice(&packed),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (count.max(1) * 4) as u64;
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xs_dst"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("xs_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xs_staging"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xs_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: src.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: dst.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("xs_encoder"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xs_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(d.x.div_ceil(4), d.y.div_ceil(4), d.z.div_ceil(4));
        }
        enc.copy_buffer_to_buffer(&dst, 0, &staging, 0, out_size);
        queue.submit(Some(enc.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("voxel shader readback failed: {e}"))?;
        let mut out = VoxelGrid::new(d.x, d.y, d.z);
        {
            let mapped = slice.get_mapped_range();
            let values: &[u32] = bytemuck::cast_slice(&mapped);
            for (dst, &v) in out.raw_data_mut().iter_mut().zip(values) {
                *dst = v.min(255) as u8;
            }
        }
        staging.unmap();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOD: &str = r#"// xs flood [Height]
// xs_begin
// author : 'test'
// arg : { name = 'Height'  var = 'm_height'  range = '1 256'  value = '1'  step = '1'  precision = '0' }
// xs_end

float map(vec3 v) {
	float index = voxel(v);
	if (index == 0.0) {
		if (v.z < m_height) {
			return i_color_index;
		}
	}
	return index;
}
"#;

    /// Exercises what real MagicaVoxel shaders use: globals initialized from inputs,
    /// overloads, loops, local arrays, vector comparisons, `color_sel`, `i_axis`.
    const OUTLINE: &str = r#"// xs_begin
// arg : { name = 'Color'  var = 'm_color'  range = '0 255'  value = '7'  step = '1'  precision = '0' }
// arg : { name = 'Scale'  var = 'm_scale'  range = '0 100'  value = '50'  step = '1'  precision = '1' }
// xs_end

float unused_factor = m_scale / 100.0;
bool no_axis_mode = all(equal(ivec3(i_axis), ivec3(0)));

bool isSelectedPal(float p) {
	for (int i = 0; i < i_num_color_sels; i += 1) {
		if (p == color_sel(float(i))) {
			return true;
		}
	}
	return false;
}
bool isSelectedPal(vec3 v) { return isSelectedPal(voxel(v)); }

float map(vec3 v) {
	float index = voxel(v);
	float a[4];
	a[0] = index;
	if (index == 0.0 && no_axis_mode) {
		for (float x = -1.0; x <= 1.0; x += 1.0) {
			vec3 p = v + vec3(x, 0.0, 0.0);
			if (p != v && isSelectedPal(p)) {
				return m_color;
			}
		}
	}
	return a[0];
}
"#;

    #[test]
    fn parses_magicavoxel_arg_header() {
        let args = parse_args(OUTLINE);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].name, "Color");
        assert_eq!(args[0].var.as_deref(), Some("m_color"));
        assert_eq!(
            (args[0].min, args[0].max, args[0].default),
            (0.0, 255.0, 7.0)
        );
        assert_eq!(args[1].precision, 1);
        let shader = VoxelShader::from_source("outline", OUTLINE);
        assert_eq!(shader.arg_values(&[3.0])[..2], [3.0, 50.0]);
    }

    #[test]
    fn hoists_global_initializers_only() {
        let src = "const float K = 2.0;\nfloat t = m_a / 100.0;\nbvec3 b = equal(x, y);\n\
                   float f(float x);\nfloat g(float x) { float local = x * 2.0; return local; }\n\
                   int arr[2] = int[](1, 2);\nbool ok = a <= b;\n";
        let (out, init) = hoist_global_initializers(src);
        assert!(out.contains("const float K = 2.0;"));
        assert!(out.contains("float t;"));
        assert!(out.contains("bvec3 b;"));
        assert!(out.contains("float f(float x);"));
        assert!(
            out.contains("float local = x * 2.0;"),
            "function bodies untouched"
        );
        assert!(out.contains("int arr[2];"));
        assert!(init.contains("t = m_a / 100.0;"));
        assert!(init.contains("b = equal(x, y);"));
        assert!(init.contains("arr = int[](1, 2);"));
        assert!(init.contains("ok = a <= b;"));
        assert!(!init.contains("local"));
        assert!(out.contains("return 0.0;"), "fallback return added to g()");
        assert!(
            !out.contains("float f(float x);\n    return"),
            "not to prototypes"
        );
    }

    fn inputs(color: u8, args: [f32; MAX_SHADER_ARGS]) -> ShaderInputs {
        ShaderInputs {
            args,
            color_index: color,
            color_sels: vec![color],
            palette: [[0.0; 4]; 256],
            iter: 0,
            frame: 0,
            num_frames: 1,
            rand: [0.0; 4],
            axis: [false; 3],
            mirror: [false; 3],
        }
    }

    fn engine() -> Option<VoxelShaderEngine> {
        match VoxelShaderEngine::new() {
            Ok(e) => Some(e),
            Err(e) => {
                eprintln!("skipping GPU voxel shader test: {e}");
                None
            }
        }
    }

    #[test]
    fn runs_flood_on_gpu() {
        let Some(mut engine) = engine() else { return };
        let shader = VoxelShader::from_source("flood", FLOOD);
        let mut grid = VoxelGrid::new(5, 4, 6);
        grid.set(1, 1, 0, 9); // existing voxel is kept
        let out = engine
            .run(&shader, &grid, &inputs(3, shader.arg_values(&[2.0])))
            .expect("run flood");
        assert_eq!(out.get(1, 1, 0), 9);
        assert_eq!(out.get(0, 0, 0), 3);
        assert_eq!(out.get(4, 3, 1), 3);
        assert_eq!(out.get(0, 0, 2), 0, "only z < height");
        assert_eq!(out.count_solid(), 5 * 4 * 2);
    }

    #[test]
    fn runs_real_world_style_shader_on_gpu() {
        let Some(mut engine) = engine() else { return };
        let shader = VoxelShader::from_source("outline", OUTLINE);
        let mut grid = VoxelGrid::new(8, 3, 3);
        grid.set(4, 1, 1, 2);
        let out = engine
            .run(&shader, &grid, &inputs(2, shader.arg_values(&[])))
            .expect("run outline");
        assert_eq!(out.get(4, 1, 1), 2);
        assert_eq!(out.get(3, 1, 1), 7, "neighbor of a selected color");
        assert_eq!(out.get(5, 1, 1), 7);
        assert_eq!(out.get(4, 0, 1), 0, "only along x");
        assert_eq!(out.count_solid(), 3);
    }

    #[test]
    fn compile_errors_are_reported_not_panicking() {
        let Some(mut engine) = engine() else { return };
        let bad = VoxelShader::from_source("bad", "float map(vec3 v) { return undefined_thing; }");
        let err = engine.compile(&bad).unwrap_err();
        assert!(err.contains("bad"), "{err}");
    }
}

/// The sample shaders shipped in the repository's `shader/` folder compile, run, and
/// do what their header says.
#[cfg(test)]
#[test]
fn bundled_sample_shaders() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shader");
    let found = discover_shaders(&dir);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["grow", "hollow", "ripple", "terrain"]);
    let Ok(mut engine) = VoxelShaderEngine::new() else {
        eprintln!("skipping: no GPU");
        return;
    };
    let load = |name: &str| {
        let path = &found.iter().find(|(n, _)| n == name).unwrap().1;
        VoxelShader::load(name, path).unwrap()
    };
    let inputs = |shader: &VoxelShader, frame: u32, frames: u32| ShaderInputs {
        args: shader.arg_values(&[]),
        color_index: 4,
        color_sels: vec![4],
        palette: [[1.0; 4]; 256],
        iter: 0,
        frame,
        num_frames: frames,
        rand: [0.0; 4],
        axis: [false; 3],
        mirror: [false; 3],
    };

    // grow: one layer per iteration on top of the selected color.
    let grow = load("grow");
    let mut g = VoxelGrid::new(3, 3, 4);
    g.set(1, 1, 0, 4);
    let g1 = engine.run(&grow, &g, &inputs(&grow, 0, 1)).unwrap();
    assert_eq!((g1.get(1, 1, 1), g1.count_solid()), (4, 2));

    // hollow: a solid 6³ cube keeps only its shell.
    let hollow = load("hollow");
    let mut cube = VoxelGrid::new(6, 6, 6);
    cube.full(9);
    let shell = engine.run(&hollow, &cube, &inputs(&hollow, 0, 1)).unwrap();
    assert_eq!(shell.count_solid(), 6 * 6 * 6 - 4 * 4 * 4);

    // terrain: a filled column per (x, y), no floating voxels.
    let terrain = load("terrain");
    let t = engine
        .run(
            &terrain,
            &VoxelGrid::new(32, 32, 32),
            &inputs(&terrain, 0, 1),
        )
        .unwrap();
    assert!(t.count_solid() > 0);
    for x in 0..32 {
        for y in 0..32 {
            let top = (0..32).rev().find(|&z| t.get(x, y, z) != 0);
            if let Some(top) = top {
                assert!((0..=top).all(|z| t.get(x, y, z) == 4), "column {x},{y}");
            }
        }
    }

    // ripple: frames differ, and frame N wraps back to frame 0.
    let ripple = load("ripple");
    let empty = VoxelGrid::new(24, 24, 24);
    let f0 = engine.run(&ripple, &empty, &inputs(&ripple, 0, 4)).unwrap();
    let f1 = engine.run(&ripple, &empty, &inputs(&ripple, 1, 4)).unwrap();
    let f4 = engine.run(&ripple, &empty, &inputs(&ripple, 4, 4)).unwrap();
    assert_ne!(f0, f1);
    assert_eq!(f0, f4);
}

/// Compatibility check against a folder of real MagicaVoxel shaders:
/// `XS_DIR=path/to/shader cargo test -p mv-render xs_folder_compat -- --ignored --nocapture`
#[cfg(test)]
#[test]
#[ignore]
fn xs_folder_compat() {
    let Ok(dir) = std::env::var("XS_DIR") else {
        return;
    };
    let Ok(mut engine) = VoxelShaderEngine::new() else {
        return;
    };
    let mut grid = VoxelGrid::new(24, 24, 24);
    for z in 0..8 {
        for y in 4..20 {
            for x in 4..20 {
                grid.set(x, y, z, 1 + ((x + y) % 3) as u8);
            }
        }
    }
    let (mut ok, mut failed) = (0, Vec::new());
    for (name, path) in discover_shaders(Path::new(&dir)) {
        let shader = VoxelShader::load(&name, &path).expect("read shader");
        let inputs = ShaderInputs {
            args: shader.arg_values(&[]),
            color_index: 1,
            color_sels: vec![1, 2],
            palette: [[0.5; 4]; 256],
            iter: 0,
            frame: 0,
            num_frames: 1,
            rand: [0.25, 0.5, 0.75, 0.1],
            axis: [false; 3],
            mirror: [false; 3],
        };
        match engine.run(&shader, &grid, &inputs) {
            Ok(_) => ok += 1,
            Err(e) => failed.push(format!(
                "{name}: {}",
                e.lines().skip(3).take(30).collect::<Vec<_>>().join(" | ")
            )),
        }
    }
    eprintln!("compiled+ran {ok}, failed {}", failed.len());
    for f in &failed {
        eprintln!("  FAIL {f}");
    }
}
