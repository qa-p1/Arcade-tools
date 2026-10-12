#[path = "build/tray_assets.rs"]
mod tray_assets;

fn main() {
    tray_assets::build_from(std::path::Path::new(".."));
    tauri_build::build()
}
