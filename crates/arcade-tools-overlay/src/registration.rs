//! Global shortcut helpers adapted from Arcade Find's `hotkey.rs`
//! (MIT OR Apache-2.0), whose Hyprland implementation credits Arcade Lens.
//! Only explicit register/unregister calls change compositor runtime state.

use std::cell::RefCell;
use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::Serialize;

pub const MANUAL_HINT: &str =
    "Bind `arcade-tools-overlay --toggle` in Settings → Keyboard → Custom Shortcuts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    Native,
    HyprlandRuntime,
    Manual,
}

#[derive(Debug, Serialize)]
pub struct Registration {
    pub via: Via,
    pub hint: Option<String>,
}

enum Active {
    Native(GlobalHotKeyManager, HotKey),
    Hyprland(hyprland::Binding),
}
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
    if hyprland {
        Via::HyprlandRuntime
    } else if wayland {
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
    let hotkey = parse(accelerator)?;
    unregister()?;
    let via = backend_for(
        cfg!(target_os = "linux") && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        crate::focus::is_wayland(),
    );
    match via {
        Via::Manual => Ok(Registration {
            via,
            hint: Some(MANUAL_HINT.into()),
        }),
        Via::HyprlandRuntime => {
            let command = format!(
                "{} --toggle",
                hyprland::shell_quote(&overlay_executable().to_string_lossy())
            );
            let binding = hyprland::Binding::register(hotkey, &command)?;
            ACTIVE.with(|active| *active.borrow_mut() = Some(Active::Hyprland(binding)));
            Ok(Registration { via, hint: None })
        }
        Via::Native => {
            let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
            manager.register(hotkey).map_err(|e| e.to_string())?;
            let id = hotkey.id();
            let executable = overlay_executable();
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.id == id && event.state == HotKeyState::Pressed {
                    // Reap the short-lived launcher without blocking the host UI.
                    if let Ok(mut child) = std::process::Command::new(&executable)
                        .arg("--toggle")
                        .spawn()
                    {
                        let _ = std::thread::spawn(move || {
                            let _ = child.wait();
                        });
                    }
                }
            }));
            ACTIVE.with(|active| *active.borrow_mut() = Some(Active::Native(manager, hotkey)));
            Ok(Registration { via, hint: None })
        }
    }
}

pub fn unregister() -> Result<(), String> {
    ACTIVE.with(|active| match active.borrow_mut().take() {
        Some(Active::Native(manager, key)) => {
            GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
            manager.unregister(key).map_err(|e| e.to_string())
        }
        Some(Active::Hyprland(binding)) => binding.unregister(),
        None => Ok(()),
    })
}

pub mod hyprland {
    use super::*;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    fn run(args: &[&str]) -> Result<(), String> {
        let output = Command::new("hyprctl")
            .args(args)
            .output()
            .map_err(|e| format!("hyprctl: {e}"))?;
        let reply = String::from_utf8_lossy(&output.stdout);
        if output.status.success() && reply.trim() == "ok" {
            Ok(())
        } else {
            Err(format!(
                "hyprctl: {} {}",
                reply.trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

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

    fn unbind_arguments(key: HotKey, lua: bool) -> Result<Vec<String>, String> {
        let (modifiers, symbol) = keys(key)?;
        if lua {
            Ok(vec![
                "eval".into(),
                format!(
                    "hl.unbind({})",
                    lua_string(&format!("{} + {symbol}", modifiers.replace(' ', " + ")))
                ),
            ])
        } else {
            Ok(vec![
                "keyword".into(),
                "unbind".into(),
                format!("{modifiers}, {symbol}"),
            ])
        }
    }

    fn execute(args: &[String]) -> Result<(), String> {
        run(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    pub struct Binding {
        key: HotKey,
        lua: bool,
        alive: Arc<Mutex<bool>>,
        #[cfg(unix)]
        events: Option<std::os::unix::net::UnixStream>,
    }

    impl Binding {
        pub(super) fn register(key: HotKey, command: &str) -> Result<Self, String> {
            // The exact Lua probe used by Find; never falls back to editing files.
            let lua = run(&["eval", "return hl ~= nil"]).is_ok();
            let args = bind_arguments(key, command, lua)?;
            execute(&args)?;
            #[allow(unused_mut)]
            let mut binding = Self {
                key,
                lua,
                alive: Arc::new(Mutex::new(true)),
                #[cfg(unix)]
                events: None,
            };
            #[cfg(unix)]
            {
                use std::io::{BufRead, BufReader};
                use std::os::unix::net::UnixStream;
                if let (Some(runtime), Some(signature)) = (
                    std::env::var_os("XDG_RUNTIME_DIR"),
                    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE"),
                ) {
                    let socket = std::path::PathBuf::from(runtime)
                        .join("hypr")
                        .join(signature)
                        .join(".socket2.sock");
                    if let Ok(stream) = UnixStream::connect(socket) {
                        binding.events = stream.try_clone().ok();
                        let alive = binding.alive.clone();
                        let _ = std::thread::Builder::new()
                            .name("overlay-hypr-reload".into())
                            .spawn(move || {
                                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                                    if line.starts_with("configreloaded>>") {
                                        let Ok(guard) = alive.lock() else { break };
                                        if !*guard {
                                            break;
                                        }
                                        let _ = execute(&args);
                                    }
                                }
                            });
                    }
                }
            }
            Ok(binding)
        }

        pub(super) fn unregister(self) -> Result<(), String> {
            let mut guard = self
                .alive
                .lock()
                .map_err(|_| "Hyprland registration lock poisoned")?;
            *guard = false;
            #[cfg(unix)]
            if let Some(stream) = self.events {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            execute(&unbind_arguments(self.key, self.lua)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_selection_and_manual_hint_without_registering() {
        assert_eq!(backend_for(true, true), Via::HyprlandRuntime);
        assert_eq!(backend_for(false, true), Via::Manual);
        assert_eq!(backend_for(false, false), Via::Native);
        assert_eq!(
            MANUAL_HINT,
            "Bind `arcade-tools-overlay --toggle` in Settings → Keyboard → Custom Shortcuts"
        );
    }

    #[test]
    fn defaults_and_lua_056_arguments() {
        for shortcut in ["Super+/", "Ctrl+Alt+/", "Ctrl+Option+/"] {
            assert!(parse(shortcut).is_ok());
        }
        let key = parse("Super+/").unwrap();
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
