//! The canonical schema is validated here; installation policy is separate.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Nightly,
}
impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Windows,
    Macos,
}
impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X64,
    Arm64,
    Universal,
}
impl Arch {
    pub fn current() -> Result<Self> {
        match std::env::consts::ARCH {
            "x86_64" => Ok(Self::X64),
            "aarch64" => Ok(Self::Arm64),
            a => Err(Error::new(
                "unavailable",
                format!("No per-user installer for {a}."),
            )),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Appimage,
    Deb,
    Rpm,
    Tarball,
    Nsis,
    Inno,
    Msi,
    Dmg,
    Portable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub os: Os,
    pub arch: Arch,
    pub kind: Kind,
    pub file: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<Vec<String>>,
    #[serde(
        rename = "installArgs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub install_args: Option<Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub channel: Channel,
    pub link_protocol: Vec<u32>,
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// Last validated release metadata and its GitHub API validator. Schema 1 is
/// checked before reuse; the installer itself is still verified on download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCache {
    pub schema: u32,
    pub api_url: String,
    pub etag: Option<String>,
    pub release: Release,
    pub asset: Asset,
    pub download_url: String,
    pub body: String,
}

/// Receipts may record the compiled base version for a nightly build. Its
/// channel is checked separately; two explicit nightly identities must agree.
pub(crate) fn versions_match(left: &str, right: &str, channel: Channel) -> bool {
    left == right
        || match (semver::Version::parse(left), semver::Version::parse(right)) {
            (Ok(left), Ok(right)) if channel == Channel::Stable => {
                left.cmp_precedence(&right).is_eq()
            }
            (Ok(left), Ok(right)) => {
                let nightly = |v: &semver::Version| {
                    v.pre.as_str() == "nightly" || v.pre.as_str().starts_with("nightly.")
                };
                (left.major, left.minor, left.patch) == (right.major, right.minor, right.patch)
                    && (left.pre.is_empty() && nightly(&right)
                        || right.pre.is_empty() && nightly(&left))
            }
            _ => false,
        }
}

impl Release {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // serde's Option accepts explicit null; the schema does not.
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if let Some(assets) = value.get("assets").and_then(|a| a.as_array()) {
            for a in assets {
                for key in ["size", "silent", "installArgs"] {
                    if a.get(key).is_some_and(|v| v.is_null()) {
                        return Err(Error::new("manifest", format!("{key} cannot be null")));
                    }
                }
            }
        }
        let r: Self = serde_json::from_value(value)?;
        let suffix = r.id.strip_prefix("arcade.").unwrap_or("");
        let valid = r.schema == 1
            && !suffix.is_empty()
            && suffix.bytes().all(|b| b.is_ascii_lowercase())
            && r.version.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && !r.link_protocol.is_empty()
            && r.link_protocol.iter().all(|p| *p >= 1)
            && !r.assets.is_empty()
            && r.assets.iter().all(|a| {
                !a.file.is_empty()
                    && !a.file.contains(['/', '\\'])
                    && a.sha256.len() == 64
                    && a.sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
        if !valid {
            return Err(Error::new(
                "manifest",
                "Release does not match arcade-release.schema.json (schema 1).",
            ));
        }
        Ok(r)
    }

    pub fn select(&self, os: Os, arch: Arch) -> Result<Asset> {
        let mut assets: Vec<_> = self
            .assets
            .iter()
            .filter(|a| {
                a.os == os
                    && (a.arch == arch || os == Os::Macos && a.arch == Arch::Universal)
                    && match (os, a.kind) {
                        (Os::Linux, Kind::Appimage)
                        | (Os::Windows, Kind::Nsis | Kind::Inno)
                        | (Os::Macos, Kind::Dmg) => true,
                        (Os::Linux, Kind::Tarball) => {
                            self.id == arcade_link::manifest::ids::CLIPBOARD
                                || self.id == crate::apps::SHELF
                        }
                        _ => false,
                    }
            })
            .collect();
        // Prefer native architecture and AppImages; never pick a system package.
        assets.sort_by_key(|a| (a.arch != arch, a.kind == Kind::Tarball));
        let Some(a) = assets.first() else {
            return Err(Error::new(
                "unavailable",
                "This release has no supported per-user installer for this OS and architecture.",
            ));
        };
        if assets.get(1).is_some_and(|b| {
            (b.arch != arch, b.kind == Kind::Tarball) == (a.arch != arch, a.kind == Kind::Tarball)
        }) {
            return Err(Error::new(
                "manifest",
                "Release has ambiguous installers for this platform.",
            ));
        }
        Ok((*a).clone())
    }

    pub fn compatible(&self, id: &str, channel: Channel) -> Result<()> {
        self.matches(id, channel)?;
        if !self.link_protocol.contains(&1) {
            return Err(Error::new(
                "version_mismatch",
                "This release does not support Arcade Link protocol 1.",
            ));
        }
        Ok(())
    }
    /// Pure metadata validation, safe on any thread. Update discovery must see
    /// incompatible releases so it can report a held-back reason.
    pub fn matches(&self, id: &str, channel: Channel) -> Result<()> {
        if self.id != id || self.channel != channel {
            return Err(Error::new(
                "manifest",
                "Release app or channel does not match the requested app.",
            ));
        }
        Ok(())
    }
}

pub fn safe_filename(name: &str) -> Result<()> {
    if matches!(name, "." | "..")
        || name.is_empty()
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(Error::new("manifest", "Unsafe release asset filename."));
    }
    Ok(())
}
/// File/hash work on a worker thread; never a UI thread.
pub fn checksum(path: &Path) -> Result<String> {
    let mut reader = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut block = [0u8; 65536];
    loop {
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Filesystem/hash verification on a worker thread; performs no mutation.
pub fn verify(path: &Path, asset: &Asset) -> Result<()> {
    if asset
        .size
        .is_some_and(|size| std::fs::metadata(path).map_or(true, |m| m.len() != size))
        || checksum(path)? != asset.sha256
    {
        return Err(Error::new(
            "checksum",
            "Downloaded file failed SHA-256 or size verification. Nothing was installed.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn nightly_base_receipts_require_matching_base_and_nightly_identity() {
        for id in ["0.2.0-nightly.42", "0.2.0-nightly.20261014+abc"] {
            assert!(versions_match("0.2.0", id, Channel::Nightly));
            assert!(versions_match(id, "0.2.0", Channel::Nightly));
            assert!(!versions_match("0.2.0", id, Channel::Stable));
            assert!(!versions_match("0.1.0", id, Channel::Nightly));
        }
        assert!(!versions_match(
            "0.2.0-nightly.41",
            "0.2.0-nightly.42",
            Channel::Nightly
        ));
        assert!(!versions_match("0.2.0", "0.2.0-beta.1", Channel::Nightly));
        assert!(!versions_match("build-a", "build-b", Channel::Nightly));
        assert!(versions_match("0.2.0", "0.2.0+build.42", Channel::Stable));
    }
}
