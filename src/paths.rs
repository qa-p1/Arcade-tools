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
    pub fn discover() -> Result<Self> {
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
    pub fn under(root: &Path) -> Self {
        Self {
            home: root.join("home"),
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            local: root.join("local"),
            roaming: root.join("roaming"),
            link: Locations::under(&root.join("arcade")),
            isolated: true,
        }
    }
    pub fn state(&self) -> PathBuf {
        self.data.join("arcade-tools")
    }
    pub fn downloads(&self) -> PathBuf {
        self.cache.join("arcade-tools/downloads")
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
                Ok(m) if m.file_type().is_symlink() => {
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
}

pub fn app_id(id: &str) -> Result<()> {
    if !crate::apps::APPS.contains(&id) {
        return Err(Error::new("unsupported_input", "Unknown Arcade app."));
    }
    Ok(())
}
