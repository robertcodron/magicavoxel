use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{
    self, Color32, ColorImage, Key, PointerButton, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Vec2 as EguiVec2,
};
use glam::Vec2;
use mv_core::{
    BrushAction, BrushMode, CommandOutcome, EditorProject, Material, MaterialType, VoxelSubShape,
};
use mv_format::{
    export_obj_greedy, export_ply_point_cloud, export_slice_png, load_vox_file, save_vox_file,
};
use mv_render::{
    pick_voxel, render_viewport_rgba, DisplaySettings, OrbitCamera, PathTracerEngine,
    PathTracerSettings, RayHit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportMode {
    ModelEditor,
    PathTracer,
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MagicaVoxel - Rust Edition (Model Editor + GPU Path Tracer)")
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MagicaVoxel Rust",
        native_options,
        Box::new(|cc| Ok(Box::new(MagicaVoxelApp::new(cc)))),
    )
}

struct MagicaVoxelApp {
    project: EditorProject,
    mode: ViewportMode,
    camera: OrbitCamera,
    display: DisplaySettings,
    pathtracer: PathTracerEngine,
    pt_settings: PathTracerSettings,
    copied_material: Option<Material>,
    viewport_texture: Option<TextureHandle>,
    rgba_buffer: Vec<u8>,
    last_render_dims: [usize; 2],
    hovered_hit: Option<RayHit>,
    drag_start_hit: Option<RayHit>,
    is_sculpting: bool,
    console_input: String,
    focus_console_next_frame: bool,
    history_cursor: Option<usize>,
    size_input: [u32; 3],
    ui_scale: f32,
    current_file: Option<PathBuf>,
}

impl MagicaVoxelApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(30, 32, 36);
        visuals.window_fill = Color32::from_rgb(34, 36, 40);
        cc.egui_ctx.set_visuals(visuals);

        let project = EditorProject::default();
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
        }
    }

    fn sync_size_input(&mut self) {
        let d = self.project.grid.dimensions();
        self.size_input = [d.x, d.y, d.z];
    }

    fn execute_command_str(&mut self, cmd: &str) {
        match self.project.run_command(cmd) {
            Ok(CommandOutcome::ExportRequested { format, .. }) => {
                self.handle_export(&format);
            }
            Ok(_) => {
                self.sync_size_input();
                self.pathtracer.reset_accumulation();
            }
            Err(_) => {}
        }
    }

    fn handle_open_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MagicaVoxel", &["vox"])
            .pick_file()
        {
            match load_vox_file(&path) {
                Ok(proj) => {
                    self.project = proj;
                    self.camera.recenter(&self.project.grid);
                    self.sync_size_input();
                    self.pathtracer.reset_accumulation();
                    self.current_file = Some(path.clone());
                    self.project
                        .console_logs
                        .push(format!("Opened {}", path.display()));
                }
                Err(e) => {
                    self.project
                        .console_logs
                        .push(format!("[ERR] Failed to open {}: {}", path.display(), e));
                }
            }
        }
    }

    fn handle_save_project(&mut self, save_as: bool) {
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
                    self.project
                        .console_logs
                        .push(format!("[ERR] Failed to save {}: {}", p.display(), e));
                }
            }
        }
    }

    fn handle_save_screenshot(&mut self) {
        let [w, h] = self.last_render_dims;
        if w == 0 || h == 0 || self.rgba_buffer.len() != w * h * 4 {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&format!("{}_render.png", self.project.name))
            .add_filter("PNG Image", &["png"])
            .save_file()
        {
            if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, self.rgba_buffer.clone()) {
                match img.save(&path) {
                    Ok(()) => self
                        .project
                        .console_logs
                        .push(format!("Saved screenshot ({}x{}) to {}", w, h, path.display())),
                    Err(e) => self
                        .project
                        .console_logs
                        .push(format!("[ERR] Failed to save screenshot: {}", e)),
                }
            }
        }
    }

    fn handle_export(&mut self, format: &str) {
        let fmt = format.to_ascii_lowercase();
        let ext = match fmt.as_str() {
            "obj" => "obj",
            "ply" => "ply",
            "slice" | "png" => "png",
            _ => "vox",
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&format!("{}.{}", self.project.name, ext))
            .add_filter(ext, &[ext])
            .save_file()
        {
            let res = match ext {
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

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let text_focused = ctx.memory(|m| m.focused().is_some());

        ctx.input(|i| {
            if i.key_pressed(Key::F1) || (!text_focused && i.key_pressed(Key::Tab)) {
                self.focus_console_next_frame = true;
            }
            if !text_focused && (i.key_pressed(Key::F6) || i.key_pressed(Key::Num6)) {
                self.handle_save_screenshot();
            }
        });

        if text_focused {
            return;
        }

        ctx.input(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            let shift = i.modifiers.shift;

            // Global Undo / Redo / File shortcuts
            if cmd && !shift && i.key_pressed(Key::Z) {
                if self.project.undo() {
                    self.sync_size_input();
                }
            } else if (cmd && shift && i.key_pressed(Key::Z)) || (cmd && i.key_pressed(Key::Y)) {
                if self.project.redo() {
                    self.sync_size_input();
                }
            } else if cmd && i.key_pressed(Key::S) {
                self.handle_save_project(shift);
            } else if cmd && i.key_pressed(Key::O) {
                self.handle_open_project();
            } else if cmd && i.key_pressed(Key::P) {
                self.project.record_undo();
                self.project.grid.clear();
                self.project.selection.clear();
            } else if self.mode == ViewportMode::PathTracer {
                // Renderer material shortcuts (`mv_controls.html`: Ctrl+R reset, Ctrl+C copy, Ctrl+V paste)
                let sel = self.project.palette.selected as usize;
                if cmd && i.key_pressed(Key::R) {
                    self.project.materials[sel] = Material {
                        id: sel as u8,
                        ..Default::default()
                    };
                } else if cmd && i.key_pressed(Key::C) {
                    self.copied_material = Some(self.project.materials[sel]);
                } else if cmd && i.key_pressed(Key::V) {
                    if let Some(mut m) = self.copied_material {
                        m.id = sel as u8;
                        self.project.materials[sel] = m;
                    }
                }
            } else if cmd && i.key_pressed(Key::A) {
                self.execute_command_str("sel_all");
            } else if cmd && i.key_pressed(Key::I) {
                self.execute_command_str("sel_inv");
            } else if cmd && i.key_pressed(Key::D) {
                self.execute_command_str("sel_none");
            } else if cmd && i.key_pressed(Key::C) {
                self.execute_command_str("copy");
            } else if cmd && i.key_pressed(Key::X) {
                self.execute_command_str("cut");
            } else if cmd && i.key_pressed(Key::V) {
                self.execute_command_str("paste");
            }

            // Display shortcuts (Ctrl+E/G/F/U/B/W/M and Ctrl +/- UI scale)
            if cmd && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)) {
                self.ui_scale = (self.ui_scale + 0.1).clamp(0.75, 2.0);
                ctx.set_pixels_per_point(self.ui_scale);
            }
            if cmd && i.key_pressed(Key::Minus) {
                self.ui_scale = (self.ui_scale - 0.1).clamp(0.75, 2.0);
                ctx.set_pixels_per_point(self.ui_scale);
            }
            if cmd && i.key_pressed(Key::E) {
                self.display.show_edges = !self.display.show_edges;
            }
            if cmd && i.key_pressed(Key::G) {
                self.display.show_grid = !self.display.show_grid;
            }
            if cmd && i.key_pressed(Key::F) {
                self.display.show_frame = !self.display.show_frame;
            }
            if cmd && i.key_pressed(Key::U) {
                self.display.show_ground = !self.display.show_ground;
            }
            if cmd && i.key_pressed(Key::B) {
                self.display.show_background = !self.display.show_background;
            }
            if cmd && i.key_pressed(Key::W) {
                self.display.show_shadows = !self.display.show_shadows;
            }
            if cmd && i.key_pressed(Key::M) {
                self.display.show_wireframe = !self.display.show_wireframe;
            }

            // Axis mode (Ctrl+1/2/3) vs Mirror mode (1/2/3)
            if cmd && i.key_pressed(Key::Num1) {
                self.project.brush.axis.x = !self.project.brush.axis.x;
            } else if !cmd && i.key_pressed(Key::Num1) {
                self.project.brush.mirror.x = !self.project.brush.mirror.x;
            }
            if cmd && i.key_pressed(Key::Num2) {
                self.project.brush.axis.y = !self.project.brush.axis.y;
            } else if !cmd && i.key_pressed(Key::Num2) {
                self.project.brush.mirror.y = !self.project.brush.mirror.y;
            }
            if cmd && i.key_pressed(Key::Num3) {
                self.project.brush.axis.z = !self.project.brush.axis.z;
            } else if !cmd && i.key_pressed(Key::Num3) {
                self.project.brush.mirror.z = !self.project.brush.mirror.z;
            }

            // Camera shortcuts (4 / F4 recenter, 5 / F5 90-deg view, 7 / F7 save, 8 / F8 load)
            if !cmd && (i.key_pressed(Key::Num4) || i.key_pressed(Key::F4)) {
                self.camera.recenter(&self.project.grid);
            }
            if !cmd && (i.key_pressed(Key::Num5) || i.key_pressed(Key::F5)) {
                self.camera.snap_to_90_view();
            }
            if !cmd && (i.key_pressed(Key::Num7) || i.key_pressed(Key::F7)) {
                let slot = self.camera.active_slot;
                self.camera.save_slot(slot);
            }
            if !cmd && (i.key_pressed(Key::Num8) || i.key_pressed(Key::F8)) {
                let slot = self.camera.active_slot;
                self.camera.load_slot(slot);
            }

            // Brush Action & Shape shortcuts (when Ctrl is not held)
            if !cmd && self.mode == ViewportMode::ModelEditor {
                if i.key_pressed(Key::T) {
                    self.project.brush.action = BrushAction::Attach;
                }
                if i.key_pressed(Key::R) {
                    self.project.brush.action = BrushAction::Erase;
                }
                if i.key_pressed(Key::G) {
                    self.project.brush.action = BrushAction::Paint;
                }
                if i.key_pressed(Key::N) {
                    self.project.brush.action = BrushAction::Select;
                }
                if i.key_pressed(Key::V) {
                    self.project.brush.mode = BrushMode::Voxel;
                }
                if i.key_pressed(Key::F) {
                    self.project.brush.mode = BrushMode::Face;
                }
                if i.key_pressed(Key::B) {
                    self.project.brush.mode = BrushMode::Box;
                }
                if i.key_pressed(Key::L) {
                    self.project.brush.mode = BrushMode::Line;
                }
                if i.key_pressed(Key::C) {
                    self.project.brush.mode = BrushMode::Center;
                }
                if i.key_pressed(Key::P) {
                    self.project.brush.mode = BrushMode::Pattern;
                }
                if i.key_pressed(Key::U) {
                    self.execute_command_str("full");
                }
                if i.key_pressed(Key::I) {
                    self.execute_command_str("fill");
                }
                if i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete) {
                    if self.project.delete_selected() == 0 {
                        self.execute_command_str("zero");
                    }
                }
                if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                    self.project.brush.size = (self.project.brush.size + 1).clamp(1, 9);
                }
                if i.key_pressed(Key::Minus) {
                    self.project.brush.size = (self.project.brush.size - 1).clamp(1, 9);
                }
            }
        });
    }
}

impl eframe::App for MagicaVoxelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_global_shortcuts(ctx);

        // 1. TOP BAR
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading(
                    RichText::new("MagicaVoxel RS")
                        .strong()
                        .color(Color32::from_rgb(255, 185, 70)),
                );
                ui.separator();

                // Mode Switcher: Model Editor vs GPU Path Tracer Renderer
                ui.selectable_value(
                    &mut self.mode,
                    ViewportMode::ModelEditor,
                    RichText::new("✏ Model").strong(),
                );
                ui.selectable_value(
                    &mut self.mode,
                    ViewportMode::PathTracer,
                    RichText::new("✨ Render (Path Tracer)").strong(),
                );

                ui.separator();
                ui.label("Project:");
                ui.add(egui::TextEdit::singleline(&mut self.project.name).desired_width(90.0));
                if self.project.dirty {
                    ui.colored_label(Color32::from_rgb(255, 160, 50), "●");
                }

                if ui.button("📂 Open").on_hover_text("Open .vox (Ctrl+O)").clicked() {
                    self.handle_open_project();
                }
                if ui.button("💾 Save").on_hover_text("Save .vox (Ctrl+S)").clicked() {
                    self.handle_save_project(false);
                }
                if ui.button("📷 Snap").on_hover_text("Save Screenshot PNG (F6)").clicked() {
                    self.handle_save_screenshot();
                }
                ui.menu_button("📤 Export", |ui| {
                    if ui.button("Wavefront .obj (Greedy Mesh)").clicked() {
                        ui.close_menu();
                        self.handle_export("obj");
                    }
                    if ui.button("Point Cloud .ply").clicked() {
                        ui.close_menu();
                        self.handle_export("ply");
                    }
                    if ui.button("2D Slice Strip .png").clicked() {
                        ui.close_menu();
                        self.handle_export("slice");
                    }
                });

                ui.separator();
                if ui
                    .add_enabled(self.project.history.can_undo(), egui::Button::new("↩ Undo"))
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                {
                    self.project.undo();
                    self.sync_size_input();
                }
                if ui
                    .add_enabled(self.project.history.can_redo(), egui::Button::new("↪ Redo"))
                    .on_hover_text("Ctrl+Y / Ctrl+Shift+Z")
                    .clicked()
                {
                    self.project.redo();
                    self.sync_size_input();
                }

                if self.mode == ViewportMode::PathTracer {
                    ui.separator();
                    let pause_label = if self.pt_settings.paused {
                        "▶ Resume"
                    } else {
                        "⏸ Pause"
                    };
                    if ui.button(pause_label).clicked() {
                        self.pt_settings.paused = !self.pt_settings.paused;
                    }
                    if ui.button("🔄 Restart").clicked() {
                        self.pathtracer.reset_accumulation();
                    }
                    let progress = (self.pathtracer.current_samples as f32
                        / self.pt_settings.target_samples.max(1) as f32)
                        .clamp(0.0, 1.0);
                    let backend_tag = if self.pathtracer.is_gpu_accelerated() {
                        "wgpu GPU"
                    } else {
                        "CPU"
                    };
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .desired_width(150.0)
                            .text(format!(
                                "{} / {} spp ({})",
                                self.pathtracer.current_samples,
                                self.pt_settings.target_samples,
                                backend_tag
                            )),
                    );
                } else {
                    ui.separator();
                    ui.label("Size:");
                    let mut size_changed = false;
                    for idx in 0..3 {
                        size_changed |= ui
                            .add(egui::DragValue::new(&mut self.size_input[idx]).range(1..=256))
                            .lost_focus();
                    }
                    if size_changed || ui.button("Apply").clicked() {
                        let [sx, sy, sz] = self.size_input;
                        self.project.record_undo();
                        self.project.resize_canvas(sx, sy, sz);
                    }
                }
            });
        });

        // 2. BOTTOM CONSOLE PANEL (`mv_commands.html`)
        egui::TopBottomPanel::bottom("console_panel")
            .resizable(true)
            .default_height(125.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Console (TAB / F1)");
                    ui.separator();
                    if let Some(hit) = self.hovered_hit {
                        let c = self
                            .project
                            .grid
                            .get_i32(hit.voxel.x, hit.voxel.y, hit.voxel.z);
                        ui.colored_label(
                            Color32::from_rgb(130, 210, 255),
                            format!(
                                "Voxel: ({}, {}, {}) | Dist: {:.1} | Color: #{}",
                                hit.voxel.x, hit.voxel.y, hit.voxel.z, hit.t, c
                            ),
                        );
                    } else {
                        ui.label("Voxel: (-, -, -)");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let d = self.project.grid.dimensions();
                        ui.label(format!(
                            "Canvas: {}x{}x{} | Solid: {} | Undo: {}",
                            d.x,
                            d.y,
                            d.z,
                            self.project.grid.count_solid(),
                            self.project.history.undo_count()
                        ));
                    });
                });

                egui::ScrollArea::vertical()
                    .max_height(64.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for log in &self.project.console_logs {
                            let color = if log.contains("[ERR]") {
                                Color32::from_rgb(255, 110, 110)
                            } else {
                                Color32::from_rgb(200, 205, 215)
                            };
                            ui.colored_label(color, log);
                        }
                    });

                ui.horizontal(|ui| {
                    ui.label(">");
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.console_input)
                            .hint_text("Enter command (e.g. 'xs gyroid', 'flood 0', 'mir x', 'rot90 z', 'scale 2.0', 'o obj', 'help')...")
                            .desired_width(ui.available_width() - 80.0),
                    );
                    if self.focus_console_next_frame {
                        edit.request_focus();
                        self.focus_console_next_frame = false;
                    }
                    if edit.has_focus() {
                        if ui.input(|i| i.key_pressed(Key::ArrowUp))
                            && !self.project.console_history.is_empty()
                        {
                            let next = match self.history_cursor {
                                Some(c) => c.saturating_sub(1),
                                None => self.project.console_history.len() - 1,
                            };
                            self.history_cursor = Some(next);
                            self.console_input = self.project.console_history[next].clone();
                        }
                        if ui.input(|i| i.key_pressed(Key::ArrowDown))
                            && !self.project.console_history.is_empty()
                        {
                            if let Some(c) = self.history_cursor {
                                if c + 1 < self.project.console_history.len() {
                                    self.history_cursor = Some(c + 1);
                                    self.console_input = self.project.console_history[c + 1].clone();
                                } else {
                                    self.history_cursor = None;
                                    self.console_input.clear();
                                }
                            }
                        }
                    }
                    let submitted = (edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                        || ui.button("Run").clicked();
                    if submitted {
                        let cmd = self.console_input.trim().to_string();
                        self.console_input.clear();
                        self.history_cursor = None;
                        if !cmd.is_empty() {
                            self.execute_command_str(&cmd);
                        }
                    }
                });
            });

        // 3. LEFT PANEL: PALETTE + PBR MATTER + BRUSH SETTINGS
        egui::SidePanel::left("left_palette_brush_panel")
            .resizable(true)
            .default_width(285.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Palette (256)");
                    ui.horizontal(|ui| {
                        if ui.small_button("Default").clicked() {
                            self.execute_command_str("pal mac");
                        }
                        if ui.small_button("Grey").clicked() {
                            self.execute_command_str("pal bw");
                        }
                        if ui.small_button("Mask Unused").clicked() {
                            self.execute_command_str("pal mask");
                        }
                        if ui.small_button("Sort HSV").clicked() {
                            self.execute_command_str("pal sort hsv");
                        }
                    });

                    // 8 columns x 32 rows swatch grid
                    let swatch_size = EguiVec2::new(25.0, 12.5);
                    egui::Grid::new("palette_grid")
                        .spacing([2.0, 2.0])
                        .show(ui, |ui| {
                            for row in 0..32u8 {
                                for col in 0..8u8 {
                                    let idx = row
                                        .saturating_mul(8)
                                        .saturating_add(col)
                                        .saturating_add(1);
                                    if idx == 0 {
                                        continue;
                                    }
                                    let rgba = self.project.palette.colors[idx as usize];
                                    let mat_kind = self.project.materials[idx as usize].kind;
                                    let c32 = Color32::from_rgb(rgba.r, rgba.g, rgba.b);
                                    let (rect, resp) =
                                        ui.allocate_exact_size(swatch_size, Sense::click());
                                    ui.painter().rect_filled(rect, 2.0, c32);
                                    if mat_kind != MaterialType::Diffuse {
                                        // Small corner indicator for non-diffuse PBR materials
                                        let badge_col = match mat_kind {
                                            MaterialType::Emit => Color32::from_rgb(255, 230, 80),
                                            MaterialType::Metal => Color32::from_rgb(200, 220, 255),
                                            MaterialType::Glass => Color32::from_rgb(100, 240, 255),
                                            _ => Color32::WHITE,
                                        };
                                        ui.painter().circle_filled(
                                            rect.right_top() + egui::vec2(-3.5, 3.5),
                                            2.2,
                                            badge_col,
                                        );
                                    }
                                    if self.project.palette.selected == idx {
                                        ui.painter().rect_stroke(
                                            rect,
                                            2.0,
                                            Stroke::new(2.0, Color32::WHITE),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    if resp.clicked() {
                                        self.project.palette.selected = idx;
                                    }
                                    resp.on_hover_text(format!(
                                        "Swatch #{}\nRGB({}, {}, {})\nMaterial: {:?}",
                                        idx, rgba.r, rgba.g, rgba.b, mat_kind
                                    ));
                                }
                                ui.end_row();
                            }
                        });

                    ui.separator();
                    let sel = self.project.palette.selected as usize;
                    let cur_col = self.project.palette.colors[sel];
                    let mut rgb = [cur_col.r, cur_col.g, cur_col.b];
                    ui.horizontal(|ui| {
                        ui.strong(format!("Swatch #{}:", sel));
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            self.project.palette.colors[sel].r = rgb[0];
                            self.project.palette.colors[sel].g = rgb[1];
                            self.project.palette.colors[sel].b = rgb[2];
                        }
                        if ui.small_button("Copy Mat").on_hover_text("Ctrl+C in Renderer").clicked() {
                            self.copied_material = Some(self.project.materials[sel]);
                        }
                        if ui.small_button("Paste").on_hover_text("Ctrl+V in Renderer").clicked() {
                            if let Some(mut m) = self.copied_material {
                                m.id = sel as u8;
                                self.project.materials[sel] = m;
                            }
                        }
                        if ui.small_button("Reset").on_hover_text("Ctrl+R in Renderer").clicked() {
                            self.project.materials[sel] = Material {
                                id: sel as u8,
                                ..Default::default()
                            };
                        }
                    });

                    // Full PBR Material Editor (`Matter`)
                    let mat = &mut self.project.materials[sel];
                    ui.horizontal_wrapped(|ui| {
                        for (kind, label) in [
                            (MaterialType::Diffuse, "Diffuse"),
                            (MaterialType::Metal, "Metal"),
                            (MaterialType::Glass, "Glass"),
                            (MaterialType::Emit, "Emit"),
                            (MaterialType::Cloud, "Cloud"),
                        ] {
                            ui.selectable_value(&mut mat.kind, kind, label);
                        }
                    });
                    match mat.kind {
                        MaterialType::Diffuse | MaterialType::Blend => {
                            ui.add(egui::Slider::new(&mut mat.rough, 0.02..=1.0).text("Roughness"));
                            ui.add(egui::Slider::new(&mut mat.spec, 0.0..=1.0).text("Specular"));
                        }
                        MaterialType::Metal => {
                            ui.add(egui::Slider::new(&mut mat.rough, 0.02..=1.0).text("Roughness"));
                            ui.add(egui::Slider::new(&mut mat.metal, 0.0..=1.0).text("Metallic"));
                            ui.add(egui::Slider::new(&mut mat.spec, 0.0..=1.0).text("Specular"));
                        }
                        MaterialType::Glass => {
                            ui.add(egui::Slider::new(&mut mat.rough, 0.0..=0.6).text("Roughness"));
                            ui.add(egui::Slider::new(&mut mat.ior, 1.05..=2.4).text("IOR (Refraction)"));
                        }
                        MaterialType::Emit => {
                            ui.add(egui::Slider::new(&mut mat.emit, 0.1..=5.0).text("Emit Power"));
                        }
                        MaterialType::Cloud => {
                            ui.add(egui::Slider::new(&mut mat.density, 0.05..=1.0).text("Density"));
                        }
                    }

                    if self.mode == ViewportMode::ModelEditor {
                        ui.separator();
                        ui.heading("Brush");
                        ui.label("Action (T / R / G / N):");
                        ui.horizontal_wrapped(|ui| {
                            for action in [
                                BrushAction::Attach,
                                BrushAction::Erase,
                                BrushAction::Paint,
                                BrushAction::Select,
                            ] {
                                ui.selectable_value(
                                    &mut self.project.brush.action,
                                    action,
                                    action.label(),
                                );
                            }
                        });

                        ui.label("Mode (V / F / B / L / C / P):");
                        ui.horizontal_wrapped(|ui| {
                            for mode in [
                                BrushMode::Voxel,
                                BrushMode::Face,
                                BrushMode::Box,
                                BrushMode::Line,
                                BrushMode::Center,
                                BrushMode::Pattern,
                            ] {
                                ui.selectable_value(
                                    &mut self.project.brush.mode,
                                    mode,
                                    mode.label(),
                                );
                            }
                        });

                        if self.project.brush.mode == BrushMode::Voxel {
                            ui.horizontal(|ui| {
                                ui.label("Shape:");
                                ui.selectable_value(
                                    &mut self.project.brush.sub_shape,
                                    VoxelSubShape::Cube,
                                    "Cube",
                                );
                                ui.selectable_value(
                                    &mut self.project.brush.sub_shape,
                                    VoxelSubShape::Sphere,
                                    "Sphere",
                                );
                            });
                            ui.add(
                                egui::Slider::new(&mut self.project.brush.size, 1..=9)
                                    .text("Size (+/-)"),
                            );
                        } else if self.project.brush.mode == BrushMode::Center {
                            ui.checkbox(
                                &mut self.project.brush.center_circle,
                                "Circle (uncheck for Square)",
                            );
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Mirror (1/2/3):");
                            ui.checkbox(&mut self.project.brush.mirror.x, "X");
                            ui.checkbox(&mut self.project.brush.mirror.y, "Y");
                            ui.checkbox(&mut self.project.brush.mirror.z, "Z");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Axis (Ctrl+1/2/3):");
                            ui.checkbox(&mut self.project.brush.axis.x, "X");
                            ui.checkbox(&mut self.project.brush.axis.y, "Y");
                            ui.checkbox(&mut self.project.brush.axis.z, "Z");
                        });
                    }
                });
            });

        // 4. RIGHT PANEL: CONTEXTUAL (MODEL EDITOR TOOLS vs PATH TRACER RENDERER SETTINGS)
        egui::SidePanel::right("right_edit_panel")
            .resizable(true)
            .default_width(255.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.mode == ViewportMode::PathTracer {
                        ui.heading("Path Tracer (GPU)");
                        ui.label(
                            RichText::new("Tip: Click any voxel with LButton to pick DOF focus, or Alt+LButton to pick its material.")
                                .small()
                                .color(Color32::from_rgb(160, 200, 255)),
                        );
                        ui.separator();

                        ui.heading("Sample & Quality");
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.max_bounces, 1..=12)
                                .text("Max Bounces"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.target_samples, 16..=2048)
                                .text("Target Samples"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.samples_per_frame, 1..=8)
                                .text("Samples / Frame"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.display.render_scale, 1..=3)
                                .text("Pixel Scale (1=Native)"),
                        );

                        ui.separator();
                        ui.heading("Sun, Sky & Fog");
                        ui.add(
                            egui::Slider::new(&mut self.display.sun_azimuth_deg, 0.0..=360.0)
                                .text("Sun Azimuth"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.display.sun_elevation_deg, 5.0..=89.0)
                                .text("Sun Elevation"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.sun_intensity, 0.0..=4.0)
                                .text("Sun Intensity"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.fog_density, 0.0..=0.015)
                                .text("Fog Density"),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Sky Color:");
                            ui.color_edit_button_rgb(&mut self.pt_settings.sky_color);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Ground Color:");
                            ui.color_edit_button_rgb(&mut self.pt_settings.ground_color);
                        });
                        ui.checkbox(&mut self.display.show_ground, "Show Ground Plane");
                        ui.checkbox(&mut self.display.show_background, "Show Sky Background");

                        ui.separator();
                        ui.heading("Lens & Depth of Field");
                        ui.add(
                            egui::Slider::new(&mut self.camera.fov_deg, 15.0..=110.0)
                                .text("FOV (deg)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.aperture, 0.0..=1.5)
                                .text("Aperture (DOF)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.focus_dist, 5.0..=250.0)
                                .text("Focus Dist"),
                        );

                        ui.separator();
                        ui.heading("Post-Processing");
                        ui.checkbox(
                            &mut self.pt_settings.use_aces,
                            "ACES Filmic Tone Mapping",
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.exposure, 0.2..=3.5)
                                .text("Exposure (Post->E)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.pt_settings.vignette, 0.0..=1.2)
                                .text("Vignette (Post->V)"),
                        );
                    } else {
                        ui.heading("Edit & Volume");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Zero").on_hover_text("Clear all voxels").clicked() {
                                self.execute_command_str("zero");
                            }
                            if ui.button("Full (U)").on_hover_text("Fill entire volume").clicked() {
                                self.execute_command_str("full");
                            }
                            if ui.button("Fill (I)").on_hover_text("Paint all solid voxels").clicked() {
                                self.execute_command_str("fill");
                            }
                            if ui.button("Invert").clicked() {
                                self.execute_command_str("inv");
                            }
                            if ui.button("Shrink").on_hover_text("Fit canvas to bounds").clicked() {
                                self.execute_command_str("shrink");
                            }
                            if ui.button("x2 Size").clicked() {
                                self.execute_command_str("x2");
                            }
                            if ui.button("Hollow (flood 0)").clicked() {
                                self.execute_command_str("flood 0");
                            }
                        });

                        ui.separator();
                        ui.heading("Transform");
                        ui.horizontal(|ui| {
                            ui.label("Flip:");
                            if ui.button("X").clicked() {
                                self.execute_command_str("flip x");
                            }
                            if ui.button("Y").clicked() {
                                self.execute_command_str("flip y");
                            }
                            if ui.button("Z").clicked() {
                                self.execute_command_str("flip z");
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Rot90:");
                            if ui.button("X").clicked() {
                                self.execute_command_str("rot90 x");
                            }
                            if ui.button("Y").clicked() {
                                self.execute_command_str("rot90 y");
                            }
                            if ui.button("Z").clicked() {
                                self.execute_command_str("rot90 z");
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Mirror:");
                            if ui.button("X").clicked() {
                                self.execute_command_str("mir x");
                            }
                            if ui.button("Y").clicked() {
                                self.execute_command_str("mir y");
                            }
                            if ui.button("Z").clicked() {
                                self.execute_command_str("mir z");
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Loop:");
                            if ui.button("+X").clicked() {
                                self.execute_command_str("loop x 1");
                            }
                            if ui.button("-X").clicked() {
                                self.execute_command_str("loop x -1");
                            }
                            if ui.button("+Y").clicked() {
                                self.execute_command_str("loop y 1");
                            }
                            if ui.button("+Z").clicked() {
                                self.execute_command_str("loop z 1");
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Dilate (dil)").clicked() {
                                self.execute_command_str("dil xyz");
                            }
                            if ui.button("Erode (ero)").clicked() {
                                self.execute_command_str("ero xyz");
                            }
                        });

                        ui.separator();
                        ui.heading("Generators & Shaders");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("xs wave").clicked() {
                                self.execute_command_str("xs wave");
                            }
                            if ui.button("xs sphere").clicked() {
                                self.execute_command_str("xs sphere");
                            }
                            if ui.button("xs torus").clicked() {
                                self.execute_command_str("xs torus");
                            }
                            if ui.button("xs gyroid").clicked() {
                                self.execute_command_str("xs gyroid");
                            }
                            if ui.button("xs pyramid").clicked() {
                                self.execute_command_str("xs pyramid");
                            }
                            if ui.button("xs checker").clicked() {
                                self.execute_command_str("xs checker");
                            }
                            if ui.button("xs life (CA)").clicked() {
                                self.execute_command_str("xs life");
                            }
                            if ui.button("Noise").clicked() {
                                self.execute_command_str("noise 123 0.08 0.35 0.65");
                            }
                            if ui.button("Rand Color").clicked() {
                                self.execute_command_str("rand 1 64");
                            }
                            if ui.button("Maze").clicked() {
                                self.execute_command_str("maze 6");
                            }
                        });

                        ui.separator();
                        ui.heading("Display & Sun");
                        ui.checkbox(&mut self.display.show_edges, "Edges (Ctrl+E)");
                        ui.checkbox(&mut self.display.show_grid, "Grid (Ctrl+G)");
                        ui.checkbox(&mut self.display.show_frame, "Frame (Ctrl+F)");
                        ui.checkbox(&mut self.display.show_ground, "Ground (Ctrl+U)");
                        ui.checkbox(&mut self.display.show_background, "Background (Ctrl+B)");
                        ui.checkbox(&mut self.display.show_shadows, "Shadows (Ctrl+W)");
                        ui.checkbox(&mut self.display.show_wireframe, "Wireframe (Ctrl+M)");
                        ui.checkbox(&mut self.display.enable_ao, "Voxel Ambient Occlusion");
                        ui.add(
                            egui::Slider::new(&mut self.display.sun_azimuth_deg, 0.0..=360.0)
                                .text("Sun Azimuth"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.display.sun_elevation_deg, 10.0..=89.0)
                                .text("Sun Elevation"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.display.render_scale, 1..=3)
                                .text("Ray-Cast Scale"),
                        );
                    }

                    ui.separator();
                    ui.heading("Camera & View Cube");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Recenter (F4)").clicked() {
                            self.camera.recenter(&self.project.grid);
                        }
                        if ui.button("Snap 90° (F5)").clicked() {
                            self.camera.snap_to_90_view();
                        }
                        if ui.button("Front").clicked() {
                            self.camera.yaw_deg = -90.0;
                            self.camera.pitch_deg = 0.0;
                        }
                        if ui.button("Right").clicked() {
                            self.camera.yaw_deg = 0.0;
                            self.camera.pitch_deg = 0.0;
                        }
                        if ui.button("Top").clicked() {
                            self.camera.yaw_deg = -90.0;
                            self.camera.pitch_deg = 89.0;
                        }
                        if ui.button("Iso").clicked() {
                            self.camera.yaw_deg = -135.0;
                            self.camera.pitch_deg = 30.0;
                        }
                    });
                });
            });

        // 5. CENTRAL 3D VIEWPORT (DDA Ray-Caster in ModelEditor, Progressive GPU Path Tracer in PathTracer)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(24, 26, 30)))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let (rect, response) = ui.allocate_exact_size(avail, Sense::click_and_drag());

                let scale = self.display.render_scale.max(1) as usize;
                let render_w = ((rect.width() as usize) / scale).max(64);
                let render_h = ((rect.height() as usize) / scale).max(64);
                let aspect = rect.width() / rect.height().max(1.0);
                self.last_render_dims = [render_w, render_h];

                // Camera orbit / pan / zoom interaction
                let space_down = ui.input(|i| i.key_down(Key::Space));
                let alt_down = ui.input(|i| i.modifiers.alt);
                let shift_down = ui.input(|i| i.modifiers.shift);
                let x_down = ui.input(|i| i.key_down(Key::X));

                if response.dragged_by(PointerButton::Secondary) {
                    let delta = response.drag_delta();
                    self.camera.yaw_deg -= delta.x * 0.45;
                    self.camera.pitch_deg =
                        (self.camera.pitch_deg + delta.y * 0.45).clamp(-89.0, 89.0);
                } else if response.dragged_by(PointerButton::Middle)
                    || (space_down && response.dragged_by(PointerButton::Primary))
                {
                    let delta = response.drag_delta();
                    let (_, _, right, up) = self.camera.basis();
                    let pan_speed = self.camera.distance * 0.0018;
                    self.camera.target -= right * (delta.x * pan_speed);
                    self.camera.target += up * (delta.y * pan_speed);
                }

                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.1 {
                        self.camera.distance =
                            (self.camera.distance * (1.0 - scroll * 0.0025)).clamp(4.0, 450.0);
                    }
                }

                // Compute 3D ray hit under cursor
                self.hovered_hit = None;
                if let Some(pointer_pos) =
                    response.hover_pos().or_else(|| response.interact_pointer_pos())
                {
                    let ndc_x = ((pointer_pos.x - rect.left()) / rect.width()) * 2.0 - 1.0;
                    let ndc_y = 1.0 - ((pointer_pos.y - rect.top()) / rect.height()) * 2.0;
                    let (ray_o, ray_d) = self.camera.ray_for_ndc(Vec2::new(ndc_x, ndc_y), aspect);
                    let allow_ground = self.mode == ViewportMode::ModelEditor
                        && self.project.brush.action == BrushAction::Attach;
                    self.hovered_hit = pick_voxel(&self.project.grid, ray_o, ray_d, allow_ground);
                }

                if self.mode == ViewportMode::PathTracer {
                    // Renderer interactions (`mv_controls.html`: LButton = pick DOF focus, ALT+LButton = pick material)
                    if !space_down && response.clicked_by(PointerButton::Primary) {
                        if let Some(hit) = self.hovered_hit {
                            if x_down {
                                self.camera.target = hit.hit_pos;
                            } else if alt_down && !hit.is_ground {
                                let c = self
                                    .project
                                    .grid
                                    .get_i32(hit.voxel.x, hit.voxel.y, hit.voxel.z);
                                if c != 0 {
                                    self.project.palette.selected = c;
                                }
                            } else {
                                self.pt_settings.focus_dist = hit.t.max(1.0);
                                self.project.console_logs.push(format!(
                                    "DOF Focus Distance set to {:.2} at voxel ({}, {}, {})",
                                    self.pt_settings.focus_dist,
                                    hit.voxel.x,
                                    hit.voxel.y,
                                    hit.voxel.z
                                ));
                            }
                        }
                    }

                    self.pathtracer.render_progressive(
                        &self.project,
                        &self.camera,
                        &self.display,
                        &self.pt_settings,
                        render_w,
                        render_h,
                        &mut self.rgba_buffer,
                    );

                    if !self.pt_settings.paused
                        && self.pathtracer.current_samples < self.pt_settings.target_samples
                    {
                        ctx.request_repaint();
                    }
                } else {
                    // Model Editor sculpting with LButton
                    if !space_down {
                        if response.clicked_by(PointerButton::Primary)
                            || response.drag_started_by(PointerButton::Primary)
                        {
                            if let Some(hit) = self.hovered_hit {
                                if x_down {
                                    self.camera.target = hit.hit_pos;
                                } else if alt_down && !hit.is_ground {
                                    let c = self
                                        .project
                                        .grid
                                        .get_i32(hit.voxel.x, hit.voxel.y, hit.voxel.z);
                                    if c != 0 {
                                        self.project.palette.selected = c;
                                    }
                                } else {
                                    self.project.record_undo();
                                    self.drag_start_hit = Some(hit);
                                    self.is_sculpting = true;
                                    if matches!(
                                        self.project.brush.mode,
                                        BrushMode::Voxel | BrushMode::Face | BrushMode::Pattern
                                    ) {
                                        let mut brush = self.project.brush.clone();
                                        if shift_down && brush.action == BrushAction::Attach {
                                            brush.action = BrushAction::Erase;
                                        } else if shift_down && brush.action == BrushAction::Erase {
                                            brush.action = BrushAction::Attach;
                                        }
                                        brush.apply(
                                            &mut self.project.grid,
                                            &mut self.project.selection,
                                            self.project.palette.selected,
                                            hit.voxel,
                                            hit.voxel,
                                            hit.normal,
                                            shift_down && alt_down,
                                        );
                                    }
                                }
                            }
                        } else if self.is_sculpting && response.dragged_by(PointerButton::Primary) {
                            if matches!(
                                self.project.brush.mode,
                                BrushMode::Voxel | BrushMode::Pattern
                            ) {
                                if let Some(hit) = self.hovered_hit {
                                    let mut brush = self.project.brush.clone();
                                    if shift_down && brush.action == BrushAction::Attach {
                                        brush.action = BrushAction::Erase;
                                    } else if shift_down && brush.action == BrushAction::Erase {
                                        brush.action = BrushAction::Attach;
                                    }
                                    brush.apply(
                                        &mut self.project.grid,
                                        &mut self.project.selection,
                                        self.project.palette.selected,
                                        hit.voxel,
                                        hit.voxel,
                                        hit.normal,
                                        shift_down && alt_down,
                                    );
                                }
                            }
                        }

                        if self.is_sculpting && response.drag_stopped_by(PointerButton::Primary) {
                            if matches!(
                                self.project.brush.mode,
                                BrushMode::Box | BrushMode::Line | BrushMode::Center
                            ) {
                                if let (Some(start), Some(end)) =
                                    (self.drag_start_hit, self.hovered_hit)
                                {
                                    self.project.brush.apply(
                                        &mut self.project.grid,
                                        &mut self.project.selection,
                                        self.project.palette.selected,
                                        start.voxel,
                                        end.voxel,
                                        start.normal,
                                        shift_down && alt_down,
                                    );
                                }
                            }
                            self.is_sculpting = false;
                            self.drag_start_hit = None;
                        }
                    }

                    let mut preview_set = HashSet::new();
                    if let Some(hit) = self.hovered_hit {
                        let start = self.drag_start_hit.unwrap_or(hit);
                        let base = self.project.brush.compute_shape_voxels(
                            &self.project.grid,
                            start.voxel,
                            hit.voxel,
                            start.normal,
                        );
                        for p in self
                            .project
                            .brush
                            .expand_symmetries(&self.project.grid, &base)
                        {
                            preview_set.insert(p);
                        }
                    }

                    render_viewport_rgba(
                        &self.project,
                        &self.camera,
                        &self.display,
                        &preview_set,
                        render_w,
                        render_h,
                        &mut self.rgba_buffer,
                    );
                }

                let image = ColorImage::from_rgba_unmultiplied(
                    [render_w, render_h],
                    &self.rgba_buffer,
                );
                match &mut self.viewport_texture {
                    Some(tex) => tex.set(image, TextureOptions::NEAREST),
                    None => {
                        self.viewport_texture = Some(ui.ctx().load_texture(
                            "dda_viewport",
                            image,
                            TextureOptions::NEAREST,
                        ));
                    }
                }

                if let Some(tex) = &self.viewport_texture {
                    ui.painter().image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            });
    }
}
