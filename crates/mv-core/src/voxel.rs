use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec3, UVec3, Vec3};

pub const MAX_GRID_DIM: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AxisFlags {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl AxisFlags {
    pub const NONE: Self = Self { x: false, y: false, z: false };
    pub const ALL: Self = Self { x: true, y: true, z: true };

    pub fn from_token(s: &str) -> Self {
        let mut flags = Self::NONE;
        for ch in s.chars() {
            match ch {
                'x' | 'X' => flags.x = true,
                'y' | 'Y' => flags.y = true,
                'z' | 'Z' => flags.z = true,
                _ => {}
            }
        }
        flags
    }

    pub fn any(self) -> bool {
        self.x || self.y || self.z
    }
}

/// 3D Voxel grid with MagicaVoxel coordinates:
/// X = right [0..size_x), Y = depth [0..size_y), Z = up [0..size_z).
/// Each voxel is a `u8` palette index (`0` = empty, `1..=255` = solid).
static NEXT_GRID_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_grid_revision() -> u64 {
    NEXT_GRID_REVISION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct VoxelGrid {
    size_x: u32,
    size_y: u32,
    size_z: u32,
    data: Vec<u8>,
    /// Globally unique id of the current content. Refreshed by every `&mut self`
    /// method, so two grids with the same revision are guaranteed identical.
    revision: u64,
}

impl PartialEq for VoxelGrid {
    fn eq(&self, other: &Self) -> bool {
        self.size_x == other.size_x
            && self.size_y == other.size_y
            && self.size_z == other.size_z
            && self.data == other.data
    }
}

impl Eq for VoxelGrid {}

impl Default for VoxelGrid {
    fn default() -> Self {
        Self::new(40, 40, 40)
    }
}

impl VoxelGrid {
    pub fn new(size_x: u32, size_y: u32, size_z: u32) -> Self {
        let sx = size_x.clamp(1, MAX_GRID_DIM);
        let sy = size_y.clamp(1, MAX_GRID_DIM);
        let sz = size_z.clamp(1, MAX_GRID_DIM);
        Self {
            size_x: sx,
            size_y: sy,
            size_z: sz,
            data: vec![0; (sx * sy * sz) as usize],
            revision: next_grid_revision(),
        }
    }

    #[inline]
    pub fn dimensions(&self) -> UVec3 {
        UVec3::new(self.size_x, self.size_y, self.size_z)
    }

    #[inline]
    pub fn size_x(&self) -> u32 {
        self.size_x
    }

    #[inline]
    pub fn size_y(&self) -> u32 {
        self.size_y
    }

    #[inline]
    pub fn size_z(&self) -> u32 {
        self.size_z
    }

    #[inline]
    /// Content revision id (see field doc). Cheap change detection for renderers.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn raw_data(&self) -> &[u8] {
        &self.data
    }

    #[inline]
    pub fn raw_data_mut(&mut self) -> &mut [u8] {
        self.revision = next_grid_revision();
        &mut self.data
    }

    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0
            && y >= 0
            && z >= 0
            && (x as u32) < self.size_x
            && (y as u32) < self.size_y
            && (z as u32) < self.size_z
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x < self.size_x && y < self.size_y && z < self.size_z {
            Some((x + y * self.size_x + z * self.size_x * self.size_y) as usize)
        } else {
            None
        }
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32, z: u32) -> u8 {
        self.index(x, y, z).map(|i| self.data[i]).unwrap_or(0)
    }

    #[inline]
    pub fn get_i32(&self, x: i32, y: i32, z: i32) -> u8 {
        if self.in_bounds(x, y, z) {
            self.get(x as u32, y as u32, z as u32)
        } else {
            0
        }
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, z: u32, color: u8) {
        self.revision = next_grid_revision();
        if let Some(i) = self.index(x, y, z) {
            self.data[i] = color;
        }
    }

    #[inline]
    pub fn set_i32(&mut self, x: i32, y: i32, z: i32, color: u8) {
        self.revision = next_grid_revision();
        if self.in_bounds(x, y, z) {
            self.set(x as u32, y as u32, z as u32, color);
        }
    }

    pub fn count_solid(&self) -> usize {
        self.data.iter().filter(|&&v| v != 0).count()
    }

    pub fn used_colors(&self) -> [bool; 256] {
        let mut used = [false; 256];
        for &v in &self.data {
            used[v as usize] = true;
        }
        used
    }

    pub fn color_histogram(&self) -> [u32; 256] {
        let mut counts = [0u32; 256];
        for &v in &self.data {
            counts[v as usize] += 1;
        }
        counts
    }

    /// Check if a solid voxel at (x, y, z) has all 6 neighbors also solid (interior/invisible)
    #[inline]
    pub fn is_interior_solid(&self, x: i32, y: i32, z: i32) -> bool {
        if self.get_i32(x, y, z) == 0 {
            return false;
        }
        self.get_i32(x - 1, y, z) != 0
            && self.get_i32(x + 1, y, z) != 0
            && self.get_i32(x, y - 1, z) != 0
            && self.get_i32(x, y + 1, z) != 0
            && self.get_i32(x, y, z - 1) != 0
            && self.get_i32(x, y, z + 1) != 0
    }

    /// `zero`: clear all voxels
    pub fn clear(&mut self) {
        self.revision = next_grid_revision();
        self.data.fill(0);
    }

    /// `full`: fill entire volume with `color`
    pub fn full(&mut self, color: u8) {
        self.revision = next_grid_revision();
        self.data.fill(color);
    }

    /// `fill`: paint all existing solid voxels with `color`
    pub fn fill_solid(&mut self, color: u8) {
        self.revision = next_grid_revision();
        for v in &mut self.data {
            if *v != 0 {
                *v = color;
            }
        }
    }

    /// `inv`: invert solid and empty space using `color` for newly created voxels
    pub fn invert(&mut self, color: u8) {
        self.revision = next_grid_revision();
        let fill_color = if color == 0 { 1 } else { color };
        for v in &mut self.data {
            *v = if *v == 0 { fill_color } else { 0 };
        }
    }

    /// `size [x y z]`: resize volume while preserving existing voxels anchored at (0,0,0)
    pub fn resize(&mut self, new_x: u32, new_y: u32, new_z: u32) {
        self.revision = next_grid_revision();
        let nx = new_x.clamp(1, MAX_GRID_DIM);
        let ny = new_y.clamp(1, MAX_GRID_DIM);
        let nz = new_z.clamp(1, MAX_GRID_DIM);
        let mut next = Self::new(nx, ny, nz);
        let min_x = self.size_x.min(nx);
        let min_y = self.size_y.min(ny);
        let min_z = self.size_z.min(nz);
        for z in 0..min_z {
            for y in 0..min_y {
                for x in 0..min_x {
                    next.set(x, y, z, self.get(x, y, z));
                }
            }
        }
        *self = next;
    }

    /// `shrink`: fit model size tightly around solid voxels
    pub fn shrink(&mut self) -> bool {
        self.revision = next_grid_revision();
        let mut min_b = UVec3::new(self.size_x, self.size_y, self.size_z);
        let mut max_b = UVec3::ZERO;
        let mut found = false;

        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    if self.get(x, y, z) != 0 {
                        found = true;
                        min_b = min_b.min(UVec3::new(x, y, z));
                        max_b = max_b.max(UVec3::new(x, y, z));
                    }
                }
            }
        }

        if !found {
            return false;
        }

        let nx = (max_b.x - min_b.x + 1).max(1);
        let ny = (max_b.y - min_b.y + 1).max(1);
        let nz = (max_b.z - min_b.z + 1).max(1);
        let mut next = Self::new(nx, ny, nz);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    next.set(x, y, z, self.get(min_b.x + x, min_b.y + y, min_b.z + z));
                }
            }
        }
        *self = next;
        true
    }

    /// `flip [axis]`: flip model along specified axes
    pub fn flip(&mut self, axes: AxisFlags) {
        self.revision = next_grid_revision();
        let prev = self.clone();
        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    let sx = if axes.x { self.size_x - 1 - x } else { x };
                    let sy = if axes.y { self.size_y - 1 - y } else { y };
                    let sz = if axes.z { self.size_z - 1 - z } else { z };
                    self.set(x, y, z, prev.get(sx, sy, sz));
                }
            }
        }
    }

    /// `loop [axis] [offset]`: toroidal wrap-around shift
    pub fn loop_shift(&mut self, delta: IVec3) {
        self.revision = next_grid_revision();
        let prev = self.clone();
        let sx = self.size_x as i32;
        let sy = self.size_y as i32;
        let sz = self.size_z as i32;
        for z in 0..sz {
            for y in 0..sy {
                for x in 0..sx {
                    let src_x = (x - delta.x).rem_euclid(sx) as u32;
                    let src_y = (y - delta.y).rem_euclid(sy) as u32;
                    let src_z = (z - delta.z).rem_euclid(sz) as u32;
                    self.set(x as u32, y as u32, z as u32, prev.get(src_x, src_y, src_z));
                }
            }
        }
    }

    /// `scale [xyz] [factor]` & `x2`: scale volume dimensions and nearest-neighbor sample voxels
    pub fn scale(&mut self, factors: Vec3) {
        self.revision = next_grid_revision();
        let fx = factors.x.max(0.05);
        let fy = factors.y.max(0.05);
        let fz = factors.z.max(0.05);
        let nx = ((self.size_x as f32 * fx).round() as u32).clamp(1, MAX_GRID_DIM);
        let ny = ((self.size_y as f32 * fy).round() as u32).clamp(1, MAX_GRID_DIM);
        let nz = ((self.size_z as f32 * fz).round() as u32).clamp(1, MAX_GRID_DIM);

        let mut next = Self::new(nx, ny, nz);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let sx = (((x as f32 + 0.5) / nx as f32) * self.size_x as f32)
                        .floor()
                        .clamp(0.0, (self.size_x - 1) as f32) as u32;
                    let sy = (((y as f32 + 0.5) / ny as f32) * self.size_y as f32)
                        .floor()
                        .clamp(0.0, (self.size_y - 1) as f32) as u32;
                    let sz = (((z as f32 + 0.5) / nz as f32) * self.size_z as f32)
                        .floor()
                        .clamp(0.0, (self.size_z - 1) as f32) as u32;
                    next.set(x, y, z, self.get(sx, sy, sz));
                }
            }
        }
        *self = next;
    }

    /// `rot90 [axis]`: rotate volume 90 degrees counter-clockwise around X, Y, or Z
    pub fn rot90(&mut self, axis: Axis) {
        self.revision = next_grid_revision();
        let prev = self.clone();
        match axis {
            Axis::Z => {
                let mut next = Self::new(prev.size_y, prev.size_x, prev.size_z);
                for z in 0..prev.size_z {
                    for y in 0..prev.size_y {
                        for x in 0..prev.size_x {
                            next.set(prev.size_y - 1 - y, x, z, prev.get(x, y, z));
                        }
                    }
                }
                *self = next;
            }
            Axis::X => {
                let mut next = Self::new(prev.size_x, prev.size_z, prev.size_y);
                for z in 0..prev.size_z {
                    for y in 0..prev.size_y {
                        for x in 0..prev.size_x {
                            next.set(x, prev.size_z - 1 - z, y, prev.get(x, y, z));
                        }
                    }
                }
                *self = next;
            }
            Axis::Y => {
                let mut next = Self::new(prev.size_z, prev.size_y, prev.size_x);
                for z in 0..prev.size_z {
                    for y in 0..prev.size_y {
                        for x in 0..prev.size_x {
                            next.set(z, y, prev.size_x - 1 - x, prev.get(x, y, z));
                        }
                    }
                }
                *self = next;
            }
        }
    }

    /// `rot [axis] [degree]`: rotate model around volume center by arbitrary angle in degrees
    pub fn rotate_degrees(&mut self, axis: Axis, degrees: f32) {
        self.revision = next_grid_revision();
        let rem = degrees.rem_euclid(360.0);
        if (rem - 90.0).abs() < 1e-3 {
            self.rot90(axis);
            return;
        }
        if (rem - 180.0).abs() < 1e-3 {
            self.rot90(axis);
            self.rot90(axis);
            return;
        }
        if (rem - 270.0).abs() < 1e-3 {
            self.rot90(axis);
            self.rot90(axis);
            self.rot90(axis);
            return;
        }

        let prev = self.clone();
        let rad = -degrees.to_radians();
        let (sin_a, cos_a) = rad.sin_cos();
        let center = Vec3::new(
            prev.size_x as f32 * 0.5,
            prev.size_y as f32 * 0.5,
            prev.size_z as f32 * 0.5,
        );
        self.clear();
        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5) - center;
                    let sp = match axis {
                        Axis::X => Vec3::new(
                            p.x,
                            p.y * cos_a - p.z * sin_a,
                            p.y * sin_a + p.z * cos_a,
                        ),
                        Axis::Y => Vec3::new(
                            p.x * cos_a + p.z * sin_a,
                            p.y,
                            -p.x * sin_a + p.z * cos_a,
                        ),
                        Axis::Z => Vec3::new(
                            p.x * cos_a - p.y * sin_a,
                            p.x * sin_a + p.y * cos_a,
                            p.z,
                        ),
                    } + center;
                    let sx = sp.x.floor() as i32;
                    let sy = sp.y.floor() as i32;
                    let sz = sp.z.floor() as i32;
                    self.set(x, y, z, prev.get_i32(sx, sy, sz));
                }
            }
        }
    }

    /// `repeat [xyz] [factor]`: tile/repeat model (negative value for mirror repeat)
    pub fn repeat(&mut self, factors: Vec3) {
        self.revision = next_grid_revision();
        let prev = self.clone();
        let rx = factors.x.abs().max(0.1);
        let ry = factors.y.abs().max(0.1);
        let rz = factors.z.abs().max(0.1);
        let mx = factors.x < 0.0;
        let my = factors.y < 0.0;
        let mz = factors.z < 0.0;

        let nx = ((prev.size_x as f32 * rx).round() as u32).clamp(1, MAX_GRID_DIM);
        let ny = ((prev.size_y as f32 * ry).round() as u32).clamp(1, MAX_GRID_DIM);
        let nz = ((prev.size_z as f32 * rz).round() as u32).clamp(1, MAX_GRID_DIM);
        let mut next = Self::new(nx, ny, nz);

        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let map_coord = |coord: u32, orig: u32, mirror: bool| -> u32 {
                        let tile = coord / orig;
                        let local = coord % orig;
                        if mirror && (tile % 2 == 1) {
                            orig - 1 - local
                        } else {
                            local
                        }
                    };
                    let sx = map_coord(x, prev.size_x, mx);
                    let sy = map_coord(y, prev.size_y, my);
                    let sz = map_coord(z, prev.size_z, mz);
                    next.set(x, y, z, prev.get(sx, sy, sz));
                }
            }
        }
        *self = next;
    }

    /// `mir [axis]`: mirror symmetry across specified axes
    pub fn mirror_symmetry(&mut self, axes: AxisFlags) {
        self.revision = next_grid_revision();
        if axes.x {
            for z in 0..self.size_z {
                for y in 0..self.size_y {
                    for x in 0..self.size_x {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(self.size_x - 1 - x, y, z, v);
                        }
                    }
                }
            }
        }
        if axes.y {
            for z in 0..self.size_z {
                for y in 0..self.size_y {
                    for x in 0..self.size_x {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(x, self.size_y - 1 - y, z, v);
                        }
                    }
                }
            }
        }
        if axes.z {
            for z in 0..self.size_z {
                for y in 0..self.size_y {
                    for x in 0..self.size_x {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(x, y, self.size_z - 1 - z, v);
                        }
                    }
                }
            }
        }
    }

    /// `dia [axis]`: diagonal symmetry across the plane perpendicular to axis
    pub fn diagonal_symmetry(&mut self, axes: AxisFlags) {
        self.revision = next_grid_revision();
        if axes.z {
            let limit = self.size_x.min(self.size_y);
            for z in 0..self.size_z {
                for y in 0..limit {
                    for x in 0..limit {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(y, x, z, v);
                        }
                    }
                }
            }
        }
        if axes.x {
            let limit = self.size_y.min(self.size_z);
            for z in 0..limit {
                for y in 0..limit {
                    for x in 0..self.size_x {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(x, z, y, v);
                        }
                    }
                }
            }
        }
        if axes.y {
            let limit = self.size_x.min(self.size_z);
            for z in 0..limit {
                for y in 0..self.size_y {
                    for x in 0..limit {
                        let v = self.get(x, y, z);
                        if v != 0 {
                            self.set(z, y, x, v);
                        }
                    }
                }
            }
        }
    }

    /// Parse directional spec used by `dil` and `ero` (e.g., `+z`, `-yx`, `xyz`)
    pub fn parse_direction_offsets(spec: &str) -> Vec<IVec3> {
        if spec.trim().is_empty() {
            return vec![
                IVec3::X,
                IVec3::NEG_X,
                IVec3::Y,
                IVec3::NEG_Y,
                IVec3::Z,
                IVec3::NEG_Z,
            ];
        }
        let mut dirs = Vec::new();
        let mut sign: i32 = 0; // 0 = both, 1 = positive, -1 = negative
        for ch in spec.chars() {
            match ch {
                '+' => sign = 1,
                '-' => sign = -1,
                'x' | 'X' | 'y' | 'Y' | 'z' | 'Z' => {
                    let unit = match ch.to_ascii_lowercase() {
                        'x' => IVec3::X,
                        'y' => IVec3::Y,
                        _ => IVec3::Z,
                    };
                    match sign {
                        1 => dirs.push(unit),
                        -1 => dirs.push(-unit),
                        _ => {
                            dirs.push(unit);
                            dirs.push(-unit);
                        }
                    }
                }
                _ => {}
            }
        }
        if dirs.is_empty() {
            vec![
                IVec3::X,
                IVec3::NEG_X,
                IVec3::Y,
                IVec3::NEG_Y,
                IVec3::Z,
                IVec3::NEG_Z,
            ]
        } else {
            dirs
        }
    }

    /// `dil [axis]`: morphological dilation along specified directions
    pub fn dilate(&mut self, spec: &str) {
        self.revision = next_grid_revision();
        let dirs = Self::parse_direction_offsets(spec);
        let prev = self.clone();
        let sx = self.size_x as i32;
        let sy = self.size_y as i32;
        let sz = self.size_z as i32;
        for z in 0..sz {
            for y in 0..sy {
                for x in 0..sx {
                    if prev.get_i32(x, y, z) == 0 {
                        for &d in &dirs {
                            let neighbor = prev.get_i32(x - d.x, y - d.y, z - d.z);
                            if neighbor != 0 {
                                self.set_i32(x, y, z, neighbor);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `ero [axis]`: morphological erosion along specified directions
    pub fn erode(&mut self, spec: &str) {
        self.revision = next_grid_revision();
        let dirs = Self::parse_direction_offsets(spec);
        let prev = self.clone();
        let sx = self.size_x as i32;
        let sy = self.size_y as i32;
        let sz = self.size_z as i32;
        for z in 0..sz {
            for y in 0..sy {
                for x in 0..sx {
                    if prev.get_i32(x, y, z) != 0 {
                        for &d in &dirs {
                            if prev.get_i32(x + d.x, y + d.y, z + d.z) == 0 {
                                self.set_i32(x, y, z, 0);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `flood [colorIndex]`:
    /// - if `color_index == 0`, hollow out the volume by removing all interior invisible voxels
    /// - otherwise, fill all interior invisible cavities with `color_index`
    pub fn flood(&mut self, color_index: u8) {
        self.revision = next_grid_revision();
        if color_index == 0 {
            let prev = self.clone();
            for z in 1..self.size_z.saturating_sub(1) {
                for y in 1..self.size_y.saturating_sub(1) {
                    for x in 1..self.size_x.saturating_sub(1) {
                        if prev.is_interior_solid(x as i32, y as i32, z as i32) {
                            self.set(x, y, z, 0);
                        }
                    }
                }
            }
        } else {
            // Flood-fill outside air from boundary, then fill unvisited empty voxels with `color_index`
            let len = self.data.len();
            let mut outside = vec![false; len];
            let mut queue = std::collections::VecDeque::new();

            for z in 0..self.size_z {
                for y in 0..self.size_y {
                    for x in 0..self.size_x {
                        let on_border = x == 0
                            || y == 0
                            || z == 0
                            || x == self.size_x - 1
                            || y == self.size_y - 1
                            || z == self.size_z - 1;
                        if on_border && self.get(x, y, z) == 0 {
                            let idx = self.index(x, y, z).unwrap();
                            outside[idx] = true;
                            queue.push_back(IVec3::new(x as i32, y as i32, z as i32));
                        }
                    }
                }
            }

            let neighbors = [
                IVec3::X,
                IVec3::NEG_X,
                IVec3::Y,
                IVec3::NEG_Y,
                IVec3::Z,
                IVec3::NEG_Z,
            ];
            while let Some(p) = queue.pop_front() {
                for d in neighbors {
                    let np = p + d;
                    if self.in_bounds(np.x, np.y, np.z) {
                        let idx = self.index(np.x as u32, np.y as u32, np.z as u32).unwrap();
                        if !outside[idx] && self.data[idx] == 0 {
                            outside[idx] = true;
                            queue.push_back(np);
                        }
                    }
                }
            }

            for (i, v) in self.data.iter_mut().enumerate() {
                if *v == 0 && !outside[i] {
                    *v = color_index;
                }
            }
        }
    }

    /// `noise [seed scale min max]`: 3D value noise volume generation
    pub fn generate_noise(&mut self, seed: u32, scale: f32, min_val: f32, max_val: f32, base_color: u8) {
        self.revision = next_grid_revision();
        let c = if base_color == 0 { 1 } else { base_color };
        let scale = scale.max(0.001);
        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    let n = value_noise_3d(
                        x as f32 * scale,
                        y as f32 * scale,
                        z as f32 * scale,
                        seed,
                    );
                    if n >= min_val && n <= max_val {
                        let offset = ((n - min_val) / (max_val - min_val).max(0.01) * 7.0) as u8;
                        self.set(x, y, z, c.saturating_add(offset).max(1));
                    } else {
                        self.set(x, y, z, 0);
                    }
                }
            }
        }
    }

    /// `rand [min max]`: paint solid voxels with random color index between `min` and `max`
    pub fn rand_paint(&mut self, min_idx: u8, max_idx: u8, seed: u32) {
        self.revision = next_grid_revision();
        let lo = min_idx.min(max_idx).max(1);
        let hi = min_idx.max(max_idx).max(1);
        let span = (hi - lo) as u32 + 1;
        for (i, v) in self.data.iter_mut().enumerate() {
            if *v != 0 {
                let h = hash_u32(seed.wrapping_add(i as u32));
                *v = lo + (h % span) as u8;
            }
        }
    }

    /// `maze [edge_length]`: generate 3D maze / architectural grid structure
    pub fn generate_maze(&mut self, edge: u32, color: u8) {
        self.revision = next_grid_revision();
        let step = edge.max(2);
        let c = if color == 0 { 1 } else { color };
        self.clear();
        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    let cell_x = x / step;
                    let cell_y = y / step;
                    let cell_z = z / step;
                    let lx = x % step;
                    let ly = y % step;
                    let lz = z % step;

                    let h = hash_u32(
                        cell_x
                            .wrapping_mul(73856093)
                            ^ cell_y.wrapping_mul(19349663)
                            ^ cell_z.wrapping_mul(83492791),
                    );
                    let open_dir = h % 3;
                    let is_pillar = (lx == 0 && ly == 0)
                        || (ly == 0 && lz == 0)
                        || (lx == 0 && lz == 0);
                    let is_wall = (open_dir == 0 && lx == 0)
                        || (open_dir == 1 && ly == 0)
                        || (open_dir == 2 && lz == 0);
                    if is_pillar || is_wall || z == 0 {
                        self.set(x, y, z, c);
                    }
                }
            }
        }
    }

    /// `shear [axis] [s1] [s2]`: shear model along axis
    pub fn shear(&mut self, axis: Axis, s1: f32, s2: f32) {
        self.revision = next_grid_revision();
        let prev = self.clone();
        self.clear();
        let cx = self.size_x as f32 * 0.5;
        let cy = self.size_y as f32 * 0.5;
        let cz = self.size_z as f32 * 0.5;
        for z in 0..self.size_z {
            for y in 0..self.size_y {
                for x in 0..self.size_x {
                    let (sx, sy, sz) = match axis {
                        Axis::Z => (
                            (x as f32 - (z as f32 - cz) * s1).round() as i32,
                            (y as f32 - (z as f32 - cz) * s2).round() as i32,
                            z as i32,
                        ),
                        Axis::X => (
                            x as i32,
                            (y as f32 - (x as f32 - cx) * s1).round() as i32,
                            (z as f32 - (x as f32 - cx) * s2).round() as i32,
                        ),
                        Axis::Y => (
                            (x as f32 - (y as f32 - cy) * s1).round() as i32,
                            y as i32,
                            (z as f32 - (y as f32 - cy) * s2).round() as i32,
                        ),
                    };
                    self.set(x, y, z, prev.get_i32(sx, sy, sz));
                }
            }
        }
    }

    /// Build a rich default MagicaVoxel-style monument/castle showcase model
    pub fn sample_monument() -> Self {
        let mut grid = Self::new(40, 40, 40);
        // Base terraced island
        for z in 0..6 {
            let margin = z as i32;
            let color = if z < 4 { 150 } else { 109 }; // stone + grass top
            for y in (2 + margin)..(38 - margin) {
                for x in (2 + margin)..(38 - margin) {
                    grid.set_i32(x, y, z, color);
                }
            }
        }
        // Central tower
        for z in 6..28 {
            for y in 14..26 {
                for x in 14..26 {
                    let on_wall = x == 14 || x == 25 || y == 14 || y == 25;
                    let is_window = (z >= 14 && z <= 18) && ((x >= 18 && x <= 21) || (y >= 18 && y <= 21));
                    let is_door = z < 11 && x >= 18 && x <= 21 && y == 14;
                    if on_wall && !is_window && !is_door {
                        grid.set(x, y, z, if (z + x + y) % 5 == 0 { 43 } else { 37 });
                    }
                }
            }
        }
        // glowing crystal inside tower
        for z in 12..20 {
            for y in 18..22 {
                for x in 18..22 {
                    grid.set(x, y, z, 19);
                }
            }
        }
        // Stepped pyramid roof
        for step in 0..7 {
            let z = 28 + step;
            let lo = 13 + step;
            let hi = 27 - step;
            for y in lo..hi {
                for x in lo..hi {
                    grid.set(x, y, z, 217);
                }
            }
        }
        // Corner pillars / battlements
        for &(cx, cy) in &[(10u32, 10u32), (28, 10), (10, 28), (28, 28)] {
            for z in 6..16 {
                for dy in 0..3 {
                    for dx in 0..3 {
                        grid.set(cx + dx, cy + dy, z, 79);
                    }
                }
            }
        }
        grid
    }

    /// Pattern preset: Architectural stone archway
    pub fn sample_arch(color: u8) -> Self {
        let mut grid = Self::new(16, 6, 16);
        let col = color.max(1);
        for z in 0..16 {
            for y in 0..6 {
                for x in 0..16 {
                    let is_leg = (x < 3 || x >= 13) && z < 11;
                    let dx = (x as f32 - 7.5).abs();
                    let dz = z as f32 - 10.0;
                    let dist = (dx * dx + dz * dz).sqrt();
                    let is_arch = dz >= 0.0 && dist >= 4.5 && dist <= 7.5;
                    if is_leg || is_arch {
                        grid.set(x, y, z, col);
                    }
                }
            }
        }
        grid
    }

    /// Pattern preset: Classical round column with base and capital
    pub fn sample_column(color: u8) -> Self {
        let mut grid = Self::new(8, 8, 20);
        let col = color.max(1);
        for z in 0..20 {
            let is_cap = z < 2 || z >= 18;
            for y in 0..8 {
                for x in 0..8 {
                    let dx = x as f32 - 3.5;
                    let dy = y as f32 - 3.5;
                    if is_cap || (dx * dx + dy * dy).sqrt() <= 3.2 {
                        grid.set(x, y, z, col);
                    }
                }
            }
        }
        grid
    }

    /// Pattern preset: Flight of 16 steps
    pub fn sample_stairs(color: u8) -> Self {
        let mut grid = Self::new(16, 16, 16);
        let col = color.max(1);
        for step in 0..16 {
            for z in 0..=step {
                for y in 0..16 {
                    grid.set(step, y, z, col);
                }
            }
        }
        grid
    }

    /// Pattern preset: Crenellated defensive wall section
    pub fn sample_wall(color: u8) -> Self {
        let mut grid = Self::new(20, 4, 12);
        let col = color.max(1);
        for x in 0..20 {
            for y in 0..4 {
                for z in 0..12 {
                    let is_crenel = z == 11 && (x / 3) % 2 == 1;
                    if !is_crenel {
                        grid.set(x, y, z, col);
                    }
                }
            }
        }
        grid
    }

    /// Pattern preset: Stylized tree with wood trunk and foliage
    pub fn sample_tree() -> Self {
        let mut grid = Self::new(14, 14, 22);
        let wood = 16;
        let foliage = 112;
        for z in 0..10 {
            for y in 5..9 {
                for x in 5..9 {
                    grid.set(x, y, z, wood);
                }
            }
        }
        for z in 8..22 {
            for y in 0..14 {
                for x in 0..14 {
                    let dx = x as f32 - 6.5;
                    let dy = y as f32 - 6.5;
                    let dz = z as f32 - 15.0;
                    if (dx * dx + dy * dy + dz * dz).sqrt() <= 6.2 {
                        grid.set(x, y, z, foliage);
                    }
                }
            }
        }
        grid
    }
}

#[inline]
pub fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

fn hash_coords(ix: i32, iy: i32, iz: i32, seed: u32) -> f32 {
    let h = hash_u32(
        (ix as u32)
            .wrapping_mul(374761393)
            .wrapping_add((iy as u32).wrapping_mul(668265263))
            .wrapping_add((iz as u32).wrapping_mul(2147483647))
            .wrapping_add(seed.wrapping_mul(1664525)),
    );
    (h & 0x00FF_FFFF) as f32 / 16_777_215.0
}

pub fn value_noise_3d(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let fz = z - iz as f32;

    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let uz = fz * fz * (3.0 - 2.0 * fz);

    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;

    let c000 = hash_coords(ix, iy, iz, seed);
    let c100 = hash_coords(ix + 1, iy, iz, seed);
    let c010 = hash_coords(ix, iy + 1, iz, seed);
    let c110 = hash_coords(ix + 1, iy + 1, iz, seed);
    let c001 = hash_coords(ix, iy, iz + 1, seed);
    let c101 = hash_coords(ix + 1, iy, iz + 1, seed);
    let c011 = hash_coords(ix, iy + 1, iz + 1, seed);
    let c111 = hash_coords(ix + 1, iy + 1, iz + 1, seed);

    let x00 = lerp(c000, c100, ux);
    let x10 = lerp(c010, c110, ux);
    let x01 = lerp(c001, c101, ux);
    let x11 = lerp(c011, c111, ux);

    let y0 = lerp(x00, x10, uy);
    let y1 = lerp(x01, x11, uy);
    lerp(y0, y1, uz)
}

