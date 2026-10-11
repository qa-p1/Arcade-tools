//! Reader for the pinned, prebuilt schema-1 shortcut index. No source evaluation.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const MISSING_MESSAGE: &str = "Open Arcade Tools once to build the shortcut index";
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

/// Thin overlay behavior around the builder's shared schema types.
#[derive(Debug)]
pub struct Index(pub arcade_shortcuts::Index);

impl std::ops::Deref for Index {
    type Target = arcade_shortcuts::Index;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Index {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MatchKind {
    Class,
    Exe,
    Bundle,
}

impl Index {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let index: arcade_shortcuts::Index =
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if index.schema != 1 {
            return Err(format!(
                "unsupported shortcut index schema {}",
                index.schema
            ));
        }
        Ok(Self(index))
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
            let key = match kind {
                MatchKind::Class => "class",
                MatchKind::Exe => "exe",
                MatchKind::Bundle => "bundle",
            };
            app.matches.get(key).is_some_and(|list| {
                list.iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(identity))
            })
        })
    }

    pub fn has_conflict(&self, kind: &str, owner: &str, label: &str, keys: &[String]) -> bool {
        self.conflicts.iter().any(|conflict| {
            keys.iter().any(|key| {
                arcade_shortcuts::keys::conflicts(
                    &normalized_chord(key),
                    &normalized_chord(&conflict.chord),
                )
            }) && [&conflict.a, &conflict.b].iter().any(|side| {
                side.kind.eq_ignore_ascii_case(kind)
                    && side.owner.eq_ignore_ascii_case(owner)
                    && side.label.eq_ignore_ascii_case(label)
            })
        })
    }
}

fn normalized_chord(chord: &str) -> String {
    arcade_shortcuts::keys::normalize(chord).unwrap_or_else(|| chord.to_owned())
}

pub fn default_path() -> io::Result<PathBuf> {
    arcade_shortcuts::default_index_path()
}

/// A missing override file is normal. A bad one does not hide a valid index.
pub fn apply_saved_labels(index: &mut Index) {
    let result = arcade_shortcuts::labels::default_path()
        .map_err(|error| error.to_string())
        .and_then(|path| {
            if !path.exists() {
                return Ok(Default::default());
            }
            arcade_shortcuts::labels::load_user_labels(&path)
        });
    match result {
        Ok(labels) => index.apply_user_labels(&labels),
        Err(error) => eprintln!("arcade-tools-overlay: shortcut labels: {error}"),
    }
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

    #[test]
    fn shared_schema_overrides_global_and_submap_labels_without_losing_badges() {
        let mut index = Index::parse(FIXTURE).unwrap();
        let global = index
            .wm
            .groups
            .iter()
            .flat_map(|group| &group.shortcuts)
            .find(|bind| bind.chord == "Ctrl+Alt+V")
            .unwrap()
            .clone();
        let mut modal = global.clone();
        modal.submap = Some("resize".into());
        index.wm.groups[0].shortcuts.push(modal);
        index.apply_user_labels(&arcade_shortcuts::labels::UserLabels::from([
            ("|Ctrl+Alt+V".into(), "My audio".into()),
            ("resize|Ctrl+Alt+V".into(), "Resize audio".into()),
            ("|Super+T".into(), "  ".into()),
        ]));
        let binds = index
            .wm
            .groups
            .iter()
            .flat_map(|group| &group.shortcuts)
            .filter(|bind| bind.chord == "Ctrl+Alt+V")
            .collect::<Vec<_>>();
        assert_eq!(
            binds
                .iter()
                .find(|bind| bind.submap.is_none())
                .unwrap()
                .label,
            "My audio"
        );
        assert_eq!(
            binds
                .iter()
                .find(|bind| bind.submap.is_some())
                .unwrap()
                .label,
            "Resize audio"
        );
        assert!(index.has_conflict("wm", "hyprland", "My audio", &["Alt+Ctrl+V".into()]));
        assert_eq!(index.conflicts.len(), 3);
        assert!(index.has_conflict("wm", "hyprland", "Resize audio", &["Ctrl+Alt+V".into()]));
        let mut model = crate::model::Model::new(Some(index), Some(0));
        model.type_text("My audio");
        assert!(model
            .sections()
            .iter()
            .flat_map(|section| &section.groups)
            .flat_map(|group| &group.rows)
            .any(|row| row.title == "My audio" && row.conflict));
    }
}
