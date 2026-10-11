#[path = "build/tray_assets.rs"]
mod tray_assets;

fn main() {
    tray_assets::build();
    tauri_build::build()
}
