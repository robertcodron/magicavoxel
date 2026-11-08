use eframe::egui::{self, Color32, RichText};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// 4. RIGHT PANEL: CONTEXTUAL (MODEL EDITOR TOOLS vs PATH TRACER RENDERER SETTINGS)
    pub(crate) fn show_right_panel(&mut self, ctx: &egui::Context) {
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
    }
}
