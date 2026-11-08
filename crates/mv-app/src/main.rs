mod app;
mod shortcuts;
mod ui;
mod viewport;

use eframe::egui;

use crate::app::MagicaVoxelApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MagicaVoxel - Rust Edition (Model Editor + GPU Path Tracer)")
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MagicaVoxel Rust",
        native_options,
        Box::new(|cc| Ok(Box::new(MagicaVoxelApp::new(cc)))),
    )
}
