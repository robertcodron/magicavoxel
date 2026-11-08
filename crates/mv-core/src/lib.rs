pub mod brush;
pub mod command;
pub mod history;
pub mod material;
pub mod palette;
pub mod project;
pub mod selection;
pub mod voxel;

pub use brush::{BrushAction, BrushMode, BrushSettings, VoxelSubShape};
pub use command::{execute_console_command, CommandOutcome};
pub use history::History;
pub use material::{Material, MaterialType};
pub use palette::{Palette, Rgba};
pub use project::EditorProject;
pub use selection::{SelectionMask, VoxelClipboard};
pub use voxel::{Axis, AxisFlags, VoxelGrid, MAX_GRID_DIM};

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;

    #[test]
    fn test_voxel_grid_operations() {
        let mut grid = VoxelGrid::new(16, 16, 16);
        assert_eq!(grid.count_solid(), 0);

        grid.set(2, 3, 4, 7);
        assert_eq!(grid.get(2, 3, 4), 7);
        assert_eq!(grid.count_solid(), 1);

        // Mirror X
        grid.mirror_symmetry(AxisFlags {
            x: true,
            y: false,
            z: false,
        });
        assert_eq!(grid.get(13, 3, 4), 7);
        assert_eq!(grid.count_solid(), 2);

        // Shrink
        assert!(grid.shrink());
        assert_eq!(grid.dimensions(), glam::UVec3::new(12, 1, 1));
    }

    #[test]
    fn test_console_commands_and_undo_redo() {
        let mut proj = EditorProject::new_empty(16, 16, 16);
        proj.palette.selected = 5;

        proj.run_command("full").unwrap();
        assert_eq!(proj.grid.count_solid(), 16 * 16 * 16);

        // Flood 0 hollows out interior invisible voxels
        proj.run_command("flood 0").unwrap();
        let shell_count = 16 * 16 * 16 - 14 * 14 * 14;
        assert_eq!(proj.grid.count_solid(), shell_count);

        // Undo restores full cube
        assert!(proj.undo());
        assert_eq!(proj.grid.count_solid(), 16 * 16 * 16);

        // Redo re-applies flood 0
        assert!(proj.redo());
        assert_eq!(proj.grid.count_solid(), shell_count);

        // Test voxel shader `xs sphere`
        proj.run_command("xs sphere 6").unwrap();
        assert!(proj.grid.count_solid() > 100);
    }

    #[test]
    fn test_brush_with_symmetries() {
        let mut proj = EditorProject::new_empty(10, 10, 10);
        proj.brush.action = BrushAction::Attach;
        proj.brush.mode = BrushMode::Voxel;
        proj.brush.mirror = AxisFlags {
            x: true,
            y: true,
            z: false,
        };
        proj.brush.apply(
            &mut proj.grid,
            &mut proj.selection,
            12,
            IVec3::new(1, 1, 0),
            IVec3::new(1, 1, 0),
            IVec3::ZERO,
            false,
        );
        assert_eq!(proj.grid.get(1, 1, 0), 12);
        assert_eq!(proj.grid.get(8, 1, 0), 12);
        assert_eq!(proj.grid.get(1, 8, 0), 12);
        assert_eq!(proj.grid.get(8, 8, 0), 12);
    }
}

