use crate::apps::app_name;
use crate::release::Os;
use crate::{Error, Result};
use arcade_link::accelerator::Platform;
use arcade_link::manifest::ids;
use arcade_link::paths::{self as link_paths, AppDirs, Locations, Migration, UserDirs};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub link: Locations,
    pub isolated: bool,
}
impl Paths {
    /// Environment/platform discovery on a worker thread. ARCADE_HOME replaces
    /// every root for isolation; no environment/startup files are changed.
    pub fn discover() -> Result<Self> {
        if let Some(root) = std::env::var_os("ARCADE_HOME").filter(|v| !v.is_empty()) {
            let root = PathBuf::from(root);
            if !root.is_absolute() {
                return Err(Error::new("paths", "ARCADE_HOME must be absolute."));
            }
            return Ok(Self::under(&root));
        }
        let base = directories::BaseDirs::new()
            .ok_or_else(|| Error::new("paths", "A home directory is required."))?;
        let home = base.home_dir().to_path_buf();
        Ok(Self {
            config: base.config_dir().into(),
            data: base.data_dir().into(),
            cache: base.cache_dir().into(),
            local: std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Local")),
            roaming: std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Roaming")),
            home,
            link: Locations::discover(),
            isolated: false,
        })
    }
    /// Pure isolated path calculation, safe on any thread.
    pub fn under(root: &Path) -> Self {
        Self {
            // Match Link's install::Environment::under exactly, including the
            // receipt store. Child --install processes inherit this one root.
            home: root.into(),
            config: root.join(".config"),
            data: root.join(".local/share"),
            cache: root.join(".cache"),
            local: root.join("AppData/Local"),
            roaming: root.join("AppData/Roaming"),
            link: Locations::under(root),
            isolated: true,
        }
    }
    /// These roots as Link's user folders, for the shared `arcade/<app>` layout.
    pub fn user_dirs(&self) -> UserDirs {
        UserDirs {
            home: self.home.clone(),
            config: self.config.clone(),
            data: self.data.clone(),
            cache: self.cache.clone(),
            roaming: self.roaming.clone(),
            local: self.local.clone(),
        }
    }
    /// An app's config, data and cache folders (`arcade/<app>`, STANDARD §10).
    pub fn app_dirs(&self, id: &str, os: Os) -> AppDirs {
        link_paths::app_dirs_in(&self.user_dirs(), platform(os), id)
    }
    /// Moves Tools' own folders from their 0.3 locations (`arcade-tools`,
    /// `app.arcadetools`) into `arcade/tools`. Safe to run on every start.
    pub fn migrate_own_folders(&self) -> Migration {
        link_paths::migrate_app_dirs_in(&self.user_dirs(), platform(Os::current()), ids::TOOLS)
    }
    fn own(&self) -> AppDirs {
        self.app_dirs(ids::TOOLS, Os::current())
    }
    /// Tools' state: preferences, receipts' history, tray settings.
    pub fn state(&self) -> PathBuf {
        self.own().data
    }
    pub fn downloads(&self) -> PathBuf {
        self.own().cache.join("downloads")
    }
    /// Tools' log (stderr when nothing else receives it, via Link's logs).
    pub fn logs(&self) -> PathBuf {
        self.state().join("logs")
    }
    /// Path calculation only; no I/O, safe on any thread.
    pub fn catalog(&self) -> PathBuf {
        self.own().cache.join("catalog")
    }
    /// Path calculation only; no I/O, safe on any thread.
    pub fn docs(&self) -> PathBuf {
        self.own().cache.join("docs")
    }
    /// The Tools window's webview storage (Linux and Windows; macOS keeps
    /// WebKit's own per-bundle folders).
    pub fn webview(&self) -> PathBuf {
        self.own().cache.join("webview")
    }
    pub fn install_root(&self, id: &str, os: Os, tarball: bool) -> PathBuf {
        match os {
            Os::Linux if tarball => self.data.join(if id == crate::apps::SHELF {
                "arcade-shelf"
            } else {
                "arcade-clipboard"
            }),
            Os::Linux => self.home.join("Applications/Arcade"),
            Os::Windows => self.local.join("Programs").join(app_name(id)),
            Os::Macos => self
                .home
                .join("Applications")
                .join(format!("{}.app", app_name(id))),
        }
    }
    pub fn executable(&self, id: &str, os: Os, tarball: bool) -> PathBuf {
        let root = self.install_root(id, os, tarball);
        let slug = id.replace('.', "-");
        match os {
            Os::Linux if tarball => root.join(if id == crate::apps::SHELF {
                "arcade-shelf"
            } else {
                "clipboard"
            }),
            Os::Linux => root.join(format!("{}.AppImage", app_name(id).replace(' ', "-"))),
            Os::Windows => root.join(format!(
                "{}.exe",
                if id == ids::BOX {
                    "arcade-desktop"
                } else if id == ids::CLIPBOARD {
                    "clipboard"
                } else {
                    &slug
                }
            )),
            Os::Macos => root.join("Contents/MacOS").join(if id == ids::BOX {
                "arcade-desktop"
            } else if id == ids::CLIPBOARD {
                "Arcade Clipboard"
            } else {
                &slug
            }),
        }
    }
    /// Every folder an app's settings and data may be in: its 0.3 locations,
    /// the shared `arcade/<app>` folders (STANDARD §10) and every older location
    /// Link knows, so backup, diagnostics and "remove data" find them whether
    /// or not the app has moved yet. Not every folder exists.
    pub fn data_folders(&self, id: &str, os: Os) -> Vec<PathBuf> {
        let mut folders = self.known_data_folders(id, os);
        if !ids::APPS.contains(&id) && id != ids::TOOLS {
            return folders;
        }
        let dirs = self.app_dirs(id, os);
        let legacy = link_paths::legacy_dirs_in(&self.user_dirs(), platform(os), id);
        for folder in [dirs.config, dirs.data, dirs.cache]
            .into_iter()
            .chain(legacy.into_iter().map(|dir| dir.path))
        {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }
    fn known_data_folders(&self, id: &str, os: Os) -> Vec<PathBuf> {
        match os {
            Os::Linux => match id {
                ids::BOX => vec![
                    self.data.join("dev.arcadebox.app"),
                    self.data.join("arcadebox"),
                ],
                ids::LENS => vec![self.config.join("arcadelens"), self.data.join("arcadelens")],
                // Look's settings, and its WebView storage and cache (Tauri
                // keeps those under the app identifier).
                ids::LOOK => vec![
                    self.config.join("arcade-look"),
                    self.data.join("app.arcadelook"),
                    self.cache.join("app.arcadelook"),
                ],
                ids::WHEEL => vec![self.config.join("Arcade Wheel/Arcade Wheel")],
                crate::apps::SHELF => vec![self.data.join("qa-p1/ArcadeShelf")],
                crate::apps::FIND => vec![
                    self.config.join("arcade-find"),
                    self.data.join("arcade-find"),
                ],
                ids::CLIPBOARD => vec![
                    self.data.join("dev.arcade.clipboard"),
                    self.data.join("clipboard"),
                ],
                ids::TOOLS => vec![],
                _ => vec![],
            },
            Os::Windows => match id {
                ids::BOX => vec![
                    self.roaming.join("dev.arcadebox.app"),
                    self.roaming.join("Arcade Box/Arcade Box/data"),
                ],
                ids::LENS => vec![
                    self.roaming.join("Arcade/Arcade Lens/config"),
                    self.roaming.join("Arcade/Arcade Lens/data"),
                ],
                ids::LOOK => vec![
                    self.roaming.join("arcade-look"),
                    self.local.join("app.arcadelook"),
                ],
                ids::WHEEL => vec![self.local.join("Arcade Wheel/Arcade Wheel")],
                crate::apps::SHELF => vec![self.local.join("qa-p1/ArcadeShelf")],
                crate::apps::FIND => vec![
                    self.roaming.join("Arcade/Arcade Find"),
                    self.local.join("Arcade/Arcade Find"),
                ],
                ids::CLIPBOARD => vec![self.roaming.join("dev.arcade/clipboard")],
                ids::TOOLS => vec![],
                _ => vec![],
            },
            Os::Macos => {
                let support = self.home.join("Library/Application Support");
                match id {
                    ids::BOX => vec![
                        support.join("dev.arcadebox.app"),
                        support.join("dev.Arcade-Box.Arcade-Box"),
                    ],
                    ids::LENS => vec![support.join("dev.Arcade.Arcade-Lens")],
                    ids::LOOK => vec![
                        support.join("arcade-look"),
                        self.home.join("Library/WebKit/app.arcadelook"),
                        self.home.join("Library/Caches/app.arcadelook"),
                    ],
                    ids::WHEEL => vec![self
                        .home
                        .join("Library/Preferences/Arcade Wheel/Arcade Wheel")],
                    crate::apps::SHELF => vec![support.join("qa-p1/ArcadeShelf")],
                    crate::apps::FIND => vec![support.join("Arcade Find")],
                    ids::CLIPBOARD => vec![support.join("dev.arcade.clipboard")],
                    ids::TOOLS => vec![],
                    _ => vec![],
                }
            }
        }
    }
    pub fn autostart(&self, id: &str, os: Os) -> PathBuf {
        match os {
            Os::Linux => self.config.join("autostart").join(format!(
                "{}.desktop",
                if id == crate::apps::SHELF {
                    id.to_string()
                } else {
                    id.replace('.', "-")
                }
            )),
            Os::Macos => self.home.join("Library/LaunchAgents").join(format!(
                "{}.plist",
                match id {
                    ids::LOOK => "app.arcadelook",
                    ids::LENS => "dev.arcade.lens",
                    _ => id,
                }
            )),
            // The shared installers' Startup task creates `<Name>.lnk`. Tools'
            // own and Shelf's transition entries are VBScript launchers.
            Os::Windows => self
                .roaming
                .join("Microsoft/Windows/Start Menu/Programs/Startup")
                .join(format!(
                    "{}.{}",
                    app_name(id),
                    if id == crate::apps::SHELF {
                        "vbs"
                    } else {
                        "lnk"
                    }
                )),
        }
    }
    /// Filesystem validation on a worker thread, including symlink resolution.
    pub fn check_persistent_executable(&self, executable: &Path) -> Result<()> {
        if !executable.is_absolute() || !executable.is_file() {
            return Err(Error::new("paths", "The installed executable is missing."));
        }
        let path = executable.canonicalize()?;
        // Compare canonical with canonical: macOS's temp dir is a /var symlink
        // into /private/var, and Windows canonical paths carry a \\?\ prefix.
        let temp = std::env::temp_dir();
        let temp = temp.canonicalize().unwrap_or(temp);
        if !self.isolated
            && (path.starts_with(&temp)
                || path.starts_with("/tmp")
                || path.starts_with("/var/tmp")
                || path.starts_with("/run")
                || path.starts_with("/Volumes")
                || path.to_string_lossy().contains("AppTranslocation"))
        {
            return Err(Error::new(
                "paths",
                "Move the app to a permanent install location before enabling start at login.",
            ));
        }
        Ok(())
    }
    /// Filesystem/path validation on a worker thread; performs no mutation.
    pub fn guard(&self, path: &Path) -> Result<()> {
        // No symlinked parents, path traversal or deletion outside these per-user roots.
        let allowed_root = [
            &self.home,
            &self.config,
            &self.data,
            &self.local,
            &self.roaming,
            &self.cache,
            &self.link.registry,
        ]
        .iter()
        .any(|r| path.starts_with(r) && path != **r)
            || (!self.isolated && cfg!(target_os = "macos") && system_bundle_path(path));
        if !path.is_absolute()
            || path.components().any(|c| matches!(c, Component::ParentDir))
            || !allowed_root
        {
            return Err(Error::new(
                "paths",
                format!("Refusing to modify {}", path.display()),
            ));
        }
        for p in path.ancestors() {
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.file_type().is_symlink() && !system_alias(p) => {
                    return Err(Error::new(
                        "paths",
                        format!("Refusing symlinked install or data path {}", p.display()),
                    ))
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Validate a receipt/registry path on a worker. These private stores are
    /// siblings, not app data; only their own children may be modified.
    pub fn guard_receipt(&self, id: &str) -> Result<PathBuf> {
        let path = arcade_link::receipt::Store::new(&self.link).path(id)?;
        for parent in path.ancestors() {
            if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink())
                && !system_alias(parent)
            {
                return Err(Error::new("paths", "Receipt paths must not be symlinked."));
            }
        }
        Ok(path)
    }
}

fn system_bundle_path(path: &Path) -> bool {
    crate::apps::APPS
        .iter()
        .copied()
        .chain([ids::TOOLS])
        .any(|id| {
            path.starts_with(Path::new("/Applications").join(format!("{}.app", app_name(id))))
        })
}

pub fn app_id(id: &str) -> Result<()> {
    if !crate::apps::APPS.contains(&id) {
        return Err(Error::new("unsupported_input", "Unknown Arcade app."));
    }
    Ok(())
}

pub(crate) fn system_alias(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::fs::read_link(path).is_ok_and(|link| macos_alias_target(path, &link))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_alias_target(path: &Path, link: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let target = parent.join(link);
    [
        ("/var", "/private/var"),
        ("/tmp", "/private/tmp"),
        ("/etc", "/private/etc"),
    ]
    .iter()
    .any(|(alias, expected)| path == Path::new(alias) && target == Path::new(expected))
}

/// Link's name for an OS.
fn platform(os: Os) -> Platform {
    match os {
        Os::Linux => Platform::Linux,
        Os::Windows => Platform::Windows,
        Os::Macos => Platform::Macos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn system_applications_exception_is_limited_to_known_arcade_bundles() {
        assert!(system_bundle_path(Path::new(
            "/Applications/Arcade Look.app"
        )));
        assert!(system_bundle_path(Path::new(
            "/Applications/Arcade Tools.app/Contents/MacOS/arcade-tools"
        )));
        for path in [
            "/Applications",
            "/Applications/Other.app",
            "/Applications/Arcade Look.app-old",
            "/Applications/Other/Arcade Look.app",
        ] {
            assert!(!system_bundle_path(Path::new(path)), "{path}");
        }
        let root = tempfile::tempdir().unwrap();
        let mut paths = Paths::under(root.path());
        assert!(paths
            .guard(Path::new("/Applications/Arcade Look.app"))
            .is_err());
        paths.isolated = false;
        assert_eq!(
            paths
                .guard(Path::new("/Applications/Arcade Look.app"))
                .is_ok(),
            cfg!(target_os = "macos")
        );
        assert!(paths
            .guard(Path::new("/Applications/Arcade Look.app/../Other.app"))
            .is_err());
        assert!(paths
            .guard(&paths.home.join("Applications/Arcade Look.app"))
            .is_ok());
    }

    #[test]
    fn macos_system_aliases_accept_relative_and_absolute_targets_only() {
        for name in ["var", "tmp", "etc"] {
            let alias = PathBuf::from(format!("/{name}"));
            let relative = PathBuf::from(format!("private/{name}"));
            let absolute = PathBuf::from(format!("/private/{name}"));
            assert!(macos_alias_target(&alias, &relative));
            assert!(macos_alias_target(&alias, &absolute));
            assert!(!macos_alias_target(&alias, Path::new("/home/user/data")));
            assert!(!macos_alias_target(&alias, Path::new("private/other")));
            assert!(!macos_alias_target(
                &Path::new("/home/user").join(name),
                &absolute
            ));
            assert!(!macos_alias_target(
                &Path::new("/home/user").join(name),
                &relative
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn user_symlinked_install_data_and_receipt_directories_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir(root.path().join("owned")).unwrap();
        let receipt = arcade_link::receipt::Store::new(&paths.link)
            .path(ids::TOOLS)
            .unwrap();
        for directory in [
            paths.home.join("Applications"),
            paths.data.clone(),
            receipt.parent().unwrap().to_owned(),
        ] {
            std::fs::create_dir_all(directory.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(root.path().join("owned"), &directory).unwrap();
            assert!(paths.guard(&directory.join("child")).is_err());
        }
        assert!(paths.guard_receipt(ids::TOOLS).is_err());
    }
}
