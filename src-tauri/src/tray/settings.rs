use arcade_link::manifest::ids;
use arcade_tools_core::{paths::Paths, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub host_icons: bool,
    pub excluded: Vec<String>,
    pub show_update_badge: bool,
    pub start_at_login: bool,
    pub shortcuts_enabled: bool,
    pub shortcut: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            host_icons: true,
            excluded: vec![],
            show_update_badge: true,
            start_at_login: true,
            shortcuts_enabled: true,
            shortcut: arcade_tools_overlay::registration::default_accelerator().into(),
        }
    }
}

impl Preferences {
    pub fn load(paths: &Paths) -> Result<Self> {
        match std::fs::read(paths.state().join("tray-preferences.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
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
        arcade_link::paths::write_atomic(&path, &serde_json::to_vec_pretty(self)?, true)?;
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
