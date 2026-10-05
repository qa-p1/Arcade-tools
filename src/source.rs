//! GitHub is the only production source. Loopback HTTP is opt-in for tests.
use crate::release::{safe_filename, verify, Asset, Channel, Release};
use crate::{Error, Result};
use reqwest::{blocking::Client, redirect::Policy, Url};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::NamedTempFile;

pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct Source {
    client: Client,
    api: Url,
    test: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}
#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}
#[derive(Debug, Clone)]
pub struct CheckedRelease {
    pub release: Release,
    pub asset: Asset,
    pub url: String,
}

fn allowed(url: &Url, test: bool) -> bool {
    if test {
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    } else {
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some(
                    "api.github.com"
                        | "github.com"
                        | "objects.githubusercontent.com"
                        | "release-assets.githubusercontent.com"
                        | "github-releases.githubusercontent.com"
                )
            )
    }
}

impl Source {
    pub fn github() -> Result<Self> {
        Self::new(Url::parse("https://api.github.com/").unwrap(), false)
    }
    #[cfg(debug_assertions)]
    pub fn test_server(url: &str) -> Result<Self> {
        let url = Url::parse(url).map_err(|e| Error::new("source", e.to_string()))?;
        if !allowed(&url, true) {
            return Err(Error::new(
                "source",
                "Test releases require a loopback HTTP server.",
            ));
        }
        Self::new(url, true)
    }
    fn new(api: Url, test: bool) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("Arcade-Tools/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirect(Policy::custom(move |attempt| {
                if attempt.previous().len() >= 8 || !allowed(attempt.url(), test) {
                    attempt.error("Unsafe release redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self { client, api, test })
    }
    fn response(&self, url: &str) -> Result<reqwest::blocking::Response> {
        let parsed = Url::parse(url).map_err(|e| Error::new("source", e.to_string()))?;
        if !allowed(&parsed, self.test) {
            return Err(Error::new(
                "source",
                "Release URL is outside the trusted GitHub source.",
            ));
        }
        Ok(self
            .client
            .get(parsed)
            .header("Accept", "application/vnd.github+json")
            .send()?
            .error_for_status()?)
    }
    fn json(&self, url: &str) -> Result<Vec<u8>> {
        let mut response = self.response(url)?;
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(MAX_JSON + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JSON {
            return Err(Error::new("too_large", "Release metadata exceeds 1 MiB."));
        }
        Ok(bytes)
    }
    pub fn check(
        &self,
        id: &str,
        channel: Channel,
        os: crate::release::Os,
        arch: crate::release::Arch,
    ) -> Result<CheckedRelease> {
        if !arcade_link::manifest::ids::APPS.contains(&id) {
            return Err(Error::new("unsupported_input", "Unknown Arcade app."));
        }
        let releases = arcade_link::manifest::releases_url(id);
        let repository = releases
            .strip_prefix("https://github.com/")
            .unwrap()
            .trim_end_matches("/releases");
        let selector = if channel == Channel::Stable {
            "latest"
        } else {
            "tags/nightly"
        };
        let url = self
            .api
            .join(&format!("repos/{repository}/releases/{selector}"))
            .map_err(|e| Error::new("source", e.to_string()))?;
        let github: GithubRelease = serde_json::from_slice(&self.json(url.as_str())?)?;
        if github.draft
            || channel == Channel::Stable
                && (github.prerelease || !github.tag_name.starts_with('v'))
            || channel == Channel::Nightly && github.tag_name != "nightly"
        {
            return Err(Error::new(
                "manifest",
                "GitHub release does not match the requested channel.",
            ));
        }
        let manifest = unique_asset(&github.assets, "arcade-release.json")?;
        let release = Release::parse(&self.json(&manifest.browser_download_url)?)?;
        release.compatible(id, channel)?;
        let asset = release.select(os, arch)?;
        safe_filename(&asset.file)?;
        let download = unique_asset(&github.assets, &asset.file)?;
        if !self.test {
            for item in [manifest, download] {
                let url = Url::parse(&item.browser_download_url)
                    .map_err(|e| Error::new("source", e.to_string()))?;
                if url.scheme() != "https"
                    || url.host_str() != Some("github.com")
                    || !url
                        .path()
                        .starts_with(&format!("/{repository}/releases/download/"))
                {
                    return Err(Error::new(
                        "source",
                        "Asset is not from this app's GitHub Releases.",
                    ));
                }
            }
        }
        Ok(CheckedRelease {
            release,
            asset,
            url: download.browser_download_url.clone(),
        })
    }
    pub fn download(
        &self,
        checked: &CheckedRelease,
        cache: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<NamedTempFile> {
        std::fs::create_dir_all(cache)?;
        let mut file = NamedTempFile::new_in(cache)?;
        let mut response = self.response(&checked.url)?;
        let total = checked.asset.size.or(response.content_length());
        if total.is_some_and(|n| n > MAX_DOWNLOAD) {
            return Err(Error::new(
                "too_large",
                "Installer exceeds the 1 GiB download limit.",
            ));
        }
        let mut count = 0u64;
        let mut block = [0u8; 65536];
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::new("cancelled", "Download cancelled."));
            }
            let n = response.read(&mut block)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > MAX_DOWNLOAD {
                return Err(Error::new(
                    "too_large",
                    "Installer exceeds the 1 GiB download limit.",
                ));
            }
            file.write_all(&block[..n])?;
            progress(count, total);
        }
        file.as_file().sync_all()?;
        verify(file.path(), &checked.asset)?;
        Ok(file)
    }
}
fn unique_asset<'a>(assets: &'a [GithubAsset], name: &str) -> Result<&'a GithubAsset> {
    let matches: Vec<_> = assets.iter().filter(|a| a.name == name).collect();
    if matches.len() != 1 {
        return Err(Error::new(
            "unavailable",
            format!("The release must include exactly one {name}."),
        ));
    }
    Ok(matches[0])
}
