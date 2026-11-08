use std::path::PathBuf;

use eframe::egui::{self, Color32, TextureHandle};
use mv_core::{CommandOutcome, EditorProject, Material};
use mv_format::{
    export_gltf, export_obj_greedy, export_ply_point_cloud, export_slice_png, load_vox_file,
    save_vox_file, GltfOptions,
};
use mv_render::{DisplaySettings, OrbitCamera, PathTracerEngine, PathTracerSettings, RayHit};

use crate::backup::AutoBackup;
use crate::shaders::ShaderPanel;
use crate::unsaved::PendingAction;
use crate::world_editor::WorldEditorState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewportMode {
    ModelEditor,
    WorldEditor,
    PathTracer,
}

pub(crate) struct MagicaVoxelApp {
    pub(crate) project: EditorProject,
    pub(crate) mode: ViewportMode,
    pub(crate) camera: OrbitCamera,
    pub(crate) display: DisplaySettings,
    pub(crate) pathtracer: PathTracerEngine,
    pub(crate) pt_settings: PathTracerSettings,
    pub(crate) copied_material: Option<Material>,
    pub(crate) viewport_texture: Option<TextureHandle>,
    pub(crate) rgba_buffer: Vec<u8>,
    pub(crate) last_render_dims: [usize; 2],
    pub(crate) hovered_hit: Option<RayHit>,
    pub(crate) drag_start_hit: Option<RayHit>,
    pub(crate) is_sculpting: bool,
    pub(crate) console_input: String,
    pub(crate) focus_console_next_frame: bool,
    pub(crate) history_cursor: Option<usize>,
    pub(crate) size_input: [u32; 3],
    pub(crate) ui_scale: f32,
    pub(crate) current_file: Option<PathBuf>,
    /// True while a swatch/material edit gesture is ongoing (one undo step per gesture).
    pub(crate) swatch_edit_in_progress: bool,
    pub(crate) world: WorldEditorState,
    /// `TAB` / `Shift+TAB` intercepted in `raw_input_hook` (value = shift held).
    pub(crate) pending_tab: Option<bool>,
    /// UI scale change requested from inside an input closure (applied after it).
    pub(crate) pending_ui_scale: Option<f32>,
    /// Screen rectangle of the 3D viewport during the last frame.
    pub(crate) viewport_rect: egui::Rect,
    pub(crate) anim_playing: bool,
    pub(crate) anim_fps: f32,
    /// `egui` time of the last playback step.
    pub(crate) anim_last_step: f64,
    pub(crate) shaders: ShaderPanel,
    pub(crate) backup: AutoBackup,
    /// Action waiting for the "unsaved changes" dialog.
    pub(crate) pending_action: Option<PendingAction>,
    /// Set once the user confirmed quitting (lets the next close request through).
    pub(crate) allow_close: bool,
}

impl MagicaVoxelApp {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(30, 32, 36);
        visuals.window_fill = Color32::from_rgb(34, 36, 40);
        cc.egui_ctx.set_visuals(visuals);
        Self::with_project(EditorProject::default())
    }

    /// Construct the app state without a window (also used by headless tests).
    pub(crate) fn with_project(project: EditorProject) -> Self {
        let project_root = project.scene.root();
        let d = project.grid.dimensions();
        let mut camera = OrbitCamera::default();
        camera.recenter(&project.grid);
        let pathtracer = PathTracerEngine::new();

        Self {
            project,
            mode: ViewportMode::ModelEditor,
            camera,
            display: DisplaySettings {
                render_scale: 2,
                ..Default::default()
            },
            pathtracer,
            pt_settings: PathTracerSettings::default(),
            copied_material: None,
            viewport_texture: None,
            rgba_buffer: Vec::new(),
            last_render_dims: [0, 0],
            hovered_hit: None,
            drag_start_hit: None,
            is_sculpting: false,
            console_input: String::new(),
            focus_console_next_frame: false,
            history_cursor: None,
            size_input: [d.x, d.y, d.z],
            ui_scale: 1.0,
            current_file: None,
            swatch_edit_in_progress: false,
            world: WorldEditorState::new(project_root),
            pending_tab: None,
            pending_ui_scale: None,
            viewport_rect: egui::Rect::NOTHING,
            anim_playing: false,
            anim_fps: 8.0,
            anim_last_step: 0.0,
            shaders: ShaderPanel::new(),
            backup: AutoBackup::new(),
            pending_action: None,
            allow_close: false,
        }
    }

    pub(crate) fn sync_size_input(&mut self) {
        let d = self.project.grid.dimensions();
        self.size_input = [d.x, d.y, d.z];
    }

    pub(crate) fn execute_command_str(&mut self, cmd: &str) {
        match self.project.run_command(cmd) {
            Ok(CommandOutcome::ExportRequested { format, .. }) => {
                self.handle_export(&format);
            }
            Ok(CommandOutcome::ExternalShader(inv)) => {
                let msg = match self.run_external_shader(&inv) {
                    Ok(m) => m,
                    Err(e) => format!("[ERR] {e}"),
                };
                self.project
                    .console_logs
                    .push(format!("> {} : {}", cmd.trim(), msg));
            }
            Ok(_) => {
                self.sync_size_input();
                self.pathtracer.reset_accumulation();
            }
            Err(_) => {}
        }
    }

    pub(crate) fn handle_open_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MagicaVoxel", &["vox"])
            .pick_file()
        {
            self.request_open_path(path);
        }
    }

    /// Load a `.vox` file into the editor (open dialog, drag & drop).
    pub(crate) fn open_vox_path(&mut self, path: PathBuf) {
        {
            match load_vox_file(&path) {
                Ok(proj) => {
                    self.project = proj;
                    self.backup.reset();
                    self.reset_world_state();
                    if self.mode == ViewportMode::WorldEditor {
                        self.frame_world_camera();
                    } else {
                        self.camera.recenter(&self.project.grid);
                    }
                    self.sync_size_input();
                    self.pathtracer.reset_accumulation();
                    self.current_file = Some(path.clone());
                    self.project
                        .console_logs
                        .push(format!("Opened {}", path.display()));
                }
                Err(e) => {
                    self.project.console_logs.push(format!(
                        "[ERR] Failed to open {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }
    }

    pub(crate) fn handle_save_project(&mut self, save_as: bool) {
        let path = if !save_as {
            self.current_file.clone().or_else(|| {
                rfd::FileDialog::new()
                    .set_file_name(&format!("{}.vox", self.project.name))
                    .add_filter("MagicaVoxel", &["vox"])
                    .save_file()
            })
        } else {
            rfd::FileDialog::new()
                .set_file_name(&format!("{}.vox", self.project.name))
                .add_filter("MagicaVoxel", &["vox"])
                .save_file()
        };

        if let Some(p) = path {
            match save_vox_file(&self.project, &p) {
                Ok(()) => {
                    self.project.dirty = false;
                    self.current_file = Some(p.clone());
                    self.project
                        .console_logs
                        .push(format!("Saved project to {}", p.display()));
                }
                Err(e) => {
                    self.project.console_logs.push(format!(
                        "[ERR] Failed to save {}: {}",
                        p.display(),
                        e
                    ));
                }
            }
        }
    }

    pub(crate) fn handle_save_screenshot(&mut self) {
        let [w, h] = self.last_render_dims;
        if w == 0 || h == 0 || self.rgba_buffer.len() != w * h * 4 {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&format!("{}_render.png", self.project.name))
            .add_filter("PNG Image", &["png"])
            .save_file()
        {
            if let Some(img) =
                image::RgbaImage::from_raw(w as u32, h as u32, self.rgba_buffer.clone())
            {
                match img.save(&path) {
                    Ok(()) => self.project.console_logs.push(format!(
                        "Saved screenshot ({}x{}) to {}",
                        w,
                        h,
                        path.display()
                    )),
                    Err(e) => self
                        .project
                        .console_logs
                        .push(format!("[ERR] Failed to save screenshot: {}", e)),
                }
            }
        }
    }

    pub(crate) fn handle_export(&mut self, format: &str) {
        let fmt = format.to_ascii_lowercase();
        let ext = match fmt.as_str() {
            "obj" => "obj",
            "ply" => "ply",
            "glb" => "glb",
            "gltf" => "gltf",
            "slice" | "png" => "png",
            _ => "vox",
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&format!("{}.{}", self.project.name, ext))
            .add_filter(ext, &[ext])
            .save_file()
        {
            let res = match ext {
                "glb" | "gltf" => {
                    export_gltf(&self.project, &path, &GltfOptions::default()).map(|doc| {
                        format!(
                            "Exported glTF scene ({} nodes, {} meshes, {} triangles) to {}",
                            doc.nodes.len(),
                            doc.meshes.len(),
                            doc.triangle_count(),
                            path.display()
                        )
                    })
                }
                "obj" => export_obj_greedy(&self.project, &path)
                    .map(|q| format!("Exported OBJ ({} greedy quads) to {}", q, path.display())),
                "ply" => export_ply_point_cloud(&self.project, &path)
                    .map(|pts| format!("Exported PLY ({} points) to {}", pts, path.display())),
                "png" => export_slice_png(&self.project, &path)
                    .map(|()| format!("Exported 2D slice PNG to {}", path.display())),
                _ => save_vox_file(&self.project, &path)
                    .map(|()| format!("Exported VOX to {}", path.display())),
            };
            match res {
                Ok(msg) => self.project.console_logs.push(msg),
                Err(e) => self
                    .project
                    .console_logs
                    .push(format!("[ERR] Export failed: {}", e)),
            }
        }
    }
}

impl eframe::App for MagicaVoxelApp {
    /// Intercept `TAB` before egui uses it for keyboard focus navigation, unless a
    /// widget (e.g. the console) has focus.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        raw_input.events.retain(|e| match e {
            egui::Event::Key {
                key: egui::Key::Tab,
                pressed,
                modifiers,
                ..
            } => {
                if *pressed {
                    self.pending_tab = Some(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }

    /// Let a running auto-backup finish writing before the process exits.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.wait_backup();
    }
}

impl MagicaVoxelApp {
    /// One UI frame (separate from `eframe::App::update` so tests can drive it).
    pub(crate) fn frame(&mut self, ctx: &egui::Context) {
        self.handle_close_request(ctx);
        if let Some(shift) = self.pending_tab.take() {
            if self.pending_action.is_none() {
                self.handle_tab(shift);
            }
        }
        let world_before =
            (self.mode == ViewportMode::WorldEditor).then(|| self.project.scene.structure());

        self.handle_global_shortcuts(ctx);
        self.tick_animation(ctx);
        if let Some(scale) = self.pending_ui_scale.take() {
            ctx.set_pixels_per_point(scale);
        }
        // Panel order matters in egui: top/bottom/side panels before the central viewport.
        self.show_top_bar(ctx);
        self.show_console_panel(ctx);
        self.show_timeline(ctx);
        self.show_left_panel(ctx);
        self.show_right_panel(ctx);
        self.show_viewport(ctx);
        self.handle_dropped_files(ctx);

        self.show_unsaved_dialog(ctx);

        self.finish_world_frame(ctx, world_before);
        self.tick_auto_backup();
    }

    /// Drag & drop: open a dropped `.vox` file; show a hint while files hover the window.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let (dropped, hovering) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i
                .raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect();
            (dropped, !i.raw.hovered_files.is_empty())
        });
        if hovering {
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(150));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop a .vox file to open it",
                egui::FontId::proportional(26.0),
                Color32::WHITE,
            );
        }
        let is_vox = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vox"));
        match dropped.iter().find(|p| is_vox(p)) {
            Some(path) => self.request_open_path(path.clone()),
            None => {
                for p in &dropped {
                    self.project.console_logs.push(format!(
                        "[ERR] Unsupported file dropped: {} (only .vox for now)",
                        p.display()
                    ));
                }
            }
        }
    }
}
