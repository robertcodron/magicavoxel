use eframe::egui::{self, Key};
use mv_core::{BrushAction, BrushMode, Material};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    pub(crate) fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending_action.is_some() {
            return; // the "unsaved changes" dialog is modal
        }
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

        // Custom hotkey bindings from config/hotkey.txt
        let mut triggered_hotkeys = Vec::new();
        ctx.input(|i| {
            if i.key_pressed(Key::Escape) && self.radial_menu.is_open {
                self.radial_menu.close();
            }
            if i.key_pressed(Key::Space) {
                if self.radial_menu.is_open {
                    self.radial_menu.close();
                } else if self.mode == ViewportMode::ModelEditor {
                    let center = self.viewport_rect.center();
                    self.radial_menu.open(center, i.time);
                }
            }
            for (binding, action) in &self.hotkeys.bindings {
                if binding.matches(i) {
                    triggered_hotkeys.push(action.clone());
                }
            }
        });

        for act in triggered_hotkeys {
            self.dispatch_hotkey_action(&act);
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
                if world || self.mode == ViewportMode::PathTracer {
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

    pub(crate) fn dispatch_hotkey_action(&mut self, action: &str) {
        let act = action.trim();
        match act.to_ascii_lowercase().as_str() {
            "radial_menu" | "radial" | "pie_menu" => {
                let center = self.viewport_rect.center();
                self.radial_menu.open(center, 0.0);
            }
            "undo" => {
                if self.mode == ViewportMode::WorldEditor {
                    self.world_undo_redo(false);
                } else if self.project.undo() {
                    self.sync_size_input();
                }
            }
            "redo" => {
                if self.mode == ViewportMode::WorldEditor {
                    self.world_undo_redo(true);
                } else if self.project.redo() {
                    self.sync_size_input();
                }
            }
            "switch_mode" | "switch_editor" => {
                self.mode = match self.mode {
                    ViewportMode::ModelEditor => ViewportMode::WorldEditor,
                    ViewportMode::WorldEditor => ViewportMode::ModelEditor,
                    ViewportMode::PathTracer => ViewportMode::ModelEditor,
                };
            }
            cmd => {
                self.execute_command_str(cmd);
            }
        }
    }
}

/// A custom keyboard shortcut binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyBinding {
    pub fn matches(&self, input: &egui::InputState) -> bool {
        let cmd = input.modifiers.command || input.modifiers.ctrl;
        let shift = input.modifiers.shift;
        let alt = input.modifiers.alt;

        cmd == self.ctrl && shift == self.shift && alt == self.alt && input.key_pressed(self.key)
    }
}

/// Customizable keyboard shortcuts configuration loaded from `config/hotkey.txt`.
#[derive(Debug, Clone, Default)]
pub struct HotkeyConfig {
    pub bindings: Vec<(KeyBinding, String)>,
}

impl HotkeyConfig {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from `config/hotkey.txt` if it exists.
    pub fn load_default_or_file() -> Self {
        let path = std::path::Path::new("config/hotkey.txt");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                return Self::parse_str(&content);
            }
        }
        Self::default()
    }

    /// Parse configuration content into key bindings.
    pub fn parse_str(content: &str) -> Self {
        let mut bindings = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }

            let (key_part, act_part) = if let Some((k, a)) = line.split_once(':') {
                (k.trim(), a.trim())
            } else if let Some((k, a)) = line.split_once('=') {
                (k.trim(), a.trim())
            } else {
                continue;
            };

            let key_clean = key_part.trim_matches('\'').trim_matches('"').trim();
            let act_clean = act_part.trim_matches('\'').trim_matches('"').trim();

            if let Some(binding) = parse_key_combo(key_clean) {
                bindings.push((binding, act_clean.to_string()));
            }
        }
        Self { bindings }
    }
}

/// Parse a string representing a key combo like `CTRL+Z`, `SHIFT+F2`, `ALT+R`, `SPACE`, etc.
pub fn parse_key_combo(spec: &str) -> Option<KeyBinding> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut parsed_key = None;

    let parts: Vec<&str> = spec
        .split(|c| c == '+' || c == '-')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    for part in parts {
        let upper = part.to_ascii_uppercase();
        match upper.as_str() {
            "CTRL" | "CMD" | "COMMAND" | "CONTROL" => ctrl = true,
            "SHIFT" => shift = true,
            "ALT" | "OPT" | "OPTION" => alt = true,
            "A" => parsed_key = Some(Key::A),
            "B" => parsed_key = Some(Key::B),
            "C" => parsed_key = Some(Key::C),
            "D" => parsed_key = Some(Key::D),
            "E" => parsed_key = Some(Key::E),
            "F" => parsed_key = Some(Key::F),
            "G" => parsed_key = Some(Key::G),
            "H" => parsed_key = Some(Key::H),
            "I" => parsed_key = Some(Key::I),
            "J" => parsed_key = Some(Key::J),
            "K" => parsed_key = Some(Key::K),
            "L" => parsed_key = Some(Key::L),
            "M" => parsed_key = Some(Key::M),
            "N" => parsed_key = Some(Key::N),
            "O" => parsed_key = Some(Key::O),
            "P" => parsed_key = Some(Key::P),
            "Q" => parsed_key = Some(Key::Q),
            "R" => parsed_key = Some(Key::R),
            "S" => parsed_key = Some(Key::S),
            "T" => parsed_key = Some(Key::T),
            "U" => parsed_key = Some(Key::U),
            "V" => parsed_key = Some(Key::V),
            "W" => parsed_key = Some(Key::W),
            "X" => parsed_key = Some(Key::X),
            "Y" => parsed_key = Some(Key::Y),
            "Z" => parsed_key = Some(Key::Z),
            "0" | "NUM0" => parsed_key = Some(Key::Num0),
            "1" | "NUM1" => parsed_key = Some(Key::Num1),
            "2" | "NUM2" => parsed_key = Some(Key::Num2),
            "3" | "NUM3" => parsed_key = Some(Key::Num3),
            "4" | "NUM4" => parsed_key = Some(Key::Num4),
            "5" | "NUM5" => parsed_key = Some(Key::Num5),
            "6" | "NUM6" => parsed_key = Some(Key::Num6),
            "7" | "NUM7" => parsed_key = Some(Key::Num7),
            "8" | "NUM8" => parsed_key = Some(Key::Num8),
            "9" | "NUM9" => parsed_key = Some(Key::Num9),
            "F1" => parsed_key = Some(Key::F1),
            "F2" => parsed_key = Some(Key::F2),
            "F3" => parsed_key = Some(Key::F3),
            "F4" => parsed_key = Some(Key::F4),
            "F5" => parsed_key = Some(Key::F5),
            "F6" => parsed_key = Some(Key::F6),
            "F7" => parsed_key = Some(Key::F7),
            "F8" => parsed_key = Some(Key::F8),
            "F9" => parsed_key = Some(Key::F9),
            "F10" => parsed_key = Some(Key::F10),
            "F11" => parsed_key = Some(Key::F11),
            "F12" => parsed_key = Some(Key::F12),
            "SPACE" => parsed_key = Some(Key::Space),
            "TAB" => parsed_key = Some(Key::Tab),
            "ENTER" | "RETURN" => parsed_key = Some(Key::Enter),
            "ESCAPE" | "ESC" => parsed_key = Some(Key::Escape),
            "BACKSPACE" => parsed_key = Some(Key::Backspace),
            "DELETE" | "DEL" => parsed_key = Some(Key::Delete),
            "PLUS" | "=" => parsed_key = Some(Key::Plus),
            "MINUS" => parsed_key = Some(Key::Minus),
            "LEFT" => parsed_key = Some(Key::ArrowLeft),
            "RIGHT" => parsed_key = Some(Key::ArrowRight),
            "UP" => parsed_key = Some(Key::ArrowUp),
            "DOWN" => parsed_key = Some(Key::ArrowDown),
            _ => {}
        }
    }

    parsed_key.map(|key| KeyBinding {
        key,
        ctrl,
        shift,
        alt,
    })
}
