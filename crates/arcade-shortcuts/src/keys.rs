//! Hyprland key strings and modmasks → Arcade's canonical chord notation.
//!
//! Canonical form (Link SPEC §8.5): modifiers `Ctrl`, `Alt`, `Shift`, `Super`
//! in that order, then one key, joined by `+` (`Ctrl+Super+Left`). A modifier
//! on its own is a chord (`Super`, for Caelestia's `SUPER + SUPER_L` launcher).

/// Modifier bits as Hyprland reports them in `hyprctl binds -j` (`modmask`).
const MOD_SHIFT: u32 = 1;
const MOD_CAPS: u32 = 2;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8;
const MOD_MOD2: u32 = 16;
const MOD_MOD3: u32 = 32;
const MOD_SUPER: u32 = 64;
const MOD_MOD5: u32 = 128;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Mods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    sup: bool,
}

impl Mods {
    fn chord(self, key: Option<&str>) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.sup {
            parts.push("Super");
        }
        if let Some(key) = key {
            parts.push(key);
        }
        parts.join("+")
    }

    fn set(&mut self, name: &str) -> bool {
        match name {
            "ctrl" | "control" | "ctl" => self.ctrl = true,
            "alt" | "mod1" | "option" => self.alt = true,
            "shift" => self.shift = true,
            "super" | "win" | "logo" | "mod4" | "meta" | "cmd" | "command" => self.sup = true,
            _ => return false,
        }
        true
    }
}

/// `SUPER + SHIFT + left` (the Lua API's form) → `Shift+Super+Left`.
///
/// `None` when the string uses something the canonical notation can't express
/// (CapsLock or Mod2/3/5 as modifiers, an empty key).
pub fn chord_from_key_string(text: &str) -> Option<String> {
    if let Some(prefix) = text.trim().strip_suffix('+') {
        if prefix.trim().ends_with('+') || prefix.trim().is_empty() {
            let prefix = prefix.trim().trim_end_matches('+').trim();
            return chord_from_conf(prefix, "+");
        }
    }
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    // A literal plus key ("SUPER + +") leaves an empty last part.
    let (key, mods) = parts.split_last()?;
    let mut m = Mods::default();
    for part in mods {
        if part.is_empty() {
            continue;
        }
        if !m.set(&part.to_ascii_lowercase()) {
            return None;
        }
    }
    finish(m, key)
}

/// A `hyprctl binds -j` entry (`modmask`, `key`) → canonical chord.
pub fn chord_from_modmask(modmask: u32, key: &str) -> Option<String> {
    if modmask & (MOD_CAPS | MOD_MOD2 | MOD_MOD3 | MOD_MOD5) != 0 {
        return None;
    }
    let m = Mods {
        ctrl: modmask & MOD_CTRL != 0,
        alt: modmask & MOD_ALT != 0,
        shift: modmask & MOD_SHIFT != 0,
        sup: modmask & MOD_SUPER != 0,
    };
    finish(m, key)
}

/// hyprlang `bind = SUPER SHIFT, Q, …` modifier field (also `SUPER_SHIFT`,
/// `SUPERSHIFT`) with the key field → canonical chord. `$variables` must be
/// expanded by the caller.
pub fn chord_from_conf(mods: &str, key: &str) -> Option<String> {
    let mut m = Mods::default();
    for word in mods
        .split(|c: char| c.is_whitespace() || c == '_' || c == '+')
        .filter(|w| !w.is_empty())
    {
        let lower = word.to_ascii_lowercase();
        if m.set(&lower) {
            continue;
        }
        // Run-together names ("SUPERSHIFT", "CTRLALT").
        let mut rest = lower.as_str();
        'outer: while !rest.is_empty() {
            for name in [
                "control", "super", "shift", "ctrl", "mod4", "mod1", "logo", "win", "alt",
            ] {
                if let Some(tail) = rest.strip_prefix(name) {
                    m.set(name);
                    rest = tail;
                    continue 'outer;
                }
            }
            return None;
        }
    }
    finish(m, key.trim())
}

fn finish(mut m: Mods, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    // A modifier keysym as the key ("SUPER + SUPER_L") is a modifier-only chord.
    let lower = key.to_ascii_lowercase();
    let modifier_key = match lower.as_str() {
        "super_l" | "super_r" | "super" | "meta_l" | "meta_r" | "hyper_l" | "hyper_r" => {
            Some("super")
        }
        "control_l" | "control_r" | "ctrl" | "control" => Some("ctrl"),
        "alt_l" | "alt_r" | "alt" => Some("alt"),
        "shift_l" | "shift_r" | "shift" => Some("shift"),
        _ => None,
    };
    if let Some(name) = modifier_key {
        m.set(name);
        return Some(m.chord(None));
    }
    let shifted = match key {
        "!" => Some("1"),
        "@" => Some("2"),
        "#" => Some("3"),
        "$" => Some("4"),
        "%" => Some("5"),
        "^" => Some("6"),
        "&" => Some("7"),
        "*" => Some("8"),
        "(" => Some("9"),
        ")" => Some("0"),
        "_" => Some("Minus"),
        "+" => Some("Equal"),
        "{" => Some("BracketLeft"),
        "}" => Some("BracketRight"),
        "|" => Some("Backslash"),
        ":" => Some("Semicolon"),
        "\"" => Some("Quote"),
        "~" => Some("Backquote"),
        "<" => Some("Comma"),
        ">" => Some("Period"),
        "?" => Some("Slash"),
        _ if lower == "plus" => Some("Equal"),
        _ => None,
    };
    let name = if let Some(key) = shifted {
        m.shift = true;
        key.to_owned()
    } else {
        key_name(key)?
    };
    Some(m.chord(Some(&name)))
}

/// One key (an XKB keysym name, a Hyprland mouse name or `code:NN`) → its
/// canonical name.
pub fn key_name(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    // Already canonical names (including media/mouse names from app sheets).
    for name in [
        "Space",
        "Enter",
        "Tab",
        "Escape",
        "Backspace",
        "Delete",
        "Insert",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Up",
        "Down",
        "Left",
        "Right",
        "Minus",
        "Equal",
        "BracketLeft",
        "BracketRight",
        "Backslash",
        "Semicolon",
        "Quote",
        "Backquote",
        "Comma",
        "Period",
        "Slash",
        "Print",
        "Pause",
        "ScrollLock",
        "CapsLock",
        "NumLock",
        "Menu",
        "VolumeUp",
        "VolumeDown",
        "VolumeMute",
        "MicMute",
        "MediaPlayPause",
        "MediaNext",
        "MediaPrevious",
        "MediaStop",
        "BrightnessUp",
        "BrightnessDown",
        "MouseLeft",
        "MouseRight",
        "MouseMiddle",
        "MouseBack",
        "MouseForward",
        "WheelUp",
        "WheelDown",
        "WheelLeft",
        "WheelRight",
        "NumpadAdd",
        "NumpadSubtract",
        "NumpadMultiply",
        "NumpadDivide",
        "NumpadDecimal",
        "NumpadEnter",
    ] {
        if key.eq_ignore_ascii_case(name) {
            return Some(name.into());
        }
    }
    if let Some(code) = lower
        .strip_prefix("code")
        .and_then(|s| s.parse::<u32>().ok())
    {
        return Some(format!("Code{code}"));
    }
    if let Some(n) = lower
        .strip_prefix("numpad")
        .and_then(|n| n.parse::<u32>().ok())
    {
        return (n <= 9).then(|| format!("Numpad{n}"));
    }
    if let Some(code) = lower.strip_prefix("code:") {
        return code.parse::<u32>().ok().map(|n| format!("Code{n}"));
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        let named = match c {
            '/' => "Slash",
            '-' => "Minus",
            '=' => "Equal",
            ',' => "Comma",
            '.' => "Period",
            ';' => "Semicolon",
            '\'' => "Quote",
            '`' => "Backquote",
            '[' => "BracketLeft",
            ']' => "BracketRight",
            '\\' => "Backslash",
            ' ' => "Space",
            _ => return None,
        };
        return Some(named.into());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(format!("F{n}"));
        }
    }
    if let Some(rest) = lower.strip_prefix("kp_") {
        if let Ok(n) = rest.parse::<u32>() {
            if n <= 9 {
                return Some(format!("Numpad{n}"));
            }
        }
        let named = match rest {
            "add" => "NumpadAdd",
            "subtract" => "NumpadSubtract",
            "multiply" => "NumpadMultiply",
            "divide" => "NumpadDivide",
            "decimal" | "separator" => "NumpadDecimal",
            "enter" => "NumpadEnter",
            _ => return None,
        };
        return Some(named.into());
    }
    let named = match lower.as_str() {
        "space" => "Space",
        "return" | "enter" => "Enter",
        "tab" | "iso_left_tab" => "Tab",
        "escape" | "esc" => "Escape",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "page_up" | "prior" | "pageup" | "pgup" => "PageUp",
        "page_down" | "next" | "pagedown" | "pgdn" => "PageDown",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "minus" => "Minus",
        "equal" | "plus" => "Equal",
        "bracketleft" => "BracketLeft",
        "bracketright" => "BracketRight",
        "backslash" => "Backslash",
        "semicolon" => "Semicolon",
        "apostrophe" | "quote" => "Quote",
        "grave" | "backquote" => "Backquote",
        "comma" => "Comma",
        "period" => "Period",
        "slash" => "Slash",
        "print" | "sys_req" => "Print",
        "pause" | "break" => "Pause",
        "scroll_lock" => "ScrollLock",
        "caps_lock" => "CapsLock",
        "num_lock" => "NumLock",
        "menu" => "Menu",
        "xf86audioraisevolume" => "VolumeUp",
        "xf86audiolowervolume" => "VolumeDown",
        "xf86audiomute" => "VolumeMute",
        "xf86audiomicmute" => "MicMute",
        "xf86audioplay" | "xf86audiopause" => "MediaPlayPause",
        "xf86audionext" => "MediaNext",
        "xf86audioprev" => "MediaPrevious",
        "xf86audiostop" => "MediaStop",
        "xf86monbrightnessup" => "BrightnessUp",
        "xf86monbrightnessdown" => "BrightnessDown",
        "mouse:272" => "MouseLeft",
        "mouse:273" => "MouseRight",
        "mouse:274" => "MouseMiddle",
        "mouse:275" => "MouseBack",
        "mouse:276" => "MouseForward",
        "mouse_up" => "WheelUp",
        "mouse_down" => "WheelDown",
        "mouse_left" => "WheelLeft",
        "mouse_right" => "WheelRight",
        _ => return None,
    };
    Some(named.into())
}

/// Normalize an app accelerator or chord sequence, preserving alternatives at
/// the input layer rather than treating them as one sequence.
pub fn normalize(text: &str) -> Option<String> {
    let chords = text
        .split_whitespace()
        .map(chord_from_key_string)
        .collect::<Option<Vec<_>>>()?;
    (!chords.is_empty()).then(|| chords.join(" "))
}

pub fn display(text: &str, os: &str) -> String {
    text.split_whitespace()
        .map(|chord| {
            if os == "macos" {
                chord
                    .split('+')
                    .map(|part| match part {
                        "Ctrl" => "⌃",
                        "Alt" => "⌥",
                        "Shift" => "⇧",
                        "Super" => "⌘",
                        key => key,
                    })
                    .collect::<String>()
            } else {
                chord
                    .split('+')
                    .map(|part| {
                        if os == "windows" && part == "Super" {
                            "Win"
                        } else {
                            part
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("+")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn conflicts(a: &str, b: &str) -> bool {
    a == b
        || a.strip_prefix(b).is_some_and(|s| s.starts_with(' '))
        || b.strip_prefix(a).is_some_and(|s| s.starts_with(' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_key_strings() {
        assert_eq!(
            chord_from_key_string("SUPER + SHIFT + left").as_deref(),
            Some("Shift+Super+Left")
        );
        assert_eq!(
            chord_from_key_string("CTRL + SUPER + ALT + R").as_deref(),
            Some("Ctrl+Alt+Super+R")
        );
        assert_eq!(
            chord_from_key_string("SUPER + SUPER_L").as_deref(),
            Some("Super")
        );
        assert_eq!(
            chord_from_key_string("SUPER + mouse:272").as_deref(),
            Some("Super+MouseLeft")
        );
        assert_eq!(
            chord_from_key_string("SUPER + ALT + Page_Down").as_deref(),
            Some("Alt+Super+PageDown")
        );
        assert_eq!(
            chord_from_key_string("SUPER + mouse_down").as_deref(),
            Some("Super+WheelDown")
        );
        assert_eq!(
            chord_from_key_string("XF86AudioRaiseVolume").as_deref(),
            Some("VolumeUp")
        );
        assert_eq!(chord_from_key_string("Print").as_deref(), Some("Print"));
        assert_eq!(
            chord_from_key_string("SUPER + Comma").as_deref(),
            Some("Super+Comma")
        );
        assert_eq!(
            chord_from_key_string("SUPER + 0").as_deref(),
            Some("Super+0")
        );
        assert_eq!(
            chord_from_key_string("SUPER + code:10").as_deref(),
            Some("Super+Code10")
        );
        assert_eq!(chord_from_key_string("CAPS + A"), None);
        assert_eq!(chord_from_key_string("SUPER + "), None);
    }

    #[test]
    fn modmasks() {
        assert_eq!(chord_from_modmask(64, "SUPER_L").as_deref(), Some("Super"));
        assert_eq!(
            chord_from_modmask(12, "Delete").as_deref(),
            Some("Ctrl+Alt+Delete")
        );
        assert_eq!(
            chord_from_modmask(69, "R").as_deref(),
            Some("Ctrl+Shift+Super+R")
        );
        assert_eq!(
            chord_from_modmask(68, "mouse_up").as_deref(),
            Some("Ctrl+Super+WheelUp")
        );
        assert_eq!(chord_from_modmask(64 | 16, "A"), None);
    }

    #[test]
    fn conf_modifiers() {
        assert_eq!(
            chord_from_conf("SUPER SHIFT", "Q").as_deref(),
            Some("Shift+Super+Q")
        );
        assert_eq!(
            chord_from_conf("SUPER_SHIFT", "q").as_deref(),
            Some("Shift+Super+Q")
        );
        assert_eq!(
            chord_from_conf("SUPERSHIFT", "Return").as_deref(),
            Some("Shift+Super+Enter")
        );
        assert_eq!(chord_from_conf("", "Print").as_deref(), Some("Print"));
        assert_eq!(
            chord_from_conf("CTRL ALT", "Delete").as_deref(),
            Some("Ctrl+Alt+Delete")
        );
        assert_eq!(chord_from_conf("HYPERX", "Q"), None);
    }

    #[test]
    fn canonical_sequences_symbols_display_and_conflicts() {
        assert_eq!(
            normalize("Cmd+Ctrl+K Ctrl+S").as_deref(),
            Some("Ctrl+Super+K Ctrl+S")
        );
        assert_eq!(
            normalize("Ctrl+MediaPlayPause").as_deref(),
            Some("Ctrl+MediaPlayPause")
        );
        assert_eq!(normalize("Code42").as_deref(), Some("Code42"));
        assert_eq!(normalize("Numpad9").as_deref(), Some("Numpad9"));
        assert_eq!(
            chord_from_key_string("SUPER + +").as_deref(),
            Some("Shift+Super+Equal")
        );
        assert_eq!(
            chord_from_conf("CTRL", "?").as_deref(),
            Some("Ctrl+Shift+Slash")
        );
        assert_eq!(display("Ctrl+Alt+Shift+Super+X", "macos"), "⌃⌥⇧⌘X");
        assert_eq!(display("Super+X", "windows"), "Win+X");
        assert!(conflicts("Ctrl+K", "Ctrl+K Ctrl+S"));
        assert!(!conflicts("Ctrl+K", "Ctrl+KX"));
    }
}
