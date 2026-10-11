//! Shared logical layout for both backends; all row clipping is to the viewport.

use crate::model::{Model, Section};

pub const WIDTH: u32 = 1100;
pub const HEIGHT: u32 = 720;
pub const ROW_HEIGHT: f32 = 78.0;
pub const GROUP_HEIGHT: f32 = 32.0;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn right(self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

pub struct Layout {
    pub panel: Rect,
    pub search: Rect,
    pub tabs: [Rect; 4],
    pub picker: Option<Rect>,
    pub viewport: Rect,
    pub columns: Vec<Rect>,
    pub max_scroll: f32,
}

pub fn content_height(section: &Section) -> f32 {
    section
        .groups
        .iter()
        .map(|group| GROUP_HEIGHT + group.rows.len() as f32 * ROW_HEIGHT + 10.0)
        .sum()
}

impl Layout {
    pub fn new(model: &Model, width: f32, height: f32, sections: &[Section]) -> Self {
        let width = width.max(300.0);
        let height = height.max(280.0);
        let panel = Rect {
            x: 16.0,
            y: 16.0,
            w: width - 32.0,
            h: height - 32.0,
        };
        let search = Rect {
            x: 36.0,
            y: 68.0,
            w: width - 72.0,
            h: 42.0,
        };
        let tab_width = ((width - 72.0) / 4.0).min(172.0);
        let tabs = std::array::from_fn(|i| Rect {
            x: 36.0 + i as f32 * tab_width,
            y: 120.0,
            w: tab_width - 6.0,
            h: 30.0,
        });
        let picker = model.picking.then_some(Rect {
            x: 36.0,
            y: 160.0,
            w: width - 72.0,
            h: (height - 242.0).clamp(34.0, 118.0),
        });
        let top = picker.map_or(190.0, |picker| picker.bottom() + 38.0);
        let viewport = Rect {
            x: 36.0,
            y: top,
            w: width - 72.0,
            h: (height - top - 58.0).max(0.0),
        };
        let count = sections.len().max(1);
        let column_width = (viewport.w - (count - 1) as f32 * 18.0) / count as f32;
        let columns = (0..sections.len())
            .map(|i| Rect {
                x: viewport.x + i as f32 * (column_width + 18.0),
                y: viewport.y,
                w: column_width,
                h: viewport.h,
            })
            .collect();
        let max_scroll =
            (sections.iter().map(content_height).fold(0.0, f32::max) - viewport.h).max(0.0);
        Self {
            panel,
            search,
            tabs,
            picker,
            viewport,
            columns,
            max_scroll,
        }
    }

    pub fn scroll(&self, current: f32, delta: f32) -> f32 {
        (current + delta).clamp(0.0, self.max_scroll)
    }

    pub fn picker_visible_rows(&self) -> usize {
        self.picker.map_or(0, |picker| {
            (((picker.h - 4.0).max(0.0) / 34.0).floor() as usize).min(3)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;

    #[test]
    fn columns_and_controls_never_overlap_and_scroll_reaches_last_row() {
        for size in [(1100.0, 720.0), (800.0, 600.0), (600.0, 480.0)] {
            let model = Model::new(
                Some(
                    Index::parse(include_bytes!("../tests/fixtures/shortcuts-index.json")).unwrap(),
                ),
                Some(0),
            );
            let sections = model.sections();
            let layout = Layout::new(&model, size.0, size.1, &sections);
            assert!(layout.search.bottom() <= layout.tabs[0].y);
            assert!(layout.tabs[0].bottom() < layout.viewport.y);
            for pair in layout.columns.windows(2) {
                assert!(pair[0].right() < pair[1].x);
            }
            for column in &layout.columns {
                assert!(column.right() <= layout.panel.right());
                assert!(column.bottom() <= layout.panel.bottom());
            }
            assert!(layout.max_scroll > 0.0);
            assert_eq!(layout.scroll(0.0, -50.0), 0.0);
            assert_eq!(layout.scroll(0.0, 1e6), layout.max_scroll);
            let wm_height = content_height(&sections[2]);
            assert!((wm_height - layout.max_scroll - layout.viewport.h).abs() < 0.01);
        }
    }

    #[test]
    fn picker_is_above_the_scrolling_content() {
        let model = Model::new(
            Some(Index::parse(include_bytes!("../tests/fixtures/shortcuts-index.json")).unwrap()),
            None,
        );
        let sections = model.sections();
        for height in [720.0, 360.0, 280.0] {
            let layout = Layout::new(&model, 1100.0, height, &sections);
            let picker = layout.picker.unwrap();
            assert!(picker.bottom() < layout.viewport.y);
            assert!(layout.picker_visible_rows() > 0);
            let last_row_bottom =
                picker.y + 8.0 + (layout.picker_visible_rows() - 1) as f32 * 34.0 + 30.0;
            assert!(last_row_bottom <= picker.bottom());
        }
    }
}
