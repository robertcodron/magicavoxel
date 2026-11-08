//! Visible shapes of the scene with their world placement, for the mesh / point
//! exporters (same convention as the in-app renderers and the glTF export).

use glam::{IVec3, Vec3};
use mv_core::{EditorProject, Transform, VoxelGrid};

pub(crate) struct PlacedShape<'a> {
    pub name: String,
    pub model_id: usize,
    pub grid: &'a VoxelGrid,
    /// Model voxel coordinates -> world voxel coordinates.
    pub voxel_xf: Transform,
}

impl PlacedShape<'_> {
    /// World position of a continuous model-space point (voxel corners): negated axes
    /// are shifted by one so voxel cells land on the integer world cells of `voxel_xf`.
    pub fn world_point(&self, p: [f32; 3]) -> Vec3 {
        let r = self.voxel_xf.rotation;
        let flipped = r.apply_f32(Vec3::ONE);
        let shift = Vec3::new(
            (flipped.x < 0.0) as i32 as f32,
            (flipped.y < 0.0) as i32 as f32,
            (flipped.z < 0.0) as i32 as f32,
        );
        r.apply_f32(Vec3::from_array(p)) + self.voxel_xf.translation.as_vec3() + shift
    }

    pub fn world_normal(&self, n: [i32; 3]) -> IVec3 {
        self.voxel_xf.rotation.apply(IVec3::from_array(n))
    }
}

/// Visible shapes (node, ancestors and layer not hidden) at the current animation
/// frame, in scene order.
pub(crate) fn visible_shapes(project: &EditorProject) -> Vec<PlacedShape<'_>> {
    let scene = &project.scene;
    scene
        .shapes_under(scene.root())
        .into_iter()
        .filter(|&s| scene.is_visible(s))
        .filter_map(|s| {
            let model_id = scene.shape_model(s)?;
            let grid = project.model(model_id)?;
            let voxel_xf = scene.shape_voxel_transform(s).ok()?;
            let name = scene
                .node(s)
                .and_then(|n| n.name.clone())
                .unwrap_or_else(|| format!("model_{model_id}_node_{s}"));
            Some(PlacedShape {
                name,
                model_id,
                grid,
                voxel_xf,
            })
        })
        .collect()
}
