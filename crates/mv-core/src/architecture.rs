use glam::IVec3;

use crate::voxel::VoxelGrid;

/// Types of procedural roofs supported by the architecture generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoofType {
    /// Classic two-sided gabled roof with eaves and ridge line.
    Gabled,
    /// Four-sided hipped roof tapering from all four sides.
    Hipped,
    /// French Mansard roof with steep lower slopes and shallow upper slopes.
    Mansard,
    /// Round conical turret roof.
    Conical,
}

impl RoofType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "gabled" | "gable" | "g" => Some(Self::Gabled),
            "hipped" | "hip" | "h" => Some(Self::Hipped),
            "mansard" | "m" => Some(Self::Mansard),
            "conical" | "cone" | "c" => Some(Self::Conical),
            _ => None,
        }
    }
}

/// Generates a procedural roof over a bounding box with configurable overhang and roof style.
///
/// - `min`: lower corner of the roof bounding box (including base Z).
/// - `max`: upper corner of the roof bounding box (including apex Z).
/// - `roof_type`: shape profile (`Gabled`, `Hipped`, `Mansard`, `Conical`).
/// - `roof_color`: palette index for roof tiles/shingles.
/// - `overhang`: eave overhang extension in voxels beyond the footprint.
pub fn generate_roof(
    grid: &mut VoxelGrid,
    min: IVec3,
    max: IVec3,
    roof_type: RoofType,
    roof_color: u8,
    overhang: u32,
) {
    if roof_color == 0 {
        return;
    }

    let min_x = min.x.min(max.x);
    let max_x = min.x.max(max.x);
    let min_y = min.y.min(max.y);
    let max_y = min.y.max(max.y);
    let min_z = min.z.min(max.z);
    let max_z = min.z.max(max.z);

    let o = overhang as i32;
    let x0 = min_x - o;
    let x1 = max_x + o;
    let y0 = min_y - o;
    let y1 = max_y + o;

    let height = (max_z - min_z + 1).max(1) as f32;
    let width = (x1 - x0 + 1) as f32;
    let depth = (y1 - y0 + 1) as f32;

    match roof_type {
        RoofType::Gabled => {
            // Align ridge along the longer axis
            if width <= depth {
                // Ridge runs along Y at center X
                let mid_x = (x0 + x1) as f32 * 0.5;
                let half_span = (width * 0.5).max(0.5);

                for x in x0..=x1 {
                    let dist = (x as f32 - mid_x).abs();
                    let frac = (1.0 - (dist / half_span)).clamp(0.0, 1.0);
                    let z_top = min_z + (frac * (height - 1.0)).round() as i32;

                    for y in y0..=y1 {
                        grid.set_i32(x, y, z_top, roof_color);
                        // Ensure slope is continuous without vertical gaps
                        if z_top > min_z {
                            grid.set_i32(x, y, z_top - 1, roof_color);
                        }

                        // Close the gable end walls
                        if (y == y0 || y == y1) && z_top > min_z {
                            for z in min_z..z_top {
                                grid.set_i32(x, y, z, roof_color);
                            }
                        }
                    }
                }
            } else {
                // Ridge runs along X at center Y
                let mid_y = (y0 + y1) as f32 * 0.5;
                let half_span = (depth * 0.5).max(0.5);

                for y in y0..=y1 {
                    let dist = (y as f32 - mid_y).abs();
                    let frac = (1.0 - (dist / half_span)).clamp(0.0, 1.0);
                    let z_top = min_z + (frac * (height - 1.0)).round() as i32;

                    for x in x0..=x1 {
                        grid.set_i32(x, y, z_top, roof_color);
                        if z_top > min_z {
                            grid.set_i32(x, y, z_top - 1, roof_color);
                        }

                        if (x == x0 || x == x1) && z_top > min_z {
                            for z in min_z..z_top {
                                grid.set_i32(x, y, z, roof_color);
                            }
                        }
                    }
                }
            }
        }

        RoofType::Hipped => {
            let max_d = ((width.min(depth) * 0.5) - 0.5).max(0.5);

            for x in x0..=x1 {
                for y in y0..=y1 {
                    let dx = (x - x0).min(x1 - x).max(0) as f32;
                    let dy = (y - y0).min(y1 - y).max(0) as f32;
                    let d = dx.min(dy);
                    let frac = (d / max_d).clamp(0.0, 1.0);
                    let z_top = min_z + (frac * (height - 1.0)).round() as i32;

                    grid.set_i32(x, y, z_top, roof_color);
                    if z_top > min_z {
                        grid.set_i32(x, y, z_top - 1, roof_color);
                    }
                }
            }
        }

        RoofType::Mansard => {
            let max_d = ((width.min(depth) * 0.5) - 0.5).max(0.5);

            for x in x0..=x1 {
                for y in y0..=y1 {
                    let dx = (x - x0).min(x1 - x).max(0) as f32;
                    let dy = (y - y0).min(y1 - y).max(0) as f32;
                    let d = dx.min(dy);
                    let frac = (d / max_d).clamp(0.0, 1.0);

                    // Mansard break: steep lower 35% of run rises 70% of height
                    let h_frac = if frac < 0.35 {
                        (frac / 0.35) * 0.70
                    } else {
                        0.70 + ((frac - 0.35) / 0.65) * 0.30
                    };
                    let z_top = min_z + (h_frac * (height - 1.0)).round() as i32;

                    grid.set_i32(x, y, z_top, roof_color);
                    if z_top > min_z {
                        grid.set_i32(x, y, z_top - 1, roof_color);
                    }
                }
            }
        }

        RoofType::Conical => {
            let cx = (x0 + x1) as f32 * 0.5;
            let cy = (y0 + y1) as f32 * 0.5;
            let radius = (width.max(depth) * 0.5).max(0.5);

            for x in x0..=x1 {
                for y in y0..=y1 {
                    let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                    if dist <= radius + 0.3 {
                        let frac = (1.0 - (dist / radius)).clamp(0.0, 1.0);
                        let z_top = min_z + (frac * (height - 1.0)).round() as i32;

                        grid.set_i32(x, y, z_top, roof_color);
                        if z_top > min_z {
                            grid.set_i32(x, y, z_top - 1, roof_color);
                        }
                    }
                }
            }
        }
    }
}

/// Carves a window opening, adds an outer frame, recessed glass pane, and optional shutters.
///
/// - `min`: lower corner of window aperture.
/// - `max`: upper corner of window aperture.
/// - `frame_color`: palette index for timber/stone frame.
/// - `glass_color`: palette index for translucent glass pane.
/// - `shutters`: if true, places wooden shutters on the sides of the window.
pub fn punch_window(
    grid: &mut VoxelGrid,
    min: IVec3,
    max: IVec3,
    frame_color: u8,
    glass_color: u8,
    shutters: bool,
) {
    let x0 = min.x.min(max.x);
    let x1 = min.x.max(max.x);
    let y0 = min.y.min(max.y);
    let y1 = min.y.max(max.y);
    let z0 = min.z.min(max.z);
    let z1 = min.z.max(max.z);

    let span_x = x1 - x0;
    let span_y = y1 - y0;

    // Detect wall orientation: is thickness along X or along Y?
    let wall_along_y = span_x <= span_y;

    // 1. Carve aperture cavity
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                grid.set_i32(x, y, z, 0);
            }
        }
    }

    // 2. Build window frame around the perimeter
    if frame_color != 0 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let is_border = if wall_along_y {
                        y == y0 || y == y1 || z == z0 || z == z1
                    } else {
                        x == x0 || x == x1 || z == z0 || z == z1
                    };

                    if is_border {
                        grid.set_i32(x, y, z, frame_color);
                    }
                }
            }
        }
    }

    // 3. Place recessed glass pane in the center of the aperture
    if glass_color != 0 {
        if wall_along_y {
            let mid_x = (x0 + x1) / 2;
            for y in (y0 + 1)..y1 {
                for z in (z0 + 1)..z1 {
                    grid.set_i32(mid_x, y, z, glass_color);
                }
            }
        } else {
            let mid_y = (y0 + y1) / 2;
            for x in (x0 + 1)..x1 {
                for z in (z0 + 1)..z1 {
                    grid.set_i32(x, mid_y, z, glass_color);
                }
            }
        }
    }

    // 4. Optional exterior shutters flanking the window
    if shutters && frame_color != 0 {
        if wall_along_y {
            // Shutters placed at y0 - 1 and y1 + 1
            for z in z0..=z1 {
                grid.set_i32(x0, y0 - 1, z, frame_color);
                grid.set_i32(x0, y1 + 1, z, frame_color);
            }
        } else {
            // Shutters placed at x0 - 1 and x1 + 1
            for z in z0..=z1 {
                grid.set_i32(x0 - 1, y0, z, frame_color);
                grid.set_i32(x1 + 1, y0, z, frame_color);
            }
        }
    }
}

/// Carves a doorway with a doorframe chambranle and wooden door panel.
///
/// - `origin`: bottom-left corner of the doorway.
/// - `width`: width of the door aperture (along X).
/// - `height`: height of the door aperture (along Z).
/// - `frame_color`: palette index for outer doorframe.
/// - `door_color`: palette index for the wooden door panel.
pub fn punch_door(
    grid: &mut VoxelGrid,
    origin: IVec3,
    width: u32,
    height: u32,
    frame_color: u8,
    door_color: u8,
) {
    if width == 0 || height == 0 {
        return;
    }

    let w = width as i32;
    let h = height as i32;
    let x0 = origin.x;
    let x1 = origin.x + w - 1;
    let y = origin.y;
    let z0 = origin.z;
    let z1 = origin.z + h - 1;

    // 1. Carve opening cavity
    for x in x0..=x1 {
        for z in z0..=z1 {
            grid.set_i32(x, y, z, 0);
        }
    }

    // 2. Doorframe along left, right, and top edges
    if frame_color != 0 {
        for z in z0..=z1 {
            grid.set_i32(x0, y, z, frame_color);
            grid.set_i32(x1, y, z, frame_color);
        }
        for x in x0..=x1 {
            grid.set_i32(x, y, z1, frame_color);
        }
    }

    // 3. Wooden door panel inside the frame
    if door_color != 0 && w > 2 && h > 1 {
        for x in (x0 + 1)..x1 {
            for z in z0..z1 {
                grid.set_i32(x, y, z, door_color);
            }
        }

        // Small doorknob accent
        let handle_x = if w > 3 { x1 - 1 } else { x0 + 1 };
        let handle_z = z0 + (h / 2).max(1);
        if frame_color != 0 {
            grid.set_i32(handle_x, y, handle_z, frame_color);
        }
    }
}

/// Builds a brick chimney stack with a hollow flue and decorative chimney pot (mitron).
///
/// - `origin`: base corner of the chimney column.
/// - `height`: height in voxels.
/// - `width`: square dimension of the stack in voxels (e.g. 2, 3, or 4).
/// - `brick_color`: palette index for brick/stone masonry.
pub fn build_chimney(
    grid: &mut VoxelGrid,
    origin: IVec3,
    height: u32,
    width: u32,
    brick_color: u8,
) {
    if height == 0 || width == 0 || brick_color == 0 {
        return;
    }

    let w = width as i32;
    let h = height as i32;
    let x0 = origin.x;
    let y0 = origin.y;
    let z0 = origin.z;

    // 1. Build main chimney shaft
    for dz in 0..h {
        let z = z0 + dz;
        for dx in 0..w {
            for dy in 0..w {
                let is_outer = dx == 0 || dx == w - 1 || dy == 0 || dy == w - 1;
                let vx = x0 + dx;
                let vy = y0 + dy;

                if is_outer {
                    grid.set_i32(vx, vy, z, brick_color);
                } else {
                    // Hollow interior flue
                    grid.set_i32(vx, vy, z, 0);
                }
            }
        }
    }

    // 2. Crown / mitron on top of chimney
    let top_z = z0 + h;
    if w >= 3 {
        // Extended crown rim
        for dx in 0..w {
            for dy in 0..w {
                let is_outer = dx == 0 || dx == w - 1 || dy == 0 || dy == w - 1;
                if is_outer {
                    grid.set_i32(x0 + dx, y0 + dy, top_z, brick_color);
                }
            }
        }
        // Chimney pot / mitron rising from the central flue
        let pot_x = x0 + w / 2;
        let pot_y = y0 + w / 2;
        grid.set_i32(pot_x, pot_y, top_z, brick_color);
        grid.set_i32(pot_x, pot_y, top_z + 1, brick_color);
    } else {
        // Compact 1x1 or 2x2 mitron
        grid.set_i32(x0, y0, top_z, brick_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_roof_styles() {
        for style in [RoofType::Gabled, RoofType::Hipped, RoofType::Mansard, RoofType::Conical] {
            let mut grid = VoxelGrid::new(20, 20, 20);
            generate_roof(
                &mut grid,
                IVec3::new(4, 4, 2),
                IVec3::new(14, 14, 8),
                style,
                55,
                1,
            );
            assert!(grid.count_solid() > 20, "Failed for style {:?}", style);
            assert_eq!(grid.get_i32(9, 9, 8), 55);
        }
    }

    #[test]
    fn test_punch_window_and_door() {
        let mut grid = VoxelGrid::new(20, 20, 20);
        // Build solid wall
        for x in 0..20 {
            for z in 0..10 {
                grid.set_i32(x, 5, z, 10);
            }
        }

        // Punch window with shutters
        punch_window(
            &mut grid,
            IVec3::new(2, 5, 3),
            IVec3::new(6, 5, 7),
            12, // frame
            13, // glass
            true,
        );

        // Frame and glass must be placed
        assert_eq!(grid.get_i32(2, 5, 3), 12); // frame corner
        assert_eq!(grid.get_i32(4, 5, 5), 13); // glass center

        // Punch door
        punch_door(
            &mut grid,
            IVec3::new(10, 5, 0),
            4,
            7,
            12, // frame
            14, // wood door
        );
        assert_eq!(grid.get_i32(10, 5, 0), 12); // frame
        assert_eq!(grid.get_i32(11, 5, 1), 14); // door panel
    }

    #[test]
    fn test_build_chimney() {
        let mut grid = VoxelGrid::new(16, 16, 16);
        build_chimney(&mut grid, IVec3::new(4, 4, 0), 8, 3, 25);

        // Outer wall solid, interior flue hollow
        assert_eq!(grid.get_i32(4, 4, 0), 25);
        assert_eq!(grid.get_i32(5, 5, 3), 0); // hollow flue

        // Chimney pot at top
        assert_eq!(grid.get_i32(5, 5, 9), 25);
    }
}
