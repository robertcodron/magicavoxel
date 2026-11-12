use glam::{IVec3, Vec3};

use crate::voxel::VoxelGrid;

/// A local omnidirectional point light for atmospheric illumination.
#[derive(Debug, Clone, PartialEq)]
pub struct PointLight {
    /// 3D position in voxel space.
    pub position: Vec3,
    /// Effective radius of illumination in voxels.
    pub radius: f32,
    /// Linear RGB color in `[0.0, 1.0]`.
    pub color: [f32; 3],
    /// Luminous intensity multiplier.
    pub intensity: f32,
}

impl PointLight {
    pub fn new(position: Vec3, radius: f32, color: [f32; 3], intensity: f32) -> Self {
        Self {
            position,
            radius,
            color,
            intensity,
        }
    }

    /// Warm incandescent / candlelight preset.
    pub fn warm(position: Vec3, radius: f32, intensity: f32) -> Self {
        Self {
            position,
            radius,
            color: [1.0, 0.78, 0.45],
            intensity,
        }
    }

    /// Cool moonlight preset.
    pub fn moonlight(position: Vec3, radius: f32, intensity: f32) -> Self {
        Self {
            position,
            radius,
            color: [0.65, 0.75, 1.0],
            intensity,
        }
    }
}

/// Spawns a stylized voxel lantern prop mounted to a wall, hanging from a ceiling, or standing on the floor.
///
/// - `pos`: attachment position.
/// - `wall_normal`: outward normal from the supporting surface (e.g. `(0, 1, 0)` for a wall facing +Y,
///   `(0, 0, -1)` for hanging from a ceiling, or `(0, 0, 1)` / `(0, 0, 0)` for standing).
/// - `lantern_color`: palette index for metal frame / iron bracket.
/// - `light_color`: palette index for emissive flame / crystal core.
pub fn spawn_lantern(
    grid: &mut VoxelGrid,
    pos: IVec3,
    wall_normal: IVec3,
    lantern_color: u8,
    light_color: u8,
) {
    if lantern_color == 0 && light_color == 0 {
        return;
    }

    if wall_normal.x != 0 || wall_normal.y != 0 {
        // Wall-mounted sconce lantern
        let nx = wall_normal.x.signum();
        let ny = wall_normal.y.signum();

        // 1. Iron wall plate and outward bracket arm
        grid.set_i32(pos.x, pos.y, pos.z, lantern_color);
        grid.set_i32(pos.x + nx, pos.y + ny, pos.z, lantern_color);
        grid.set_i32(pos.x + nx, pos.y + ny, pos.z - 1, lantern_color);

        // 2. Hanging lantern body
        let lx = pos.x + nx;
        let ly = pos.y + ny;
        let lz = pos.z - 2;

        grid.set_i32(lx, ly, lz, lantern_color);     // Cap
        grid.set_i32(lx, ly, lz - 1, light_color);   // Glowing core
        grid.set_i32(lx, ly, lz - 2, lantern_color); // Base
    } else if wall_normal.z < 0 {
        // Ceiling hanging lantern
        grid.set_i32(pos.x, pos.y, pos.z, lantern_color);     // Chain link 1
        grid.set_i32(pos.x, pos.y, pos.z - 1, lantern_color); // Chain link 2
        grid.set_i32(pos.x, pos.y, pos.z - 2, lantern_color); // Cap
        grid.set_i32(pos.x, pos.y, pos.z - 3, light_color);   // Glowing core
        grid.set_i32(pos.x, pos.y, pos.z - 4, lantern_color); // Base
    } else {
        // Freestanding lantern (table / ground)
        grid.set_i32(pos.x, pos.y, pos.z, lantern_color);     // Base
        grid.set_i32(pos.x, pos.y, pos.z + 1, light_color);   // Glowing core
        grid.set_i32(pos.x, pos.y, pos.z + 2, lantern_color); // Cap & ring handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_light_creation() {
        let l = PointLight::new(Vec3::new(5.0, 5.0, 5.0), 10.0, [1.0, 0.8, 0.5], 2.5);
        assert_eq!(l.position, Vec3::new(5.0, 5.0, 5.0));
        assert_eq!(l.radius, 10.0);
        assert_eq!(l.intensity, 2.5);

        let warm = PointLight::warm(Vec3::ZERO, 8.0, 1.0);
        assert!(warm.color[0] > warm.color[2]);
    }

    #[test]
    fn test_spawn_lantern_wall_and_freestanding() {
        let mut grid = VoxelGrid::new(16, 16, 16);

        // Wall mounted
        spawn_lantern(&mut grid, IVec3::new(5, 5, 8), IVec3::new(0, 1, 0), 10, 11);
        assert_eq!(grid.get_i32(5, 5, 8), 10); // Wall bracket
        assert_eq!(grid.get_i32(5, 6, 5), 11); // Glowing core

        // Freestanding
        spawn_lantern(&mut grid, IVec3::new(10, 10, 0), IVec3::ZERO, 20, 21);
        assert_eq!(grid.get_i32(10, 10, 0), 20); // Base
        assert_eq!(grid.get_i32(10, 10, 1), 21); // Core
        assert_eq!(grid.get_i32(10, 10, 2), 20); // Cap
    }
}
