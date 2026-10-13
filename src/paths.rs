use crate::apps::app_name;
use crate::release::Os;
use crate::{Error, Result};
use arcade_link::manifest::ids;
use arcade_link::paths::Locations;
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
    pub fn state(&self) -> PathBuf {
        self.data.join("arcade-tools")
    }
    pub fn downloads(&self) -> PathBuf {
        self.cache.join("arcade-tools/downloads")
    }
    /// Path calculation only; no I/O, safe on any thread.
    pub fn catalog(&self) -> PathBuf {
        self.cache.join("arcade/tools/catalog")
    }
    /// Path calculation only; no I/O, safe on any thread.
    pub fn docs(&self) -> PathBuf {
        self.cache.join("arcade/tools/docs")
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
    pub fn data_folders(&self, id: &str, os: Os) -> Vec<PathBuf> {
        match os {
            Os::Linux => match id {
                ids::BOX => vec![
                    self.data.join("dev.arcadebox.app"),
                    self.data.join("arcadebox"),
                ],
                ids::LENS => vec![self.config.join("arcadelens"), self.data.join("arcadelens")],
                ids::LOOK => vec![self.config.join("arcade-look")],
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
                ids::TOOLS => vec![self.state()],
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
                ids::LOOK => vec![self.roaming.join("arcade-look")],
                ids::WHEEL => vec![self.local.join("Arcade Wheel/Arcade Wheel")],
                crate::apps::SHELF => vec![self.local.join("qa-p1/ArcadeShelf")],
                crate::apps::FIND => vec![
                    self.roaming.join("Arcade/Arcade Find"),
                    self.local.join("Arcade/Arcade Find"),
                ],
                ids::CLIPBOARD => vec![self.roaming.join("dev.arcade/clipboard")],
                ids::TOOLS => vec![self.state()],
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
                    ids::LOOK => vec![support.join("arcade-look")],
                    ids::WHEEL => vec![self
                        .home
                        .join("Library/Preferences/Arcade Wheel/Arcade Wheel")],
                    crate::apps::SHELF => vec![support.join("qa-p1/ArcadeShelf")],
                    crate::apps::FIND => vec![support.join("Arcade Find")],
                    ids::CLIPBOARD => vec![support.join("dev.arcade.clipboard")],
                    ids::TOOLS => vec![self.state()],
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
            Os::Windows => self
                .roaming
                .join("Microsoft/Windows/Start Menu/Programs/Startup")
                .join(format!(
                    "{}.{}",
                    app_name(id),
                    if id == crate::apps::SHELF {
                        "vbs"
                    } else {
                        "cmd"
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
        if !path.is_absolute()
            || path.components().any(|c| matches!(c, Component::ParentDir))
            || ![
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

#[cfg(test)]
mod tests {
    use super::*;

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
