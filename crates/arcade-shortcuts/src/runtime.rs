//! Read-only Hyprland runtime binds. This synchronous API belongs on a worker.

use crate::{keys, read, Action};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBind {
    pub chord: Option<String>,
    pub key_text: String,
    pub submap: Option<String>,
    pub action: Action,
    pub flags: BTreeMap<String, bool>,
    pub description: Option<String>,
}

pub fn parse_json(text: &str) -> Result<Vec<RuntimeBind>, String> {
    let entries: Vec<Value> =
        serde_json::from_str(text).map_err(|e| format!("hyprctl binds JSON: {e}"))?;
    entries
        .into_iter()
        .map(|v| {
            let entry = v.as_object().ok_or("hyprctl bind is not an object")?;
            let key = entry.get("key").and_then(Value::as_str).unwrap_or_default();
            let keycode = entry
                .get("keycode")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let key = if key.is_empty() && keycode > 0 {
                format!("code:{keycode}")
            } else {
                key.to_owned()
            };
            let modmask = entry
                .get("modmask")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let chord = u32::try_from(modmask)
                .ok()
                .and_then(|m| keys::chord_from_modmask(m, &key));
            let dispatcher = entry
                .get("dispatcher")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let action = if dispatcher == "__lua" || dispatcher.is_empty() {
                Action::Unknown
            } else {
                Action::Dispatcher {
                    name: dispatcher.into(),
                    args: vec![entry.get("arg").cloned().unwrap_or(Value::Null)],
                }
            };
            let flags = entry
                .iter()
                .filter_map(|(k, v)| {
                    let name = match k.as_str() {
                        "repeat" => "repeating",
                        "longPress" => "long_press",
                        other => other,
                    };
                    if [
                        "locked",
                        "mouse",
                        "release",
                        "repeating",
                        "long_press",
                        "non_consuming",
                        "transparent",
                        "ignore_mods",
                        "dont_inhibit",
                        "click",
                        "drag",
                        "separate",
                        "bypass",
                        "auto_consuming",
                        "catch_all",
                        "allow_input_capture",
                    ]
                    .contains(&name)
                    {
                        v.as_bool().map(|b| (name.to_owned(), b))
                    } else {
                        None
                    }
                })
                .collect();
            let description = entry
                .get("description")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned);
            Ok(RuntimeBind {
                chord,
                key_text: key,
                submap: entry
                    .get("submap")
                    .and_then(Value::as_str)
                    .and_then(read::submap),
                action,
                flags,
                description,
            })
        })
        .collect()
}

/// `Ok(None)` means this process is outside a Hyprland session. No command runs.
pub fn query() -> Result<Option<Vec<RuntimeBind>>, String> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Ok(None);
    }
    let mut command = Command::new("hyprctl");
    command.args(["binds", "-j"]);
    let output = command_output(&mut command, Duration::from_secs(1))?;
    parse_json(&output).map(Some)
}

pub(crate) fn command_output(command: &mut Command, timeout: Duration) -> Result<String, String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let start = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{name} stdout unavailable"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(2)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => format!("{name}: {e}"),
                    _ => format!("{name} timed out after {} ms", timeout.as_millis()),
                });
            }
        }
    };
    if !status.success() {
        return Err(format!("{name} failed: {status}"));
    }
    let remaining = timeout.saturating_sub(start.elapsed());
    let bytes = rx
        .recv_timeout(remaining)
        .map_err(|_| format!("{name} output timed out"))?
        .map_err(|e| format!("{name} output: {e}"))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(format!("{name} output size limit exceeded"));
    }
    String::from_utf8(bytes).map_err(|e| format!("{name} output: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_descriptions_keycodes_and_unknown_lua() {
        let binds = parse_json(r#"[{"modmask":64,"key":"","keycode":10,"dispatcher":"exec","arg":"arcade-find --toggle","description":"Find","has_description":true,"repeat":true,"longPress":true},{"modmask":64,"key":"SUPER_L","dispatcher":"__lua","arg":"12","submap":"resize"}]"#).unwrap();
        assert_eq!(binds[0].chord.as_deref(), Some("Super+Code10"));
        assert_eq!(binds[0].flags.get("repeating"), Some(&true));
        assert_eq!(binds[0].flags.get("long_press"), Some(&true));
        assert_eq!(binds[0].description.as_deref(), Some("Find"));
        assert_eq!(binds[1].action, Action::Unknown);
        assert_eq!(binds[1].submap.as_deref(), Some("resize"));
        assert!(parse_json("{}").is_err());
        assert!(parse_json("[4]").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn command_timeout_kills_and_reaps_child() {
        let mut command = Command::new("sleep");
        command.arg("2");
        let start = Instant::now();
        assert!(command_output(&mut command, Duration::from_millis(20))
            .unwrap_err()
            .contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(1));
        let mut command = Command::new("printf");
        command.arg("[]");
        assert_eq!(
            command_output(&mut command, Duration::from_secs(1)).unwrap(),
            "[]"
        );
    }
}
