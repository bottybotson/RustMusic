mod app;
mod audio;
mod importer;
mod library;

use std::path::PathBuf;

use eframe::egui;
use app::MusicApp;
use library::Library;

fn main() -> eframe::Result {
    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = data_dir.join("MusicLibrary").join("library.sqlite3");
    let library = match Library::open(&path) {
        Ok(library) => library,
        Err(error) => {
            eprintln!("Cannot open library database at {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    eframe::run_native(
        "Music Library",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1150.0, 720.0])
                .with_min_inner_size([850.0, 520.0]),
            ..Default::default()
        },
        Box::new(move |creation_context| {
            egui_extras::install_image_loaders(&creation_context.egui_ctx);
            Ok(Box::new(MusicApp::new(library, &creation_context.egui_ctx)))
        }),
    )
}
