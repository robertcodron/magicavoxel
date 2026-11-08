//! Animation timeline (bottom bar) and playback.
//!
//! - Model Editor: frames of the shape being edited (`project.active_shape`), with
//!   `+` (copy of the current frame), `+ empty` and delete. Frame changes swap the
//!   edited model; each model keeps its own undo history.
//! - World Editor: global timeline of the scene (every shape loops over its own length).

use eframe::egui::{self, Color32, RichText};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// `(frame count, current frame index, frames editable)` for the current mode.
    fn timeline_state(&self) -> (usize, usize, bool) {
        match self.mode {
            ViewportMode::WorldEditor => {
                let len = self.project.scene.animation_length() as usize;
                (len, self.project.scene.current_frame as usize % len, false)
            }
            _ => (
                self.project.active_frame_count(),
                self.project.active_frame_index(),
                self.project.active_shape.is_some(),
            ),
        }
    }

    pub(crate) fn goto_frame(&mut self, frame: usize) {
        self.project.set_frame(frame as u32);
        self.sync_size_input();
        self.pathtracer.reset_accumulation();
    }

    /// Advance playback; called once per frame before drawing.
    pub(crate) fn tick_animation(&mut self, ctx: &egui::Context) {
        if !self.anim_playing {
            return;
        }
        let (count, current, _) = self.timeline_state();
        if count <= 1 {
            self.anim_playing = false;
            return;
        }
        let period = 1.0 / self.anim_fps.clamp(1.0, 60.0) as f64;
        let now = ctx.input(|i| i.time);
        if now - self.anim_last_step >= period {
            self.anim_last_step = now;
            self.goto_frame((current + 1) % count);
        }
        let remaining = (self.anim_last_step + period - now).max(0.0);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
    }

    pub(crate) fn show_timeline(&mut self, ctx: &egui::Context) {
        if !matches!(
            self.mode,
            ViewportMode::ModelEditor | ViewportMode::WorldEditor
        ) {
            return;
        }
        egui::TopBottomPanel::bottom("timeline_panel").show(ctx, |ui| {
            let (count, current, editable) = self.timeline_state();
            ui.horizontal(|ui| {
                ui.label(RichText::new("🎞 Timeline").strong());
                if ui
                    .small_button("⏮")
                    .on_hover_text("Previous frame")
                    .clicked()
                {
                    self.goto_frame((current + count - 1) % count);
                }
                let play = if self.anim_playing { "⏸" } else { "▶" };
                if ui
                    .add_enabled(count > 1, egui::Button::new(play).small())
                    .on_hover_text("Play / pause (loops)")
                    .clicked()
                {
                    self.anim_playing = !self.anim_playing;
                    self.anim_last_step = ui.input(|i| i.time);
                }
                if ui.small_button("⏭").on_hover_text("Next frame").clicked() {
                    self.goto_frame((current + 1) % count);
                }
                ui.add(
                    egui::DragValue::new(&mut self.anim_fps)
                        .range(1.0..=60.0)
                        .speed(0.2)
                        .suffix(" fps"),
                );
                ui.separator();

                let mut goto = None;
                egui::ScrollArea::horizontal()
                    .id_salt("timeline_frames")
                    .max_width(ui.available_width() - 230.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for f in 0..count {
                                if ui.selectable_label(f == current, format!("{f}")).clicked() {
                                    goto = Some(f);
                                }
                            }
                        });
                    });
                if let Some(f) = goto {
                    self.goto_frame(f);
                }

                ui.separator();
                ui.label(format!("{} / {}", current, count));
                if editable {
                    if ui
                        .small_button("➕")
                        .on_hover_text("Insert a copy of this frame after it")
                        .clicked()
                    {
                        self.project.add_frame(false);
                        self.sync_size_input();
                    }
                    if ui
                        .small_button("➕ empty")
                        .on_hover_text("Insert an empty frame after this one")
                        .clicked()
                    {
                        self.project.add_frame(true);
                        self.sync_size_input();
                    }
                    if ui
                        .add_enabled(count > 1, egui::Button::new("🗑").small())
                        .on_hover_text("Delete this frame")
                        .clicked()
                    {
                        self.project.delete_frame();
                        self.sync_size_input();
                    }
                } else if self.mode == ViewportMode::ModelEditor {
                    ui.label(
                        RichText::new("open an object from the World Editor to animate it")
                            .small()
                            .color(Color32::GRAY),
                    );
                }
            });
        });
    }
}
