//! "Unsaved changes" confirmation before an action would discard the current project:
//! opening another file (dialog or drag & drop) or closing the window
//! (`mv_main.html`: "Program will ask for saving changes when opening new models or
//! closing the program").

use std::path::PathBuf;

use eframe::egui::{self, Color32, Key, RichText};

use crate::app::MagicaVoxelApp;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PendingAction {
    Open(PathBuf),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

impl MagicaVoxelApp {
    /// Open a file, asking first if the current project has unsaved changes.
    pub(crate) fn request_open_path(&mut self, path: PathBuf) {
        if self.project.dirty {
            self.pending_action = Some(PendingAction::Open(path));
        } else {
            self.open_vox_path(path);
        }
    }

    /// Intercept the window close button when there are unsaved changes.
    pub(crate) fn handle_close_request(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        if requested && self.project.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_action = Some(PendingAction::Quit);
        }
    }

    pub(crate) fn resolve_pending(&mut self, ctx: &egui::Context, choice: UnsavedChoice) {
        let Some(action) = self.pending_action.take() else {
            return;
        };
        let proceed = match choice {
            UnsavedChoice::Cancel => false,
            UnsavedChoice::Discard => true,
            UnsavedChoice::Save => {
                self.handle_save_project(false);
                // Still dirty: the save dialog was cancelled or saving failed.
                !self.project.dirty
            }
        };
        if !proceed {
            return;
        }
        match action {
            PendingAction::Open(path) => self.open_vox_path(path),
            PendingAction::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Modal dialog for the pending action, if any.
    pub(crate) fn show_unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = &self.pending_action else {
            return;
        };
        let what = match action {
            PendingAction::Open(path) => format!(
                "opening {}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().to_string()
                )
            ),
            PendingAction::Quit => "quitting".to_string(),
        };
        let name = self.project.name.clone();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved_changes")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Unsaved changes");
            ui.label(format!(
                "\"{name}\" has unsaved changes. Save them before {what}?"
            ));
            ui.label(
                RichText::new("An auto-backup may exist in cache/backup.vox.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("💾 Save").clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
            });
        });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            choice = Some(UnsavedChoice::Cancel);
        }
        if let Some(c) = choice {
            self.resolve_pending(ctx, c);
        }
    }
}
