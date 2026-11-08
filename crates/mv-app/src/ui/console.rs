use eframe::egui::{self, Color32, Key};

use crate::app::MagicaVoxelApp;

impl MagicaVoxelApp {
    /// 2. BOTTOM CONSOLE PANEL (`mv_commands.html`)
    pub(crate) fn show_console_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("console_panel")
            .resizable(true)
            .default_height(125.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Console (F1)");
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
    }
}
