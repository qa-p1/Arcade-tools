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
    /// Build a blocking HTTP client on a worker thread. No request is sent.
    pub fn github() -> Result<Self> {
        Self::new(Url::parse("https://api.github.com/").unwrap(), false)
    }
    #[cfg(debug_assertions)]
    /// Build a loopback-only test HTTP client on a worker thread. Debug-only;
    /// no request is sent by construction.
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
        let response = request.send()?;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(http_error(status, response.headers()));
        }
        Ok(response)
    }
    /// Where release files live outside the REST API (not subject to its
    /// per-IP limit): github.com in production, the loopback server in tests.
    fn web(&self, path: &str) -> Result<Url> {
        let base = if self.test {
            self.api.clone()
        } else {
            Url::parse("https://github.com/").unwrap()
        };
        base.join(path)
            .map_err(|e| Error::new("source", e.to_string()))
    }
    /// The same release read through github.com download URLs, used when the
    /// API's anonymous limit is exhausted. Release notes aren't available there.
    fn check_without_api(
        &self,
        id: &str,
        repository: &str,
        channel: Channel,
        os: crate::release::Os,
        arch: crate::release::Arch,
    ) -> Result<ReleaseCheck> {
        let tag_path =
            |tag: &str, file: &str| format!("{repository}/releases/download/{tag}/{file}");
        let manifest_url = if channel == Channel::Stable {
            self.web(&format!(
                "{repository}/releases/latest/download/arcade-release.json"
            ))?
        } else {
            self.web(&tag_path("nightly", "arcade-release.json"))?
        };
        let release = Release::parse(&self.json(manifest_url.as_str())?)?;
        release.matches(id, channel)?;
        let asset = release.select(os, arch)?;
        safe_filename(&asset.file)?;
        let tag = if channel == Channel::Stable {
            format!("v{}", release.version)
        } else {
            "nightly".into()
        };
        let download_url = self.web(&tag_path(&tag, &asset.file))?.to_string();
        self.asset_url(&download_url, repository)?;
        let cache = ReleaseCache {
            schema: 1,
            api_url: manifest_url.to_string(),
            etag: None,
            release: release.clone(),
            asset: asset.clone(),
            download_url: download_url.clone(),
            body: String::new(),
        };
        Ok(ReleaseCheck {
            checked: CheckedRelease {
                release,
                asset,
                url: download_url,
            },
            cache,
            unchanged: false,
        })
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

    /// Bounded release history, fetched explicitly on a worker.
    pub fn release_history(&self, repo: &str) -> Result<serde_json::Value> {
        crate::catalog::allowed_repo(repo)?;
        let url = self
            .api
            .join(&format!("repos/{repo}/releases?per_page=20"))
            .map_err(|e| Error::new("url", e.to_string()))?;
        let value: serde_json::Value = serde_json::from_slice(&self.json(url.as_str())?)?;
        if !value.is_array() {
            return Err(Error::new("release", "Release history must be an array."));
        }
        Ok(value)
    }
    /// Explicit GitHub/manifest requests on a worker thread. Kept for existing
    /// callers; catalog-aware callers use `check_repo_conditional`.
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
        let response = match self.fetch(
            url.as_str(),
            cached.and_then(|c| c.etag.as_deref()),
            MAX_JSON,
        ) {
            Ok(response) => response,
            // GitHub's anonymous API allows 60 requests an hour per IP, shared
            // by everyone behind the same NAT. Release files don't count.
            Err(error) if error.code == "rate_limited" => {
                return self
                    .check_without_api(id, repository, channel, os, arch)
                    .map_err(|fallback| {
                        if fallback.code == "not_found" {
                            fallback
                        } else {
                            error
                        }
                    });
            }
            Err(error) => return Err(error),
        };
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
        let prefix = format!("/{repository}/");
        let repository_matches = url
            .path()
            .get(..prefix.len())
            .is_some_and(|path| path.eq_ignore_ascii_case(&prefix))
            && url
                .path()
                .get(prefix.len()..)
                .is_some_and(|path| path.starts_with("releases/download/"));
        if !allowed(&url, self.test)
            || !self.test && (url.host_str() != Some("github.com") || !repository_matches)
        {
            return Err(Error::new(
                "source",
                "Asset is not from this app's GitHub Releases.",
            ));
        }
        Ok(())
    }
    /// Download and verify SHA-256/size on a worker thread. Cancellation is
    /// checked during bounded streaming, before any installer runs.
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
            let n = response.read(&mut block).map_err(|error| {
                eprintln!("Download: {error}");
                Error::new(
                    "network",
                    "The download was interrupted. Check your internet connection and try again.",
                )
            })?;
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
/// A readable error for a failed GitHub response: what happened and what to do.
fn http_error(status: StatusCode, headers: &reqwest::header::HeaderMap) -> Error {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let exhausted = header("x-ratelimit-remaining").as_deref() == Some("0");
    if status == StatusCode::TOO_MANY_REQUESTS || (status == StatusCode::FORBIDDEN && exhausted) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let minutes = header("x-ratelimit-reset")
            .and_then(|reset| reset.parse::<u64>().ok())
            .map(|reset| reset.saturating_sub(now).div_ceil(60).max(1));
        let wait = minutes.map_or_else(
            || "a while".to_owned(),
            |m| format!("about {m} minute{}", if m == 1 { "" } else { "s" }),
        );
        return Error::new(
            "rate_limited",
            format!("GitHub's limit for update checks from this network is used up. Try again in {wait}."),
        );
    }
    if status == StatusCode::NOT_FOUND {
        return Error::new("not_found", "That release isn't published on GitHub.");
    }
    if status.is_server_error() {
        return Error::new(
            "http",
            format!("GitHub is having trouble right now ({status}). Try again later."),
        );
    }
    Error::new("http", format!("GitHub refused the request ({status})."))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_repository_matching_ignores_case_but_keeps_repository_boundaries() {
        let source = Source::github().unwrap();
        for repository in ["qa-p1/arcade-look", "QA-P1/Arcade-look"] {
            assert!(source
                .asset_url(
                    "https://github.com/qa-p1/Arcade-look/releases/download/v1/Look.AppImage",
                    repository
                )
                .is_ok());
        }
        for url in [
            "https://github.com/other/Arcade-look/releases/download/v1/Look.AppImage",
            "https://github.com/qa-p1/Arcade-look-evil/releases/download/v1/Look.AppImage",
            "https://github.com/qa-p1/Arcade-look/Releases/download/v1/Look.AppImage",
            "https://github.com/qa-p1/Arcade-look/releases/other/v1/Look.AppImage",
            "https://example.com/qa-p1/Arcade-look/releases/download/v1/Look.AppImage",
            "http://github.com/qa-p1/Arcade-look/releases/download/v1/Look.AppImage",
        ] {
            assert!(source.asset_url(url, "qa-p1/arcade-look").is_err(), "{url}");
        }
    }
}
