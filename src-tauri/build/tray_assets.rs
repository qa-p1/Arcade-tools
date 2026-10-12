//! Rasterize the checked-in glyphs at build time; no SVG engine stays resident.
use resvg::{tiny_skia, usvg};
use std::path::Path;

pub fn build_from(root: &Path) {
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for id in [
        "box",
        "lens",
        "look",
        "wheel",
        "clipboard",
        "shelf",
        "find",
        "tools",
    ] {
        let path = root.join(format!("public/glyphs/arcade.{id}.svg"));
        println!("cargo:rerun-if-changed={}", path.display());
        let svg = std::fs::read_to_string(path)
            .unwrap()
            .replace("currentColor", "#8b7cf8");
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
        let mut pixmap = tiny_skia::Pixmap::new(32, 32).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(
                32.0 / tree.size().width(),
                32.0 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        pixmap.save_png(output.join(format!("{id}.png"))).unwrap();
        write_rgba(&output.join(format!("{id}.rgba")), &pixmap);
    }
    let mark = root.join("src-tauri/icons/32x32.png");
    println!("cargo:rerun-if-changed={}", mark.display());
    let mark = tiny_skia::Pixmap::load_png(mark).unwrap();
    write_rgba(&output.join("mark.rgba"), &mark);
}

fn write_rgba(path: &Path, pixmap: &tiny_skia::Pixmap) {
    let mut rgba = pixmap.data().to_vec();
    for p in rgba.as_chunks_mut::<4>().0 {
        if p[3] != 0 {
            for c in 0..3 {
                p[c] = ((p[c] as u32 * 255) / p[3] as u32).min(255) as u8;
            }
        }
    }
    std::fs::write(path, rgba).unwrap();
}
