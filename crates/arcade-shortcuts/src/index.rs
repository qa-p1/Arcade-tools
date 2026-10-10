//! Pinned overlay index, assembled on a worker and atomically published once.

use crate::{
    hyprland::{ConfigKind, HyprlandReport},
    keys, labels, read, BindState, Origin, WmBind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
        }
    }
    fn match_key(self) -> &'static str {
        match self {
            Self::Linux => "class",
            Self::Windows => "exe",
            Self::Macos => "bundle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildInputs {
    pub os: Os,
    pub catalog_dir: Option<PathBuf>,
    pub arcade_docs: Vec<PathBuf>,
    pub registry_dir: Option<PathBuf>,
    pub hyprland: Option<HyprlandReport>,
    pub user_labels: Option<PathBuf>,
    /// For deterministic fixture builds; otherwise the current UTC timestamp.
    pub built_at: Option<String>,
}

impl Default for BuildInputs {
    fn default() -> Self {
        Self {
            os: Os::current(),
            catalog_dir: None,
            arcade_docs: Vec::new(),
            registry_dir: None,
            hyprland: None,
            user_labels: None,
            built_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub schema: u32,
    pub built_at: String,
    pub os: Os,
    pub apps: Vec<AppSheet>,
    pub arcade: Vec<ArcadeShortcut>,
    pub wm: WmIndex,
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSheet {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(rename = "match")]
    pub matches: BTreeMap<String, Vec<String>>,
    pub groups: Vec<AppGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppGroup {
    pub title: String,
    pub context: String,
    pub shortcuts: Vec<AppShortcut>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppShortcut {
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub display: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArcadeShortcut {
    pub app: String,
    pub name: String,
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub display: Vec<String>,
    pub rebindable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WmIndex {
    pub name: String,
    pub config_kind: ConfigKind,
    pub groups: Vec<WmGroup>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WmGroup {
    pub title: String,
    pub shortcuts: Vec<WmShortcut>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WmShortcut {
    pub chord: String,
    pub display: String,
    pub label: String,
    pub detail: Option<String>,
    pub submap: Option<String>,
    pub state: BindState,
    pub origin: Option<Origin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictParty {
    pub kind: String,
    pub owner: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub chord: String,
    pub a: ConflictParty,
    pub b: ConflictParty,
}

/// Input failures are isolated per file and reported in `wm.warnings`; valid
/// sources still produce an index. This function performs no Hyprland IPC.
pub fn build_index(inputs: &BuildInputs) -> Index {
    let mut warnings = Vec::new();
    let user_labels = inputs
        .user_labels
        .as_deref()
        .map(labels::load_user_labels)
        .transpose()
        .unwrap_or_else(|e| {
            warnings.push(format!("User shortcut labels: {e}"));
            None
        })
        .unwrap_or_default();
    let mut apps = Vec::new();
    let mut docs = BTreeMap::new();
    if let Some(dir) = &inputs.catalog_dir {
        for path in json_files(dir, &mut warnings) {
            if path.file_name().is_some_and(|n| n == "index.json") {
                continue;
            }
            if let Some(value) = read_json(&path, &mut warnings) {
                if let Some(sheet) = sheet(&value, "catalog", inputs.os, &path, &mut warnings) {
                    apps.push(sheet);
                }
            }
        }
    }
    for path in &inputs.arcade_docs {
        if let Some(value) = read_json(path, &mut warnings) {
            if let Some(sheet) = sheet(&value, "arcade", inputs.os, path, &mut warnings) {
                docs.insert(sheet.id.clone(), value);
                apps.push(sheet);
            }
        }
    }
    let mut arcade = Vec::new();
    if let Some(dir) = &inputs.registry_dir {
        for path in json_files(dir, &mut warnings) {
            let Some(value) = read_json(&path, &mut warnings) else {
                continue;
            };
            let Some(app) = value.get("id").and_then(Value::as_str) else {
                warnings.push(format!("{}: manifest has no id", path.display()));
                continue;
            };
            let name = value.get("name").and_then(Value::as_str).unwrap_or(app);
            if let Some(shortcuts) = value.get("shortcuts").and_then(Value::as_array) {
                for shortcut in shortcuts {
                    let Some(id) = shortcut.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let keys = effective_keys(
                        shortcut.get("accelerator").or_else(|| shortcut.get("keys")),
                        inputs.os,
                        &path,
                        &mut warnings,
                    );
                    if keys.is_empty() {
                        continue;
                    }
                    let doc = docs.get(app).and_then(|d| doc_shortcut(d, id));
                    let title = shortcut
                        .get("title")
                        .and_then(Value::as_str)
                        .or_else(|| doc.and_then(|d| d.get("title").and_then(Value::as_str)))
                        .map(str::to_owned)
                        .unwrap_or_else(|| labels::humanize(id));
                    let rebindable = shortcut
                        .get("rebindable")
                        .and_then(Value::as_bool)
                        .or_else(|| doc.and_then(|d| d.get("rebindable").and_then(Value::as_bool)))
                        .unwrap_or(false);
                    arcade.push(ArcadeShortcut {
                        app: app.into(),
                        name: name.into(),
                        id: id.into(),
                        title,
                        display: keys
                            .iter()
                            .map(|k| keys::display(k, inputs.os.as_str()))
                            .collect(),
                        keys,
                        rebindable,
                    });
                }
            }
            // Installed manifests supply a name even when the static docs omit it.
            if let Some(sheet) = apps
                .iter_mut()
                .find(|a| a.source == "arcade" && a.id == app)
            {
                sheet.name = name.into();
                if sheet.matches.is_empty() {
                    sheet.matches = inferred_match(&value, app, inputs.os);
                }
            }
        }
    }
    for app in apps
        .iter_mut()
        .filter(|a| a.source == "arcade" && a.matches.is_empty())
    {
        app.matches = inferred_match(&Value::Null, &app.id, inputs.os);
    }
    let report = inputs.hyprland.as_ref();
    let binds = report.map(|r| r.binds.as_slice()).unwrap_or_default();
    let groups = wm_groups(binds, inputs.os, &user_labels, &mut warnings);
    if let Some(report) = report {
        warnings.extend(report.warnings.iter().cloned());
    }
    let wm = WmIndex {
        name: if report.is_some() || inputs.os == Os::Linux {
            "hyprland"
        } else {
            "none"
        }
        .into(),
        config_kind: report.map(|r| r.config_kind).unwrap_or_default(),
        groups,
        warnings,
    };
    let conflicts = conflicts(&arcade, &wm);
    Index {
        schema: 1,
        built_at: inputs
            .built_at
            .clone()
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        os: inputs.os,
        apps,
        arcade,
        wm,
        conflicts,
    }
}

fn read_json(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    let result = std::fs::File::open(path)
        .and_then(|file| {
            let mut bytes = Vec::new();
            file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("JSON source size limit exceeded".into());
            }
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())
        });
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

fn json_files(dir: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warnings.push(format!("{}: {e}", dir.display()));
            return Vec::new();
        }
    };
    let mut files = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn effective_keys(
    value: Option<&Value>,
    os: Os,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let value = if let Some(object) = value.as_object() {
        object
            .get(os.as_str())
            .or_else(|| object.get("default"))
            .unwrap_or(&Value::Null)
    } else {
        value
    };
    let alternatives = match value {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let mut result = Vec::new();
    for text in alternatives {
        if text.trim().is_empty() {
            continue;
        }
        match keys::normalize(text) {
            Some(chord) if !result.contains(&chord) => result.push(chord),
            Some(_) => {}
            None => warnings.push(format!(
                "{}: unsupported accelerator {text:?}",
                path.display()
            )),
        }
    }
    result
}

fn sheet(
    value: &Value,
    source: &str,
    os: Os,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<AppSheet> {
    let Some(id) = value
        .get(if source == "arcade" { "app" } else { "id" })
        .and_then(Value::as_str)
    else {
        warnings.push(format!("{}: shortcut sheet has no app/id", path.display()));
        return None;
    };
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            id.strip_prefix("arcade.")
                .map(|app| format!("Arcade {}", labels::humanize(app)))
                .unwrap_or_else(|| id.into())
        });
    let mut matches = BTreeMap::new();
    if let Some(m) = value.get("match") {
        if let Some(list) = m
            .get(os.as_str())
            .unwrap_or(m)
            .get(os.match_key())
            .and_then(Value::as_array)
        {
            matches.insert(
                os.match_key().into(),
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            );
        }
    }
    let mut groups = Vec::new();
    if let Some(gs) = value.get("groups").and_then(Value::as_array) {
        for group in gs {
            let mut shortcuts = Vec::new();
            if let Some(ss) = group.get("shortcuts").and_then(Value::as_array) {
                for shortcut in ss {
                    let Some(id) = shortcut.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let keys = effective_keys(shortcut.get("keys"), os, path, warnings);
                    if keys.is_empty() {
                        continue;
                    }
                    shortcuts.push(AppShortcut {
                        id: id.into(),
                        title: shortcut
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or(id)
                            .into(),
                        display: keys.iter().map(|k| keys::display(k, os.as_str())).collect(),
                        keys,
                        description: shortcut
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    });
                }
            }
            if !shortcuts.is_empty() {
                groups.push(AppGroup {
                    title: group
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Other")
                        .into(),
                    context: group
                        .get("context")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    shortcuts,
                });
            }
        }
    }
    Some(AppSheet {
        id: id.into(),
        name,
        source: source.into(),
        matches,
        groups,
    })
}

fn doc_shortcut<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.get("groups")?
        .as_array()?
        .iter()
        .filter(|g| g.get("context").and_then(Value::as_str) == Some("global"))
        .filter_map(|g| g.get("shortcuts").and_then(Value::as_array))
        .flatten()
        .find(|s| s.get("id").and_then(Value::as_str) == Some(id))
}

fn inferred_match(manifest: &Value, id: &str, os: Os) -> BTreeMap<String, Vec<String>> {
    let values = match os {
        Os::Linux => vec![id.to_owned(), id.replace('.', "-")],
        Os::Windows => vec![manifest
            .get("executable")
            .and_then(Value::as_str)
            .and_then(|e| e.rsplit(['/', '\\']).next())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}.exe", id.replace('.', "-")))],
        Os::Macos => vec![manifest
            .get("bundle")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .into()],
    };
    BTreeMap::from([(os.match_key().into(), values)])
}

fn wm_groups(
    binds: &[WmBind],
    os: Os,
    user_labels: &labels::UserLabels,
    warnings: &mut Vec<String>,
) -> Vec<WmGroup> {
    let mut groups: Vec<WmGroup> = Vec::new();
    let mut other = Vec::new();
    let mut media: Vec<&WmBind> = Vec::new();
    let home = read::home();
    for bind in binds {
        let Some(chord) = &bind.chord else {
            warnings.push(format!("Cannot display WM chord: {}", bind.key_text));
            continue;
        };
        if chord.ends_with("MediaPlayPause") {
            if media.iter().any(|b| {
                b.chord == bind.chord
                    && b.action == bind.action
                    && b.submap == bind.submap
                    && b.state == bind.state
                    && b.flags == bind.flags
            }) {
                continue;
            }
            media.push(bind);
        }
        let label = labels::user_label(user_labels, bind.submap.as_deref(), Some(chord))
            .map(str::to_owned)
            .unwrap_or_else(|| bind.label.clone());
        let detail = if label != bind.label {
            match labels::dispatcher_label(&bind.action) {
                Some(detail) => (detail != label).then_some(detail),
                None => bind.detail.clone().filter(|s| s != &label),
            }
        } else {
            bind.detail.clone()
        };
        let origin = bind.origin.clone().map(|mut origin| {
            if let Some(home) = &home {
                if let Ok(tail) = origin.file.strip_prefix(home) {
                    origin.file = Path::new("~").join(tail);
                }
            }
            origin
        });
        let shortcut = WmShortcut {
            chord: chord.clone(),
            display: keys::display(chord, os.as_str()),
            label,
            detail,
            submap: bind.submap.clone(),
            state: bind.state,
            origin,
        };
        if let Some(category) = bind
            .category
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "Other")
        {
            if let Some(group) = groups.iter_mut().find(|g| g.title == category) {
                group.shortcuts.push(shortcut);
            } else {
                groups.push(WmGroup {
                    title: category.into(),
                    shortcuts: vec![shortcut],
                });
            }
        } else {
            other.push(shortcut);
        }
    }
    if !other.is_empty() {
        groups.push(WmGroup {
            title: "Other".into(),
            shortcuts: other,
        });
    }
    groups
}

fn conflicts(arcade: &[ArcadeShortcut], wm: &WmIndex) -> Vec<Conflict> {
    let party = |a: &ArcadeShortcut| ConflictParty {
        kind: "arcade".into(),
        owner: a.app.clone(),
        label: a.title.clone(),
    };
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |chord: &str, a: ConflictParty, b: ConflictParty| {
        let key = (
            chord.to_owned(),
            a.owner.clone(),
            a.label.clone(),
            b.owner.clone(),
            b.label.clone(),
        );
        if seen.insert(key) {
            result.push(Conflict {
                chord: chord.into(),
                a,
                b,
            });
        }
    };
    for (i, a) in arcade.iter().enumerate() {
        for b in &arcade[i + 1..] {
            if a.app == b.app && a.id == b.id {
                continue;
            }
            for ka in &a.keys {
                for kb in &b.keys {
                    if keys::conflicts(ka, kb) {
                        add(
                            if ka.len() <= kb.len() { ka } else { kb },
                            party(a),
                            party(b),
                        );
                    }
                }
            }
        }
        for bind in wm.groups.iter().flat_map(|g| &g.shortcuts) {
            // Arcade globals have no submap, so they compete in every submap.
            for key in &a.keys {
                if keys::conflicts(key, &bind.chord) {
                    add(
                        if key.len() <= bind.chord.len() {
                            key
                        } else {
                            &bind.chord
                        },
                        party(a),
                        ConflictParty {
                            kind: "wm".into(),
                            owner: wm.name.clone(),
                            label: bind.label.clone(),
                        },
                    );
                }
            }
        }
    }
    result
}

impl Index {
    /// Same-directory temp file + flush + atomic replace. Interrupted writes
    /// leave the last complete index intact; no startup configuration is edited.
    pub fn write_index(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        {
            // JSON emits many small writes; buffering keeps publication cheap.
            let mut writer = BufWriter::new(temp.as_file_mut());
            serde_json::to_writer(&mut writer, self).map_err(io::Error::other)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

pub fn default_index_path() -> io::Result<PathBuf> {
    let arcade_home = std::env::var_os("ARCADE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let xdg_cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let platform_cache = directories::BaseDirs::new().map(|d| d.cache_dir().to_owned());
    cache_path(
        arcade_home.as_deref(),
        xdg_cache.as_deref(),
        platform_cache.as_deref(),
        Os::current(),
    )
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user cache directory"))
}

fn cache_path(
    arcade_home: Option<&Path>,
    xdg: Option<&Path>,
    platform: Option<&Path>,
    os: Os,
) -> Option<PathBuf> {
    let cache = if let Some(home) = arcade_home {
        home.join("cache")
    } else if os == Os::Linux {
        xdg.or(platform)?.to_owned()
    } else {
        platform?.to_owned()
    };
    Some(cache.join("arcade/tools/shortcuts-index.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn effective_os_keys_alternatives_and_null() {
        let value = json!({"default":["Ctrl+T", "Alt+T"], "macos":"Cmd+T", "windows":null});
        let mut warnings = Vec::new();
        assert_eq!(
            effective_keys(Some(&value), Os::Linux, Path::new("fixture"), &mut warnings),
            ["Ctrl+T", "Alt+T"]
        );
        assert_eq!(
            effective_keys(Some(&value), Os::Macos, Path::new("fixture"), &mut warnings),
            ["Super+T"]
        );
        assert!(effective_keys(
            Some(&value),
            Os::Windows,
            Path::new("fixture"),
            &mut warnings
        )
        .is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn atomic_round_trip_and_cache_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache/shortcuts-index.json");
        let mut index = build_index(&BuildInputs::default());
        index.write_index(&path).unwrap();
        index.built_at = "2026-10-11T12:00:00Z".into();
        index.write_index(&path).unwrap();
        let loaded: Index = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(index, loaded);
        assert_eq!(
            cache_path(
                Some(Path::new("arcade")),
                Some(Path::new("xdg")),
                Some(Path::new("platform")),
                Os::Linux
            )
            .unwrap(),
            Path::new("arcade/cache/arcade/tools/shortcuts-index.json")
        );
        assert_eq!(
            cache_path(
                None,
                Some(Path::new("xdg")),
                Some(Path::new("platform")),
                Os::Macos
            )
            .unwrap(),
            Path::new("platform/arcade/tools/shortcuts-index.json")
        );
    }
}
