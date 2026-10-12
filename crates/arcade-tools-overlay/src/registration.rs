//! Global shortcut helpers adapted from Arcade Find's `hotkey.rs`
//! (MIT OR Apache-2.0), whose Hyprland implementation credits Arcade Lens.
//! Native registration lives in Tools. Wayland returns a user-owned bind hint.

use std::cell::RefCell;
use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
#[cfg(not(target_os = "linux"))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::Serialize;

pub const MANUAL_HINT: &str =
    "Bind `arcade-tools --show-shortcuts` in Settings → Keyboard → Custom Shortcuts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    Native,
    Manual,
}

#[derive(Debug, Serialize)]
pub struct Registration {
    pub via: Via,
    pub hint: Option<String>,
}

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11::Active;
#[cfg(not(target_os = "linux"))]
struct Active(GlobalHotKeyManager, HotKey);
thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }

pub fn default_accelerator() -> &'static str {
    if cfg!(target_os = "macos") {
        "Ctrl+Option+/"
    } else if cfg!(windows) {
        "Ctrl+Alt+/"
    } else {
        "Super+/"
    }
}

pub fn parse(accelerator: &str) -> Result<HotKey, String> {
    let parts: Vec<String> = accelerator
        .split('+')
        .map(str::trim)
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "win" | "meta" | "super" | "cmd" | "command" | "logo" => "Super".into(),
            "control" | "ctrl" => "Ctrl".into(),
            "option" | "alt" => "Alt".into(),
            "/" | "slash" => "Slash".into(),
            _ => part.to_string(),
        })
        .collect();
    if parts.len() < 2 {
        return Err("the overlay shortcut needs a modifier".into());
    }
    HotKey::from_str(&parts.join("+")).map_err(|error| error.to_string())
}

pub fn backend_for(hyprland: bool, wayland: bool) -> Via {
    if hyprland || wayland {
        Via::Manual
    } else {
        Via::Native
    }
}

fn overlay_executable() -> std::path::PathBuf {
    let name = if cfg!(windows) {
        "arcade-tools-overlay.exe"
    } else {
        "arcade-tools-overlay"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join(name)))
        .filter(|exe| exe.is_file())
        .unwrap_or_else(|| name.into())
}

/// Call on the Tools main thread (required by global-hotkey on macOS/Windows).
/// Tools must keep its native event loop running and call unregister on quit.
pub fn register(accelerator: &str) -> Result<Registration, String> {
    let executable = overlay_executable();
    register_with_handler(accelerator, move || {
        if let Ok(mut child) = std::process::Command::new(&executable)
            .arg("--toggle")
            .spawn()
        {
            let _ = std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    })
}

/// Native registration must run on the Tools main thread. The event callback
/// queues work; it must not block the platform's event handler.
pub fn register_with_handler(
    accelerator: &str,
    pressed: impl Fn() + Send + Sync + 'static,
) -> Result<Registration, String> {
    let hotkey = parse(accelerator)?;
    unregister()?;
    let hyprland =
        cfg!(target_os = "linux") && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();
    let wayland = cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland"));
    let via = backend_for(hyprland, wayland);
    if via == Via::Manual {
        return Ok(Registration {
            via,
            hint: Some(manual_hint(hotkey, hyprland)?),
        });
    }
    #[cfg(target_os = "linux")]
    let registration = x11::register(hotkey, pressed)?;
    #[cfg(not(target_os = "linux"))]
    let registration = {
        let manager = GlobalHotKeyManager::new().map_err(|error| error.to_string())?;
        manager
            .register(hotkey)
            .map_err(|error| error.to_string())?;
        let id = hotkey.id();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id == id && event.state == HotKeyState::Pressed {
                pressed();
            }
        }));
        Active(manager, hotkey)
    };
    ACTIVE.with(|active| *active.borrow_mut() = Some(registration));
    Ok(Registration { via, hint: None })
}

fn manual_hint(key: HotKey, hyprland: bool) -> Result<String, String> {
    if !hyprland {
        return Ok(MANUAL_HINT.into());
    }
    let command = "arcade-tools --show-shortcuts";
    let lua = hyprland::bind_arguments(key, command, true)?;
    let conf = hyprland::bind_arguments(key, command, false)?;
    Ok(format!(
        "Add a bind to your Hyprland configuration.\nLua: {}\nhyprlang: bind = {}",
        lua[1], conf[2]
    ))
}

pub fn unregister() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        ACTIVE.with(|active| active.borrow_mut().take());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    ACTIVE.with(|active| match active.borrow_mut().take() {
        Some(Active(manager, key)) => {
            GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
            manager.unregister(key).map_err(|error| error.to_string())
        }
        None => Ok(()),
    })
}

pub mod hyprland {
    use super::*;

    pub fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
    fn lua_string(value: &str) -> String {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
        )
    }

    fn keys(key: HotKey) -> Result<(String, String), String> {
        let modifiers: Vec<_> = [
            (Modifiers::SUPER, "SUPER"),
            (Modifiers::CONTROL, "CTRL"),
            (Modifiers::ALT, "ALT"),
            (Modifiers::SHIFT, "SHIFT"),
        ]
        .iter()
        .filter(|(modifier, _)| key.mods.contains(*modifier))
        .map(|(_, name)| *name)
        .collect();
        let name = format!("{:?}", key.key);
        let symbol = if let Some(letter) = name
            .strip_prefix("Key")
            .or_else(|| name.strip_prefix("Digit"))
        {
            letter.to_owned()
        } else if name.starts_with('F') && name[1..].parse::<u8>().is_ok() {
            name
        } else {
            match key.key {
                Code::Slash => "slash",
                Code::Space => "space",
                Code::Tab => "Tab",
                Code::Enter => "Return",
                Code::Escape => "Escape",
                Code::Minus => "minus",
                Code::Equal => "equal",
                Code::Comma => "comma",
                Code::Period => "period",
                Code::ArrowUp => "Up",
                Code::ArrowDown => "Down",
                Code::ArrowLeft => "Left",
                Code::ArrowRight => "Right",
                _ => return Err("unsupported Hyprland key".into()),
            }
            .into()
        };
        Ok((modifiers.join(" "), symbol))
    }

    pub fn bind_arguments(key: HotKey, command: &str, lua: bool) -> Result<Vec<String>, String> {
        let (modifiers, symbol) = keys(key)?;
        if lua {
            let chord = format!("{} + {symbol}", modifiers.replace(' ', " + "));
            Ok(vec![
                "eval".into(),
                format!(
                    "hl.bind({}, hl.dsp.exec_cmd({}))",
                    lua_string(&chord),
                    lua_string(command)
                ),
            ])
        } else {
            Ok(vec![
                "keyword".into(),
                "bind".into(),
                format!("{modifiers}, {symbol}, exec, {command}"),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_selection_and_manual_hint_without_registering() {
        assert_eq!(backend_for(true, true), Via::Manual);
        assert_eq!(backend_for(false, true), Via::Manual);
        assert_eq!(backend_for(false, false), Via::Native);
        assert_eq!(
            MANUAL_HINT,
            "Bind `arcade-tools --show-shortcuts` in Settings → Keyboard → Custom Shortcuts"
        );
    }

    #[test]
    fn defaults_and_lua_056_arguments() {
        for shortcut in ["Super+/", "Ctrl+Alt+/", "Ctrl+Option+/"] {
            assert!(parse(shortcut).is_ok());
        }
        let key = parse("Super+/").unwrap();
        let hint = manual_hint(key, true).unwrap();
        assert!(hint.contains("hl.bind"));
        assert!(hint.contains("bind = SUPER, slash, exec, arcade-tools --show-shortcuts"));
        assert!(!hint.contains("/tmp/"));
        assert_eq!(
            hyprland::bind_arguments(key, "'arcade-tools-overlay' --toggle", true).unwrap(),
            [
                "eval",
                "hl.bind(\"SUPER + slash\", hl.dsp.exec_cmd(\"'arcade-tools-overlay' --toggle\"))"
            ]
        );
        assert_eq!(
            hyprland::bind_arguments(key, "overlay --toggle", false).unwrap(),
            ["keyword", "bind", "SUPER, slash, exec, overlay --toggle"]
        );
        assert_eq!(hyprland::shell_quote("/a b/it's"), "'/a b/it'\\''s'");
        assert!(parse("/").is_err());
    }
}
