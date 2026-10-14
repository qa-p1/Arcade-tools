//! Local, redacted diagnostics archives. Nothing in this module uploads data.
use crate::catalog::Catalog;
use crate::manager::AppView;
use crate::paths::Paths;
use crate::{Error, Result};
use arcade_link::manifest::{self, ids};
use arcade_link::receipt::Store;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const IDS: &[&str] = &[
    ids::BOX,
    ids::CLIPBOARD,
    ids::FIND,
    ids::LENS,
    ids::LOOK,
    ids::SHELF,
    ids::TOOLS,
    ids::WHEEL,
];
const MAX_MANIFEST: u64 = 256 * 1024;
const MAX_RECEIPT: u64 = 128 * 1024;
const MAX_HISTORY: u64 = 1024 * 1024;
const MAX_LOG_FILES_PER_APP: usize = 12;
const MAX_LOG_FILE_BYTES: usize = 128 * 1024;
const MAX_LOG_BYTES_TOTAL: usize = 512 * 1024;
const MAX_LOG_LINES: usize = 160;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedArchive {
    pub filename: String,
    pub included_logs: usize,
    pub included_manifests: usize,
    pub included_receipts: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueApp {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsDocument {
    schema: u32,
    generated_at: String,
    tools_version: String,
    system: SystemInfo,
    apps: Vec<AppDiagnostic>,
    operation_history: Vec<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInfo {
    os: String,
    architecture: String,
    desktop: Option<String>,
    session_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppDiagnostic {
    id: String,
    version: Option<String>,
    installed: bool,
    endpoint: &'static str,
    manifest_included: bool,
    receipt_included: bool,
    logs_included: usize,
}

struct FileEntry {
    name: String,
    bytes: Vec<u8>,
}

/// Builds a diagnostics ZIP at the path selected by the user. Log discovery is
/// limited to each app's declared `logs` directory and direct regular files.
pub fn create_archive(
    paths: &Paths,
    app_views: &[AppView],
    destination: &Path,
) -> Result<SavedArchive> {
    validate_output_path(destination)?;
    let running: BTreeMap<_, _> = app_views
        .iter()
        .map(|app| (app.id.as_str(), app.running))
        .collect();
    let mut entries = Vec::new();
    let mut apps = Vec::new();
    let mut included_manifests = 0;
    let mut included_receipts = 0;
    let mut included_logs = 0;
    let receipts = Store::new(&paths.link);

    for id in IDS {
        let manifest_path = paths.link.manifest(id);
        let raw_manifest =
            read_regular_bounded(&paths.link.registry, &manifest_path, MAX_MANIFEST).ok();
        let mut version = None;
        let mut declared_logs = None;
        if let Some(bytes) = raw_manifest.as_ref() {
            if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
                version = value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                declared_logs = value
                    .get("logs")
                    .and_then(Value::as_str)
                    .map(|path| expand_home(path, &paths.home));
                let clean = sanitize_value(value, &paths.home, *id == ids::CLIPBOARD);
                entries.push(FileEntry {
                    name: format!("manifests/{id}.json"),
                    bytes: serde_json::to_vec_pretty(&clean)?,
                });
                included_manifests += 1;
            }
        }

        let receipt_path = receipts.path(id).ok();
        let mut receipt_included = false;
        if let Some(path) = receipt_path {
            if let Ok(bytes) = read_regular_bounded(receipts.dir(), &path, MAX_RECEIPT) {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    if value.get("id").and_then(Value::as_str) == Some(*id) {
                        if version.is_none() {
                            version = value
                                .get("version")
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                        }
                        let clean = sanitize_value(value, &paths.home, *id == ids::CLIPBOARD);
                        entries.push(FileEntry {
                            name: format!("receipts/{id}.json"),
                            bytes: serde_json::to_vec_pretty(&clean)?,
                        });
                        receipt_included = true;
                        included_receipts += 1;
                    }
                }
            }
        }

        let log_entries = declared_logs
            .as_deref()
            .map(|directory| collect_logs(directory, id, &paths.home))
            .unwrap_or_default();
        included_logs += log_entries.len();
        entries.extend(log_entries);
        let endpoint = endpoint_state(
            paths,
            id,
            running.get(id).copied().unwrap_or(*id == ids::TOOLS),
        );
        let installed =
            *id == ids::TOOLS || app_views.iter().any(|app| app.id == *id && app.installed);
        apps.push(AppDiagnostic {
            id: (*id).into(),
            version,
            installed,
            endpoint,
            manifest_included: raw_manifest.is_some(),
            receipt_included,
            logs_included: entries
                .iter()
                .filter(|entry| entry.name.starts_with(&format!("logs/{id}/")))
                .count(),
        });
    }

    let operation_history = read_history(paths);
    let document = DiagnosticsDocument {
        schema: 1,
        generated_at: manifest::now_rfc3339(),
        tools_version: env!("CARGO_PKG_VERSION").into(),
        system: SystemInfo {
            os: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            desktop: env_nonempty("XDG_CURRENT_DESKTOP"),
            session_type: env_nonempty("XDG_SESSION_TYPE"),
        },
        apps,
        operation_history,
    };
    entries.push(FileEntry {
        name: "diagnostics.json".into(),
        bytes: serde_json::to_vec_pretty(&document)?,
    });
    let bytes = write_zip(entries)?;
    write_selected_file(destination, &bytes)?;
    let filename = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("diagnostics.zip")
        .to_owned();
    Ok(SavedArchive {
        filename,
        included_logs,
        included_manifests,
        included_receipts,
    })
}

/// The issue form is opened locally. Tools never attaches or uploads the ZIP.
pub fn issue_apps() -> Vec<IssueApp> {
    let catalog = Catalog::snapshot();
    let mut apps: Vec<_> = catalog
        .apps
        .iter()
        .map(|app| IssueApp {
            id: app.id.clone(),
            name: app.name.clone(),
        })
        .collect();
    apps.push(IssueApp {
        id: catalog.tools.id,
        name: catalog.tools.name,
    });
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// Returns a GitHub issue form URL for an Arcade app, with a concise report
/// prompt. The user attaches the locally saved archive themselves.
pub fn issue_url(app_id: &str) -> Result<String> {
    let catalog = Catalog::snapshot();
    let repo = if app_id == ids::TOOLS {
        catalog.tools.repo
    } else {
        catalog
            .app(app_id)
            .ok_or_else(|| Error::new("app_id", "Choose an Arcade app from the list."))?
            .repo
            .clone()
    };
    crate::catalog::allowed_repo(&repo)?;
    let mut url = reqwest::Url::parse(&format!("https://github.com/{repo}/issues/new"))
        .map_err(|e| Error::new("url", e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("title", "Arcade diagnostics report")
        .append_pair(
            "body",
            "Please describe the problem and attach the diagnostics ZIP you created in Arcade Tools. The ZIP stays on your device until you attach it.",
        );
    Ok(url.into())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| redact_text(&value, Path::new("")))
}

fn endpoint_state(paths: &Paths, id: &str, running: bool) -> &'static str {
    let endpoint = paths.link.endpoint(id);
    match read_regular_bounded(&paths.link.runtime, &endpoint, 32 * 1024) {
        Ok(bytes) => {
            if serde_json::from_slice::<arcade_link::endpoint::EndpointInfo>(&bytes).is_err() {
                "invalid"
            } else if running {
                "running"
            } else {
                "not-responding"
            }
        }
        Err(_) if !endpoint.exists() => "missing",
        Err(_) => "unreadable",
    }
}

fn read_history(paths: &Paths) -> Vec<Value> {
    let path = paths.state().join("update-history.jsonl");
    let Ok(bytes) = read_regular_bounded(&paths.state(), &path, MAX_HISTORY) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<_> = text.lines().rev().take(MAX_LOG_LINES).collect();
    lines
        .into_iter()
        .rev()
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(value) => sanitize_value(value, &paths.home, false),
            Err(_) => Value::String(redact_text(line, &paths.home)),
        })
        .collect()
}

fn collect_logs(directory: &Path, id: &str, home: &Path) -> Vec<FileEntry> {
    let Ok(canonical_directory) = safe_directory(directory) else {
        return Vec::new();
    };
    let Ok(read_dir) = fs::read_dir(&canonical_directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read_dir
        .flatten()
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            if !kind.is_file() || kind.is_symlink() {
                return None;
            }
            let path = entry.path();
            let canonical = path.canonicalize().ok()?;
            canonical.starts_with(&canonical_directory).then_some(path)
        })
        .collect();
    paths.sort();
    let mut output = Vec::new();
    let mut total = 0;
    for (index, path) in paths.into_iter().take(MAX_LOG_FILES_PER_APP).enumerate() {
        if total >= MAX_LOG_BYTES_TOTAL {
            break;
        }
        let Ok(bytes) = read_tail(&canonical_directory, &path, MAX_LOG_FILE_BYTES) else {
            continue;
        };
        let remaining = MAX_LOG_BYTES_TOTAL - total;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<_> = text.lines().collect();
        if lines.len() > MAX_LOG_LINES {
            lines.drain(..lines.len() - MAX_LOG_LINES);
        }
        let sanitized = lines
            .into_iter()
            .map(|line| redact_text(line, home))
            .collect::<Vec<_>>()
            .join("\n");
        let sanitized = truncate_utf8(&sanitized, remaining);
        total += sanitized.len();
        output.push(FileEntry {
            name: format!("logs/{id}/{index:02}.txt"),
            bytes: sanitized.into_bytes(),
        });
    }
    output
}

fn read_tail(root: &Path, path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let canonical_root = safe_directory(root)?;
    let canonical_file = safe_regular_file(path)?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(Error::new(
            "paths",
            "Log file escaped its declared directory.",
        ));
    }
    let mut file = File::open(&canonical_file)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(len.saturating_sub(start) as usize);
    file.take(max_bytes as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn sanitize_value(mut value: Value, home: &Path, clipboard: bool) -> Value {
    match &mut value {
        Value::Object(object) => {
            let keys: Vec<_> = object.keys().cloned().collect();
            for key in keys {
                if sensitive_key(&key, clipboard) {
                    object.insert(key, Value::String("[REDACTED]".into()));
                } else if let Some(child) = object.remove(&key) {
                    object.insert(key, sanitize_value(child, home, clipboard));
                }
            }
            Value::Object(std::mem::take(object))
        }
        Value::Array(items) => Value::Array(
            std::mem::take(items)
                .into_iter()
                .map(|item| sanitize_value(item, home, clipboard))
                .collect(),
        ),
        Value::String(text) => Value::String(redact_text(text, home)),
        _ => value,
    }
}

fn sensitive_key(key: &str, clipboard: bool) -> bool {
    let normalized: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "authorization",
        "apikey",
        "accesskey",
        "privatekey",
        "deviceid",
        "deviceidentity",
        "machineid",
        "keyring",
        "identity",
        "serialnumber",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
        || clipboard
            && ["key", "keys", "history", "device", "identity"].contains(&normalized.as_str())
}

fn redaction_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(bearer\s+)[a-z0-9._~+/-]+=*",
            r"(?i)\b(?:gh[pousr]_[a-z0-9_]{20,}|github_pat_[a-z0-9_]{20,})\b",
            r#"(?i)(\b(?:[a-z0-9]+[-_])*(?:token|secret|password|passwd|credential|authorization|api[-_]?key|access[-_]?key|device[-_]?(?:id|identity)|machine[-_]?id)\b\s*[\"']?\s*[:=]\s*[\"']?)[^\s,\"';&}]+"#,
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("static redaction expression"))
        .collect()
    })
}

pub(crate) fn redact_text(input: &str, home: &Path) -> String {
    let mut text = input.to_owned();
    if home.is_absolute() {
        let home = home.to_string_lossy();
        if !home.is_empty() {
            text = text.replace(home.as_ref(), "~");
            // Windows paths also appear with forward slashes (C:/Users/…).
            if cfg!(windows) {
                text = text.replace(&home.replace('\\', "/"), "~");
            }
        }
    }
    for pattern in redaction_patterns() {
        if pattern.captures_len() > 1 {
            text = pattern.replace_all(&text, "$1[REDACTED]").into_owned();
        } else {
            text = pattern.replace_all(&text, "[REDACTED]").into_owned();
        }
    }
    text
}

pub(crate) fn has_secret_markers(input: &str) -> bool {
    redact_text(input, Path::new("")) != input
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        home.to_path_buf()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    }
}

fn safe_directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || has_symlink_component(path)? {
        return Err(Error::new(
            "paths",
            "Refusing a symlinked or relative directory.",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new("paths", "Declared log path is not a directory."));
    }
    Ok(path.canonicalize()?)
}

fn safe_regular_file(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || has_symlink_component(path)? {
        return Err(Error::new(
            "paths",
            "Refusing a symlinked or relative file.",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::new("paths", "Expected a regular file."));
    }
    Ok(path.canonicalize()?)
}

fn has_symlink_component(path: &Path) -> Result<bool> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            // A bare drive prefix (`C:` or canonical `\\?\C:`) is not a path
            // that can be stat'ed on its own; check from the root onward.
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                continue;
            }
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(part) => current.push(part),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(Error::new(
                    "paths",
                    "Parent path components are not allowed.",
                ));
            }
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() && !crate::paths::system_alias(&current) =>
            {
                return Ok(true)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

fn read_regular_bounded(root: &Path, path: &Path, max: u64) -> Result<Vec<u8>> {
    let canonical_root = safe_directory(root)?;
    let canonical_path = safe_regular_file(path)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(Error::new("paths", "File escaped its allowed directory."));
    }
    let metadata = fs::metadata(&canonical_path)?;
    if metadata.len() > max {
        return Err(Error::new(
            "size",
            "File exceeds the diagnostics size limit.",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(canonical_path)?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(Error::new(
            "size",
            "File exceeds the diagnostics size limit.",
        ));
    }
    Ok(bytes)
}

fn truncate_utf8(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub(crate) fn validate_output_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.extension().and_then(|ext| ext.to_str()) != Some("zip")
        || path.file_name().is_none()
    {
        return Err(Error::new("path", "Choose an absolute .zip file path."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("path", "Choose a destination folder."))?;
    if !parent.is_dir() || has_symlink_component(parent)? {
        return Err(Error::new(
            "path",
            "The destination folder is missing or contains a symlink.",
        ));
    }
    Ok(())
}

pub(crate) fn write_selected_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("path", "Choose a destination folder."))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| Error::new("write", error.error.to_string()))?;
    Ok(())
}

fn write_zip(entries: Vec<FileEntry>) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for entry in entries {
        writer
            .start_file(entry.name, options)
            .map_err(|error| Error::new("archive", error.to_string()))?;
        writer
            .write_all(&entry.bytes)
            .map_err(|error| Error::new("archive", error.to_string()))?;
    }
    let cursor = writer
        .finish()
        .map_err(|error| Error::new("archive", error.to_string()))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn redacts_home_tokens_secrets_and_clipboard_device_fields() {
        let (home, settings) = if cfg!(windows) {
            (
                Path::new(r"C:\Users\alice"),
                r"C:\Users\alice\data\settings.json",
            )
        } else {
            (Path::new("/home/alice"), "/home/alice/data/settings.json")
        };
        let value = sanitize_value(
            json!({
                "path": settings,
                "apiToken": "visible-secret",
                "nested": { "deviceId": "machine-42", "safe": "Bearer abc.def-123" },
                "clipboard": { "device": "phone", "history": ["sensitive"] }
            }),
            home,
            true,
        );
        assert_eq!(
            value["path"],
            if cfg!(windows) {
                r"~\data\settings.json"
            } else {
                "~/data/settings.json"
            }
        );
        let text = value.to_string();
        assert!(!text.contains("visible-secret"));
        assert!(!text.contains("machine-42"));
        assert!(!text.contains("abc.def-123"));
        assert!(!text.contains("phone"));
        assert!(!text.contains("sensitive"));
    }

    #[test]
    fn only_reads_direct_regular_files_from_declared_log_directory() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        fs::write(logs.join("app.log"), "line 1\napi_token=secret\n").unwrap();
        let outside = root.path().join("outside.log");
        fs::write(&outside, "outside-secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, logs.join("escape.log")).unwrap();

        let entries = collect_logs(&logs, ids::FIND, root.path());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].name.starts_with("logs/arcade.find/"));
        let text = String::from_utf8_lossy(&entries[0].bytes);
        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("outside-secret"));
    }

    #[test]
    fn archives_contain_sanitized_documents_and_only_bounded_log_tail() {
        let root = tempdir().unwrap();
        let paths = Paths::under(root.path());
        fs::create_dir_all(&paths.link.registry).unwrap();
        fs::create_dir_all(&paths.link.runtime).unwrap();
        fs::create_dir_all(paths.state()).unwrap();
        let log_dir = root.path().join("app-logs");
        fs::create_dir(&log_dir).unwrap();
        fs::write(
            log_dir.join("main.log"),
            format!("{}\n", "z".repeat(MAX_LOG_FILE_BYTES + 100)),
        )
        .unwrap();
        fs::write(
            paths.link.manifest(ids::FIND),
            serde_json::to_vec(&json!({
                "schema": 1,
                "id": ids::FIND,
                "name": "Arcade Find",
                "version": "1.2.3",
                "executable": "/tmp/find",
                "logs": log_dir,
                "apiToken": "do-not-include"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            paths.state().join("update-history.jsonl"),
            "{\"app\":\"arcade.find\",\"reason\":\"Bearer abcdefghijklmnop\"}\n",
        )
        .unwrap();
        let output = root.path().join("diagnostics.zip");
        create_archive(&paths, &[], &output).unwrap();
        let file = File::open(output).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut manifest = String::new();
        archive
            .by_name("manifests/arcade.find.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert!(manifest.contains("[REDACTED]"));
        assert!(!manifest.contains("do-not-include"));
        let mut log = String::new();
        archive
            .by_name("logs/arcade.find/00.txt")
            .unwrap()
            .read_to_string(&mut log)
            .unwrap();
        assert!(log.len() <= MAX_LOG_FILE_BYTES);
    }
}
