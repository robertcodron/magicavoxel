//! World Editor side panel: Scene Outline, selected object properties, Layer Manager.
//!
//! Edits made here are recorded for undo by `finish_world_frame` (see `world_editor`).

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, RichText};
use mv_core::{Axis, NodeId, NodeKind, Scene};

use crate::app::MagicaVoxelApp;

enum OutlineAction {
    Select { id: NodeId, additive: bool },
    Open(NodeId),
    ToggleHidden(NodeId),
    ToggleCollapsed(NodeId),
}

fn node_label(scene: &Scene, id: NodeId) -> String {
    let Some(n) = scene.node(id) else {
        return format!("#{id}");
    };
    let kind = match &n.kind {
        NodeKind::Group { children } => format!("📁 Group ({})", children.len()),
        NodeKind::Shape { models } => {
            let model_id = scene.shape_model(id).unwrap_or(models[0]);
            let refs = scene.model_ref_count(model_id);
            let mut label = format!("🧊 Model #{model_id}");
            if models.len() > 1 {
                label += &format!(" 🎞{}", models.len());
            }
            if refs > 1 {
                label += &format!(" (ref ×{refs})");
            }
            label
        }
    };
    match &n.name {
        Some(name) if !name.is_empty() => format!("{name} — {kind}"),
        _ => kind,
    }
}

impl MagicaVoxelApp {
    pub(crate) fn show_world_panel(&mut self, ui: &mut egui::Ui) {
        self.sanitize_world_state();
        ui.heading("World Editor");
        ui.label(
            RichText::new(
                "Click: select · Shift+click: add · Drag: move · Double-click / TAB: open \
                 · Shift+TAB: leave group · Arrows/PgUp/PgDn: move (Shift ×10) · G: ground \
                 · -/+: rotate (Shift X, Alt Y) · U: union · Ctrl+N/T/R/H",
            )
            .small()
            .color(Color32::from_rgb(160, 200, 255)),
        );
        ui.separator();

        self.show_outline(ui);
        ui.separator();
        self.show_patterns(ui);
        ui.separator();
        self.show_selection_properties(ui);
        ui.separator();
        self.show_layers(ui);
    }

    fn show_patterns(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pattern Library");
        ui.horizontal_wrapped(|ui| {
            for pattern in &["Arch", "Column", "Stairs", "Wall", "Tree", "Monument"] {
                if ui
                    .button(*pattern)
                    .on_hover_text(format!("Spawn {pattern} prefab into current group"))
                    .clicked()
                {
                    self.spawn_pattern(pattern);
                }
            }
        });
    }

    fn show_outline(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scene Outline");

        // Breadcrumb of the group being edited.
        let scene = &self.project.scene;
        let mut path = Vec::new();
        let mut cur = Some(self.world.group);
        while let Some(id) = cur {
            path.push(id);
            cur = scene.node(id).and_then(|n| n.parent);
        }
        path.reverse();
        let mut enter: Option<NodeId> = None;
        ui.horizontal_wrapped(|ui| {
            for (i, &id) in path.iter().enumerate() {
                if i > 0 {
                    ui.label("›");
                }
                let name = if i == 0 {
                    "Scene".to_string()
                } else {
                    scene
                        .node(id)
                        .and_then(|n| n.name.clone())
                        .unwrap_or_else(|| format!("Group {id}"))
                };
                if ui.link(name).clicked() {
                    enter = Some(id);
                }
            }
        });
        if let Some(g) = enter {
            self.enter_group(g);
        }

        let mut actions = Vec::new();
        let scene = &self.project.scene;
        let root = scene.root();
        egui::ScrollArea::vertical()
            .id_salt("outline_scroll")
            .max_height(260.0)
            .show(ui, |ui| {
                for &child in scene.node(root).map(|n| n.children()).unwrap_or(&[]) {
                    self.outline_row(ui, scene, child, 0, &mut actions);
                }
            });

        let shift = ui.input(|i| i.modifiers.shift);
        for a in actions {
            match a {
                OutlineAction::Select { id, additive } => {
                    // Selecting outside the current group moves into the node's group.
                    let parent = self.project.scene.node(id).and_then(|n| n.parent);
                    if let Some(p) = parent {
                        if p != self.world.group {
                            self.world.group = p;
                            self.world.selection.clear();
                        }
                    }
                    if additive || shift {
                        if !self.world.selection.remove(&id) {
                            self.world.selection.insert(id);
                        }
                    } else {
                        self.world.selection = BTreeSet::from([id]);
                    }
                }
                OutlineAction::Open(id) => match self.project.scene.node(id).map(|n| &n.kind) {
                    Some(NodeKind::Group { .. }) => self.enter_group(id),
                    Some(NodeKind::Shape { .. }) => self.edit_shape_in_model_editor(id),
                    None => {}
                },
                OutlineAction::ToggleHidden(id) => {
                    if let Some(n) = self.project.scene.node_mut(id) {
                        n.hidden = !n.hidden;
                    }
                }
                OutlineAction::ToggleCollapsed(id) => {
                    if !self.world.collapsed.remove(&id) {
                        self.world.collapsed.insert(id);
                    }
                }
            }
        }

        ui.horizontal_wrapped(|ui| {
            if ui.button("➕ New").on_hover_text("Ctrl+N").clicked() {
                self.world_new_object();
            }
            if ui.button("Select all").on_hover_text("Ctrl+A").clicked() {
                self.world_select_all();
            }
            if ui
                .button("Unhide all")
                .on_hover_text("Ctrl+Shift+H")
                .clicked()
            {
                self.world_unhide_all();
            }
        });
    }

    fn outline_row(
        &self,
        ui: &mut egui::Ui,
        scene: &Scene,
        id: NodeId,
        depth: usize,
        actions: &mut Vec<OutlineAction>,
    ) {
        let Some(node) = scene.node(id) else { return };
        let is_group = matches!(node.kind, NodeKind::Group { .. });
        let collapsed = self.world.collapsed.contains(&id);
        let in_group = node.parent == Some(self.world.group);
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if is_group {
                if ui.small_button(if collapsed { "▶" } else { "▼" }).clicked() {
                    actions.push(OutlineAction::ToggleCollapsed(id));
                }
            } else {
                ui.add_space(18.0);
            }
            let eye = if node.hidden { "◌" } else { "👁" };
            if ui
                .small_button(eye)
                .on_hover_text("Hide / show (Ctrl+H)")
                .clicked()
            {
                actions.push(OutlineAction::ToggleHidden(id));
            }
            if let Some(layer) = scene.layers.get(node.layer_id) {
                let [r, g, b] = layer.color;
                ui.colored_label(Color32::from_rgb(r, g, b), "■")
                    .on_hover_text(format!("Layer {}: {}", node.layer_id, layer.name));
            }
            let mut text = RichText::new(node_label(scene, id));
            if !scene.is_visible(id) {
                text = text.color(Color32::GRAY);
            } else if !in_group {
                text = text.color(Color32::from_gray(170));
            }
            let resp = ui.selectable_label(self.world.selection.contains(&id), text);
            if resp.double_clicked() {
                actions.push(OutlineAction::Open(id));
            } else if resp.clicked() {
                actions.push(OutlineAction::Select {
                    id,
                    additive: false,
                });
            }
        });
        if is_group && !collapsed {
            for &c in node.children() {
                self.outline_row(ui, scene, c, depth + 1, actions);
            }
        }
    }

    fn show_selection_properties(&mut self, ui: &mut egui::Ui) {
        let count = self.world.selection.len();
        ui.heading(match count {
            0 => "Selection: none".to_string(),
            1 => "Selected object".to_string(),
            n => format!("{n} objects selected"),
        });
        if count == 0 {
            return;
        }

        if count == 1 {
            let id = *self.world.selection.iter().next().expect("one selected");
            let layer_names: Vec<String> = self
                .project
                .scene
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{i}: {}", l.name))
                .collect();
            let bounds = self.project.scene.world_bounds(id);
            if let Some(node) = self.project.scene.node_mut(id) {
                let mut name = node.name.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    if ui
                        .add(egui::TextEdit::singleline(&mut name).desired_width(140.0))
                        .changed()
                    {
                        node.name = (!name.is_empty()).then_some(name);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Position:");
                    let t = &mut node.transform.translation;
                    ui.add(egui::DragValue::new(&mut t.x).speed(0.25).prefix("x "));
                    ui.add(egui::DragValue::new(&mut t.y).speed(0.25).prefix("y "));
                    ui.add(egui::DragValue::new(&mut t.z).speed(0.25).prefix("z "));
                });
                ui.horizontal(|ui| {
                    ui.label("Layer:");
                    let current = layer_names
                        .get(node.layer_id)
                        .cloned()
                        .unwrap_or_else(|| node.layer_id.to_string());
                    egui::ComboBox::from_id_salt("node_layer")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (i, label) in layer_names.iter().enumerate() {
                                ui.selectable_value(&mut node.layer_id, i, label);
                            }
                        });
                });
            }
            if let Some((lo, hi)) = bounds {
                let s = hi - lo + glam::IVec3::ONE;
                ui.label(
                    RichText::new(format!(
                        "World box: ({}, {}, {}) → ({}, {}, {})  size {}×{}×{}",
                        lo.x, lo.y, lo.z, hi.x, hi.y, hi.z, s.x, s.y, s.z
                    ))
                    .small(),
                );
            }
        }

        ui.horizontal_wrapped(|ui| {
            ui.label("Rotate 90°:");
            for (axis, label) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
                if ui.button(label).clicked() {
                    self.world_rotate(axis);
                }
            }
            if ui.button("⬇ Ground").on_hover_text("G").clicked() {
                self.world_ground();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Instance")
                .on_hover_text("Copy that shares the same model (Reference)")
                .clicked()
            {
                self.world_copy(false);
            }
            if ui
                .button("Duplicate")
                .on_hover_text("Copy with its own model")
                .clicked()
            {
                self.world_copy(true);
            }
            if ui
                .button("Ref → Dup")
                .on_hover_text("Convert reference to duplicate (Ctrl+T)")
                .clicked()
            {
                self.world_reference_to_duplicate();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Group").on_hover_text("Ctrl+R").clicked() {
                self.world_group();
            }
            if ui.button("Ungroup").on_hover_text("Ctrl+Shift+R").clicked() {
                self.world_ungroup();
            }
            if ui.button("Union").on_hover_text("U").clicked() {
                self.world_union();
            }
            if ui.button("Hide").on_hover_text("Ctrl+H").clicked() {
                self.world_set_hidden(true);
            }
            if ui.button("🗑 Delete").on_hover_text("Delete").clicked() {
                self.world_delete();
            }
        });
    }

    fn show_layers(&mut self, ui: &mut egui::Ui) {
        ui.heading("Layers");
        let has_selection = !self.world.selection.is_empty();
        let mut assign = None;
        for (i, layer) in self.project.scene.layers.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let eye = if layer.hidden { "◌" } else { "👁" };
                if ui
                    .small_button(eye)
                    .on_hover_text("Show / hide layer")
                    .clicked()
                {
                    layer.hidden = !layer.hidden;
                }
                ui.color_edit_button_srgb(&mut layer.color);
                ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(90.0));
                if ui
                    .add_enabled(has_selection, egui::Button::new("Set").small())
                    .on_hover_text("Move the selection to this layer")
                    .clicked()
                {
                    assign = Some(i);
                }
            });
        }
        if let Some(i) = assign {
            self.world_assign_layer(i);
        }
    }
}
