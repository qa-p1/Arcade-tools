//! Versioned documentation, explicit fetching, and an in-memory search index.
//! The desktop layer supplies the bundled resource directory. Every filesystem
//! or network API is synchronous worker work; resolving/searching never starts
//! an implicit fetch, watcher, timer or process.
use crate::catalog::{allowed_repo, read_bounded, relative_path, valid_app_id, CatalogApp};
use crate::paths::Paths;
use crate::source::{Fetch, HttpSource};
use crate::{Error, Result};
use pulldown_cmark::{Event, Parser};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_SNAPSHOT: u64 = 8_000_000;
const MAX_FILES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub schema: u32,
    pub entries: Vec<SnapshotEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub id: String,
    pub repo: String,
    pub version: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub status: String,
    #[serde(default)]
    pub files: Vec<DocFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
impl SnapshotManifest {
    /// Parse a bounded snapshot manifest in memory, safe on any thread. Unknown
    /// fields are ignored; paths, IDs, schema and the content budget are checked.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if value.get("schema").and_then(|v| v.as_u64()) != Some(1) {
            return Err(Error::new(
                "schema",
                "Only docs snapshot schema 1 is supported.",
            ));
        }
        let manifest: Self = serde_json::from_value(value)?;
        let mut size = bytes.len() as u64;
        let mut ids = BTreeSet::new();
        for entry in &manifest.entries {
            validate_location(&entry.id, &entry.version)?;
            allowed_repo(&entry.repo)?;
            if !ids.insert((&entry.id, &entry.version)) || entry.files.len() > MAX_FILES {
                return Err(Error::new(
                    "docs",
                    "Duplicate or oversized docs snapshot entry.",
                ));
            }
            let mut files = BTreeSet::new();
            for file in &entry.files {
                relative_path(&file.path)?;
                if !files.insert(&file.path)
                    || file.sha256.len() != 64
                    || !file
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(Error::new("docs", "Invalid docs file entry."));
                }
                size = size
                    .checked_add(file.size)
                    .ok_or_else(|| Error::new("too_large", "Docs size overflow."))?;
            }
        }
        if size > MAX_SNAPSHOT {
            return Err(Error::new("too_large", "Docs snapshot exceeds 8 MB."));
        }
        Ok(manifest)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DocSource {
    Installed,
    Cached,
    Bundled,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocLocation {
    pub id: String,
    pub version: String,
    pub root: PathBuf,
    pub source: DocSource,
    /// Caller can offer an explicit fetch when only another version is bundled.
    pub needs_fetch: bool,
}
pub struct DocsResolver {
    paths: Paths,
    bundled: PathBuf,
}
impl DocsResolver {
    /// Construct without I/O; safe on any thread. `bundled` is the desktop's
    /// resolved `resources/docs` directory, rather than a development-only path.
    pub fn new(paths: Paths, bundled: PathBuf) -> Self {
        Self { paths, bundled }
    }
    /// Resolve installed docs first, then exact-version cache, then a bundled
    /// snapshot on a worker thread. Never fetches. The optional installed root
    /// is the app's actual `docs/user` directory, supplied by its install contract.
    pub fn resolve(
        &self,
        id: &str,
        version: Option<&str>,
        installed_root: Option<&Path>,
    ) -> Result<Option<DocLocation>> {
        validate_location(id, version.unwrap_or("local"))?;
        if let Some(version) = version {
            if let Some(root) = installed_root {
                if has_markdown(root)? {
                    return Ok(Some(location(
                        id,
                        version,
                        root,
                        DocSource::Installed,
                        false,
                    )));
                }
            }
            let cached = self.paths.docs().join(id).join(version);
            if has_markdown(&cached)? {
                return Ok(Some(location(
                    id,
                    version,
                    &cached,
                    DocSource::Cached,
                    false,
                )));
            }
        }
        let manifest_path = self.bundled.join("manifest.json");
        let bytes = match read_bounded(&manifest_path, MAX_SNAPSHOT) {
            Ok(bytes) => bytes,
            Err(error) if !manifest_path.exists() && error.code == "io" => return Ok(None),
            Err(error) => return Err(error),
        };
        let manifest = SnapshotManifest::parse(&bytes)?;
        let mut entries = manifest
            .entries
            .iter()
            .filter(|entry| entry.id == id && entry.status == "snapshotted")
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            let exact_a = version == Some(a.version.as_str());
            let exact_b = version == Some(b.version.as_str());
            exact_b.cmp(&exact_a).then_with(|| {
                semver::Version::parse(&b.version)
                    .ok()
                    .cmp(&semver::Version::parse(&a.version).ok())
            })
        });
        for entry in entries {
            let root = self.bundled.join(id).join(&entry.version);
            if has_markdown(&root)? {
                return Ok(Some(location(
                    id,
                    &entry.version,
                    &root,
                    DocSource::Bundled,
                    version.is_some_and(|v| v != entry.version),
                )));
            }
        }
        Ok(None)
    }
    /// Explicit repository/tag fetch and atomic cache replacement, on a worker
    /// thread. Use in response to Learn/navigation or a confirmed docs refresh.
    /// Fetches `docs/user` even when the catalog temporarily points to README.
    /// Missing docs return an error; the existing cache/snapshot stays available.
    pub fn fetch_version(
        &self,
        app: &CatalogApp,
        version: &str,
        source: &impl HttpSource,
    ) -> Result<DocLocation> {
        validate_location(&app.id, version)?;
        allowed_repo(&app.repo)?;
        crate::catalog::parse_version(version)?;
        let reference = format!("v{version}");
        let parent = self.paths.docs().join(&app.id);
        self.paths.guard(&parent)?;
        fs::create_dir_all(&parent)?;
        let staged = tempfile::Builder::new()
            .prefix(".docs-")
            .tempdir_in(&parent)?;
        let mut pending = vec!["docs/user".to_owned()];
        let mut seen = BTreeSet::new();
        let mut entry = SnapshotEntry {
            id: app.id.clone(),
            repo: app.repo.clone(),
            version: version.into(),
            reference: reference.clone(),
            status: "snapshotted".into(),
            files: Vec::new(),
        };
        let mut size = 0u64;
        while let Some(directory) = pending.pop() {
            if !seen.insert(directory.clone()) || seen.len() > MAX_FILES {
                return Err(Error::new("too_large", "Invalid or oversized docs tree."));
            }
            let mut api = Url::parse(&format!(
                "https://api.github.com/repos/{}/contents/",
                app.repo
            ))
            .unwrap();
            api.path_segments_mut()
                .unwrap()
                .pop_if_empty()
                .extend(directory.split('/'));
            api.query_pairs_mut().append_pair("ref", &reference);
            let bytes = modified(source.fetch(api.as_str(), None, 1024 * 1024)?)?;
            let listing: Vec<ContentsEntry> = serde_json::from_slice(&bytes)?;
            for item in listing {
                relative_path(&item.path)?;
                if Path::new(&item.path).parent() != Some(Path::new(&directory)) {
                    return Err(Error::new(
                        "docs",
                        "Docs response escaped the requested directory.",
                    ));
                }
                if item.kind == "dir" {
                    pending.push(item.path);
                    continue;
                }
                if item.kind != "file" {
                    return Err(Error::new(
                        "docs",
                        "Docs cannot contain symlinks or submodules.",
                    ));
                }
                if !doc_extension(Path::new(&item.path)) {
                    continue;
                }
                if entry.files.len() >= MAX_FILES || item.size > MAX_SNAPSHOT.saturating_sub(size) {
                    return Err(Error::new("too_large", "Docs cache exceeds 8 MB."));
                }
                let relative = item
                    .path
                    .strip_prefix("docs/user/")
                    .ok_or_else(|| Error::new("docs", "Unsafe docs file path."))?;
                let mut raw =
                    Url::parse(&format!("https://raw.githubusercontent.com/{}/", app.repo))
                        .unwrap();
                raw.path_segments_mut()
                    .unwrap()
                    .pop_if_empty()
                    .push(&reference)
                    .extend(item.path.split('/'));
                let bytes = modified(source.fetch(raw.as_str(), None, MAX_SNAPSHOT - size)?)?;
                if bytes.len() as u64 != item.size {
                    return Err(Error::new(
                        "docs",
                        "Docs size differs from GitHub metadata.",
                    ));
                }
                size += bytes.len() as u64;
                let target = staged.path().join(relative);
                fs::create_dir_all(target.parent().unwrap())?;
                fs::write(target, &bytes)?;
                entry.files.push(DocFile {
                    path: relative.into(),
                    size: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                });
            }
        }
        if !has_markdown(staged.path())? {
            return Err(Error::new(
                "unavailable",
                "This tag has no user markdown docs.",
            ));
        }
        let manifest = serde_json::to_vec_pretty(&SnapshotManifest {
            schema: 1,
            entries: vec![entry],
        })?;
        if size + manifest.len() as u64 > MAX_SNAPSHOT {
            return Err(Error::new(
                "too_large",
                "Docs cache plus manifest exceeds 8 MB.",
            ));
        }
        fs::write(staged.path().join("manifest.json"), manifest)?;
        let destination = parent.join(version);
        self.paths.guard(&destination)?;
        let backup = tempfile::Builder::new()
            .prefix(".docs-previous-")
            .tempdir_in(&parent)?;
        let previous = backup.path().join("previous");
        if destination.exists() {
            fs::rename(&destination, &previous)?;
        }
        if let Err(error) = fs::rename(staged.path(), &destination) {
            if previous.exists() {
                fs::rename(previous, &destination)?;
            }
            return Err(error.into());
        }
        Ok(location(
            &app.id,
            version,
            &destination,
            DocSource::Cached,
            false,
        ))
    }
}
#[derive(Deserialize)]
struct ContentsEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
}
fn modified(fetch: Fetch) -> Result<Vec<u8>> {
    match fetch {
        Fetch::Modified { bytes, .. } => Ok(bytes),
        Fetch::NotModified => Err(Error::new("docs", "Unexpected 304 on a fresh docs fetch.")),
    }
}
fn location(
    id: &str,
    version: &str,
    root: &Path,
    source: DocSource,
    needs_fetch: bool,
) -> DocLocation {
    DocLocation {
        id: id.into(),
        version: version.into(),
        root: root.into(),
        source,
        needs_fetch,
    }
}
fn validate_location(id: &str, version: &str) -> Result<()> {
    if !valid_app_id(id) {
        return Err(Error::new("docs", "Invalid docs app ID."));
    }
    relative_path(version)?;
    if version.contains('/') {
        return Err(Error::new("docs", "Invalid docs version directory."));
    }
    Ok(())
}
fn doc_extension(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        [
            "md", "txt", "json", "png", "jpg", "jpeg", "webp", "gif", "svg", "avif",
        ]
        .iter()
        .any(|allowed| ext.to_string_lossy().eq_ignore_ascii_case(allowed))
    })
}
fn has_markdown(root: &Path) -> Result<bool> {
    Ok(!markdown_files(root)?.is_empty())
}
fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut files = Vec::new();
    let mut visited = 0;
    while let Some((path, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_FILES || depth > 16 {
            return Err(Error::new(
                "too_large",
                "Docs directory exceeds scan limits.",
            ));
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push((entry?.path(), depth + 1));
            }
        } else if metadata.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub app: String,
    pub version: String,
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub score: usize,
}
#[derive(Debug, Default)]
pub struct SearchIndex {
    documents: Vec<SearchHit>,
    terms: BTreeMap<String, BTreeMap<usize, usize>>,
}
impl SearchIndex {
    /// Scan/read markdown and build an inverted index on a worker thread. Each
    /// supplied location is budgeted at 8 MB. Symlinks are never followed.
    pub fn build(locations: &[DocLocation]) -> Result<Self> {
        let mut index = Self::default();
        for location in locations {
            let mut bytes = 0u64;
            for path in markdown_files(&location.root)? {
                let content = read_bounded(&path, MAX_SNAPSHOT.saturating_sub(bytes))?;
                bytes += content.len() as u64;
                let markdown =
                    String::from_utf8(content).map_err(|e| Error::new("docs", e.to_string()))?;
                let (title, body) = document_text(&markdown, &path);
                let doc = index.documents.len();
                for token in tokens(&format!("{title} {body}")) {
                    *index
                        .terms
                        .entry(token)
                        .or_default()
                        .entry(doc)
                        .or_default() += 1;
                }
                let relative = path
                    .strip_prefix(&location.root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                index.documents.push(SearchHit {
                    app: location.id.clone(),
                    version: location.version.clone(),
                    path: relative,
                    title,
                    snippet: body.chars().take(220).collect(),
                    score: 0,
                });
            }
        }
        Ok(index)
    }
    /// In-memory prefix search, safe on any thread. Terms are ANDed; alternatives
    /// matching each prefix are ORed. No file reads, network or index rebuild.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let mut scores: Option<BTreeMap<usize, usize>> = None;
        for prefix in tokens(query) {
            let mut matches = BTreeMap::<usize, usize>::new();
            for (_, posting) in self
                .terms
                .range(prefix.clone()..)
                .take_while(|(term, _)| term.starts_with(&prefix))
            {
                for (&doc, &count) in posting {
                    *matches.entry(doc).or_default() += count;
                }
            }
            scores = Some(match scores {
                None => matches,
                Some(prior) => prior
                    .into_iter()
                    .filter_map(|(doc, score)| matches.get(&doc).map(|count| (doc, score + count)))
                    .collect(),
            });
        }
        let mut hits = scores
            .unwrap_or_default()
            .into_iter()
            .map(|(doc, score)| {
                let mut hit = self.documents[doc].clone();
                hit.score = score;
                hit
            })
            .collect::<Vec<_>>();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| (&a.app, &a.path).cmp(&(&b.app, &b.path)))
        });
        hits.truncate(limit);
        hits
    }
}
fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}
fn document_text(markdown: &str, path: &Path) -> (String, String) {
    let (front, body) = if let Some(rest) = markdown.strip_prefix("---\n") {
        rest.split_once("\n---\n").unwrap_or(("", markdown))
    } else {
        ("", markdown)
    };
    let title = front
        .lines()
        .find_map(|line| {
            line.strip_prefix("title:")
                .map(|s| s.trim().trim_matches(['\'', '"']).to_string())
        })
        .or_else(|| {
            body.lines().find_map(|line| {
                line.strip_prefix('#')
                    .map(|s| s.trim_start_matches('#').trim().to_string())
            })
        })
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let mut text = String::new();
    for event in Parser::new(body) {
        match event {
            Event::Text(s) | Event::Code(s) => {
                text.push_str(&s);
                text.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
    }
    (title, text)
}
