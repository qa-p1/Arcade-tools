use arcade_link::manifest::ids;
use arcade_tools_core::{paths::Paths, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    /// Version of `tray-preferences.json`: 0.x wrote none (the same fields). A newer Tools may write a higher one,
    /// kept with the fields this version does not know (DECISIONS.md D9).
    pub schema: u32,
    pub host_icons: bool,
    pub excluded: Vec<String>,
    pub show_update_badge: bool,
    pub start_at_login: bool,
    pub shortcuts_enabled: bool,
    pub shortcut: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The `tray-preferences.json` schema this version writes.
pub const SCHEMA: u32 = 1;

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            host_icons: true,
            excluded: vec![],
            show_update_badge: true,
            start_at_login: true,
            shortcuts_enabled: true,
            shortcut: arcade_tools_overlay::registration::default_accelerator().into(),
            extra: serde_json::Map::new(),
        }
    }
}

impl Preferences {
    pub fn load(paths: &Paths) -> Result<Self> {
        match std::fs::read(paths.state().join("tray-preferences.json")) {
            Ok(bytes) => {
                let mut prefs: Self = serde_json::from_slice(&bytes)?;
                prefs.schema = prefs.schema.max(SCHEMA);
                Ok(prefs)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn normalize(&mut self, previous: &Self) -> Result<()> {
        self.shortcut = self.shortcut.trim().to_owned();
        arcade_tools_overlay::registration::parse(&self.shortcut)
            .map_err(|error| Error::new("shortcut", error))?;
        if self
            .excluded
            .iter()
            .any(|id| !ids::APPS.contains(&id.as_str()))
        {
            return Err(Error::new("tray_settings", "Unknown Arcade app exclusion."));
        }
        self.excluded.sort();
        self.excluded.dedup();
        if self.host_icons && !previous.host_icons {
            self.start_at_login = true;
        }
        Ok(())
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let path = paths.state().join("tray-preferences.json");
        paths.guard(&path)?;
        let prefs = Self {
            schema: self.schema.max(SCHEMA),
            ..self.clone()
        };
        arcade_link::paths::write_atomic(&path, &serde_json::to_vec_pretty(&prefs)?, true)?;
        Ok(())
    }

    pub fn apply_login(&self, paths: &Paths, executable: &Path) -> Result<()> {
        crate::resident::login::set(paths, executable, self.start_at_login)
    }
}

/// Development builds never add themselves to the real login environment.
pub fn login_available(paths: &Paths) -> bool {
    !cfg!(debug_assertions)
        && (paths.isolated
            || !arcade_link::install::is_dev_build(Path::new(
                &arcade_link::manifest::current_executable(),
            )))
        && paths
            .check_persistent_executable(Path::new(&arcade_link::manifest::current_executable()))
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D9: every released 0.x (0.2.0–0.4.6) wrote these six fields and no schema.
    #[test]
    fn released_tray_preferences_load_and_gain_a_schema() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir_all(paths.state()).unwrap();
        let file = paths.state().join("tray-preferences.json");
        std::fs::write(
            &file,
            br#"{"hostIcons":false,"excluded":["arcade.find"],"showUpdateBadge":false,"startAtLogin":false,"shortcutsEnabled":true,"shortcut":"Super+Shift+Slash"}"#,
        )
        .unwrap();
        let prefs = Preferences::load(&paths).unwrap();
        assert_eq!(prefs.schema, SCHEMA);
        assert!(!prefs.host_icons && !prefs.show_update_badge && !prefs.start_at_login);
        assert_eq!(prefs.excluded, [ids::FIND]);
        assert_eq!(prefs.shortcut, "Super+Shift+Slash");
        prefs.save(&paths).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(written["schema"], SCHEMA);
        assert_eq!(written["shortcut"], "Super+Shift+Slash");
    }

    /// D9 downgrade rule: a file from a newer Tools keeps its schema and the fields this version doesn't know.
    #[test]
    fn a_newer_tray_preferences_file_keeps_what_this_version_does_not_know() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir_all(paths.state()).unwrap();
        let file = paths.state().join("tray-preferences.json");
        std::fs::write(
            &file,
            br#"{"schema":7,"hostIcons":true,"futureOption":{"a":1}}"#,
        )
        .unwrap();
        let mut prefs = Preferences::load(&paths).unwrap();
        prefs.show_update_badge = false;
        prefs.save(&paths).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(written["schema"], 7);
        assert_eq!(written["futureOption"], serde_json::json!({"a": 1}));
        assert_eq!(written["showUpdateBadge"], false);
    }

    #[test]
    fn hosting_enables_login_once_and_preferences_survive_core_changes() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let old = Preferences {
            host_icons: false,
            start_at_login: false,
            ..Default::default()
        };
        let mut prefs = Preferences {
            excluded: vec![ids::FIND.into(), ids::FIND.into()],
            ..old.clone()
        };
        prefs.host_icons = true;
        prefs.normalize(&old).unwrap();
        assert!(prefs.start_at_login);
        assert_eq!(prefs.excluded, [ids::FIND]);
        prefs.save(&paths).unwrap();
        std::fs::write(
            paths.state().join("preferences.json"),
            b"{\"linkEnabled\":false}",
        )
        .unwrap();
        assert_eq!(Preferences::load(&paths).unwrap(), prefs);
        let old = prefs.clone();
        prefs.start_at_login = false;
        prefs.normalize(&old).unwrap();
        assert!(!prefs.start_at_login);
        prefs.excluded.push("arcade.unknown".into());
        assert!(prefs.normalize(&old).is_err());
    }

    #[test]
    fn missing_shortcut_migrates_and_invalid_input_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir_all(paths.state()).unwrap();
        std::fs::write(
            paths.state().join("tray-preferences.json"),
            br#"{"hostIcons":false,"shortcutsEnabled":true}"#,
        )
        .unwrap();
        let mut prefs = Preferences::load(&paths).unwrap();
        assert_eq!(
            prefs.shortcut,
            arcade_tools_overlay::registration::default_accelerator()
        );
        let old = prefs.clone();
        prefs.shortcut = " Ctrl+Alt+/ ".into();
        prefs.normalize(&old).unwrap();
        assert_eq!(prefs.shortcut, "Ctrl+Alt+/");
        prefs.shortcut = "/".into();
        assert!(prefs.normalize(&old).is_err());
        assert_eq!(Preferences::load(&paths).unwrap(), old);
    }
}
