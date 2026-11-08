//! Interactive View Cube in the top-right corner of the viewport: shows the camera
//! orientation (Z up) and snaps the camera to an axis view when a face is clicked.

use eframe::egui::{self, Color32, FontId, Pos2, Sense, Shape, Stroke};
use glam::Vec3;
use mv_render::OrbitCamera;

use crate::app::MagicaVoxelApp;

const CUBE_SIZE: f32 = 86.0;
const MARGIN: f32 = 10.0;

pub(crate) struct CubeFace {
    pub(crate) label: &'static str,
    pub(crate) normal: Vec3,
    pub(crate) polygon: [Pos2; 4],
    /// Camera yaw for this view (`None`: keep the current yaw, for top / bottom).
    pub(crate) yaw: Option<f32>,
    pub(crate) pitch: f32,
}

impl CubeFace {
    pub(crate) fn center(&self) -> Pos2 {
        let s = self
            .polygon
            .iter()
            .fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2());
        (s / 4.0).to_pos2()
    }

    /// Point inside the (convex) projected face.
    fn contains(&self, p: Pos2) -> bool {
        let mut sign = 0.0f32;
        for i in 0..4 {
            let a = self.polygon[i];
            let b = self.polygon[(i + 1) % 4];
            let cross = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
            if cross.abs() < f32::EPSILON {
                continue;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
        true
    }
}

/// Screen rectangle of the cube inside a viewport rectangle.
pub(crate) fn view_cube_rect(viewport: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            viewport.right() - CUBE_SIZE - MARGIN,
            viewport.top() + MARGIN,
        ),
        egui::vec2(CUBE_SIZE, CUBE_SIZE),
    )
}

/// Faces turned toward the camera, projected into `rect`.
pub(crate) fn view_cube_faces(camera: &OrbitCamera, rect: egui::Rect) -> Vec<CubeFace> {
    let (_, forward, right, up) = camera.basis();
    let center = rect.center();
    let half = rect.width() * 0.27;
    let project = |p: Vec3| center + egui::vec2(p.dot(right), -p.dot(up)) * half;
    let faces: [(&str, Vec3, Option<f32>, f32); 6] = [
        ("Top", Vec3::Z, None, 89.0),
        ("Bottom", -Vec3::Z, None, -89.0),
        ("Right", Vec3::X, Some(0.0), 0.0),
        ("Left", -Vec3::X, Some(180.0), 0.0),
        ("Back", Vec3::Y, Some(90.0), 0.0),
        ("Front", -Vec3::Y, Some(-90.0), 0.0),
    ];
    faces
        .into_iter()
        .filter(|(_, n, _, _)| n.dot(forward) < -1e-3)
        .map(|(label, n, yaw, pitch)| {
            let (u, v) = if n.z.abs() > 0.5 {
                (Vec3::X, Vec3::Y)
            } else if n.x.abs() > 0.5 {
                (Vec3::Y, Vec3::Z)
            } else {
                (Vec3::X, Vec3::Z)
            };
            CubeFace {
                label,
                normal: n,
                polygon: [
                    project(n - u - v),
                    project(n + u - v),
                    project(n + u + v),
                    project(n - u + v),
                ],
                yaw,
                pitch,
            }
        })
        .collect()
}

fn face_color(normal: Vec3, hovered: bool) -> Color32 {
    let base = if normal.x.abs() > 0.5 {
        [150, 92, 92]
    } else if normal.y.abs() > 0.5 {
        [92, 140, 96]
    } else {
        [92, 110, 160]
    };
    let k = if hovered { 1.35 } else { 1.0 };
    let c = base.map(|v| ((v as f32 * k).min(255.0)) as u8);
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 225)
}

impl MagicaVoxelApp {
    /// Draw the cube over the viewport and handle clicks on its faces.
    pub(crate) fn show_view_cube(&mut self, ui: &mut egui::Ui, viewport: egui::Rect) {
        let rect = view_cube_rect(viewport);
        let response = ui.interact(rect, ui.id().with("view_cube"), Sense::click());
        let faces = view_cube_faces(&self.camera, rect);
        let hover = response.hover_pos();
        let hovered = hover.and_then(|p| faces.iter().position(|f| f.contains(p)));

        let painter = ui.painter_at(rect);
        for (i, face) in faces.iter().enumerate() {
            painter.add(Shape::convex_polygon(
                face.polygon.to_vec(),
                face_color(face.normal, hovered == Some(i)),
                Stroke::new(1.0, Color32::from_gray(30)),
            ));
            painter.text(
                face.center(),
                egui::Align2::CENTER_CENTER,
                face.label,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        if response.clicked() {
            if let Some(face) = response
                .interact_pointer_pos()
                .and_then(|p| faces.iter().find(|f| f.contains(p)))
            {
                if let Some(yaw) = face.yaw {
                    self.camera.yaw_deg = yaw;
                }
                self.camera.pitch_deg = face.pitch;
                self.pathtracer.reset_accumulation();
            }
        }
        if hovered.is_some() {
            response.on_hover_text("Click a face to look along that axis");
        }
    }
}
