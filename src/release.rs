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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Release {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // serde's Option accepts explicit null; the schema does not.
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if let Some(assets) = value.get("assets").and_then(|a| a.as_array()) {
            for a in assets {
                for key in ["size", "silent"] {
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
        if self.id != id || self.channel != channel {
            return Err(Error::new(
                "manifest",
                "Release app or channel does not match the requested app.",
            ));
        }
        if !self.link_protocol.contains(&1) {
            return Err(Error::new(
                "version_mismatch",
                "This release does not support Arcade Link protocol 1.",
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
