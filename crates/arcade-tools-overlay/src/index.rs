//! Reader for the pinned, prebuilt schema-1 shortcut index. No source evaluation.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const MISSING_MESSAGE: &str = "Open Arcade Tools once to build the shortcut index";
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub schema: u32,
    pub built_at: String,
    pub os: String,
    pub apps: Vec<App>,
    pub arcade: Vec<ArcadeShortcut>,
    pub wm: Option<WindowManager>,
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(rename = "match")]
    pub matches: AppMatch,
    pub groups: Vec<Group>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AppMatch {
    #[serde(default)]
    pub class: Vec<String>,
    #[serde(default)]
    pub exe: Vec<String>,
    #[serde(default)]
    pub bundle: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Group {
    pub title: String,
    #[serde(default)]
    pub context: Option<String>,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Deserialize)]
pub struct Shortcut {
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub display: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArcadeShortcut {
    pub app: String,
    pub name: String,
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub display: Vec<String>,
    pub rebindable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowManager {
    pub name: String,
    pub config_kind: Option<String>,
    pub groups: Vec<WmGroup>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct WmGroup {
    pub title: String,
    pub shortcuts: Vec<WmShortcut>,
}

#[derive(Debug, Deserialize)]
pub struct WmShortcut {
    pub chord: String,
    pub display: String,
    pub label: String,
    pub detail: Option<String>,
    pub submap: Option<String>,
    pub state: String,
    pub origin: Option<Origin>,
}

#[derive(Debug, Deserialize)]
pub struct Origin {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Deserialize)]
pub struct Conflict {
    pub chord: String,
    pub a: ConflictSide,
    pub b: ConflictSide,
}

#[derive(Debug, Deserialize)]
pub struct ConflictSide {
    pub kind: String,
    pub owner: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy)]
pub enum MatchKind {
    Class,
    Exe,
    Bundle,
}

impl Index {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let index: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if index.schema != 1 {
            return Err(format!(
                "unsupported shortcut index schema {}",
                index.schema
            ));
        }
        Ok(index)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_INDEX_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_INDEX_BYTES {
            return Err("shortcut index is too large".into());
        }
        Self::parse(&bytes)
    }

    pub fn match_app(&self, identity: &str, kind: MatchKind) -> Option<usize> {
        let identity = match kind {
            MatchKind::Exe => identity.rsplit(['/', '\\']).next().unwrap_or(identity),
            _ => identity,
        };
        self.apps.iter().position(|app| {
            let list = match kind {
                MatchKind::Class => &app.matches.class,
                MatchKind::Exe => &app.matches.exe,
                MatchKind::Bundle => &app.matches.bundle,
            };
            list.iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(identity))
        })
    }

    pub fn has_conflict(&self, kind: &str, owner: &str, label: &str, keys: &[String]) -> bool {
        self.conflicts.iter().any(|conflict| {
            keys.iter()
                .any(|key| normalized_chord(key) == normalized_chord(&conflict.chord))
                && [&conflict.a, &conflict.b].iter().any(|side| {
                    side.kind.eq_ignore_ascii_case(kind)
                        && side.owner.eq_ignore_ascii_case(owner)
                        && side.label.eq_ignore_ascii_case(label)
                })
        })
    }
}

fn normalized_chord(chord: &str) -> String {
    let mut parts: Vec<_> = chord
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    parts.sort_unstable();
    parts.join("+")
}

pub fn default_path() -> io::Result<PathBuf> {
    if let Some(root) = std::env::var_os("ARCADE_HOME") {
        return Ok(PathBuf::from(root).join("cache/arcade/tools/shortcuts-index.json"));
    }
    #[cfg(target_os = "linux")]
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache).join("arcade/tools/shortcuts-index.json"));
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.cache_dir().join("arcade/tools/shortcuts-index.json"))
        .ok_or_else(|| io::Error::other("cannot locate the user cache directory"))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/shortcuts-index.json");

    #[test]
    fn pinned_index_and_unknown_fields() {
        let index = Index::parse(FIXTURE).unwrap();
        assert_eq!(index.schema, 1);
        assert_eq!(
            index
                .wm
                .unwrap()
                .groups
                .iter()
                .map(|g| g.shortcuts.len())
                .sum::<usize>(),
            40
        );
        let mut value: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        value["future"] = serde_json::json!({"feature": true});
        value["apps"][0]["future"] = serde_json::json!(123);
        value["wm"] = serde_json::Value::Null;
        assert!(Index::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    }

    #[test]
    fn corrupt_and_incompatible_indexes() {
        for bytes in [b"{".as_slice(), b"null", b"{}"] {
            assert!(Index::parse(bytes).is_err());
        }
        let mut value: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        value["schema"] = serde_json::json!(2);
        assert!(Index::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn identities_include_flatpak_arcade_and_platform_names() {
        let index = Index::parse(FIXTURE).unwrap();
        assert_eq!(
            index.match_app("ORG.MOZILLA.FIREFOX", MatchKind::Class),
            Some(0)
        );
        assert_eq!(
            index.match_app("C:\\Program Files\\Firefox\\FIREFOX.EXE", MatchKind::Exe),
            Some(0)
        );
        assert_eq!(
            index.match_app("org.mozilla.firefox", MatchKind::Bundle),
            Some(0)
        );
        assert_eq!(index.match_app("Arcade.Find", MatchKind::Class), Some(2));
        assert_eq!(index.match_app("unknown", MatchKind::Class), None);
    }

    #[test]
    fn conflicts_are_scoped_to_both_involved_entries() {
        let index = Index::parse(FIXTURE).unwrap();
        assert!(index.has_conflict(
            "arcade",
            "arcade.clipboard",
            "Show clipboard",
            &["Alt+Ctrl+V".into()]
        ));
        assert!(!index.has_conflict("app", "firefox", "Paste", &["Ctrl+Alt+V".into()]));
    }
}
