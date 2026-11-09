//! World Editor: state, scene operations shared by shortcuts / panel / viewport, the
//! viewport interaction, and the per-frame undo recording.
//!
//! Undo model: at the start of each World Editor frame the scene structure is
//! snapshotted; if it changed by the end of the frame, the snapshot becomes one undo
//! step. A continuous gesture (dragging an object, dragging a value, typing a name)
//! only records once, when it starts. Operations therefore never record undo
//! themselves.

use std::collections::{BTreeSet, HashSet};

use eframe::egui::{self, Color32, PointerButton, Response};
use glam::{IVec3, Vec2, Vec3};
use mv_core::{Axis, NodeId, NodeKind, SceneStructure};
use mv_render::{pick_world, render_world_rgba, Highlight, WorldView};

use crate::app::{MagicaVoxelApp, ViewportMode};

/// Size of a model created with `Ctrl+N`.
const NEW_OBJECT_SIZE: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GizmoAxis {
    X,
    Y,
    Z,
    PlaneXY,
    Surface,
}

pub(crate) struct WorldDrag {
    pub(crate) axis: GizmoAxis,
    pub(crate) center: Vec3,
    pub(crate) start_param: f32,
    pub(crate) plane_z: f32,
    pub(crate) start: Vec3,
    pub(crate) applied: IVec3,
}

pub(crate) struct WorldEditorState {
    /// Selected nodes; always direct children of `group`.
    pub(crate) selection: BTreeSet<NodeId>,
    /// Group being edited (`TAB` enters a group, `Shift+TAB` leaves it).
    pub(crate) group: NodeId,
    pub(crate) hover: Option<NodeId>,
    pub(crate) gizmo_hover: Option<GizmoAxis>,
    pub(crate) drag: Option<WorldDrag>,
    pub(crate) collapsed: HashSet<NodeId>,
    gesture_in_progress: bool,
    /// Set when the scene changed for a reason that must not create an undo step
    /// (undo/redo itself, opening a file).
    pub(crate) skip_record: bool,
}

impl WorldEditorState {
    pub(crate) fn new(root: NodeId) -> Self {
        Self {
            selection: BTreeSet::new(),
            group: root,
            hover: None,
            gizmo_hover: None,
            drag: None,
            collapsed: HashSet::new(),
            gesture_in_progress: false,
            skip_record: true,
        }
    }
}

/// Dominant world axis (±X or ±Y) of a horizontal direction.
fn dominant_xy_axis(v: Vec3) -> IVec3 {
    if v.x.abs() >= v.y.abs() {
        IVec3::new(v.x.signum() as i32, 0, 0)
    } else {
        IVec3::new(0, v.y.signum() as i32, 0)
    }
}

/// Closest coordinate along `axis_dir` from the ray `(origin, dir)`.
fn axis_parameter(origin: Vec3, dir: Vec3, center: Vec3, axis_dir: Vec3) -> Option<f32> {
    let w0 = origin - center;
    let b = dir.dot(axis_dir);
    let d = dir.dot(w0);
    let e = axis_dir.dot(w0);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-4 {
        return None;
    }
    Some((b * d - e) / denom)
}

/// Distance from point `p` to 2D line segment `(a, b)`.
fn dist_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-4 {
        return (p - a).length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

impl MagicaVoxelApp {
    // -----------------------------------------------------------------------
    // State & mode switching
    // -----------------------------------------------------------------------

    pub(crate) fn reset_world_state(&mut self) {
        self.world = WorldEditorState::new(self.project.scene.root());
        self.anim_playing = false;
    }

    /// Drop references to nodes that no longer exist (after undo, delete, union...).
    pub(crate) fn sanitize_world_state(&mut self) {
        let scene = &self.project.scene;
        let group_ok = scene
            .node(self.world.group)
            .is_some_and(|n| matches!(n.kind, NodeKind::Group { .. }));
        if !group_ok {
            self.world.group = scene.root();
        }
        let group = self.world.group;
        self.world
            .selection
            .retain(|&id| scene.node(id).is_some_and(|n| n.parent == Some(group)));
        if self.world.hover.is_some_and(|h| scene.node(h).is_none()) {
            self.world.hover = None;
        }
        self.world.collapsed.retain(|&id| scene.node(id).is_some());
    }

    fn single_selection(&self) -> Option<NodeId> {
        (self.world.selection.len() == 1)
            .then(|| self.world.selection.iter().next().copied())
            .flatten()
    }

    fn selection_vec(&self) -> Vec<NodeId> {
        self.world.selection.iter().copied().collect()
    }

    fn log(&mut self, msg: impl Into<String>) {
        self.project.console_logs.push(msg.into());
    }

    pub(crate) fn enter_world_editor(&mut self) {
        if self.mode == ViewportMode::WorldEditor {
            return;
        }
        // The Model Editor camera is in model space; the renderer's is already in world
        // space and is kept.
        let from_model = self.mode == ViewportMode::ModelEditor;
        self.mode = ViewportMode::WorldEditor;
        self.world.skip_record = true;
        self.sanitize_world_state();
        if from_model {
            self.frame_world_camera();
        }
    }

    /// Path-traced view of the whole scene (world space). Coming from the Model Editor,
    /// frame the object being edited.
    pub(crate) fn enter_render_mode(&mut self) {
        if self.mode == ViewportMode::PathTracer {
            return;
        }
        if self.mode == ViewportMode::ModelEditor {
            let scene = &self.project.scene;
            let target = self
                .project
                .active_shape
                .filter(|&s| scene.node(s).is_some())
                .unwrap_or(scene.root());
            if let Some((lo, hi)) = scene
                .world_bounds(target)
                .or_else(|| scene.world_bounds(scene.root()))
            {
                self.camera
                    .frame_box(lo.as_vec3(), (hi + IVec3::ONE).as_vec3());
            }
        }
        self.mode = ViewportMode::PathTracer;
        self.pathtracer.reset_accumulation();
    }

    /// Open a shape's model in the Model Editor.
    pub(crate) fn edit_shape_in_model_editor(&mut self, shape: NodeId) {
        if self.project.open_shape(shape) {
            self.mode = ViewportMode::ModelEditor;
            self.camera.recenter(&self.project.grid);
            self.sync_size_input();
            self.pathtracer.reset_accumulation();
        }
    }

    /// Model Editor button / `TAB` without selection: edit the selected shape if there is
    /// exactly one, else keep the active model.
    pub(crate) fn enter_model_editor(&mut self) {
        let from_world = self.mode == ViewportMode::WorldEditor;
        match self.single_selection().filter(|_| from_world) {
            Some(s)
                if self
                    .project
                    .scene
                    .node(s)
                    .and_then(|n| n.model_id())
                    .is_some() =>
            {
                self.edit_shape_in_model_editor(s);
            }
            _ => {
                if self.mode != ViewportMode::ModelEditor {
                    // World / render cameras are in world space.
                    self.camera.recenter(&self.project.grid);
                }
                self.mode = ViewportMode::ModelEditor;
            }
        }
    }

    pub(crate) fn enter_group(&mut self, group: NodeId) {
        if matches!(
            self.project.scene.node(group).map(|n| &n.kind),
            Some(NodeKind::Group { .. })
        ) {
            self.world.group = group;
            self.world.selection.clear();
        }
    }

    pub(crate) fn leave_group(&mut self) {
        let group = self.world.group;
        if let Some(parent) = self.project.scene.node(group).and_then(|n| n.parent) {
            self.world.group = parent;
            self.world.selection = BTreeSet::from([group]);
        }
    }

    /// `TAB` (`shift` = `Shift+TAB`), following `mv_controls.html`.
    pub(crate) fn handle_tab(&mut self, shift: bool) {
        match self.mode {
            ViewportMode::WorldEditor if shift => self.leave_group(),
            ViewportMode::WorldEditor => match self.single_selection() {
                Some(id)
                    if matches!(
                        self.project.scene.node(id).map(|n| &n.kind),
                        Some(NodeKind::Group { .. })
                    ) =>
                {
                    self.enter_group(id)
                }
                _ => self.enter_model_editor(),
            },
            ViewportMode::ModelEditor | ViewportMode::PathTracer => self.enter_world_editor(),
        }
    }

    /// `F4` in the World Editor: frame the selection, or the whole scene.
    pub(crate) fn frame_world_camera(&mut self) {
        let scene = &self.project.scene;
        let ids: Vec<NodeId> = if self.world.selection.is_empty() {
            vec![scene.root()]
        } else {
            self.world.selection.iter().copied().collect()
        };
        let bounds = ids
            .iter()
            .filter_map(|&id| scene.world_bounds(id))
            .reduce(|(l1, h1), (l2, h2)| (l1.min(l2), h1.max(h2)));
        if let Some((lo, hi)) = bounds {
            self.camera
                .frame_box(lo.as_vec3(), (hi + IVec3::ONE).as_vec3());
        }
    }

    // -----------------------------------------------------------------------
    // Scene operations (no undo recording: see module docs)
    // -----------------------------------------------------------------------

    pub(crate) fn world_move(&mut self, delta: IVec3) {
        for id in self.selection_vec() {
            let _ = self.project.scene.translate_world(id, delta);
        }
    }

    pub(crate) fn world_rotate(&mut self, axis: Axis) {
        for id in self.selection_vec() {
            let _ = self.project.scene.rotate90(id, axis);
        }
    }

    pub(crate) fn world_ground(&mut self) {
        for id in self.selection_vec() {
            let _ = self.project.scene.move_to_ground(id);
        }
    }

    pub(crate) fn world_set_hidden(&mut self, hidden: bool) {
        for id in self.selection_vec() {
            if let Some(n) = self.project.scene.node_mut(id) {
                n.hidden = hidden;
            }
        }
    }

    /// `Ctrl+Shift+H`: show every object of the current group.
    pub(crate) fn world_unhide_all(&mut self) {
        let group = self.world.group;
        for id in self.project.scene.subtree(group) {
            if id != group {
                if let Some(n) = self.project.scene.node_mut(id) {
                    n.hidden = false;
                }
            }
        }
    }

    pub(crate) fn world_select_all(&mut self) {
        let group = self.world.group;
        if let Some(n) = self.project.scene.node(group) {
            self.world.selection = n.children().iter().copied().collect();
        }
    }

    pub(crate) fn world_delete(&mut self) {
        for id in self.selection_vec() {
            let _ = self.project.scene.remove_node(id);
        }
        self.world.selection.clear();
    }

    pub(crate) fn world_new_object(&mut self) {
        let s = NEW_OBJECT_SIZE;
        let group = self.world.group;
        let model = self
            .project
            .scene
            .add_model(mv_core::VoxelGrid::new(s, s, s));
        match self
            .project
            .scene
            .add_shape(group, model, mv_core::Transform::IDENTITY)
        {
            Ok(id) => {
                let _ = self.project.scene.move_to_ground(id);
                self.world.selection = BTreeSet::from([id]);
            }
            Err(e) => self.log(format!("[ERR] New object: {e}")),
        }
    }

    /// Spawn a prefabricated pattern model into the scene.
    pub(crate) fn spawn_pattern(&mut self, kind: &str) {
        let group = self.world.group;
        let col = self.project.palette.selected.max(1);
        let grid = match kind {
            "Arch" => mv_core::VoxelGrid::sample_arch(col),
            "Column" => mv_core::VoxelGrid::sample_column(col),
            "Stairs" => mv_core::VoxelGrid::sample_stairs(col),
            "Wall" => mv_core::VoxelGrid::sample_wall(col),
            "Tree" => mv_core::VoxelGrid::sample_tree(),
            "Monument" => mv_core::VoxelGrid::sample_monument(),
            _ => mv_core::VoxelGrid::new(NEW_OBJECT_SIZE, NEW_OBJECT_SIZE, NEW_OBJECT_SIZE),
        };
        let m = self.project.scene.add_model(grid);
        let t = mv_core::Transform::from_translation(self.camera.target.round().as_ivec3());
        match self.project.scene.add_shape(group, m, t) {
            Ok(id) => {
                let _ = self.project.scene.move_to_ground(id);
                if let Some(n) = self.project.scene.node_mut(id) {
                    n.name = Some(kind.to_string());
                }
                self.world.selection = BTreeSet::from([id]);
                self.log(format!("Spawned pattern: {kind}"));
            }
            Err(e) => self.log(format!("[ERR] Pattern: {e}")),
        }
    }

    /// `Ctrl+T`: give each selected shape its own copy of a shared model.
    pub(crate) fn world_reference_to_duplicate(&mut self) {
        let mut converted = 0;
        for id in self.selection_vec() {
            for shape in self.project.scene.shapes_under(id) {
                if let Ok(true) = self.project.scene.convert_reference_to_duplicate(shape) {
                    converted += 1;
                }
            }
        }
        self.log(format!("Converted {converted} reference(s) to duplicate"));
    }

    /// Copy the selected objects (shapes or groups): `duplicate` gives them their own models,
    /// otherwise they reference the same models (instances).
    pub(crate) fn world_copy(&mut self, duplicate: bool) {
        let mut new = BTreeSet::new();
        for id in self.selection_vec() {
            let res = if duplicate {
                self.project.scene.duplicate(id)
            } else {
                self.project.scene.instantiate(id)
            };
            match res {
                Ok(n) => {
                    new.insert(n);
                }
                Err(e) => self.log(format!("[ERR] Copy: {e}")),
            }
        }
        if !new.is_empty() {
            self.world.selection = new;
        }
    }

    pub(crate) fn world_group(&mut self) {
        let ids = self.selection_vec();
        if ids.is_empty() {
            return;
        }
        match self.project.scene.group_nodes(&ids) {
            Ok(g) => self.world.selection = BTreeSet::from([g]),
            Err(e) => self.log(format!("[ERR] Group: {e}")),
        }
    }

    pub(crate) fn world_ungroup(&mut self) {
        let mut freed = BTreeSet::new();
        for id in self.selection_vec() {
            if let Ok(children) = self.project.scene.ungroup(id) {
                freed.extend(children);
            }
        }
        if !freed.is_empty() {
            self.world.selection = freed;
        }
    }

    /// `U`: merge the selected objects into one new object in the current group.
    pub(crate) fn world_union(&mut self) {
        let ids = self.selection_vec();
        if ids.is_empty() {
            return;
        }
        match self.project.scene.union(&ids) {
            Ok(id) => {
                let group = self.world.group;
                if group != self.project.scene.root() {
                    let _ = self.project.scene.set_parent(id, group);
                }
                self.world.selection = BTreeSet::from([id]);
                self.log("Union: objects merged into one model");
            }
            Err(e) => self.log(format!("[ERR] Union: {e}")),
        }
    }

    pub(crate) fn world_assign_layer(&mut self, layer: usize) {
        for id in self.selection_vec() {
            if let Some(n) = self.project.scene.node_mut(id) {
                n.layer_id = layer;
            }
        }
    }

    /// World undo/redo (`Ctrl+Z` / `Ctrl+Y` in the World Editor).
    pub(crate) fn world_undo_redo(&mut self, redo: bool) {
        let ok = if redo {
            self.project.world_redo()
        } else {
            self.project.world_undo()
        };
        if ok {
            self.world.skip_record = true;
            self.sanitize_world_state();
            // Frames may have been added/removed: keep editing a model that exists.
            self.project.refresh_active_model();
            self.sync_size_input();
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard
    // -----------------------------------------------------------------------

    /// World Editor shortcuts (`mv_controls.html`, "Edit [World Editor]").
    pub(crate) fn handle_world_shortcuts(&mut self, ctx: &egui::Context) {
        #[derive(Clone, Copy)]
        enum A {
            Move(IVec3),
            Rotate(Axis),
            Ground,
            Union,
            Hide(bool),
            UnhideAll,
            New,
            Delete,
            ToDuplicate,
            Group,
            Ungroup,
            SelectAll,
            SelectNone,
        }
        let (_, forward, right, _) = self.camera.basis();
        let fwd = dominant_xy_axis(forward);
        let rgt = dominant_xy_axis(right);

        let actions: Vec<A> = ctx.input(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            let shift = i.modifiers.shift;
            let step = if shift && !cmd { 10 } else { 1 };
            let mut out = Vec::new();
            let mut key = |k: egui::Key, a: A| {
                if i.key_pressed(k) {
                    out.push(a);
                }
            };
            if cmd {
                key(
                    egui::Key::H,
                    if shift { A::UnhideAll } else { A::Hide(true) },
                );
                key(egui::Key::N, A::New);
                key(egui::Key::T, A::ToDuplicate);
                key(egui::Key::R, if shift { A::Ungroup } else { A::Group });
                key(egui::Key::A, A::SelectAll);
                key(egui::Key::D, A::SelectNone);
            } else {
                // Arrows move in the camera's frame, snapped to world axes.
                key(egui::Key::ArrowUp, A::Move(fwd * step));
                key(egui::Key::ArrowDown, A::Move(-fwd * step));
                key(egui::Key::ArrowRight, A::Move(rgt * step));
                key(egui::Key::ArrowLeft, A::Move(-rgt * step));
                key(egui::Key::PageUp, A::Move(IVec3::Z * step));
                key(egui::Key::PageDown, A::Move(-IVec3::Z * step));
                key(egui::Key::G, A::Ground);
                key(egui::Key::U, A::Union);
                // -/+ rotate around Z; with Shift around X, with Alt around Y.
                let axis = if shift {
                    Axis::X
                } else if i.modifiers.alt {
                    Axis::Y
                } else {
                    Axis::Z
                };
                key(egui::Key::Minus, A::Rotate(axis));
                key(egui::Key::Plus, A::Rotate(axis));
                key(egui::Key::Equals, A::Rotate(axis));
                key(egui::Key::Backspace, A::Delete);
                key(egui::Key::Delete, A::Delete);
            }
            out
        });

        for a in actions {
            match a {
                A::Move(d) => self.world_move(d),
                A::Rotate(axis) => self.world_rotate(axis),
                A::Ground => self.world_ground(),
                A::Union => self.world_union(),
                A::Hide(h) => self.world_set_hidden(h),
                A::UnhideAll => self.world_unhide_all(),
                A::New => self.world_new_object(),
                A::Delete => self.world_delete(),
                A::ToDuplicate => self.world_reference_to_duplicate(),
                A::Group => self.world_group(),
                A::Ungroup => self.world_ungroup(),
                A::SelectAll => self.world_select_all(),
                A::SelectNone => self.world.selection.clear(),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Viewport
    // -----------------------------------------------------------------------

    pub(crate) fn selection_bounds(&self) -> Option<(IVec3, IVec3)> {
        let mut bounds: Option<(IVec3, IVec3)> = None;
        for &id in &self.world.selection {
            if let Some((lo, hi)) = self.project.scene.world_bounds(id) {
                bounds = Some(match bounds {
                    Some((l, h)) => (l.min(lo), h.max(hi)),
                    None => (lo, hi),
                });
            }
        }
        bounds
    }

    /// Picking, selection, drag-move and rendering of the World Editor viewport.
    pub(crate) fn update_world_viewport(
        &mut self,
        ui: &egui::Ui,
        response: &Response,
        rect: egui::Rect,
        render_w: usize,
        render_h: usize,
    ) {
        self.sanitize_world_state();
        let aspect = rect.width() / rect.height().max(1.0);
        let (space, alt, shift, ctrl, x_down) = ui.input(|i| {
            (
                i.key_down(egui::Key::Space),
                i.modifiers.alt,
                i.modifiers.shift,
                i.modifiers.ctrl || i.modifiers.command,
                i.key_down(egui::Key::X),
            )
        });
        let pointer_ndc = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .map(|p| {
                Vec2::new(
                    ((p.x - rect.left()) / rect.width()) * 2.0 - 1.0,
                    1.0 - ((p.y - rect.top()) / rect.height()) * 2.0,
                )
            });

        // --- 3D Translation Gizmo layout ---
        let gizmo_center = self.selection_bounds().map(|(lo, hi)| (lo.as_vec3() + (hi + IVec3::ONE).as_vec3()) * 0.5);
        let gizmo_len = (self.camera.distance * 0.12).clamp(14.0, 140.0);
        let to_screen = |world_pos: Vec3| -> Option<egui::Pos2> {
            let ndc = self.camera.project_to_ndc(world_pos, aspect)?;
            Some(egui::pos2(
                rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
            ))
        };
        let s_c = gizmo_center.and_then(to_screen);
        let s_x = gizmo_center.map(|gc| gc + Vec3::X * gizmo_len).and_then(to_screen);
        let s_y = gizmo_center.map(|gc| gc + Vec3::Y * gizmo_len).and_then(to_screen);
        let s_z = gizmo_center.map(|gc| gc + Vec3::Z * gizmo_len).and_then(to_screen);

        let mouse_pos = response.hover_pos().or_else(|| response.interact_pointer_pos());
        self.world.gizmo_hover = if let (Some(m), Some(sc), Some(sx), Some(sy), Some(sz)) = (mouse_pos, s_c, s_x, s_y, s_z) {
            if (m - sc).length() <= 12.0 {
                Some(GizmoAxis::PlaneXY)
            } else if dist_to_segment(m, sc, sz) <= 8.0 {
                Some(GizmoAxis::Z)
            } else if dist_to_segment(m, sc, sy) <= 8.0 {
                Some(GizmoAxis::Y)
            } else if dist_to_segment(m, sc, sx) <= 8.0 {
                Some(GizmoAxis::X)
            } else {
                None
            }
        } else {
            None
        };

        // --- Picking (on the scene state at the start of the interaction) ---
        let (hit, hovered_obj) = {
            let view = WorldView::build(&self.project, |_| (Highlight::None, false));
            let hit = pointer_ndc.and_then(|ndc| pick_world(&view, &self.camera, ndc, aspect));
            let obj = hit.and_then(|h| {
                self.project
                    .scene
                    .child_of_group_containing(h.node, self.world.group)
            });
            (hit, obj)
        };
        self.world.hover = hovered_obj;

        // --- Clicks ---
        let axis_hovered = matches!(
            self.world.gizmo_hover,
            Some(GizmoAxis::X | GizmoAxis::Y | GizmoAxis::Z)
        );
        if !space && response.double_clicked_by(PointerButton::Primary) {
            if let Some(obj) = hovered_obj {
                match self.project.scene.node(obj).map(|n| &n.kind) {
                    Some(NodeKind::Group { .. }) => self.enter_group(obj),
                    Some(NodeKind::Shape { .. }) => {
                        self.edit_shape_in_model_editor(obj);
                        return;
                    }
                    None => {}
                }
            }
        } else if !space && response.clicked_by(PointerButton::Primary) && !axis_hovered {
            if x_down {
                if let Some(h) = hit {
                    self.camera.target = h.hit_pos;
                }
            } else if alt {
                // Pick the color of the clicked voxel.
                if let Some(h) = hit {
                    let model = self
                        .project
                        .scene
                        .shape_model(h.node)
                        .and_then(|m| self.project.model(m));
                    if let Some(c) = model.map(|g| g.get_i32(h.voxel.x, h.voxel.y, h.voxel.z)) {
                        if c != 0 {
                            self.project.palette.selected = c;
                        }
                    }
                }
            } else {
                match hovered_obj {
                    Some(obj) if shift => {
                        if !self.world.selection.remove(&obj) {
                            self.world.selection.insert(obj);
                        }
                    }
                    Some(obj) => self.world.selection = BTreeSet::from([obj]),
                    None if !shift => self.world.selection.clear(),
                    None => {}
                }
            }
        }

        // --- Drag (Gizmo axes, surface snap, or horizontal plane) ---
        let camera = &self.camera;
        let (ray_o, ray_d) = pointer_ndc
            .map(|ndc| camera.ray_for_ndc(ndc, aspect))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO));

        let plane_point = |plane_z: f32| -> Option<Vec3> {
            let ndc = pointer_ndc?;
            let (o, d) = camera.ray_for_ndc(ndc, aspect);
            if d.z.abs() < 1e-4 {
                return None;
            }
            let t = (plane_z - o.z) / d.z;
            (t > 0.0).then(|| o + d * t)
        };

        if !space && !alt && response.drag_started_by(PointerButton::Primary) {
            if let (Some(axis), Some(gc)) = (self.world.gizmo_hover, gizmo_center) {
                let start_param = match axis {
                    GizmoAxis::X => axis_parameter(ray_o, ray_d, gc, Vec3::X).unwrap_or(0.0),
                    GizmoAxis::Y => axis_parameter(ray_o, ray_d, gc, Vec3::Y).unwrap_or(0.0),
                    GizmoAxis::Z => axis_parameter(ray_o, ray_d, gc, Vec3::Z).unwrap_or(0.0),
                    _ => 0.0,
                };
                self.world.drag = Some(WorldDrag {
                    axis,
                    center: gc,
                    start_param,
                    plane_z: gc.z,
                    start: gc,
                    applied: IVec3::ZERO,
                });
            } else if let (Some(obj), Some(h)) = (hovered_obj, hit) {
                if !self.world.selection.contains(&obj) {
                    self.world.selection = BTreeSet::from([obj]);
                }
                let axis = if ctrl { GizmoAxis::Surface } else { GizmoAxis::PlaneXY };
                self.world.drag = Some(WorldDrag {
                    axis,
                    center: h.hit_pos,
                    start_param: 0.0,
                    plane_z: h.hit_pos.z,
                    start: h.hit_pos,
                    applied: IVec3::ZERO,
                });
            }
        }

        if let Some(drag) = &self.world.drag {
            if response.dragged_by(PointerButton::Primary) {
                let total = match drag.axis {
                    GizmoAxis::X => {
                        let p = axis_parameter(ray_o, ray_d, drag.center, Vec3::X).unwrap_or(drag.start_param);
                        IVec3::new((p - drag.start_param).round() as i32, 0, 0)
                    }
                    GizmoAxis::Y => {
                        let p = axis_parameter(ray_o, ray_d, drag.center, Vec3::Y).unwrap_or(drag.start_param);
                        IVec3::new(0, (p - drag.start_param).round() as i32, 0)
                    }
                    GizmoAxis::Z => {
                        let p = axis_parameter(ray_o, ray_d, drag.center, Vec3::Z).unwrap_or(drag.start_param);
                        IVec3::new(0, 0, (p - drag.start_param).round() as i32)
                    }
                    GizmoAxis::PlaneXY => {
                        if let Some(p) = plane_point(drag.plane_z) {
                            (p - drag.start).round().as_ivec3() * IVec3::new(1, 1, 0)
                        } else {
                            drag.applied
                        }
                    }
                    GizmoAxis::Surface => {
                        let selected_roots = self.world.selection.clone();
                        let scene = &self.project.scene;
                        let view = WorldView::build(&self.project, |_| (Highlight::None, false));
                        let non_sel_hit = pointer_ndc.and_then(|ndc| {
                            let (o, d) = camera.ray_for_ndc(ndc, aspect);
                            let hit = view.raycast(o, d, 4000.0)?;
                            let is_sel = selected_roots.iter().any(|&r| scene.is_ancestor_or_self(r, hit.node));
                            if is_sel {
                                None
                            } else {
                                Some(hit)
                            }
                        });
                        if let Some(sh) = non_sel_hit {
                            let target = sh.hit_pos + sh.normal.as_vec3() * 0.5;
                            (target - drag.start).round().as_ivec3()
                        } else if let Some(p) = plane_point(0.0) {
                            (p - drag.start).round().as_ivec3() * IVec3::new(1, 1, 0)
                        } else {
                            drag.applied
                        }
                    }
                };

                let diff = total - drag.applied;
                if diff != IVec3::ZERO {
                    self.world_move(diff);
                    if let Some(d) = &mut self.world.drag {
                        d.applied = total;
                    }
                }
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.world.drag = None;
            }
        }

        // --- Render ---
        let selected_roots = self.world.selection.clone();
        let hover = self.world.hover;
        let group = self.world.group;
        let root = self.project.scene.root();
        let scene = &self.project.scene;
        let mut view = WorldView::build(&self.project, |shape| {
            let inside = |set_member: NodeId| scene.is_ancestor_or_self(set_member, shape);
            let highlight = if selected_roots.iter().any(|&s| inside(s)) {
                Highlight::Selected
            } else if hover.is_some_and(inside) {
                Highlight::Hovered
            } else {
                Highlight::None
            };
            let dimmed = group != root && !scene.is_ancestor_or_self(group, shape);
            (highlight, dimmed)
        });
        view.frames = selected_roots
            .iter()
            .filter_map(|&id| scene.world_bounds(id))
            .map(|(lo, hi)| (lo.as_vec3(), (hi + IVec3::ONE).as_vec3()))
            .collect();
        render_world_rgba(
            &self.project,
            &view,
            &self.camera,
            &self.display,
            render_w,
            render_h,
            &mut self.rgba_buffer,
        );

        // --- Draw 3D Translation Gizmo ---
        if let (Some(sc), Some(sx), Some(sy), Some(sz)) = (s_c, s_x, s_y, s_z) {
            let painter = ui.painter_at(rect);
            let active_axis = self.world.drag.as_ref().map(|d| d.axis).or(self.world.gizmo_hover);

            let center_col = if active_axis == Some(GizmoAxis::PlaneXY) {
                Color32::YELLOW
            } else {
                Color32::from_rgba_unmultiplied(220, 220, 220, 200)
            };
            painter.circle_stroke(sc, 8.0, egui::Stroke::new(2.5, center_col));

            let x_col = if active_axis == Some(GizmoAxis::X) {
                Color32::YELLOW
            } else {
                Color32::from_rgb(235, 60, 60)
            };
            let x_stroke = egui::Stroke::new(if active_axis == Some(GizmoAxis::X) { 4.0 } else { 2.5 }, x_col);
            painter.line_segment([sc, sx], x_stroke);
            painter.circle_filled(sx, 5.0, x_col);
            painter.text(sx + egui::vec2(7.0, -7.0), egui::Align2::LEFT_CENTER, "X", egui::FontId::monospace(12.0), x_col);

            let y_col = if active_axis == Some(GizmoAxis::Y) {
                Color32::YELLOW
            } else {
                Color32::from_rgb(60, 220, 60)
            };
            let y_stroke = egui::Stroke::new(if active_axis == Some(GizmoAxis::Y) { 4.0 } else { 2.5 }, y_col);
            painter.line_segment([sc, sy], y_stroke);
            painter.circle_filled(sy, 5.0, y_col);
            painter.text(sy + egui::vec2(7.0, -7.0), egui::Align2::LEFT_CENTER, "Y", egui::FontId::monospace(12.0), y_col);

            let z_col = if active_axis == Some(GizmoAxis::Z) {
                Color32::YELLOW
            } else {
                Color32::from_rgb(60, 150, 255)
            };
            let z_stroke = egui::Stroke::new(if active_axis == Some(GizmoAxis::Z) { 4.0 } else { 2.5 }, z_col);
            painter.line_segment([sc, sz], z_stroke);
            painter.circle_filled(sz, 5.0, z_col);
            painter.text(sz + egui::vec2(7.0, -7.0), egui::Align2::LEFT_CENTER, "Z", egui::FontId::monospace(12.0), z_col);
        }
    }

    // -----------------------------------------------------------------------
    // Undo recording
    // -----------------------------------------------------------------------

    /// Called at the end of every frame with the structure snapshot taken at its start
    /// (`None` when the frame did not start in the World Editor).
    pub(crate) fn finish_world_frame(
        &mut self,
        ctx: &egui::Context,
        before: Option<SceneStructure>,
    ) {
        let Some(before) = before else {
            self.world.gesture_in_progress = false;
            return;
        };
        if std::mem::take(&mut self.world.skip_record) {
            self.world.gesture_in_progress = false;
            return;
        }
        let busy = ctx.input(|i| i.pointer.any_down()) || ctx.memory(|m| m.focused().is_some());
        if self.project.scene.structure() != before {
            if !self.world.gesture_in_progress {
                self.project.push_world_undo(before);
            }
            self.project.dirty = true;
            self.world.gesture_in_progress = busy;
        } else if !busy {
            self.world.gesture_in_progress = false;
        }
        self.sanitize_world_state();
    }
}
