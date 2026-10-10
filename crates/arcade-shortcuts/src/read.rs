//! Bounded reads shared by both configuration readers.

use crate::ConfigBind;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ConfigReport {
    pub binds: Vec<ConfigBind>,
    pub warnings: Vec<String>,
    /// Sources already read by the parser, used for labels without more disk IO.
    pub sources: BTreeMap<PathBuf, String>,
}

/// Canonical roots, never an unrestricted filesystem fallback.
#[derive(Debug, Clone)]
pub struct ReadPolicy {
    roots: Vec<PathBuf>,
    pub max_file_bytes: usize,
}

impl ReadPolicy {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            roots: roots
                .into_iter()
                .filter_map(|p| p.canonicalize().ok())
                .collect(),
            max_file_bytes: 4 * 1024 * 1024,
        }
    }

    pub fn user_config() -> Self {
        Self::new(config_home())
    }

    pub fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        let path = path.canonicalize()?;
        if !self.roots.iter().any(|root| path.starts_with(root)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes sandbox read roots",
            ));
        }
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "sandbox reads regular files only",
            ));
        }
        Ok(path)
    }

    pub fn read(&self, path: &Path) -> io::Result<(PathBuf, Vec<u8>)> {
        let path = self.resolve(path)?;
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(self.max_file_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > self.max_file_bytes {
            return Err(io::Error::other("sandbox file size limit exceeded"));
        }
        Ok((path, bytes))
    }

    pub fn read_text(&self, path: &Path) -> io::Result<(PathBuf, String)> {
        let (path, bytes) = self.read(path)?;
        let text =
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok((path, text))
    }
}

pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()))
}

pub(crate) fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|p| p.join(".config")))
}

pub(crate) fn expand_home(text: &str, home: Option<&Path>) -> PathBuf {
    if let Some(tail) = text.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(tail);
        }
    }
    PathBuf::from(text)
}

pub(crate) fn submap(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "reset").then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_regular_file_reads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        std::fs::write(&file, "12345").unwrap();
        let mut policy = ReadPolicy::new([dir.path().to_owned()]);
        assert_eq!(policy.read_text(&file).unwrap().1, "12345");
        policy.max_file_bytes = 4;
        assert!(policy
            .read(&file)
            .unwrap_err()
            .to_string()
            .contains("size limit"));
        assert!(policy.resolve(dir.path()).is_err());
        assert_eq!(expand_home("~/x", Some(dir.path())), dir.path().join("x"));
        assert_eq!(submap("reset"), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret");
        std::fs::write(&target, "secret").unwrap();
        let link = inside.path().join("link");
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(
            ReadPolicy::new([inside.path().to_owned()])
                .read(&link)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
