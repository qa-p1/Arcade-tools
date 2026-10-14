#[path = "../../src-tauri/build/tray_assets.rs"]
mod tray_assets;

fn main() {
    // Rasterize glyphs while building; the on-demand renderer reads raw pixels.
    tray_assets::build_from(std::path::Path::new("../.."));
}
