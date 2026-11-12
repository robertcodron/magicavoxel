//! Direct Manipulation Extrusion Gizmo for the Model Editor.
//!
//! Provides interactive handles on the 6 faces (+X, -X, +Y, -Y, +Z, -Z)
//! of the active selection or grid bounds.
//! Supports discrete snapping (1, 2, 4, 8 voxels) and real-time interactive extrusion.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use glam::{IVec3, Vec3};
use mv_core::{Axis, EditorProject, SelectionMask, VoxelGrid};

/// The 6 primary cuboid faces for direct extrusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtrudeFace {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl ExtrudeFace {
    pub const ALL: [ExtrudeFace; 6] = [
        ExtrudeFace::PosX,
        ExtrudeFace::NegX,
        ExtrudeFace::PosY,
        ExtrudeFace::NegY,
        ExtrudeFace::PosZ,
        ExtrudeFace::NegZ,
    ];

    pub fn axis(&self) -> Axis {
        match self {
            Self::PosX | Self::NegX => Axis::X,
            Self::PosY | Self::NegY => Axis::Y,
            Self::PosZ | Self::NegZ => Axis::Z,
        }
    }

    pub fn sign(&self) -> i32 {
        match self {
            Self::PosX | Self::PosY | Self::PosZ => 1,
            Self::NegX | Self::NegY | Self::NegZ => -1,
        }
    }

    pub fn normal(&self) -> Vec3 {
        match self {
            Self::PosX => Vec3::X,
            Self::NegX => Vec3::NEG_X,
            Self::PosY => Vec3::Y,
            Self::NegY => Vec3::NEG_Y,
            Self::PosZ => Vec3::Z,
            Self::NegZ => Vec3::NEG_Z,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::PosX => "+X",
            Self::NegX => "-X",
            Self::PosY => "+Y",
            Self::NegY => "-Y",
            Self::PosZ => "+Z",
            Self::NegZ => "-Z",
        }
    }

    pub fn color(&self) -> Color32 {
        match self {
            Self::PosX => Color32::from_rgb(240, 75, 75),
            Self::NegX => Color32::from_rgb(200, 50, 50),
            Self::PosY => Color32::from_rgb(60, 210, 80),
            Self::NegY => Color32::from_rgb(40, 160, 60),
            Self::PosZ => Color32::from_rgb(65, 145, 255),
            Self::NegZ => Color32::from_rgb(45, 105, 210),
        }
    }
}

/// An ongoing extrusion drag gesture.
#[derive(Debug, Clone)]
pub struct ExtrudeDrag {
    pub face: ExtrudeFace,
    pub start_pointer: Pos2,
    pub screen_dir: Vec2,
    pub px_per_voxel: f32,
    pub initial_grid: VoxelGrid,
    pub initial_selection: Option<SelectionMask>,
    pub applied_amount: i32,
}

/// Persistent state for the extrusion gizmo.
#[derive(Debug, Clone)]
pub struct ExtrudeGizmoState {
    pub enabled: bool,
    /// Discrete snapping step in voxels: 1, 2, 4, 8
    pub snap: u32,
    pub hovered_face: Option<ExtrudeFace>,
    pub active_drag: Option<ExtrudeDrag>,
}

impl Default for ExtrudeGizmoState {
    fn default() -> Self {
        Self {
            enabled: true,
            snap: 1,
            hovered_face: None,
            active_drag: None,
        }
    }
}

impl ExtrudeGizmoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active_or_hovered(&self) -> bool {
        self.active_drag.is_some() || self.hovered_face.is_some()
    }

    /// Compute the 3D bounding volume of selection or grid bounds.
    pub fn volume_bounds(project: &EditorProject) -> (Vec3, Vec3) {
        if project.selection.any_selected() {
            let mut lo = IVec3::splat(i32::MAX);
            let mut hi = IVec3::splat(i32::MIN);
            for z in 0..project.grid.size_z() {
                for y in 0..project.grid.size_y() {
                    for x in 0..project.grid.size_x() {
                        if project.selection.get(x, y, z) {
                            lo = lo.min(IVec3::new(x as i32, y as i32, z as i32));
                            hi = hi.max(IVec3::new(x as i32 + 1, y as i32 + 1, z as i32 + 1));
                        }
                    }
                }
            }
            if lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z {
                return (lo.as_vec3(), hi.as_vec3());
            }
        }
        (
            Vec3::ZERO,
            Vec3::new(
                project.grid.size_x() as f32,
                project.grid.size_y() as f32,
                project.grid.size_z() as f32,
            ),
        )
    }

    /// Center of a specific face in 3D world space.
    pub fn face_center(face: ExtrudeFace, min: Vec3, max: Vec3) -> Vec3 {
        match face {
            ExtrudeFace::PosX => Vec3::new(max.x, (min.y + max.y) * 0.5, (min.z + max.z) * 0.5),
            ExtrudeFace::NegX => Vec3::new(min.x, (min.y + max.y) * 0.5, (min.z + max.z) * 0.5),
            ExtrudeFace::PosY => Vec3::new((min.x + max.x) * 0.5, max.y, (min.z + max.z) * 0.5),
            ExtrudeFace::NegY => Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5),
            ExtrudeFace::PosZ => Vec3::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, max.z),
            ExtrudeFace::NegZ => Vec3::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, min.z),
        }
    }

    /// Process mouse input, update hover state or active drag gesture.
    pub fn handle_input<F>(
        &mut self,
        project: &mut EditorProject,
        pointer_pos: Option<Pos2>,
        primary_down: bool,
        primary_clicked: bool,
        primary_released: bool,
        to_screen: &F,
    ) -> Option<String>
    where
        F: Fn(Vec3) -> Option<Pos2>,
    {
        if !self.enabled {
            self.hovered_face = None;
            self.active_drag = None;
            return None;
        }

        let (min, max) = Self::volume_bounds(project);

        // 1. If currently dragging, update continuous extrusion with discrete snap
        if let Some(mut drag) = self.active_drag.take() {
            if primary_down {
                if let Some(pos) = pointer_pos {
                    let delta = pos - drag.start_pointer;
                    let proj = delta.dot(drag.screen_dir);
                    let raw_steps = (proj / drag.px_per_voxel.max(1.0)).round() as i32;
                    let snap_val = self.snap.max(1) as i32;
                    let snapped = (raw_steps / snap_val) * snap_val;

                    if snapped != drag.applied_amount {
                        // Restore starting snapshot
                        project.grid = drag.initial_grid.clone();
                        if let Some(ref sel) = drag.initial_selection {
                            project.selection = sel.clone();
                        }

                        // Apply extrusion from baseline
                        if snapped != 0 {
                            let amount = snapped * drag.face.sign();
                            apply_volume_extrusion(project, drag.face.axis(), amount);
                        }
                        drag.applied_amount = snapped;
                    }
                }
                self.active_drag = Some(drag);
                return None;
            } else if primary_released || !primary_down {
                // Drag gesture completed: commit atomic undo step
                let final_amount = drag.applied_amount;
                let face_name = drag.face.label();
                if final_amount != 0 {
                    // Force revision refresh & record single undo entry
                    project.record_undo();
                    // Re-apply to ensure the snapshot in undo stack reflects state
                    project.grid = drag.initial_grid.clone();
                    if let Some(ref sel) = drag.initial_selection {
                        project.selection = sel.clone();
                    }
                    let amount = final_amount * drag.face.sign();
                    apply_volume_extrusion(project, drag.face.axis(), amount);

                    return Some(format!(
                        "Extruded {} by {} voxels (Snap {})",
                        face_name, final_amount, self.snap
                    ));
                }
                return None;
            }
        }

        // 2. Not dragging: detect hover over the 6 face handles
        self.hovered_face = None;
        let Some(pos) = pointer_pos else {
            return None;
        };

        for &face in &ExtrudeFace::ALL {
            let fc = Self::face_center(face, min, max);
            let handle_3d = fc + face.normal() * 0.9;
            if let Some(s_pos) = to_screen(handle_3d) {
                if pos.distance(s_pos) <= 13.0 {
                    self.hovered_face = Some(face);

                    // If user clicks, begin drag
                    if primary_clicked {
                        let arrow_3d = fc + face.normal() * 2.8;
                        let s_arrow = to_screen(arrow_3d).unwrap_or(s_pos + Vec2::new(0.0, -20.0));
                        let dir_vec = s_arrow - s_pos;
                        let len = dir_vec.length().max(1.0);
                        let screen_dir = dir_vec / len;
                        let px_per_vx = (len / 1.9).max(2.0);

                        self.active_drag = Some(ExtrudeDrag {
                            face,
                            start_pointer: pos,
                            screen_dir,
                            px_per_voxel: px_per_vx,
                            initial_grid: project.grid.clone(),
                            initial_selection: if project.selection.any_selected() {
                                Some(project.selection.clone())
                            } else {
                                None
                            },
                            applied_amount: 0,
                        });
                    }
                    break;
                }
            }
        }

        None
    }

    /// Render the handles and interactive overlays with `egui::Painter`.
    pub fn render<F>(
        &self,
        painter: &egui::Painter,
        project: &EditorProject,
        to_screen: &F,
    ) where
        F: Fn(Vec3) -> Option<Pos2>,
    {
        if !self.enabled {
            return;
        }

        let (min, max) = Self::volume_bounds(project);

        // 1. Draw subtle wireframe bounding box of the volume
        let corners = [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(min.x, max.y, max.z),
        ];

        let edges = [
            (0, 1), (1, 2), (2, 3), (3, 0), // Bottom
            (4, 5), (5, 6), (6, 7), (7, 4), // Top
            (0, 4), (1, 5), (2, 6), (3, 7), // Verticals
        ];

        let box_stroke = Stroke::new(
            1.0,
            if project.selection.any_selected() {
                Color32::from_rgba_premultiplied(255, 180, 50, 110)
            } else {
                Color32::from_rgba_premultiplied(120, 160, 220, 75)
            },
        );

        for &(i, j) in &edges {
            if let (Some(p0), Some(p1)) = (to_screen(corners[i]), to_screen(corners[j])) {
                painter.line_segment([p0, p1], box_stroke);
            }
        }

        // 2. Render each of the 6 face handles
        for &face in &ExtrudeFace::ALL {
            let fc = Self::face_center(face, min, max);
            let handle_3d = fc + face.normal() * 0.9;
            let _arrow_3d = fc + face.normal() * 2.8;

            let (Some(s_fc), Some(s_handle)) = (to_screen(fc), to_screen(handle_3d)) else {
                continue;
            };

            let is_hov = self.hovered_face == Some(face);
            let is_drag = self.active_drag.as_ref().map(|d| d.face) == Some(face);
            let color = face.color();

            // Stem line from face center to handle
            painter.line_segment(
                [s_fc, s_handle],
                Stroke::new(if is_drag || is_hov { 2.5 } else { 1.2 }, color),
            );

            // Handle Cap / Diamond
            let size = if is_drag { 10.0 } else if is_hov { 8.5 } else { 6.5 };

            painter.circle_filled(
                s_handle,
                size,
                if is_drag || is_hov {
                    Color32::WHITE
                } else {
                    color
                },
            );
            painter.circle_stroke(
                s_handle,
                size,
                Stroke::new(2.0, if is_drag || is_hov { color } else { Color32::WHITE }),
            );

            // Label pill badge if hovered or dragging
            if is_drag {
                if let Some(ref drag) = self.active_drag {
                    let pill_text = format!(
                        "⇥ Extrude {}: {}{} vx (Snap {})",
                        face.label(),
                        if drag.applied_amount >= 0 { "+" } else { "" },
                        drag.applied_amount,
                        self.snap
                    );
                    let badge_pos = s_handle + Vec2::new(0.0, -22.0);
                    let badge_rect = Rect::from_center_size(badge_pos, Vec2::new(170.0, 22.0));
                    painter.rect_filled(badge_rect, 4.0, Color32::from_rgb(20, 24, 32));
                    painter.rect_stroke(
                        badge_rect,
                        4.0,
                        Stroke::new(1.5, color),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        badge_pos,
                        Align2::CENTER_CENTER,
                        pill_text,
                        FontId::monospace(10.5),
                        Color32::WHITE,
                    );
                }
            } else if is_hov {
                let badge_pos = s_handle + Vec2::new(0.0, -18.0);
                let badge_rect = Rect::from_center_size(badge_pos, Vec2::new(80.0, 18.0));
                painter.rect_filled(badge_rect, 4.0, Color32::from_rgb(24, 28, 36));
                painter.rect_stroke(
                    badge_rect,
                    4.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    badge_pos,
                    Align2::CENTER_CENTER,
                    format!("{} (Snap {})", face.label(), self.snap),
                    FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
        }
    }
}

/// Helper function to apply extrusion to either active selection or whole grid face.
pub fn apply_volume_extrusion(project: &mut EditorProject, axis: Axis, amount: i32) {
    if amount == 0 {
        return;
    }

    if project.selection.any_selected() {
        let dir = match axis {
            Axis::X => IVec3::new(amount.signum(), 0, 0),
            Axis::Y => IVec3::new(0, amount.signum(), 0),
            Axis::Z => IVec3::new(0, 0, amount.signum()),
        };
        let steps = amount.abs();
        for step in 1..=steps {
            for z in 0..project.grid.size_z() {
                for y in 0..project.grid.size_y() {
                    for x in 0..project.grid.size_x() {
                        if project.selection.get(x, y, z) {
                            let col = project.grid.get(x, y, z);
                            if col != 0 {
                                let target = IVec3::new(x as i32, y as i32, z as i32) + dir * step;
                                if project.grid.in_bounds(target.x, target.y, target.z) {
                                    project.grid.set_i32(target.x, target.y, target.z, col);
                                    project.selection.set(
                                        target.x as u32,
                                        target.y as u32,
                                        target.z as u32,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        project.grid.extrude(axis, amount);
    }
}
