//! High-level 3D geometric voxel drawing primitives for AI modeling (`voxel_box`,
//! `voxel_sphere`, `voxel_cylinder`, `voxel_line`, `voxel_batch`, etc.).
//!
//! All coordinates follow MagicaVoxel's `Z`-up convention:
//! - `X`: right (`0..size_x`)
//! - `Y`: depth / back (`0..size_y`)
//! - `Z`: height / up (`0..size_z`, ground is `z = 0`)

use glam::{IVec3, Vec3};
use mv_core::{Axis, VoxelGrid};

use crate::json::JsonValue;

/// Largest absolute value accepted for any number of a batch operation. Together with
/// the loop clipping below it bounds the cost of an operation by the canvas size.
pub const MAX_COORD: f64 = 100_000.0;
/// Largest thickness of a `line` (cost grows with its cube).
pub const MAX_LINE_RADIUS: i32 = 32;

/// Inclusive iteration range `[lo, hi]` clipped to `0..dim` (empty if outside).
fn clip(lo: i32, hi: i32, dim: u32) -> std::ops::RangeInclusive<i32> {
    lo.max(0)..=hi.min(dim as i32 - 1)
}

/// `true` if the grid cell `(x, y, z)` could ever matter for a shape of the given
/// extent around the canvas; used to skip far-away steps cheaply.
fn near_canvas(grid: &VoxelGrid, p: IVec3, margin: i32) -> bool {
    p.x >= -margin
        && p.y >= -margin
        && p.z >= -margin
        && p.x < grid.size_x() as i32 + margin
        && p.y < grid.size_y() as i32 + margin
        && p.z < grid.size_z() as i32 + margin
}

/// Draw an axis-aligned 3D box (solid or hollow with wall thickness) into `grid`.
/// Returns the number of voxel cells modified.
pub fn draw_box(
    grid: &mut VoxelGrid,
    min: IVec3,
    max: IVec3,
    color: u8,
    hollow: bool,
    wall_thickness: i32,
) -> usize {
    let lo = min.min(max);
    let hi = min.max(max);
    let wall = wall_thickness.max(1);
    let mut count = 0usize;
    for z in clip(lo.z, hi.z, grid.size_z()) {
        for y in clip(lo.y, hi.y, grid.size_y()) {
            for x in clip(lo.x, hi.x, grid.size_x()) {
                if hollow {
                    let on_shell = (x - lo.x) < wall
                        || (hi.x - x) < wall
                        || (y - lo.y) < wall
                        || (hi.y - y) < wall
                        || (z - lo.z) < wall
                        || (hi.z - z) < wall;
                    if !on_shell {
                        continue;
                    }
                }
                grid.set_i32(x, y, z, color);
                count += 1;
            }
        }
    }
    count
}

/// Draw a sphere or ellipsoid centered at `center` with radii `radii = [rx, ry, rz]`.
/// Returns the number of voxel cells modified.
pub fn draw_ellipsoid(
    grid: &mut VoxelGrid,
    center: Vec3,
    radii: Vec3,
    color: u8,
    hollow: bool,
    wall_thickness: f32,
) -> usize {
    let rx = radii.x.max(0.5);
    let ry = radii.y.max(0.5);
    let rz = radii.z.max(0.5);
    let min_x = (center.x - rx).floor() as i32;
    let max_x = (center.x + rx).ceil() as i32;
    let min_y = (center.y - ry).floor() as i32;
    let max_y = (center.y + ry).ceil() as i32;
    let min_z = (center.z - rz).floor() as i32;
    let max_z = (center.z + rz).ceil() as i32;
    let wall = wall_thickness.max(1.0);
    let irx = (rx - wall).max(0.0);
    let iry = (ry - wall).max(0.0);
    let irz = (rz - wall).max(0.0);

    let mut count = 0usize;
    for z in clip(min_z, max_z, grid.size_z()) {
        for y in clip(min_y, max_y, grid.size_y()) {
            for x in clip(min_x, max_x, grid.size_x()) {
                let dx = (x as f32 - center.x) / rx;
                let dy = (y as f32 - center.y) / ry;
                let dz = (z as f32 - center.z) / rz;
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > 1.0 {
                    continue;
                }
                if hollow && irx > 0.0 && iry > 0.0 && irz > 0.0 {
                    let idx = (x as f32 - center.x) / irx;
                    let idy = (y as f32 - center.y) / iry;
                    let idz = (z as f32 - center.z) / irz;
                    if idx * idx + idy * idy + idz * idz < 1.0 {
                        continue;
                    }
                }
                grid.set_i32(x, y, z, color);
                count += 1;
            }
        }
    }
    count
}

/// Draw a cylinder or cone/frustum along `axis`.
/// `base_center` is the center of the bottom cap (lowest coordinate along `axis`),
/// `height` extends in the positive `axis` direction (`height >= 1`).
#[allow(clippy::too_many_arguments)]
pub fn draw_cylinder(
    grid: &mut VoxelGrid,
    base_center: Vec3,
    axis: Axis,
    height: i32,
    radius_bottom: f32,
    radius_top: f32,
    color: u8,
    hollow: bool,
) -> usize {
    let h = height.max(1);
    let rb = radius_bottom.max(0.0);
    let rt = radius_top.max(0.0);
    let max_r = rb.max(rt).ceil() as i32;
    let mut count = 0usize;

    // Only iterate the part of the cylinder that can touch the canvas: component index
    // of the axis (`step`) and of the two cross-section directions (`u`, `v`).
    let b = [
        base_center.x.round() as i32,
        base_center.y.round() as i32,
        base_center.z.round() as i32,
    ];
    let d = [
        grid.size_x() as i32,
        grid.size_y() as i32,
        grid.size_z() as i32,
    ];
    let (ia, iu, iv) = match axis {
        Axis::Z => (2, 0, 1),
        Axis::Y => (1, 0, 2),
        Axis::X => (0, 1, 2),
    };
    let steps = (-b[ia]).max(0)..=(d[ia] - 1 - b[ia]).min(h - 1);
    let urange = (-b[iu]).max(-max_r)..=(d[iu] - 1 - b[iu]).min(max_r);
    let vrange = (-b[iv]).max(-max_r)..=(d[iv] - 1 - b[iv]).min(max_r);

    for step in steps {
        let t = if h <= 1 {
            0.0
        } else {
            step as f32 / (h - 1) as f32
        };
        let r = rb + (rt - rb) * t;
        let r_inner = (r - 1.0).max(0.0);
        for u in urange.clone() {
            for v in vrange.clone() {
                let (x, y, z, du, dv) = match axis {
                    Axis::Z => (
                        (base_center.x.round() as i32) + u,
                        (base_center.y.round() as i32) + v,
                        (base_center.z.round() as i32) + step,
                        u as f32,
                        v as f32,
                    ),
                    Axis::Y => (
                        (base_center.x.round() as i32) + u,
                        (base_center.y.round() as i32) + step,
                        (base_center.z.round() as i32) + v,
                        u as f32,
                        v as f32,
                    ),
                    Axis::X => (
                        (base_center.x.round() as i32) + step,
                        (base_center.y.round() as i32) + u,
                        (base_center.z.round() as i32) + v,
                        u as f32,
                        v as f32,
                    ),
                };
                if !grid.in_bounds(x, y, z) {
                    continue;
                }
                let dist = (du * du + dv * dv).sqrt();
                if dist <= r + 0.25 {
                    let is_cap = step == 0 || step == h - 1;
                    if hollow && !is_cap && dist < r_inner {
                        continue;
                    }
                    grid.set_i32(x, y, z, color);
                    count += 1;
                }
            }
        }
    }
    count
}

/// Draw a 3D pyramid inside the bounding box `[min, max]` tapering up along `axis`.
pub fn draw_pyramid(
    grid: &mut VoxelGrid,
    min: IVec3,
    max: IVec3,
    axis: Axis,
    color: u8,
    hollow: bool,
) -> usize {
    let lo = min.min(max);
    let hi = min.max(max);
    let mut count = 0usize;
    let h = match axis {
        Axis::Z => (hi.z - lo.z + 1).max(1),
        Axis::Y => (hi.y - lo.y + 1).max(1),
        Axis::X => (hi.x - lo.x + 1).max(1),
    };
    for step in 0..h {
        let inset = step / 2;
        let (x0, x1, y0, y1, z0, z1) = match axis {
            Axis::Z => (
                lo.x + inset,
                hi.x - inset,
                lo.y + inset,
                hi.y - inset,
                lo.z + step,
                lo.z + step,
            ),
            Axis::Y => (
                lo.x + inset,
                hi.x - inset,
                lo.y + step,
                lo.y + step,
                lo.z + inset,
                hi.z - inset,
            ),
            Axis::X => (
                lo.x + step,
                lo.x + step,
                lo.y + inset,
                hi.y - inset,
                lo.z + inset,
                hi.z - inset,
            ),
        };
        if x0 > x1 || y0 > y1 || z0 > z1 {
            break;
        }
        for z in clip(z0, z1, grid.size_z()) {
            for y in clip(y0, y1, grid.size_y()) {
                for x in clip(x0, x1, grid.size_x()) {
                    if hollow
                        && step > 0
                        && x > x0
                        && x < x1
                        && y > y0
                        && y < y1
                        && (axis != Axis::Z || (x > x0 && x < x1 && y > y0 && y < y1))
                    {
                        continue;
                    }
                    grid.set_i32(x, y, z, color);
                    count += 1;
                }
            }
        }
    }
    count
}

/// Draw a torus centered at `center` normal to `axis` with `major_radius` and `tube_radius`.
pub fn draw_torus(
    grid: &mut VoxelGrid,
    center: Vec3,
    axis: Axis,
    major_radius: f32,
    tube_radius: f32,
    color: u8,
) -> usize {
    let r_maj = major_radius.max(1.0);
    let r_tube = tube_radius.max(0.5);
    let ext = (r_maj + r_tube + 1.0).ceil() as i32;
    let cx = center.x.round() as i32;
    let cy = center.y.round() as i32;
    let cz = center.z.round() as i32;
    let mut count = 0usize;

    for z in clip(cz - ext, cz + ext, grid.size_z()) {
        for y in clip(cy - ext, cy + ext, grid.size_y()) {
            for x in clip(cx - ext, cx + ext, grid.size_x()) {
                let p = Vec3::new(
                    x as f32 - center.x,
                    y as f32 - center.y,
                    z as f32 - center.z,
                );
                let (planar, axial) = match axis {
                    Axis::Z => ((p.x * p.x + p.y * p.y).sqrt(), p.z),
                    Axis::Y => ((p.x * p.x + p.z * p.z).sqrt(), p.y),
                    Axis::X => ((p.y * p.y + p.z * p.z).sqrt(), p.x),
                };
                let q = planar - r_maj;
                if q * q + axial * axial <= r_tube * r_tube {
                    grid.set_i32(x, y, z, color);
                    count += 1;
                }
            }
        }
    }
    count
}

/// Draw a 3D line segment from `start` to `end` with optional thickness `radius` (`0` = 1-voxel line).
pub fn draw_line(grid: &mut VoxelGrid, start: IVec3, end: IVec3, color: u8, radius: i32) -> usize {
    let delta = end - start;
    let steps = delta.abs().max_element().max(1);
    let r = radius.clamp(0, MAX_LINE_RADIUS);
    let mut count = 0usize;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let cx = (start.x as f32 + delta.x as f32 * t).round() as i32;
        let cy = (start.y as f32 + delta.y as f32 * t).round() as i32;
        let cz = (start.z as f32 + delta.z as f32 * t).round() as i32;
        if !near_canvas(grid, IVec3::new(cx, cy, cz), r) {
            continue; // far from the canvas: nothing to draw, skip the thick-line loops
        }
        if r == 0 {
            if grid.in_bounds(cx, cy, cz) {
                grid.set_i32(cx, cy, cz, color);
                count += 1;
            }
        } else {
            for dz in -r..=r {
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx * dx + dy * dy + dz * dz <= r * r + 1 {
                            let (x, y, z) = (cx + dx, cy + dy, cz + dz);
                            if grid.in_bounds(x, y, z) {
                                grid.set_i32(x, y, z, color);
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    count
}

/// Replace all voxels of `from_color` with `to_color` (optionally restricted to `[min, max]`).
pub fn replace_color(
    grid: &mut VoxelGrid,
    from_color: u8,
    to_color: u8,
    bounds: Option<(IVec3, IVec3)>,
) -> usize {
    let (lo, hi) = match bounds {
        Some((a, b)) => (a.min(b), a.max(b)),
        None => (
            IVec3::ZERO,
            IVec3::new(
                grid.size_x() as i32 - 1,
                grid.size_y() as i32 - 1,
                grid.size_z() as i32 - 1,
            ),
        ),
    };
    let mut count = 0usize;
    for z in lo.z.max(0)..=hi.z.min(grid.size_z() as i32 - 1) {
        for y in lo.y.max(0)..=hi.y.min(grid.size_y() as i32 - 1) {
            for x in lo.x.max(0)..=hi.x.min(grid.size_x() as i32 - 1) {
                if grid.get(x as u32, y as u32, z as u32) == from_color {
                    grid.set(x as u32, y as u32, z as u32, to_color);
                    count += 1;
                }
            }
        }
    }
    count
}

pub fn parse_axis(s: Option<&str>) -> Axis {
    match s.unwrap_or("z").to_ascii_lowercase().as_str() {
        "x" => Axis::X,
        "y" => Axis::Y,
        _ => Axis::Z,
    }
}

/// Reject numbers that are not finite or exceed [`MAX_COORD`] anywhere in an operation.
fn check_numbers(v: &JsonValue) -> Result<(), String> {
    match v {
        JsonValue::Number(n) if !n.is_finite() || n.abs() > MAX_COORD => Err(format!(
            "Number {n} out of range: values must be within +/-{MAX_COORD} (the canvas is at most 256 voxels wide)"
        )),
        JsonValue::Array(items) => items.iter().try_for_each(check_numbers),
        JsonValue::Object(fields) => fields.iter().try_for_each(|(_, f)| check_numbers(f)),
        _ => Ok(()),
    }
}

/// Execute a single batch operation object on `grid`.
pub fn execute_batch_op(
    grid: &mut VoxelGrid,
    op: &JsonValue,
    default_color: u8,
) -> Result<usize, String> {
    check_numbers(op)?;
    let kind = op
        .get_str("op")
        .or_else(|| op.get_str("type"))
        .ok_or_else(|| "Batch operation missing 'op' field".to_string())?;
    let color = op.get_u8("color").unwrap_or(default_color);
    let hollow = op.get_bool("hollow").unwrap_or(false);

    match kind.to_ascii_lowercase().as_str() {
        "set" | "voxels" => {
            if let Some(voxels) = op.get_array("voxels") {
                let mut count = 0usize;
                for v in voxels {
                    let (x, y, z, c) = if let Some(arr) = v.as_array() {
                        if arr.len() < 3 {
                            continue;
                        }
                        let x = arr[0].as_i64().unwrap_or(0) as i32;
                        let y = arr[1].as_i64().unwrap_or(0) as i32;
                        let z = arr[2].as_i64().unwrap_or(0) as i32;
                        let c = arr
                            .get(3)
                            .and_then(|c| c.as_u64())
                            .map(|c| c.min(255) as u8)
                            .unwrap_or(color);
                        (x, y, z, c)
                    } else {
                        let x = v.get_i32("x").unwrap_or(0);
                        let y = v.get_i32("y").unwrap_or(0);
                        let z = v.get_i32("z").unwrap_or(0);
                        let c = v.get_u8("color").unwrap_or(color);
                        (x, y, z, c)
                    };
                    if grid.in_bounds(x, y, z) {
                        grid.set_i32(x, y, z, c);
                        count += 1;
                    }
                }
                Ok(count)
            } else {
                let x = op
                    .get_i32("x")
                    .ok_or_else(|| "'set' requires 'x', 'y', 'z' or 'voxels'".to_string())?;
                let y = op.get_i32("y").unwrap_or(0);
                let z = op.get_i32("z").unwrap_or(0);
                if grid.in_bounds(x, y, z) {
                    grid.set_i32(x, y, z, color);
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
        }
        "box" | "cuboid" => {
            let min = op
                .get_ivec3("min")
                .ok_or_else(|| "'box' op requires 'min': [x,y,z]".to_string())?;
            let max = op
                .get_ivec3("max")
                .ok_or_else(|| "'box' op requires 'max': [x,y,z]".to_string())?;
            let wall = op.get_i32("wall_thickness").unwrap_or(1);
            Ok(draw_box(
                grid,
                IVec3::from_array(min),
                IVec3::from_array(max),
                color,
                hollow,
                wall,
            ))
        }
        "sphere" | "ellipsoid" => {
            let c = op
                .get_vec3("center")
                .ok_or_else(|| "'sphere' op requires 'center': [x,y,z]".to_string())?;
            let radii = if let Some(r) = op.get_vec3("radii") {
                Vec3::from_array(r)
            } else {
                let r = op.get_f32("radius").unwrap_or(4.0);
                Vec3::splat(r)
            };
            let wall = op.get_f32("wall_thickness").unwrap_or(1.0);
            Ok(draw_ellipsoid(
                grid,
                Vec3::from_array(c),
                radii,
                color,
                hollow,
                wall,
            ))
        }
        "cylinder" | "cone" => {
            let c = op
                .get_vec3("center")
                .or_else(|| op.get_vec3("base"))
                .ok_or_else(|| "'cylinder' op requires 'center': [x,y,z]".to_string())?;
            let axis = parse_axis(op.get_str("axis"));
            let height = op.get_i32("height").unwrap_or(8);
            let r_bottom = op
                .get_f32("radius_bottom")
                .or_else(|| op.get_f32("radius"))
                .unwrap_or(4.0);
            let r_top = op.get_f32("radius_top").unwrap_or_else(|| {
                if kind.eq_ignore_ascii_case("cone") {
                    0.0
                } else {
                    r_bottom
                }
            });
            Ok(draw_cylinder(
                grid,
                Vec3::from_array(c),
                axis,
                height,
                r_bottom,
                r_top,
                color,
                hollow,
            ))
        }
        "pyramid" => {
            let min = op
                .get_ivec3("min")
                .ok_or_else(|| "'pyramid' op requires 'min': [x,y,z]".to_string())?;
            let max = op
                .get_ivec3("max")
                .ok_or_else(|| "'pyramid' op requires 'max': [x,y,z]".to_string())?;
            let axis = parse_axis(op.get_str("axis"));
            Ok(draw_pyramid(
                grid,
                IVec3::from_array(min),
                IVec3::from_array(max),
                axis,
                color,
                hollow,
            ))
        }
        "torus" => {
            let c = op
                .get_vec3("center")
                .ok_or_else(|| "'torus' op requires 'center': [x,y,z]".to_string())?;
            let axis = parse_axis(op.get_str("axis"));
            let major = op.get_f32("major_radius").unwrap_or(8.0);
            let tube = op.get_f32("tube_radius").unwrap_or(3.0);
            Ok(draw_torus(
                grid,
                Vec3::from_array(c),
                axis,
                major,
                tube,
                color,
            ))
        }
        "line" => {
            let start = op
                .get_ivec3("start")
                .ok_or_else(|| "'line' op requires 'start': [x,y,z]".to_string())?;
            let end = op
                .get_ivec3("end")
                .ok_or_else(|| "'line' op requires 'end': [x,y,z]".to_string())?;
            let radius = op.get_i32("radius").unwrap_or(0);
            Ok(draw_line(
                grid,
                IVec3::from_array(start),
                IVec3::from_array(end),
                color,
                radius,
            ))
        }
        "replace_color" => {
            let from = op
                .get_u8("from_color")
                .ok_or_else(|| "'replace_color' requires 'from_color'".to_string())?;
            let to = op.get_u8("to_color").unwrap_or(color);
            let bounds = match (op.get_ivec3("min"), op.get_ivec3("max")) {
                (Some(a), Some(b)) => Some((IVec3::from_array(a), IVec3::from_array(b))),
                _ => None,
            };
            Ok(replace_color(grid, from, to, bounds))
        }
        "clear" => {
            grid.clear();
            Ok(0)
        }
        other => Err(format!(
            "Unknown batch op '{other}' (expected set, box, sphere, ellipsoid, cylinder, cone, pyramid, torus, line, replace_color, clear)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: u32) -> VoxelGrid {
        VoxelGrid::new(n, n, n)
    }

    fn parse(s: &str) -> JsonValue {
        JsonValue::parse(s).unwrap()
    }

    fn extent(g: &VoxelGrid) -> (IVec3, IVec3) {
        let mut lo = IVec3::splat(i32::MAX);
        let mut hi = IVec3::splat(i32::MIN);
        for z in 0..g.size_z() {
            for y in 0..g.size_y() {
                for x in 0..g.size_x() {
                    if g.get(x, y, z) != 0 {
                        let p = IVec3::new(x as i32, y as i32, z as i32);
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
            }
        }
        (lo, hi)
    }

    #[test]
    fn box_solid_and_hollow_counts() {
        let mut g = grid(16);
        // Inclusive bounds: 5x5x5 = 125.
        assert_eq!(
            draw_box(
                &mut g,
                IVec3::new(2, 2, 2),
                IVec3::new(6, 6, 6),
                3,
                false,
                1
            ),
            125
        );
        assert_eq!(g.count_solid(), 125);
        let mut g = grid(16);
        // Hollow, wall 1: 125 - 3^3 = 98.
        assert_eq!(
            draw_box(&mut g, IVec3::new(2, 2, 2), IVec3::new(6, 6, 6), 3, true, 1),
            98
        );
        assert_eq!(g.get(4, 4, 4), 0);
        assert_eq!(g.get(2, 4, 4), 3);
        // Swapped corners behave the same; wall 2 on 5-wide leaves 1 inside voxel.
        let mut g = grid(16);
        assert_eq!(
            draw_box(&mut g, IVec3::new(6, 6, 6), IVec3::new(2, 2, 2), 3, true, 2),
            124
        );
        assert_eq!(g.get(4, 4, 4), 0);
    }

    #[test]
    fn box_clipped_to_canvas() {
        let mut g = grid(8);
        let n = draw_box(
            &mut g,
            IVec3::new(-5, -5, -5),
            IVec3::new(2, 2, 2),
            1,
            false,
            1,
        );
        assert_eq!(n, 27);
        let n = draw_box(
            &mut g,
            IVec3::new(6, 6, 6),
            IVec3::new(100, 100, 100),
            1,
            false,
            1,
        );
        assert_eq!(n, 8);
        assert_eq!(
            draw_box(&mut g, IVec3::splat(50), IVec3::splat(60), 1, false, 1),
            0
        );
    }

    #[test]
    fn ellipsoid_symmetric_and_bounded() {
        let mut g = grid(17);
        let n = draw_ellipsoid(
            &mut g,
            Vec3::splat(8.0),
            Vec3::new(6.0, 4.0, 3.0),
            2,
            false,
            1.0,
        );
        assert!(n > 0 && n == g.count_solid());
        for z in 0..17u32 {
            for y in 0..17u32 {
                for x in 0..17u32 {
                    assert_eq!(g.get(x, y, z), g.get(16 - x, y, z));
                    assert_eq!(g.get(x, y, z), g.get(x, 16 - y, z));
                    assert_eq!(g.get(x, y, z), g.get(x, y, 16 - z));
                }
            }
        }
        let (lo, hi) = extent(&g);
        assert_eq!(lo, IVec3::new(2, 4, 5));
        assert_eq!(hi, IVec3::new(14, 12, 11));
    }

    #[test]
    fn ellipsoid_hollow_has_empty_core_and_clips() {
        let mut g = grid(21);
        let solid = draw_ellipsoid(&mut g, Vec3::splat(10.0), Vec3::splat(8.0), 2, false, 1.0);
        let mut h = grid(21);
        let hollow = draw_ellipsoid(&mut h, Vec3::splat(10.0), Vec3::splat(8.0), 2, true, 1.0);
        assert!(hollow < solid);
        assert_eq!(h.get(10, 10, 10), 0);
        let mut c = grid(8);
        let n = draw_ellipsoid(
            &mut c,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::splat(5.0),
            1,
            false,
            1.0,
        );
        assert_eq!(n, c.count_solid());
        assert_eq!(
            draw_ellipsoid(&mut c, Vec3::splat(100.0), Vec3::splat(3.0), 1, false, 1.0),
            0
        );
    }

    #[test]
    fn cylinder_extent_each_axis() {
        // Height 6 along each axis, radius 3 (diameter 7).
        let base = Vec3::new(10.0, 10.0, 10.0);
        let mut g = grid(24);
        draw_cylinder(&mut g, base, Axis::Z, 6, 3.0, 3.0, 1, false);
        assert_eq!(extent(&g), (IVec3::new(7, 7, 10), IVec3::new(13, 13, 15)));
        let mut g = grid(24);
        draw_cylinder(&mut g, base, Axis::Y, 6, 3.0, 3.0, 1, false);
        assert_eq!(extent(&g), (IVec3::new(7, 10, 7), IVec3::new(13, 15, 13)));
        let mut g = grid(24);
        draw_cylinder(&mut g, base, Axis::X, 6, 3.0, 3.0, 1, false);
        assert_eq!(extent(&g), (IVec3::new(10, 7, 7), IVec3::new(15, 13, 13)));
    }

    #[test]
    fn cylinder_hollow_and_clipping() {
        let mut s = grid(24);
        let solid = draw_cylinder(&mut s, Vec3::splat(10.0), Axis::Z, 6, 4.0, 4.0, 1, false);
        let mut h = grid(24);
        let hollow = draw_cylinder(&mut h, Vec3::splat(10.0), Axis::Z, 6, 4.0, 4.0, 1, true);
        assert!(hollow < solid);
        assert_eq!(h.get(10, 10, 12), 0); // interior
        assert_eq!(h.get(10, 10, 10), 1); // bottom cap
        let mut c = grid(8);
        let n = draw_cylinder(
            &mut c,
            Vec3::new(-2.0, -2.0, -3.0),
            Axis::Z,
            20,
            6.0,
            6.0,
            1,
            false,
        );
        assert_eq!(n, c.count_solid());
    }

    #[test]
    fn cone_narrows() {
        let mut g = grid(24);
        draw_cylinder(
            &mut g,
            Vec3::new(10.0, 10.0, 0.0),
            Axis::Z,
            9,
            6.0,
            0.0,
            1,
            false,
        );
        let layer = |z: u32| {
            let mut n = 0;
            for y in 0..24 {
                for x in 0..24 {
                    if g.get(x, y, z) != 0 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(layer(0) > layer(4));
        assert!(layer(4) > layer(8));
        assert_eq!(layer(8), 1);
    }

    #[test]
    fn line_hits_endpoints() {
        let mut g = grid(16);
        let (a, b) = (IVec3::new(1, 1, 1), IVec3::new(12, 5, 3));
        let n = draw_line(&mut g, a, b, 9, 0);
        assert_eq!(n, 12); // max delta 11 -> 12 samples
        assert_eq!(g.get(1, 1, 1), 9);
        assert_eq!(g.get(12, 5, 3), 9);
        // Degenerate single point and reverse direction.
        let mut g = grid(16);
        draw_line(&mut g, a, a, 2, 0);
        assert_eq!(g.count_solid(), 1);
        let mut g = grid(16);
        draw_line(&mut g, b, a, 9, 1);
        assert_eq!(g.get(1, 1, 1), 9);
        assert_eq!(g.get(12, 5, 3), 9);
    }

    #[test]
    fn line_clipped_no_panic() {
        let mut g = grid(8);
        let n = draw_line(&mut g, IVec3::splat(-10), IVec3::splat(20), 1, 0);
        assert_eq!(n, g.count_solid());
        assert!(n > 0);
        draw_line(&mut g, IVec3::splat(-10), IVec3::splat(-20), 1, 2);
    }

    #[test]
    fn pyramid_and_torus_basic() {
        let mut g = grid(16);
        let n = draw_pyramid(
            &mut g,
            IVec3::new(2, 2, 0),
            IVec3::new(10, 10, 4),
            Axis::Z,
            1,
            false,
        );
        assert!(n > 0);
        assert_eq!(g.get(2, 2, 0), 1);
        assert_eq!(g.get(2, 2, 4), 0); // tapers
        let mut g = grid(24);
        let n = draw_torus(&mut g, Vec3::splat(12.0), Axis::Z, 6.0, 2.0, 1);
        assert!(n > 0);
        assert_eq!(g.get(12, 12, 12), 0); // hole
        assert_eq!(g.get(18, 12, 12), 1);
        // Clipped, no panic.
        let mut g = grid(8);
        draw_torus(&mut g, Vec3::new(-3.0, 40.0, 2.0), Axis::X, 6.0, 2.0, 1);
        draw_pyramid(&mut g, IVec3::splat(-5), IVec3::splat(30), Axis::X, 1, true);
    }

    #[test]
    fn replace_color_respects_bounds() {
        let mut g = grid(8);
        draw_box(&mut g, IVec3::ZERO, IVec3::splat(7), 4, false, 1);
        // Inclusive bounds [1,2]^3 = 8 voxels.
        assert_eq!(
            replace_color(&mut g, 4, 5, Some((IVec3::splat(1), IVec3::splat(2)))),
            8
        );
        assert_eq!(g.get(1, 1, 1), 5);
        assert_eq!(g.get(2, 2, 2), 5);
        assert_eq!(g.get(3, 3, 3), 4);
        assert_eq!(g.get(0, 0, 0), 4);
        // Out-of-canvas bounds are clipped: [-3,0]^3 -> just the origin.
        assert_eq!(
            replace_color(&mut g, 4, 6, Some((IVec3::splat(-3), IVec3::ZERO))),
            1
        );
        assert_eq!(g.get(0, 0, 0), 6);
        // Unbounded replaces everything left.
        let rest = g.count_solid() - 8 - 1;
        assert_eq!(replace_color(&mut g, 4, 7, None), rest);
        // Fully outside.
        assert_eq!(
            replace_color(&mut g, 7, 1, Some((IVec3::splat(20), IVec3::splat(30)))),
            0
        );
    }

    #[test]
    fn parse_axis_values() {
        assert_eq!(parse_axis(Some("X")), Axis::X);
        assert_eq!(parse_axis(Some("y")), Axis::Y);
        assert_eq!(parse_axis(Some("z")), Axis::Z);
        assert_eq!(parse_axis(Some("bogus")), Axis::Z);
        assert_eq!(parse_axis(None), Axis::Z);
    }

    #[test]
    fn batch_set_variants() {
        let mut g = grid(8);
        assert_eq!(
            execute_batch_op(
                &mut g,
                &parse(r#"{"op":"set","x":1,"y":2,"z":3,"color":5}"#),
                1
            ),
            Ok(1)
        );
        assert_eq!(g.get(1, 2, 3), 5);
        // Default color and out-of-bounds.
        assert_eq!(
            execute_batch_op(&mut g, &parse(r#"{"op":"set","x":0,"y":0,"z":0}"#), 7),
            Ok(1)
        );
        assert_eq!(g.get(0, 0, 0), 7);
        assert_eq!(
            execute_batch_op(&mut g, &parse(r#"{"op":"set","x":-1,"y":0,"z":0}"#), 7),
            Ok(0)
        );
        assert_eq!(
            execute_batch_op(&mut g, &parse(r#"{"op":"set","x":99,"y":0,"z":0}"#), 7),
            Ok(0)
        );
        let op = parse(
            r#"{"op":"voxels","voxels":[[1,1,1],[2,2,2,9],{"x":3,"y":3,"z":3,"color":4},[50,0,0],[1,2]],"color":6}"#,
        );
        assert_eq!(execute_batch_op(&mut g, &op, 1), Ok(3));
        assert_eq!((g.get(1, 1, 1), g.get(2, 2, 2), g.get(3, 3, 3)), (6, 9, 4));
        // "type" accepted as alias for "op".
        assert_eq!(
            execute_batch_op(&mut g, &parse(r#"{"type":"set","x":4,"y":4,"z":4}"#), 2),
            Ok(1)
        );
    }

    #[test]
    fn batch_shapes() {
        let mut g = grid(24);
        let cases = [
            r#"{"op":"box","min":[0,0,0],"max":[2,2,2]}"#,
            r#"{"op":"cuboid","min":[3,3,3],"max":[6,6,6],"hollow":true,"wall_thickness":1}"#,
            r#"{"op":"sphere","center":[12,12,12],"radius":3}"#,
            r#"{"op":"ellipsoid","center":[12,12,12],"radii":[4,3,2]}"#,
            r#"{"op":"cylinder","center":[5,15,2],"axis":"y","height":4,"radius":2}"#,
            r#"{"op":"cone","base":[18,18,0],"height":6,"radius":3}"#,
            r#"{"op":"pyramid","min":[0,10,10],"max":[6,16,16],"axis":"x"}"#,
            r#"{"op":"torus","center":[12,12,18],"major_radius":4,"tube_radius":1.5}"#,
            r#"{"op":"line","start":[0,0,0],"end":[10,10,10],"radius":1}"#,
        ];
        for c in cases {
            let n = execute_batch_op(&mut g, &parse(c), 3).unwrap_or_else(|e| panic!("{c}: {e}"));
            assert!(n > 0, "{c} drew nothing");
        }
        // Cone defaults radius_top to 0, cylinder keeps radius_bottom.
        let mut a = grid(24);
        let mut b = grid(24);
        let cone = execute_batch_op(
            &mut a,
            &parse(r#"{"op":"cone","center":[10,10,0],"height":6,"radius":4}"#),
            1,
        )
        .unwrap();
        let cyl = execute_batch_op(
            &mut b,
            &parse(r#"{"op":"cylinder","center":[10,10,0],"height":6,"radius":4}"#),
            1,
        )
        .unwrap();
        assert!(cone < cyl);
        // Box count through the batch path matches draw_box.
        let mut g = grid(8);
        assert_eq!(
            execute_batch_op(
                &mut g,
                &parse(r#"{"op":"box","min":[0,0,0],"max":[1,1,1]}"#),
                2
            ),
            Ok(8)
        );
    }

    #[test]
    fn batch_replace_and_clear() {
        let mut g = grid(8);
        execute_batch_op(
            &mut g,
            &parse(r#"{"op":"box","min":[0,0,0],"max":[3,3,3],"color":2}"#),
            1,
        )
        .unwrap();
        let n = execute_batch_op(
            &mut g,
            &parse(
                r#"{"op":"replace_color","from_color":2,"to_color":8,"min":[0,0,0],"max":[1,1,1]}"#,
            ),
            1,
        );
        assert_eq!(n, Ok(8));
        // to_color falls back to op color.
        let n = execute_batch_op(
            &mut g,
            &parse(r#"{"op":"replace_color","from_color":2,"color":9}"#),
            1,
        );
        assert_eq!(n, Ok(64 - 8));
        assert_eq!(g.get(3, 3, 3), 9);
        assert_eq!(
            execute_batch_op(&mut g, &parse(r#"{"op":"clear"}"#), 1),
            Ok(0)
        );
        assert_eq!(g.count_solid(), 0);
    }

    #[test]
    fn batch_errors() {
        let mut g = grid(8);
        let err = |g: &mut VoxelGrid, s: &str| execute_batch_op(g, &parse(s), 1).unwrap_err();
        assert_eq!(err(&mut g, "{}"), "Batch operation missing 'op' field");
        assert_eq!(
            err(&mut g, r#"{"op":"set"}"#),
            "'set' requires 'x', 'y', 'z' or 'voxels'"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"box","max":[1,1,1]}"#),
            "'box' op requires 'min': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"box","min":[1,1,1]}"#),
            "'box' op requires 'max': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"sphere"}"#),
            "'sphere' op requires 'center': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"cylinder"}"#),
            "'cylinder' op requires 'center': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"pyramid","max":[1,1,1]}"#),
            "'pyramid' op requires 'min': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"pyramid","min":[1,1,1]}"#),
            "'pyramid' op requires 'max': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"torus"}"#),
            "'torus' op requires 'center': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"line","end":[1,1,1]}"#),
            "'line' op requires 'start': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"line","start":[1,1,1]}"#),
            "'line' op requires 'end': [x,y,z]"
        );
        assert_eq!(
            err(&mut g, r#"{"op":"replace_color"}"#),
            "'replace_color' requires 'from_color'"
        );
        let e = err(&mut g, r#"{"op":"frobnicate"}"#);
        assert!(e.starts_with("Unknown batch op 'frobnicate'"), "{e}");
    }

    #[test]
    fn batch_out_of_canvas_is_clipped() {
        let mut g = grid(8);
        for c in [
            r#"{"op":"box","min":[-50,-50,-50],"max":[-40,-40,-40]}"#,
            r#"{"op":"sphere","center":[-20,100,3],"radius":6}"#,
            r#"{"op":"cylinder","center":[-9,-9,-9],"height":50,"radius":30}"#,
            r#"{"op":"line","start":[-100,0,0],"end":[100,0,0],"radius":2}"#,
            r#"{"op":"torus","center":[100,100,100]}"#,
        ] {
            execute_batch_op(&mut g, &parse(c), 1).unwrap();
        }
    }

    /// Shapes far larger than the canvas must cost no more than the canvas itself
    /// (loops are clipped), never hang the server.
    #[test]
    fn huge_shapes_are_fast_and_correct() {
        use std::time::Instant;
        let mut g = VoxelGrid::new(16, 16, 16);
        let t0 = Instant::now();

        // A box covering the canvas and far beyond fills it exactly.
        let n = draw_box(
            &mut g,
            IVec3::splat(-99_999),
            IVec3::splat(99_999),
            3,
            false,
            1,
        );
        assert_eq!(n, 16 * 16 * 16);
        // A hollow one leaves the shell only (no wall inside the canvas: nothing drawn).
        g.clear();
        let n = draw_box(
            &mut g,
            IVec3::splat(-99_999),
            IVec3::splat(99_999),
            3,
            true,
            1,
        );
        assert_eq!(n, 0, "walls are 100k voxels away");

        g.clear();
        assert!(
            draw_ellipsoid(
                &mut g,
                Vec3::splat(8.0),
                Vec3::splat(99_000.0),
                4,
                false,
                1.0
            ) > 4000
        );
        g.clear();
        let n = draw_cylinder(
            &mut g,
            Vec3::new(8.0, 8.0, -50_000.0),
            Axis::Z,
            100_000,
            99_000.0,
            99_000.0,
            5,
            false,
        );
        assert_eq!(n, 16 * 16 * 16, "a huge cylinder covers the canvas");
        g.clear();
        assert!(
            draw_cylinder(
                &mut g,
                Vec3::new(-90_000.0, 8.0, 8.0),
                Axis::X,
                99_999,
                4.0,
                4.0,
                5,
                false
            ) > 0
        );
        g.clear();
        assert!(
            draw_pyramid(
                &mut g,
                IVec3::splat(-90_000),
                IVec3::splat(90_000),
                Axis::Z,
                6,
                false
            ) > 0
        );
        g.clear();
        assert!(draw_torus(&mut g, Vec3::splat(8.0), Axis::Z, 80_000.0, 3.0, 7) <= 16 * 16 * 16);
        g.clear();
        // A very long, thick line crossing the canvas.
        draw_line(
            &mut g,
            IVec3::new(-100_000, 8, 8),
            IVec3::new(100_000, 8, 8),
            8,
            32,
        );
        assert_eq!(
            g.count_solid(),
            16 * 16 * 16,
            "radius 32 around a line through the canvas covers it"
        );
        // A very long line that misses the canvas entirely.
        g.clear();
        assert_eq!(
            draw_line(
                &mut g,
                IVec3::new(-100_000, 90_000, 8),
                IVec3::new(100_000, 90_000, 8),
                8,
                32
            ),
            0
        );

        assert!(t0.elapsed().as_secs_f32() < 2.0, "took {:?}", t0.elapsed());
    }

    #[test]
    fn batch_rejects_absurd_numbers() {
        let mut g = VoxelGrid::new(8, 8, 8);
        for bad in [
            r#"{"op":"box","min":[0,0,0],"max":[1000000,5,5]}"#,
            r#"{"op":"sphere","center":[4,4,4],"radius":1e9}"#,
            r#"{"op":"set","voxels":[[0,0,-1000001,3]]}"#,
        ] {
            let op = JsonValue::parse(bad).unwrap();
            let err = execute_batch_op(&mut g, &op, 1).unwrap_err();
            assert!(err.contains("out of range"), "{bad}: {err}");
        }
        assert_eq!(g.count_solid(), 0);
        // The limit itself is accepted.
        let ok =
            JsonValue::parse(r#"{"op":"box","min":[-100000,0,0],"max":[100000,1,1]}"#).unwrap();
        assert_eq!(execute_batch_op(&mut g, &ok, 1).unwrap(), 8 * 2 * 2);
    }
}
