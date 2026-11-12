use eframe::egui::{self, Color32, RichText};
use mv_render::{AtmospherePreset, VoxelShape};

use crate::app::{MagicaVoxelApp, ViewportMode};

impl MagicaVoxelApp {
    /// 4. RIGHT PANEL: CONTEXTUAL (MODEL EDITOR TOOLS vs PATH TRACER RENDERER SETTINGS)
    pub(crate) fn show_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("right_edit_panel")
            .resizable(true)
            .default_width(255.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.mode == ViewportMode::WorldEditor {
                        self.show_world_panel(ui);
                        return;
                    }
                    if self.mode == ViewportMode::PathTracer {
                        ui.heading("Path Tracer (GPU)");
                        ui.label(
                            RichText::new("Tip: Click any voxel with LButton to pick DOF focus, or Alt+LButton to pick its material.")
                                .small()
                                .color(Color32::from_rgb(160, 200, 255)),
                        );
                        ui.separator();

                        ui.heading("Atmosphere Presets");
                        ui.horizontal_wrapped(|ui| {
                            for &preset in &AtmospherePreset::ALL {
                                let is_active = self.active_atmosphere == Some(preset);
                                let btn_text = match preset {
                                    AtmospherePreset::Default => "☀️ Default",
                                    AtmospherePreset::GoldenHour => "🌅 Golden Hour",
                                    AtmospherePreset::MidnightFairytale => "🌌 Midnight Fairytale",
                                    AtmospherePreset::MistyMorning => "🌫️ Misty Morning",
                                };
                                let btn = if is_active {
                                    ui.button(
                                        RichText::new(btn_text)
                                            .strong()
                                            .color(Color32::from_rgb(255, 220, 100)),
                                    )
                                } else {
                                    ui.button(btn_text)
                                };
                                if btn.clicked() {
                                    self.active_atmosphere = Some(preset);
                                    preset.apply_to_settings(&mut self.pt_settings);
                                    self.display.apply_preset(preset);
                                    self.pathtracer.current_samples = 0;
                                }
                            }
                        });

                        ui.separator();
                        ui.heading("Voxel Geometry & Shape");
                        ui.horizontal_wrapped(|ui| {
                            for &shape in &VoxelShape::ALL {
                                let is_sel = self.display.voxel_shape == shape;
                                let label = match shape {
                                    VoxelShape::Cube => "🧊 Cube",
                                    VoxelShape::Lego => "🧱 Lego",
                                    VoxelShape::Sphere => "⚪ Sphere",
                                    VoxelShape::Cylinder => "🥫 Cylinder",
                                    VoxelShape::Clay => "🏺 Clay",
                                };
                                if ui.selectable_label(is_sel, label).clicked() && !is_sel {
                                    self.display.voxel_shape = shape;
                                    self.pathtracer.current_samples = 0;
                                }
                            }
                        });
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
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(!self.cozy_tab_open, "🎨 Edit & Volume")
                                .clicked()
                            {
                                self.cozy_tab_open = false;
                            }
                            if ui
                                .selectable_label(self.cozy_tab_open, "🏡 Cozy Builder")
                                .clicked()
                            {
                                self.cozy_tab_open = true;
                            }
                        });
                        ui.separator();

                        if self.cozy_tab_open {
                            self.show_cozy_builder_panel(ui);
                        } else {
                            ui.heading("Direct Extrusion");
                            ui.checkbox(
                                &mut self.extrude_state.enabled,
                                "Extrusion Handles (6 faces)",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Snap:");
                                for &s in &[1, 2, 4, 8] {
                                    if ui
                                        .selectable_label(
                                            self.extrude_state.snap == s,
                                            format!("{} vx", s),
                                        )
                                        .clicked()
                                    {
                                        self.extrude_state.snap = s;
                                    }
                                }
                            });
                            ui.separator();

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

    pub(crate) fn show_cozy_builder_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("🏡 Cozy Architectural Builder");
        ui.label(
            RichText::new("Procedural organic building tools for roofs, windows, nature & lights.")
                .small()
                .color(Color32::from_rgb(160, 220, 180)),
        );
        ui.separator();

        // 1. Roofs
        ui.collapsing("🏡 Roof Generator", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Gabled Roof").on_hover_text("Classic two-sided gable with eaves").clicked() {
                    self.execute_cozy_roof(mv_core::architecture::RoofType::Gabled);
                }
                if ui.button("Hipped Roof").on_hover_text("Four-sided sloped roof").clicked() {
                    self.execute_cozy_roof(mv_core::architecture::RoofType::Hipped);
                }
                if ui.button("Mansard Roof").on_hover_text("French steep Mansard roof").clicked() {
                    self.execute_cozy_roof(mv_core::architecture::RoofType::Mansard);
                }
                if ui.button("Conical Turret").on_hover_text("Round conical fairy turret").clicked() {
                    self.execute_cozy_roof(mv_core::architecture::RoofType::Conical);
                }
            });
        });

        // 2. Windows, Doors & Chimneys
        ui.collapsing("🪟 Windows, Doors & Chimneys", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Bay Window").clicked() {
                    self.execute_cozy_window();
                }
                if ui.button("Wooden Door").clicked() {
                    self.execute_cozy_door();
                }
                if ui.button("Stone Chimney").clicked() {
                    self.execute_cozy_chimney();
                }
            });
        });

        // 3. Nature & Biome
        ui.collapsing("🌳 Nature & Biome", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Fluffy Tree").clicked() {
                    self.execute_cozy_tree();
                }
                if ui.button("Organic Pond").clicked() {
                    self.execute_cozy_pond();
                }
                if ui.button("Grow Ivy").clicked() {
                    self.execute_cozy_ivy();
                }
                if ui.button("Cobblestone Path").clicked() {
                    self.execute_cozy_path();
                }
            });
        });

        // 4. Lanterns
        ui.collapsing("🏮 Cozy Lanterns & Lights", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Post Lantern").clicked() {
                    self.execute_cozy_lantern(true);
                }
                if ui.button("Wall Sconce").clicked() {
                    self.execute_cozy_lantern(false);
                }
            });
        });

        // 5. Walls & Arches
        ui.collapsing("🏰 Spline Wall, Fence & Arch", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Stone Arch").clicked() {
                    self.execute_cozy_arch();
                }
                if ui.button("Curved Wall").clicked() {
                    self.execute_cozy_spline_wall();
                }
                if ui.button("Wooden Fence").clicked() {
                    self.execute_cozy_fence();
                }
            });
        });
    }

    pub(crate) fn execute_cozy_roof(&mut self, roof_type: mv_core::architecture::RoofType) {
        let (min, max) = if self.project.selection.any_selected() {
            let mut lo = glam::IVec3::splat(i32::MAX);
            let mut hi = glam::IVec3::splat(i32::MIN);
            for z in 0..self.project.grid.size_z() {
                for y in 0..self.project.grid.size_y() {
                    for x in 0..self.project.grid.size_x() {
                        if self.project.selection.get(x, y, z) {
                            lo = lo.min(glam::IVec3::new(x as i32, y as i32, z as i32));
                            hi = hi.max(glam::IVec3::new(x as i32 + 1, y as i32 + 1, z as i32 + 1));
                        }
                    }
                }
            }
            (lo, hi)
        } else {
            let top_z = (0..self.project.grid.size_z())
                .rev()
                .find(|&z| {
                    (0..self.project.grid.size_x()).any(|x| {
                        (0..self.project.grid.size_y()).any(|y| self.project.grid.get(x, y, z) != 0)
                    })
                })
                .unwrap_or(self.project.grid.size_z() / 2);
            (
                glam::IVec3::new(0, 0, top_z as i32),
                glam::IVec3::new(
                    self.project.grid.size_x() as i32,
                    self.project.grid.size_y() as i32,
                    self.project.grid.size_z() as i32,
                ),
            )
        };
        self.project.record_undo();
        let color = self.project.palette.selected.max(1);
        mv_core::architecture::generate_roof(&mut self.project.grid, min, max, roof_type, color, 1);
        self.project
            .console_logs
            .push(format!("Generated cozy {:?} roof", roof_type));
    }

    pub(crate) fn execute_cozy_window(&mut self) {
        let cx = self.project.grid.size_x() as i32 / 2;
        let min = glam::IVec3::new(cx - 3, 0, (self.project.grid.size_z() as i32 / 4).max(2));
        let max = glam::IVec3::new(cx + 3, 2, min.z + 6);
        self.project.record_undo();
        let frame = self.project.palette.selected.max(1);
        let glass = 2;
        mv_core::architecture::punch_window(&mut self.project.grid, min, max, frame, glass, true);
        self.project.console_logs.push("Punched cozy bay window with shutters".to_string());
    }

    pub(crate) fn execute_cozy_door(&mut self) {
        let origin = glam::IVec3::new((self.project.grid.size_x() as i32 / 2 - 2).max(0), 0, 0);
        self.project.record_undo();
        let frame = self.project.palette.selected.max(1);
        let door = 4;
        mv_core::architecture::punch_door(&mut self.project.grid, origin, 4, 8, frame, door);
        self.project.console_logs.push("Punched cozy wooden doorway".to_string());
    }

    pub(crate) fn execute_cozy_chimney(&mut self) {
        let origin = glam::IVec3::new(
            (self.project.grid.size_x() as i32 - 6).max(0),
            self.project.grid.size_y() as i32 / 2,
            self.project.grid.size_z() as i32 / 2,
        );
        self.project.record_undo();
        let brick = self.project.palette.selected.max(1);
        mv_core::architecture::build_chimney(&mut self.project.grid, origin, 8, 3, brick);
        self.project.console_logs.push("Built cozy stone chimney".to_string());
    }

    pub(crate) fn execute_cozy_tree(&mut self) {
        let base = glam::IVec3::new(
            self.project.grid.size_x() as i32 / 2,
            self.project.grid.size_y() as i32 / 2,
            0,
        );
        let h = (self.project.grid.size_z() / 2).max(10);
        self.project.record_undo();
        let trunk = self.project.palette.selected.max(1);
        let foliage = trunk.wrapping_add(10).max(1);
        mv_core::biome::generate_fluffy_tree(&mut self.project.grid, base, h, 5, trunk, foliage);
        self.project.console_logs.push("Spawned fluffy organic tree".to_string());
    }

    pub(crate) fn execute_cozy_pond(&mut self) {
        let center = glam::IVec3::new(
            self.project.grid.size_x() as i32 / 2,
            self.project.grid.size_y() as i32 / 2,
            0,
        );
        self.project.record_undo();
        let water = self.project.palette.selected.max(1);
        mv_core::biome::carve_pond(&mut self.project.grid, center, 7, 3, water);
        self.project.console_logs.push("Carved organic cozy pond".to_string());
    }

    pub(crate) fn execute_cozy_path(&mut self) {
        let sx = self.project.grid.size_x() as f32;
        let sy = self.project.grid.size_y() as f32;
        let pts = vec![
            glam::Vec3::new(2.0, sy * 0.5, 0.0),
            glam::Vec3::new(sx * 0.35, sy * 0.65, 0.0),
            glam::Vec3::new(sx * 0.65, sy * 0.35, 0.0),
            glam::Vec3::new(sx - 2.0, sy * 0.5, 0.0),
        ];
        self.project.record_undo();
        let stone = self.project.palette.selected.max(1);
        let dirt = stone.wrapping_add(5).max(1);
        mv_core::biome::paint_path(&mut self.project.grid, &pts, 3, stone, dirt);
        self.project.console_logs.push("Painted organic cobblestone pathway".to_string());
    }

    pub(crate) fn execute_cozy_ivy(&mut self) {
        let mut surfaces = Vec::new();
        for z in 0..self.project.grid.size_z() {
            for y in 0..self.project.grid.size_y() {
                for x in 0..self.project.grid.size_x() {
                    if self.project.grid.get(x, y, z) != 0 {
                        surfaces.push(glam::IVec3::new(x as i32, y as i32, z as i32));
                    }
                }
            }
        }
        if surfaces.is_empty() {
            return;
        }
        self.project.record_undo();
        let leaf = self.project.palette.selected.max(1);
        mv_core::biome::grow_ivy(&mut self.project.grid, &surfaces, 25, leaf);
        self.project.console_logs.push("Grew climbing ivy vines".to_string());
    }

    pub(crate) fn execute_cozy_lantern(&mut self, freestanding: bool) {
        let pos = glam::IVec3::new(
            self.project.grid.size_x() as i32 / 2,
            self.project.grid.size_y() as i32 / 2,
            if freestanding { 0 } else { 4 },
        );
        let normal = if freestanding {
            glam::IVec3::ZERO
        } else {
            glam::IVec3::new(0, 1, 0)
        };
        self.project.record_undo();
        let frame = self.project.palette.selected.max(1);
        mv_core::light::spawn_lantern(&mut self.project.grid, pos, normal, frame, 3);
        self.project.console_logs.push(format!(
            "Spawned {} cozy lantern",
            if freestanding { "post" } else { "wall" }
        ));
    }

    pub(crate) fn execute_cozy_arch(&mut self) {
        let center = glam::IVec3::new(
            self.project.grid.size_x() as i32 / 2,
            self.project.grid.size_y() as i32 / 2,
            0,
        );
        self.project.record_undo();
        let frame = self.project.palette.selected.max(1);
        mv_core::spline::carve_arch(&mut self.project.grid, center, 8, 10, 4, frame);
        self.project.console_logs.push("Carved classical stone archway".to_string());
    }

    pub(crate) fn execute_cozy_spline_wall(&mut self) {
        let sx = self.project.grid.size_x() as f32;
        let sy = self.project.grid.size_y() as f32;
        let pts = vec![
            glam::Vec3::new(4.0, 4.0, 0.0),
            glam::Vec3::new(sx * 0.4, sy * 0.6, 0.0),
            glam::Vec3::new(sx * 0.7, sy * 0.3, 0.0),
            glam::Vec3::new(sx - 4.0, sy - 4.0, 0.0),
        ];
        self.project.record_undo();
        let color = self.project.palette.selected.max(1);
        mv_core::spline::generate_spline_wall(&mut self.project.grid, &pts, 2, 6, true, color);
        self.project.console_logs.push("Built cozy stone spline wall with crenels".to_string());
    }

    pub(crate) fn execute_cozy_fence(&mut self) {
        let sx = self.project.grid.size_x() as f32;
        let sy = self.project.grid.size_y() as f32;
        let pts = vec![
            glam::Vec3::new(3.0, 3.0, 0.0),
            glam::Vec3::new(sx * 0.5, sy * 0.3, 0.0),
            glam::Vec3::new(sx - 3.0, 3.0, 0.0),
        ];
        self.project.record_undo();
        let wood = self.project.palette.selected.max(1);
        mv_core::spline::generate_fence(&mut self.project.grid, &pts, 3, wood);
        self.project.console_logs.push("Generated rustic wooden fence".to_string());
    }
}
