use eframe::egui::{self, Color32, Sense, Stroke, Vec2 as EguiVec2};
use mv_core::{BrushAction, BrushMode, Material, MaterialType, VoxelSubShape};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// 3. LEFT PANEL: PALETTE + PBR MATTER + BRUSH SETTINGS
    pub(crate) fn show_left_panel(&mut self, ctx: &egui::Context) {
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
                                    let idx =
                                        row.saturating_mul(8).saturating_add(col).saturating_add(1);
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
                    let mat_before = self.project.materials[sel];
                    let color_before = self.project.palette.colors[sel];
                    let cur_col = self.project.palette.colors[sel];
                    let mut rgb = [cur_col.r, cur_col.g, cur_col.b];
                    ui.horizontal(|ui| {
                        ui.strong(format!("Swatch #{}:", sel));
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            self.project.palette.colors[sel].r = rgb[0];
                            self.project.palette.colors[sel].g = rgb[1];
                            self.project.palette.colors[sel].b = rgb[2];
                        }
                        if ui
                            .small_button("Copy Mat")
                            .on_hover_text("Ctrl+C in Renderer")
                            .clicked()
                        {
                            self.copied_material = Some(self.project.materials[sel]);
                        }
                        if ui
                            .small_button("Paste")
                            .on_hover_text("Ctrl+V in Renderer")
                            .clicked()
                        {
                            if let Some(mut m) = self.copied_material {
                                m.id = sel as u8;
                                self.project.materials[sel] = m;
                            }
                        }
                        if ui
                            .small_button("Reset")
                            .on_hover_text("Ctrl+R in Renderer")
                            .clicked()
                        {
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
                            ui.add(
                                egui::Slider::new(&mut mat.ior, 1.05..=2.4)
                                    .text("IOR (Refraction)"),
                            );
                        }
                        MaterialType::Emit => {
                            ui.add(egui::Slider::new(&mut mat.emit, 0.1..=5.0).text("Emit Power"));
                        }
                        MaterialType::Cloud => {
                            ui.add(egui::Slider::new(&mut mat.density, 0.05..=1.0).text("Density"));
                        }
                    }

                    // Group one swatch/material edit gesture (slider drag, click, picker) into a
                    // single undo step: record the pre-edit state the first time a change is seen.
                    let pointer_down = ui.input(|i| i.pointer.any_down());
                    let changed = self.project.materials[sel] != mat_before
                        || self.project.palette.colors[sel] != color_before;
                    if changed && !self.swatch_edit_in_progress {
                        let mat_after = self.project.materials[sel];
                        let color_after = self.project.palette.colors[sel];
                        self.project.materials[sel] = mat_before;
                        self.project.palette.colors[sel] = color_before;
                        self.project.record_undo();
                        self.project.materials[sel] = mat_after;
                        self.project.palette.colors[sel] = color_after;
                        self.swatch_edit_in_progress = pointer_down;
                    } else if !pointer_down {
                        self.swatch_edit_in_progress = false;
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
    }
}
