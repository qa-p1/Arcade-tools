//! GitHub is the only production source. Loopback HTTP is opt-in for tests.
use crate::delta;
use crate::release::{safe_filename, verify, Asset, Channel, Release, ReleaseCache};
use crate::{trust, Error, Result};
use arcade_link::release::{assemble, ChunkIndex, DeltaPlan, FileEntry, ReleaseManifest};
use reqwest::{blocking::Client, redirect::Policy, StatusCode, Url};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::NamedTempFile;

pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;
/// A release manifest lists every chunk of every file: about 5 MB for 1 GiB of installers.
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;

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
#[derive(Clone, Deserialize)]
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
    /// The GitHub release tag (`v1.2.0`, `v1.3.0-rc.1`); empty for packages Tools already holds.
    pub tag: String,
    /// The chosen installer's entry in the verified `manifest.json`: its chunks drive delta downloads. `None` only
    /// for packages Tools already holds (rollback).
    pub entry: Option<FileEntry>,
}
#[derive(Debug, Clone)]
pub struct ReleaseCheck {
    pub checked: CheckedRelease,
    pub cache: ReleaseCache,
    pub unchanged: bool,
}

/// The newest published release whose tag belongs to `channel`: stable releases are never prereleases, nightly and
/// rc builds always are.
/// GitHub lists releases newest first. The highest version wins; among equal precedence (build metadata only differs)
/// the most recently published one does, as GitHub's own "latest" would.
fn newest_on(
    listing: Vec<GithubRelease>,
    channel: Channel,
    withdrawn: &[String],
) -> Option<GithubRelease> {
    let mut best: Option<(semver::Version, GithubRelease)> = None;
    for release in listing {
        if release.draft
            || release.prerelease != (channel != Channel::Stable)
            || !channel.owns_tag(&release.tag_name)
            || withdrawn.contains(&release.tag_name)
        {
            continue;
        }
        let Ok(version) = semver::Version::parse(&release.tag_name[1..]) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(b, _)| version.cmp_precedence(b).is_gt())
        {
            best = Some((version, release));
        }
    }
    best.map(|(_, release)| release)
}

/// `v1.2.3`, `v1.2.3-rc.4` and `v1.2.3-nightly.20261107.1` all carry version 1.2.3 (Link D4: rc and nightly builds
/// report the version they are built as).
fn tag_carries(tag: &str, version: &str) -> bool {
    let (Some(tag), Ok(version)) = (
        tag.strip_prefix('v')
            .and_then(|t| semver::Version::parse(t).ok()),
        semver::Version::parse(version),
    ) else {
        return false;
    };
    (tag.major, tag.minor, tag.patch) == (version.major, version.minor, version.patch)
        && (version.pre.is_empty()
            || tag.pre == version.pre
            || tag.pre.as_str().starts_with(&format!("{}.", version.pre)))
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

/// Read-idle timeout: the blocking client applies it to each read, so a slow but steady download finishes (D3, D5).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

impl Source {
    /// Build a blocking HTTP client on a worker thread. No request is sent.
    pub fn github() -> Result<Self> {
        Self::new(
            Url::parse("https://api.github.com/").unwrap(),
            false,
            DOWNLOAD_TIMEOUT,
        )
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
        Self::new(url, true, DOWNLOAD_TIMEOUT)
    }
    #[cfg(debug_assertions)]
    /// [`Source::test_server`] with a shorter download timeout, so tests can pin how it applies.
    pub fn test_server_with_timeout(url: &str, timeout: Duration) -> Result<Self> {
        let mut source = Self::test_server(url)?;
        source.client = Self::new(source.api.clone(), true, timeout)?.client;
        Ok(source)
    }
    fn new(api: Url, test: bool, timeout: Duration) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("Arcade-Tools/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
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
        withdrawn: &[String],
    ) -> Result<ReleaseCheck> {
        if channel != Channel::Stable {
            // Only the stable release has a fixed download path outside the API.
            return Err(Error::new(
                "rate_limited",
                "GitHub's request limit was reached; Tools will check again later.",
            ));
        }
        let tag_path =
            |tag: &str, file: &str| format!("{repository}/releases/download/{tag}/{file}");
        let latest =
            |file: &str| self.web(&format!("{repository}/releases/latest/download/{file}"));
        // The signature binds the manifest to its tag, so the tag it names is the one to download from.
        let signed = self.bytes(latest(trust::MANIFEST)?.as_str(), MAX_MANIFEST)?;
        let tag = ReleaseManifest::from_json(&signed)
            .map_err(|e| Error::new("manifest", format!("manifest.json: {e}")))?
            .tag;
        if withdrawn.contains(&tag) {
            return Err(Error::new(
                "rate_limited",
                "GitHub's request limit was reached; Tools will check again later.",
            ));
        }
        let signature = self.text(latest(trust::SIGNATURE)?.as_str())?;
        let signed = trust::verify(
            id,
            channel,
            &tag,
            &signed,
            Some(&signature),
            &trust::trusted_keys(self.test)?,
        )?;
        let manifest_url = latest("arcade-release.json")?;
        let release = Release::parse(&self.json(manifest_url.as_str())?)?;
        release.matches(id, channel)?;
        let asset = release.select(os, arch)?;
        safe_filename(&asset.file)?;
        let entry = trust::entry_for(&signed, &release, &asset)?;
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
            entry: Some(entry.clone()),
            tag: tag.clone(),
        };
        Ok(ReleaseCheck {
            checked: CheckedRelease {
                release,
                asset,
                url: download_url,
                tag,
                entry: Some(entry),
            },
            cache,
            unchanged: false,
        })
    }
    fn json(&self, url: &str) -> Result<Vec<u8>> {
        self.bytes(url, MAX_JSON)
    }
    fn bytes(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let mut response = self.response(url)?;
        let mut bytes = Vec::new();
        response.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(Error::new(
                "too_large",
                format!("Release metadata exceeds {} MiB.", limit / (1024 * 1024)),
            ));
        }
        Ok(bytes)
    }
    fn text(&self, url: &str) -> Result<String> {
        String::from_utf8(self.json(url)?)
            .map_err(|_| Error::new("signature", "The release signature is not text."))
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
        let check = self.check_repo_conditional(id, repository, channel, os, arch, None, &[])?;
        check.checked.release.compatible(id, channel)?;
        Ok(check.checked)
    }
    /// Conditional GitHub request and manifest validation, on a worker thread.
    /// A 304 reuses only validated metadata for this source, app, channel and
    /// platform; it performs no asset/manifest request. Protocol eligibility is
    /// reported by `updates` and enforced separately before installation.
    #[allow(clippy::too_many_arguments)]
    pub fn check_repo_conditional(
        &self,
        id: &str,
        repository: &str,
        channel: Channel,
        os: crate::release::Os,
        arch: crate::release::Arch,
        cached: Option<&ReleaseCache>,
        withdrawn: &[String],
    ) -> Result<ReleaseCheck> {
        // Unsigned rc manifests are trusted only on the Test channel, so it needs the gates' override.
        if channel == Channel::Test && !self.test && Channel::overridden().is_none() {
            return Err(Error::new(
                "invalid",
                "Release candidates are only followed for release testing.",
            ));
        }
        crate::catalog::allowed_repo(repository)?;
        // One listing serves every channel; the newest release whose tag belongs to the channel wins (D5).
        let url = self
            .api
            .join(&format!("repos/{repository}/releases?per_page=30"))
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
                && cache
                    .entry
                    .as_ref()
                    .is_some_and(|e| e.name == cache.asset.file && e.sha256 == cache.asset.sha256)
                && !cache.tag.is_empty()
                && !withdrawn.contains(&cache.tag)
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
                    .check_without_api(id, repository, channel, os, arch, withdrawn)
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
                        tag: cache.tag.clone(),
                        entry: cache.entry.clone(),
                    },
                    cache,
                    unchanged: true,
                });
            }
            Fetch::Modified { bytes, etag } => (bytes, etag),
        };
        let listing: Vec<GithubRelease> = serde_json::from_slice(&bytes)?;
        let github = newest_on(listing, channel, withdrawn).ok_or_else(|| {
            Error::new(
                "not_found",
                format!("No {} release is published for this app.", channel.as_str()),
            )
        })?;
        // Trust first: nothing else is read from a release whose signed manifest doesn't verify (D5).
        let signed = unique_asset(&github.assets, trust::MANIFEST)?;
        self.asset_url(&signed.browser_download_url, repository)?;
        let signed = self.bytes(&signed.browser_download_url, MAX_MANIFEST)?;
        let signature = match github
            .assets
            .iter()
            .filter(|a| a.name == trust::SIGNATURE)
            .count()
        {
            0 => None,
            _ => {
                let asset = unique_asset(&github.assets, trust::SIGNATURE)?;
                self.asset_url(&asset.browser_download_url, repository)?;
                Some(self.text(&asset.browser_download_url)?)
            }
        };
        let signed = trust::verify(
            id,
            channel,
            &github.tag_name,
            &signed,
            signature.as_deref(),
            &trust::trusted_keys(self.test)?,
        )?;
        let manifest = unique_asset(&github.assets, "arcade-release.json")?;
        self.asset_url(&manifest.browser_download_url, repository)?;
        let release = Release::parse(&self.json(&manifest.browser_download_url)?)?;
        release.matches(id, channel)?;
        if !tag_carries(&github.tag_name, &release.version) {
            return Err(Error::new(
                "manifest",
                "Release version does not match its tag.",
            ));
        }
        let asset = release.select(os, arch)?;
        safe_filename(&asset.file)?;
        let entry = trust::entry_for(&signed, &release, &asset)?;
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
            entry: Some(entry.clone()),
            tag: github.tag_name.clone(),
        };
        Ok(ReleaseCheck {
            checked: CheckedRelease {
                release,
                asset,
                url: cache.download_url.clone(),
                tag: cache.tag.clone(),
                entry: Some(entry),
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
    /// Download and verify SHA-256/size on a worker thread, without local chunks.
    pub fn download(
        &self,
        checked: &CheckedRelease,
        cache: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<NamedTempFile> {
        self.download_from(checked, cache, &ChunkIndex::default(), cancel, progress)
    }
    /// The download ladder (D5) on a worker thread: a delta built from `have` when it saves enough, else (or when the
    /// delta fails) the whole file, resumed from `<sha256>.part` across up to three attempts. Cancellation is checked
    /// while streaming; nothing is installed here. Progress counts the bytes to download.
    pub fn download_from(
        &self,
        checked: &CheckedRelease,
        cache: &Path,
        have: &ChunkIndex,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<NamedTempFile> {
        std::fs::create_dir_all(cache)?;
        if let Some(entry) = &checked.entry {
            if let Some(plan) = delta::plan(have, entry) {
                match self.download_delta(checked, entry, &plan, have, cache, cancel, progress) {
                    Ok(file) => return Ok(file),
                    Err(error) if error.code == "cancelled" => return Err(error),
                    Err(error) => eprintln!(
                        "Delta download failed; downloading the whole file: {}",
                        error.message
                    ),
                }
            }
        }
        self.download_whole(checked, cache, cancel, progress)
    }
    #[allow(clippy::too_many_arguments)]
    fn download_delta(
        &self,
        checked: &CheckedRelease,
        entry: &FileEntry,
        plan: &DeltaPlan,
        have: &ChunkIndex,
        cache: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<NamedTempFile> {
        let requests = delta::requests(plan);
        let total: u64 = requests.iter().map(|r| r.end - r.start).sum();
        let mut file = NamedTempFile::new_in(cache)?;
        let mut buffer = Vec::new();
        let mut buffered = 0..0;
        let mut next = 0;
        let mut count = 0;
        let mut stopped = None;
        let mut out = std::io::BufWriter::new(file.as_file_mut());
        let assembled = assemble(
            entry,
            plan,
            have,
            &mut |range| {
                if range.start < buffered.start || range.end > buffered.end {
                    while requests.get(next).is_some_and(|r| r.end <= range.start) {
                        next += 1;
                    }
                    let request = match requests.get(next) {
                        Some(r) if r.start <= range.start && range.end <= r.end => r.clone(),
                        _ => {
                            return Err(std::io::Error::other("range outside the planned requests"))
                        }
                    };
                    buffer =
                        match self.fetch_range(&checked.url, request.clone(), cancel, &mut |n| {
                            count += n;
                            progress(count, Some(total));
                        }) {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                stopped = Some(error);
                                return Err(std::io::Error::other("stopped"));
                            }
                        };
                    buffered = request;
                }
                let at = (range.start - buffered.start) as usize;
                Ok(buffer[at..at + (range.end - range.start) as usize].to_vec())
            },
            &mut out,
        );
        if let Some(error) = stopped {
            return Err(error);
        }
        assembled.map_err(|e| Error::new("delta", e.to_string()))?;
        out.flush()?;
        drop(out);
        file.as_file().sync_all()?;
        verify(file.path(), &checked.asset)?;
        Ok(file)
    }
    /// One `Range: bytes=a-b` request, read whole into memory (at most [`delta::REQUEST_MAX`]).
    fn fetch_range(
        &self,
        url: &str,
        range: std::ops::Range<u64>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<Vec<u8>> {
        let mut response = self.ranged_response(url, range.start, Some(range.end - 1))?;
        let expected = format!("bytes {}-{}/", range.start, range.end - 1);
        if response.status() != StatusCode::PARTIAL_CONTENT
            || !content_range(&response).is_some_and(|v| v.starts_with(&expected))
        {
            return Err(Error::new(
                "delta",
                "The server did not return the requested range.",
            ));
        }
        let len = (range.end - range.start) as usize;
        let mut bytes = Vec::with_capacity(len);
        let mut block = [0u8; 65536];
        while bytes.len() < len {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::new("cancelled", "Cancelled."));
            }
            let n = response.read(&mut block).map_err(interrupted)?;
            if n == 0 {
                break;
            }
            let n = n.min(len - bytes.len());
            bytes.extend_from_slice(&block[..n]);
            progress(n as u64);
        }
        if bytes.len() != len {
            return Err(Error::new("delta", "A range ended early."));
        }
        Ok(bytes)
    }
    fn download_whole(
        &self,
        checked: &CheckedRelease,
        cache: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<NamedTempFile> {
        let part = cache.join(format!("{}.part", checked.asset.sha256));
        let size = checked
            .entry
            .as_ref()
            .map(|e| e.size)
            .or(checked.asset.size);
        if size.is_some_and(|n| n > MAX_DOWNLOAD) {
            return Err(too_large());
        }
        let mut attempt = 0;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::new("cancelled", "Cancelled."));
            }
            match self.stream_to_part(&checked.url, &part, size, cancel, progress) {
                Ok(()) => break,
                Err(error) if error.code == "network" && attempt + 1 < DOWNLOAD_ATTEMPTS => {
                    attempt += 1;
                    eprintln!("Download attempt {attempt} interrupted; resuming.");
                    std::thread::sleep(Duration::from_millis(500 * attempt as u64));
                }
                Err(error) => return Err(error),
            }
        }
        if let Err(error) = verify(&part, &checked.asset) {
            let _ = std::fs::remove_file(&part);
            return Err(error);
        }
        let file = std::fs::File::open(&part)?;
        Ok(NamedTempFile::from_parts(
            file,
            tempfile::TempPath::try_from_path(&part)?,
        ))
    }
    /// Continues `part` from its current length (`Range: bytes=N-`); a server that answers 200 starts it over.
    fn stream_to_part(
        &self,
        url: &str,
        part: &Path,
        size: Option<u64>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let mut have = std::fs::metadata(part).map_or(0, |m| m.len());
        if size.is_some_and(|n| have > n) {
            std::fs::remove_file(part)?;
            have = 0;
        }
        if size.is_some_and(|n| have == n) {
            return Ok(());
        }
        let mut response = self.ranged_response(url, have, None)?;
        let resumed = have > 0
            && response.status() == StatusCode::PARTIAL_CONTENT
            && content_range(&response).is_some_and(|v| v.starts_with(&format!("bytes {have}-")));
        if !resumed {
            if response.status() != StatusCode::OK {
                return Err(Error::new(
                    "source",
                    "The download did not return HTTP 200.",
                ));
            }
            have = 0;
        }
        // What this response promises; the expected size is checked by verification, not here.
        let served = response.content_length().map(|n| n + have);
        let total = size.or(served);
        if total.is_some_and(|n| n > MAX_DOWNLOAD) {
            return Err(too_large());
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)?;
        let mut count = have;
        let mut block = [0u8; 65536];
        loop {
            if cancel.load(Ordering::SeqCst) {
                file.sync_all()?;
                return Err(Error::new("cancelled", "Cancelled."));
            }
            let n = match response.read(&mut block) {
                Ok(n) => n,
                Err(error) => {
                    file.sync_all()?;
                    return Err(interrupted(error));
                }
            };
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > MAX_DOWNLOAD {
                return Err(too_large());
            }
            file.write_all(&block[..n])?;
            progress(count, total);
        }
        file.sync_all()?;
        if served.is_some_and(|n| count < n) {
            return Err(interrupted(std::io::Error::other("the body ended early")));
        }
        Ok(())
    }
    /// A download request starting at `from` (no Range header at 0), ending at `to` inclusive if given.
    fn ranged_response(
        &self,
        url: &str,
        from: u64,
        to: Option<u64>,
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
            .header("Accept", "application/octet-stream");
        if from > 0 || to.is_some() {
            let end = to.map(|t| t.to_string()).unwrap_or_default();
            request = request.header("Range", format!("bytes={from}-{end}"));
        }
        let response = request.send().map_err(|error| {
            eprintln!("Download: {error}");
            interrupted(std::io::Error::other(error))
        })?;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(http_error(status, response.headers()));
        }
        Ok(response)
    }
}
const DOWNLOAD_ATTEMPTS: u32 = 3;
fn too_large() -> Error {
    Error::new("too_large", "Installer exceeds the 1 GiB download limit.")
}
fn interrupted(error: std::io::Error) -> Error {
    eprintln!("Download: {error}");
    Error::new(
        "network",
        "The download was interrupted. Check your internet connection and try again.",
    )
}
fn content_range(response: &reqwest::blocking::Response) -> Option<&str> {
    response.headers().get("Content-Range")?.to_str().ok()
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

    fn listed(tag: &str, prerelease: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease,
            assets: Vec::new(),
            body: None,
        }
    }

    #[test]
    fn the_newest_release_of_each_channel_is_chosen_from_the_listing() {
        let mut draft = listed("v9.0.0", false);
        draft.draft = true;
        let listing = vec![
            draft,
            listed("v1.1.0-rc.2", true),
            listed("v1.1.0-nightly.20261107.1", true),
            listed("v0.9.0", false),
            listed("v1.0.0+build.2", false),
            listed("v1.0.0+build.1", false),
            listed("v1.1.0-rc.1", true),
            listed("v1.0.0-nightly.20261101.3", true),
            listed("nightly", true),
        ];
        let tag = |c| newest_on(listing.clone(), c, &[]).map(|r| r.tag_name);
        assert_eq!(tag(Channel::Stable).as_deref(), Some("v1.0.0+build.2"));
        assert_eq!(
            tag(Channel::Nightly).as_deref(),
            Some("v1.1.0-nightly.20261107.1")
        );
        assert_eq!(tag(Channel::Test).as_deref(), Some("v1.1.0-rc.2"));
        assert!(newest_on(vec![listed("v1.0.0-rc.1", false)], Channel::Test, &[]).is_none());
        // A withdrawn release is skipped: the next newest is offered.
        assert_eq!(
            newest_on(
                listing.clone(),
                Channel::Stable,
                &["v1.0.0+build.2".into(), "v1.0.0+build.1".into()]
            )
            .map(|r| r.tag_name)
            .as_deref(),
            Some("v0.9.0")
        );
    }

    #[test]
    fn a_tag_carries_the_version_it_is_built_as() {
        assert!(tag_carries("v1.2.3", "1.2.3"));
        assert!(tag_carries("v1.2.3-rc.4", "1.2.3"));
        assert!(tag_carries("v1.2.3-nightly.20261107.1", "1.2.3"));
        assert!(tag_carries("v1.2.3-nightly.42.1", "1.2.3-nightly.42"));
        assert!(!tag_carries("v1.2.3-nightly.420.1", "1.2.3-nightly.42"));
        assert!(!tag_carries("v1.2.4", "1.2.3"));
        assert!(!tag_carries("1.2.3", "1.2.3"));
    }

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
