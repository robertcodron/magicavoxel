//! External voxel shaders in the application: discovery of `shader/*.txt` files, the
//! lazily created GPU engine, and the "Voxel Shader" panel state.
//!
//! Search folders, in priority order: `MV_SHADER_DIR` (if set), `./shader`,
//! `./shaders`, then `shader/` next to the executable.

use std::path::PathBuf;

use mv_core::{XsInvocation, XsMode, BUILTIN_SHADERS};
use mv_render::{discover_shaders, ShaderInputs, VoxelShader, VoxelShaderEngine};

use crate::app::MagicaVoxelApp;

pub(crate) struct ShaderPanel {
    pub(crate) dirs: Vec<PathBuf>,
    /// `(name, path)` of the shader files found, sorted by name.
    pub(crate) files: Vec<(String, PathBuf)>,
    /// Selected shader name (built-in or file).
    pub(crate) selected: String,
    pub(crate) loaded: Option<VoxelShader>,
    pub(crate) arg_values: Vec<f32>,
    pub(crate) iterations: u32,
    pub(crate) mode: XsMode,
    engine: Option<VoxelShaderEngine>,
    seed: u64,
}

impl ShaderPanel {
    pub(crate) fn new() -> Self {
        let mut panel = Self {
            dirs: Vec::new(),
            files: Vec::new(),
            selected: String::new(),
            loaded: None,
            arg_values: Vec::new(),
            iterations: 1,
            mode: XsMode::InPlace,
            engine: None,
            seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9, |d| d.as_nanos() as u64),
        };
        panel.refresh();
        panel
    }

    fn search_dirs() -> Vec<PathBuf> {
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

    /// Rescan the shader folders (first folder wins for duplicate names).
    pub(crate) fn refresh(&mut self) {
        self.dirs = Self::search_dirs()
            .into_iter()
            .filter(|d| d.is_dir())
            .collect();
        self.files.clear();
        for dir in &self.dirs {
            for (name, path) in discover_shaders(dir) {
                if !self.files.iter().any(|(n, _)| *n == name) {
                    self.files.push((name, path));
                }
            }
        }
        self.files.sort();
        if !self.selected.is_empty() {
            let name = self.selected.clone();
            self.select(&name);
        }
    }

    pub(crate) fn path_of(&self, name: &str) -> Option<&PathBuf> {
        self.files
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, p)| p)
    }

    /// Select a shader for the panel and reset its arguments to the declared defaults.
    pub(crate) fn select(&mut self, name: &str) {
        self.selected = name.to_string();
        self.loaded = self
            .path_of(name)
            .and_then(|p| VoxelShader::load(name, p).ok());
        self.arg_values = self
            .loaded
            .as_ref()
            .map(|s| s.args.iter().map(|a| a.default).collect())
            .unwrap_or_default();
    }

    /// Invocation for the panel's current settings.
    pub(crate) fn invocation(&self) -> Option<XsInvocation> {
        (!self.selected.is_empty()).then(|| XsInvocation {
            shader: self.selected.clone(),
            args: self.arg_values.clone(),
            iterations: self.iterations.max(1),
            mode: self.mode,
        })
    }

    pub(crate) fn builtin_names() -> &'static [&'static str] {
        BUILTIN_SHADERS
    }

    fn next_rand(&mut self) -> f32 {
        // xorshift64*
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let v = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }
}

impl MagicaVoxelApp {
    /// Run a non-built-in `xs` shader from the shader folders on the GPU.
    pub(crate) fn run_external_shader(&mut self, inv: &XsInvocation) -> Result<String, String> {
        let path = match self.shaders.path_of(&inv.shader) {
            Some(p) => p.clone(),
            None => {
                self.shaders.refresh();
                self.shaders.path_of(&inv.shader).cloned().ok_or_else(|| {
                    let dirs: Vec<String> = ShaderPanel::search_dirs()
                        .iter()
                        .map(|d| d.display().to_string())
                        .collect();
                    format!(
                        "Unknown voxel shader '{}' (built-ins: {}; searched {})",
                        inv.shader,
                        BUILTIN_SHADERS.join(", "),
                        dirs.join(", ")
                    )
                })?
            }
        };
        let shader = VoxelShader::load(&inv.shader, &path).map_err(|e| e.to_string())?;
        if self.shaders.engine.is_none() {
            self.shaders.engine = Some(VoxelShaderEngine::new()?);
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

        let panel = &mut self.shaders;
        let result = self.project.run_xs(inv, |grid, step| {
            base.iter = step.iter;
            base.frame = step.frame;
            base.num_frames = step.num_frames;
            base.rand = [
                panel.next_rand(),
                panel.next_rand(),
                panel.next_rand(),
                panel.next_rand(),
            ];
            let engine = panel.engine.as_mut().expect("engine created above");
            *grid = engine.run(&shader, grid, &base)?;
            Ok(())
        });
        if result.is_ok() {
            self.sync_size_input();
            self.pathtracer.reset_accumulation();
        }
        result
    }
}
