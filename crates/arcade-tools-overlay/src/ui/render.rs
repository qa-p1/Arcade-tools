//! Opaque panel, tiny-skia + cosmic-text software rendering, adapted from
//! Arcade Find (MIT OR Apache-2.0). No SVG engine or GPU context is needed.

use tiny_skia::{
    FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

use crate::layout::{row_height, Layout, Rect, GROUP_GAP, GROUP_HEIGHT, HEIGHT, PICKER_ROW, WIDTH};
use crate::model::{Model, Scope};

use super::text::Text;
use arcade_link::design::{self, radius, size, text as ty, Theme};

type Clip = (i32, i32, i32, i32);

const REGULAR: u16 = 400;
const MEDIUM: u16 = 500;
const SEMIBOLD: u16 = 600;

/// The kit palette (`arcade_link::design`) with Tools' accent, named for
/// what the sheet draws with it.
#[derive(Clone, Copy)]
pub struct Palette {
    pub panel: u32,
    border: u32,
    strong: u32,
    surface: u32,
    text: u32,
    muted: u32,
    faint: u32,
    accent: u32,
    accent_soft: u32,
    warning: u32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        let kit = design::palette(theme, "arcade.tools");
        Self {
            panel: kit.panel,
            border: kit.line,
            strong: kit.line_strong,
            surface: kit.sunken,
            text: kit.text,
            muted: kit.muted,
            faint: kit.faint,
            accent: kit.accent,
            accent_soft: kit.accent_soft,
            warning: kit.warn,
        }
    }
}

pub struct Renderer {
    text: Text,
    pub dark: bool,
    /// Tools' Appearance is System: system theme changes apply live.
    pub follow_system: bool,
}
impl Renderer {
    pub fn new(dark: bool) -> Self {
        Self {
            text: Text::default(),
            dark,
            follow_system: true,
        }
    }

    pub fn geometry(model: &Model, width: f32, height: f32) -> (Layout, f32, f32) {
        let logical_width = width.min(WIDTH as f32);
        let logical_height = height.min(HEIGHT as f32);
        (
            Layout::new(model, logical_width, logical_height, &model.sections()),
            (width - logical_width) / 2.0,
            (height - logical_height) / 2.0,
        )
    }

    pub fn render(
        &mut self,
        model: &mut Model,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Pixmap, String> {
        let pixmap = Pixmap::new(width, height).ok_or("cannot allocate the overlay frame")?;
        let (layout, ox, oy) = Self::geometry(model, width as f32 / scale, height as f32 / scale);
        let mut canvas = Canvas {
            pixmap,
            ox,
            oy,
            scale,
        };
        let text = &mut self.text;
        let palette = Palette::new(self.dark);
        let sections = model.sections();
        model.scroll = model.scroll.clamp(0.0, layout.max_scroll);

        // Only the click-catcher outside the rounded panel stays transparent.
        canvas.round(
            layout.panel,
            radius::LG,
            Some(palette.panel),
            Some(palette.border),
            None,
        );
        let full = canvas.clip(layout.panel);

        // Header: app glyph and name, with the app switcher on the right.
        let app = model
            .index
            .as_ref()
            .and_then(|index| model.app.and_then(|app| index.apps.get(app)));
        // The app mark at 16 px in the text colour (STANDARD §7, §2).
        canvas.glyph(
            app.map_or("arcade.tools", |app| app.id.as_str()),
            42.0,
            34.0,
            palette.text,
        )?;
        let keys_x = layout.panel.right() - 20.0 - keycap_width(text, "Ctrl P", scale);
        keycap(
            &mut canvas,
            text,
            &palette,
            "Ctrl P",
            keys_x,
            42.0,
            full,
            None,
        );
        let hint = "Choose app";
        let hint_x = keys_x - 8.0 - text.width(hint, ty::SMALL.size * scale, REGULAR) / scale;
        canvas.text(
            text,
            hint,
            ty::SMALL.size,
            REGULAR,
            palette.muted,
            hint_x,
            42.0,
            200.0,
            full,
        );
        canvas.text(
            text,
            model.title(),
            ty::TITLE_SM.size,
            SEMIBOLD,
            palette.text,
            70.0,
            42.0,
            hint_x - 86.0,
            full,
        );

        // Search field with a drawn magnifier and caret.
        let search = layout.search;
        let cy = search.y + search.h / 2.0;
        canvas.round(
            search,
            radius::SM,
            Some(palette.surface),
            Some(palette.strong),
            None,
        );
        canvas.magnifier(search.x + 19.0, cy - 1.0, palette.muted);
        let typed = if model.query.is_empty() {
            let placeholder = if model.picking {
                "Type an app name, or search any shortcut"
            } else {
                "Search shortcuts or keys"
            };
            canvas.text(
                text,
                placeholder,
                ty::HEADING.size,
                REGULAR,
                palette.faint,
                search.x + 42.0,
                cy,
                search.w - 56.0,
                full,
            );
            0.0
        } else {
            canvas.text(
                text,
                &model.query,
                ty::HEADING.size,
                REGULAR,
                palette.text,
                search.x + 38.0,
                cy,
                search.w - 56.0,
                full,
            ) + 1.0
        };
        canvas.round(
            Rect {
                x: search.x + 37.0 + typed,
                y: cy - 8.0,
                w: 1.5,
                h: 16.0,
            },
            0.0,
            Some(palette.accent),
            None,
            None,
        );

        // Scope control: one segmented track, the active scope raised.
        canvas.round(
            layout.track,
            radius::TILE,
            Some(palette.surface),
            Some(palette.border),
            None,
        );
        for (scope, tab) in Scope::ORDER.iter().zip(layout.tabs) {
            let active = model.scope == *scope && !model.picking;
            if active {
                canvas.round(tab, 6.0, Some(palette.panel), Some(palette.border), None);
            }
            let width = text.width(scope.name(), ty::CONTROL.size * scale, MEDIUM) / scale;
            canvas.text(
                text,
                scope.name(),
                ty::CONTROL.size,
                MEDIUM,
                if active { palette.text } else { palette.muted },
                tab.x + (tab.w - width) / 2.0,
                tab.y + tab.h / 2.0,
                tab.w,
                full,
            );
        }

        if let Some(picker) = layout.picker {
            canvas.round(
                picker,
                radius::MD,
                Some(palette.panel),
                Some(palette.border),
                None,
            );
            let candidates = model.picker_candidates();
            let rows = layout.picker_visible_rows();
            let start = model.picker_selected.saturating_sub(rows.saturating_sub(1));
            if let Some(index) = &model.index {
                for (row, candidate) in candidates.iter().enumerate().skip(start).take(rows) {
                    let line = Rect {
                        x: picker.x + 4.0,
                        y: picker.y + 4.0 + (row - start) as f32 * PICKER_ROW,
                        w: picker.w - 8.0,
                        h: PICKER_ROW,
                    };
                    let cy = line.y + line.h / 2.0;
                    if row == model.picker_selected {
                        // Template B: accent-soft fill and a 2 px accent bar.
                        canvas.round(line, radius::SM, Some(palette.accent_soft), None, None);
                        canvas.round(
                            Rect {
                                x: line.x,
                                y: line.y + 8.0,
                                w: size::SELECTION_BAR,
                                h: line.h - 16.0,
                            },
                            1.0,
                            Some(palette.accent),
                            None,
                            None,
                        );
                        let enter = keycap_width(text, "Enter", scale);
                        keycap(
                            &mut canvas,
                            text,
                            &palette,
                            "Enter",
                            line.right() - 6.0 - enter,
                            cy,
                            full,
                            None,
                        );
                    }
                    canvas.text(
                        text,
                        &index.apps[*candidate].name,
                        ty::BODY.size,
                        MEDIUM,
                        palette.text,
                        line.x + 10.0,
                        cy,
                        line.w - 90.0,
                        full,
                    );
                }
            }
            if candidates.is_empty() {
                canvas.text(
                    text,
                    "No matching app. Shortcut results are below.",
                    ty::BODY.size,
                    REGULAR,
                    palette.muted,
                    picker.x + 16.0,
                    picker.y + 23.0,
                    picker.w - 32.0,
                    full,
                );
            }
        }

        if let Some(message) = model.message() {
            let max_width = layout.viewport.w;
            let width =
                (text.width(message, ty::BODY.size * scale, REGULAR) / scale).min(max_width);
            canvas.text(
                text,
                message,
                ty::BODY.size,
                REGULAR,
                palette.muted,
                layout.viewport.x + (max_width - width) / 2.0,
                layout.viewport.y + layout.viewport.h / 2.0,
                max_width,
                full,
            );
        }

        let mut conflicts = false;
        for (section, column) in sections.iter().zip(&layout.columns) {
            let count: usize = section.groups.iter().map(|group| group.rows.len()).sum();
            let heading = canvas.text(
                text,
                &section.title,
                ty::HEADING.size,
                SEMIBOLD,
                palette.text,
                column.x,
                column.y - 22.0,
                column.w - 40.0,
                full,
            );
            canvas.text(
                text,
                &count.to_string(),
                ty::SMALL.size,
                REGULAR,
                palette.faint,
                column.x + heading + 8.0,
                column.y - 22.0,
                40.0,
                full,
            );
            canvas.round(
                Rect {
                    x: column.x,
                    y: column.y - 8.0,
                    w: column.w,
                    h: 1.0,
                },
                0.0,
                Some(palette.border),
                None,
                None,
            );
            let clip = canvas.clip(*column);
            let mask = canvas.mask(*column);
            let mask = mask.as_ref();
            let mut y = column.y - model.scroll;
            for group in &section.groups {
                canvas.text(
                    text,
                    &group.title.to_uppercase(),
                    ty::CAPTION.size,
                    SEMIBOLD,
                    palette.faint,
                    column.x,
                    y + 18.0,
                    column.w,
                    clip,
                );
                y += GROUP_HEIGHT;
                for row in &group.rows {
                    let height = row_height(row);
                    conflicts |= row.conflict;
                    if y + height > column.y && y < column.bottom() {
                        let cy = y + size::ROW_OVERLAY / 2.0;
                        // Keys sit flush right; extra alternatives drop before the title does.
                        let widths: Vec<f32> = row
                            .chips
                            .iter()
                            .map(|chip| keycap_width(text, chip, scale))
                            .collect();
                        let mut keys = 0;
                        let mut total = 0.0;
                        for width in &widths {
                            let next = total + width + if keys > 0 { 6.0 } else { 0.0 };
                            if keys > 0 && next > column.w * 0.6 {
                                break;
                            }
                            total = next.min(column.w * 0.6);
                            keys += 1;
                        }
                        let mut x = column.right() - total;
                        let keys_left = x;
                        for (chip, width) in row.chips.iter().zip(&widths).take(keys) {
                            let width = width.min(column.right() - x);
                            keycap_sized(
                                &mut canvas,
                                text,
                                &palette,
                                chip,
                                x,
                                cy,
                                width,
                                size::KEY_CHIP,
                                clip,
                                mask,
                            );
                            x += width + 6.0;
                        }
                        let mut title_right = keys_left - 12.0;
                        if row.conflict {
                            canvas.dot(keys_left - 10.0, cy, 3.5, palette.warning, mask);
                            title_right -= 12.0;
                        }
                        canvas.text(
                            text,
                            &row.title,
                            ty::BODY.size,
                            MEDIUM,
                            palette.text,
                            column.x,
                            cy,
                            title_right - column.x,
                            clip,
                        );
                        if let Some(detail) = &row.detail {
                            canvas.text(
                                text,
                                detail,
                                ty::SMALL.size,
                                REGULAR,
                                palette.muted,
                                column.x,
                                cy + 17.0,
                                column.w,
                                clip,
                            );
                        }
                    }
                    y += height;
                }
                y += GROUP_GAP;
            }
            if section.groups.is_empty() {
                canvas.text(
                    text,
                    "No matching shortcuts",
                    ty::BODY.size,
                    REGULAR,
                    palette.faint,
                    column.x,
                    column.y + 18.0,
                    column.w,
                    clip,
                );
            }
        }

        if layout.max_scroll > 0.0 && layout.viewport.h >= 24.0 {
            let track = layout.viewport.h;
            let thumb = (track * track / (track + layout.max_scroll)).max(24.0);
            canvas.round(
                Rect {
                    x: layout.panel.right() - 11.0,
                    y: layout.viewport.y + (track - thumb) * model.scroll / layout.max_scroll,
                    w: 4.0,
                    h: thumb,
                },
                2.0,
                Some(palette.strong),
                None,
                None,
            );
        }

        // Footer: the Template B hint bar.
        canvas.round(
            Rect {
                x: layout.panel.x,
                y: layout.panel.bottom() - size::HINT_BAR,
                w: layout.panel.w,
                h: 1.0,
            },
            0.0,
            Some(palette.border),
            None,
            None,
        );
        let cy = layout.panel.bottom() - size::HINT_BAR / 2.0;
        let mut x = layout.panel.x + 20.0;
        for (key, action) in [("Esc", "Close"), ("↑ ↓", "Move"), ("Tab", "Scope")] {
            let width = keycap_width(text, key, scale);
            keycap_sized(
                &mut canvas,
                text,
                &palette,
                key,
                x,
                cy,
                width,
                18.0,
                full,
                None,
            );
            x += width + 6.0;
            x += canvas.text(
                text,
                action,
                ty::CAPTION.size,
                MEDIUM,
                palette.faint,
                x,
                cy,
                120.0,
                full,
            ) + 14.0;
        }
        canvas.text(
            text,
            "Type to filter",
            ty::CAPTION.size,
            MEDIUM,
            palette.faint,
            x,
            cy,
            160.0,
            full,
        );
        if conflicts {
            let legend = "Shortcut conflict";
            let width = text.width(legend, ty::CAPTION.size * scale, MEDIUM) / scale;
            let x = layout.panel.right() - 20.0 - width;
            canvas.text(
                text,
                legend,
                ty::CAPTION.size,
                MEDIUM,
                palette.faint,
                x,
                cy,
                width + 4.0,
                full,
            );
            canvas.dot(x - 10.0, cy, 3.5, palette.warning, None);
        }
        Ok(canvas.pixmap)
    }
}

/// Logical-to-device drawing over the frame; all positions are logical.
struct Canvas {
    pixmap: Pixmap,
    ox: f32,
    oy: f32,
    scale: f32,
}

impl Canvas {
    fn map(&self, r: Rect) -> Rect {
        Rect {
            x: (r.x + self.ox) * self.scale,
            y: (r.y + self.oy) * self.scale,
            w: r.w * self.scale,
            h: r.h * self.scale,
        }
    }

    fn clip(&self, r: Rect) -> Clip {
        let r = self.map(r);
        (
            r.x.floor() as i32,
            r.y.floor() as i32,
            r.right().ceil() as i32,
            r.bottom().ceil() as i32,
        )
    }

    fn mask(&self, r: Rect) -> Option<Mask> {
        let mut mask = Mask::new(self.pixmap.width(), self.pixmap.height())?;
        let r = self.map(r);
        let path = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h)?);
        mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
        Some(mask)
    }

    fn round(
        &mut self,
        r: Rect,
        radius: f32,
        fill: Option<u32>,
        stroke: Option<u32>,
        mask: Option<&Mask>,
    ) {
        let r = self.map(r);
        if let Some(color) = fill {
            if let Some(path) = rounded(r, radius * self.scale) {
                let identity = Transform::identity();
                self.pixmap
                    .fill_path(&path, &paint(color), FillRule::Winding, identity, mask);
            }
        }
        if let Some(color) = stroke {
            // Inset by half the stroke so hairlines land on whole pixels.
            let half = self.scale / 2.0;
            let inner = Rect {
                x: r.x + half,
                y: r.y + half,
                w: r.w - self.scale,
                h: r.h - self.scale,
            };
            if let Some(path) = rounded(inner, (radius * self.scale - half).max(0.0)) {
                let stroke = Stroke {
                    width: self.scale,
                    ..Default::default()
                };
                self.pixmap
                    .stroke_path(&path, &paint(color), &stroke, Transform::identity(), mask);
            }
        }
    }

    fn dot(&mut self, x: f32, y: f32, radius: f32, color: u32, mask: Option<&Mask>) {
        let s = self.scale;
        if let Some(path) =
            PathBuilder::from_circle((x + self.ox) * s, (y + self.oy) * s, radius * s)
        {
            self.pixmap.fill_path(
                &path,
                &paint(color),
                FillRule::Winding,
                Transform::identity(),
                mask,
            );
        }
    }

    fn magnifier(&mut self, x: f32, y: f32, color: u32) {
        let s = self.scale;
        let (x, y) = ((x + self.ox) * s, (y + self.oy) * s);
        let mut path = PathBuilder::new();
        path.push_circle(x, y, 5.5 * s);
        path.move_to(x + 4.0 * s, y + 4.0 * s);
        path.line_to(x + 8.0 * s, y + 8.0 * s);
        if let Some(path) = path.finish() {
            let stroke = Stroke {
                width: 1.6 * s,
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };
            self.pixmap
                .stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
        }
    }

    fn glyph(&mut self, id: &str, x: f32, y: f32, color: u32) -> Result<(), String> {
        let mut icon = Pixmap::new(32, 32).ok_or("cannot allocate app glyph")?;
        icon.data_mut().copy_from_slice(glyph(id));
        // Only the glyph's coverage is used: it takes the given colour.
        let rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
        for pixel in icon.data_mut().as_chunks_mut::<4>().0 {
            for channel in 0..3 {
                pixel[channel] = (rgb[channel] as u32 * pixel[3] as u32 / 255) as u8;
            }
        }
        let size = 16.0 / 32.0 * self.scale;
        self.pixmap.draw_pixmap(
            0,
            0,
            icon.as_ref(),
            &PixmapPaint {
                quality: FilterQuality::Bicubic,
                ..Default::default()
            },
            Transform::from_scale(size, size)
                .post_translate((x + self.ox) * self.scale, (y + self.oy) * self.scale),
            None,
        );
        Ok(())
    }

    /// Draws one line vertically centred on `cy`; returns its logical width.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &mut Text,
        value: &str,
        size: f32,
        weight: u16,
        color: u32,
        x: f32,
        cy: f32,
        max_width: f32,
        clip: Clip,
    ) -> f32 {
        let device = size * self.scale;
        let fitted = text.fit(value, device, weight, max_width.max(0.0) * self.scale);
        let top = (cy - size * 0.65 + self.oy) * self.scale;
        if top <= clip.3 as f32 && top + device * 1.3 >= clip.1 as f32 {
            let left = (x + self.ox) * self.scale;
            text.draw(
                &mut self.pixmap,
                &fitted,
                device,
                weight,
                color,
                left,
                top,
                clip,
            );
        }
        text.width(&fitted, device, weight) / self.scale
    }
}

fn keycap_width(text: &mut Text, label: &str, scale: f32) -> f32 {
    text.width(label, ty::CAPTION.size * scale, MEDIUM) / scale + 14.0
}

#[allow(clippy::too_many_arguments)]
fn keycap(
    canvas: &mut Canvas,
    text: &mut Text,
    palette: &Palette,
    label: &str,
    x: f32,
    cy: f32,
    clip: Clip,
    mask: Option<&Mask>,
) {
    let width = keycap_width(text, label, canvas.scale);
    keycap_sized(
        canvas,
        text,
        palette,
        label,
        x,
        cy,
        width,
        size::KEY_CHIP,
        clip,
        mask,
    );
}

#[allow(clippy::too_many_arguments)]
fn keycap_sized(
    canvas: &mut Canvas,
    text: &mut Text,
    palette: &Palette,
    label: &str,
    x: f32,
    cy: f32,
    width: f32,
    height: f32,
    clip: Clip,
    mask: Option<&Mask>,
) {
    canvas.round(
        Rect {
            x,
            y: cy - height / 2.0,
            w: width,
            h: height,
        },
        radius::XS,
        Some(palette.surface),
        Some(palette.strong),
        mask,
    );
    canvas.text(
        text,
        label,
        ty::CAPTION.size,
        MEDIUM,
        palette.text,
        x + 7.0,
        cy,
        // Slack for float rounding, so an exactly sized cap never ellipsizes.
        width - 13.0,
        clip,
    );
}

fn rounded(r: Rect, radius: f32) -> Option<Path> {
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(
            r.x, r.y, r.w, r.h,
        )?));
    }
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0);
    // Cubic approximation of a quarter circle.
    let k = radius * (1.0 - 0.552_284_8);
    let (left, top, right, bottom) = (r.x, r.y, r.right(), r.bottom());
    let mut path = PathBuilder::new();
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.cubic_to(right - k, top, right, top + k, right, top + radius);
    path.line_to(right, bottom - radius);
    path.cubic_to(right, bottom - k, right - k, bottom, right - radius, bottom);
    path.line_to(left + radius, bottom);
    path.cubic_to(left + k, bottom, left, bottom - k, left, bottom - radius);
    path.line_to(left, top + radius);
    path.cubic_to(left, top + k, left + k, top, left + radius, top);
    path.close();
    path.finish()
}

fn paint(color: u32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8((color >> 16) as u8, (color >> 8) as u8, color as u8, 255);
    paint
}

fn glyph(id: &str) -> &'static [u8] {
    match id {
        "arcade.box" => include_bytes!(concat!(env!("OUT_DIR"), "/box.rgba")),
        "arcade.lens" => include_bytes!(concat!(env!("OUT_DIR"), "/lens.rgba")),
        "arcade.look" => include_bytes!(concat!(env!("OUT_DIR"), "/look.rgba")),
        "arcade.wheel" => include_bytes!(concat!(env!("OUT_DIR"), "/wheel.rgba")),
        "arcade.clipboard" => include_bytes!(concat!(env!("OUT_DIR"), "/clipboard.rgba")),
        "arcade.find" => include_bytes!(concat!(env!("OUT_DIR"), "/find.rgba")),
        "arcade.shelf" => include_bytes!(concat!(env!("OUT_DIR"), "/shelf.rgba")),
        _ => include_bytes!(concat!(env!("OUT_DIR"), "/tools.rgba")),
    }
}
