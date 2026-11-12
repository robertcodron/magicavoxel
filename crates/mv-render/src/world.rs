//! World Editor viewport: CPU ray casting of every visible shape of the scene.
//!
//! Each shape is traced in its own model space (the camera ray is transformed by the
//! inverse placement, which is exact because rotations are signed permutations), so
//! instances share voxel data and the scene is not limited to a single 256³ grid.
//! A world-space AABB test culls shapes before the per-model DDA traversal.

use glam::{IVec3, Vec2, Vec3};
use mv_core::{EditorProject, MaterialType, NodeId, Rgba, Rotation, VoxelGrid};
use rayon::prelude::*;

use crate::{
    bounding_frame_wire_t, compute_voxel_ao, dda_raycast_grid, dda_raycast_grid_shaped,
    intersect_aabb_with_normal, voxel_face_edge_distance, DisplaySettings, OrbitCamera, VoxelShape,
};

const MAX_RAY_DIST: f32 = 4000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Highlight {
    #[default]
    None,
    Hovered,
    Selected,
}

/// One visible shape placed in the world.
pub struct WorldInstance<'a> {
    pub node: NodeId,
    pub grid: &'a VoxelGrid,
    pub(crate) rotation: Rotation,
    pub(crate) inverse: Rotation,
    /// `world = rotation * p + offset` for continuous model-space points `p`.
    pub(crate) offset: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub highlight: Highlight,
    /// Drawn faded (outside the group being edited).
    pub dimmed: bool,
}

impl WorldInstance<'_> {
    fn to_local(&self, p: Vec3) -> Vec3 {
        self.inverse.apply_f32(p - self.offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldHit {
    /// Index into [`WorldView::instances`].
    pub instance: usize,
    pub node: NodeId,
    /// Voxel coordinates inside the shape's model.
    pub voxel: IVec3,
    pub normal_local: IVec3,
    pub normal: IVec3,
    pub normal_vec: Vec3,
    pub hit_pos: Vec3,
    pub hit_pos_local: Vec3,
    pub t: f32,
}

/// Flat Bounding Volume Hierarchy node for spatial acceleration.
#[derive(Clone, Copy, Debug)]
pub struct BvhNode {
    pub min: Vec3,
    pub max: Vec3,
    /// If count > 0, leaf node: start index in `instance_indices`.
    /// If count == 0, internal node: left child index.
    pub left_or_first: u32,
    /// Right child index for internal nodes (0 for leaves).
    pub right: u32,
    pub count: u32,
}

/// Acceleration structure for world instance ray casting.
#[derive(Clone, Debug, Default)]
pub struct WorldBvh {
    pub nodes: Vec<BvhNode>,
    pub instance_indices: Vec<usize>,
}

impl WorldBvh {
    pub fn build(instances: &[WorldInstance]) -> Self {
        if instances.is_empty() {
            return Self::default();
        }
        let mut instance_indices: Vec<usize> = (0..instances.len()).collect();
        let mut nodes = Vec::with_capacity(instances.len() * 2);
        Self::build_recursive(instances, &mut instance_indices, 0, instances.len(), &mut nodes);
        Self {
            nodes,
            instance_indices,
        }
    }

    fn build_recursive(
        instances: &[WorldInstance],
        indices: &mut [usize],
        start: usize,
        end: usize,
        nodes: &mut Vec<BvhNode>,
    ) -> u32 {
        let node_idx = nodes.len() as u32;
        nodes.push(BvhNode {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
            left_or_first: 0,
            right: 0,
            count: 0,
        });

        let count = end - start;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &idx in &indices[start..end] {
            min = min.min(instances[idx].min);
            max = max.max(instances[idx].max);
        }

        if count <= 4 {
            nodes[node_idx as usize] = BvhNode {
                min,
                max,
                left_or_first: start as u32,
                right: 0,
                count: count as u32,
            };
            return node_idx;
        }

        let mut c_min = Vec3::splat(f32::INFINITY);
        let mut c_max = Vec3::splat(f32::NEG_INFINITY);
        for &idx in &indices[start..end] {
            let center = (instances[idx].min + instances[idx].max) * 0.5;
            c_min = c_min.min(center);
            c_max = c_max.max(center);
        }

        let extent = c_max - c_min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };

        let mid_split = (c_min[axis] + c_max[axis]) * 0.5;
        let mut mid = start;
        for i in start..end {
            let idx = indices[i];
            let center = (instances[idx].min[axis] + instances[idx].max[axis]) * 0.5;
            if center < mid_split {
                indices.swap(i, mid);
                mid += 1;
            }
        }

        if mid == start || mid == end {
            mid = start + count / 2;
        }

        let left = Self::build_recursive(instances, indices, start, mid, nodes);
        let right = Self::build_recursive(instances, indices, mid, end, nodes);

        nodes[node_idx as usize] = BvhNode {
            min,
            max,
            left_or_first: left,
            right,
            count: 0,
        };
        node_idx
    }
}

pub struct WorldView<'a> {
    pub instances: Vec<WorldInstance<'a>>,
    pub bvh: WorldBvh,
    /// World boxes outlined on top of the scene (one per selected object or group).
    pub frames: Vec<(Vec3, Vec3)>,
}

impl<'a> WorldView<'a> {
    /// Collect the visible shapes of the project's scene. `style` gives each shape's
    /// highlight and whether it is dimmed.
    pub fn build(
        project: &'a EditorProject,
        mut style: impl FnMut(NodeId) -> (Highlight, bool),
    ) -> Self {
        let scene = &project.scene;
        let mut instances = Vec::new();
        for shape in scene.shapes_under(scene.root()) {
            if !scene.is_visible(shape) {
                continue;
            }
            let Some(grid) = scene.shape_model(shape).and_then(|m| project.model(m)) else {
                continue;
            };
            let Ok(xf) = scene.shape_voxel_transform(shape) else {
                continue;
            };
            let rotation = xf.rotation;
            // A negated axis maps the voxel cell [v, v+1) to (-v-1, -v]: shift by one so
            // the cell of voxel v lands on the cell of the integer world voxel `xf(v)`.
            let flipped = rotation.apply_f32(Vec3::ONE);
            let offset = xf.translation.as_vec3()
                + Vec3::new(
                    (flipped.x < 0.0) as i32 as f32,
                    (flipped.y < 0.0) as i32 as f32,
                    (flipped.z < 0.0) as i32 as f32,
                );
            let a = offset;
            let b = rotation.apply_f32(grid.dimensions().as_vec3()) + offset;
            let (highlight, dimmed) = style(shape);
            instances.push(WorldInstance {
                node: shape,
                grid,
                rotation,
                inverse: rotation.inverse(),
                offset,
                min: a.min(b),
                max: a.max(b),
                highlight,
                dimmed,
            });
        }
        let bvh = WorldBvh::build(&instances);
        Self {
            instances,
            bvh,
            frames: Vec::new(),
        }
    }

    /// Union of the instances' world boxes.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.instances.iter().fold(None, |acc, i| {
            Some(match acc {
                Some((lo, hi)) => (i.min.min(lo), i.max.max(hi)),
                None => (i.min, i.max),
            })
        })
    }

    /// Nearest voxel hit along a world ray (accelerated by BVH).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<WorldHit> {
        self.raycast_shaped(origin, dir, max_t, VoxelShape::Cube)
    }

    /// Nearest voxel hit along a world ray with specified voxel shape (accelerated by BVH).
    pub fn raycast_shaped(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_t: f32,
        shape: VoxelShape,
    ) -> Option<WorldHit> {
        if self.bvh.nodes.is_empty() {
            return None;
        }

        let mut best: Option<WorldHit> = None;
        let mut stack = [0u32; 64];
        let mut stack_ptr = 1;
        stack[0] = 0;

        while stack_ptr > 0 {
            stack_ptr -= 1;
            let node_idx = stack[stack_ptr] as usize;
            let node = &self.bvh.nodes[node_idx];

            let best_t = best.as_ref().map_or(max_t, |b| b.t);
            let Some((t_enter, t_exit, _)) =
                intersect_aabb_with_normal(origin, dir, node.min, node.max)
            else {
                continue;
            };
            if t_exit < 0.0 || t_enter > best_t {
                continue;
            }

            if node.count > 0 {
                let start = node.left_or_first as usize;
                let end = start + node.count as usize;
                for &idx in &self.bvh.instance_indices[start..end] {
                    let inst = &self.instances[idx];
                    let curr_best_t = best.as_ref().map_or(max_t, |b| b.t);
                    let Some((inst_enter, inst_exit, _)) =
                        intersect_aabb_with_normal(origin, dir, inst.min, inst.max)
                    else {
                        continue;
                    };
                    if inst_exit < 0.0 || inst_enter > curr_best_t {
                        continue;
                    }
                    let lo = inst.to_local(origin);
                    let ld = inst.inverse.apply_f32(dir);
                    if let Some(hit) = dda_raycast_grid_shaped(inst.grid, lo, ld, curr_best_t, shape) {
                        if hit.t < curr_best_t {
                            best = Some(WorldHit {
                                instance: idx,
                                node: inst.node,
                                voxel: hit.voxel,
                                normal_local: hit.normal,
                                normal: inst.rotation.apply(hit.normal),
                                normal_vec: inst.rotation.apply_f32(hit.normal_vec),
                                hit_pos: origin + dir * hit.t,
                                hit_pos_local: hit.hit_pos,
                                t: hit.t,
                            });
                        }
                    }
                }
            } else {
                let left = node.left_or_first;
                let right = node.right;
                if stack_ptr + 2 < stack.len() {
                    stack[stack_ptr] = right;
                    stack_ptr += 1;
                    stack[stack_ptr] = left;
                    stack_ptr += 1;
                }
            }
        }
        best
    }

    /// Whether anything blocks the ray before `max_t` (shadow rays, accelerated by BVH).
    pub fn occluded(&self, origin: Vec3, dir: Vec3, max_t: f32) -> bool {
        if self.bvh.nodes.is_empty() {
            return false;
        }

        let mut stack = [0u32; 64];
        let mut stack_ptr = 1;
        stack[0] = 0;

        while stack_ptr > 0 {
            stack_ptr -= 1;
            let node_idx = stack[stack_ptr] as usize;
            let node = &self.bvh.nodes[node_idx];

            let Some((t_enter, t_exit, _)) =
                intersect_aabb_with_normal(origin, dir, node.min, node.max)
            else {
                continue;
            };
            if t_exit < 0.0 || t_enter > max_t {
                continue;
            }

            if node.count > 0 {
                let start = node.left_or_first as usize;
                let end = start + node.count as usize;
                for &idx in &self.bvh.instance_indices[start..end] {
                    let inst = &self.instances[idx];
                    if let Some((i_enter, i_exit, _)) =
                        intersect_aabb_with_normal(origin, dir, inst.min, inst.max)
                    {
                        if i_exit >= 0.0 && i_enter <= max_t {
                            let lo = inst.to_local(origin);
                            let ld = inst.inverse.apply_f32(dir);
                            if dda_raycast_grid(inst.grid, lo, ld, max_t).is_some() {
                                return true;
                            }
                        }
                    }
                }
            } else {
                let left = node.left_or_first;
                let right = node.right;
                if stack_ptr + 2 < stack.len() {
                    stack[stack_ptr] = right;
                    stack_ptr += 1;
                    stack[stack_ptr] = left;
                    stack_ptr += 1;
                }
            }
        }
        false
    }
}

/// World ray under a viewport position (`ndc` in `[-1, 1]²`).
pub fn pick_world(
    view: &WorldView,
    camera: &OrbitCamera,
    ndc: Vec2,
    aspect: f32,
) -> Option<WorldHit> {
    pick_world_shaped(view, camera, ndc, aspect, VoxelShape::Cube)
}

/// World ray under a viewport position with configurable voxel shape.
pub fn pick_world_shaped(
    view: &WorldView,
    camera: &OrbitCamera,
    ndc: Vec2,
    aspect: f32,
    shape: VoxelShape,
) -> Option<WorldHit> {
    let (o, d) = camera.ray_for_ndc(ndc, aspect);
    view.raycast_shaped(o, d, MAX_RAY_DIST, shape)
}

/// Render the World Editor viewport into `out_rgba` (`width * height * 4`).
pub fn render_world_rgba(
    project: &EditorProject,
    view: &WorldView,
    camera: &OrbitCamera,
    display: &DisplaySettings,
    width: usize,
    height: usize,
    out_rgba: &mut Vec<u8>,
) {
    out_rgba.resize(width * height * 4, 0);
    if width == 0 || height == 0 {
        return;
    }
    let aspect = width as f32 / height as f32;
    let sun_dir = display.sun_direction();
    let ground_range = (camera.distance * 4.0).max(300.0);

    out_rgba
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(py, row)| {
            let ndc_y = 1.0 - ((py as f32 + 0.5) / height as f32) * 2.0;
            for px in 0..width {
                let ndc_x = ((px as f32 + 0.5) / width as f32) * 2.0 - 1.0;
                let (o, d) = camera.ray_for_ndc(Vec2::new(ndc_x, ndc_y), aspect);
                let c = shade_world_ray(project, view, display, sun_dir, ground_range, o, d);
                row[px * 4..px * 4 + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
            }
        });
}

fn shade_world_ray(
    project: &EditorProject,
    view: &WorldView,
    display: &DisplaySettings,
    sun_dir: Vec3,
    ground_range: f32,
    o: Vec3,
    d: Vec3,
) -> Rgba {
    let mut rgb = if display.show_background {
        let t = (d.z * 0.5 + 0.5).clamp(0.0, 1.0);
        Vec3::new(0.14, 0.15, 0.17).lerp(Vec3::new(0.22, 0.25, 0.30), t)
    } else {
        Vec3::splat(0.12)
    };

    let hit = view.raycast_shaped(o, d, MAX_RAY_DIST, display.voxel_shape);
    let ground_t = if (display.show_ground || display.show_grid) && d.z.abs() > 1e-5 {
        let t = -o.z / d.z;
        (t > 0.0 && t < ground_range).then_some(t)
    } else {
        None
    };

    if let Some(h) = hit {
        let inst = &view.instances[h.instance];
        let c_idx = inst.grid.get_i32(h.voxel.x, h.voxel.y, h.voxel.z) as usize;
        let pal = project.palette.colors[c_idx];
        let mat = project.materials[c_idx];
        let mut base = Vec3::new(pal.r as f32, pal.g as f32, pal.b as f32) / 255.0;

        let n = h.normal_vec;
        let ndotl = n.dot(sun_dir).max(0.0);
        let shadow = if display.show_shadows
            && ndotl > 0.0
            && view.occluded(h.hit_pos + n * 0.02, sun_dir, 512.0)
        {
            0.42
        } else {
            1.0
        };
        let ao = if display.enable_ao {
            compute_voxel_ao(inst.grid, h.voxel, h.normal_local, h.hit_pos_local)
        } else {
            1.0
        };

        let hemi = 0.38 + 0.14 * (n.z * 0.5 + 0.5);
        let diffuse = hemi * ao + ndotl * 0.62 * shadow;
        let half = (sun_dir - d).normalize_or_zero();
        let (spec_power, spec_strength) = match mat.kind {
            MaterialType::Metal => ((2.0 / mat.rough.max(0.05).powi(2)).clamp(8.0, 256.0), 0.65),
            MaterialType::Glass => (96.0, 0.45),
            _ => (24.0, 0.06),
        };
        let spec = n.dot(half).max(0.0).powf(spec_power) * spec_strength * shadow;
        let emit = if mat.kind == MaterialType::Emit || mat.emit > 0.01 {
            1.0 + mat.emit.max(0.8) * 1.2
        } else {
            1.0
        };
        base = base * (diffuse * emit) + Vec3::splat(spec);

        // Point lights evaluation in World Editor
        let view_dir = -d;
        for light in &project.lights {
            let to_light = light.position - h.hit_pos;
            let dist = to_light.length();
            if dist < light.radius && dist > 1e-4 {
                let l_dir = to_light / dist;
                let l_ndotl = n.dot(l_dir).max(0.0);
                if l_ndotl > 0.0 {
                    let occluded = display.show_shadows && view.occluded(h.hit_pos + n * 0.02, l_dir, dist - 0.05);
                    if !occluded {
                        let atten = light.intensity / (1.0 + dist * dist);
                        let cutoff = (1.0 - dist / light.radius).clamp(0.0, 1.0);
                        let smooth_cutoff = cutoff * cutoff * (3.0 - 2.0 * cutoff);
                        let l_col = Vec3::from_array(light.color);
                        let factor = atten * smooth_cutoff * l_ndotl;
                        let l_half = (l_dir + view_dir).normalize_or_zero();
                        let l_spec = n.dot(l_half).max(0.0).powf(spec_power) * spec_strength * (atten * smooth_cutoff);
                        base += (base * l_col) * factor + l_col * l_spec;
                    }
                }
            }
        }

        if display.show_edges || display.show_wireframe {
            let threshold = if display.show_wireframe { 0.09 } else { 0.045 };
            if voxel_face_edge_distance(h.hit_pos_local, h.normal_local) < threshold {
                base *= 0.72;
            }
        }
        match inst.highlight {
            Highlight::Selected => base = base.lerp(Vec3::new(1.0, 0.62, 0.2), 0.22),
            Highlight::Hovered => base = base.lerp(Vec3::ONE, 0.12),
            Highlight::None => {}
        }
        if inst.dimmed {
            base = base.lerp(Vec3::splat(base.dot(Vec3::splat(1.0 / 3.0))), 0.7) * 0.55;
        }
        rgb = base;
    } else if let Some(gt) = ground_t {
        let gp = o + d * gt;
        let fade = (1.0 - gt / ground_range).clamp(0.0, 1.0);
        if display.show_ground {
            let mut g = Vec3::new(0.18, 0.20, 0.23);
            if display.show_shadows && view.occluded(gp + Vec3::Z * 0.02, sun_dir, 512.0) {
                g *= 0.62;
            }

            // Point lights illumination on ground
            for light in &project.lights {
                let to_light = light.position - gp;
                let dist = to_light.length();
                if dist < light.radius && dist > 1e-4 {
                    let l_dir = to_light / dist;
                    let l_ndotl = l_dir.z.max(0.0);
                    if l_ndotl > 0.0 {
                        let occluded = display.show_shadows && view.occluded(gp + Vec3::Z * 0.02, l_dir, dist - 0.05);
                        if !occluded {
                            let atten = light.intensity / (1.0 + dist * dist);
                            let cutoff = (1.0 - dist / light.radius).clamp(0.0, 1.0);
                            let smooth_cutoff = cutoff * cutoff * (3.0 - 2.0 * cutoff);
                            let l_col = Vec3::from_array(light.color);
                            g += g * l_col * (atten * smooth_cutoff * l_ndotl);
                        }
                    }
                }
            }
            if display.show_grid {
                let line = |v: f32, period: f32, w: f32| {
                    let f = v.rem_euclid(period);
                    f < w || f > period - w
                };
                if line(gp.x, 16.0, 0.12) || line(gp.y, 16.0, 0.12) {
                    g = g.lerp(Vec3::new(0.42, 0.47, 0.55), 0.5);
                } else if gt < 160.0 && (line(gp.x, 1.0, 0.04) || line(gp.y, 1.0, 0.04)) {
                    g = g.lerp(Vec3::new(0.28, 0.32, 0.38), 0.3 * (1.0 - gt / 160.0));
                }
                // World axes through the origin.
                if gp.y.abs() < 0.15 {
                    g = g.lerp(Vec3::new(0.85, 0.3, 0.3), 0.6);
                } else if gp.x.abs() < 0.15 {
                    g = g.lerp(Vec3::new(0.3, 0.8, 0.35), 0.6);
                }
            }
            rgb = rgb.lerp(g, fade);
        }
    }

    // Bounding frames of selected objects, drawn on top.
    if display.show_frame {
        let limit = hit
            .map_or(f32::INFINITY, |h| h.t)
            .min(ground_t.unwrap_or(f32::INFINITY));
        for &(min, max) in &view.frames {
            if let Some(ft) = bounding_frame_wire_t(o, d, min, max) {
                let color = Vec3::new(0.95, 0.68, 0.20);
                rgb = rgb.lerp(color, if ft <= limit + 0.1 { 0.85 } else { 0.35 });
            }
        }
    }

    Rgba::new(
        (rgb.x.clamp(0.0, 1.0) * 255.0) as u8,
        (rgb.y.clamp(0.0, 1.0) * 255.0) as u8,
        (rgb.z.clamp(0.0, 1.0) * 255.0) as u8,
        255,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mv_core::{Axis, Transform};

    /// Brute-force reference: world voxel set of every visible shape.
    fn world_voxels(project: &EditorProject) -> std::collections::HashSet<IVec3> {
        let scene = &project.scene;
        let mut out = std::collections::HashSet::new();
        for s in scene.shapes_under(scene.root()) {
            let xf = scene.shape_voxel_transform(s).unwrap();
            let grid = &scene.models[scene.node(s).unwrap().model_id().unwrap()];
            let d = grid.dimensions();
            for z in 0..d.z {
                for y in 0..d.y {
                    for x in 0..d.x {
                        if grid.get(x, y, z) != 0 {
                            out.insert(xf.apply(IVec3::new(x as i32, y as i32, z as i32)));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn raycast_matches_voxel_placement_for_all_rotations() {
        // An asymmetric model so every rotation places voxels differently.
        let mut grid = VoxelGrid::new(3, 4, 5);
        grid.set(0, 0, 0, 1);
        grid.set(2, 3, 4, 2);
        grid.set(1, 0, 3, 3);

        for byte in 0u8..128 {
            let r = Rotation::from_vox_byte(byte);
            if r.to_vox_byte() != byte {
                continue;
            }
            let mut project = EditorProject::new_empty(1, 1, 1);
            project.scene = mv_core::Scene::new();
            let m = project.scene.add_model(grid.clone());
            let root = project.scene.root();
            project
                .scene
                .add_shape(
                    root,
                    m,
                    Transform {
                        rotation: r,
                        translation: IVec3::new(7, -3, 11),
                    },
                )
                .unwrap();
            project.active_model = usize::MAX; // read models from the scene
            let expected = world_voxels(&project);
            let view = WorldView::build(&project, |_| (Highlight::None, false));

            for v in &expected {
                // Shoot straight down onto the voxel's center from above everything.
                let o = v.as_vec3() + Vec3::new(0.5, 0.5, 100.0);
                let hit = view
                    .raycast(o, -Vec3::Z, 1000.0)
                    .unwrap_or_else(|| panic!("rotation {byte}: missed voxel {v}"));
                let top = expected
                    .iter()
                    .filter(|w| w.x == v.x && w.y == v.y)
                    .map(|w| w.z)
                    .max()
                    .unwrap();
                let world_cell = (hit.hit_pos - Vec3::Z * 0.5).floor().as_ivec3();
                assert_eq!(world_cell, IVec3::new(v.x, v.y, top), "rotation {byte}");
                assert_eq!(hit.normal, IVec3::Z, "rotation {byte}");
            }
        }
    }

    #[test]
    fn nearest_instance_wins_and_hidden_shapes_are_skipped() {
        let mut project = EditorProject::new_empty(1, 1, 1);
        project.scene = mv_core::Scene::new();
        let mut cube = VoxelGrid::new(2, 2, 2);
        cube.full(5);
        let m = project.scene.add_model(cube);
        project.active_model = usize::MAX;
        let root = project.scene.root();
        let near = project
            .scene
            .add_shape(root, m, Transform::from_translation(IVec3::new(0, 0, 10)))
            .unwrap();
        let far = project.scene.instantiate(near).unwrap();
        project
            .scene
            .translate_world(far, IVec3::new(0, 0, -10))
            .unwrap();
        project.scene.rotate90(far, Axis::Z).unwrap();

        let view = WorldView::build(&project, |_| (Highlight::None, false));
        let hit = view
            .raycast(Vec3::new(0.5, 0.5, 50.0), -Vec3::Z, 1000.0)
            .unwrap();
        assert_eq!(hit.node, near);
        assert!(view.occluded(Vec3::new(0.5, 0.5, 5.0), -Vec3::Z, 100.0));

        project.scene.node_mut(near).unwrap().hidden = true;
        let view = WorldView::build(&project, |_| (Highlight::None, false));
        let hit = view
            .raycast(Vec3::new(0.5, 0.5, 50.0), -Vec3::Z, 1000.0)
            .unwrap();
        assert_eq!(hit.node, far);
        assert_eq!(view.instances.len(), 1);

        let mut rgba = Vec::new();
        let mut cam = OrbitCamera::default();
        let (lo, hi) = view.bounds().unwrap();
        cam.frame_box(lo, hi);
        render_world_rgba(
            &project,
            &view,
            &cam,
            &DisplaySettings::default(),
            24,
            16,
            &mut rgba,
        );
        assert_eq!(rgba.len(), 24 * 16 * 4);
    }

    #[test]
    fn bvh_many_instances_accel() {
        let mut project = EditorProject::new_empty(1, 1, 1);
        project.scene = mv_core::Scene::new();
        let mut cube = VoxelGrid::new(2, 2, 2);
        cube.full(3);
        let m = project.scene.add_model(cube);
        project.active_model = usize::MAX;
        let root = project.scene.root();

        // Create a 5x5 grid of instances (25 instances)
        let mut nodes = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                let id = project
                    .scene
                    .add_shape(
                        root,
                        m,
                        Transform::from_translation(IVec3::new(x * 10, y * 10, 0)),
                    )
                    .unwrap();
                nodes.push(id);
            }
        }

        let view = WorldView::build(&project, |_| (Highlight::None, false));
        assert_eq!(view.instances.len(), 25);
        assert!(!view.bvh.nodes.is_empty());

        // Raycast over instance (2, 3)
        let target_x = 20.5;
        let target_y = 30.5;
        let hit = view
            .raycast(Vec3::new(target_x, target_y, 50.0), -Vec3::Z, 100.0)
            .unwrap();
        assert_eq!(hit.node, nodes[3 * 5 + 2]);
        assert_eq!(hit.normal, IVec3::Z);
    }
}
