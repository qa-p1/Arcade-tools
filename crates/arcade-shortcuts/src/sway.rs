//! Sway config and read-only IPC snapshots. Commands in config are never run.

use crate::{
    keys, labels, read, wm, Action, BindState, ConfigBind, ConfigReport, Origin, ReadPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Options {
    pub read_policy: ReadPolicy,
    pub home: Option<PathBuf>,
    pub max_files: usize,
    pub max_depth: usize,
    pub max_bytes: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            read_policy: ReadPolicy::new(
                read::config_home()
                    .into_iter()
                    .chain([PathBuf::from("/etc/sway"), PathBuf::from("/etc/i3")])
                    .chain(read::home().map(|home| home.join(".sway"))),
            ),
            home: read::home(),
            max_files: 128,
            max_depth: 16,
            max_bytes: 8 * 1024 * 1024,
        }
    }
}

struct Parser<'a> {
    options: &'a Options,
    report: ConfigReport,
    variables: BTreeMap<String, String>,
    active: BTreeSet<PathBuf>,
    bytes: usize,
    files: usize,
    mode: Option<String>,
    blocks: Vec<(Option<String>, Option<String>)>,
    prefix: Option<String>,
}

pub fn parse(path: &Path, options: &Options) -> ConfigReport {
    let mut parser = Parser::new(options);
    parser.file(path, 0);
    parser.report
}

pub fn parse_text(text: &str, origin: &Path, options: &Options) -> ConfigReport {
    let mut parser = Parser::new(options);
    parser.source(origin.to_owned(), text.to_owned(), 0);
    parser.report
}

impl<'a> Parser<'a> {
    fn new(options: &'a Options) -> Self {
        Self {
            options,
            report: ConfigReport::default(),
            variables: BTreeMap::new(),
            active: BTreeSet::new(),
            bytes: 0,
            files: 0,
            mode: None,
            blocks: Vec::new(),
            prefix: None,
        }
    }
    fn warn(&mut self, path: &Path, line: u32, message: &str) {
        self.report
            .warnings
            .push(format!("{}:{line}: {message}", path.display()));
    }
    fn file(&mut self, path: &Path, depth: usize) {
        if depth > self.options.max_depth || self.files >= self.options.max_files {
            self.warn(path, 0, "config file/depth limit exceeded");
            return;
        }
        match self.options.read_policy.read_text(path) {
            Ok((path, source)) => self.source(path, source, depth),
            Err(error) => self.warn(path, 0, &error.to_string()),
        }
    }
    fn source(&mut self, path: PathBuf, source: String, depth: usize) {
        if depth > self.options.max_depth
            || self.files >= self.options.max_files
            || self.bytes.saturating_add(source.len()) > self.options.max_bytes
            || self.active.contains(&path)
        {
            self.warn(&path, 0, "include cycle or config limit exceeded");
            return;
        }
        self.files += 1;
        self.bytes += source.len();
        self.active.insert(path.clone());
        self.report.sources.insert(path.clone(), source.clone());
        let mut pending = String::new();
        let mut start = 1;
        for (index, physical) in source.lines().enumerate() {
            if pending.is_empty() {
                start = index as u32 + 1;
            }
            let line = physical.trim_end();
            if let Some(piece) = line.strip_suffix('\\') {
                pending.push_str(piece);
                pending.push(' ');
                continue;
            }
            pending.push_str(line);
            self.line(&path, start, &pending, depth);
            pending.clear();
        }
        if !pending.is_empty() {
            self.line(&path, start, &pending, depth);
        }
        self.active.remove(&path);
    }
    fn expand(&self, text: &str) -> String {
        let mut value = text.to_owned();
        for _ in 0..8 {
            let mut next = String::new();
            let mut rest = value.as_str();
            while let Some(position) = rest.find('$') {
                next.push_str(&rest[..position]);
                rest = &rest[position..];
                let length = rest[1..]
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .count()
                    + 1;
                let name = &rest[..length];
                if let Some(replacement) = self.variables.get(name) {
                    next.push_str(replacement);
                } else if name == "$HOME" {
                    next.push_str(
                        &self
                            .options
                            .home
                            .as_ref()
                            .map(|path| path.to_string_lossy())
                            .unwrap_or_default(),
                    );
                } else {
                    next.push_str(name);
                }
                rest = &rest[length..];
            }
            next.push_str(rest);
            if next == value || next.len() > 64 * 1024 {
                break;
            }
            value = next;
        }
        value
    }
    fn line(&mut self, path: &Path, line: u32, text: &str, depth: usize) {
        let text = strip_comment(text).trim();
        if text.is_empty() || text.starts_with('#') {
            return;
        }
        if text.starts_with('}') {
            if let Some((mode, prefix)) = self.blocks.pop() {
                self.mode = mode;
                self.prefix = prefix;
            }
            return;
        }
        let Some((command, rest)) = word(text) else {
            return;
        };
        if command == "set" {
            if let Some((name, value)) = word(rest) {
                if name.starts_with('$') {
                    let value = self.expand(value.trim());
                    let value = word(&value)
                        .filter(|(_, tail)| tail.trim().is_empty())
                        .map_or_else(|| value.clone(), |(word, _)| word);
                    self.variables.insert(name, value);
                }
            }
            return;
        }
        if command == "include" {
            let expanded = self.expand(rest);
            let mut patterns = expanded.as_str();
            while let Some((pattern, tail)) = word(patterns) {
                patterns = tail;
                let pattern = read::expand_home(&pattern, self.options.home.as_deref());
                let pattern = if pattern.is_absolute() {
                    pattern
                } else {
                    path.parent().unwrap_or(Path::new(".")).join(pattern)
                };
                match glob::glob(&pattern.to_string_lossy()) {
                    Ok(paths) => {
                        let mut files = paths
                            .filter_map(Result::ok)
                            .take(self.options.max_files + 1)
                            .collect::<Vec<_>>();
                        files.sort();
                        if files.is_empty() {
                            self.warn(path, line, "include did not match a file");
                        }
                        for file in files {
                            self.file(&file, depth + 1);
                        }
                    }
                    Err(error) => self.warn(path, line, &format!("include: {error}")),
                }
            }
            return;
        }
        if command == "mode" && rest.trim_end().ends_with('{') {
            let rest = rest
                .trim_start()
                .strip_prefix("--pango_markup")
                .unwrap_or(rest);
            if let Some((name, _)) = word(rest) {
                let name = self.expand(&name);
                self.blocks.push((self.mode.clone(), self.prefix.clone()));
                self.mode = (name != "default").then_some(name);
                self.prefix = None;
            }
            return;
        }
        if rest.trim() == "{" {
            self.blocks.push((self.mode.clone(), self.prefix.clone()));
            self.prefix = ["bindsym", "bindcode"]
                .contains(&command.as_str())
                .then_some(command);
            return;
        }
        let (kind, rest) = if ["bindsym", "bindcode"].contains(&command.as_str()) {
            (command.as_str(), rest)
        } else if let Some(prefix) = self.prefix.as_deref() {
            (prefix, text)
        } else {
            return;
        };
        let code = kind == "bindcode";
        let mut flags = BTreeMap::new();
        let mut rest = rest;
        let key = loop {
            let Some((token, tail)) = word(rest) else {
                return;
            };
            rest = tail;
            if let Some(flag) = token.strip_prefix("--") {
                flags.insert(
                    flag.split('=').next().unwrap_or(flag).replace('-', "_"),
                    true,
                );
            } else {
                break self.expand(&token);
            }
        };
        let action_text = self.expand(rest.trim());
        if action_text.is_empty() {
            self.warn(path, line, "binding has no command");
            return;
        }
        let key = if code {
            match key.rsplit_once('+') {
                Some((mods, key)) => format!("{mods}+code:{key}"),
                None => format!("code:{key}"),
            }
        } else {
            key
        };
        let chord = keys::chord_from_key_string(&key);
        if chord.is_none() {
            self.warn(path, line, "unsupported key or unexpanded variable");
        }
        let Some((name, args)) = word(&action_text) else {
            return;
        };
        let name = match name.as_str() {
            "kill" => "window.close",
            "focus" => "movefocus",
            "mode" => "submap",
            other => other,
        };
        let source = self.report.sources.get(path).unwrap();
        let description = labels::direct_comment(source, line, "#")
            .filter(|comment| Some(comment) != labels::category(source, line, "#").as_ref());
        self.report.binds.push(ConfigBind {
            key_text: key,
            chord,
            submap: self.mode.clone(),
            action: Action::Dispatcher {
                name: name.into(),
                args: vec![serde_json::Value::String(args.trim().to_owned())],
            },
            flags,
            description,
            origin: Some(Origin {
                file: path.to_owned(),
                line,
            }),
        });
    }
}

/// One shell-like config word. Only quoting is parsed; no shell expansion.
fn word(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    let mut result = String::new();
    let mut quote = None;
    let mut escaped = false;
    for (position, ch) in text.char_indices() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if ch == current {
                quote = None;
            } else {
                result.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            return Some((result, &text[position..]));
        } else {
            result.push(ch);
        }
    }
    Some((result, ""))
}

fn strip_comment(text: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut whitespace = true;
    for (position, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            whitespace = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if ch == current {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '#' && whitespace {
            return &text[..position];
        }
        whitespace = ch.is_whitespace();
    }
    text
}

pub fn parse_modes(text: &str) -> Result<Vec<String>, String> {
    serde_json::from_str(text).map_err(|error| format!("Sway binding modes: {error}"))
}

pub fn report(
    config: ConfigReport,
    loaded_modes: Option<&[String]>,
    overrides: &labels::UserLabels,
) -> wm::WmReport {
    let mut report = wm::configured("sway", config, overrides);
    let mut missing = BTreeSet::new();
    for bind in &mut report.binds {
        if let Some(mode) = &bind.submap {
            bind.category.get_or_insert_with(|| format!("Mode: {mode}"));
        }
        // Modes attest that a mode was loaded, not each binding/action.
        if loaded_modes.is_some_and(|modes| {
            !modes
                .iter()
                .any(|mode| mode == bind.submap.as_deref().unwrap_or("default"))
        }) {
            bind.state = BindState::ConfiguredOnly;
            missing.insert(bind.submap.as_deref().unwrap_or("default").to_owned());
        }
    }
    for mode in missing {
        report.warnings.push(format!(
            "Sway mode {mode:?} is configured but absent from the loaded mode list"
        ));
    }
    report
}

pub fn read_machine(overrides: &labels::UserLabels) -> wm::WmReport {
    let options = Options::default();
    let mut warnings = Vec::new();
    let modes = wm::query("swaymsg", &["-r", "-t", "get_binding_modes"])
        .and_then(|text| parse_modes(&text))
        .map_err(|error| warnings.push(error))
        .ok();
    let loaded = wm::query("swaymsg", &["-r", "-t", "get_config"])
        .and_then(|text| {
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            value
                .get("config")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or("Sway config reply has no config".into())
        })
        .map_err(|error| warnings.push(error))
        .ok();
    let path = wm::query("swaymsg", &["-r", "-t", "get_version"])
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("loaded_config_file_name")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        })
        .or_else(|| read::config_home().map(|home| home.join("sway/config")));
    let config = match (loaded, path) {
        (Some(text), path) => parse_text(
            &text,
            &path.unwrap_or_else(|| PathBuf::from("sway-ipc-config")),
            &options,
        ),
        (None, Some(path)) => parse(&path, &options),
        _ => ConfigReport {
            warnings: vec!["No Sway config found".into()],
            ..Default::default()
        },
    };
    let mut report = report(config, modes.as_deref(), overrides);
    report.warnings.extend(warnings);
    report.warnings.push("Sway IPC exposes mode names and loaded config, not an authoritative binding list; bindings are configured, not runtime-verified".into());
    report
}
