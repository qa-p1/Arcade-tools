//! GitHub is the only production source. Loopback HTTP is opt-in for tests.
use crate::release::{safe_filename, verify, Asset, Channel, Release, ReleaseCache};
use crate::{Error, Result};
use reqwest::{blocking::Client, redirect::Policy, StatusCode, Url};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::NamedTempFile;

pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;

/// Bounded conditional HTTP result. A 304 never carries a replacement body.
#[derive(Debug, Clone)]
pub enum Fetch {
    Modified {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
    NotModified,
}

/// Injectable synchronous HTTP source. Call from a worker thread.
pub trait HttpSource: Send + Sync {
    /// Perform a bounded request on a worker thread; no background work is started.
    fn fetch(&self, url: &str, etag: Option<&str>, limit: u64) -> Result<Fetch>;
}

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
    #[serde(default)]
    body: Option<String>,
}
#[derive(Debug, Clone)]
pub struct CheckedRelease {
    pub release: Release,
    pub asset: Asset,
    pub url: String,
}
#[derive(Debug, Clone)]
pub struct ReleaseCheck {
    pub checked: CheckedRelease,
    pub cache: ReleaseCache,
    pub unchanged: bool,
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
                        | "raw.githubusercontent.com"
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
        self.conditional_response(url, None)
    }
    fn conditional_response(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<reqwest::blocking::Response> {
        let parsed = Url::parse(url).map_err(|e| Error::new("source", e.to_string()))?;
        if !allowed(&parsed, self.test) {
            return Err(Error::new(
                "source",
                "Release URL is outside the trusted GitHub source.",
            ));
        }
        let mut request = self
            .client
            .get(parsed)
            .header("Accept", "application/vnd.github+json");
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        request
            .send()?
            .error_for_status()
            .map_err(|error| Error::new("http", error.to_string()))
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
        if !crate::apps::APPS.contains(&id) {
            return Err(Error::new("unsupported_input", "Unknown Arcade app."));
        }
        let releases = crate::apps::releases_url(id);
        let repository = releases
            .strip_prefix("https://github.com/")
            .unwrap()
            .trim_end_matches("/releases");
        self.check_repo(id, repository, channel, os, arch)
    }
    /// Explicit release network I/O; call on a worker thread. The repository
    /// comes from the validated catalog, not display names or install paths.
    pub fn check_repo(
        &self,
        id: &str,
        repository: &str,
        channel: Channel,
        os: crate::release::Os,
        arch: crate::release::Arch,
    ) -> Result<CheckedRelease> {
        let check = self.check_repo_conditional(id, repository, channel, os, arch, None)?;
        check.checked.release.compatible(id, channel)?;
        Ok(check.checked)
    }
    /// Conditional GitHub request and manifest validation, on a worker thread.
    /// A 304 reuses only validated metadata for this source, app, channel and
    /// platform; it performs no asset/manifest request. Protocol eligibility is
    /// reported by `updates` and enforced separately before installation.
    pub fn check_repo_conditional(
        &self,
        id: &str,
        repository: &str,
        channel: Channel,
        os: crate::release::Os,
        arch: crate::release::Arch,
        cached: Option<&ReleaseCache>,
    ) -> Result<ReleaseCheck> {
        crate::catalog::allowed_repo(repository)?;
        let selector = if channel == Channel::Stable {
            "latest"
        } else {
            "tags/nightly"
        };
        let url = self
            .api
            .join(&format!("repos/{repository}/releases/{selector}"))
            .map_err(|e| Error::new("source", e.to_string()))?;
        let cached = cached.filter(|cache| {
            cache.schema == 1
                && cache.api_url == url.as_str()
                && Release::parse(&serde_json::to_vec(&cache.release).unwrap_or_default()).is_ok()
                && cache.release.matches(id, channel).is_ok()
                && cache
                    .release
                    .select(os, arch)
                    .is_ok_and(|a| a == cache.asset)
                && self.asset_url(&cache.download_url, repository).is_ok()
        });
        let response = self.fetch(
            url.as_str(),
            cached.and_then(|c| c.etag.as_deref()),
            MAX_JSON,
        )?;
        let (bytes, etag) = match response {
            Fetch::NotModified => {
                let cache = cached
                    .ok_or_else(|| Error::new("source", "304 without valid release cache."))?
                    .clone();
                return Ok(ReleaseCheck {
                    checked: CheckedRelease {
                        release: cache.release.clone(),
                        asset: cache.asset.clone(),
                        url: cache.download_url.clone(),
                    },
                    cache,
                    unchanged: true,
                });
            }
            Fetch::Modified { bytes, etag } => (bytes, etag),
        };
        let github: GithubRelease = serde_json::from_slice(&bytes)?;
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
        self.asset_url(&manifest.browser_download_url, repository)?;
        let release = Release::parse(&self.json(&manifest.browser_download_url)?)?;
        release.matches(id, channel)?;
        if channel == Channel::Stable && github.tag_name != format!("v{}", release.version) {
            return Err(Error::new(
                "manifest",
                "Release version does not match its stable tag.",
            ));
        }
        let asset = release.select(os, arch)?;
        safe_filename(&asset.file)?;
        let download = unique_asset(&github.assets, &asset.file)?;
        self.asset_url(&download.browser_download_url, repository)?;
        let cache = ReleaseCache {
            schema: 1,
            api_url: url.to_string(),
            etag,
            release: release.clone(),
            asset: asset.clone(),
            download_url: download.browser_download_url.clone(),
            body: github.body.unwrap_or_default(),
        };
        Ok(ReleaseCheck {
            checked: CheckedRelease {
                release,
                asset,
                url: cache.download_url.clone(),
            },
            cache,
            unchanged: false,
        })
    }
    fn asset_url(&self, value: &str, repository: &str) -> Result<()> {
        let url = Url::parse(value).map_err(|e| Error::new("source", e.to_string()))?;
        if !allowed(&url, self.test)
            || !self.test
                && (url.host_str() != Some("github.com")
                    || !url
                        .path()
                        .starts_with(&format!("/{repository}/releases/download/")))
        {
            return Err(Error::new(
                "source",
                "Asset is not from this app's GitHub Releases.",
            ));
        }
        Ok(())
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
                return Err(Error::new("cancelled", "Cancelled."));
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
impl HttpSource for Source {
    fn fetch(&self, url: &str, etag: Option<&str>, limit: u64) -> Result<Fetch> {
        let mut response = self.conditional_response(url, etag)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            if etag.is_none() {
                return Err(Error::new(
                    "source",
                    "Unexpected 304 without a cached document.",
                ));
            }
            return Ok(Fetch::NotModified);
        }
        if response.status() != StatusCode::OK {
            return Err(Error::new(
                "source",
                "Metadata request did not return HTTP 200.",
            ));
        }
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(Error::new("too_large", "Metadata exceeds its size limit."));
        }
        let etag = response
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(Error::new("too_large", "Metadata exceeds its size limit."));
        }
        Ok(Fetch::Modified { bytes, etag })
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
