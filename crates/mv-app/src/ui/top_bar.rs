use eframe::egui::{self, Color32, RichText};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// 1. TOP BAR
    pub(crate) fn show_top_bar(&mut self, ctx: &egui::Context) {
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

                if ui
                    .button("📂 Open")
                    .on_hover_text("Open .vox (Ctrl+O)")
                    .clicked()
                {
                    self.handle_open_project();
                }
                if ui
                    .button("💾 Save")
                    .on_hover_text("Save .vox (Ctrl+S)")
                    .clicked()
                {
                    self.handle_save_project(false);
                }
                if ui
                    .button("📷 Snap")
                    .on_hover_text("Save Screenshot PNG (F6)")
                    .clicked()
                {
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
    }
}
