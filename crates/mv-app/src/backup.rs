//! Auto-backup: every `every` recorded modifications, the project is written to
//! `cache/backup.vox` (relative to the working directory).
//!
//! The `.vox` bytes are produced on the UI thread (a consistent snapshot) and written
//! by a background thread to a temporary file that is then renamed, so a crash
//! mid-write never leaves a truncated backup.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crate::app::MagicaVoxelApp;

pub(crate) const BACKUP_EVERY: u64 = 25;

pub(crate) struct AutoBackup {
    pub(crate) path: PathBuf,
    pub(crate) every: u64,
    /// `project.edit_count` at the last backup.
    last_count: u64,
    pending: Option<JoinHandle<std::io::Result<PathBuf>>>,
}

impl AutoBackup {
    pub(crate) fn new() -> Self {
        Self {
            path: PathBuf::from("cache").join("backup.vox"),
            every: BACKUP_EVERY,
            last_count: 0,
            pending: None,
        }
    }

    /// A new project was loaded: its edit counter restarts from 0.
    pub(crate) fn reset(&mut self) {
        self.last_count = 0;
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("vox.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(path.to_path_buf())
}

impl MagicaVoxelApp {
    /// Report a finished backup and start a new one when enough edits accumulated.
    pub(crate) fn tick_auto_backup(&mut self) {
        if self
            .backup
            .pending
            .as_ref()
            .is_some_and(|h| h.is_finished())
        {
            self.finish_backup();
        }
        let due = self.project.edit_count >= self.backup.last_count + self.backup.every.max(1);
        if !due || self.backup.pending.is_some() {
            return;
        }
        self.backup.last_count = self.project.edit_count;
        let bytes = mv_format::to_vox_bytes(&self.project);
        let path = self.backup.path.clone();
        self.backup.pending = Some(std::thread::spawn(move || write_atomically(&path, &bytes)));
    }

    fn finish_backup(&mut self) {
        let Some(handle) = self.backup.pending.take() else {
            return;
        };
        let msg = match handle.join() {
            Ok(Ok(path)) => format!("Auto-backup saved to {}", path.display()),
            Ok(Err(e)) => format!("[ERR] Auto-backup failed: {e}"),
            Err(_) => "[ERR] Auto-backup thread panicked".to_string(),
        };
        self.project.console_logs.push(msg);
    }

    /// Block until the running backup (if any) is written (tests, shutdown).
    pub(crate) fn wait_backup(&mut self) {
        self.finish_backup();
    }
}
