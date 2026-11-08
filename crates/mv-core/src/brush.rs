use glam::IVec3;
use std::collections::VecDeque;

use crate::selection::SelectionMask;
use crate::voxel::{AxisFlags, VoxelGrid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushAction {
    Attach,
    Erase,
    Paint,
    Select,
}

impl BrushAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Attach => "Attach (T)",
            Self::Erase => "Erase (R)",
            Self::Paint => "Paint (G)",
            Self::Select => "Select (N)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushMode {
    Voxel,
    Face,
    Box,
    Line,
    Center,
    Pattern,
}

impl BrushMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Voxel => "Voxel (V)",
            Self::Face => "Face (F)",
            Self::Box => "Box (B)",
            Self::Line => "Line (L)",
            Self::Center => "Center (C)",
            Self::Pattern => "Pattern (P)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoxelSubShape {
    Cube,
    Sphere,
}

#[derive(Debug, Clone)]
pub struct BrushSettings {
    pub action: BrushAction,
    pub mode: BrushMode,
    pub sub_shape: VoxelSubShape,
    /// Brush radius/size (1..=9)
    pub size: i32,
    /// Mirror symmetry X, Y, Z (`1`, `2`, `3`)
    pub mirror: AxisFlags,
    /// Axis line fill X, Y, Z (`CTRL+1`, `CTRL+2`, `CTRL+3`)
    pub axis: AxisFlags,
    /// Center mode circle vs square
    pub center_circle: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            action: BrushAction::Attach,
            mode: BrushMode::Voxel,
            sub_shape: VoxelSubShape::Cube,
            size: 1,
            mirror: AxisFlags::NONE,
            axis: AxisFlags::NONE,
            center_circle: true,
        }
    }
}

impl BrushSettings {
    /// Compute candidate voxel coordinates before symmetry expansion
    pub fn compute_shape_voxels(
        &self,
        grid: &VoxelGrid,
        start_pos: IVec3,
        end_pos: IVec3,
        hit_normal: IVec3,
    ) -> Vec<IVec3> {
        let anchor = match self.action {
            BrushAction::Attach if self.mode != BrushMode::Face => start_pos + hit_normal,
            _ => start_pos,
        };
        let target = match self.action {
            BrushAction::Attach if self.mode != BrushMode::Face => end_pos + hit_normal,
            _ => end_pos,
        };

        match self.mode {
            BrushMode::Voxel => self.voxel_stamp_at(target),
            BrushMode::Box => {
                let lo = anchor.min(target);
                let hi = anchor.max(target);
                let mut out = Vec::new();
                for z in lo.z..=hi.z {
                    for y in lo.y..=hi.y {
                        for x in lo.x..=hi.x {
                            out.push(IVec3::new(x, y, z));
                        }
                    }
                }
                out
            }
            BrushMode::Line => bresenham_3d(anchor, target),
            BrushMode::Center => {
                let diff = target - anchor;
                let radius = (diff.x.abs().max(diff.y.abs()).max(diff.z.abs())).max(0);
                let mut out = Vec::new();
                // Orient in the plane perpendicular to hit_normal (default XY plane if normal is 0)
                let n = if hit_normal == IVec3::ZERO { IVec3::Z } else { hit_normal };
                for a in -radius..=radius {
                    for b in -radius..=radius {
                        if self.center_circle && (a * a + b * b > radius * radius + radius / 2) {
                            continue;
                        }
                        let p = if n.x != 0 {
                            IVec3::new(anchor.x, anchor.y + a, anchor.z + b)
                        } else if n.y != 0 {
                            IVec3::new(anchor.x + a, anchor.y, anchor.z + b)
                        } else {
                            IVec3::new(anchor.x + a, anchor.y + b, anchor.z)
                        };
                        out.push(p);
                    }
                }
                out
            }
            BrushMode::Face => {
                let face_voxels = flood_contiguous_face(grid, start_pos, hit_normal);
                if self.action == BrushAction::Attach {
                    face_voxels.into_iter().map(|p| p + hit_normal).collect()
                } else {
                    face_voxels
                }
            }
            BrushMode::Pattern => {
                // 3x3x3 cross/arch pattern stamp centered at target
                let mut out = Vec::new();
                for dz in -1..=1i32 {
                    for dy in -1..=1i32 {
                        for dx in -1..=1i32 {
                            if dx.abs() + dy.abs() + dz.abs() <= 2 {
                                out.push(target + IVec3::new(dx, dy, dz));
                            }
                        }
                    }
                }
                out
            }
        }
    }

    fn voxel_stamp_at(&self, center: IVec3) -> Vec<IVec3> {
        let r = (self.size - 1).max(0);
        if r == 0 {
            return vec![center];
        }
        let mut out = Vec::new();
        for dz in -r..=r {
            for dy in -r..=r {
                for dx in -r..=r {
                    match self.sub_shape {
                        VoxelSubShape::Cube => out.push(center + IVec3::new(dx, dy, dz)),
                        VoxelSubShape::Sphere => {
                            if dx * dx + dy * dy + dz * dz <= r * r + r / 2 {
                                out.push(center + IVec3::new(dx, dy, dz));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Expand a list of voxel positions according to Mirror (`1/2/3`) and Axis (`Ctrl+1/2/3`) modes
    pub fn expand_symmetries(&self, grid: &VoxelGrid, points: &[IVec3]) -> Vec<IVec3> {
        let mut out = points.to_vec();
        let sx = grid.size_x() as i32;
        let sy = grid.size_y() as i32;
        let sz = grid.size_z() as i32;

        // Axis mode expansion
        if self.axis.x {
            let base = out.clone();
            for p in base {
                for x in 0..sx {
                    out.push(IVec3::new(x, p.y, p.z));
                }
            }
        }
        if self.axis.y {
            let base = out.clone();
            for p in base {
                for y in 0..sy {
                    out.push(IVec3::new(p.x, y, p.z));
                }
            }
        }
        if self.axis.z {
            let base = out.clone();
            for p in base {
                for z in 0..sz {
                    out.push(IVec3::new(p.x, p.y, z));
                }
            }
        }

        // Mirror mode expansion
        if self.mirror.x {
            let base = out.clone();
            for p in base {
                out.push(IVec3::new(sx - 1 - p.x, p.y, p.z));
            }
        }
        if self.mirror.y {
            let base = out.clone();
            for p in base {
                out.push(IVec3::new(p.x, sy - 1 - p.y, p.z));
            }
        }
        if self.mirror.z {
            let base = out.clone();
            for p in base {
                out.push(IVec3::new(p.x, p.y, sz - 1 - p.z));
            }
        }

        out
    }

    /// Apply brush stroke to the grid/selection
    pub fn apply(
        &self,
        grid: &mut VoxelGrid,
        selection: &mut SelectionMask,
        color: u8,
        start_pos: IVec3,
        end_pos: IVec3,
        hit_normal: IVec3,
        subtract_select: bool,
    ) {
        let base_pts = self.compute_shape_voxels(grid, start_pos, end_pos, hit_normal);
        let all_pts = self.expand_symmetries(grid, &base_pts);
        let paint_color = color.max(1);

        for p in all_pts {
            if !grid.in_bounds(p.x, p.y, p.z) {
                continue;
            }
            let (ux, uy, uz) = (p.x as u32, p.y as u32, p.z as u32);
            match self.action {
                BrushAction::Attach => {
                    grid.set(ux, uy, uz, paint_color);
                }
                BrushAction::Erase => {
                    grid.set(ux, uy, uz, 0);
                    selection.set(ux, uy, uz, false);
                }
                BrushAction::Paint => {
                    if grid.get(ux, uy, uz) != 0 {
                        grid.set(ux, uy, uz, paint_color);
                    }
                }
                BrushAction::Select => {
                    if grid.get(ux, uy, uz) != 0 {
                        selection.set(ux, uy, uz, !subtract_select);
                    }
                }
            }
        }
    }
}

/// 3D Bresenham line algorithm
pub fn bresenham_3d(a: IVec3, b: IVec3) -> Vec<IVec3> {
    let mut pts = Vec::new();
    let mut p = a;
    let d = (b - a).abs();
    let s = IVec3::new(
        if b.x > a.x { 1 } else { -1 },
        if b.y > a.y { 1 } else { -1 },
        if b.z > a.z { 1 } else { -1 },
    );

    pts.push(p);
    if d.x >= d.y && d.x >= d.z {
        let mut p1 = 2 * d.y - d.x;
        let mut p2 = 2 * d.z - d.x;
        while p.x != b.x {
            p.x += s.x;
            if p1 >= 0 {
                p.y += s.y;
                p1 -= 2 * d.x;
            }
            if p2 >= 0 {
                p.z += s.z;
                p2 -= 2 * d.x;
            }
            p1 += 2 * d.y;
            p2 += 2 * d.z;
            pts.push(p);
        }
    } else if d.y >= d.x && d.y >= d.z {
        let mut p1 = 2 * d.x - d.y;
        let mut p2 = 2 * d.z - d.y;
        while p.y != b.y {
            p.y += s.y;
            if p1 >= 0 {
                p.x += s.x;
                p1 -= 2 * d.y;
            }
            if p2 >= 0 {
                p.z += s.z;
                p2 -= 2 * d.y;
            }
            p1 += 2 * d.x;
            p2 += 2 * d.z;
            pts.push(p);
        }
    } else {
        let mut p1 = 2 * d.y - d.z;
        let mut p2 = 2 * d.x - d.z;
        while p.z != b.z {
            p.z += s.z;
            if p1 >= 0 {
                p.y += s.y;
                p1 -= 2 * d.z;
            }
            if p2 >= 0 {
                p.x += s.x;
                p2 -= 2 * d.z;
            }
            p1 += 2 * d.y;
            p2 += 2 * d.x;
            pts.push(p);
        }
    }
    pts
}

/// Flood-fill a contiguous exposed face of the same color in the plane perpendicular to `normal`
fn flood_contiguous_face(grid: &VoxelGrid, start: IVec3, normal: IVec3) -> Vec<IVec3> {
    let target_color = grid.get_i32(start.x, start.y, start.z);
    if target_color == 0 {
        return vec![start];
    }
    let n = if normal == IVec3::ZERO { IVec3::Z } else { normal };
    let tangent_dirs = if n.x != 0 {
        [IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z]
    } else if n.y != 0 {
        [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
    } else {
        [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y]
    };

    let mut visited = std::collections::HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    visited.insert(start);
    queue.push_back(start);

    while let Some(p) = queue.pop_front() {
        result.push(p);
        if result.len() >= 4096 {
            break;
        }
        for d in tangent_dirs {
            let np = p + d;
            if !visited.contains(&np)
                && grid.get_i32(np.x, np.y, np.z) == target_color
                && grid.get_i32(np.x + n.x, np.y + n.y, np.z + n.z) == 0
            {
                visited.insert(np);
                queue.push_back(np);
            }
        }
    }
    result
}

