//! "Voxel Shader" section of the left panel (Model Editor): choose a built-in or
//! `shader/*.txt` shader, edit the arguments declared in its header, run it.

use eframe::egui::{self, Color32, RichText};
use mv_core::XsMode;

use crate::app::MagicaVoxelApp;
use crate::shaders::ShaderPanel;

impl MagicaVoxelApp {
    pub(crate) fn show_shader_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Voxel Shader");
        let mut pick = None;
        ui.horizontal(|ui| {
            let label = if self.shaders.selected.is_empty() {
                "choose…".to_string()
            } else {
                self.shaders.selected.clone()
            };
            egui::ComboBox::from_id_salt("xs_shader")
                .selected_text(label)
                .width(170.0)
                .show_ui(ui, |ui| {
                    ui.label(RichText::new("shader/ folder").small().color(Color32::GRAY));
                    for (name, _) in &self.shaders.files {
                        if ui
                            .selectable_label(self.shaders.selected == *name, name)
                            .clicked()
                        {
                            pick = Some(name.clone());
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new("built-in").small().color(Color32::GRAY));
                    for name in ShaderPanel::builtin_names() {
                        if ui
                            .selectable_label(self.shaders.selected == *name, *name)
                            .clicked()
                        {
                            pick = Some(name.to_string());
                        }
                    }
                });
            if ui
                .small_button("↻")
                .on_hover_text("Rescan the shader folders")
                .clicked()
            {
                self.shaders.refresh();
            }
        });
        if let Some(name) = pick {
            self.shaders.select(&name);
        }
        if self.shaders.files.is_empty() {
            ui.label(
                RichText::new("No shader/*.txt found (MagicaVoxel shaders are supported).")
                    .small()
                    .color(Color32::GRAY),
            );
        }

        if let Some(shader) = &self.shaders.loaded {
            for (arg, value) in shader.args.iter().zip(self.shaders.arg_values.iter_mut()) {
                let (lo, hi) = if arg.min <= arg.max {
                    (arg.min, arg.max)
                } else {
                    (arg.max, arg.min)
                };
                ui.add(
                    egui::Slider::new(value, lo..=hi)
                        .step_by(arg.step.max(0.0) as f64)
                        .max_decimals(arg.precision as usize)
                        .text(&arg.name),
                );
            }
        }

        ui.horizontal(|ui| {
            ui.label("Iterations:");
            ui.add(egui::DragValue::new(&mut self.shaders.iterations).range(1..=360));
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.shaders.mode, XsMode::InPlace, "In place")
                .on_hover_text("Run the iterations on the current frame");
            ui.selectable_value(
                &mut self.shaders.mode,
                XsMode::Animate {
                    from_previous: true,
                },
                "Anim -prev",
            )
            .on_hover_text("One new frame per iteration, each built on the previous one");
            ui.selectable_value(
                &mut self.shaders.mode,
                XsMode::Animate {
                    from_previous: false,
                },
                "Anim -cur",
            )
            .on_hover_text("One new frame per iteration, each built on the current frame");
        });
        let invocation = self.shaders.invocation();
        if ui
            .add_enabled(invocation.is_some(), egui::Button::new("▶ Run shader"))
            .on_hover_text(
                invocation
                    .as_ref()
                    .map_or(String::new(), |i| format!("Same as: {}", i.to_command())),
            )
            .clicked()
        {
            if let Some(inv) = invocation {
                // Same path as typing the command (logged in the console history).
                self.execute_command_str(&inv.to_command());
            }
        }
    }
}
