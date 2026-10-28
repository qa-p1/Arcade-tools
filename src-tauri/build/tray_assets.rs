//! Rasterize the design kit's glyphs at build time; no SVG engine stays resident.
//! Menu and overlay glyphs are drawn in the kit's neutral `faint` grey (brand
//! marks stay neutral, STANDARD §2); the overlay recolours them from their
//! coverage. The tray mark is Tools' glyph, monochrome: white for dark panels,
//! black for light panels and macOS template images (STANDARD §8).
use arcade_link::design::{palette, Theme};
use resvg::{tiny_skia, usvg};
use std::path::Path;

pub fn build_from(root: &Path) {
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let grey = format!("#{:06x}", palette(Theme::Light, "arcade.tools").faint);
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
        let pixmap = render(root, id, &grey, 32);
        pixmap.save_png(output.join(format!("{id}.png"))).unwrap();
        write_rgba(&output.join(format!("{id}.rgba")), &pixmap);
    }
    for (name, colour) in [("mark-white", "#ffffff"), ("mark-black", "#000000")] {
        write_rgba(
            &output.join(format!("{name}.rgba")),
            &render(root, "tools", colour, 32),
        );
    }
}

fn render(root: &Path, id: &str, colour: &str, size: u32) -> tiny_skia::Pixmap {
    let path = root.join(format!("src-ui/design/glyphs/arcade.{id}.svg"));
    println!("cargo:rerun-if-changed={}", path.display());
    let svg = std::fs::read_to_string(path)
        .unwrap()
        .replace("currentColor", colour);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
    let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            size as f32 / tree.size().width(),
            size as f32 / tree.size().height(),
        ),
        &mut pixmap.as_mut(),
    );
    pixmap
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
