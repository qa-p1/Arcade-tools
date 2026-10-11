//! Native backends adapted from Arcade Find (MIT OR Apache-2.0): winit +
//! softbuffer for desktop windows and SCTK layer shell where advertised.

mod desktop;
mod render;
mod text;
#[cfg(target_os = "linux")]
mod wayland;

use crate::{focus::Focus, instance::Instance, model::Model, timing::Timing};
use render::Renderer;

pub enum Key {
    Escape,
    Tab,
    Backspace,
    Enter,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Picker,
    Clear,
    Text(String),
}

pub struct Overlay {
    pub model: Model,
    renderer: Renderer,
    pub timing: Timing,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub first_frame: bool,
    pub restore_focus: bool,
}

impl Overlay {
    fn new(model: Model, dark: bool, timing: Timing) -> Self {
        let began = std::time::Instant::now();
        let renderer = Renderer::new(dark);
        timing.phase("fonts", began);
        Self {
            model,
            renderer,
            timing,
            width: crate::layout::WIDTH,
            height: crate::layout::HEIGHT,
            scale: 1.0,
            first_frame: false,
            restore_focus: true,
        }
    }

    fn key(&mut self, key: Key, shift: bool) -> bool {
        match key {
            Key::Escape => return true,
            Key::Tab => self.model.cycle(shift),
            Key::Backspace => self.model.backspace(),
            Key::Enter if self.model.picking => self.model.pick(None),
            Key::Up if self.model.picking => self.model.move_picker(-1),
            Key::Down if self.model.picking => self.model.move_picker(1),
            Key::Up => self.scroll(-crate::layout::ROW_HEIGHT),
            Key::Down => self.scroll(crate::layout::ROW_HEIGHT),
            Key::PageUp => self.scroll(-self.viewport_height() * 0.85),
            Key::PageDown => self.scroll(self.viewport_height() * 0.85),
            Key::Home => self.model.scroll = 0.0,
            Key::End => self.scroll(f32::MAX),
            Key::Picker => self.model.open_picker(),
            Key::Clear => self.model.clear_query(),
            Key::Text(text) => self.model.type_text(&text),
            _ => {}
        }
        false
    }

    fn viewport_height(&self) -> f32 {
        Renderer::geometry(
            &self.model,
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        )
        .0
        .viewport
        .h
    }
    fn scroll(&mut self, delta: f32) {
        let (layout, _, _) = Renderer::geometry(
            &self.model,
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        );
        self.model.scroll = layout.scroll(self.model.scroll, delta);
    }

    /// Positions are logical surface coordinates (both backends).
    fn click(&mut self, x: f32, y: f32) -> bool {
        let (layout, ox, oy) = Renderer::geometry(
            &self.model,
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        );
        let (x, y) = (x - ox, y - oy);
        if !layout.panel.contains(x, y) {
            return true;
        }
        if y < layout.search.y && x > layout.panel.right() - 255.0 {
            self.model.open_picker();
        }
        for (scope, tab) in crate::model::Scope::ORDER.iter().zip(layout.tabs) {
            if tab.contains(x, y) {
                self.model.scope = *scope;
                self.model.picking = false;
                self.model.scroll = 0.0;
            }
        }
        if let Some(picker) = layout.picker {
            if picker.contains(x, y) {
                let rows = layout.picker_visible_rows();
                let start = self
                    .model
                    .picker_selected
                    .saturating_sub(rows.saturating_sub(1));
                let row = ((y - picker.y - 8.0).max(0.0) / 34.0) as usize;
                if row < rows {
                    let candidate = self.model.picker_candidates().get(start + row).copied();
                    if candidate.is_some() {
                        self.model.pick(candidate);
                    }
                }
            }
        }
        false
    }

    fn draw(&mut self) -> Result<tiny_skia::Pixmap, String> {
        let began = std::time::Instant::now();
        let frame = self
            .renderer
            .render(&mut self.model, self.width, self.height, self.scale)?;
        if !self.first_frame {
            self.timing.phase("render", began);
        }
        Ok(frame)
    }

    fn presented(&mut self, backend: &str) {
        if !self.first_frame {
            self.timing.first_frame(backend);
            self.first_frame = true;
        }
    }
}

pub fn run(
    model: Model,
    focus: Focus,
    instance: &Instance,
    timing: Timing,
    dark: bool,
) -> Result<(), String> {
    let overlay = Overlay::new(model, dark, timing);
    #[cfg(target_os = "linux")]
    if crate::focus::is_wayland() {
        let began = std::time::Instant::now();
        let setup = wayland::prepare()?;
        overlay.timing.phase("wayland-probe", began);
        if let Some(setup) = setup {
            return wayland::run(overlay, instance, setup);
        }
    }
    desktop::run(overlay, focus, instance)
}

/// Cheap GTK preference read, called on the startup loader worker. Native
/// Windows/macOS theme comes from winit; no portal connection delays mapping.
pub fn dark_preference() -> bool {
    if let Ok(theme) = std::env::var("GTK_THEME") {
        return theme.to_lowercase().contains("dark");
    }
    #[cfg(target_os = "linux")]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.config_dir().to_path_buf()));
        if let Some(config) = config {
            for folder in ["gtk-4.0", "gtk-3.0"] {
                if let Ok(settings) =
                    std::fs::read_to_string(config.join(folder).join("settings.ini"))
                {
                    if let Some(dark) = gtk_dark(&settings) {
                        return dark;
                    }
                }
            }
        }
    }
    true
}

#[cfg(any(target_os = "linux", test))]
fn gtk_dark(settings: &str) -> Option<bool> {
    for line in settings.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "gtk-application-prefer-dark-theme" {
            return Some(matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true"
            ));
        }
    }
    settings
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "gtk-theme-name")
        .map(|(_, value)| value.to_lowercase().contains("dark"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gtk_light_dark_preferences() {
        assert_eq!(
            gtk_dark("[Settings]\ngtk-theme-name=Adwaita\ngtk-application-prefer-dark-theme=1"),
            Some(true)
        );
        assert_eq!(
            gtk_dark("gtk-application-prefer-dark-theme = false"),
            Some(false)
        );
        assert_eq!(gtk_dark("gtk-theme-name=Adwaita-dark"), Some(true));
        assert_eq!(gtk_dark("# ignored"), None);
    }
}
