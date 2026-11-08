use crate::material::Material;
use crate::palette::Palette;
use crate::voxel::VoxelGrid;

const MAX_UNDO_STEPS: usize = 100;

/// Run-length encoded snapshot of a VoxelGrid + Palette for compact Undo/Redo (up to 100 steps)
#[derive(Debug, Clone)]
struct CompressedSnapshot {
    size_x: u32,
    size_y: u32,
    size_z: u32,
    rle_voxels: Vec<(u16, u8)>,
    palette: Palette,
    materials: Box<[Material; 256]>,
}

impl CompressedSnapshot {
    fn capture(grid: &VoxelGrid, palette: &Palette, materials: &[Material; 256]) -> Self {
        let mut rle = Vec::new();
        let raw = grid.raw_data();
        if !raw.is_empty() {
            let mut cur_val = raw[0];
            let mut run_len: u16 = 1;
            for &b in &raw[1..] {
                if b == cur_val && run_len < u16::MAX {
                    run_len += 1;
                } else {
                    rle.push((run_len, cur_val));
                    cur_val = b;
                    run_len = 1;
                }
            }
            rle.push((run_len, cur_val));
        }
        Self {
            size_x: grid.size_x(),
            size_y: grid.size_y(),
            size_z: grid.size_z(),
            rle_voxels: rle,
            palette: palette.clone(),
            materials: Box::new(*materials),
        }
    }

    fn restore(&self, grid_out: &mut VoxelGrid, palette_out: &mut Palette, materials_out: &mut [Material; 256]) {
        let mut grid = VoxelGrid::new(self.size_x, self.size_y, self.size_z);
        let dst = grid.raw_data_mut();
        let mut pos = 0usize;
        for &(len, val) in &self.rle_voxels {
            let count = len as usize;
            let end = (pos + count).min(dst.len());
            dst[pos..end].fill(val);
            pos = end;
        }
        *grid_out = grid;
        *palette_out = self.palette.clone();
        *materials_out = *self.materials;
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo_stack: Vec<CompressedSnapshot>,
    redo_stack: Vec<CompressedSnapshot>,
}

impl History {
    pub fn record(&mut self, grid: &VoxelGrid, palette: &Palette, materials: &[Material; 256]) {
        if self.undo_stack.len() >= MAX_UNDO_STEPS {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(CompressedSnapshot::capture(grid, palette, materials));
        self.redo_stack.clear();
    }

    pub fn undo(
        &mut self,
        current_grid: &mut VoxelGrid,
        current_palette: &mut Palette,
        current_materials: &mut [Material; 256],
    ) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack
                .push(CompressedSnapshot::capture(
                current_grid,
                current_palette,
                current_materials,
            ));
            snapshot.restore(current_grid, current_palette, current_materials);
            true
        } else {
            false
        }
    }

    pub fn redo(
        &mut self,
        current_grid: &mut VoxelGrid,
        current_palette: &mut Palette,
        current_materials: &mut [Material; 256],
    ) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack
                .push(CompressedSnapshot::capture(
                current_grid,
                current_palette,
                current_materials,
            ));
            snapshot.restore(current_grid, current_palette, current_materials);
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

