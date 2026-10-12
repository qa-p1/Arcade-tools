pub fn mark(badge: bool) -> Vec<u8> {
    let mut rgba = include_bytes!(concat!(env!("OUT_DIR"), "/mark.rgba")).to_vec();
    if badge {
        for y in 22i32..32 {
            for x in 22i32..32 {
                if (x - 27).pow(2) + (y - 27).pow(2) <= 20 {
                    let at = (y as usize * 32 + x as usize) * 4;
                    rgba[at..at + 4].copy_from_slice(&[235, 76, 87, 255]);
                }
            }
        }
    }
    rgba
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
