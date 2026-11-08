use glam::IVec3;

use crate::brush::BrushSettings;
use crate::command::{execute_console_command, CommandOutcome};
use crate::history::History;
use crate::material::Material;
use crate::palette::Palette;
use crate::selection::{SelectionMask, VoxelClipboard};
use crate::voxel::VoxelGrid;

#[derive(Debug, Clone)]
pub struct EditorProject {
    pub name: String,
    pub grid: VoxelGrid,
    pub palette: Palette,
    pub materials: [Material; 256],
    pub selection: SelectionMask,
    pub clipboard: VoxelClipboard,
    pub brush: BrushSettings,
    pub history: History,
    pub console_history: Vec<String>,
    pub console_logs: Vec<String>,
    pub dirty: bool,
}

impl Default for EditorProject {
    fn default() -> Self {
        let grid = VoxelGrid::sample_monument();
        let d = grid.dimensions();
        let mut materials = Material::default_table();
        // Glowing core crystal (#19) -> Emissive material
        materials[19].kind = crate::material::MaterialType::Emit;
        materials[19].emit = 2.8;
        materials[19].flux = 2.0;
        // Golden/metallic roof (#217) -> Metal material
        materials[217].kind = crate::material::MaterialType::Metal;
        materials[217].rough = 0.12;
        materials[217].metal = 0.9;
        materials[217].spec = 0.85;
        // Corner pillars (#79) -> Glass material
        materials[79].kind = crate::material::MaterialType::Glass;
        materials[79].rough = 0.05;
        materials[79].ior = 1.52;
        materials[79].alpha = 0.35;

        Self {
            name: "monu_rust".to_string(),
            grid,
            palette: Palette::default(),
            materials,
            selection: SelectionMask::new(d.x, d.y, d.z),
            clipboard: VoxelClipboard::default(),
            brush: BrushSettings::default(),
            history: History::default(),
            console_history: Vec::new(),
            console_logs: vec![
                "MagicaVoxel Rust Edition ready. Press TAB or F1 to focus console, type 'help' for commands."
                    .to_string(),
            ],
            dirty: false,
        }
    }
}

impl EditorProject {
    pub fn new_empty(size_x: u32, size_y: u32, size_z: u32) -> Self {
        let grid = VoxelGrid::new(size_x, size_y, size_z);
        let d = grid.dimensions();
        Self {
            name: "untitled".to_string(),
            grid,
            selection: SelectionMask::new(d.x, d.y, d.z),
            ..Default::default()
        }
    }

    pub fn record_undo(&mut self) {
        self.history.record(&self.grid, &self.palette);
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.history.undo(&mut self.grid, &mut self.palette);
        if ok {
            let d = self.grid.dimensions();
            self.selection.resize(d.x, d.y, d.z);
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.history.redo(&mut self.grid, &mut self.palette);
        if ok {
            let d = self.grid.dimensions();
            self.selection.resize(d.x, d.y, d.z);
        }
        ok
    }

    pub fn resize_canvas(&mut self, sx: u32, sy: u32, sz: u32) {
        self.grid.resize(sx, sy, sz);
        let d = self.grid.dimensions();
        self.selection.resize(d.x, d.y, d.z);
    }

    pub fn copy_selected(&mut self) -> usize {
        self.clipboard.voxels.clear();
        let any_sel = self.selection.any_selected();
        for z in 0..self.grid.size_z() {
            for y in 0..self.grid.size_y() {
                for x in 0..self.grid.size_x() {
                    let v = self.grid.get(x, y, z);
                    if v != 0 && (!any_sel || self.selection.get(x, y, z)) {
                        self.clipboard
                            .voxels
                            .push((IVec3::new(x as i32, y as i32, z as i32), v));
                    }
                }
            }
        }
        self.clipboard.voxels.len()
    }

    pub fn cut_selected(&mut self) -> usize {
        let count = self.copy_selected();
        let any_sel = self.selection.any_selected();
        for z in 0..self.grid.size_z() {
            for y in 0..self.grid.size_y() {
                for x in 0..self.grid.size_x() {
                    if !any_sel || self.selection.get(x, y, z) {
                        self.grid.set(x, y, z, 0);
                    }
                }
            }
        }
        self.selection.clear();
        count
    }

    pub fn delete_selected(&mut self) -> usize {
        let any_sel = self.selection.any_selected();
        if !any_sel {
            return 0;
        }
        self.record_undo();
        let mut deleted = 0;
        for z in 0..self.grid.size_z() {
            for y in 0..self.grid.size_y() {
                for x in 0..self.grid.size_x() {
                    if self.selection.get(x, y, z) && self.grid.get(x, y, z) != 0 {
                        self.grid.set(x, y, z, 0);
                        deleted += 1;
                    }
                }
            }
        }
        self.selection.clear();
        deleted
    }

    pub fn paste_clipboard(&mut self) -> usize {
        let mut count = 0;
        for &(pos, color) in &self.clipboard.voxels {
            if self.grid.in_bounds(pos.x, pos.y, pos.z) {
                self.grid.set_i32(pos.x, pos.y, pos.z, color);
                count += 1;
            }
        }
        count
    }

    pub fn run_command(&mut self, cmd_line: &str) -> Result<CommandOutcome, String> {
        let trimmed = cmd_line.trim();
        if !trimmed.is_empty() {
            self.console_history.push(trimmed.to_string());
        }
        let res = execute_console_command(self, trimmed);
        match &res {
            Ok(CommandOutcome::Message(msg)) if !msg.is_empty() => {
                self.console_logs.push(format!("> {} : {}", trimmed, msg));
            }
            Ok(CommandOutcome::ExportRequested { format, .. }) => {
                self.console_logs
                    .push(format!("> {} : Export requested ({})", trimmed, format));
            }
            Err(err) => {
                self.console_logs.push(format!("> {} : [ERR] {}", trimmed, err));
            }
            _ => {}
        }
        res
    }
}

