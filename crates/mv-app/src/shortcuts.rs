use eframe::egui::{self, Key};
use mv_core::{BrushAction, BrushMode, Material};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    pub(crate) fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let text_focused = ctx.memory(|m| m.focused().is_some());

        ctx.input(|i| {
            // `F1` activates the console (`TAB` switches editors, see `raw_input_hook`).
            if i.key_pressed(Key::F1) {
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

            let world = self.mode == ViewportMode::WorldEditor;

            // Global Undo / Redo / File shortcuts (World Editor has its own scene history)
            if cmd && !shift && i.key_pressed(Key::Z) {
                if world {
                    self.world_undo_redo(false);
                } else if self.project.undo() {
                    self.sync_size_input();
                }
            } else if (cmd && shift && i.key_pressed(Key::Z)) || (cmd && i.key_pressed(Key::Y)) {
                if world {
                    self.world_undo_redo(true);
                } else if self.project.redo() {
                    self.sync_size_input();
                }
            } else if cmd && i.key_pressed(Key::S) {
                self.handle_save_project(shift);
            } else if cmd && i.key_pressed(Key::O) {
                self.handle_open_project();
            } else if world {
                // World Editor shortcuts are handled after this closure.
            } else if cmd && i.key_pressed(Key::P) {
                self.project.record_undo();
                self.project.grid.clear();
                self.project.selection.clear();
            } else if self.mode == ViewportMode::PathTracer {
                // Renderer material shortcuts (`mv_controls.html`: Ctrl+R reset, Ctrl+C copy, Ctrl+V paste)
                let sel = self.project.palette.selected as usize;
                if cmd && i.key_pressed(Key::R) {
                    self.project.record_undo();
                    self.project.materials[sel] = Material {
                        id: sel as u8,
                        ..Default::default()
                    };
                } else if cmd && i.key_pressed(Key::C) {
                    self.copied_material = Some(self.project.materials[sel]);
                } else if cmd && i.key_pressed(Key::V) {
                    if let Some(mut m) = self.copied_material {
                        self.project.record_undo();
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
                // Applied after the closure: `ctx` is locked while reading input.
                self.pending_ui_scale = Some(self.ui_scale);
            }
            if cmd && i.key_pressed(Key::Minus) {
                self.ui_scale = (self.ui_scale - 0.1).clamp(0.75, 2.0);
                self.pending_ui_scale = Some(self.ui_scale);
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
                if world {
                    self.frame_world_camera();
                } else {
                    self.camera.recenter(&self.project.grid);
                }
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

        if self.mode == ViewportMode::WorldEditor {
            self.handle_world_shortcuts(ctx);
        }
    }
}
