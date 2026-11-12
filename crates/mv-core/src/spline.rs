use glam::{IVec3, Vec3};

use crate::voxel::VoxelGrid;

/// Evaluates a 3D Catmull-Rom spline segment for `t` in `[0.0, 1.0]`.
/// Standard uniform Catmull-Rom formulation with tension tau = 0.5.
pub fn eval_catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Evaluates a 3D cubic Bézier curve segment for `t` in `[0.0, 1.0]`.
pub fn eval_cubic_bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    let u2 = u * u;
    let u3 = u2 * u;
    let t2 = t * t;
    let t3 = t2 * t;
    u3 * p0 + 3.0 * u2 * t * p1 + 3.0 * u * t2 * p2 + t3 * p3
}

/// Samples a multi-point Catmull-Rom spline passing through all control points.
/// Endpoints are clamped for natural tangent extrapolation.
pub fn sample_catmull_rom(points: &[Vec3], step_size: f32) -> Vec<Vec3> {
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0]];
    }
    if points.len() == 2 {
        let p0 = points[0];
        let p1 = points[1];
        let dist = p0.distance(p1);
        let n = ((dist / step_size.max(0.1)).ceil() as usize).max(2);
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            samples.push(p0.lerp(p1, t));
        }
        return samples;
    }

    let n = points.len();
    let step = step_size.max(0.1);
    let mut result = Vec::new();

    for i in 0..n - 1 {
        let p0 = if i == 0 {
            points[0] + (points[0] - points[1])
        } else {
            points[i - 1]
        };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < n {
            points[i + 2]
        } else {
            points[n - 1] + (points[n - 1] - points[n - 2])
        };

        let seg_len = p1.distance(p2);
        let steps = ((seg_len / step).ceil() as usize).max(2);
        let max_k = if i == n - 2 { steps } else { steps - 1 };

        for k in 0..=max_k {
            let t = k as f32 / steps as f32;
            result.push(eval_catmull_rom(p0, p1, p2, p3, t));
        }
    }

    result
}

/// Samples a cubic Bézier curve from `p0` to `p3` with given `step_size`.
pub fn sample_cubic_bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, step_size: f32) -> Vec<Vec3> {
    let approx_len = p0.distance(p1) + p1.distance(p2) + p2.distance(p3);
    let steps = ((approx_len / step_size.max(0.1)).ceil() as usize).max(2);
    let mut result = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        result.push(eval_cubic_bezier(p0, p1, p2, p3, t));
    }
    result
}

/// Voxelizes a continuous wall along 3D spline control points with optional crenelations.
///
/// - `thickness`: width of the wall in voxels (1 = single voxel, 2+ = thick wall).
/// - `height`: height in voxels.
/// - `crenelated`: if true, battlements alternate at the top layer.
/// - `stone_color`: palette index used for the stone wall.
pub fn generate_spline_wall(
    grid: &mut VoxelGrid,
    points: &[Vec3],
    thickness: u32,
    height: u32,
    crenelated: bool,
    stone_color: u8,
) {
    if points.is_empty() || height == 0 || stone_color == 0 {
        return;
    }

    // Step size <= 0.4 ensures continuous 3D rasterization without diagonal holes.
    let samples = sample_catmull_rom(points, 0.4);
    if samples.is_empty() {
        return;
    }

    let radius = if thickness <= 1 {
        0.0f32
    } else {
        (thickness as f32 - 1.0) * 0.5
    };
    let radius_sq = radius * radius;
    let r_ceil = radius.ceil() as i32;

    let mut cumulative_dist = 0.0f32;
    let mut prev_pos = samples[0];

    for pt in &samples {
        cumulative_dist += pt.distance(prev_pos);
        prev_pos = *pt;

        let cx = pt.x.round() as i32;
        let cy = pt.y.round() as i32;
        let base_z = pt.z.round() as i32;

        for dx in -r_ceil..=r_ceil {
            for dy in -r_ceil..=r_ceil {
                if thickness > 1 && (dx * dx + dy * dy) as f32 > radius_sq + 0.15 {
                    continue;
                }
                let vx = cx + dx;
                let vy = cy + dy;

                for dz in 0..height {
                    let vz = base_z + dz as i32;

                    // Crenelation at the top layer: 2-voxel solid merlon, 2-voxel open embrasure
                    if crenelated && dz == height - 1 {
                        let phase = (cumulative_dist.round() as i32).rem_euclid(4);
                        if phase >= 2 {
                            continue;
                        }
                    }

                    grid.set_i32(vx, vy, vz, stone_color);
                }
            }
        }
    }
}

/// Carves a clean vaulted arch with a surrounding stone frame / lintel.
///
/// - `center`: bottom center of the opening.
/// - `width`: span of the opening along X.
/// - `height`: total height of the opening along Z.
/// - `depth`: thickness of the opening along Y.
/// - `frame_color`: palette index for stone border / voussoirs.
pub fn carve_arch(
    grid: &mut VoxelGrid,
    center: IVec3,
    width: u32,
    height: u32,
    depth: u32,
    frame_color: u8,
) {
    if width == 0 || height == 0 || depth == 0 {
        return;
    }

    let w = width as i32;
    let h = height as i32;
    let d = depth as i32;

    let x_min = center.x - w / 2;
    let x_max = x_min + w - 1;
    let y_min = center.y - d / 2;
    let y_max = y_min + d - 1;
    let z_min = center.z;
    let z_max = z_min + h - 1;

    let mid_x = (x_min + x_max) as f32 * 0.5;
    let arch_radius = w as f32 * 0.5;
    let springline_z = (z_max as f32 - arch_radius + 0.5).max(z_min as f32);

    let is_inside_opening = |x: i32, z: i32| -> bool {
        if x < x_min || x > x_max || z < z_min || z > z_max {
            return false;
        }
        let zf = z as f32;
        if zf <= springline_z {
            true
        } else {
            let dx = x as f32 - mid_x;
            let dz = zf - springline_z;
            (dx * dx + dz * dz) <= arch_radius * arch_radius + 0.15
        }
    };

    // 1. Carve the interior of the arch (set to empty 0)
    for x in x_min..=x_max {
        for z in z_min..=z_max {
            if is_inside_opening(x, z) {
                for y in y_min..=y_max {
                    grid.set_i32(x, y, z, 0);
                }
            }
        }
    }

    // 2. Add stone lintel and jamb frames around the arch perimeter
    if frame_color != 0 {
        let frame_x_min = x_min - 1;
        let frame_x_max = x_max + 1;
        let frame_z_max = z_max + 1;

        for x in frame_x_min..=frame_x_max {
            for z in z_min..=frame_z_max {
                if is_inside_opening(x, z) {
                    continue;
                }
                // Check if this voxel is directly adjacent to the opening
                let adjacent_to_opening = is_inside_opening(x - 1, z)
                    || is_inside_opening(x + 1, z)
                    || is_inside_opening(x, z - 1)
                    || is_inside_opening(x, z + 1);

                if adjacent_to_opening {
                    for y in y_min..=y_max {
                        grid.set_i32(x, y, z, frame_color);
                    }
                }
            }
        }
    }
}

/// Generates a wooden fence along 3D spline control points with posts and horizontal rails.
///
/// - `points`: control points defining fence path.
/// - `height`: height of fence posts (typical 2..4 voxels).
/// - `wood_color`: palette index for the fence timber.
pub fn generate_fence(
    grid: &mut VoxelGrid,
    points: &[Vec3],
    height: u32,
    wood_color: u8,
) {
    if points.is_empty() || height == 0 || wood_color == 0 {
        return;
    }

    let samples = sample_catmull_rom(points, 0.5);
    if samples.is_empty() {
        return;
    }

    // Determine post locations: at control points and spaced ~3.5 voxels apart
    let mut post_positions = Vec::new();
    let mut cum_dist = 0.0f32;
    let mut prev_pt = samples[0];
    post_positions.push(samples[0]);

    for pt in &samples[1..] {
        cum_dist += pt.distance(prev_pt);
        prev_pt = *pt;
        if cum_dist >= 3.5 {
            post_positions.push(*pt);
            cum_dist = 0.0;
        }
    }
    if let Some(&last) = samples.last() {
        if post_positions.last().map(|p| p.distance(last) > 1.0).unwrap_or(true) {
            post_positions.push(last);
        }
    }

    // 1. Draw vertical fence posts
    for post in &post_positions {
        let px = post.x.round() as i32;
        let py = post.y.round() as i32;
        let pz = post.z.round() as i32;

        for dz in 0..height {
            grid.set_i32(px, py, pz + dz as i32, wood_color);
        }
        // Small decorative cap on post
        grid.set_i32(px, py, pz + height as i32, wood_color);
    }

    // 2. Draw continuous horizontal rails connecting along the path
    let rail_levels: Vec<i32> = if height <= 2 {
        vec![1]
    } else if height == 3 {
        vec![1, 2]
    } else {
        vec![(height / 3) as i32, ((height * 2) / 3) as i32]
    };

    for pt in &samples {
        let rx = pt.x.round() as i32;
        let ry = pt.y.round() as i32;
        let rz = pt.z.round() as i32;

        for &dz in &rail_levels {
            grid.set_i32(rx, ry, rz + dz, wood_color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catmull_rom_and_bezier_eval() {
        let p0 = Vec3::new(0.0, 0.0, 0.0);
        let p1 = Vec3::new(10.0, 0.0, 0.0);
        let p2 = Vec3::new(10.0, 10.0, 0.0);
        let p3 = Vec3::new(0.0, 10.0, 0.0);

        let cr_start = eval_catmull_rom(p0, p1, p2, p3, 0.0);
        let cr_end = eval_catmull_rom(p0, p1, p2, p3, 1.0);
        assert!((cr_start - p1).length() < 1e-4);
        assert!((cr_end - p2).length() < 1e-4);

        let bez_start = eval_cubic_bezier(p0, p1, p2, p3, 0.0);
        let bez_end = eval_cubic_bezier(p0, p1, p2, p3, 1.0);
        assert!((bez_start - p0).length() < 1e-4);
        assert!((bez_end - p3).length() < 1e-4);
    }

    #[test]
    fn test_spline_wall_generation() {
        let mut grid = VoxelGrid::new(32, 32, 16);
        let points = vec![
            Vec3::new(2.0, 2.0, 1.0),
            Vec3::new(16.0, 8.0, 1.0),
            Vec3::new(28.0, 28.0, 1.0),
        ];

        generate_spline_wall(&mut grid, &points, 2, 5, true, 42);
        assert!(grid.count_solid() > 50);

        // Verify stone color is present
        assert_eq!(grid.get_i32(2, 2, 1), 42);
    }

    #[test]
    fn test_carve_arch() {
        let mut grid = VoxelGrid::new(20, 20, 20);
        grid.full(10);
        let count_before = grid.count_solid();

        carve_arch(&mut grid, IVec3::new(10, 10, 2), 6, 8, 4, 99);

        // Opening must have carved out interior
        assert_eq!(grid.get_i32(10, 10, 2), 0);
        assert_eq!(grid.get_i32(10, 10, 5), 0);

        // Frame must have placed frame_color
        assert!(grid.raw_data().contains(&99));
        assert!(grid.count_solid() < count_before);
    }

    #[test]
    fn test_fence_generation() {
        let mut grid = VoxelGrid::new(24, 24, 16);
        let points = vec![
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(10.0, 5.0, 0.0),
            Vec3::new(20.0, 2.0, 0.0),
        ];

        generate_fence(&mut grid, &points, 3, 15);
        assert!(grid.count_solid() > 20);
        assert_eq!(grid.get_i32(2, 2, 0), 15);
    }
}
