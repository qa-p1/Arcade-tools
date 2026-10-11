//! Opaque panel, tiny-skia + cosmic-text software rendering, adapted from
//! Arcade Find (MIT OR Apache-2.0). No SVG engine or GPU context is needed.

use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Transform};

use crate::layout::{Layout, Rect, GROUP_HEIGHT, HEIGHT, ROW_HEIGHT, WIDTH};
use crate::model::{Model, Scope};

use super::text::Text;

#[derive(Clone, Copy)]
pub struct Palette {
    panel: u32,
    inset: u32,
    text: u32,
    muted: u32,
    accent: u32,
    border: u32,
    warning: u32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                panel: 0x14171c,
                inset: 0x1c2027,
                text: 0xe7eaf0,
                muted: 0x9aa3b2,
                accent: 0x4ade80,
                border: 0x343b47,
                warning: 0xf5bc60,
            }
        } else {
            Self {
                panel: 0xffffff,
                inset: 0xf2f4f7,
                text: 0x161a20,
                muted: 0x5b6472,
                accent: 0x15803d,
                border: 0xdde1e7,
                warning: 0x975500,
            }
        }
    }
}

pub struct Renderer {
    text: Text,
    pub dark: bool,
}
impl Renderer {
    pub fn new(dark: bool) -> Self {
        Self {
            text: Text::default(),
            dark,
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
        let mut pixmap = Pixmap::new(width, height).ok_or("cannot allocate the overlay frame")?;
        let (layout, ox, oy) = Self::geometry(model, width as f32 / scale, height as f32 / scale);
        let palette = Palette::new(self.dark);
        let sections = model.sections();
        model.scroll = model.scroll.clamp(0.0, layout.max_scroll);
        let rect = |r: Rect| Rect {
            x: (r.x + ox) * scale,
            y: (r.y + oy) * scale,
            w: r.w * scale,
            h: r.h * scale,
        };
        let panel = rect(layout.panel);
        fill(&mut pixmap, panel, palette.panel);
        // A fully opaque panel; only the click-catcher outside it is transparent.
        let full_clip = (
            panel.x as i32,
            panel.y as i32,
            panel.right() as i32,
            panel.bottom() as i32,
        );
        let mut label = |text: &str,
                         size: f32,
                         bold: bool,
                         color: u32,
                         x: f32,
                         y: f32,
                         max_width: f32,
                         clip| {
            let text = self
                .text
                .fit(text, size * scale, bold, max_width.max(0.0) * scale);
            self.text.draw(
                &mut pixmap,
                &text,
                size * scale,
                bold,
                color,
                (x + ox) * scale,
                (y + oy) * scale,
                clip,
            );
        };
        label(
            model.title(),
            22.0,
            true,
            palette.text,
            36.0,
            27.0,
            layout.panel.w - 280.0,
            full_clip,
        );
        label(
            "Arcade Tools   ·   Choose app  Ctrl+P",
            12.0,
            false,
            palette.muted,
            layout.panel.right() - 255.0,
            33.0,
            240.0,
            full_clip,
        );
        fill(&mut pixmap, rect(layout.search), palette.inset);
        border(&mut pixmap, rect(layout.search), palette.accent, scale);
        let placeholder = if model.picking {
            "Type an app name · Enter to pick · or search any shortcut"
        } else {
            "Search shortcuts or keys…"
        };
        let search_text = if model.query.is_empty() {
            placeholder
        } else {
            &model.query
        };
        self.draw_label(
            &mut pixmap,
            search_text,
            15.0,
            false,
            if model.query.is_empty() {
                palette.muted
            } else {
                palette.text
            },
            rect(Rect {
                x: layout.search.x + 12.0,
                y: layout.search.y + 10.0,
                w: layout.search.w - 24.0,
                h: 26.0,
            }),
            full_clip,
            scale,
        );
        for (scope, tab) in Scope::ORDER.iter().zip(layout.tabs) {
            if model.scope == *scope {
                fill(&mut pixmap, rect(tab), palette.inset);
                border(&mut pixmap, rect(tab), palette.border, scale);
            }
            self.draw_label(
                &mut pixmap,
                scope.name(),
                12.0,
                model.scope == *scope,
                if model.scope == *scope {
                    palette.accent
                } else {
                    palette.muted
                },
                rect(Rect {
                    x: tab.x + 10.0,
                    y: tab.y + 6.0,
                    w: tab.w - 20.0,
                    h: 20.0,
                }),
                full_clip,
                scale,
            );
        }
        if let Some(picker) = layout.picker {
            fill(&mut pixmap, rect(picker), palette.inset);
            let candidates = model.picker_candidates();
            let rows = layout.picker_visible_rows();
            let start = model.picker_selected.saturating_sub(rows.saturating_sub(1));
            if let Some(index) = &model.index {
                for (row, candidate) in candidates.iter().enumerate().skip(start).take(rows) {
                    let line = Rect {
                        x: picker.x + 8.0,
                        y: picker.y + 8.0 + (row - start) as f32 * 34.0,
                        w: picker.w - 16.0,
                        h: 30.0,
                    };
                    if row == model.picker_selected {
                        border(&mut pixmap, rect(line), palette.accent, scale);
                    }
                    self.draw_label(
                        &mut pixmap,
                        &index.apps[*candidate].name,
                        14.0,
                        false,
                        palette.text,
                        rect(Rect {
                            x: line.x + 10.0,
                            y: line.y + 5.0,
                            w: line.w - 20.0,
                            h: 22.0,
                        }),
                        full_clip,
                        scale,
                    );
                }
            }
            if candidates.is_empty() {
                self.draw_label(
                    &mut pixmap,
                    "No matching app. Shortcut results are below.",
                    13.0,
                    false,
                    palette.muted,
                    rect(Rect {
                        x: picker.x + 12.0,
                        y: picker.y + 12.0,
                        w: picker.w - 24.0,
                        h: 22.0,
                    }),
                    full_clip,
                    scale,
                );
            }
        }
        if let Some(message) = model.message() {
            self.draw_label(
                &mut pixmap,
                message,
                17.0,
                false,
                palette.text,
                rect(Rect {
                    x: 48.0,
                    y: 230.0,
                    w: layout.panel.w - 64.0,
                    h: 30.0,
                }),
                full_clip,
                scale,
            );
        }
        for (section, column) in sections.iter().zip(&layout.columns) {
            self.draw_label(
                &mut pixmap,
                &section.title,
                14.0,
                true,
                palette.accent,
                rect(Rect {
                    x: column.x,
                    y: column.y - 30.0,
                    w: column.w,
                    h: 26.0,
                }),
                full_clip,
                scale,
            );
            let clip_rect = rect(*column);
            let clip = (
                clip_rect.x as i32,
                clip_rect.y as i32,
                clip_rect.right() as i32,
                clip_rect.bottom() as i32,
            );
            let mut y = column.y - model.scroll;
            for group in &section.groups {
                self.draw_label(
                    &mut pixmap,
                    &group.title,
                    12.0,
                    true,
                    palette.muted,
                    rect(Rect {
                        x: column.x,
                        y: y + 5.0,
                        w: column.w,
                        h: 22.0,
                    }),
                    clip,
                    scale,
                );
                y += GROUP_HEIGHT;
                for row in &group.rows {
                    if y + ROW_HEIGHT > column.y && y < column.bottom() {
                        self.draw_label(
                            &mut pixmap,
                            &row.title,
                            14.0,
                            false,
                            palette.text,
                            rect(Rect {
                                x: column.x,
                                y,
                                w: column.w - if row.conflict { 24.0 } else { 0.0 },
                                h: 22.0,
                            }),
                            clip,
                            scale,
                        );
                        if row.conflict {
                            let badge = rect(Rect {
                                x: column.right() - 20.0,
                                y: y + 2.0,
                                w: 18.0,
                                h: 18.0,
                            });
                            fill_clipped(&mut pixmap, badge, palette.warning, clip);
                            self.draw_label(
                                &mut pixmap,
                                "!",
                                12.0,
                                true,
                                palette.panel,
                                Rect {
                                    x: badge.x + 6.0 * scale,
                                    y: badge.y,
                                    w: badge.w,
                                    h: badge.h,
                                },
                                clip,
                                scale,
                            );
                        }
                        let mut x = column.x;
                        for chip in &row.chips {
                            let chip_width = (self.text.width(chip, 12.0 * scale, false) / scale
                                + 16.0)
                                .min(column.right() - x);
                            if chip_width < 18.0 {
                                break;
                            }
                            let chip_rect = rect(Rect {
                                x,
                                y: y + 25.0,
                                w: chip_width,
                                h: 24.0,
                            });
                            fill_clipped(&mut pixmap, chip_rect, palette.inset, clip);
                            self.draw_label(
                                &mut pixmap,
                                chip,
                                12.0,
                                false,
                                palette.text,
                                Rect {
                                    x: chip_rect.x + 8.0 * scale,
                                    y: chip_rect.y + 3.0 * scale,
                                    w: chip_rect.w - 16.0 * scale,
                                    h: chip_rect.h,
                                },
                                clip,
                                scale,
                            );
                            x += chip_width + 6.0;
                        }
                        if let Some(detail) = &row.detail {
                            self.draw_label(
                                &mut pixmap,
                                detail,
                                11.0,
                                false,
                                palette.muted,
                                rect(Rect {
                                    x: column.x,
                                    y: y + 53.0,
                                    w: column.w,
                                    h: 20.0,
                                }),
                                clip,
                                scale,
                            );
                        }
                    }
                    y += ROW_HEIGHT;
                }
                y += 10.0;
            }
            if section.groups.is_empty() {
                self.draw_label(
                    &mut pixmap,
                    "No matching shortcuts",
                    13.0,
                    false,
                    palette.muted,
                    rect(Rect {
                        x: column.x,
                        y: column.y + 12.0,
                        w: column.w,
                        h: 24.0,
                    }),
                    clip,
                    scale,
                );
            }
        }
        if layout.max_scroll > 0.0 && layout.viewport.h >= 24.0 {
            let track = layout.viewport.h;
            let thumb = (track * track / (track + layout.max_scroll)).max(24.0);
            fill(
                &mut pixmap,
                rect(Rect {
                    x: layout.panel.right() - 10.0,
                    y: layout.viewport.y + (track - thumb) * model.scroll / layout.max_scroll,
                    w: 3.0,
                    h: thumb,
                }),
                palette.muted,
            );
        }
        self.draw_label(&mut pixmap, "Tab  scope    Up Down / PgUp PgDn  scroll    Ctrl+P  choose app    Esc  close    !  shortcut conflict", 11.0, false, palette.muted, rect(Rect { x: 36.0, y: layout.panel.bottom() - 26.0, w: layout.panel.w - 40.0, h: 20.0 }), full_clip, scale);
        Ok(pixmap)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_label(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        size: f32,
        bold: bool,
        color: u32,
        rect: Rect,
        clip: (i32, i32, i32, i32),
        scale: f32,
    ) {
        if rect.bottom() < clip.1 as f32 || rect.y > clip.3 as f32 {
            return;
        }
        let text = self.text.fit(text, size * scale, bold, rect.w.max(0.0));
        self.text.draw(
            pixmap,
            &text,
            size * scale,
            bold,
            color,
            rect.x,
            rect.y,
            clip,
        );
    }
}

fn fill(pixmap: &mut Pixmap, rect: Rect, color: u32) {
    let Some(rect) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(
        (color >> 16) as u8,
        (color >> 8) as u8,
        color as u8,
        255,
    ));
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn fill_clipped(pixmap: &mut Pixmap, rect: Rect, color: u32, clip: (i32, i32, i32, i32)) {
    let x = rect.x.max(clip.0 as f32);
    let y = rect.y.max(clip.1 as f32);
    fill(
        pixmap,
        Rect {
            x,
            y,
            w: (rect.right().min(clip.2 as f32) - x).max(0.0),
            h: (rect.bottom().min(clip.3 as f32) - y).max(0.0),
        },
        color,
    );
}

fn border(pixmap: &mut Pixmap, rect: Rect, color: u32, scale: f32) {
    let Some(rect) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h) else {
        return;
    };
    let path = PathBuilder::from_rect(rect);
    let mut paint = Paint::default();
    paint.set_color_rgba8((color >> 16) as u8, (color >> 8) as u8, color as u8, 255);
    let stroke = tiny_skia::Stroke {
        width: scale,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}
