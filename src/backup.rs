//! Portable app-owned settings through feature-detected Link methods.
use crate::archive;
use crate::catalog::relative_path;
use crate::manager::{me, LaunchMode, Manager, Operation, Preferences, Request};
use crate::release::{checksum, Channel};
use crate::{Error, Result};
use arcade_link::client::Client;
use arcade_link::content::Content;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const LABELS: &str = "arcade/tools/shortcut-labels.json";
const MAX_SETTINGS: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupApp {
    pub id: String,
    pub version: String,
    pub channel: Channel,
    pub settings: Vec<String>,
    pub not_included: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema: u32,
    tools: Preferences,
    #[serde(default)]
    tray: Option<Value>,
    labels: BTreeMap<String, String>,
    apps: Vec<BackupApp>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub file: PathBuf,
    pub apps: Vec<BackupApp>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreApp {
    pub backup: BackupApp,
    pub installed: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlan {
    pub sha256: String,
    pub apps: Vec<RestoreApp>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub app: String,
    pub imported: bool,
    pub restart_required: bool,
    pub message: String,
}

pub fn export(manager: &Manager, cancel: &AtomicBool) -> Result<BackupReport> {
    let mut files = BTreeMap::new();
    let mut apps = Vec::new();
    for app in manager.list(false)?.into_iter().filter(|app| app.installed) {
        cancelled(cancel)?;
        let mut backup = BackupApp {
            id: app.id.clone(),
            version: app.version.unwrap_or_default(),
            channel: app.channel,
            settings: Vec::new(),
            not_included: None,
        };
        let result = (|| -> Result<Vec<(String, Vec<u8>)>> {
            let mut client = connect(manager, &app.id, cancel)?;
            if !client.describe_full()?.supports("app.settings.export") {
                return Err(Error::new(
                    "unsupported",
                    "App does not offer portable settings export.",
                ));
            }
            let outputs = client.settings_export()?;
            if outputs.is_empty() || outputs.len() > 4 {
                return Err(Error::new(
                    "backup",
                    "App returned an invalid settings file count.",
                ));
            }
            let handoff = manager.paths.link.handoff.canonicalize()?;
            outputs
                .into_iter()
                .enumerate()
                .map(|(index, output)| {
                    cancelled(cancel)?;
                    if output.kind != "file/any"
                        || output.owner.as_deref().is_some_and(|owner| owner != app.id)
                    {
                        return Err(Error::new(
                            "backup",
                            "App returned an invalid settings handoff.",
                        ));
                    }
                    let path = PathBuf::from(
                        output
                            .path
                            .ok_or_else(|| Error::new("backup", "Missing handoff path."))?,
                    );
                    manager.paths.guard(&path)?;
                    if !path.canonicalize()?.starts_with(&handoff)
                        || fs::symlink_metadata(&path)?.file_type().is_symlink()
                    {
                        return Err(Error::new(
                            "backup",
                            "Settings export escaped the Link handoff directory.",
                        ));
                    }
                    let bytes = crate::catalog::read_bounded(&path, MAX_SETTINGS)?;
                    validate_settings(&bytes)?;
                    Ok((format!("settings/{}/{index}.json", app.id), bytes))
                })
                .collect()
        })();
        match result {
            Ok(settings) => {
                for (path, bytes) in settings {
                    backup.settings.push(path.clone());
                    files.insert(path, bytes);
                }
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => backup.not_included = Some(error.message),
        }
        apps.push(backup);
    }
    let labels_path = manager.paths.config.join(LABELS);
    let labels = match crate::catalog::read_bounded(&labels_path, 1024 * 1024) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if !labels_path.exists() => {
            let _ = error;
            BTreeMap::new()
        }
        Err(error) => return Err(error),
    };
    let manifest = Manifest {
        schema: 1,
        tools: manager.preferences()?,
        tray: read_tray(manager)?,
        labels,
        apps: apps.clone(),
    };
    files.insert("backup.json".into(), serde_json::to_vec_pretty(&manifest)?);
    cancelled(cancel)?;
    let name = archive::name("backup");
    let file = archive::write(&manager.paths, &name, &files)?;
    Ok(BackupReport { file, apps })
}

pub fn plan_restore(manager: &Manager, path: &Path) -> Result<RestorePlan> {
    let (_, manifest) = read(manager, path)?;
    let installed = manager.list(false)?;
    Ok(RestorePlan {
        sha256: checksum(path)?,
        apps: manifest
            .apps
            .into_iter()
            .map(|backup| {
                let present = installed
                    .iter()
                    .any(|app| app.id == backup.id && app.installed);
                RestoreApp {
                    backup,
                    installed: present,
                }
            })
            .collect(),
    })
}

pub fn restore(
    manager: &Manager,
    path: &Path,
    expected_sha256: &str,
    cancel: &AtomicBool,
) -> Result<Vec<RestoreResult>> {
    if checksum(path)? != expected_sha256 {
        return Err(Error::new(
            "state_changed",
            "Backup changed after review; review it again.",
        ));
    }
    let (files, manifest) = read(manager, path)?;
    cancelled(cancel)?;
    manager.restore_preferences(&manifest.tools)?;
    if let Some(tray) = &manifest.tray {
        let path = manager.paths.state().join("tray-preferences.json");
        manager.paths.guard(&path)?;
        arcade_link::paths::write_atomic(&path, &serde_json::to_vec_pretty(tray)?, true)?;
    }
    let label_path = manager.paths.config.join(LABELS);
    manager.paths.guard(&label_path)?;
    arcade_link::paths::write_atomic(
        &label_path,
        &serde_json::to_vec_pretty(&manifest.labels)?,
        true,
    )?;
    let mut results = Vec::new();
    for app in manifest.apps {
        cancelled(cancel)?;
        let result = (|| -> Result<arcade_link::app::ImportResult> {
            if !manager
                .list(false)?
                .iter()
                .any(|installed| installed.id == app.id && installed.installed)
            {
                manager.set_channel(&app.id, app.channel)?;
                manager.operate(
                    &Request {
                        id: app.id.clone(),
                        operation: Operation::Install,
                        mode: Some(LaunchMode::Background),
                        remove_data: false,
                        app_closed: false,
                    },
                    cancel,
                    &mut |_| {},
                )?;
            }
            if app.settings.is_empty() {
                return Err(Error::new(
                    "not_included",
                    app.not_included
                        .clone()
                        .unwrap_or_else(|| "No settings were included.".into()),
                ));
            }
            let mut client = connect(manager, &app.id, cancel)?;
            if !client.describe_full()?.supports("app.settings.import") {
                return Err(Error::new(
                    "unsupported",
                    "App does not offer portable settings import.",
                ));
            }
            let directory = manager.paths.link.handoff.join("arcade.tools-backup");
            manager.paths.guard(&directory)?;
            arcade_link::paths::ensure_private_dir(&directory)?;
            let staged = tempfile::Builder::new()
                .prefix("restore-")
                .tempdir_in(&directory)?;
            let mut inputs = Vec::new();
            for (index, name) in app.settings.iter().enumerate() {
                let path = staged.path().join(format!("{index}.json"));
                arcade_link::paths::write_atomic(&path, &files[name], true)?;
                let mut content = Content::file(&path).with_owner("arcade.tools");
                content.kind = "file/any".into();
                inputs.push(content);
            }
            client.settings_import(&inputs).map_err(Into::into)
        })();
        match result {
            Ok(result) => results.push(RestoreResult {
                app: app.id,
                imported: result.imported,
                restart_required: result.restart_required,
                message: if result.imported {
                    "Settings restored."
                } else {
                    "App declined the settings import."
                }
                .into(),
            }),
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => results.push(RestoreResult {
                app: app.id,
                imported: false,
                restart_required: false,
                message: error.message,
            }),
        }
    }
    Ok(results)
}

fn read(manager: &Manager, path: &Path) -> Result<(BTreeMap<String, Vec<u8>>, Manifest)> {
    let files = archive::read(&manager.paths, path)?;
    let bytes = files
        .get("backup.json")
        .ok_or_else(|| Error::new("backup", "This is not an Arcade settings backup."))?;
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    if manifest.schema != 1
        || manifest.apps.len() > crate::apps::APPS.len()
        || manifest.labels.len() > 4096
    {
        return Err(Error::new(
            "schema",
            "Unsupported or oversized backup manifest.",
        ));
    }
    manager.validate_preferences(&manifest.tools)?;
    if let Some(tray) = &manifest.tray {
        validate_tray(tray)?;
    }
    if manifest
        .labels
        .iter()
        .any(|(key, label)| key.len() > 1024 || label.len() > 512)
    {
        return Err(Error::new("backup", "Invalid shortcut labels."));
    }
    let mut names = BTreeSet::from(["backup.json".to_string()]);
    let mut ids = BTreeSet::new();
    for app in &manifest.apps {
        crate::paths::app_id(&app.id)?;
        if !ids.insert(&app.id) || app.settings.len() > 4 {
            return Err(Error::new(
                "backup",
                "Duplicate app or too many settings files.",
            ));
        }
        for name in &app.settings {
            relative_path(name)?;
            if !name.starts_with(&format!("settings/{}/", app.id)) || !names.insert(name.clone()) {
                return Err(Error::new("backup", "Invalid settings file ownership."));
            }
            validate_settings(
                files
                    .get(name)
                    .ok_or_else(|| Error::new("backup", "Settings file is missing."))?,
            )?;
        }
    }
    if files.keys().any(|name| !names.contains(name)) {
        return Err(Error::new("backup", "Backup contains unlisted files."));
    }
    Ok((files, manifest))
}

fn connect(manager: &Manager, id: &str, cancel: &AtomicBool) -> Result<Client> {
    if let Ok(client) = Client::connect(&manager.paths.link, id, &me()) {
        return Ok(client);
    }
    let registry = arcade_link::registry::Registry::load(&manager.paths.link);
    if registry
        .get(id)
        .is_some_and(|peer| !peer.settings.link_enabled)
    {
        return Err(Error::new(
            "disabled",
            "Enable Arcade Link in the app to include its settings.",
        ));
    }
    manager.launch(id, LaunchMode::Background)?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        cancelled(cancel)?;
        if let Ok(client) =
            Client::connect_with(&manager.paths.link, id, &me(), Duration::from_millis(250))
        {
            return Ok(client);
        }
        if Instant::now() >= deadline {
            return Err(Error::new(
                "timeout",
                "App did not offer its Link endpoint after launch.",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn validate_settings(bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_SETTINGS {
        return Err(Error::new("too_large", "Settings handoff exceeds 8 MiB."));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    fn forbidden(value: &Value) -> bool {
        match value {
            Value::Object(object) => object.iter().any(|(key, value)| {
                let key = key.to_ascii_lowercase().replace(['_', '-'], "");
                let protected = [
                    "token",
                    "password",
                    "secret",
                    "credential",
                    "privatekey",
                    "deviceidentity",
                    "devicekeys",
                    "pairingkey",
                ];
                protected.iter().any(|word| key.contains(word))
                    || matches!(
                        key.as_str(),
                        "history" | "index" | "cache" | "caches" | "identity" | "keys" | "devices"
                    )
                    || forbidden(value)
            }),
            Value::Array(values) => values.iter().any(forbidden),
            Value::String(value) => crate::diagnostics_archive::has_secret_markers(value),
            _ => false,
        }
    }
    if forbidden(&value) {
        return Err(Error::new(
            "privacy",
            "Settings contain excluded identity, secrets, history, index or cache fields.",
        ));
    }
    Ok(())
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(Error::new("cancelled", "Backup or restore cancelled."))
    } else {
        Ok(())
    }
}

fn read_tray(manager: &Manager) -> Result<Option<Value>> {
    let path = manager.paths.state().join("tray-preferences.json");
    match crate::catalog::read_bounded(&path, 64 * 1024) {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes)?;
            validate_tray(&value)?;
            Ok(Some(value))
        }
        Err(_) if !path.exists() => Ok(None),
        Err(error) => Err(error),
    }
}
fn validate_tray(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::new("backup", "Invalid tray preferences."))?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "hostIcons" | "showUpdateBadge" | "startAtLogin" | "shortcutsEnabled" => {
                value.is_boolean()
            }
            "excluded" => value.as_array().is_some_and(|ids| {
                ids.len() <= 7
                    && ids.iter().all(|id| {
                        id.as_str()
                            .is_some_and(|id| crate::apps::APPS.contains(&id))
                    })
            }),
            "shortcut" => value
                .as_str()
                .is_some_and(|key| arcade_link::accelerator::normalize(key).is_ok()),
            _ => false,
        };
        if !valid {
            return Err(Error::new("backup", "Invalid tray preference in backup."));
        }
    }
    Ok(())
}
