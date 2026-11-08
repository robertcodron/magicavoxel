pub mod brush;
pub mod command;
pub mod history;
pub mod material;
pub mod palette;
pub mod project;
pub mod scene;
pub mod selection;
pub mod voxel;

pub use brush::{BrushAction, BrushMode, BrushSettings, VoxelSubShape};
pub use command::{execute_console_command, CommandOutcome};
pub use history::History;
pub use material::{Material, MaterialType};
pub use palette::{Palette, Rgba};
pub use project::EditorProject;
pub use scene::{
    NodeId, NodeKind, Rotation, Scene, SceneError, SceneLayer, SceneNode, SceneStructure, Transform,
};
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

    #[test]
    fn test_grid_revision_tracks_mutations() {
        let mut grid = VoxelGrid::new(8, 8, 8);
        let r0 = grid.revision();
        assert_eq!(grid.clone().revision(), r0, "clone keeps identical content id");

        grid.set(1, 1, 1, 3);
        let r1 = grid.revision();
        assert_ne!(r1, r0);

        grid.rot90(Axis::Z);
        assert_ne!(grid.revision(), r1);

        // A freshly constructed grid never reuses an existing revision.
        assert_ne!(VoxelGrid::new(8, 8, 8).revision(), grid.revision());
        // Equality ignores the revision.
        assert_eq!(VoxelGrid::new(4, 4, 4), VoxelGrid::new(4, 4, 4));
    }

    #[test]
    fn test_undo_restores_materials() {
        let mut proj = EditorProject::new_empty(8, 8, 8);
        let original = proj.materials[10];

        proj.record_undo();
        proj.materials[10].kind = MaterialType::Metal;
        proj.materials[10].rough = 0.42;
        let edited = proj.materials[10];

        assert!(proj.undo());
        assert_eq!(proj.materials[10], original);
        assert!(proj.redo());
        assert_eq!(proj.materials[10], edited);
    }

    #[test]
    fn test_switching_active_model_keeps_edits() {
        let mut proj = EditorProject::new_empty(4, 4, 4);
        let other = proj.scene.add_model(VoxelGrid::new(2, 2, 2));
        let root = proj.scene.root();
        proj.scene
            .add_shape(root, other, scene::Transform::IDENTITY)
            .unwrap();

        proj.grid.set(1, 1, 1, 9);
        assert!(proj.set_active_model(other));
        assert_eq!(proj.grid.dimensions().to_array(), [2, 2, 2]);
        assert_eq!(proj.model(0).unwrap().get(1, 1, 1), 9, "edit committed to scene");

        proj.grid.set(0, 0, 0, 3);
        assert_eq!(proj.model(other).unwrap().get(0, 0, 0), 3, "live view of active model");
        assert!(proj.set_active_model(0));
        assert_eq!(proj.grid.get(1, 1, 1), 9);
        assert!(!proj.set_active_model(42));
    }

    #[test]
    fn test_world_undo_redo_structure_only() {
        let mut proj = EditorProject::new_empty(4, 4, 4);
        let root = proj.scene.root();
        let shape = proj.add_new_object(root, 10, 10, 6).unwrap();
        assert_eq!(proj.scene.world_bounds(shape).unwrap().0.z, 0, "rests on ground");
        assert_eq!(proj.scene.shapes_under(root).len(), 2);

        proj.grid.set(1, 1, 1, 4); // Model Editor edit after the world op
        assert!(proj.world_undo());
        assert_eq!(proj.scene.shapes_under(root).len(), 1);
        assert_eq!(proj.grid.get(1, 1, 1), 4);
        assert!(proj.world_redo());
        assert_eq!(proj.scene.shapes_under(root).len(), 2);
        assert!(!proj.world_redo());
    }
}
