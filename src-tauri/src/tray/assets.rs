use arcade_link::design::{palette, rgb, Theme};

/// The tray mark: Tools' glyph, monochrome, with a badge dot for updates.
/// White on Linux panels (dark on GNOME, Plasma's default and the usual
/// Hyprland bars), black on macOS (a template image the menu bar recolours),
/// and the taskbar's contrast colour on Windows.
pub fn mark(badge: bool) -> Vec<u8> {
    let dark_panel = dark_panel();
    let mut rgba = if dark_panel {
        include_bytes!(concat!(env!("OUT_DIR"), "/mark-white.rgba")).to_vec()
    } else {
        include_bytes!(concat!(env!("OUT_DIR"), "/mark-black.rgba")).to_vec()
    };
    if badge {
        let theme = if dark_panel {
            Theme::Dark
        } else {
            Theme::Light
        };
        let [r, g, b] = rgb(palette(theme, "arcade.tools").danger);
        for y in 22i32..32 {
            for x in 22i32..32 {
                if (x - 27).pow(2) + (y - 27).pow(2) <= 20 {
                    let at = (y as usize * 32 + x as usize) * 4;
                    rgba[at..at + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
    }
    rgba
}

/// The tooltip: the name, then the tagline (STANDARD §8).
#[cfg(any(not(target_os = "linux"), feature = "native-tray-check"))]
pub fn tooltip() -> String {
    format!(
        "Arcade Tools\n{}",
        arcade_link::manifest::app_pitch("arcade.tools")
    )
}

#[cfg(target_os = "windows")]
fn dark_panel() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and the output is a u32.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    // A missing value means the classic dark taskbar.
    status != 0 || value == 0
}

#[cfg(target_os = "macos")]
fn dark_panel() -> bool {
    false
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn dark_panel() -> bool {
    true
}

macro_rules! glyphs {
    ($function:ident, $extension:literal) => {
        pub fn $function(id: &str) -> &'static [u8] {
            match id {
                "arcade.box" => include_bytes!(concat!(env!("OUT_DIR"), "/box", $extension)),
                "arcade.lens" => include_bytes!(concat!(env!("OUT_DIR"), "/lens", $extension)),
                "arcade.look" => include_bytes!(concat!(env!("OUT_DIR"), "/look", $extension)),
                "arcade.wheel" => include_bytes!(concat!(env!("OUT_DIR"), "/wheel", $extension)),
                "arcade.clipboard" => {
                    include_bytes!(concat!(env!("OUT_DIR"), "/clipboard", $extension))
                }
                "arcade.shelf" => include_bytes!(concat!(env!("OUT_DIR"), "/shelf", $extension)),
                "arcade.find" => include_bytes!(concat!(env!("OUT_DIR"), "/find", $extension)),
                _ => include_bytes!(concat!(env!("OUT_DIR"), "/tools", $extension)),
            }
        }
    };
}
#[cfg(target_os = "linux")]
glyphs!(png, ".png");
#[cfg(any(not(target_os = "linux"), feature = "native-tray-check"))]
glyphs!(rgba, ".rgba");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mark_and_badge_are_rgba_and_distinct() {
        assert_eq!(mark(false).len(), 32 * 32 * 4);
        assert_ne!(mark(false), mark(true));
    }
}
