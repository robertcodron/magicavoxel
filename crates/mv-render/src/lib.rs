pub mod gpu_pathtracer;
pub mod world;

use std::collections::HashSet;

use glam::{IVec3, Vec2, Vec3};
use mv_core::{EditorProject, MaterialType, Rgba, VoxelGrid};
use rayon::prelude::*;

pub use gpu_pathtracer::{PathTracerEngine, PathTracerSettings};
pub use world::{pick_world, render_world_rgba, Highlight, WorldHit, WorldView};

#[derive(Debug, Clone, Copy)]
pub struct DisplaySettings {
    pub show_edges: bool,      // CTRL+E
    pub show_grid: bool,       // CTRL+G
    pub show_frame: bool,      // CTRL+F
    pub show_ground: bool,     // CTRL+U
    pub show_background: bool, // CTRL+B
    pub show_shadows: bool,    // CTRL+W
    pub show_wireframe: bool,  // CTRL+M
    pub enable_ao: bool,
    pub sun_azimuth_deg: f32,
    pub sun_elevation_deg: f32,
    pub render_scale: u32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            show_edges: true,
            show_grid: true,
            show_frame: true,
            show_ground: true,
            show_background: true,
            show_shadows: true,
            show_wireframe: false,
            enable_ao: true,
            sun_azimuth_deg: 48.0,
            sun_elevation_deg: 58.0,
            render_scale: 1,
        }
    }
}

impl DisplaySettings {
    pub fn sun_direction(&self) -> Vec3 {
        let az = self.sun_azimuth_deg.to_radians();
        let el = self.sun_elevation_deg.to_radians();
        Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin()).normalize()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CameraPose {
    pub target: Vec3,
    pub distance: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub fov_deg: f32,
}

#[derive(Debug, Clone)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub fov_deg: f32,
    pub active_slot: usize,
    pub saved_slots: [CameraPose; 10],
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let default_pose = CameraPose {
            target: Vec3::new(20.0, 20.0, 14.0),
            distance: 72.0,
            yaw_deg: -135.0,
            pitch_deg: 30.0,
            fov_deg: 45.0,
        };
        Self {
            target: default_pose.target,
            distance: default_pose.distance,
            yaw_deg: default_pose.yaw_deg,
            pitch_deg: default_pose.pitch_deg,
            fov_deg: default_pose.fov_deg,
            active_slot: 0,
            saved_slots: [default_pose; 10],
        }
    }
}

impl OrbitCamera {
    pub fn recenter(&mut self, grid: &VoxelGrid) {
        let d = grid.dimensions().as_vec3();
        self.frame_box(Vec3::ZERO, d);
    }

    /// Aim at a box and back off so it fits the view (World Editor `F4`).
    pub fn frame_box(&mut self, min: Vec3, max: Vec3) {
        let size = max - min;
        self.target = Vec3::new(
            (min.x + max.x) * 0.5,
            (min.y + max.y) * 0.5,
            min.z + size.z * 0.4,
        );
        self.distance = (size.max_element() * 1.85).max(16.0);
    }

    /// `F5 / 5`: snap camera to nearest 90-degree / axial view
    pub fn snap_to_90_view(&mut self) {
        self.yaw_deg = (self.yaw_deg / 90.0).round() * 90.0;
        self.pitch_deg = ((self.pitch_deg / 45.0).round() * 45.0).clamp(-89.0, 89.0);
    }

    pub fn save_slot(&mut self, slot: usize) {
        let s = slot.min(9);
        self.active_slot = s;
        self.saved_slots[s] = CameraPose {
            target: self.target,
            distance: self.distance,
            yaw_deg: self.yaw_deg,
            pitch_deg: self.pitch_deg,
            fov_deg: self.fov_deg,
        };
    }

    pub fn load_slot(&mut self, slot: usize) {
        let s = slot.min(9);
        self.active_slot = s;
        let pose = self.saved_slots[s];
        self.target = pose.target;
        self.distance = pose.distance;
        self.yaw_deg = pose.yaw_deg;
        self.pitch_deg = pose.pitch_deg;
        self.fov_deg = pose.fov_deg;
    }

    pub fn basis(&self) -> (Vec3, Vec3, Vec3, Vec3) {
        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.clamp(-89.5, 89.5).to_radians();

        // Z-up spherical coordinates
        let offset = Vec3::new(
            pitch.cos() * yaw.cos(),
            pitch.cos() * yaw.sin(),
            pitch.sin(),
        ) * self.distance;
        let eye = self.target + offset;
        let forward = (self.target - eye).normalize_or_zero();
        let right = forward.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        (eye, forward, right, up)
    }

    pub fn ray_for_ndc(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let (eye, forward, right, up) = self.basis();
        let half_tan = (self.fov_deg.to_radians() * 0.5).tan();
        let dir = (forward + right * (ndc.x * aspect * half_tan) + up * (ndc.y * half_tan))
            .normalize();
        (eye, dir)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub voxel: IVec3,
    pub normal: IVec3,
    pub hit_pos: Vec3,
    pub t: f32,
    pub is_ground: bool,
}

/// Exact 3D DDA Ray-Picker against the VoxelGrid and optional `z = 0` ground plane
pub fn pick_voxel(
    grid: &VoxelGrid,
    ray_origin: Vec3,
    ray_dir: Vec3,
    allow_ground: bool,
) -> Option<RayHit> {
    if let Some(hit) = dda_raycast_grid(grid, ray_origin, ray_dir, 2048.0) {
        return Some(hit);
    }
    if allow_ground && ray_dir.z.abs() > 1e-5 {
        let t = -ray_origin.z / ray_dir.z;
        if t > 0.0 {
            let p = ray_origin + ray_dir * t;
            let gx = p.x.floor() as i32;
            let gy = p.y.floor() as i32;
            if gx >= 0
                && gy >= 0
                && (gx as u32) < grid.size_x()
                && (gy as u32) < grid.size_y()
            {
                return Some(RayHit {
                    voxel: IVec3::new(gx, gy, -1),
                    normal: IVec3::Z,
                    hit_pos: p,
                    t,
                    is_ground: true,
                });
            }
        }
    }
    None
}

/// Fast 3D Amanatides & Woo DDA ray traversal through `[0..size_x, 0..size_y, 0..size_z]`
pub fn dda_raycast_grid(
    grid: &VoxelGrid,
    ray_origin: Vec3,
    ray_dir: Vec3,
    max_dist: f32,
) -> Option<RayHit> {
    let bmin = Vec3::ZERO;
    let bmax = Vec3::new(
        grid.size_x() as f32,
        grid.size_y() as f32,
        grid.size_z() as f32,
    );

    let (t_enter, t_exit, entry_normal) = intersect_aabb_with_normal(ray_origin, ray_dir, bmin, bmax)?;
    if t_enter > max_dist || t_exit < 0.0 {
        return None;
    }

    let t_start = t_enter.max(0.0);
    let start_pos = ray_origin + ray_dir * (t_start + 1e-4);

    let mut map_pos = IVec3::new(
        (start_pos.x.floor() as i32).clamp(0, grid.size_x() as i32 - 1),
        (start_pos.y.floor() as i32).clamp(0, grid.size_y() as i32 - 1),
        (start_pos.z.floor() as i32).clamp(0, grid.size_z() as i32 - 1),
    );

    let step = IVec3::new(
        if ray_dir.x >= 0.0 { 1 } else { -1 },
        if ray_dir.y >= 0.0 { 1 } else { -1 },
        if ray_dir.z >= 0.0 { 1 } else { -1 },
    );

    let inv_dir = Vec3::new(
        if ray_dir.x.abs() > 1e-6 { 1.0 / ray_dir.x.abs() } else { 1e30 },
        if ray_dir.y.abs() > 1e-6 { 1.0 / ray_dir.y.abs() } else { 1e30 },
        if ray_dir.z.abs() > 1e-6 { 1.0 / ray_dir.z.abs() } else { 1e30 },
    );

    let next_boundary = Vec3::new(
        if step.x > 0 { map_pos.x as f32 + 1.0 } else { map_pos.x as f32 },
        if step.y > 0 { map_pos.y as f32 + 1.0 } else { map_pos.y as f32 },
        if step.z > 0 { map_pos.z as f32 + 1.0 } else { map_pos.z as f32 },
    );

    let mut t_max = Vec3::new(
        t_start + (next_boundary.x - start_pos.x).abs() * inv_dir.x,
        t_start + (next_boundary.y - start_pos.y).abs() * inv_dir.y,
        t_start + (next_boundary.z - start_pos.z).abs() * inv_dir.z,
    );
    let t_delta = inv_dir;

    let mut normal = entry_normal;
    let mut t_cur = t_start;

    for _ in 0..1024 {
        if !grid.in_bounds(map_pos.x, map_pos.y, map_pos.z) || t_cur > max_dist {
            break;
        }
        let c = grid.get(map_pos.x as u32, map_pos.y as u32, map_pos.z as u32);
        if c != 0 {
            return Some(RayHit {
                voxel: map_pos,
                normal,
                hit_pos: ray_origin + ray_dir * t_cur,
                t: t_cur,
                is_ground: false,
            });
        }

        if t_max.x < t_max.y {
            if t_max.x < t_max.z {
                map_pos.x += step.x;
                t_cur = t_max.x;
                t_max.x += t_delta.x;
                normal = IVec3::new(-step.x, 0, 0);
            } else {
                map_pos.z += step.z;
                t_cur = t_max.z;
                t_max.z += t_delta.z;
                normal = IVec3::new(0, 0, -step.z);
            }
        } else if t_max.y < t_max.z {
            map_pos.y += step.y;
            t_cur = t_max.y;
            t_max.y += t_delta.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            map_pos.z += step.z;
            t_cur = t_max.z;
            t_max.z += t_delta.z;
            normal = IVec3::new(0, 0, -step.z);
        }
    }
    None
}

/// Render the scene using parallel 3D DDA Ray-Casting into an RGBA buffer (`width * height * 4`)
pub fn render_viewport_rgba(
    project: &EditorProject,
    camera: &OrbitCamera,
    display: &DisplaySettings,
    preview_voxels: &HashSet<IVec3>,
    width: usize,
    height: usize,
    out_rgba: &mut Vec<u8>,
) {
    let total_pixels = width * height;
    out_rgba.resize(total_pixels * 4, 0);
    if width == 0 || height == 0 {
        return;
    }

    let aspect = width as f32 / height as f32;
    let sun_dir = display.sun_direction();
    let grid = &project.grid;
    let bmax = Vec3::new(
        grid.size_x() as f32,
        grid.size_y() as f32,
        grid.size_z() as f32,
    );

    out_rgba
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(py, row)| {
            let ndc_y = 1.0 - ((py as f32 + 0.5) / height as f32) * 2.0;
            for px in 0..width {
                let ndc_x = ((px as f32 + 0.5) / width as f32) * 2.0 - 1.0;
                let (ray_o, ray_d) = camera.ray_for_ndc(Vec2::new(ndc_x, ndc_y), aspect);
                let color = shade_ray(
                    project,
                    display,
                    preview_voxels,
                    bmax,
                    sun_dir,
                    ray_o,
                    ray_d,
                );
                let base = px * 4;
                row[base] = color.r;
                row[base + 1] = color.g;
                row[base + 2] = color.b;
                row[base + 3] = 255;
            }
        });
}

fn shade_ray(
    project: &EditorProject,
    display: &DisplaySettings,
    preview_voxels: &HashSet<IVec3>,
    bmax: Vec3,
    sun_dir: Vec3,
    ray_o: Vec3,
    ray_d: Vec3,
) -> Rgba {
    let grid = &project.grid;

    // Background sky gradient
    let mut final_rgb = if display.show_background {
        let t = (ray_d.z * 0.5 + 0.5).clamp(0.0, 1.0);
        Vec3::new(0.14, 0.15, 0.17).lerp(Vec3::new(0.22, 0.25, 0.30), t)
    } else {
        Vec3::splat(0.12)
    };

    // Check bounding frame wireframe hit distance
    let frame_t = if display.show_frame {
        bounding_frame_wire_t(ray_o, ray_d, Vec3::ZERO, bmax)
    } else {
        None
    };

    let voxel_hit = dda_raycast_grid(grid, ray_o, ray_d, 1500.0);

    // Ground plane intersection at z = 0
    let ground_t = if (display.show_ground || display.show_grid) && ray_d.z.abs() > 1e-5 {
        let t = -ray_o.z / ray_d.z;
        if t > 0.0 && t < 600.0 {
            Some(t)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(hit) = voxel_hit {
        let c_idx = grid.get(hit.voxel.x as u32, hit.voxel.y as u32, hit.voxel.z as u32);
        let pal_col = project.palette.colors[c_idx as usize];
        let mat = project.materials[c_idx as usize];

        let mut base_col = Vec3::new(
            pal_col.r as f32 / 255.0,
            pal_col.g as f32 / 255.0,
            pal_col.b as f32 / 255.0,
        );

        // Selection & Brush preview tint
        let is_selected = project
            .selection
            .get(hit.voxel.x as u32, hit.voxel.y as u32, hit.voxel.z as u32);
        let is_preview = preview_voxels.contains(&hit.voxel)
            || preview_voxels.contains(&(hit.voxel + hit.normal));

        let n_vec = Vec3::new(hit.normal.x as f32, hit.normal.y as f32, hit.normal.z as f32);
        let ndotl = n_vec.dot(sun_dir).max(0.0);

        // Shadow ray
        let shadow = if display.show_shadows && ndotl > 0.0 {
            let shadow_o = hit.hit_pos + n_vec * 0.02;
            if dda_raycast_grid(grid, shadow_o, sun_dir, 256.0).is_some() {
                0.42
            } else {
                1.0
            }
        } else {
            1.0
        };

        // Analytical Voxel Ambient Occlusion
        let ao = if display.enable_ao {
            compute_voxel_ao(grid, hit.voxel, hit.normal, hit.hit_pos)
        } else {
            1.0
        };

        // Hemisphere ambient + directional sun + GGX-like specular for metal/plastic
        let hemi = 0.38 + 0.14 * (n_vec.z * 0.5 + 0.5);
        let diffuse = hemi * ao + ndotl * 0.62 * shadow;

        let view_dir = -ray_d;
        let half_vec = (sun_dir + view_dir).normalize_or_zero();
        let spec_power = match mat.kind {
            MaterialType::Metal => (2.0 / (mat.rough.max(0.05).powi(2))).clamp(8.0, 256.0),
            MaterialType::Glass => 96.0,
            _ => 24.0,
        };
        let spec_strength = match mat.kind {
            MaterialType::Metal => 0.65 * shadow,
            MaterialType::Glass => 0.45 * shadow,
            _ => 0.06 * shadow,
        };
        let spec = n_vec.dot(half_vec).max(0.0).powf(spec_power) * spec_strength;

        let emit_boost = if mat.kind == MaterialType::Emit || mat.emit > 0.01 {
            1.0 + mat.emit.max(0.8) * 1.2
        } else {
            1.0
        };

        base_col = base_col * (diffuse * emit_boost) + Vec3::splat(spec);

        // Voxel edges (`CTRL+E`) or wireframe (`CTRL+M`)
        if display.show_edges || display.show_wireframe {
            let edge_dist = voxel_face_edge_distance(hit.hit_pos, hit.normal);
            let threshold = if display.show_wireframe { 0.09 } else { 0.045 };
            if edge_dist < threshold {
                base_col *= 0.72;
            }
        }

        if is_selected {
            base_col = base_col.lerp(Vec3::new(1.0, 0.55, 0.15), 0.38);
        }
        if is_preview {
            base_col = base_col.lerp(Vec3::new(0.25, 0.85, 1.0), 0.45);
        }

        final_rgb = base_col;

        if let Some(ft) = frame_t {
            if ft < hit.t {
                final_rgb = final_rgb.lerp(Vec3::new(0.95, 0.68, 0.20), 0.75);
            }
        }
    } else if let Some(gt) = ground_t {
        let gp = ray_o + ray_d * gt;
        let in_box = gp.x >= 0.0 && gp.x <= bmax.x && gp.y >= 0.0 && gp.y <= bmax.y;
        let dist_fade = (1.0 - (gt / 420.0)).clamp(0.0, 1.0);

        if display.show_ground && (in_box || gt < 260.0) {
            let mut gcol = if in_box {
                Vec3::new(0.20, 0.22, 0.25)
            } else {
                Vec3::new(0.16, 0.18, 0.20)
            };

            // Ground shadow from volume
            if display.show_shadows {
                if dda_raycast_grid(grid, gp + Vec3::Z * 0.02, sun_dir, 256.0).is_some() {
                    gcol *= 0.62;
                }
            }

            if display.show_grid {
                let fx = gp.x.rem_euclid(1.0);
                let fy = gp.y.rem_euclid(1.0);
                let line_1 = fx < 0.05 || fx > 0.95 || fy < 0.05 || fy > 0.95;
                let fx8 = gp.x.rem_euclid(8.0);
                let fy8 = gp.y.rem_euclid(8.0);
                let line_8 = fx8 < 0.08 || fx8 > 7.92 || fy8 < 0.08 || fy8 > 7.92;
                if line_8 && in_box {
                    gcol = gcol.lerp(Vec3::new(0.45, 0.50, 0.58), 0.55);
                } else if line_1 && in_box {
                    gcol = gcol.lerp(Vec3::new(0.30, 0.34, 0.40), 0.35);
                }
            }

            // Highlight floor cell when hovering empty canvas
            let cell = IVec3::new(gp.x.floor() as i32, gp.y.floor() as i32, 0);
            if preview_voxels.contains(&cell) {
                gcol = gcol.lerp(Vec3::new(0.25, 0.85, 1.0), 0.55);
            }

            final_rgb = final_rgb.lerp(gcol, dist_fade);
        }

        if let Some(ft) = frame_t {
            if ft <= gt + 0.1 {
                final_rgb = final_rgb.lerp(Vec3::new(0.95, 0.68, 0.20), 0.75);
            }
        }
    } else if frame_t.is_some() {
        final_rgb = final_rgb.lerp(Vec3::new(0.95, 0.68, 0.20), 0.75);
    }

    Rgba::new(
        (final_rgb.x.clamp(0.0, 1.0) * 255.0) as u8,
        (final_rgb.y.clamp(0.0, 1.0) * 255.0) as u8,
        (final_rgb.z.clamp(0.0, 1.0) * 255.0) as u8,
        255,
    )
}

/// Compute smooth analytical Voxel Ambient Occlusion on a hit voxel face
pub(crate) fn compute_voxel_ao(grid: &VoxelGrid, voxel: IVec3, normal: IVec3, hit_pos: Vec3) -> f32 {
    let local = hit_pos - Vec3::new(voxel.x as f32, voxel.y as f32, voxel.z as f32);
    let (u, v, t_u, t_v) = if normal.x != 0 {
        (
            local.y.clamp(0.0, 1.0),
            local.z.clamp(0.0, 1.0),
            IVec3::Y,
            IVec3::Z,
        )
    } else if normal.y != 0 {
        (
            local.x.clamp(0.0, 1.0),
            local.z.clamp(0.0, 1.0),
            IVec3::X,
            IVec3::Z,
        )
    } else {
        (
            local.x.clamp(0.0, 1.0),
            local.y.clamp(0.0, 1.0),
            IVec3::X,
            IVec3::Y,
        )
    };

    let front = voxel + normal;
    let occ = |du: i32, dv: i32| -> f32 {
        let p = front + t_u * du + t_v * dv;
        if grid.get_i32(p.x, p.y, p.z) != 0 {
            1.0
        } else {
            0.0
        }
    };

    let corner_ao = |s1: f32, s2: f32, c: f32| -> f32 {
        if s1 > 0.5 && s2 > 0.5 {
            0.35
        } else {
            1.0 - (s1 + s2 + c) * 0.18
        }
    };

    let l = occ(-1, 0);
    let r = occ(1, 0);
    let b = occ(0, -1);
    let t = occ(0, 1);

    let ao00 = corner_ao(l, b, occ(-1, -1));
    let ao10 = corner_ao(r, b, occ(1, -1));
    let ao01 = corner_ao(l, t, occ(-1, 1));
    let ao11 = corner_ao(r, t, occ(1, 1));

    let ao_b = ao00 + (ao10 - ao00) * u;
    let ao_t = ao01 + (ao11 - ao01) * u;
    ao_b + (ao_t - ao_b) * v
}

pub(crate) fn voxel_face_edge_distance(hit_pos: Vec3, normal: IVec3) -> f32 {
    let fx = hit_pos.x.rem_euclid(1.0);
    let fy = hit_pos.y.rem_euclid(1.0);
    let fz = hit_pos.z.rem_euclid(1.0);
    let dx = fx.min(1.0 - fx);
    let dy = fy.min(1.0 - fy);
    let dz = fz.min(1.0 - fz);

    if normal.x != 0 {
        dy.min(dz)
    } else if normal.y != 0 {
        dx.min(dz)
    } else {
        dx.min(dy)
    }
}

pub(crate) fn intersect_aabb_with_normal(
    origin: Vec3,
    dir: Vec3,
    bmin: Vec3,
    bmax: Vec3,
) -> Option<(f32, f32, IVec3)> {
    let mut tmin = -f32::INFINITY;
    let mut tmax = f32::INFINITY;
    let mut normal = IVec3::Z;

    for axis in 0..3 {
        let o = origin[axis];
        let d = dir[axis];
        let min_a = bmin[axis];
        let max_a = bmax[axis];

        if d.abs() < 1e-7 {
            if o < min_a || o > max_a {
                return None;
            }
        } else {
            let inv_d = 1.0 / d;
            let mut t1 = (min_a - o) * inv_d;
            let mut t2 = (max_a - o) * inv_d;
            let mut n = IVec3::ZERO;
            n[axis] = if d >= 0.0 { -1 } else { 1 };
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            if t1 > tmin {
                tmin = t1;
                normal = n;
            }
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some((tmin, tmax, normal))
}

pub(crate) fn bounding_frame_wire_t(origin: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3) -> Option<f32> {
    let (tmin, tmax, _) = intersect_aabb_with_normal(origin, dir, bmin, bmax)?;
    let t = if tmin > 0.0 { tmin } else { tmax };
    if t <= 0.0 {
        return None;
    }
    let p = origin + dir * t;
    let eps = 0.18;
    let near_x = (p.x - bmin.x).abs() < eps || (p.x - bmax.x).abs() < eps;
    let near_y = (p.y - bmin.y).abs() < eps || (p.y - bmax.y).abs() < eps;
    let near_z = (p.z - bmin.z).abs() < eps || (p.z - bmax.z).abs() < eps;
    let count = near_x as u8 + near_y as u8 + near_z as u8;
    if count >= 2 {
        Some(t)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dda_raycast_and_picking() {
        let mut grid = VoxelGrid::new(10, 10, 10);
        grid.set(5, 5, 5, 3);

        let origin = Vec3::new(5.5, 5.5, 20.0);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let hit = pick_voxel(&grid, origin, dir, false).expect("Should hit voxel at (5,5,5)");
        assert_eq!(hit.voxel, IVec3::new(5, 5, 5));
        assert_eq!(hit.normal, IVec3::Z);
    }

    #[test]
    fn test_pathtracer_engine_progressive_render() {
        let project = EditorProject::default();
        let mut camera = OrbitCamera::default();
        camera.recenter(&project.grid);
        let display = DisplaySettings::default();
        let pt_settings = PathTracerSettings::default();
        let mut engine = PathTracerEngine::new();
        let mut rgba = Vec::new();

        engine.render_progressive(&project, &camera, &display, &pt_settings, 32, 32, &mut rgba);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(engine.current_samples >= 1);
    }
}

