use std::collections::HashMap;

use glam::IVec3;

use crate::brush::BrushSettings;
use crate::command::{execute_console_command, CommandOutcome};
use crate::history::History;
use crate::material::Material;
use crate::palette::Palette;
use crate::scene::{NodeId, Scene, SceneError, SceneStructure, Transform};
use crate::selection::{SelectionMask, VoxelClipboard};
use crate::voxel::VoxelGrid;

const MAX_WORLD_UNDO: usize = 100;

#[derive(Debug, Clone)]
pub struct EditorProject {
    pub name: String,
    /// The model being edited (Model Editor). It is "checked out" of `scene.models`:
    /// `scene.models[active_model]` may be stale while editing, so always read models
    /// through [`EditorProject::model`] or call [`EditorProject::commit_active_model`].
    pub grid: VoxelGrid,
    /// World Editor scene graph (all models, nodes, layers).
    pub scene: Scene,
    pub active_model: usize,
    /// Shape whose animation frames the Model Editor timeline navigates (the shape that
    /// was opened from the World Editor). `None` if it was deleted.
    pub active_shape: Option<NodeId>,
    /// Undo histories of the models that are not active (restored on switch).
    pub model_histories: HashMap<usize, History>,
    /// World Editor undo/redo: scene structure only (never voxel content).
    pub world_undo: Vec<SceneStructure>,
    pub world_redo: Vec<SceneStructure>,
    pub palette: Palette,
    pub materials: [Material; 256],
    pub selection: SelectionMask,
    pub clipboard: VoxelClipboard,
    pub brush: BrushSettings,
    pub history: History,
    pub console_history: Vec<String>,
    pub console_logs: Vec<String>,
    pub dirty: bool,
    /// Number of recorded modifications (voxel or World undo steps) since creation or
    /// load. Drives the auto-backup.
    pub edit_count: u64,
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

        let scene = Scene::single(grid.clone());
        let active_shape = scene.shapes_under(scene.root()).first().copied();
        Self {
            name: "monu_rust".to_string(),
            scene,
            active_model: 0,
            active_shape,
            model_histories: HashMap::new(),
            world_undo: Vec::new(),
            world_redo: Vec::new(),
            grid,
            palette: Palette::default(),
            materials,
            selection: SelectionMask::new(d.x, d.y, d.z),
            clipboard: VoxelClipboard::default(),
            brush: BrushSettings::default(),
            history: History::default(),
            console_history: Vec::new(),
            console_logs: vec![
                "MagicaVoxel Rust Edition ready. Press F1 to focus the console, type 'help' for commands. TAB switches Model / World editors."
                    .to_string(),
            ],
            dirty: false,
            edit_count: 0,
        }
    }
}

impl EditorProject {
    pub fn new_empty(size_x: u32, size_y: u32, size_z: u32) -> Self {
        let grid = VoxelGrid::new(size_x, size_y, size_z);
        let d = grid.dimensions();
        let scene = Scene::single(grid.clone());
        let active_shape = scene.shapes_under(scene.root()).first().copied();
        Self {
            name: "untitled".to_string(),
            scene,
            active_model: 0,
            active_shape,
            grid,
            world_undo: Vec::new(),
            world_redo: Vec::new(),
            selection: SelectionMask::new(d.x, d.y, d.z),
            // Plain materials: the demo project's emissive/metal/glass presets must not
            // leak into new projects or into files that do not override every key.
            materials: Material::default_table(),
            ..Default::default()
        }
    }

    /// Up-to-date content of a model (the edited grid for the active model).
    pub fn model(&self, model_id: usize) -> Option<&VoxelGrid> {
        if model_id == self.active_model {
            Some(&self.grid)
        } else {
            self.scene.models.get(model_id)
        }
    }

    /// Write the edited grid back into `scene.models`.
    pub fn commit_active_model(&mut self) {
        if let Some(slot) = self.scene.models.get_mut(self.active_model) {
            if slot.revision() != self.grid.revision() {
                *slot = self.grid.clone();
            }
        }
    }

    /// Switch the Model Editor to another model of the scene. The undo history and the
    /// selection are per-model and therefore reset. Returns `false` for an unknown model.
    pub fn set_active_model(&mut self, model_id: usize) -> bool {
        if model_id >= self.scene.models.len() {
            return false;
        }
        if model_id == self.active_model {
            return true;
        }
        self.commit_active_model();
        let previous = std::mem::take(&mut self.history);
        self.model_histories.insert(self.active_model, previous);
        self.active_model = model_id;
        self.grid = self.scene.models[model_id].clone();
        let d = self.grid.dimensions();
        self.selection = SelectionMask::new(d.x, d.y, d.z);
        self.history = self.model_histories.remove(&model_id).unwrap_or_default();
        true
    }

    /// Edit `shape` in the Model Editor at the current animation frame.
    pub fn open_shape(&mut self, shape: NodeId) -> bool {
        match self.scene.shape_model(shape) {
            Some(model) if self.set_active_model(model) => {
                self.active_shape = Some(shape);
                true
            }
            _ => false,
        }
    }

    /// Re-point the edited model at the active shape's model for the current frame
    /// (after a frame change or a World undo). Clears `active_shape` if it is gone.
    pub fn refresh_active_model(&mut self) {
        if let Some(shape) = self.active_shape {
            match self.scene.shape_model(shape) {
                Some(model) => {
                    self.set_active_model(model);
                }
                None => self.active_shape = None,
            }
        }
    }

    /// Animation frames of the active shape (1 if there is none).
    pub fn active_frame_count(&self) -> usize {
        self.active_shape
            .and_then(|s| self.scene.node(s))
            .map_or(1, |n| n.frame_models().len().max(1))
    }

    /// Index of the current frame within the active shape's animation.
    pub fn active_frame_index(&self) -> usize {
        self.scene.current_frame as usize % self.active_frame_count()
    }

    /// Show animation frame `frame` everywhere and edit the active shape's model for it.
    pub fn set_frame(&mut self, frame: u32) {
        self.scene.current_frame = frame;
        self.refresh_active_model();
    }

    /// Timeline `+`: insert a frame after the current one in the active shape, as a copy
    /// of the current frame (`empty` = blank model of the same size). Records a World
    /// undo step. Returns the new frame index.
    pub fn add_frame(&mut self, empty: bool) -> Option<usize> {
        self.scene.node(self.active_shape?)?;
        self.record_world_undo();
        let index = self.add_frame_unrecorded(empty);
        if index.is_none() {
            self.world_undo.pop();
        }
        index
    }

    /// [`EditorProject::add_frame`] without the undo step (for batch generation).
    pub fn add_frame_unrecorded(&mut self, empty: bool) -> Option<usize> {
        let shape = self.active_shape?;
        self.scene.node(shape)?;
        let index = self.active_frame_index() + 1;
        let d = self.grid.dimensions();
        let grid = if empty {
            VoxelGrid::new(d.x, d.y, d.z)
        } else {
            self.grid.clone()
        };
        let model = self.scene.add_model(grid);
        self.scene.insert_frame(shape, index, model).ok()?;
        self.set_frame(index as u32);
        Some(index)
    }

    /// Timeline `-`: delete the current frame of the active shape (keeps at least one).
    /// Records a World undo step. Returns whether a frame was removed.
    pub fn delete_frame(&mut self) -> bool {
        let Some(shape) = self.active_shape else {
            return false;
        };
        if self.active_frame_count() <= 1 {
            return false;
        }
        let index = self.active_frame_index();
        self.record_world_undo();
        if self.scene.remove_frame(shape, index) != Ok(true) {
            self.world_undo.pop();
            return false;
        }
        let count = self.active_frame_count();
        self.set_frame(index.min(count - 1) as u32);
        true
    }

    /// Call before any World Editor operation that changes the scene graph or layers.
    pub fn record_world_undo(&mut self) {
        self.push_world_undo(self.scene.structure());
    }

    /// Push a structure captured before a change (UIs that detect changes afterwards).
    pub fn push_world_undo(&mut self, before: SceneStructure) {
        if self.world_undo.len() >= MAX_WORLD_UNDO {
            self.world_undo.remove(0);
        }
        self.world_undo.push(before);
        self.world_redo.clear();
        self.dirty = true;
        self.edit_count += 1;
    }

    pub fn world_undo(&mut self) -> bool {
        match self.world_undo.pop() {
            Some(prev) => {
                self.world_redo.push(self.scene.structure());
                self.scene.restore_structure(prev);
                true
            }
            None => false,
        }
    }

    pub fn world_redo(&mut self) -> bool {
        match self.world_redo.pop() {
            Some(next) => {
                self.world_undo.push(self.scene.structure());
                self.scene.restore_structure(next);
                true
            }
            None => false,
        }
    }

    /// `Ctrl+N`: new empty model of the given size, resting on the ground at the origin
    /// of `parent`. Records a World undo step. Returns the new shape.
    pub fn add_new_object(
        &mut self,
        parent: NodeId,
        size_x: u32,
        size_y: u32,
        size_z: u32,
    ) -> Result<NodeId, SceneError> {
        self.record_world_undo();
        let model = self.scene.add_model(VoxelGrid::new(size_x, size_y, size_z));
        let shape = self
            .scene
            .add_shape(parent, model, Transform::IDENTITY)
            .and_then(|id| self.scene.move_to_ground(id).map(|_| id));
        if shape.is_err() {
            self.world_undo.pop();
        }
        shape
    }

    pub fn record_undo(&mut self) {
        self.history
            .record(&self.grid, &self.palette, &self.materials);
        self.dirty = true;
        self.edit_count += 1;
    }

    pub fn undo(&mut self) -> bool {
        let ok = self
            .history
            .undo(&mut self.grid, &mut self.palette, &mut self.materials);
        if ok {
            let d = self.grid.dimensions();
            self.selection.resize(d.x, d.y, d.z);
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self
            .history
            .redo(&mut self.grid, &mut self.palette, &mut self.materials);
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
                self.console_logs
                    .push(format!("> {} : [ERR] {}", trimmed, err));
            }
            _ => {}
        }
        res
    }
}
