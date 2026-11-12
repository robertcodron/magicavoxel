use glam::{IVec3, Vec3};

use crate::spline::sample_catmull_rom;
use crate::voxel::VoxelGrid;

/// Paints an organic cobblestone and dirt pathway along a series of 3D spline points.
///
/// - `points`: path trajectory in 3D.
/// - `width`: width of the pathway in voxels.
/// - `stone_color`: palette index for cobblestones.
/// - `dirt_color`: palette index for packed earth / edges.
pub fn paint_path(
    grid: &mut VoxelGrid,
    points: &[Vec3],
    width: u32,
    stone_color: u8,
    dirt_color: u8,
) {
    if points.is_empty() || width == 0 || (stone_color == 0 && dirt_color == 0) {
        return;
    }

    let samples = sample_catmull_rom(points, 0.4);
    if samples.is_empty() {
        return;
    }

    let radius = (width as f32 * 0.5).max(0.5);
    let r_ceil = radius.ceil() as i32;
    let r_sq = radius * radius;
    let inner_r_sq = (radius * 0.55) * (radius * 0.55);

    for pt in &samples {
        let cx = pt.x.round() as i32;
        let cy = pt.y.round() as i32;
        let pz = pt.z.round() as i32;

        for dx in -r_ceil..=r_ceil {
            for dy in -r_ceil..=r_ceil {
                let dist_sq = (dx * dx + dy * dy) as f32;
                if dist_sq > r_sq + 0.15 {
                    continue;
                }

                let vx = cx + dx;
                let vy = cy + dy;

                // Find top surface voxel around pz (search vertically from pz + 3 down to pz - 4)
                let mut surf_z = pz;
                let mut found_surface = false;
                for z in (pz - 4..=pz + 3).rev() {
                    if grid.get_i32(vx, vy, z) != 0 {
                        surf_z = z;
                        found_surface = true;
                        break;
                    }
                }
                if !found_surface {
                    surf_z = pz;
                }

                // Deterministic pseudo-random noise for organic cobblestone pattern
                let hash = (((vx as u32).wrapping_mul(374761393)
                    ^ (vy as u32).wrapping_mul(668265263)
                    ^ (surf_z as u32).wrapping_mul(1274126177))
                    >> 16)
                    % 100;

                let color = if dist_sq <= inner_r_sq {
                    // Center of path: cobblestone dominant
                    if hash < 80 {
                        stone_color
                    } else {
                        dirt_color
                    }
                } else {
                    // Borders: worn dirt dominant with scattered paving stones
                    if hash < 35 {
                        stone_color
                    } else {
                        dirt_color
                    }
                };

                grid.set_i32(vx, vy, surf_z, color);
            }
        }
    }
}

/// Carves a natural curved pond basin with a sloping bed and liquid water surface.
///
/// - `center`: center of the pond at water level.
/// - `radius_xy`: horizontal radius of the pond in voxels.
/// - `depth`: maximum depth at center of the pond.
/// - `water_color`: palette index for water.
pub fn carve_pond(
    grid: &mut VoxelGrid,
    center: IVec3,
    radius_xy: u32,
    depth: u32,
    water_color: u8,
) {
    if radius_xy == 0 || depth == 0 {
        return;
    }

    let r = radius_xy as i32;
    let r_f = radius_xy as f32;
    let r_sq = r_f * r_f;
    let d_max = depth as f32;

    for dx in -r..=r {
        for dy in -r..=r {
            let dist_sq = (dx * dx + dy * dy) as f32;
            if dist_sq > r_sq {
                continue;
            }

            let dist_norm = dist_sq.sqrt() / r_f;
            // Parabolic bowl depth profile
            let curr_depth = ((1.0 - dist_norm * dist_norm) * d_max).round() as i32;
            let bed_z = center.z - curr_depth;
            let vx = center.x + dx;
            let vy = center.y + dy;

            // 1. Carve air above water surface (up to 4 voxels overhead)
            for z in (center.z + 1)..=(center.z + depth as i32 + 2) {
                grid.set_i32(vx, vy, z, 0);
            }

            // 2. Fill water from bed up to surface
            if water_color != 0 {
                for z in (bed_z + 1)..=center.z {
                    grid.set_i32(vx, vy, z, water_color);
                }
            }
        }
    }
}

/// Grows climbing ivy tendrils along vertical walls.
///
/// - `surface_voxels`: starting seed points on or near vertical walls.
/// - `count`: maximum number of ivy leaf voxels to grow.
/// - `leaf_color`: palette index for ivy foliage.
pub fn grow_ivy(
    grid: &mut VoxelGrid,
    surface_voxels: &[IVec3],
    count: u32,
    leaf_color: u8,
) {
    if count == 0 || leaf_color == 0 {
        return;
    }

    let has_solid_wall_neighbor = |g: &VoxelGrid, p: IVec3| -> bool {
        g.get_i32(p.x + 1, p.y, p.z) != 0
            || g.get_i32(p.x - 1, p.y, p.z) != 0
            || g.get_i32(p.x, p.y + 1, p.z) != 0
            || g.get_i32(p.x, p.y - 1, p.z) != 0
    };

    let mut frontier = Vec::new();

    // Collect valid seeds (empty voxels adjacent to solid wall)
    for &seed in surface_voxels {
        if grid.get_i32(seed.x, seed.y, seed.z) == 0 && has_solid_wall_neighbor(grid, seed) {
            frontier.push(seed);
        } else {
            // If seed is solid, check horizontal empty neighbors
            for offset in [
                IVec3::new(1, 0, 0),
                IVec3::new(-1, 0, 0),
                IVec3::new(0, 1, 0),
                IVec3::new(0, -1, 0),
            ] {
                let candidate = seed + offset;
                if grid.get_i32(candidate.x, candidate.y, candidate.z) == 0
                    && has_solid_wall_neighbor(grid, candidate)
                {
                    frontier.push(candidate);
                }
            }
        }
    }

    if frontier.is_empty() {
        return;
    }

    let mut placed = 0;
    let mut rng_state = 123456789u32;
    let mut next_rand = || {
        rng_state = rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
        rng_state
    };

    let mut placed_ivy = Vec::new();

    while placed < count && !frontier.is_empty() {
        let idx = (next_rand() as usize) % frontier.len();
        let current = frontier.swap_remove(idx);

        if grid.get_i32(current.x, current.y, current.z) != 0 {
            continue;
        }

        grid.set_i32(current.x, current.y, current.z, leaf_color);
        placed += 1;
        placed_ivy.push(current);

        // Branch outward: upward bias (+Z), plus lateral spread along the wall
        let candidates = [
            current + IVec3::new(0, 0, 1),  // Up
            current + IVec3::new(0, 0, 1),  // Up (higher weight)
            current + IVec3::new(1, 0, 0),  // Right
            current + IVec3::new(-1, 0, 0), // Left
            current + IVec3::new(0, 1, 0),  // Forward
            current + IVec3::new(0, -1, 0), // Back
        ];

        for &cand in &candidates {
            if grid.get_i32(cand.x, cand.y, cand.z) == 0 && has_solid_wall_neighbor(grid, cand) {
                if !frontier.contains(&cand) {
                    frontier.push(cand);
                }
            }
        }

        // Branching from existing ivy nodes when frontier gets thin
        if frontier.len() < 3 && !placed_ivy.is_empty() {
            let branch_node = placed_ivy[(next_rand() as usize) % placed_ivy.len()];
            for offset in [
                IVec3::new(0, 0, 1),
                IVec3::new(1, 0, 0),
                IVec3::new(-1, 0, 0),
                IVec3::new(0, 1, 0),
                IVec3::new(0, -1, 0),
            ] {
                let cand = branch_node + offset;
                if grid.get_i32(cand.x, cand.y, cand.z) == 0 && has_solid_wall_neighbor(grid, cand) {
                    if !frontier.contains(&cand) {
                        frontier.push(cand);
                    }
                }
            }
        }
    }
}

/// Generates a stylized, fluffy tree with a wooden trunk and puffy multi-lobed foliage.
///
/// - `base`: ground position of the trunk base.
/// - `trunk_height`: height of the wooden trunk.
/// - `crown_radius`: radius of the main foliage crown.
/// - `wood_color`: palette index for trunk and branches.
/// - `foliage_color`: palette index for fluffy leaves.
pub fn generate_fluffy_tree(
    grid: &mut VoxelGrid,
    base: IVec3,
    trunk_height: u32,
    crown_radius: u32,
    wood_color: u8,
    foliage_color: u8,
) {
    if trunk_height == 0 || crown_radius == 0 {
        return;
    }

    let th = trunk_height as i32;
    let _cr = crown_radius as i32;
    let cr_f = crown_radius as f32;

    // 1. Wooden trunk
    let is_thick_trunk = crown_radius >= 5;
    for dz in 0..th {
        let z = base.z + dz;
        grid.set_i32(base.x, base.y, z, wood_color);
        if is_thick_trunk {
            grid.set_i32(base.x + 1, base.y, z, wood_color);
            grid.set_i32(base.x, base.y + 1, z, wood_color);
            grid.set_i32(base.x + 1, base.y + 1, z, wood_color);
        }
    }

    // Small branch spurs at trunk top
    let top_trunk_z = base.z + th - 1;
    grid.set_i32(base.x + 1, base.y, top_trunk_z, wood_color);
    grid.set_i32(base.x - 1, base.y, top_trunk_z, wood_color);
    grid.set_i32(base.x, base.y + 1, top_trunk_z, wood_color);
    grid.set_i32(base.x, base.y - 1, top_trunk_z, wood_color);

    // 2. Fluffy multi-lobed crown
    let crown_center = Vec3::new(
        base.x as f32 + if is_thick_trunk { 0.5 } else { 0.0 },
        base.y as f32 + if is_thick_trunk { 0.5 } else { 0.0 },
        (base.z + th) as f32 + (cr_f * 0.4),
    );

    // Define multiple overlapping sphere lobes for the "fluffy" appearance
    let lobes = [
        (crown_center, cr_f),
        (crown_center + Vec3::new(cr_f * 0.45, 0.0, cr_f * 0.1), cr_f * 0.65),
        (crown_center + Vec3::new(-cr_f * 0.45, 0.0, cr_f * 0.1), cr_f * 0.65),
        (crown_center + Vec3::new(0.0, cr_f * 0.45, cr_f * 0.1), cr_f * 0.65),
        (crown_center + Vec3::new(0.0, -cr_f * 0.45, cr_f * 0.1), cr_f * 0.65),
        (crown_center + Vec3::new(0.0, 0.0, cr_f * 0.45), cr_f * 0.70),
    ];

    let min_x = (crown_center.x - cr_f * 1.5).floor() as i32;
    let max_x = (crown_center.x + cr_f * 1.5).ceil() as i32;
    let min_y = (crown_center.y - cr_f * 1.5).floor() as i32;
    let max_y = (crown_center.y + cr_f * 1.5).ceil() as i32;
    let min_z = (crown_center.z - cr_f * 1.5).floor() as i32;
    let max_z = (crown_center.z + cr_f * 1.5).ceil() as i32;

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                // Do not overwrite trunk wood
                if grid.get_i32(x, y, z) == wood_color {
                    continue;
                }

                let p = Vec3::new(x as f32, y as f32, z as f32);
                let mut inside_any_lobe = false;

                for &(center, radius) in &lobes {
                    if p.distance_squared(center) <= radius * radius {
                        inside_any_lobe = true;
                        break;
                    }
                }

                if inside_any_lobe {
                    grid.set_i32(x, y, z, foliage_color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_path() {
        let mut grid = VoxelGrid::new(20, 20, 10);
        // Ground plane
        for x in 0..20 {
            for y in 0..20 {
                grid.set(x, y, 0, 1);
            }
        }

        let pts = vec![
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(18.0, 18.0, 0.0),
        ];

        paint_path(&mut grid, &pts, 3, 40, 41);

        assert!(grid.raw_data().contains(&40) || grid.raw_data().contains(&41));
    }

    #[test]
    fn test_carve_pond() {
        let mut grid = VoxelGrid::new(20, 20, 10);
        grid.full(1);

        carve_pond(&mut grid, IVec3::new(10, 10, 5), 5, 4, 30);

        // Water placed at center
        assert_eq!(grid.get_i32(10, 10, 5), 30);
        // Air cleared above pond
        assert_eq!(grid.get_i32(10, 10, 8), 0);
    }

    #[test]
    fn test_grow_ivy() {
        let mut grid = VoxelGrid::new(20, 20, 20);
        // Build wall at x = 10
        for y in 5..15 {
            for z in 0..15 {
                grid.set_i32(10, y, z, 5);
            }
        }

        // Seed at base of wall
        let seeds = vec![IVec3::new(9, 10, 0)];
        grow_ivy(&mut grid, &seeds, 20, 77);

        assert!(grid.raw_data().contains(&77));
        assert!(grid.raw_data().iter().filter(|&&v| v == 77).count() >= 10);
    }

    #[test]
    fn test_generate_fluffy_tree() {
        let mut grid = VoxelGrid::new(24, 24, 24);
        generate_fluffy_tree(&mut grid, IVec3::new(12, 12, 0), 8, 4, 33, 44);

        // Trunk wood present
        assert_eq!(grid.get_i32(12, 12, 0), 33);
        assert_eq!(grid.get_i32(12, 12, 7), 33);

        // Foliage present above trunk
        assert_eq!(grid.get_i32(12, 12, 11), 44);
        assert!(grid.count_solid() > 50);
    }
}
