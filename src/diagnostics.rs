//! Local, redacted reports. Diagnostics never uploads or reads app data stores.
use crate::archive;
use crate::catalog::read_bounded;
use crate::manager::{me, Manager};
use crate::{Error, Result};
use arcade_link::client::Client;
use arcade_link::manifest::ManifestDocument;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub file: PathBuf,
    pub issue_url: String,
    pub omitted: Vec<String>,
}

pub fn create(manager: &Manager, app: &str) -> Result<Report> {
    let catalog = manager.catalog()?;
    let entry = catalog
        .install_entry(app)
        .ok_or_else(|| Error::new("unsupported_input", "Choose an Arcade app for this report."))?;
    let mut files = BTreeMap::new();
    let mut omitted = Vec::new();
    let info = json!({"os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "osVersion": sysinfo::System::long_os_version(),
        "desktop": std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        "session": std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        "toolsVersion": env!("CARGO_PKG_VERSION"), "apps": manager.list(false)?,
        "health": manager.health()?});
    insert_json(manager, &mut files, "system.json", info)?;
    for id in crate::apps::APPS.into_iter().chain(["arcade.tools"]) {
        let manifest_path = manager.paths.link.manifest(id);
        if let Ok(bytes) = read_bounded(&manifest_path, 1024 * 1024) {
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => {
                    insert_json(manager, &mut files, &format!("manifests/{id}.json"), value)?
                }
                Err(_) => omitted.push(format!("{id}: unreadable manifest")),
            }
            if id != "arcade.clipboard" {
                if let Ok(document) = ManifestDocument::from_json(&String::from_utf8_lossy(&bytes))
                {
                    if let Some(logs) = document.additions.logs {
                        collect_logs(manager, id, Path::new(&logs), &mut files, &mut omitted)?;
                    }
                }
            } else {
                omitted.push(
                    "Clipboard logs omitted to exclude device data and clipboard contents.".into(),
                );
            }
        }
        if let Ok(Some(receipt)) = manager.receipt(id) {
            insert_json(
                manager,
                &mut files,
                &format!("receipts/{id}.json"),
                serde_json::to_value(receipt)?,
            )?;
        }
        let state = match Client::connect_with(
            &manager.paths.link,
            id,
            &me(),
            Duration::from_millis(250),
        ) {
            Ok(mut client) => match client.status() {
                Ok(status) => json!({"reachable":true, "busy":status.get("busy"),
                    "mode":status.pointer("/status/mode"), "tray":status.pointer("/status/tray")}),
                Err(error) => json!({"reachable":false, "error":error.code.as_str()}),
            },
            Err(error) => json!({"reachable":false, "error":error.code.as_str()}),
        };
        insert_json(manager, &mut files, &format!("endpoints/{id}.json"), state)?;
    }
    let history = manager.paths.state().join("update-history.jsonl");
    if let Ok(text) = tail(&history) {
        files.insert(
            "tools-history.jsonl".into(),
            redact_text(&text, &manager.paths.home).into_bytes(),
        );
    }
    insert_json(manager, &mut files, "omitted.json", json!(omitted))?;
    let name = archive::name("diagnostics");
    let file = archive::write(&manager.paths, &name, &files)?;
    let mut url =
        reqwest::Url::parse(&format!("https://github.com/{}/issues/new", entry.repo)).unwrap();
    url.query_pairs_mut()
        .append_pair("template", "bug_report.yml")
        .append_pair("title", &format!("[{}] ", entry.name))
        .append_pair(
            "diagnostics",
            &format!("Attach {} manually. Tools does not upload this file.", name),
        );
    Ok(Report {
        file,
        issue_url: url.into(),
        omitted,
    })
}

fn insert_json(
    manager: &Manager,
    files: &mut BTreeMap<String, Vec<u8>>,
    name: &str,
    mut value: Value,
) -> Result<()> {
    redact_value(&mut value, &manager.paths.home);
    files.insert(name.into(), serde_json::to_vec_pretty(&value)?);
    Ok(())
}

fn collect_logs(
    manager: &Manager,
    id: &str,
    root: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    omitted: &mut Vec<String>,
) -> Result<()> {
    // A manifest cannot redirect diagnostics to arbitrary system files.
    let allowed = manager
        .paths
        .data_folders(id, crate::release::Os::current());
    let canonical = root.canonicalize().ok();
    if canonical.as_ref().is_none_or(|path| {
        !allowed
            .iter()
            .any(|base| base.canonicalize().is_ok_and(|base| path.starts_with(base)))
    }) || manager.paths.guard(root).is_err()
    {
        omitted.push(format!(
            "{id}: log directory unavailable or outside app data"
        ));
        return Ok(());
    }
    let mut paths = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e == "log" || e == "txt")
        })
        .map(|e| e.path())
        .collect::<Vec<_>>();
    paths.sort();
    for (index, path) in paths.iter().rev().take(3).enumerate() {
        match tail(path) {
            Ok(text) => {
                files.insert(
                    format!("logs/{id}/{index}.log"),
                    redact_text(&text, &manager.paths.home).into_bytes(),
                );
            }
            Err(_) => omitted.push(format!("{id}: unreadable log")),
        }
    }
    Ok(())
}

fn tail(path: &Path) -> Result<String> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(Error::new(
            "paths",
            "Diagnostics does not follow log symlinks.",
        ));
    }
    let mut file = fs::File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(64 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Ok(text
        .lines()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['_', '-', ' '], "");
    [
        "token",
        "password",
        "secret",
        "credential",
        "authorization",
        "privatekey",
        "device",
        "identity",
        "pairing",
        "clipboardcontent",
    ]
    .iter()
    .any(|word| key.contains(word))
}

pub fn redact_value(value: &mut Value, home: &Path) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if sensitive_key(key) {
                    *value = json!("[redacted]");
                } else {
                    redact_value(value, home);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_value(value, home);
            }
        }
        Value::String(text) => *text = redact_text(text, home),
        _ => {}
    }
}

pub fn redact_text(text: &str, home: &Path) -> String {
    text.lines()
        .map(|line| {
            if sensitive_key(line) || line.contains("ghp_") || line.contains("github_pat_") {
                "[redacted sensitive line]".into()
            } else {
                line.replace(home.to_string_lossy().as_ref(), "~")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
