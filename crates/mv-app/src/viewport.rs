use std::collections::HashSet;

use eframe::egui::{self, Color32, ColorImage, Key, PointerButton, Sense, TextureOptions};
use glam::Vec2;
use mv_core::{BrushAction, BrushMode};
use mv_render::{pick_voxel, render_viewport_rgba};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// 5. CENTRAL 3D VIEWPORT (DDA Ray-Caster in ModelEditor, Progressive GPU Path Tracer in PathTracer)
    pub(crate) fn show_viewport(&mut self, ctx: &egui::Context) {
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
                if let Some(pointer_pos) = response
                    .hover_pos()
                    .or_else(|| response.interact_pointer_pos())
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
                                let c = self.project.grid.get_i32(
                                    hit.voxel.x,
                                    hit.voxel.y,
                                    hit.voxel.z,
                                );
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
                                    let c = self.project.grid.get_i32(
                                        hit.voxel.x,
                                        hit.voxel.y,
                                        hit.voxel.z,
                                    );
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

                let image =
                    ColorImage::from_rgba_unmultiplied([render_w, render_h], &self.rgba_buffer);
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
