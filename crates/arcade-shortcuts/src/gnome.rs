//! Effective GSettings shortcuts, with a saved-overrides-only dconf fallback.
//! All commands use list-recursively/dump. No setting or config is ever written.

use crate::{keys, labels, wm, Action, BindState, ConfigKind, WmBind};
use std::collections::BTreeMap;

pub const WM_SCHEMA: &str = "org.gnome.desktop.wm.keybindings";
pub const MEDIA_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const CUSTOM_ROOT: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";

#[derive(Debug, Clone)]
pub struct CustomShortcut {
    pub path: String,
    pub name: String,
    pub binding: String,
    pub command: String,
}

fn fields(line: &str) -> Option<(&str, &str, &str)> {
    let (schema, rest) = line.trim().split_once(char::is_whitespace)?;
    let (key, value) = rest.trim_start().split_once(char::is_whitespace)?;
    Some((schema, key, value.trim()))
}

fn quoted(text: &str) -> Result<(String, &str), String> {
    let quote = text.chars().next().ok_or("missing GVariant string")?;
    if quote != '\'' && quote != '"' {
        return Err("expected a quoted GVariant string".into());
    }
    let mut result = String::new();
    let mut chars = text[1..].char_indices();
    while let Some((position, ch)) = chars.next() {
        if ch == quote {
            return Ok((result, &text[position + 2..]));
        }
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let (_, escaped) = chars.next().ok_or("truncated GVariant escape")?;
        match escaped {
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            '\\' | '\'' | '"' => result.push(escaped),
            'u' | 'U' => {
                let count = if escaped == 'u' { 4 } else { 8 };
                let hex: String = chars.by_ref().take(count).map(|(_, ch)| ch).collect();
                let value = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or("invalid GVariant Unicode escape")?;
                result.push(value);
            }
            _ => return Err(format!("unsupported GVariant escape {escaped}")),
        }
    }
    Err("unterminated GVariant string".into())
}

/// Parse only string/string-array GVariants, never evaluate their contents.
pub fn string_values(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim().strip_prefix("@as ").unwrap_or(text.trim());
    if text.starts_with(['\'', '"']) {
        let (value, tail) = quoted(text)?;
        return if tail.trim().is_empty() {
            Ok(vec![value])
        } else {
            Err("unexpected GVariant suffix".into())
        };
    }
    let mut rest = text
        .strip_prefix('[')
        .ok_or("not a GVariant string array")?
        .trim_start();
    let mut result = Vec::new();
    loop {
        if let Some(tail) = rest.strip_prefix(']') {
            return if tail.trim().is_empty() {
                Ok(result)
            } else {
                Err("unexpected GVariant array suffix".into())
            };
        }
        let (value, tail) = quoted(rest)?;
        result.push(value);
        if result.len() > 256 {
            return Err("too many GVariant strings".into());
        }
        rest = tail.trim_start();
        if let Some(tail) = rest.strip_prefix(',') {
            rest = tail.trim_start();
        } else if !rest.starts_with(']') {
            return Err("missing GVariant array separator".into());
        }
    }
}

pub fn gtk_chord(text: &str) -> Option<String> {
    let mut rest = text.trim();
    if rest.is_empty() || rest.eq_ignore_ascii_case("disabled") {
        return None;
    }
    let mut modifiers = Vec::new();
    while let Some(tail) = rest.strip_prefix('<') {
        let (name, tail) = tail.split_once('>')?;
        modifiers.push(match name.to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "ctl" | "primary" => "Ctrl",
            "alt" | "mod1" => "Alt",
            "shift" => "Shift",
            "super" | "mod4" | "meta" => "Super",
            _ => return None,
        });
        rest = tail;
    }
    if rest == "ISO_Left_Tab" {
        modifiers.push("Shift");
        rest = "Tab";
    }
    let key = crate::kde::qt_sequence(rest)?;
    modifiers.push(&key);
    keys::chord_from_key_string(&modifiers.join("+"))
}

pub fn custom_paths(text: &str) -> Result<Vec<String>, String> {
    let Some((_, _, value)) = text
        .lines()
        .filter_map(fields)
        .find(|(_, key, _)| *key == "custom-keybindings")
    else {
        return Ok(Vec::new());
    };
    let paths = string_values(value)?;
    if paths.iter().any(|path| {
        !path.starts_with(CUSTOM_ROOT)
            || !path.ends_with('/')
            || !path
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "/_-".contains(ch))
    }) {
        return Err("invalid GNOME custom shortcut path".into());
    }
    Ok(paths)
}

pub fn parse_custom(path: &str, text: &str) -> Result<CustomShortcut, String> {
    let mut values = BTreeMap::new();
    for (_, key, value) in text.lines().filter_map(fields) {
        if ["name", "binding", "command"].contains(&key) {
            let values_for_key = string_values(value)?;
            if values_for_key.len() != 1 {
                return Err("custom shortcut value must be a string".into());
            }
            values.insert(key, values_for_key[0].clone());
        }
    }
    Ok(CustomShortcut {
        path: path.into(),
        name: values.get("name").cloned().unwrap_or_default(),
        binding: values.get("binding").cloned().unwrap_or_default(),
        command: values.get("command").cloned().unwrap_or_default(),
    })
}

pub fn parse(
    text: &str,
    custom: &[CustomShortcut],
    overrides: &labels::UserLabels,
) -> wm::WmReport {
    let mut report = wm::WmReport {
        name: "gnome".into(),
        config_kind: ConfigKind::Gsettings,
        ..Default::default()
    };
    for (schema, key, value) in text.lines().filter_map(fields) {
        if ![WM_SCHEMA, MEDIA_SCHEMA].contains(&schema) || key == "custom-keybindings" {
            continue;
        }
        // The media-keys schema also contains booleans and timeout integers.
        if !value.starts_with(['[', '\'', '"', '@']) {
            continue;
        }
        let alternatives = match string_values(value) {
            Ok(values) => values,
            Err(error) => {
                report.warnings.push(format!("{schema} {key}: {error}"));
                continue;
            }
        };
        for alternative in alternatives {
            if alternative.is_empty() || alternative == "disabled" {
                continue;
            }
            let Some(chord) = gtk_chord(&alternative) else {
                report.warnings.push(format!(
                    "{schema} {key}: unsupported accelerator {alternative:?}"
                ));
                continue;
            };
            report.binds.push(binding(
                chord,
                &alternative,
                labels::humanize(key.trim_end_matches("-static")),
                format!("GNOME: {schema} · {key}"),
                if schema == WM_SCHEMA {
                    "Windows"
                } else {
                    "Media"
                },
            ));
        }
    }
    for custom in custom {
        if custom.binding.is_empty() || custom.binding == "disabled" {
            continue;
        }
        let Some(chord) = gtk_chord(&custom.binding) else {
            report
                .warnings
                .push(format!("{}: unsupported custom accelerator", custom.path));
            continue;
        };
        report.binds.push(binding(
            chord,
            &custom.binding,
            if custom.name.trim().is_empty() {
                "Custom shortcut".into()
            } else {
                custom.name.trim().into()
            },
            format!("Run: {}", custom.command),
            "Custom shortcuts",
        ));
    }
    report.apply_labels(overrides);
    report
}

fn binding(chord: String, text: &str, label: String, detail: String, category: &str) -> WmBind {
    WmBind {
        chord: Some(chord),
        key_text: text.into(),
        submap: None,
        label,
        detail: Some(detail),
        category: Some(category.into()),
        action: Action::Unknown,
        flags: BTreeMap::new(),
        origin: None,
        state: BindState::ConfiguredOnly,
    }
}

/// dconf dump contains saved overrides, not schema defaults.
pub fn parse_dconf(text: &str, overrides: &labels::UserLabels) -> wm::WmReport {
    let mut group = "";
    let mut settings = String::new();
    let mut customs: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            group = &line[1..line.len() - 1];
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let schema = match group {
            "desktop/wm/keybindings" => WM_SCHEMA,
            "settings-daemon/plugins/media-keys" => MEDIA_SCHEMA,
            custom
                if custom.starts_with("settings-daemon/plugins/media-keys/custom-keybindings/") =>
            {
                customs
                    .entry(format!("/org/gnome/{custom}/"))
                    .or_default()
                    .push_str(&format!("{CUSTOM_SCHEMA} {key} {value}\n"));
                continue;
            }
            _ => continue,
        };
        settings.push_str(&format!("{schema} {key} {value}\n"));
    }
    let enabled = custom_paths(&settings);
    let custom = enabled
        .as_ref()
        .map(|paths| {
            paths
                .iter()
                .filter_map(|path| {
                    customs
                        .get(path)
                        .and_then(|text| parse_custom(path, text).ok())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut report = parse(&settings, &custom, overrides);
    if let Err(error) = enabled {
        report.warnings.push(error);
    }
    for bind in &mut report.binds {
        bind.state = BindState::ConfiguredOnly;
    }
    report.warnings.push("dconf fallback contains saved overrides only; schema defaults and runtime registration are not verified".into());
    report
}

pub fn read_machine(overrides: &labels::UserLabels) -> wm::WmReport {
    let mut text = String::new();
    let mut warnings = Vec::new();
    for schema in [WM_SCHEMA, MEDIA_SCHEMA] {
        match wm::query("gsettings", &["list-recursively", schema]) {
            Ok(output) => {
                text.push_str(&output);
                text.push('\n');
            }
            Err(error) => warnings.push(error),
        }
    }
    if text.is_empty() {
        let mut report = match wm::query("dconf", &["dump", "/org/gnome/"]) {
            Ok(text) => parse_dconf(&text, overrides),
            Err(error) => wm::WmReport {
                name: "gnome".into(),
                config_kind: ConfigKind::Gsettings,
                warnings: vec![error],
                ..Default::default()
            },
        };
        report.warnings.extend(warnings);
        return report;
    }
    let mut custom = Vec::new();
    match custom_paths(&text) {
        Ok(paths) => {
            for path in paths.iter().take(16) {
                let schema = format!("{CUSTOM_SCHEMA}:{path}");
                match wm::query("gsettings", &["list-recursively", &schema])
                    .and_then(|text| parse_custom(path, &text))
                {
                    Ok(shortcut) => custom.push(shortcut),
                    Err(error) => warnings.push(error),
                }
            }
            if paths.len() > 16 {
                warnings.push("GNOME custom shortcut query limit (16) reached".into());
            }
        }
        Err(error) => warnings.push(error),
    }
    let mut report = parse(&text, &custom, overrides);
    report.warnings.extend(warnings);
    report.warnings.push(
        "GNOME shortcuts use effective GSettings values; runtime registration is not verified"
            .into(),
    );
    report
}
