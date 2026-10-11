//! Human labels from descriptions, source comments, key variables and dispatchers.

use crate::{Action, ConfigBind, Origin};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub type UserLabels = BTreeMap<String, String>;

pub fn load_user_labels(path: &Path) -> Result<UserLabels, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Empty submap is keyed as `|Super+T`.
pub fn user_label<'a>(
    labels: &'a UserLabels,
    submap: Option<&str>,
    chord: Option<&str>,
) -> Option<&'a str> {
    labels
        .get(&format!("{}|{}", submap.unwrap_or_default(), chord?))
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
}

#[derive(Debug, Default)]
pub struct Label {
    pub label: String,
    pub detail: Option<String>,
    pub category: Option<String>,
}

pub fn for_config(
    bind: &ConfigBind,
    sources: &BTreeMap<PathBuf, String>,
    labels: &UserLabels,
) -> Label {
    let source = bind
        .origin
        .as_ref()
        .and_then(|o| sources.get(&o.file).map(|s| (o, s)));
    let marker = if source.is_some_and(|(o, _)| o.file.extension().is_some_and(|e| e == "lua")) {
        "--"
    } else {
        "#"
    };
    let comment = source.and_then(|(o, s)| direct_comment(s, o.line, marker));
    let identifier = source.and_then(|(o, s)| key_identifier(&statement(s, o.line)));
    let dispatcher = dispatcher_label(&bind.action);
    let label = user_label(labels, bind.submap.as_deref(), bind.chord.as_deref())
        .map(str::to_owned)
        .or_else(|| bind.description.clone().filter(|s| !s.trim().is_empty()))
        .or_else(|| {
            matches!(bind.action, Action::LuaFunction { .. })
                .then_some(comment)
                .flatten()
        })
        .or(identifier)
        .or_else(|| dispatcher.clone())
        .unwrap_or_else(|| function_label(&bind.action, bind.origin.as_ref()));
    let detail = dispatcher.filter(|s| s != &label);
    let category = source.and_then(|(o, s)| category(s, o.line, marker));
    Label {
        label,
        detail,
        category,
    }
}

fn function_label(action: &Action, origin: Option<&Origin>) -> String {
    let definition = match action {
        Action::LuaFunction { file, line } => file.as_deref().zip(*line),
        _ => None,
    };
    let place = origin.map(|o| (o.file.as_path(), o.line)).or(definition);
    match place {
        Some((file, line)) => format!(
            "Lua function ({}:{line})",
            file.file_name().unwrap_or_default().to_string_lossy()
        ),
        None => "Lua function (added at runtime)".into(),
    }
}

fn comment_text(line: &str, marker: &str) -> Option<String> {
    let text = line
        .trim_start()
        .strip_prefix(marker)?
        .trim()
        .trim_matches('-')
        .trim();
    if text.is_empty()
        || text.contains("http")
        || text.contains("hl.")
        || text.ends_with(['(', '{', ')'])
    {
        return None;
    }
    Some(text.to_owned())
}

pub fn direct_comment(source: &str, line: u32, marker: &str) -> Option<String> {
    let lines: Vec<_> = source.lines().collect();
    let mut comments = Vec::new();
    for text in lines
        .get(..(line as usize).saturating_sub(1).min(lines.len()))?
        .iter()
        .rev()
    {
        if !text.trim_start().starts_with(marker) {
            break;
        }
        if let Some(text) = comment_text(text, marker) {
            comments.push(text);
        }
    }
    comments.reverse();
    (!comments.is_empty()).then(|| comments.join(" "))
}

pub fn category(source: &str, line: u32, marker: &str) -> Option<String> {
    let lines: Vec<_> = source.lines().collect();
    for i in (0..(line as usize).saturating_sub(1).min(lines.len())).rev() {
        if !lines[i].starts_with(marker) {
            continue;
        }
        let mut start = i;
        while start > 0 && lines[start - 1].starts_with(marker) {
            start -= 1;
        }
        if start > 0 && !lines[start - 1].trim().is_empty() {
            continue;
        }
        if let Some(text) = comment_text(lines[i], marker) {
            return Some(text);
        }
    }
    None
}

/// Strip comments and quoted strings while retaining punctuation positions.
/// Long bracket literals and block comments are handled across source lines.
fn code_mask(source: &str) -> Vec<String> {
    let mut quote = None;
    let mut long: Option<usize> = None;
    let mut escaped = false;
    source
        .lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let mut code = vec![b' '; bytes.len()];
            let mut i = 0;
            while i < bytes.len() {
                if let Some(eq) = long {
                    if bytes[i] == b']'
                        && bytes
                            .get(i + 1..i + 1 + eq)
                            .is_some_and(|s| s.iter().all(|b| *b == b'='))
                        && bytes.get(i + 1 + eq) == Some(&b']')
                    {
                        i += eq + 2;
                        long = None;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                if let Some(q) = quote {
                    if escaped {
                        escaped = false;
                    } else if bytes[i] == b'\\' {
                        escaped = true;
                    } else if bytes[i] == q {
                        quote = None;
                    }
                    i += 1;
                    continue;
                }
                if bytes[i..].starts_with(b"--") {
                    let rest = &bytes[i + 2..];
                    if rest.first() == Some(&b'[') {
                        let eq = rest[1..].iter().take_while(|b| **b == b'=').count();
                        if rest.get(eq + 1) == Some(&b'[') {
                            long = Some(eq);
                            i += eq + 4;
                            continue;
                        }
                    }
                    break;
                }
                if bytes[i] == b'\'' || bytes[i] == b'"' {
                    quote = Some(bytes[i]);
                    i += 1;
                    continue;
                }
                if bytes[i] == b'[' {
                    let eq = bytes[i + 1..].iter().take_while(|b| **b == b'=').count();
                    if bytes.get(i + eq + 1) == Some(&b'[') {
                        long = Some(eq);
                        i += eq + 2;
                        continue;
                    }
                }
                code[i] = bytes[i];
                i += 1;
            }
            String::from_utf8_lossy(&code).into_owned()
        })
        .collect()
}

/// Lua reports the closing line of a multiline call. Recover its opening line.
pub fn statement_start(source: &str, line: u32) -> u32 {
    let code = code_mask(source);
    let mut stack = Vec::new();
    let mut start = line;
    for (i, text) in code.iter().enumerate().take(line as usize) {
        if stack.is_empty() {
            start = i as u32 + 1;
        }
        for c in text.chars() {
            match c {
                '(' => stack.push(i as u32 + 1),
                ')' => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    stack.first().copied().unwrap_or(start)
}

pub fn statement(source: &str, line: u32) -> String {
    let code = code_mask(source);
    let mut text = String::new();
    let mut depth = 0i32;
    for piece in code.iter().skip((line as usize).saturating_sub(1)).take(10) {
        text.push_str(piece);
        text.push('\n');
        for c in piece.chars() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            }
        }
        if depth <= 0 {
            break;
        }
    }
    text
}

pub fn key_identifier(statement: &str) -> Option<String> {
    for token in statement.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.') {
        if !token.contains('.') {
            continue;
        }
        let last = token.rsplit('.').next()?;
        for prefix in ["kb", "key", "bind"] {
            if let Some(name) = last.strip_prefix(prefix) {
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return Some(humanize(name));
                }
            }
        }
    }
    None
}

pub fn humanize(text: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut words = String::new();
    for (i, c) in chars.iter().enumerate() {
        if *c == '_' || *c == '-' || *c == '.' {
            words.push(' ');
            continue;
        }
        if i > 0
            && c.is_ascii_uppercase()
            && (chars[i - 1].is_ascii_lowercase()
                || chars.get(i + 1).is_some_and(|c| c.is_ascii_lowercase())
                    && chars[i - 1].is_ascii_uppercase())
        {
            words.push(' ');
        }
        words.push(c.to_ascii_lowercase());
    }
    let expanded = words
        .split_whitespace()
        .map(|w| match w {
            "ws" => "workspace",
            "win" => "window",
            "notifs" => "notifications",
            "pip" => "picture-in-picture",
            "prev" => "previous",
            "del" => "delete",
            "cmd" => "command",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = expanded.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn short(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        text.to_owned()
    } else {
        text.chars()
            .take(limit.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

fn direction(value: &str) -> &str {
    match value {
        "l" => "left",
        "r" => "right",
        "u" => "up",
        "d" => "down",
        _ => value,
    }
}

fn workspace(value: &str, moving: bool) -> String {
    if moving {
        return format!("Move window to workspace {value}");
    }
    match value {
        "+1" | "e+1" => "Next workspace".into(),
        "-1" | "e-1" => "Previous workspace".into(),
        _ => match value.strip_prefix("special:") {
            Some(name) => format!("Show special workspace {name}"),
            None => format!("Go to workspace {value}"),
        },
    }
}

pub fn dispatcher_label(action: &Action) -> Option<String> {
    let Action::Dispatcher { name, args } = action else {
        return None;
    };
    let first = args.first();
    let text = first.map(scalar).unwrap_or_default();
    let field = |key: &str| first.and_then(|v| v.get(key)).map(scalar);
    let label = match name.as_str() {
        "exec_cmd" | "exec" => format!("Run: {}", short(&text, 60)),
        "global" => {
            if let Some((owner, action)) = text.split_once(':') {
                format!("{}: {action}", humanize(owner))
            } else {
                format!("Global: {text}")
            }
        }
        "focus" => {
            if let Some(ws) = field("workspace") {
                workspace(&ws, false)
            } else if let Some(dir) = field("direction") {
                format!("Focus window {}", direction(&dir))
            } else {
                "Focus window".into()
            }
        }
        "movefocus" => format!("Focus window {}", direction(&text)),
        "workspace" => workspace(&text, false),
        "window.move" => {
            if let Some(ws) = field("workspace") {
                workspace(&ws, true)
            } else if let Some(dir) = field("direction") {
                format!("Move window {}", direction(&dir))
            } else if field("out_of_group").as_deref() == Some("true") {
                "Move window out of group".into()
            } else {
                "Move window".into()
            }
        }
        "movetoworkspace" | "movetoworkspacesilent" => {
            workspace(text.split(',').next().unwrap_or(&text).trim(), true)
        }
        "movewindow" => format!("Move window {}", direction(&text)),
        "window.close" | "killactive" => "Close window".into(),
        "window.float" | "togglefloating" => "Toggle floating".into(),
        "window.fullscreen" => match field("mode").as_deref() {
            Some("maximized" | "maximize" | "1") => "Maximize",
            _ => "Fullscreen",
        }
        .into(),
        "fullscreen" => if text == "1" {
            "Maximize"
        } else {
            "Fullscreen"
        }
        .into(),
        "window.pin" | "pin" => "Pin window".into(),
        "window.center" | "centerwindow" => "Center window".into(),
        "window.drag" => "Move window with the mouse".into(),
        "window.resize" if args.is_empty() => "Resize window with the mouse".into(),
        "window.resize" | "resizewindow" => format!(
            "Resize window{}",
            if text.is_empty() {
                String::new()
            } else {
                format!(": {}", short(&text, 60))
            }
        ),
        "window.pseudo" | "pseudo" => "Toggle pseudo-tiling".into(),
        "window.cycle_next" => if field("next").as_deref() == Some("false") {
            "Cycle windows backwards"
        } else {
            "Cycle windows"
        }
        .into(),
        "group.toggle" | "togglegroup" => "Toggle group".into(),
        "group.next" => "Next window in group".into(),
        "group.prev" => "Previous window in group".into(),
        "group.lock_active" => "Lock group".into(),
        "changegroupactive" => if text == "b" {
            "Previous window in group"
        } else {
            "Next window in group"
        }
        .into(),
        "layout" => format!("Layout: {text}"),
        "togglesplit" => "Layout: togglesplit".into(),
        "workspace.toggle_special" | "togglespecialworkspace" => {
            format!("Show special workspace {text}")
        }
        "exit" => "Exit Hyprland".into(),
        "no_op" => "Nothing".into(),
        "submap" => format!("Switch submap: {text}"),
        _ => {
            let args = args.iter().map(scalar).collect::<Vec<_>>().join(", ");
            format!(
                "{}{}",
                humanize(name),
                if args.is_empty() {
                    String::new()
                } else {
                    format!(": {}", short(&args, 60))
                }
            )
        }
    };
    Some(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifiers_comments_and_multiline_calls() {
        let src = "-- Media\ncreate_bind(\n    vars.kbGoToWsGroup,\n    hl.dsp.global(\"ignore ( )\")\n)\n\n-- -----\n-- Restore lock\ncreate_bind(vars.kbLock, function() end)\n";
        assert_eq!(statement_start(src, 5), 2);
        assert_eq!(
            key_identifier(&statement(src, 2)).as_deref(),
            Some("Go to workspace group")
        );
        assert_eq!(category(src, 5, "--").as_deref(), Some("Media"));
        assert_eq!(
            direct_comment(src, 9, "--").as_deref(),
            Some("Restore lock")
        );
        assert_eq!(humanize("ClearNotifs"), "Clear notifications");
        assert_eq!(humanize("WindowPip"), "Window picture-in-picture");
        assert_eq!(
            category("# Apps\nbind=x\n# ignored\nbind=y", 4, "#").as_deref(),
            Some("Apps")
        );
        assert_eq!(
            category("-- http://x\n-- hl.bind(\n-- foo)\nx", 4, "--"),
            None
        );
    }

    #[test]
    fn every_dispatcher_family() {
        for (name, arg, expected) in [
            ("exec_cmd", json!(" foot "), "Run: foot"),
            ("global", json!("caelestia:launcher"), "Caelestia: launcher"),
            ("focus", json!({"direction":"left"}), "Focus window left"),
            ("focus", json!({"workspace":3}), "Go to workspace 3"),
            ("workspace", json!("e+1"), "Next workspace"),
            ("workspace", json!("-1"), "Previous workspace"),
            ("workspace", json!("special:x"), "Show special workspace x"),
            (
                "window.move",
                json!({"out_of_group":true}),
                "Move window out of group",
            ),
            (
                "window.move",
                json!({"workspace":3}),
                "Move window to workspace 3",
            ),
            (
                "window.move",
                json!({"direction":"right"}),
                "Move window right",
            ),
            ("window.fullscreen", json!({"mode":"maximized"}), "Maximize"),
            (
                "window.cycle_next",
                json!({"next":false}),
                "Cycle windows backwards",
            ),
            ("movefocus", json!("l"), "Focus window left"),
            ("movetoworkspace", json!("2"), "Move window to workspace 2"),
            ("layout", json!("togglesplit"), "Layout: togglesplit"),
            ("submap", json!("resize"), "Switch submap: resize"),
            ("new_api", json!("x"), "New api: x"),
        ] {
            assert_eq!(
                dispatcher_label(&Action::Dispatcher {
                    name: name.into(),
                    args: vec![arg]
                })
                .as_deref(),
                Some(expected)
            );
        }
        for (name, expected) in [
            ("window.close", "Close window"),
            ("window.float", "Toggle floating"),
            ("window.fullscreen", "Fullscreen"),
            ("window.pin", "Pin window"),
            ("window.center", "Center window"),
            ("window.drag", "Move window with the mouse"),
            ("window.resize", "Resize window with the mouse"),
            ("window.pseudo", "Toggle pseudo-tiling"),
            ("window.cycle_next", "Cycle windows"),
            ("group.toggle", "Toggle group"),
            ("group.next", "Next window in group"),
            ("group.prev", "Previous window in group"),
            ("group.lock_active", "Lock group"),
            ("exit", "Exit Hyprland"),
            ("no_op", "Nothing"),
        ] {
            assert_eq!(
                dispatcher_label(&Action::Dispatcher {
                    name: name.into(),
                    args: vec![]
                })
                .as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn label_priority_and_detail() {
        let file = PathBuf::from("binds.lua");
        let sources = BTreeMap::from([(
            file.clone(),
            "-- Comment\nhl.bind(vars.kbTerminal, action)".into(),
        )]);
        let mut bind = ConfigBind {
            key_text: "SUPER + T".into(),
            chord: Some("Super+T".into()),
            submap: None,
            action: Action::Dispatcher {
                name: "exec_cmd".into(),
                args: vec![json!("foot")],
            },
            flags: BTreeMap::new(),
            description: None,
            origin: Some(Origin { file, line: 2 }),
        };
        let mut labels = UserLabels::new();
        assert_eq!(for_config(&bind, &sources, &labels).label, "Terminal");
        assert_eq!(
            for_config(&bind, &sources, &labels).detail.as_deref(),
            Some("Run: foot")
        );
        bind.description = Some("Description".into());
        assert_eq!(for_config(&bind, &sources, &labels).label, "Description");
        labels.insert("|Super+T".into(), "Mine".into());
        assert_eq!(for_config(&bind, &sources, &labels).label, "Mine");
        bind.action = Action::LuaFunction {
            file: None,
            line: None,
        };
        bind.description = None;
        assert_eq!(
            for_config(&bind, &sources, &UserLabels::new()).label,
            "Comment"
        );
    }
}
