//! Hyprlang binds, variables, submaps, unbinds and bounded source includes.

use crate::{keys, read, Action, ConfigBind, ConfigReport, Origin, ReadPolicy};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ConfOptions {
    pub read_policy: ReadPolicy,
    pub home: Option<PathBuf>,
    pub max_depth: usize,
}

impl Default for ConfOptions {
    fn default() -> Self {
        Self {
            read_policy: ReadPolicy::user_config(),
            home: read::home(),
            max_depth: 32,
        }
    }
}

pub fn parse(path: &Path, options: &ConfOptions) -> ConfigReport {
    let mut parser = Parser {
        report: ConfigReport::default(),
        variables: BTreeMap::new(),
        submap: None,
        stack: Vec::new(),
        options,
        source_bytes: 0,
    };
    parser.file(path);
    parser.report
}

struct Parser<'a> {
    report: ConfigReport,
    variables: BTreeMap<String, String>,
    submap: Option<String>,
    stack: Vec<PathBuf>,
    options: &'a ConfOptions,
    source_bytes: usize,
}

impl Parser<'_> {
    fn warn(&mut self, text: String) {
        if self.report.warnings.len() < 128 {
            self.report.warnings.push(text);
        }
    }

    fn file(&mut self, path: &Path) {
        if self.stack.len() >= self.options.max_depth {
            self.warn(format!("{}: source depth limit exceeded", path.display()));
            return;
        }
        let (path, source) = match self.options.read_policy.read_text(path) {
            Ok(file) => file,
            Err(e) => {
                self.warn(format!("{}: {e}", path.display()));
                return;
            }
        };
        if self.stack.contains(&path) {
            self.warn(format!("{}: source include cycle", path.display()));
            return;
        }
        self.source_bytes = self.source_bytes.saturating_add(source.len());
        if self.source_bytes > 8 * 1024 * 1024 {
            self.warn("hyprlang source size limit exceeded".into());
            return;
        }
        self.report.sources.insert(path.clone(), source.clone());
        self.stack.push(path.clone());
        for (line, text) in source.lines().enumerate() {
            let text = strip_comment(text).trim();
            let Some((kind, value)) = text.split_once('=') else {
                continue;
            };
            let kind = kind.trim();
            let value = expand_variables(value.trim(), &self.variables);
            if let Some(var) = kind.strip_prefix('$') {
                self.variables.insert(var.trim().into(), value);
                continue;
            }
            match kind {
                "source" => {
                    let mut include = read::expand_home(
                        value.trim_matches(['\'', '"']),
                        self.options.home.as_deref(),
                    );
                    if include.is_relative() {
                        include = path.parent().unwrap_or(Path::new(".")).join(include);
                    }
                    let pattern = include.to_string_lossy();
                    match glob::glob(&pattern) {
                        Ok(entries) => {
                            let mut files = entries
                                .filter_map(Result::ok)
                                .take(1024)
                                .collect::<Vec<_>>();
                            files.sort();
                            if files.is_empty() {
                                self.warn(format!(
                                    "{}:{}: source matched no files: {pattern}",
                                    path.display(),
                                    line + 1
                                ));
                            }
                            for file in files {
                                self.file(&file);
                            }
                        }
                        Err(e) => self.warn(format!(
                            "{}:{}: invalid source pattern: {e}",
                            path.display(),
                            line + 1
                        )),
                    }
                }
                "submap" => self.submap = read::submap(&value),
                "unbind" => {
                    if let Some((mods, key)) = value.split_once(',') {
                        let chord = keys::chord_from_conf(mods.trim(), key.trim());
                        let key_text = format!("{}, {}", mods.trim(), key.trim());
                        self.report.binds.retain(|b| {
                            b.submap != self.submap
                                || if chord.is_some() {
                                    b.chord != chord
                                } else {
                                    b.key_text != key_text
                                }
                        });
                    }
                }
                _ if kind.starts_with("bind") => {
                    if self.report.binds.len() >= 10_000 {
                        self.warn("hyprlang bind count limit exceeded".into());
                        break;
                    }
                    self.bind(
                        kind,
                        &value,
                        Origin {
                            file: path.clone(),
                            line: line as u32 + 1,
                        },
                    );
                }
                _ => {}
            }
        }
        self.stack.pop();
    }

    fn bind(&mut self, kind: &str, value: &str, origin: Origin) {
        let letters = kind
            .strip_prefix("bind")
            .unwrap_or_default()
            .trim_matches(['[', ']']);
        let mut flags = BTreeMap::new();
        let mut has_desc = false;
        for flag in letters.chars() {
            let name = match flag {
                'l' => "locked",
                'r' => "release",
                'e' => "repeating",
                'm' => "mouse",
                'n' => "non_consuming",
                't' => "transparent",
                'i' => "ignore_mods",
                'o' => "long_press",
                'c' => "click",
                'g' => "drag",
                's' => "separate",
                'p' => "bypass",
                'd' => {
                    has_desc = true;
                    continue;
                }
                _ => {
                    self.warn(format!(
                        "{}:{}: unknown bind flag {flag}",
                        origin.file.display(),
                        origin.line
                    ));
                    continue;
                }
            };
            flags.insert(name.into(), true);
        }
        let fields = value
            .splitn(if has_desc { 5 } else { 4 }, ',')
            .map(str::trim)
            .collect::<Vec<_>>();
        if fields.len() < if has_desc { 4 } else { 3 } {
            self.warn(format!(
                "{}:{}: malformed bind",
                origin.file.display(),
                origin.line
            ));
            return;
        }
        let dispatcher = if has_desc { 3 } else { 2 };
        let args = fields
            .get(dispatcher + 1)
            .map(|arg| vec![serde_json::Value::String((*arg).into())])
            .unwrap_or_default();
        self.report.binds.push(ConfigBind {
            key_text: format!("{}, {}", fields[0], fields[1]),
            chord: keys::chord_from_conf(fields[0], fields[1]),
            submap: self.submap.clone(),
            action: Action::Dispatcher {
                name: fields[dispatcher].into(),
                args,
            },
            flags,
            description: has_desc.then(|| fields[2].to_owned()),
            origin: Some(origin),
        });
    }
}

fn strip_comment(text: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '#' {
            return &text[..i];
        }
    }
    text
}

fn expand_variables(text: &str, variables: &BTreeMap<String, String>) -> String {
    let mut result = text.to_owned();
    for _ in 0..8 {
        let mut expanded = String::new();
        let mut rest = result.as_str();
        while let Some(i) = rest.find('$') {
            expanded.push_str(&rest[..i]);
            rest = &rest[i + 1..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let name = &rest[..end];
            match variables.get(name) {
                Some(value) => expanded.push_str(value),
                None => {
                    expanded.push('$');
                    expanded.push_str(name);
                }
            }
            rest = &rest[end..];
        }
        expanded.push_str(rest);
        if expanded == result {
            break;
        }
        result = expanded;
        if result.len() > 4 * 1024 * 1024 {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_sources_descriptions_submaps_and_unbinds() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hyprland.conf");
        std::fs::write(&file, "$mod = SUPER\n$cmd = foot\nsource = *.binds.conf\nsubmap = resize\nbindle = $mod, R, Size up, resizewindow, 10, 0\nsubmap = reset\nbind = $mod, A, exec, old\nunbind = $mod, A\nbind = $mod, A, exec, $cmd\n").unwrap();
        std::fs::write(
            dir.path().join("extra.binds.conf"),
            "# Apps\nbindd = $mod, T, Terminal, exec, $cmd\nbindm = $mod, mouse:272, movewindow\n",
        )
        .unwrap();
        let report = parse(
            &file,
            &ConfOptions {
                read_policy: ReadPolicy::new([dir.path().to_owned()]),
                ..Default::default()
            },
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.binds.len(), 4);
        assert_eq!(report.binds[0].chord.as_deref(), Some("Super+T"));
        assert_eq!(report.binds[0].description.as_deref(), Some("Terminal"));
        assert_eq!(report.binds[2].submap.as_deref(), Some("resize"));
        assert_eq!(report.binds[2].flags.get("repeating"), Some(&true));
        assert_eq!(
            report.binds[3].action,
            Action::Dispatcher {
                name: "exec".into(),
                args: vec![serde_json::json!("foot")]
            }
        );
    }

    #[test]
    fn cycles_missing_sources_and_all_flags_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hyprland.conf");
        std::fs::write(&file, "source = hyprland.conf\nsource = absent*\nbindlremntiocsgp = SUPER, T, exec, foot\nbindz = nonsense\n").unwrap();
        let report = parse(
            &file,
            &ConfOptions {
                read_policy: ReadPolicy::new([dir.path().to_owned()]),
                ..Default::default()
            },
        );
        assert!(report.warnings.iter().any(|w| w.contains("cycle")));
        assert!(report.warnings.iter().any(|w| w.contains("no files")));
        assert_eq!(report.binds[0].flags.len(), 12);
        assert_eq!(
            strip_comment("exec = echo '#ok' # ignored"),
            "exec = echo '#ok' "
        );
        assert_eq!(
            expand_variables(
                "$a $b",
                &BTreeMap::from([("a".into(), "$b".into()), ("b".into(), "value".into())])
            ),
            "value value"
        );
    }
}
