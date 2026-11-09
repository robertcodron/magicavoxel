//! Headless integration tests: real egui frames driven with synthetic keyboard and
//! pointer events (no window, no GPU required).

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use eframe::App;
use glam::{IVec3, Vec3};
use mv_core::{EditorProject, NodeId};

use crate::app::{MagicaVoxelApp, ViewportMode};

const SCREEN: egui::Vec2 = egui::vec2(1400.0, 900.0);

struct Harness {
    ctx: egui::Context,
    app: MagicaVoxelApp,
    events: Vec<Event>,
    dropped: Vec<egui::DroppedFile>,
    /// Window close button pressed in the next frame.
    close_request: bool,
    /// Viewport commands emitted by the last frame (e.g. `CancelClose`).
    commands: Vec<egui::ViewportCommand>,
    modifiers: Modifiers,
    time: f64,
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    }
}

impl Harness {
    fn new(project: EditorProject) -> Self {
        let mut app = MagicaVoxelApp::with_project(project);
        app.display.render_scale = 4; // cheaper CPU rendering; picking is exact anyway
        let mut h = Self {
            ctx: egui::Context::default(),
            app,
            events: Vec::new(),
            dropped: Vec::new(),
            close_request: false,
            commands: Vec::new(),
            modifiers: Modifiers::NONE,
            time: 0.0,
        };
        h.step();
        h.step();
        h
    }

    fn step(&mut self) {
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            events: std::mem::take(&mut self.events),
            dropped_files: std::mem::take(&mut self.dropped),
            modifiers: self.modifiers,
            time: Some(self.time),
            ..Default::default()
        };
        if std::mem::take(&mut self.close_request) {
            let mut info = egui::ViewportInfo::default();
            info.events.push(egui::ViewportEvent::Close);
            raw.viewports.insert(egui::ViewportId::ROOT, info);
        }
        self.time += 1.0 / 60.0;
        self.app.raw_input_hook(&self.ctx, &mut raw);
        let out = self.ctx.run(raw, |ctx| self.app.frame(ctx));
        self.commands = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.commands.clone())
            .unwrap_or_default();
    }

    fn key_with(&mut self, key: Key, modifiers: Modifiers) {
        self.modifiers = modifiers;
        for pressed in [true, false] {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        self.step();
        self.modifiers = Modifiers::NONE;
        self.step();
    }

    fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::NONE);
    }

    fn pointer_button(&mut self, pos: Pos2, pressed: bool) {
        self.events.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        });
        self.step();
    }

    fn click_with(&mut self, pos: Pos2, modifiers: Modifiers) {
        // Leave time since any previous click so egui does not see a double-click.
        self.time += 1.0;
        self.modifiers = modifiers;
        self.events.push(Event::PointerMoved(pos));
        self.step();
        self.pointer_button(pos, true);
        self.pointer_button(pos, false);
        self.modifiers = Modifiers::NONE;
        self.step();
    }

    fn click(&mut self, pos: Pos2) {
        self.click_with(pos, Modifiers::NONE);
    }

    fn double_click(&mut self, pos: Pos2) {
        self.click(pos);
        self.pointer_button(pos, true);
        self.pointer_button(pos, false);
        self.step();
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.time += 1.0;
        self.events.push(Event::PointerMoved(from));
        self.step();
        self.pointer_button(from, true);
        for i in 1..=8 {
            let p = from + (to - from) * (i as f32 / 8.0);
            self.events.push(Event::PointerMoved(p));
            self.step();
        }
        self.pointer_button(to, false);
        self.step();
    }

    /// Screen position of a world point, using the app camera and viewport rectangle.
    fn world_to_screen(&self, p: Vec3) -> Pos2 {
        let rect = self.app.viewport_rect;
        let cam = &self.app.camera;
        let (eye, f, r, u) = cam.basis();
        let rel = p - eye;
        let z = rel.dot(f);
        let half_tan = (cam.fov_deg.to_radians() * 0.5).tan();
        let aspect = rect.width() / rect.height();
        let ndc_x = rel.dot(r) / (z * half_tan * aspect);
        let ndc_y = rel.dot(u) / (z * half_tan);
        Pos2::new(
            rect.left() + (ndc_x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - ndc_y) * 0.5 * rect.height(),
        )
    }

    /// Screen position of the top-center of a node's world box (always on a voxel for
    /// the solid test models used here).
    fn screen_of(&self, id: NodeId) -> Pos2 {
        let (lo, hi) = self.app.project.scene.world_bounds(id).expect("bounds");
        let top = Vec3::new(
            (lo.x + hi.x + 1) as f32 * 0.5,
            (lo.y + hi.y + 1) as f32 * 0.5,
            (hi.z + 1) as f32,
        );
        self.world_to_screen(top - Vec3::Z * 0.01)
    }

    fn bounds(&self, id: NodeId) -> (IVec3, IVec3) {
        self.app.project.scene.world_bounds(id).expect("bounds")
    }
}

/// A project with two solid cubes side by side (shapes `a` and `b`, instances).
fn two_cubes() -> (EditorProject, NodeId, NodeId) {
    let mut proj = EditorProject::new_empty(6, 6, 6);
    proj.grid.full(12);
    proj.scene.models[0] = proj.grid.clone();
    let root = proj.scene.root();
    let a = proj.scene.shapes_under(root)[0];
    proj.scene.translate_world(a, IVec3::new(-6, 0, 3)).unwrap();
    let b = proj.scene.instantiate(a).unwrap();
    proj.scene.translate_world(b, IVec3::new(12, 0, 0)).unwrap();
    (proj, a, b)
}

#[test]
fn tab_switches_editors_without_stealing_focus() {
    let (proj, a, _) = two_cubes();
    let mut h = Harness::new(proj);
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);

    h.key(Key::Tab);
    assert_eq!(h.app.mode, ViewportMode::WorldEditor);
    assert!(
        h.ctx.memory(|m| m.focused()).is_none(),
        "TAB must not move keyboard focus to a widget"
    );

    // TAB with one shape selected opens it in the Model Editor.
    h.click(h.screen_of(a));
    assert_eq!(
        h.app.world.selection.iter().copied().collect::<Vec<_>>(),
        vec![a]
    );
    h.key(Key::Tab);
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);
    assert_eq!(h.app.project.active_model, 0);

    // While the console has focus, TAB stays with the text field.
    h.key(Key::F1);
    h.step();
    assert!(
        h.ctx.memory(|m| m.focused()).is_some(),
        "F1 focuses the console"
    );
    h.key(Key::Tab);
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);
}

#[test]
fn keyboard_moves_grounds_and_undoes() {
    let (proj, a, b) = two_cubes();
    let mut h = Harness::new(proj);
    h.key(Key::Tab);
    let start = h.bounds(a);
    let b_start = h.bounds(b);

    h.click(h.screen_of(a));
    assert!(h.app.world.selection.contains(&a));

    h.key(Key::ArrowUp);
    let moved = h.bounds(a).0 - start.0;
    assert_eq!(moved.z, 0);
    assert_eq!(
        moved.x.abs() + moved.y.abs(),
        1,
        "one step along a horizontal axis"
    );

    h.key(Key::PageUp);
    assert_eq!(h.bounds(a).0.z, start.0.z + 1);
    h.key_with(Key::PageUp, Modifiers::SHIFT);
    assert_eq!(h.bounds(a).0.z, start.0.z + 11, "Shift moves by 10");

    h.key(Key::G);
    assert_eq!(h.bounds(a).0.z, 0, "G puts the object on the ground");
    assert_eq!(h.bounds(b), b_start, "unselected object untouched");

    let steps = h.app.project.world_undo.len();
    assert_eq!(steps, 4, "one undo step per key press");
    for _ in 0..steps {
        h.key_with(Key::Z, ctrl());
    }
    assert_eq!(h.bounds(a), start);
    h.key_with(Key::Y, ctrl());
    assert_eq!(h.bounds(a).0 - start.0, moved, "redo");
}

#[test]
fn drag_moves_object_with_a_single_undo_step() {
    let (proj, a, _) = two_cubes();
    let mut h = Harness::new(proj);
    h.key(Key::Tab);
    let start = h.bounds(a);
    let from = h.screen_of(a);
    let to = from + egui::vec2(60.0, 25.0);

    h.drag(from, to);
    let moved = h.bounds(a).0 - start.0;
    assert_eq!(moved.z, 0, "drag slides on a horizontal plane");
    assert_ne!(moved, IVec3::ZERO, "object moved");
    assert_eq!(
        h.app.project.world_undo.len(),
        1,
        "whole drag = one undo step"
    );

    h.key_with(Key::Z, ctrl());
    assert_eq!(h.bounds(a), start);
}

#[test]
fn group_enter_leave_ungroup_union() {
    let (proj, a, b) = two_cubes();
    let mut h = Harness::new(proj);
    h.key(Key::Tab);
    let root = h.app.project.scene.root();

    h.key_with(Key::A, ctrl());
    assert_eq!(h.app.world.selection.len(), 2);
    h.key_with(Key::R, ctrl());
    let g = *h.app.world.selection.iter().next().unwrap();
    assert_eq!(h.app.project.scene.node(a).unwrap().parent, Some(g));

    // Clicking an object inside the group selects the group from the root level.
    h.click(h.screen_of(b));
    assert_eq!(
        h.app.world.selection.iter().copied().collect::<Vec<_>>(),
        vec![g]
    );

    h.key(Key::Tab);
    assert_eq!(h.app.world.group, g, "TAB enters the selected group");
    h.click(h.screen_of(b));
    assert_eq!(
        h.app.world.selection.iter().copied().collect::<Vec<_>>(),
        vec![b]
    );
    h.key_with(Key::Tab, Modifiers::SHIFT);
    assert_eq!(h.app.world.group, root, "Shift+TAB leaves the group");
    assert!(h.app.world.selection.contains(&g));

    h.key_with(
        Key::R,
        Modifiers {
            shift: true,
            ..ctrl()
        },
    );
    assert!(h.app.project.scene.node(g).is_none(), "ungrouped");
    assert_eq!(h.app.world.selection.len(), 2);

    let union_box = (
        h.bounds(a).0.min(h.bounds(b).0),
        h.bounds(a).1.max(h.bounds(b).1),
    );
    h.key(Key::U);
    let shapes = h.app.project.scene.shapes_under(root);
    assert_eq!(shapes.len(), 1, "union merged both objects");
    assert_eq!(h.bounds(shapes[0]), union_box);

    h.key_with(Key::Z, ctrl());
    assert_eq!(
        h.app.project.scene.shapes_under(root).len(),
        2,
        "undo union"
    );
}

#[test]
fn hide_unhide_new_and_delete() {
    let (proj, a, _) = two_cubes();
    let mut h = Harness::new(proj);
    h.key(Key::Tab);
    let root = h.app.project.scene.root();

    h.click(h.screen_of(a));
    h.key_with(Key::H, ctrl());
    assert!(!h.app.project.scene.is_visible(a));
    h.key_with(
        Key::H,
        Modifiers {
            shift: true,
            ..ctrl()
        },
    );
    assert!(h.app.project.scene.is_visible(a));

    h.key_with(Key::N, ctrl());
    assert_eq!(h.app.project.scene.shapes_under(root).len(), 3);
    let new = *h.app.world.selection.iter().next().unwrap();
    assert_eq!(h.bounds(new).0.z, 0);

    h.key(Key::Delete);
    assert_eq!(h.app.project.scene.shapes_under(root).len(), 2);
    assert!(h.app.world.selection.is_empty());

    // Ctrl+T: the two cubes share model 0; converting gives the selection its own copy.
    h.click(h.screen_of(a));
    h.key_with(Key::T, ctrl());
    let model = h.app.project.scene.node(a).unwrap().model_id().unwrap();
    assert_ne!(model, 0);
    assert_eq!(h.app.project.scene.model_ref_count(0), 1);
}

#[test]
fn double_click_opens_shape_in_model_editor() {
    let (proj, _, b) = two_cubes();
    let mut h = Harness::new(proj);
    h.key(Key::Tab);
    h.click(h.screen_of(b));
    h.key_with(Key::T, ctrl()); // b gets its own model
    let model_b = h.app.project.scene.node(b).unwrap().model_id().unwrap();

    h.double_click(h.screen_of(b));
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);
    assert_eq!(h.app.project.active_model, model_b);
}

/// A project whose single shape has 3 frames of different sizes (2, 4, 6).
fn animated_project() -> EditorProject {
    let mut proj = EditorProject::new_empty(2, 2, 2);
    proj.grid.full(3);
    for size in [4, 6] {
        proj.add_frame(true).unwrap();
        proj.resize_canvas(size, size, size);
        proj.grid.full(5);
    }
    proj.set_frame(0);
    proj.world_undo.clear();
    proj
}

#[test]
fn timeline_playback_cycles_frames_in_model_editor() {
    let mut h = Harness::new(animated_project());
    assert_eq!(h.app.project.active_frame_count(), 3);
    assert_eq!(h.app.project.grid.dimensions().x, 2);

    // Undo history of frame 0 must survive playback.
    h.app.project.record_undo();
    h.app.project.grid.set(0, 0, 0, 9);

    h.app.anim_fps = 10.0;
    h.app.anim_playing = true;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..40 {
        h.step(); // 1/60 s per step: ~6 frames at 10 fps
        seen.insert(h.app.project.active_frame_index());
        let size = h.app.project.grid.dimensions().x;
        assert_eq!(size as usize, 2 + 2 * h.app.project.active_frame_index());
    }
    assert_eq!(seen.len(), 3, "every frame shown while looping");
    h.app.anim_playing = false;

    h.app.goto_frame(0);
    assert_eq!(h.app.project.grid.get(0, 0, 0), 9);
    assert!(h.app.project.history.can_undo(), "per-model history kept");
}

#[test]
fn world_editor_shows_current_frame() {
    let mut h = Harness::new(animated_project());
    h.key(Key::Tab);
    let shape = h.app.project.active_shape.unwrap();
    let size_at = |h: &Harness| {
        let (lo, hi) = h.bounds(shape);
        (hi - lo + IVec3::ONE).x
    };
    assert_eq!(size_at(&h), 2);
    h.app.goto_frame(2);
    h.step();
    assert_eq!(size_at(&h), 6);

    // Opening the shape in the Model Editor edits the model of the current frame.
    h.click(h.screen_of(shape));
    h.key(Key::Tab);
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);
    assert_eq!(h.app.project.grid.dimensions().x, 6);
    assert_eq!(h.app.project.active_frame_index(), 2);
}

#[test]
fn deleting_a_frame_is_undoable_from_world_editor() {
    let mut h = Harness::new(animated_project());
    h.app.goto_frame(1);
    assert!(h.app.project.delete_frame());
    assert_eq!(h.app.project.active_frame_count(), 2);
    h.key(Key::Tab);
    h.key_with(Key::Z, ctrl());
    assert_eq!(h.app.project.active_frame_count(), 3);
    assert!(
        h.app.project.scene.models.len() > h.app.project.active_model,
        "edited model still exists"
    );
}

fn repo_shader_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shader")
}

#[test]
fn external_shaders_from_console_and_panel() {
    if mv_render::VoxelShaderEngine::new().is_err() {
        eprintln!("skipping: no GPU");
        return;
    }
    // Every test that needs shaders points at the repository folder.
    std::env::set_var("MV_SHADER_DIR", repo_shader_dir());
    let mut proj = EditorProject::new_empty(4, 4, 6);
    proj.palette.selected = 4;
    proj.grid.set(1, 1, 0, 4);
    let mut h = Harness::new(proj);
    h.app.shaders.refresh();
    assert!(h.app.shaders.path_of("grow").is_some());

    // Console: animation generated by an external shader.
    h.app.execute_command_str("xs -n 3 -prev grow");
    let log = h.app.project.console_logs.last().unwrap().clone();
    assert!(log.contains("generated 3 frame(s)"), "{log}");
    assert_eq!(h.app.project.active_frame_count(), 4);
    assert_eq!(
        h.app.project.grid.count_solid(),
        4,
        "column grew by 3 over 3 frames"
    );
    h.app.goto_frame(1);
    assert_eq!(h.app.project.grid.count_solid(), 2);

    // In place, with an explicit argument; one voxel undo step.
    h.app.goto_frame(0);
    h.app.execute_command_str("xs -n 2 grow 7");
    assert_eq!(h.app.project.grid.get(1, 1, 1), 7);
    assert_eq!(
        h.app.project.grid.get(1, 1, 2),
        0,
        "7 is not a selected color"
    );
    assert!(h.app.project.undo());
    assert_eq!(h.app.project.grid.count_solid(), 1);

    // Unknown shader: reported in the console, nothing changes.
    h.app.execute_command_str("xs does_not_exist");
    let log = h.app.project.console_logs.last().unwrap();
    assert!(
        log.contains("[ERR] Unknown voxel shader 'does_not_exist'"),
        "{log}"
    );

    // Panel: arguments come from the header.
    h.app.shaders.select("hollow");
    assert_eq!(h.app.shaders.arg_values, vec![1.0]);
    assert_eq!(
        h.app.shaders.invocation().unwrap().to_command(),
        "xs hollow 1"
    );
}

/// Unique temporary directory for a test.
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mv-rs-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn dropping_a_vox_file_opens_it() {
    let dir = temp_dir("drop");
    let (proj, _, _) = two_cubes();
    let path = dir.join("scene.vox");
    mv_format::save_vox_file(&proj, &path).unwrap();

    let mut h = Harness::new(EditorProject::default());
    let root = h.app.project.scene.root();
    assert_eq!(h.app.project.scene.shapes_under(root).len(), 1);

    h.dropped.push(egui::DroppedFile {
        path: Some(path.clone()),
        ..Default::default()
    });
    h.step();
    let root = h.app.project.scene.root();
    assert_eq!(
        h.app.project.scene.shapes_under(root).len(),
        2,
        "dropped scene loaded"
    );
    assert_eq!(h.app.current_file.as_deref(), Some(path.as_path()));

    h.dropped.push(egui::DroppedFile {
        path: Some(dir.join("image.png")),
        ..Default::default()
    });
    h.step();
    let log = h.app.project.console_logs.last().unwrap();
    assert!(log.contains("Unsupported file dropped"), "{log}");
    assert_eq!(h.app.project.scene.shapes_under(root).len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn auto_backup_every_25_modifications() {
    let dir = temp_dir("backup");
    let path = dir.join("cache").join("backup.vox");
    let mut h = Harness::new(EditorProject::new_empty(8, 8, 8));
    h.app.backup.path = path.clone();

    for i in 0..24 {
        h.app.project.record_undo();
        h.app.project.grid.set(i % 8, 0, 0, 3);
        h.step();
    }
    h.app.wait_backup();
    assert!(!path.exists(), "no backup before 25 edits");

    h.app.project.record_undo();
    h.app.project.grid.set(0, 5, 5, 9);
    h.step();
    h.app.wait_backup();
    assert!(path.exists(), "backup written at the 25th edit");
    let log = h.app.project.console_logs.last().unwrap();
    assert!(log.contains("Auto-backup saved"), "{log}");
    let restored = mv_format::load_vox_file(&path).unwrap();
    assert_eq!(restored.grid.get(0, 5, 5), 9);
    assert!(
        !path.with_extension("vox.tmp").exists(),
        "temporary file renamed"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn view_cube_snaps_camera_without_sculpting() {
    let mut h = Harness::new(EditorProject::default());
    let solid = h.app.project.grid.count_solid();
    let cube_rect = crate::view_cube::view_cube_rect(h.app.viewport_rect);
    let face = |h: &Harness, label: &str| {
        crate::view_cube::view_cube_faces(&h.app.camera, cube_rect)
            .into_iter()
            .find(|f| f.label == label)
            .map(|f| f.center())
    };

    let top = face(&h, "Top").expect("default camera looks down on the top face");
    h.click(top);
    assert_eq!(h.app.camera.pitch_deg, 89.0);

    h.app.camera.pitch_deg = 30.0;
    h.app.camera.yaw_deg = -60.0; // looking at the -Y (Front) and +X (Right) faces
    h.step();
    let front = face(&h, "Front").expect("front face visible");
    h.click(front);
    assert_eq!((h.app.camera.yaw_deg, h.app.camera.pitch_deg), (-90.0, 0.0));

    assert_eq!(
        h.app.project.grid.count_solid(),
        solid,
        "the click did not sculpt"
    );
    assert!(!h.app.project.history.can_undo());
}

#[test]
fn opening_over_unsaved_changes_asks_first() {
    use crate::unsaved::{PendingAction, UnsavedChoice};
    let dir = temp_dir("unsaved");
    let (other, _, _) = two_cubes();
    let other_path = dir.join("other.vox");
    mv_format::save_vox_file(&other, &other_path).unwrap();

    let mut h = Harness::new(EditorProject::default());
    let drop = |h: &mut Harness| {
        h.dropped.push(egui::DroppedFile {
            path: Some(other_path.clone()),
            ..Default::default()
        });
        h.step();
    };
    let shapes = |h: &Harness| {
        let root = h.app.project.scene.root();
        h.app.project.scene.shapes_under(root).len()
    };

    // Clean project: opens directly.
    assert!(!h.app.project.dirty);
    drop(&mut h);
    assert_eq!(shapes(&h), 2);

    // Dirty project: the dialog is shown and nothing is replaced yet.
    h.app.project.record_undo();
    h.app.project.grid.set(0, 0, 0, 3);
    let edited = h.app.project.grid.clone();
    drop(&mut h);
    assert_eq!(
        h.app.pending_action,
        Some(PendingAction::Open(other_path.clone()))
    );
    assert_eq!(h.app.project.grid, edited);
    // Shortcuts are blocked while the dialog is open.
    h.key(Key::Tab);
    assert_eq!(h.app.mode, ViewportMode::ModelEditor);

    // Escape = cancel.
    h.key(Key::Escape);
    assert!(h.app.pending_action.is_none());
    assert_eq!(h.app.project.grid, edited);

    // Save: writes the current file, then opens the new one.
    let current = dir.join("current.vox");
    h.app.current_file = Some(current.clone());
    drop(&mut h);
    h.app.resolve_pending(&h.ctx.clone(), UnsavedChoice::Save);
    assert!(current.exists(), "changes saved first");
    assert_eq!(
        mv_format::load_vox_file(&current)
            .unwrap()
            .grid
            .get(0, 0, 0),
        3
    );
    assert_eq!(h.app.current_file.as_deref(), Some(other_path.as_path()));

    // Discard.
    h.app.project.record_undo();
    drop(&mut h);
    h.app
        .resolve_pending(&h.ctx.clone(), UnsavedChoice::Discard);
    assert!(!h.app.project.dirty, "fresh project loaded");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_with_unsaved_changes_is_cancelled_until_confirmed() {
    use crate::unsaved::{PendingAction, UnsavedChoice};
    let mut h = Harness::new(EditorProject::default());

    // Clean: the close goes through (no CancelClose).
    h.close_request = true;
    h.step();
    assert!(!h.commands.contains(&egui::ViewportCommand::CancelClose));

    h.app.project.record_undo();
    h.close_request = true;
    h.step();
    assert!(h.commands.contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(h.app.pending_action, Some(PendingAction::Quit));

    let ctx = h.ctx.clone();
    let out = ctx.run(RawInput::default(), |ctx| {
        h.app.resolve_pending(ctx, UnsavedChoice::Discard);
    });
    let cmds = &out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(cmds.contains(&egui::ViewportCommand::Close));
    assert!(h.app.allow_close);
}

#[test]
fn render_mode_shows_the_whole_scene() {
    // Edited model empty; the visible geometry lives in another object far away.
    let mut proj = EditorProject::new_empty(4, 4, 4);
    let mut cube = mv_core::VoxelGrid::new(6, 6, 6);
    cube.full(9);
    let m = proj.scene.add_model(cube);
    let root = proj.scene.root();
    let far = proj
        .scene
        .add_shape(
            root,
            m,
            mv_core::Transform::from_translation(IVec3::new(40, 0, 3)),
        )
        .unwrap();
    let mut h = Harness::new(proj);
    h.app.display.render_scale = 6;

    // Entering the renderer from the Model Editor frames the edited object (world space).
    h.app.enter_render_mode();
    assert_eq!(h.app.mode, ViewportMode::PathTracer);
    let edited_center = h.app.camera.target;
    assert!(
        edited_center.x.abs() < 4.0,
        "framed on the edited object at the origin"
    );

    // F4 frames the whole scene, which includes the far object.
    h.key(Key::F4);
    assert!(h.app.camera.target.x > 10.0, "scene framing reaches x = 40");
    h.step();
    h.step();
    let [w, hgt] = h.app.last_render_dims;
    let px = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        h.app.rgba_buffer[i..i + 3].to_vec()
    };
    let distinct: std::collections::HashSet<Vec<u8>> = (0..hgt)
        .step_by(3)
        .flat_map(|y| (0..w).step_by(3).map(move |x| (x, y)))
        .map(|(x, y)| px(x, y))
        .collect();
    assert!(distinct.len() > 8, "a rendered scene, not a flat sky");

    // Picking in the renderer hits the far object (world picking): DOF focus set.
    let (lo, hi) = h.app.project.scene.world_bounds(far).unwrap();
    let top = glam::Vec3::new(
        (lo.x + hi.x + 1) as f32 * 0.5,
        (lo.y + hi.y + 1) as f32 * 0.5,
        (hi.z + 1) as f32 - 0.01,
    );
    let before = h.app.pt_settings.focus_dist;
    h.click(h.world_to_screen(top));
    let eye = h.app.camera.basis().0;
    assert_ne!(h.app.pt_settings.focus_dist, before);
    assert!((h.app.pt_settings.focus_dist - eye.distance(top)).abs() < 1.5);
}

#[test]
fn spawn_pattern_and_copy_group_in_world_editor() {
    let mut h = Harness::new(EditorProject::new_empty(2, 2, 2));
    h.key(Key::Tab);

    // Spawn pattern arch
    h.app.spawn_pattern("Arch");
    h.step();
    assert_eq!(h.app.world.selection.len(), 1);
    let arch_id = *h.app.world.selection.iter().next().unwrap();
    assert_eq!(
        h.app.project.scene.node(arch_id).unwrap().name.as_deref(),
        Some("Arch")
    );

    // Group it
    h.app.world_group();
    h.step();
    assert_eq!(h.app.world.selection.len(), 1);
    let group_id = *h.app.world.selection.iter().next().unwrap();
    assert!(h.app.project.scene.node(group_id).unwrap().is_group());

    // Duplicate the group (cloning group + children with unique models)
    h.app.world_copy(true);
    h.step();
    assert_eq!(h.app.world.selection.len(), 1);
    let dup_group = *h.app.world.selection.iter().next().unwrap();
    assert_ne!(dup_group, group_id);
    assert!(h.app.project.scene.node(dup_group).unwrap().is_group());
    assert_eq!(
        h.app.project.scene.node(dup_group).unwrap().children().len(),
        1
    );
}

/// Manual visual check: `VOX_IN=scene.vox PNG_OUT=out.png cargo test -p mv-app
/// world_snapshot -- --ignored` renders the World Editor viewport of a file to a PNG.
#[test]
#[ignore]
fn world_snapshot() {
    let (Ok(input), Ok(output)) = (std::env::var("VOX_IN"), std::env::var("PNG_OUT")) else {
        return;
    };
    let proj = mv_format::load_vox_file(&input).expect("load .vox");
    let mut h = Harness::new(proj);
    h.app.display.render_scale = 1;
    h.key(Key::Tab);
    if std::env::var("RENDER").is_ok() {
        // Path-traced render of the whole scene (`RENDER=1`), from the World camera.
        h.app.enter_render_mode();
        h.app.pt_settings.samples_per_frame = 8;
        if let Ok(fog) = std::env::var("FOG") {
            h.app.pt_settings.fog_density = fog.parse().unwrap_or(0.0);
        }
        for _ in 0..12 {
            h.step();
        }
    }
    // Select the first top-level object to show the selection frame.
    let root = h.app.project.scene.root();
    if let Some(&first) = h.app.project.scene.node(root).unwrap().children().first() {
        h.app.world.selection.insert(first);
    }
    h.step();
    let t0 = std::time::Instant::now();
    for _ in 0..5 {
        h.step();
    }
    let [w, hgt] = h.app.last_render_dims;
    eprintln!(
        "world frame: {:.1} ms at {w}x{hgt} ({} visible shapes)",
        t0.elapsed().as_secs_f64() * 1000.0 / 5.0,
        h.app.project.scene.shapes_under(root).len()
    );
    image::save_buffer(
        &output,
        &h.app.rgba_buffer,
        w as u32,
        hgt as u32,
        image::ColorType::Rgba8,
    )
    .expect("write png");
}
