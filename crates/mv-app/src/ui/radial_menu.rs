//! In-situ contextual Radial Menu ("Pie Menu") for MagicaVoxel.
//!
//! Invoked via Right-Click or Space in the 3D viewport.
//! Features circular angular sectors drawn directly with `egui::Painter`:
//! - Brushes: Voxel, Face, Box, Line, Center, Pattern
//! - Cozy Tools: Wall Spline, Arch, Roof, Door/Window, Lantern, Tree, Pond
//! - Quick Actions: Symmetries (X, Y, Z), Recent Colors, Extrusion, Undo
//!
//! Provides smooth angle detection, hover animation/feedback, and instant execution
//! on release or click.

use std::f32::consts::TAU;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use glam::IVec3;
use mv_core::{Axis, BrushMode, EditorProject};

/// Main categories of the radial menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialCategory {
    Brushes,
    CozyTools,
    QuickActions,
}

impl RadialCategory {
    pub const ALL: [RadialCategory; 3] = [
        RadialCategory::Brushes,
        RadialCategory::CozyTools,
        RadialCategory::QuickActions,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Self::Brushes => "Brushes",
            Self::CozyTools => "Cozy Builder",
            Self::QuickActions => "Quick Actions",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Brushes => "🖌",
            Self::CozyTools => "🏡",
            Self::QuickActions => "⚡",
        }
    }

    pub fn theme_color(&self) -> Color32 {
        match self {
            Self::Brushes => Color32::from_rgb(60, 130, 240),
            Self::CozyTools => Color32::from_rgb(50, 180, 120),
            Self::QuickActions => Color32::from_rgb(220, 140, 50),
        }
    }
}

/// Action represented by an angular slice in the radial wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialAction {
    // Brushes
    BrushVoxel,
    BrushFace,
    BrushBox,
    BrushLine,
    BrushCenter,
    BrushPattern,

    // Cozy Tools
    CozyWallSpline,
    CozyArch,
    CozyRoof,
    CozyDoorWindow,
    CozyLantern,
    CozyTree,
    CozyPond,

    // Quick Actions
    ToggleMirrorX,
    ToggleMirrorY,
    ToggleMirrorZ,
    ExtrudeUp,
    ExtrudeDown,
    PickRecentColor(u8),
    Undo,
}

impl RadialAction {
    pub fn title(&self) -> &'static str {
        match self {
            Self::BrushVoxel => "Voxel",
            Self::BrushFace => "Face",
            Self::BrushBox => "Box",
            Self::BrushLine => "Line",
            Self::BrushCenter => "Center",
            Self::BrushPattern => "Pattern",

            Self::CozyWallSpline => "Wall Spline",
            Self::CozyArch => "Arch",
            Self::CozyRoof => "Roof",
            Self::CozyDoorWindow => "Door / Window",
            Self::CozyLantern => "Lantern",
            Self::CozyTree => "Tree",
            Self::CozyPond => "Pond",

            Self::ToggleMirrorX => "Mirror X",
            Self::ToggleMirrorY => "Mirror Y",
            Self::ToggleMirrorZ => "Mirror Z",
            Self::ExtrudeUp => "Extrude +Z",
            Self::ExtrudeDown => "Extrude -Z",
            Self::PickRecentColor(_) => "Recent Color",
            Self::Undo => "Undo",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::BrushVoxel => "●",
            Self::BrushFace => "▧",
            Self::BrushBox => "■",
            Self::BrushLine => "╱",
            Self::BrushCenter => "◎",
            Self::BrushPattern => "☵",

            Self::CozyWallSpline => "🏰",
            Self::CozyArch => "🏛",
            Self::CozyRoof => "🏡",
            Self::CozyDoorWindow => "🪟",
            Self::CozyLantern => "🏮",
            Self::CozyTree => "🌳",
            Self::CozyPond => "💧",

            Self::ToggleMirrorX => "⇹",
            Self::ToggleMirrorY => "⇳",
            Self::ToggleMirrorZ => "⮃",
            Self::ExtrudeUp => "⇡",
            Self::ExtrudeDown => "⇣",
            Self::PickRecentColor(_) => "🎨",
            Self::Undo => "↶",
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            Self::BrushVoxel => "V",
            Self::BrushFace => "F",
            Self::BrushBox => "B",
            Self::BrushLine => "L",
            Self::BrushCenter => "C",
            Self::BrushPattern => "P",

            Self::CozyWallSpline => "",
            Self::CozyArch => "",
            Self::CozyRoof => "",
            Self::CozyDoorWindow => "",
            Self::CozyLantern => "",
            Self::CozyTree => "",
            Self::CozyPond => "",

            Self::ToggleMirrorX => "1",
            Self::ToggleMirrorY => "2",
            Self::ToggleMirrorZ => "3",
            Self::ExtrudeUp => "",
            Self::ExtrudeDown => "",
            Self::PickRecentColor(_) => "",
            Self::Undo => "Ctrl+Z",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::BrushVoxel => "Single-voxel precision sculpting",
            Self::BrushFace => "Extrude coplanar connected faces",
            Self::BrushBox => "Drag cuboid volume",
            Self::BrushLine => "Draw straight voxel line",
            Self::BrushCenter => "Spherical centered volume brush",
            Self::BrushPattern => "Stamp pattern brush",

            Self::CozyWallSpline => "Build organic stone wall with crenels",
            Self::CozyArch => "Carve curved classical stone archway",
            Self::CozyRoof => "Generate cozy gabled or hipped roof",
            Self::CozyDoorWindow => "Punch cozy window or wooden door",
            Self::CozyLantern => "Spawn warm glowing cozy lantern",
            Self::CozyTree => "Spawn procedural fluffy tree canopy",
            Self::CozyPond => "Carve organic pond with water & stones",

            Self::ToggleMirrorX => "Toggle X-axis mirror symmetry",
            Self::ToggleMirrorY => "Toggle Y-axis mirror symmetry",
            Self::ToggleMirrorZ => "Toggle Z-axis mirror symmetry",
            Self::ExtrudeUp => "Extrude top face upward by 1 voxel",
            Self::ExtrudeDown => "Extrude bottom face downward by 1 voxel",
            Self::PickRecentColor(_) => "Select recently used palette color",
            Self::Undo => "Undo the last editing step",
        }
    }
}

/// State of the interactive radial menu.
#[derive(Debug, Clone)]
pub struct RadialMenu {
    pub is_open: bool,
    pub center: Pos2,
    pub selected_category: RadialCategory,
    pub hovered_category: Option<RadialCategory>,
    pub hovered_action: Option<RadialAction>,
    pub recent_colors: Vec<u8>,
    pub opened_frame_time: f64,
    /// Anim progress 0.0 -> 1.0
    pub anim_t: f32,
}

impl Default for RadialMenu {
    fn default() -> Self {
        Self {
            is_open: false,
            center: Pos2::ZERO,
            selected_category: RadialCategory::Brushes,
            hovered_category: None,
            hovered_action: None,
            recent_colors: vec![1, 10, 25, 42, 60],
            opened_frame_time: 0.0,
            anim_t: 1.0,
        }
    }
}

impl RadialMenu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open menu at the specified screen position.
    pub fn open(&mut self, pos: Pos2, time: f64) {
        self.is_open = true;
        self.center = pos;
        self.opened_frame_time = time;
        self.hovered_action = None;
        self.hovered_category = None;
        self.anim_t = 0.0;
    }

    /// Close the menu.
    pub fn close(&mut self) {
        self.is_open = false;
        self.hovered_action = None;
        self.hovered_category = None;
    }

    /// Record a used color into the recent colors list.
    pub fn record_color(&mut self, color: u8) {
        if color == 0 {
            return;
        }
        self.recent_colors.retain(|&c| c != color);
        self.recent_colors.insert(0, color);
        if self.recent_colors.len() > 6 {
            self.recent_colors.truncate(6);
        }
    }

    /// Actions for the currently active category.
    pub fn category_actions(&self, category: RadialCategory) -> Vec<RadialAction> {
        match category {
            RadialCategory::Brushes => vec![
                RadialAction::BrushVoxel,
                RadialAction::BrushFace,
                RadialAction::BrushBox,
                RadialAction::BrushLine,
                RadialAction::BrushCenter,
                RadialAction::BrushPattern,
            ],
            RadialCategory::CozyTools => vec![
                RadialAction::CozyRoof,
                RadialAction::CozyDoorWindow,
                RadialAction::CozyLantern,
                RadialAction::CozyTree,
                RadialAction::CozyPond,
                RadialAction::CozyArch,
                RadialAction::CozyWallSpline,
            ],
            RadialCategory::QuickActions => {
                let mut acts = vec![
                    RadialAction::ToggleMirrorX,
                    RadialAction::ToggleMirrorY,
                    RadialAction::ToggleMirrorZ,
                    RadialAction::ExtrudeUp,
                    RadialAction::ExtrudeDown,
                    RadialAction::Undo,
                ];
                for &c in self.recent_colors.iter().take(2) {
                    acts.push(RadialAction::PickRecentColor(c));
                }
                acts
            }
        }
    }

    /// Update mouse hover and determine hovered items.
    pub fn update_hover(&mut self, pointer_pos: Option<Pos2>, dt: f32) {
        self.anim_t = (self.anim_t + dt * 10.0).clamp(0.0, 1.0);

        let Some(pos) = pointer_pos else {
            self.hovered_action = None;
            self.hovered_category = None;
            return;
        };

        let delta = pos - self.center;
        let dist = delta.length();
        let mut angle = delta.y.atan2(delta.x);
        if angle < 0.0 {
            angle += TAU;
        }

        // Inner ring: Categories (radius 42..86)
        if dist >= 42.0 && dist < 90.0 {
            let cat_slice = TAU / 3.0;
            let idx = ((angle / cat_slice) as usize) % 3;
            let cat = RadialCategory::ALL[idx];
            self.hovered_category = Some(cat);
            self.selected_category = cat;
            self.hovered_action = None;
            return;
        }

        // Outer ring: Actions (radius 90..230)
        if dist >= 90.0 && dist <= 240.0 {
            let actions = self.category_actions(self.selected_category);
            if !actions.is_empty() {
                let count = actions.len();
                let slice = TAU / count as f32;
                let idx = ((angle / slice) as usize) % count;
                self.hovered_action = Some(actions[idx]);
                self.hovered_category = None;
                return;
            }
        }

        self.hovered_action = None;
        self.hovered_category = None;
    }

    /// Render the radial menu using `egui::Painter`.
    pub fn render(&self, painter: &egui::Painter, pointer_pos: Option<Pos2>) {
        if !self.is_open {
            return;
        }

        let scale = (0.7 + 0.3 * self.anim_t).clamp(0.1, 1.0);
        let center = self.center;

        // Background subtle shadow circle
        painter.circle_filled(
            center,
            245.0 * scale,
            Color32::from_rgba_premultiplied(12, 14, 18, 170),
        );

        // 1. Center Hub (radius 0..38)
        let delta = pointer_pos.map(|p| p - center);
        let dist = delta.map(|d| d.length()).unwrap_or(999.0);
        let center_hovered = dist < 40.0;

        let hub_color = if center_hovered {
            Color32::from_rgb(180, 50, 50)
        } else {
            Color32::from_rgba_premultiplied(26, 28, 34, 240)
        };
        painter.circle_filled(center, 38.0 * scale, hub_color);
        painter.circle_stroke(
            center,
            38.0 * scale,
            Stroke::new(
                2.0,
                if center_hovered {
                    Color32::from_rgb(255, 120, 120)
                } else {
                    self.selected_category.theme_color()
                },
            ),
        );

        painter.text(
            center - Vec2::new(0.0, 6.0) * scale,
            Align2::CENTER_CENTER,
            self.selected_category.icon(),
            FontId::proportional(16.0 * scale),
            Color32::WHITE,
        );
        painter.text(
            center + Vec2::new(0.0, 10.0) * scale,
            Align2::CENTER_CENTER,
            if center_hovered { "CLOSE" } else { "COZY" },
            FontId::monospace(8.5 * scale),
            if center_hovered {
                Color32::from_rgb(255, 200, 200)
            } else {
                Color32::from_rgb(180, 190, 210)
            },
        );

        // 2. Inner Ring: Category wedges (radius 42..86)
        let r_in_min = 42.0 * scale;
        let r_in_max = 86.0 * scale;
        let cat_slice = TAU / 3.0;

        for (i, &cat) in RadialCategory::ALL.iter().enumerate() {
            let a_start = i as f32 * cat_slice;
            let a_end = (i + 1) as f32 * cat_slice;
            let is_sel = cat == self.selected_category;
            let is_hov = Some(cat) == self.hovered_category;

            let fill = if is_hov {
                cat.theme_color()
            } else if is_sel {
                let c = cat.theme_color();
                Color32::from_rgba_premultiplied(c.r() / 2, c.g() / 2, c.b() / 2, 220)
            } else {
                Color32::from_rgba_premultiplied(32, 34, 42, 210)
            };

            let stroke = Stroke::new(
                if is_sel || is_hov { 2.0 } else { 1.0 },
                if is_sel || is_hov {
                    cat.theme_color()
                } else {
                    Color32::from_rgb(60, 65, 80)
                },
            );

            draw_annular_sector(painter, center, r_in_min, r_in_max, a_start, a_end, fill, stroke);

            // Label & icon at midpoint
            let mid_a = (a_start + a_end) * 0.5;
            let mid_r = (r_in_min + r_in_max) * 0.5;
            let icon_pos = center + Vec2::new(mid_a.cos(), mid_a.sin()) * mid_r;

            painter.text(
                icon_pos,
                Align2::CENTER_CENTER,
                format!("{} {}", cat.icon(), cat.title()),
                FontId::proportional(10.5 * scale),
                if is_sel || is_hov {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(190, 200, 215)
                },
            );
        }

        // 3. Outer Ring: Actions of current category (radius 92..185)
        let actions = self.category_actions(self.selected_category);
        if !actions.is_empty() {
            let count = actions.len();
            let slice = TAU / count as f32;
            let theme = self.selected_category.theme_color();

            for (i, act) in actions.iter().enumerate() {
                let a_start = i as f32 * slice;
                let a_end = (i + 1) as f32 * slice;
                let is_hov = self.hovered_action == Some(*act);

                let r_out_min = 92.0 * scale;
                let r_out_max = if is_hov { 198.0 * scale } else { 182.0 * scale };

                let fill = if is_hov {
                    Color32::from_rgba_premultiplied(
                        (theme.r() as f32 * 0.85) as u8,
                        (theme.g() as f32 * 0.85) as u8,
                        (theme.b() as f32 * 0.85) as u8,
                        245,
                    )
                } else {
                    Color32::from_rgba_premultiplied(28, 31, 38, 225)
                };

                let stroke = Stroke::new(
                    if is_hov { 2.5 } else { 1.0 },
                    if is_hov {
                        Color32::WHITE
                    } else {
                        Color32::from_rgba_premultiplied(65, 72, 90, 180)
                    },
                );

                draw_annular_sector(
                    painter, center, r_out_min, r_out_max, a_start, a_end, fill, stroke,
                );

                // Action icon, title, shortcut
                let mid_a = (a_start + a_end) * 0.5;
                let mid_r = (r_out_min + r_out_max) * 0.5;
                let text_pos = center + Vec2::new(mid_a.cos(), mid_a.sin()) * mid_r;

                let icon = act.icon();
                let title = act.title();
                let sc = act.shortcut();

                let label_text = if !sc.is_empty() {
                    format!("{} {}\n({})", icon, title, sc)
                } else {
                    format!("{} {}", icon, title)
                };

                painter.text(
                    text_pos,
                    Align2::CENTER_CENTER,
                    label_text,
                    FontId::proportional(if is_hov { 11.5 * scale } else { 10.5 * scale }),
                    if is_hov {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(220, 225, 235)
                    },
                );
            }
        }

        // 4. Floating description pill at bottom
        let desc_text = if let Some(act) = self.hovered_action {
            format!("{} {} — {}", act.icon(), act.title(), act.description())
        } else if let Some(cat) = self.hovered_category {
            format!("Category: {}", cat.title())
        } else if center_hovered {
            "Click or release to cancel".to_string()
        } else {
            format!("Radial Menu: {}", self.selected_category.title())
        };

        let pill_pos = center + Vec2::new(0.0, 215.0 * scale);
        let pill_rect = Rect::from_center_size(pill_pos, Vec2::new(320.0 * scale, 28.0 * scale));
        painter.rect_filled(
            pill_rect,
            6.0,
            Color32::from_rgba_premultiplied(18, 20, 26, 230),
        );
        painter.rect_stroke(
            pill_rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgb(60, 70, 90)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            pill_pos,
            Align2::CENTER_CENTER,
            desc_text,
            FontId::proportional(11.0 * scale),
            Color32::from_rgb(240, 240, 250),
        );
    }
}

/// Helper function to tessellate and draw an annular arc sector.
fn draw_annular_sector(
    painter: &egui::Painter,
    center: Pos2,
    r_inner: f32,
    r_outer: f32,
    a_start: f32,
    a_end: f32,
    fill: Color32,
    stroke: Stroke,
) {
    let span = (a_end - a_start).abs();
    let steps = 14.max((span * 16.0) as usize);
    let mut points = Vec::with_capacity(steps * 2 + 2);

    // Outer arc: a_start -> a_end
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = a_start + (a_end - a_start) * t;
        points.push(center + Vec2::new(a.cos(), a.sin()) * r_outer);
    }

    // Inner arc: a_end -> a_start
    for i in (0..=steps).rev() {
        let t = i as f32 / steps as f32;
        let a = a_start + (a_end - a_start) * t;
        points.push(center + Vec2::new(a.cos(), a.sin()) * r_inner);
    }

    painter.add(Shape::convex_polygon(points, fill, stroke));
}

/// Execute a selected RadialAction on the active EditorProject.
pub fn execute_radial_action(
    project: &mut EditorProject,
    action: RadialAction,
) -> Option<String> {
    match action {
        RadialAction::BrushVoxel => {
            project.brush.mode = BrushMode::Voxel;
            Some("Brush mode set to Voxel".to_string())
        }
        RadialAction::BrushFace => {
            project.brush.mode = BrushMode::Face;
            Some("Brush mode set to Face".to_string())
        }
        RadialAction::BrushBox => {
            project.brush.mode = BrushMode::Box;
            Some("Brush mode set to Box".to_string())
        }
        RadialAction::BrushLine => {
            project.brush.mode = BrushMode::Line;
            Some("Brush mode set to Line".to_string())
        }
        RadialAction::BrushCenter => {
            project.brush.mode = BrushMode::Center;
            Some("Brush mode set to Center".to_string())
        }
        RadialAction::BrushPattern => {
            project.brush.mode = BrushMode::Pattern;
            Some("Brush mode set to Pattern".to_string())
        }
        RadialAction::ToggleMirrorX => {
            project.brush.mirror.x = !project.brush.mirror.x;
            Some(format!("Mirror X: {}", project.brush.mirror.x))
        }
        RadialAction::ToggleMirrorY => {
            project.brush.mirror.y = !project.brush.mirror.y;
            Some(format!("Mirror Y: {}", project.brush.mirror.y))
        }
        RadialAction::ToggleMirrorZ => {
            project.brush.mirror.z = !project.brush.mirror.z;
            Some(format!("Mirror Z: {}", project.brush.mirror.z))
        }
        RadialAction::ExtrudeUp => {
            project.record_undo();
            project.grid.extrude(Axis::Z, 1);
            Some("Extruded top face (+Z)".to_string())
        }
        RadialAction::ExtrudeDown => {
            project.record_undo();
            project.grid.extrude(Axis::Z, -1);
            Some("Extruded bottom face (-Z)".to_string())
        }
        RadialAction::PickRecentColor(c) => {
            project.palette.selected = c;
            Some(format!("Selected palette color #{}", c))
        }
        RadialAction::Undo => {
            if project.undo() {
                Some("Undo successful".to_string())
            } else {
                Some("Nothing to undo".to_string())
            }
        }
        RadialAction::CozyRoof => {
            // Apply a default gabled cozy roof over selection or grid bounds
            let (min, max) = if project.selection.any_selected() {
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
                (lo, hi)
            } else {
                let top_z = (0..project.grid.size_z())
                    .rev()
                    .find(|&z| {
                        (0..project.grid.size_x()).any(|x| {
                            (0..project.grid.size_y()).any(|y| project.grid.get(x, y, z) != 0)
                        })
                    })
                    .unwrap_or(project.grid.size_z() / 2);

                (
                    IVec3::new(0, 0, top_z as i32),
                    IVec3::new(
                        project.grid.size_x() as i32,
                        project.grid.size_y() as i32,
                        project.grid.size_z() as i32,
                    ),
                )
            };
            project.record_undo();
            let roof_color = project.palette.selected.max(1);
            mv_core::architecture::generate_roof(
                &mut project.grid,
                min,
                max,
                mv_core::architecture::RoofType::Gabled,
                roof_color,
                1,
            );
            Some("Generated cozy gabled roof".to_string())
        }
        RadialAction::CozyDoorWindow => {
            let cx = project.grid.size_x() as i32 / 2;
            let min = IVec3::new(cx - 3, 0, (project.grid.size_z() as i32 / 4).max(2));
            let max = IVec3::new(cx + 3, 2, min.z + 6);
            project.record_undo();
            let frame_color = project.palette.selected.max(1);
            let glass_color = 2;
            mv_core::architecture::punch_window(
                &mut project.grid,
                min,
                max,
                frame_color,
                glass_color,
                true,
            );
            Some("Punched cozy framed window with shutters".to_string())
        }
        RadialAction::CozyLantern => {
            let pos = IVec3::new(
                project.grid.size_x() as i32 / 2,
                project.grid.size_y() as i32 / 2,
                1,
            );
            project.record_undo();
            let frame_color = project.palette.selected.max(1);
            mv_core::light::spawn_lantern(&mut project.grid, pos, IVec3::ZERO, frame_color, 3);
            Some("Spawned cozy warm lantern".to_string())
        }
        RadialAction::CozyTree => {
            let base = IVec3::new(
                project.grid.size_x() as i32 / 2,
                project.grid.size_y() as i32 / 2,
                0,
            );
            let tree_height = (project.grid.size_z() / 2).max(10);
            project.record_undo();
            let trunk = project.palette.selected.max(1);
            let foliage = (trunk.wrapping_add(10)).max(1);
            mv_core::biome::generate_fluffy_tree(
                &mut project.grid,
                base,
                tree_height,
                5,
                trunk,
                foliage,
            );
            Some("Spawned fluffy organic tree".to_string())
        }
        RadialAction::CozyPond => {
            let center = IVec3::new(
                project.grid.size_x() as i32 / 2,
                project.grid.size_y() as i32 / 2,
                0,
            );
            project.record_undo();
            let water = project.palette.selected.max(1);
            mv_core::biome::carve_pond(&mut project.grid, center, 7, 3, water);
            Some("Carved organic cozy pond".to_string())
        }
        RadialAction::CozyArch => {
            let center = IVec3::new(
                project.grid.size_x() as i32 / 2,
                project.grid.size_y() as i32 / 2,
                0,
            );
            project.record_undo();
            let frame = project.palette.selected.max(1);
            mv_core::spline::carve_arch(&mut project.grid, center, 8, 10, 4, frame);
            Some("Carved classical stone arch".to_string())
        }
        RadialAction::CozyWallSpline => {
            let sx = project.grid.size_x() as f32;
            let sy = project.grid.size_y() as f32;
            let pts = vec![
                glam::Vec3::new(4.0, 4.0, 0.0),
                glam::Vec3::new(sx * 0.4, sy * 0.6, 0.0),
                glam::Vec3::new(sx * 0.7, sy * 0.3, 0.0),
                glam::Vec3::new(sx - 4.0, sy - 4.0, 0.0),
            ];
            project.record_undo();
            let color = project.palette.selected.max(1);
            mv_core::spline::generate_spline_wall(
                &mut project.grid,
                &pts,
                2,
                6,
                true,
                color,
            );
            Some("Generated cozy spline wall with crenels".to_string())
        }
    }
}
