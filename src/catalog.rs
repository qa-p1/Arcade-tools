//! Catalog = what exists and how it is presented. `apps` and `platform` retain
//! the compiled installation contract. All I/O APIs are synchronous worker work;
//! loading a catalog never starts a timer or makes an implicit network request.
use crate::paths::Paths;
use crate::source::{Fetch, HttpSource, Source};
use crate::{Error, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/qa-p1/Arcade-catalog/main/catalog.json";
pub const ALLOWED_OWNERS: &[&str] = &["qa-p1"];
pub const MAX_DOCUMENT: u64 = 1024 * 1024;
pub const SNAPSHOT: &str = include_str!("../resources/catalog-snapshot/catalog.json");
pub const ENGINE_SNAPSHOT: &str = include_str!("../resources/catalog-snapshot/engines.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub pitch: String,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub screenshots: Vec<String>,
    pub platforms: BTreeMap<String, String>,
    pub link_protocol: Vec<u32>,
    pub min_tools: String,
    pub docs: String,
    pub resident: bool,
    pub homepage: String,
    #[serde(skip)]
    pub install_reason: Option<String>,
}
impl CatalogApp {
    /// Pure calculation, safe on any thread.
    pub fn releases_url(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }
    /// Pure calculation, safe on any thread. Unknown apps can be displayed but
    /// require a compiled install contract before the manager can modify them.
    pub fn supported_install(&self) -> bool {
        crate::apps::APPS.contains(&self.id.as_str()) || self.id == "arcade.tools"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsEntry {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub min_tools: String,
    pub icon: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BundleApps {
    All(String),
    Ids(Vec<String>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub id: String,
    pub title: String,
    pub description: String,
    pub apps: BundleApps,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    pub updated: String,
    pub apps: Vec<CatalogApp>,
    pub tools: ToolsEntry,
    pub bundles: Vec<Bundle>,
    #[serde(skip)]
    pub warnings: Vec<String>,
}
impl Catalog {
    /// Parse and filter in memory, safe on any thread. Future fields are ignored.
    pub fn parse(bytes: &[u8], tools_version: &str) -> Result<Self> {
        check_schema(bytes)?;
        let mut catalog: Self = serde_json::from_slice(bytes)?;
        let version = parse_version(tools_version)?;
        let mut ids = HashSet::new();
        let mut accepted = Vec::new();
        for mut app in catalog.apps {
            if let Err(error) = allowed_repo(&app.repo) {
                let warning = format!("Dropped catalog entry {}: {}", app.id, error.message);
                eprintln!("{warning}");
                catalog.warnings.push(warning);
                continue;
            }
            if !valid_app_id(&app.id) || !ids.insert(app.id.clone()) {
                return Err(Error::new(
                    "catalog",
                    "Invalid or duplicate catalog app ID.",
                ));
            }
            if app.name.is_empty()
                || app.pitch.is_empty()
                || app.link_protocol.is_empty()
                || app.link_protocol.contains(&0)
            {
                return Err(Error::new("catalog", "Incomplete catalog app metadata."));
            }
            relative_path(&app.icon)?;
            relative_path(&app.docs)?;
            if version
                .cmp_precedence(&parse_version(&app.min_tools)?)
                .is_lt()
            {
                app.install_reason = Some("Update Arcade Tools to install this".into());
            }
            accepted.push(app);
        }
        catalog.apps = accepted;
        allowed_repo(&catalog.tools.repo)?;
        parse_version(&catalog.tools.min_tools)?;
        if catalog.tools.id != "arcade.tools" {
            return Err(Error::new("catalog", "Invalid Tools catalog entry."));
        }
        for bundle in &mut catalog.bundles {
            match &mut bundle.apps {
                BundleApps::All(all) if all == "*" => {}
                BundleApps::Ids(apps) => apps.retain(|id| ids.contains(id)),
                _ => return Err(Error::new("catalog", "Invalid bundle app selector.")),
            }
        }
        Ok(catalog)
    }
    /// Parse the compiled snapshot in memory, safe on any thread.
    pub fn snapshot() -> Self {
        Self::parse(SNAPSHOT.as_bytes(), env!("CARGO_PKG_VERSION"))
            .expect("validated built-in catalog")
    }
    /// In-memory lookup, safe on any thread.
    pub fn app(&self, id: &str) -> Option<&CatalogApp> {
        self.apps.iter().find(|app| app.id == id)
    }
    /// Pure install metadata, safe on any thread. Tools has a separate catalog
    /// entry and participates in update discovery without an Apps-page row.
    pub fn install_entry(&self, id: &str) -> Option<CatalogApp> {
        if id != "arcade.tools" {
            return self.app(id).cloned();
        }
        Some(CatalogApp {
            id: self.tools.id.clone(),
            name: self.tools.name.clone(),
            repo: self.tools.repo.clone(),
            pitch: String::new(),
            description: String::new(),
            category: "tools".into(),
            icon: self.tools.icon.clone(),
            screenshots: Vec::new(),
            platforms: BTreeMap::new(),
            link_protocol: vec![1],
            min_tools: self.tools.min_tools.clone(),
            docs: "docs/user".into(),
            resident: true,
            homepage: format!("https://github.com/{}", self.tools.repo),
            install_reason: parse_version(env!("CARGO_PKG_VERSION"))
                .ok()
                .zip(parse_version(&self.tools.min_tools).ok())
                .filter(|(current, minimum)| current.cmp_precedence(minimum).is_lt())
                .map(|_| "Update Arcade Tools to install this".into()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutIndex {
    pub schema: u32,
    pub sheets: Vec<SheetEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetEntry {
    pub id: String,
    pub name: String,
    pub file: String,
    #[serde(rename = "match")]
    pub window_match: WindowMatch,
}
pub type WindowMatch = BTreeMap<String, BTreeMap<String, Vec<String>>>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutSheet {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub checked_version: String,
    pub sources: Vec<String>,
    #[serde(rename = "match")]
    pub window_match: WindowMatch,
    pub groups: Vec<ShortcutGroup>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutGroup {
    pub title: String,
    pub context: String,
    pub shortcuts: Vec<Shortcut>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shortcut {
    pub id: String,
    pub title: String,
    pub keys: BTreeMap<String, Option<Binding>>,
    #[serde(default)]
    pub description: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binding {
    Key(String),
    Alternatives(Vec<String>),
}

/// Cache envelope keeps a validator and its body together in one atomic write.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDocument {
    schema: u32,
    url: String,
    etag: Option<String>,
    body: String,
}
pub struct CatalogClient<S = Source> {
    paths: Paths,
    source: S,
    url: Url,
}
impl<S: HttpSource> CatalogClient<S> {
    /// Construct without I/O or network; safe on any thread. The debug-only
    /// override is validated by `Source` on every request, including redirects.
    pub fn new(paths: Paths, source: S) -> Result<Self> {
        #[cfg(debug_assertions)]
        let url = std::env::var("ARCADE_TOOLS_CATALOG_URL").unwrap_or_else(|_| CATALOG_URL.into());
        #[cfg(not(debug_assertions))]
        let url = CATALOG_URL.to_owned();
        Self::from_url(paths, source, &url)
    }
    #[cfg(debug_assertions)]
    /// Construct an explicit test URL without I/O; safe on any thread.
    pub fn with_url(paths: Paths, source: S, url: &str) -> Result<Self> {
        Self::from_url(paths, source, url)
    }
    fn from_url(paths: Paths, source: S, url: &str) -> Result<Self> {
        let url = Url::parse(url).map_err(|e| Error::new("catalog", e.to_string()))?;
        Ok(Self { paths, source, url })
    }
    fn read(&self, file: &str) -> Result<CachedDocument> {
        let bytes = read_bounded(&self.paths.catalog().join(file), MAX_DOCUMENT * 6)?;
        let cached: CachedDocument = serde_json::from_slice(&bytes)?;
        if cached.schema != 1 || cached.body.len() as u64 > MAX_DOCUMENT {
            return Err(Error::new("catalog", "Invalid catalog cache envelope."));
        }
        Ok(cached)
    }
    fn document<T>(
        &self,
        file: &str,
        url: &Url,
        fallback: Option<&str>,
        refresh: bool,
        parse: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<T> {
        let cached = self
            .read(file)
            .ok()
            .filter(|c| parse(c.body.as_bytes()).is_ok());
        if refresh {
            let etag = cached
                .as_ref()
                .filter(|c| c.url == url.as_str())
                .and_then(|c| c.etag.as_deref());
            let result = (|| match self.source.fetch(url.as_str(), etag, MAX_DOCUMENT)? {
                Fetch::NotModified => cached
                    .as_ref()
                    .map(|c| parse(c.body.as_bytes()))
                    .ok_or_else(|| Error::new("catalog", "304 without valid cache"))?,
                Fetch::Modified { bytes, etag } => {
                    let parsed = parse(&bytes)?;
                    let document = CachedDocument {
                        schema: 1,
                        url: url.to_string(),
                        etag,
                        body: String::from_utf8(bytes)
                            .map_err(|e| Error::new("catalog", e.to_string()))?,
                    };
                    let path = self.paths.catalog().join(file);
                    self.paths.guard(&path)?;
                    arcade_link::paths::write_atomic(&path, &serde_json::to_vec(&document)?, true)?;
                    Ok(parsed)
                }
            })();
            match result {
                Ok(parsed) => return Ok(parsed),
                Err(error) => eprintln!("Catalog {file}: {error}; using last good copy."),
            }
        }
        if let Some(cached) = cached {
            return parse(cached.body.as_bytes());
        }
        fallback
            .map(|text| parse(text.as_bytes()))
            .unwrap_or_else(|| {
                Err(Error::new(
                    "unavailable",
                    format!("No cached {file}; fetch it while online."),
                ))
            })
    }
    /// Disk read only, on a worker thread. Falls back to the built-in snapshot;
    /// never connects to the network.
    pub fn cached_catalog(&self) -> Catalog {
        self.document("catalog.json", &self.url, Some(SNAPSHOT), false, |bytes| {
            Catalog::parse(bytes, env!("CARGO_PKG_VERSION"))
        })
        .expect("validated built-in catalog")
    }
    /// Explicit network refresh and atomic cache write, on a worker thread.
    /// Offline/invalid responses preserve the last good catalog or snapshot.
    pub fn refresh_catalog(&self) -> Catalog {
        self.document("catalog.json", &self.url, Some(SNAPSHOT), true, |bytes| {
            Catalog::parse(bytes, env!("CARGO_PKG_VERSION"))
        })
        .expect("validated built-in catalog")
    }
    /// Disk/network I/O on a worker thread; refresh only when explicitly requested.
    pub fn engines(&self, refresh: bool) -> Result<crate::engines::EngineCatalog> {
        let url = self
            .url
            .join("engines.json")
            .map_err(|e| Error::new("catalog", e.to_string()))?;
        self.document(
            "engines.json",
            &url,
            Some(ENGINE_SNAPSHOT),
            refresh,
            crate::engines::EngineCatalog::parse,
        )
    }
    /// Disk/network I/O on a worker thread. Index and sheets are fetched on demand.
    pub fn shortcut_index(&self, refresh: bool) -> Result<ShortcutIndex> {
        let url = self
            .url
            .join("shortcuts/index.json")
            .map_err(|e| Error::new("catalog", e.to_string()))?;
        self.document("shortcuts/index.json", &url, None, refresh, |bytes| {
            check_schema(bytes)?;
            let index: ShortcutIndex = serde_json::from_slice(bytes)?;
            let mut ids = HashSet::new();
            for entry in &index.sheets {
                validate_sheet_id(&entry.id)?;
                if entry.file != format!("shortcuts/{}.json", entry.id) || !ids.insert(&entry.id) {
                    return Err(Error::new(
                        "catalog",
                        "Invalid shortcut sheet file or duplicate ID.",
                    ));
                }
            }
            Ok(index)
        })
    }
    /// Disk/network I/O on a worker thread. Only files in the trusted index are accepted.
    pub fn shortcut_sheet(&self, entry: &SheetEntry, refresh: bool) -> Result<ShortcutSheet> {
        validate_sheet_id(&entry.id)?;
        if entry.file != format!("shortcuts/{}.json", entry.id) {
            return Err(Error::new("catalog", "Unsafe shortcut sheet path."));
        }
        let url = self
            .url
            .join(&entry.file)
            .map_err(|e| Error::new("catalog", e.to_string()))?;
        self.document(&entry.file, &url, None, refresh, |bytes| {
            check_schema(bytes)?;
            let sheet: ShortcutSheet = serde_json::from_slice(bytes)?;
            if sheet.id != entry.id {
                return Err(Error::new("catalog", "Shortcut sheet ID mismatch."));
            }
            Ok(sheet)
        })
    }
}

pub(crate) fn check_schema(bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_DOCUMENT {
        return Err(Error::new("too_large", "Catalog metadata exceeds 1 MiB."));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value.get("schema").and_then(|v| v.as_u64()) != Some(1) {
        return Err(Error::new("schema", "Only catalog schema 1 is supported."));
    }
    Ok(())
}
pub(crate) fn parse_version(value: &str) -> Result<semver::Version> {
    semver::Version::parse(value).map_err(|e| Error::new("version", e.to_string()))
}
/// Validate a repository against the compiled owner allowlist, in memory.
/// Safe on any thread. This also prevents repository/path injection into URLs.
pub fn allowed_repo(repo: &str) -> Result<()> {
    let valid = repo.split_once('/').is_some_and(|(owner, name)| {
        ALLOWED_OWNERS.contains(&owner)
            && !name.is_empty()
            && !matches!(name, "." | "..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    });
    if !valid {
        return Err(Error::new(
            "owner",
            format!("Repository {repo} is not allowed."),
        ));
    }
    Ok(())
}
pub(crate) fn relative_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains(['\\', ':'])
        || value.chars().any(char::is_control)
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::new("paths", "Unsafe relative resource path."));
    }
    Ok(())
}
pub(crate) fn valid_app_id(id: &str) -> bool {
    id.strip_prefix("arcade.").is_some_and(|s| {
        s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn validate_sheet_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::new("catalog", "Invalid shortcut sheet ID."));
    }
    Ok(())
}
pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::new(
            "too_large",
            "Cached file exceeds its size limit.",
        ));
    }
    Ok(bytes)
}
