use arcade_link::design::{palette, rgb, Theme};

/// The tray mark: Tools' glyph, monochrome, with a badge dot for updates.
/// White on Linux panels (dark on GNOME, Plasma's default and the usual
/// Hyprland bars), black on macOS (a template image the menu bar recolours),
/// and the taskbar's contrast colour on Windows.
pub fn mark(badge: bool) -> Vec<u8> {
    let dark_panel = dark_panel();
    let mut rgba = if dark_panel {
        outlined(include_bytes!(concat!(env!("OUT_DIR"), "/mark-white.rgba")))
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

/// A white mark with a one-pixel dark edge, so it stays visible on a light panel too: Linux has no reliable way to
/// ask the panel's colour, and the white mark vanished on openbox's and XFCE's light panels (D44). Each transparent
/// pixel next to the mark takes a translucent dark edge, as strong as its most opaque neighbour.
fn outlined(white: &[u8]) -> Vec<u8> {
    const SIZE: i32 = 32;
    const EDGE: [u8; 3] = [0x20, 0x22, 0x26];
    let alpha = |x: i32, y: i32| -> u8 {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&y) {
            white[((y * SIZE + x) * 4 + 3) as usize]
        } else {
            0
        }
    };
    let mut rgba = white.to_vec();
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = ((y * SIZE + x) * 4) as usize;
            if white[at + 3] != 0 {
                continue;
            }
            let near = (-1..=1)
                .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                .map(|(dx, dy)| alpha(x + dx, y + dy))
                .max()
                .unwrap_or(0);
            if near > 0 {
                let [r, g, b] = EDGE;
                rgba[at..at + 4].copy_from_slice(&[r, g, b, (u16::from(near) * 3 / 5) as u8]);
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
    #[test]
    fn the_white_mark_has_a_dark_edge_and_keeps_its_white() {
        let white = include_bytes!(concat!(env!("OUT_DIR"), "/mark-white.rgba"));
        let edged = outlined(white);
        assert_eq!(edged.len(), white.len());
        let pixels = |bytes: &[u8]| bytes.chunks(4).map(|p| p.to_vec()).collect::<Vec<_>>();
        let (before, after) = (pixels(white), pixels(&edged));
        // Every visible pixel of the mark is unchanged; some transparent ones around it are now a dark edge.
        assert!(before.iter().zip(&after).all(|(b, a)| b[3] == 0 || b == a));
        let edge = after
            .iter()
            .zip(&before)
            .filter(|(a, b)| b[3] == 0 && a[3] > 0)
            .count();
        assert!(edge > 20, "{edge} edge pixels");
        assert!(after
            .iter()
            .zip(&before)
            .filter(|(a, b)| b[3] == 0 && a[3] > 0)
            .all(|(a, _)| a[0] < 0x40));
    }
}
