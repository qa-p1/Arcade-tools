//! Optional engine metadata. Detection is explicit worker work, never installation.
use crate::catalog::{check_schema, parse_version};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

pub const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_VERSION_OUTPUT: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCatalog {
    pub schema: u32,
    pub engines: Vec<Engine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum EngineStatus {
    Found {
        path: PathBuf,
        version: Option<String>,
    },
    Missing,
    TooOld {
        path: PathBuf,
        version: String,
        minimum: String,
    },
}
impl Engine {
    /// Pure install-hint selection, safe on any thread. Hints are displayed as
    /// text; this API never executes an install command.
    pub fn install_hint(&self, os: crate::release::Os, os_release: &str) -> Option<&str> {
        let key = match os {
            crate::release::Os::Windows => "windows",
            crate::release::Os::Macos => "macos",
            crate::release::Os::Linux => linux_distro(os_release)?,
        };
        self.install
            .get(key)
            .or_else(|| {
                (key == "ubuntu")
                    .then(|| self.install.get("debian"))
                    .flatten()
            })
            .map(String::as_str)
    }
    /// PATH scan and bounded version process on a worker thread. Binary names
    /// are alternatives (e.g. `soffice`/`libreoffice`); a usable alternative wins
    /// over an old one. Timeout/process/unverifiable-minimum errors are explicit.
    pub fn detect(&self, path: &OsStr, timeout: Duration) -> Result<EngineStatus> {
        let minimum = self
            .detect
            .min_version
            .as_deref()
            .map(parse_version)
            .transpose()?;
        let mut old = None;
        let mut failure = None;
        for name in &self.detect.binaries {
            let Some(binary) = find_binary(name, path) else {
                continue;
            };
            let output = match version_output(&binary, &self.detect.version_args, timeout) {
                Ok(output) => output,
                Err(error) => {
                    failure = Some(error);
                    continue;
                }
            };
            let version = parse_output_version(&output);
            if let Some(minimum) = &minimum {
                let Some(version) = version else {
                    failure = Some(Error::new(
                        "engine_version",
                        format!("Cannot verify {}'s minimum version.", self.name),
                    ));
                    continue;
                };
                if version.cmp_precedence(minimum).is_lt() {
                    old = Some(EngineStatus::TooOld {
                        path: binary,
                        version: version.to_string(),
                        minimum: minimum.to_string(),
                    });
                    continue;
                }
                return Ok(EngineStatus::Found {
                    path: binary,
                    version: Some(version.to_string()),
                });
            }
            return Ok(EngineStatus::Found {
                path: binary,
                version: version.map(|v| v.to_string()),
            });
        }
        if let Some(old) = old {
            return Ok(old);
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(EngineStatus::Missing)
    }
    /// Environment/disk/process work on a worker thread, never a UI thread.
    pub fn detect_current(&self) -> Result<EngineStatus> {
        if self.id == "tesseract" {
            if let Some(binary) = arcade_link::engines::find_tesseract() {
                if let Some(directory) = binary.parent() {
                    let status = self.detect(directory.as_os_str(), VERSION_TIMEOUT)?;
                    if status != EngineStatus::Missing {
                        return Ok(status);
                    }
                }
            }
        }
        self.detect(
            &std::env::var_os("PATH").unwrap_or_default(),
            VERSION_TIMEOUT,
        )
    }
}
/// Parse `/etc/os-release` contents in memory; safe on any thread. Prefer ID,
/// then ID_LIKE in order; Ubuntu falls back to the catalog's Debian hint.
pub fn linux_distro(contents: &str) -> Option<&'static str> {
    let values = contents
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim().trim_matches(['\'', '"'])))
        .collect::<BTreeMap<_, _>>();
    let candidates = values
        .get("ID")
        .into_iter()
        .chain(values.get("ID_LIKE"))
        .flat_map(|v| v.split_whitespace());
    for id in candidates {
        match id {
            "arch" => return Some("arch"),
            "ubuntu" => return Some("ubuntu"),
            "debian" => return Some("debian"),
            "fedora" | "rhel" | "centos" => return Some("fedora"),
            _ => {}
        }
    }
    None
}
/// Read Linux distro metadata once, on a worker thread. No process/network work.
pub fn read_os_release() -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        Ok(String::from_utf8_lossy(&crate::catalog::read_bounded(
            Path::new("/etc/os-release"),
            64 * 1024,
        )?)
        .into_owned())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(String::new())
    }
}
fn find_binary(name: &str, path: &OsStr) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || matches!(name, "." | "..") {
        return None;
    }
    for directory in std::env::split_paths(path) {
        // Ignore empty entries: detection must not execute binaries in the CWD.
        if directory.as_os_str().is_empty() {
            continue;
        }
        #[cfg(windows)]
        let names = [
            name.to_owned(),
            format!("{name}.exe"),
            format!("{name}.com"),
        ];
        #[cfg(not(windows))]
        let names = [name.to_owned()];
        for name in names {
            let candidate = directory.join(name);
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if !candidate
                    .metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
                {
                    continue;
                }
            }
            if let Ok(path) = candidate.canonicalize() {
                return Some(path);
            }
        }
    }
    None
}
fn version_output(binary: &Path, args: &[String], timeout: Duration) -> Result<String> {
    // File-backed output avoids pipe deadlocks, including a descendant retaining
    // stdout. Only 64 KiB per stream is read; the child is killed/reaped at timeout.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn()?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        outcome => {
            let _ = child.kill();
            let _ = child.wait();
            if let Err(error) = outcome {
                return Err(error.into());
            }
            return Err(Error::new(
                "timeout",
                format!("{} version check timed out.", binary.display()),
            ));
        }
    };
    if !status.success() {
        return Err(Error::new(
            "engine_version",
            format!("{} version command failed: {status}", binary.display()),
        ));
    }
    use std::io::{Seek, SeekFrom};
    let mut bytes = Vec::new();
    for file in [&mut stdout, &mut stderr] {
        file.seek(SeekFrom::Start(0))?;
        file.take(MAX_VERSION_OUTPUT).read_to_end(&mut bytes)?;
        bytes.push(b'\n');
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
/// Parse common engine version output in memory, safe on any thread. Two-part
/// versions get patch 0; numeric components with leading zeroes are normalized.
pub fn parse_output_version(output: &str) -> Option<semver::Version> {
    for word in output.split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '-' | '+'))
    {
        let Some(start) = word.find(|c: char| c.is_ascii_digit()) else {
            continue;
        };
        let candidate = &word[start..];
        if let Ok(version) = semver::Version::parse(candidate) {
            return Some(version);
        }
        let parts = candidate.split('.').collect::<Vec<_>>();
        if parts.len() >= 2
            && parts[0].bytes().all(|b| b.is_ascii_digit())
            && parts[1].bytes().all(|b| b.is_ascii_digit())
        {
            let major = parts[0].parse().ok()?;
            let minor = parts[1].parse().ok()?;
            let patch = if let Some(patch) = parts.get(2) {
                patch
                    .split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse()
                    .ok()?
            } else {
                0
            };
            return Some(semver::Version::new(major, minor, patch));
        }
    }
    None
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub homepage: String,
    pub detect: Detection,
    pub used_by: Vec<EngineUse>,
    pub install: BTreeMap<String, String>,
    pub link_download: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub binaries: Vec<String>,
    pub version_args: Vec<String>,
    #[serde(default)]
    pub min_version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineUse {
    pub app: String,
    #[serde(rename = "for")]
    pub purpose: String,
}
impl EngineCatalog {
    /// Parse metadata in memory, safe on any thread; unknown fields are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        check_schema(bytes)?;
        let catalog: Self = serde_json::from_slice(bytes)?;
        let mut ids = HashSet::new();
        for engine in &catalog.engines {
            if !reqwest::Url::parse(&engine.homepage).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            }) {
                return Err(Error::new("engines", "Engine homepage must use HTTPS."));
            }
            if engine.id.is_empty()
                || !ids.insert(&engine.id)
                || engine.detect.binaries.is_empty()
                || engine.detect.version_args.is_empty()
                || engine
                    .detect
                    .binaries
                    .iter()
                    .any(|b| b.is_empty() || b.contains(['/', '\\', ':']) || b == "." || b == "..")
            {
                return Err(Error::new("engines", "Invalid engine detection metadata."));
            }
            if let Some(version) = &engine.detect.min_version {
                parse_version(version)?;
            }
        }
        Ok(catalog)
    }
}
