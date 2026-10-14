//! Focus is captured before mapping. Detection is bounded even when a desktop
//! socket or X server stalls. No hyprctl/swaymsg subprocesses are used.

use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use crate::index::MatchKind;

pub const DETECTION_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub struct Focus {
    pub identity: Option<String>,
    pub kind: MatchKind,
    pub previous_window: Option<u64>,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            identity: None,
            kind: MatchKind::Class,
            previous_window: None,
        }
    }
}

pub fn detect() -> Focus {
    bounded(detect_platform, DETECTION_TIMEOUT)
}

fn bounded<T: Default + Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    timeout: Duration,
) -> T {
    let (tx, rx) = mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("overlay-focus".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .is_err()
    {
        return T::default();
    }
    rx.recv_timeout(timeout).unwrap_or_default()
}

pub fn parse_hyprland(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("class")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn parse_sway(bytes: &[u8]) -> Option<String> {
    fn visit(node: &Value) -> Option<String> {
        if node.get("focused").and_then(Value::as_bool) == Some(true) {
            let identity = node
                .get("app_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| node.get("window_properties")?.get("class")?.as_str());
            if let Some(identity) = identity.filter(|s| !s.is_empty()) {
                return Some(identity.to_owned());
            }
        }
        for branch in ["nodes", "floating_nodes"] {
            if let Some(children) = node.get(branch).and_then(Value::as_array) {
                for child in children {
                    if let Some(identity) = visit(child) {
                        return Some(identity);
                    }
                }
            }
        }
        None
    }
    visit(&serde_json::from_slice::<Value>(bytes).ok()?)
}

#[cfg(target_os = "linux")]
fn detect_platform() -> Focus {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    let mut focus = Focus::default();
    if let (Some(runtime), Some(signature)) = (
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE"),
    ) {
        let socket = PathBuf::from(runtime)
            .join("hypr")
            .join(signature)
            .join(".socket.sock");
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(40)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(5)));
            if stream.write_all(b"j/activewindow").is_ok() {
                let mut bytes = Vec::new();
                if stream.take(64 * 1024).read_to_end(&mut bytes).is_ok() {
                    focus.identity = parse_hyprland(&bytes);
                }
            }
        }
        return focus;
    }
    if let Some(socket) = std::env::var_os("SWAYSOCK") {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(40)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(5)));
            // i3 IPC: magic, zero-length payload, GET_TREE (4).
            if stream.write_all(b"i3-ipc\0\0\0\0\x04\0\0\0").is_ok() {
                let mut header = [0; 14];
                if stream.read_exact(&mut header).is_ok()
                    && &header[..6] == b"i3-ipc"
                    && header[10..14] == 4u32.to_le_bytes()
                {
                    let len = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
                    if len <= 4 * 1024 * 1024 {
                        let mut bytes = vec![0; len];
                        if stream.read_exact(&mut bytes).is_ok() {
                            focus.identity = parse_sway(&bytes);
                        }
                    }
                }
            }
        }
        return focus;
    }
    if is_wayland() {
        return focus;
    }
    x11_focus().unwrap_or_default()
}

pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland")))
}

#[cfg(target_os = "linux")]
fn x11_focus() -> Option<Focus> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let active = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let reply = conn
        .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let window = reply.value32()?.next()?;
    if window == 0 {
        return None;
    }
    let mut focus = Focus {
        identity: None,
        kind: MatchKind::Class,
        previous_window: Some(u64::from(window)),
    };
    let class = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()
        .and_then(|cookie| cookie.reply().ok());
    if let Some(class) = class {
        let mut names = class
            .value
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty());
        if let Some(instance) = names.next() {
            let class = names.next().unwrap_or(instance);
            focus.identity = Some(String::from_utf8_lossy(class).into_owned());
        }
    }
    Some(focus)
}

#[cfg(windows)]
fn detect_platform() -> Focus {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: handles and output buffers are checked before use and closed.
    unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        let mut focus = Focus {
            identity: None,
            kind: MatchKind::Exe,
            previous_window: (!window.is_null()).then_some(window as u64),
        };
        if !process.is_null() {
            let mut path = vec![0u16; 32768];
            let mut len = path.len() as u32;
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) != 0 {
                focus.identity = Some(String::from_utf16_lossy(&path[..len as usize]));
            }
            CloseHandle(process);
        }
        focus
    }
}

#[cfg(target_os = "macos")]
fn detect_platform() -> Focus {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication();
    Focus {
        identity: app
            .and_then(|app| app.bundleIdentifier())
            .map(|id| id.to_string()),
        kind: MatchKind::Bundle,
        previous_window: None,
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn detect_platform() -> Focus {
    Focus::default()
}

/// Only called when explicitly closing, never when a click focuses another app.
pub fn restore(focus: &Focus) {
    #[cfg(target_os = "linux")]
    if let Some(window) = focus.previous_window {
        // Complete the X round trip when possible, without keeping an exiting
        // overlay alive indefinitely if the server has stalled.
        bounded(
            move || {
                use x11rb::connection::Connection;
                use x11rb::protocol::xproto::{
                    ClientMessageEvent, ConnectionExt, EventMask, InputFocus,
                };
                if let Ok((conn, screen)) = x11rb::connect(None) {
                    if let Ok(cookie) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") {
                        if let Ok(atom) = cookie.reply() {
                            let event = ClientMessageEvent::new(
                                32,
                                window as u32,
                                atom.atom,
                                [2, 0, 0, 0, 0],
                            );
                            let _ = conn.send_event(
                                false,
                                conn.setup().roots[screen].root,
                                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                                event,
                            );
                            if let Ok(cookie) = conn.set_input_focus(
                                InputFocus::PARENT,
                                window as u32,
                                x11rb::CURRENT_TIME,
                            ) {
                                let _ = cookie.check();
                            }
                            let _ = conn.flush();
                        }
                    }
                }
            },
            DETECTION_TIMEOUT,
        );
    }
    #[cfg(windows)]
    if let Some(window) = focus.previous_window {
        // SAFETY: SetForegroundWindow validates the saved HWND, including a closed window.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(window as _);
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(marker) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(marker).hide(None);
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    let _ = focus;
    #[cfg(target_os = "macos")]
    let _ = focus;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_replies() {
        assert_eq!(
            parse_hyprland(include_bytes!(
                "../tests/fixtures/hyprland-activewindow.json"
            ))
            .as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(parse_hyprland(b"{}"), None);
        assert_eq!(parse_hyprland(b"{\"class\":\"\"}"), None);
        assert_eq!(parse_hyprland(b"corrupt"), None);
    }

    #[test]
    fn sway_native_and_floating_xwayland() {
        let bytes = include_bytes!("../tests/fixtures/sway-tree.json");
        assert_eq!(parse_sway(bytes).as_deref(), Some("org.mozilla.firefox"));
        let mut tree: Value = serde_json::from_slice(bytes).unwrap();
        tree["nodes"][0]["nodes"][0]["focused"] = Value::Bool(false);
        tree["floating_nodes"][0]["focused"] = Value::Bool(true);
        assert_eq!(
            parse_sway(&serde_json::to_vec(&tree).unwrap()).as_deref(),
            Some("Arcade.Find")
        );
        assert_eq!(parse_sway(b"{\"nodes\":[]}"), None);
        assert_eq!(parse_sway(b"invalid"), None);
    }

    #[test]
    fn stalled_detection_falls_back_within_budget() {
        let began = std::time::Instant::now();
        let focus = bounded(
            || {
                std::thread::sleep(Duration::from_secs(2));
                Focus::default()
            },
            DETECTION_TIMEOUT,
        );
        assert!(focus.identity.is_none());
        // Far below the stalled detector; loaded CI runners overshoot 50 ms.
        assert!(began.elapsed() < Duration::from_secs(1));
    }
}
