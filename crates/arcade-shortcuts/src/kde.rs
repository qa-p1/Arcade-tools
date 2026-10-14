//! Read kglobalshortcutsrc: current keys, default keys, friendly action name.
//! Default keys are metadata, never substituted for disabled current keys.

use crate::{
    keys, labels, read, wm::WmReport, Action, BindState, ConfigKind, Origin, ReadPolicy, WmBind,
};
use std::collections::BTreeMap;
use std::path::Path;

pub fn parse(text: &str, path: &Path, overrides: &labels::UserLabels) -> WmReport {
    let mut report = WmReport {
        name: "kde".into(),
        config_kind: ConfigKind::Kconfig,
        ..Default::default()
    };
    let mut section = String::new();
    let mut entries = Vec::new();
    let mut names = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .replace("][", "/");
            continue;
        }
        if section.is_empty() || section.starts_with('$') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            report.warnings.push(format!(
                "{}:{}: malformed KDE entry",
                path.display(),
                index + 1
            ));
            continue;
        };
        let key = key.trim().trim_end_matches("[$i]");
        if key == "_k_friendly_name" {
            names.insert(section.clone(), unescape(value));
            continue;
        }
        if key.starts_with(['_', '$']) {
            continue;
        }
        if key.contains("[$") {
            report.warnings.push(format!(
                "{}:{}: unsupported KDE expansion/deletion flag",
                path.display(),
                index + 1
            ));
            continue;
        }
        entries.push((
            section.clone(),
            key.to_owned(),
            value.to_owned(),
            index as u32 + 1,
        ));
    }
    for (section, key, value, line) in entries {
        let fields = list_fields(&value);
        if fields.len() != 3 {
            report.warnings.push(format!(
                "{}:{line}: KDE shortcut needs three fields",
                path.display()
            ));
            continue;
        }
        let label = if fields[2].trim().is_empty() {
            labels::humanize(&key)
        } else {
            fields[2].trim().to_owned()
        };
        let category = names
            .get(&section)
            .cloned()
            .unwrap_or_else(|| section.clone());
        let submap = section
            .split_once('/')
            .map(|(_, context)| context.to_owned());
        for alternative in fields[0].split('\t').map(str::trim) {
            if alternative.is_empty() || alternative.eq_ignore_ascii_case("none") {
                continue;
            }
            let Some(chord) = qt_sequence(alternative) else {
                report.warnings.push(format!(
                    "{}:{line}: unsupported KDE key {alternative:?}",
                    path.display()
                ));
                continue;
            };
            report.binds.push(WmBind {
                chord: Some(chord),
                key_text: alternative.into(),
                submap: submap.clone(),
                label: label.clone(),
                detail: Some(format!("KDE: {section} · {key}")),
                category: Some(category.clone()),
                action: Action::Dispatcher {
                    name: "kde.shortcut".into(),
                    args: vec![section.clone().into(), key.clone().into()],
                },
                flags: BTreeMap::new(),
                origin: Some(Origin {
                    file: path.to_owned(),
                    line,
                }),
                state: BindState::ConfiguredOnly,
            });
        }
    }
    report.apply_labels(overrides);
    report
}

/// KConfig QStringList commas can be escaped; \t separates alternatives.
fn list_fields(text: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            field.push('\\');
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ',' {
            fields.push(unescape(&field));
            field.clear();
        } else {
            field.push(ch);
        }
    }
    if escaped {
        field.push('\\');
    }
    fields.push(unescape(&field));
    fields
}

fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => result.push('\t'),
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('s') => result.push(' '),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(char::from(byte));
                } else {
                    result.push_str("\\x");
                    result.push_str(&hex);
                }
            }
            Some(ch) => result.push(ch),
            None => result.push('\\'),
        }
    }
    result
}

/// Qt uses ", " between sequence chords and names with spaces for media keys.
pub fn qt_sequence(text: &str) -> Option<String> {
    text.split(", ")
        .map(|chord| {
            let text = chord
                .trim()
                .replace("Volume Up", "VolumeUp")
                .replace("Volume Down", "VolumeDown")
                .replace("Volume Mute", "VolumeMute")
                .replace("Microphone Mute", "MicMute")
                .replace("Media Play", "MediaPlayPause")
                .replace("Media Pause", "MediaPlayPause")
                .replace("Media Next", "MediaNext")
                .replace("Media Previous", "MediaPrevious")
                .replace("Media Stop", "MediaStop")
                .replace("Monitor Brightness Up", "BrightnessUp")
                .replace("Monitor Brightness Down", "BrightnessDown")
                .replace("Backtab", "Shift+Tab")
                .replace("PgUp", "PageUp")
                .replace("PgDown", "PageDown");
            let (mods, key) = text.rsplit_once('+').unwrap_or(("", &text));
            let key = match key {
                "Del" => "Delete",
                "Ins" => "Insert",
                "/" => "Slash",
                "," => "Comma",
                "." => "Period",
                "-" => "Minus",
                "=" => "Equal",
                ";" => "Semicolon",
                "'" => "Quote",
                "\u{60}" => "Backquote",
                "[" => "BracketLeft",
                "]" => "BracketRight",
                "\\" => "Backslash",
                other => other,
            };
            let text = if mods.is_empty() {
                key.to_owned()
            } else {
                format!("{mods}+{key}")
            };
            keys::chord_from_key_string(&text)
        })
        .collect::<Option<Vec<_>>>()
        .map(|chords| chords.join(" "))
}

pub fn read_machine(overrides: &labels::UserLabels) -> WmReport {
    let Some(config) = read::config_home() else {
        return WmReport {
            name: "kde".into(),
            warnings: vec!["No KDE config directory".into()],
            ..Default::default()
        };
    };
    let path = config.join("kglobalshortcutsrc");
    match ReadPolicy::new([config]).read_text(&path) {
        Ok((path, text)) => {
            let mut report = parse(&text, &path, overrides);
            report.files.push(path);
            report.warnings.push(
                "KDE shortcuts are read from configuration; runtime registration is not verified"
                    .into(),
            );
            report
        }
        Err(error) => WmReport {
            name: "kde".into(),
            config_kind: ConfigKind::Kconfig,
            warnings: vec![format!("{}: {error}", path.display())],
            ..Default::default()
        },
    }
}
