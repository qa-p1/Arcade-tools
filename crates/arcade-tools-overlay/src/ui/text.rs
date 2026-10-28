//! Software glyph blending and cached cosmic-text lines adapted from Arcade
//! Find's `ui/text.rs` (MIT OR Apache-2.0). Inter comes from the design kit;
//! one known system font is added for glyphs Inter lacks, which avoids
//! FontSystem::new's full system-font scan on the first-frame path.

use std::collections::HashMap;

use cosmic_text::{
    fontdb, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
};
use tiny_skia::Pixmap;

#[derive(Hash, PartialEq, Eq)]
struct Key {
    text: String,
    size: u32,
    weight: u16,
}

pub struct Text {
    fonts: FontSystem,
    cache: SwashCache,
    lines: HashMap<Key, Buffer>,
}

impl Default for Text {
    fn default() -> Self {
        let mut db = fontdb::Database::new();
        db.load_font_data(arcade_link::design::inter_font_bytes().to_vec());
        let inter = db.len();
        // A system face for symbols and scripts Inter lacks, paired with a
        // medium/semibold face.
        #[cfg(target_os = "linux")]
        let paths = [
            (
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/noto/NotoSans-Medium.ttf",
            ),
            (
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Medium.ttf",
            ),
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ),
            (
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            ),
        ];
        #[cfg(windows)]
        let paths = [
            (
                "C:/Windows/Fonts/segoeui.ttf",
                "C:/Windows/Fonts/segoeuisb.ttf",
            ),
            ("C:/Windows/Fonts/arial.ttf", "C:/Windows/Fonts/arialbd.ttf"),
        ];
        #[cfg(target_os = "macos")]
        let paths = [
            (
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            ),
            ("/System/Library/Fonts/Helvetica.ttc", ""),
        ];
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let paths: [(&str, &str); 0] = [];
        for (regular, strong) in paths {
            if db.load_font_file(regular).is_ok() && db.len() > inter {
                let _ = db.load_font_file(strong);
                break;
            }
        }
        // Unusual installations still get fallback glyphs, at a slower first frame.
        if db.len() == inter {
            db.load_system_fonts();
        }
        db.set_sans_serif_family("Inter");
        let fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        Self {
            fonts,
            cache: SwashCache::new(),
            lines: HashMap::new(),
        }
    }
}

impl Text {
    fn key(&mut self, text: &str, size: f32, weight: u16) -> Key {
        if self.lines.len() > 800 {
            self.lines.clear();
        }
        let key = Key {
            text: text.into(),
            size: (size * 100.0).round() as u32,
            weight,
        };
        if !self.lines.contains_key(&key) {
            let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.3));
            buffer.set_wrap(&mut self.fonts, Wrap::None);
            buffer.set_size(&mut self.fonts, None, None);
            let attrs = Attrs::new()
                .family(Family::SansSerif)
                .weight(Weight(weight));
            buffer.set_text(&mut self.fonts, text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.fonts, false);
            self.lines.insert(
                Key {
                    text: text.into(),
                    size: key.size,
                    weight,
                },
                buffer,
            );
        }
        key
    }

    pub fn width(&mut self, text: &str, size: f32, weight: u16) -> f32 {
        let key = self.key(text, size, weight);
        self.lines[&key]
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    pub fn fit(&mut self, text: &str, size: f32, weight: u16, width: f32) -> String {
        if self.width(text, size, weight) <= width {
            return text.into();
        }
        let ellipsis = self.width("…", size, weight);
        let key = self.key(text, size, weight);
        let mut cut = 0;
        for run in self.lines[&key].layout_runs() {
            for glyph in run.glyphs {
                if glyph.x + glyph.w + ellipsis <= width {
                    cut = cut.max(glyph.end);
                }
            }
        }
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        size: f32,
        weight: u16,
        color: u32,
        x: f32,
        y: f32,
        clip: (i32, i32, i32, i32),
    ) {
        let key = self.key(text, size, weight);
        let color = Color::rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let (cx0, cy0, cx1, cy1) = (
            clip.0.max(0),
            clip.1.max(0),
            clip.2.min(width),
            clip.3.min(height),
        );
        let data = pixmap.data_mut();
        self.lines[&key].draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |px, py, _, _, color| {
                let px = px + x.round() as i32;
                let py = py + y.round() as i32;
                if px < cx0 || py < cy0 || px >= cx1 || py >= cy1 {
                    return;
                }
                let offset = ((py * width + px) * 4) as usize;
                let alpha = u32::from(color.a());
                for (channel, value) in [color.r(), color.g(), color.b()].into_iter().enumerate() {
                    data[offset + channel] = ((u32::from(value) * alpha
                        + u32::from(data[offset + channel]) * (255 - alpha))
                        / 255) as u8;
                }
                data[offset + 3] =
                    (alpha + u32::from(data[offset + 3]) * (255 - alpha) / 255) as u8;
            },
        );
    }
}
