//! Portable Tools backup and confirmed restore through the existing manager
//! and Arcade Link settings handoff APIs.
use crate::manager::{AppView, LaunchMode, Manager, Operation, Preferences, Request};
use crate::paths::Paths;
use crate::release::Channel;
use crate::{Error, Result};
use arcade_link::client::Client;
use arcade_link::content::Content;
use arcade_link::handoff::Handoff;
use arcade_link::manifest::ids;
use arcade_link::wire::PeerInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const BACKUP_SCHEMA: u32 = 1;
const BACKUP_KIND: &str = "arcade-tools-backup";
const BACKUP_DOCUMENT: &str = "backup.json";
const LABELS_PATH: &str = "arcade/tools/shortcut-labels.json";
const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 64;
const MAX_TOTAL_UNCOMPRESSED: usize = 8 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 2 * 1024 * 1024;
const MAX_SETTINGS_BYTES: usize = 1024 * 1024;
const MAX_EXPORT_BYTES: usize = 6 * 1024 * 1024;
const MAX_SETTINGS_FILES: usize = 32;
const MAX_SETTINGS_PER_APP: usize = 8;
const MAX_EXPANSION_RATIO: u64 = 200;
const MAX_PLAN_AGE: Duration = Duration::from_secs(10 * 60);
const MAX_PENDING_PLANS: usize = 8;
const OPAQUE_ENCODING: &str = "opaque";
const INSTALLABLE_APPS: &[&str] = &[
    ids::BOX,
    ids::CLIPBOARD,
    ids::FIND,
    ids::LENS,
    ids::LOOK,
    ids::SHELF,
    ids::WHEEL,
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedBackup {
    pub filename: String,
    pub included_apps: usize,
    pub included_settings: usize,
    pub not_included: Vec<NotIncluded>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackupDocument {
    schema: u32,
    kind: String,
    created_at: String,
    tools_version: String,
    tools_preferences: Preferences,
    shortcut_labels: BTreeMap<String, String>,
    installed_apps: Vec<InstalledApp>,
    settings_files: Vec<SettingsFile>,
    not_included: Vec<NotIncluded>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledApp {
    id: String,
    version: Option<String>,
    channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsFile {
    app_id: String,
    archive_path: String,
    filename: String,
    content_type: String,
    /// `None` is a JSON export (the original format, scanned for excluded
    /// data); `opaque` is a non-JSON export such as Shelf's
    /// `ARCADE-SHELF-PORTABLE` file, stored byte-for-byte and handed back to
    /// the app's import unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

impl SettingsFile {
    fn is_opaque(&self) -> bool {
        self.encoding.as_deref() == Some(OPAQUE_ENCODING)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotIncluded {
    pub app_id: String,
    pub reason: String,
}

struct ArchiveEntry {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreInstall {
    pub app_id: String,
    pub version: Option<String>,
    pub channel: Channel,
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSettings {
    pub app_id: String,
    pub files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub plan_id: String,
    pub installs: Vec<RestoreInstall>,
    pub settings: Vec<RestoreSettings>,
    pub not_included: Vec<NotIncluded>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRestoreResult {
    pub app_id: String,
    pub installed: bool,
    pub imported: bool,
    pub restart_required: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub tools_settings_restored: bool,
    pub apps: Vec<AppRestoreResult>,
    pub not_included: Vec<NotIncluded>,
}

struct ValidatedBackup {
    document: BackupDocument,
    files: BTreeMap<String, Vec<u8>>,
}

struct PendingPlan {
    created: Instant,
    backup: ValidatedBackup,
}

static PENDING_PLANS: OnceLock<Mutex<HashMap<String, PendingPlan>>> = OnceLock::new();

/// Creates a backup ZIP from manager-owned settings and app Link handoffs.
/// Calls are synchronous and must run on a worker thread.
pub fn create_archive(manager: &Manager, destination: &Path) -> Result<SavedBackup> {
    crate::diagnostics_archive::validate_output_path(destination)?;
    let views = manager.list(true)?;
    let preferences = manager.preferences()?;
    let shortcut_labels = read_shortcut_labels(&manager.paths)?;
    let installed_apps: Vec<_> = views
        .iter()
        .filter(|app| app.installed)
        .map(|app| InstalledApp {
            id: app.id.clone(),
            version: app.version.clone(),
            channel: app.channel,
        })
        .collect();
    let (settings_files, mut entries, mut not_included) =
        export_app_settings(manager, &views, &installed_apps)?;
    for app in &installed_apps {
        if !settings_files.iter().any(|file| file.app_id == app.id)
            && !not_included.iter().any(|entry| entry.app_id == app.id)
        {
            not_included.push(NotIncluded {
                app_id: app.id.clone(),
                reason: "The app did not provide portable settings.".into(),
            });
        }
    }
    let document = BackupDocument {
        schema: BACKUP_SCHEMA,
        kind: BACKUP_KIND.into(),
        created_at: arcade_link::manifest::now_rfc3339(),
        tools_version: env!("CARGO_PKG_VERSION").into(),
        tools_preferences: preferences,
        shortcut_labels,
        installed_apps,
        settings_files,
        not_included: not_included.clone(),
    };
    entries.push(ArchiveEntry {
        name: "tools/shortcut-labels.json".into(),
        bytes: serde_json::to_vec_pretty(&document.shortcut_labels)?,
    });
    entries.push(ArchiveEntry {
        name: BACKUP_DOCUMENT.into(),
        bytes: serde_json::to_vec_pretty(&document)?,
    });
    let bytes = write_zip(entries)?;
    // Validate our own output before making it visible to the user.
    let validated = validate_archive(&bytes)?;
    let settings_count = validated.document.settings_files.len();
    crate::diagnostics_archive::write_selected_file(destination, &bytes)?;
    let filename = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("arcade-backup.zip")
        .to_owned();
    Ok(SavedBackup {
        filename,
        included_apps: installed_apps_count(&validated.document),
        included_settings: settings_count,
        not_included,
    })
}

/// Parses and validates a user-selected backup completely before returning a
/// confirmation plan. No filesystem or manager mutation occurs here.
pub fn preview_restore(manager: &Manager, archive_path: &Path) -> Result<RestorePreview> {
    let bytes = read_archive_file(archive_path)?;
    let backup = validate_archive(&bytes)?;
    let installed: BTreeSet<_> = manager
        .list(false)?
        .into_iter()
        .filter(|app| app.installed)
        .map(|app| app.id)
        .collect();
    let installs: Vec<_> = backup
        .document
        .installed_apps
        .iter()
        .filter(|app| !installed.contains(&app.id))
        .map(|app| RestoreInstall {
            app_id: app.id.clone(),
            version: app.version.clone(),
            channel: app.channel,
            supported: INSTALLABLE_APPS.contains(&app.id.as_str()),
        })
        .collect();
    let settings = settings_summary(&backup.document.settings_files);
    let not_included = backup.document.not_included.clone();
    let plan_id = arcade_link::endpoint::new_token()
        .map_err(|error| Error::new("random", error.to_string()))?;
    store_plan(&plan_id, backup);
    Ok(RestorePreview {
        plan_id,
        installs,
        settings,
        not_included,
    })
}

/// Applies a previously previewed plan after an explicit UI confirmation.
/// The `publish_link` callback updates Tools' endpoint manifest and must verify
/// that the new value reached disk, rolling back on publication failure.
pub fn apply_restore(
    manager: &Manager,
    plan_id: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(crate::manager::Progress),
    publish_link: &mut dyn FnMut(bool) -> Result<()>,
) -> Result<RestoreResult> {
    let backup = take_plan(plan_id)?;
    let document = backup.document;
    let current: BTreeSet<_> = manager
        .list(false)?
        .into_iter()
        .filter(|app| app.installed)
        .map(|app| app.id)
        .collect();
    let mut results: BTreeMap<String, AppRestoreResult> = document
        .installed_apps
        .iter()
        .map(|app| {
            (
                app.id.clone(),
                AppRestoreResult {
                    app_id: app.id.clone(),
                    installed: current.contains(&app.id),
                    imported: false,
                    restart_required: false,
                    message: None,
                },
            )
        })
        .collect();

    for app in &document.installed_apps {
        check_cancel(cancel)?;
        manager.set_channel(&app.id, app.channel)?;
        if current.contains(&app.id) {
            continue;
        }
        let request = Request {
            id: app.id.clone(),
            operation: Operation::Install,
            mode: None,
            remove_data: false,
            app_closed: false,
        };
        match manager.operate(&request, cancel, progress) {
            Ok(_) => {
                if let Some(result) = results.get_mut(&app.id) {
                    result.installed = true;
                }
            }
            Err(error) => {
                if let Some(result) = results.get_mut(&app.id) {
                    result.message = Some(format!("Install was not completed: {}", error.message));
                }
            }
        }
    }

    restore_tools_preferences(manager, &document.tools_preferences, publish_link)?;
    restore_shortcut_labels(manager, &document.shortcut_labels)?;

    let files_by_app = group_settings_files(&document.settings_files);
    for (app_id, settings) in files_by_app {
        check_cancel(cancel)?;
        let Some(result) = results.get_mut(&app_id) else {
            continue;
        };
        if !result.installed {
            result.message.get_or_insert_with(|| {
                "Settings were not imported because the app is not installed.".into()
            });
            continue;
        }
        match import_app_settings(manager, &app_id, &settings, &backup.files) {
            Ok(Some(imported)) => {
                result.imported = imported.imported;
                result.restart_required = imported.restart_required;
                if !imported.imported {
                    result.message = Some("The app did not import the settings file.".into());
                }
            }
            Ok(None) => {
                result.message = Some("The app does not advertise app.settings.import.".into());
            }
            Err(error) => result.message = Some(error.message),
        }
    }

    Ok(RestoreResult {
        tools_settings_restored: true,
        apps: results.into_values().collect(),
        not_included: document.not_included,
    })
}

fn export_app_settings(
    manager: &Manager,
    views: &[AppView],
    installed_apps: &[InstalledApp],
) -> Result<(Vec<SettingsFile>, Vec<ArchiveEntry>, Vec<NotIncluded>)> {
    let installed: BTreeSet<_> = installed_apps.iter().map(|app| app.id.as_str()).collect();
    let mut settings_files = Vec::new();
    let mut archive_entries = Vec::new();
    let mut not_included = Vec::new();
    let mut exported_bytes = 0usize;
    for app in views {
        if !installed.contains(app.id.as_str()) {
            continue;
        }
        let Some(mut client) = connect_or_launch(manager, &app.id, app.running) else {
            not_included.push(NotIncluded {
                app_id: app.id.clone(),
                reason: "The app could not be reached for its settings handoff.".into(),
            });
            continue;
        };
        let description = match client.describe_full() {
            Ok(description) => description,
            Err(_) => {
                not_included.push(NotIncluded {
                    app_id: app.id.clone(),
                    reason: "The app does not advertise compatible settings methods.".into(),
                });
                continue;
            }
        };
        if !description.supports("app.settings.export") {
            not_included.push(NotIncluded {
                app_id: app.id.clone(),
                reason: "The app does not advertise app.settings.export.".into(),
            });
            continue;
        }
        let outputs = match client.settings_export() {
            Ok(outputs) if !outputs.is_empty() && outputs.len() <= MAX_SETTINGS_PER_APP => outputs,
            _ => {
                not_included.push(NotIncluded {
                    app_id: app.id.clone(),
                    reason: "The app did not provide a valid portable settings handoff.".into(),
                });
                continue;
            }
        };
        let mut app_exports = Vec::new();
        let mut invalid = false;
        for (index, output) in outputs.iter().enumerate() {
            let content_type = output.kind.clone();
            let Some(source) =
                handoff_source(manager.paths.link.handoff.as_path(), output, &app.id)
            else {
                invalid = true;
                break;
            };
            let bytes = match read_bounded_regular(&source, MAX_SETTINGS_BYTES) {
                Ok(bytes) => bytes,
                Err(_) => {
                    invalid = true;
                    break;
                }
            };
            let Some(opaque) = export_is_opaque(&bytes, &app.id) else {
                invalid = true;
                break;
            };
            if exported_bytes.saturating_add(bytes.len()) > MAX_EXPORT_BYTES {
                invalid = true;
                break;
            }
            let filename = output
                .name
                .as_deref()
                .filter(|name| valid_filename(name))
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!("settings-{index}.{}", if opaque { "bin" } else { "json" })
                });
            let archive_path = format!(
                "apps/{}/settings-{index:02}.{}",
                app.id,
                if opaque { "bin" } else { "json" }
            );
            app_exports.push((
                SettingsFile {
                    app_id: app.id.clone(),
                    archive_path: archive_path.clone(),
                    filename,
                    content_type,
                    encoding: opaque.then(|| OPAQUE_ENCODING.into()),
                    sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
                },
                ArchiveEntry {
                    name: archive_path,
                    bytes,
                },
            ));
        }
        if invalid || app_exports.is_empty() {
            not_included.push(NotIncluded {
                app_id: app.id.clone(),
                reason: "The app settings contained excluded data or exceeded backup limits."
                    .into(),
            });
            continue;
        }
        for (file, entry) in app_exports {
            exported_bytes = exported_bytes.saturating_add(entry.bytes.len());
            settings_files.push(file);
            archive_entries.push(entry);
        }
    }
    Ok((settings_files, archive_entries, not_included))
}

fn connect_or_launch(manager: &Manager, id: &str, running: bool) -> Option<Client> {
    if let Ok(client) = Client::connect(&manager.paths.link, id, &manager_peer()) {
        let _ = client.set_timeout(Duration::from_secs(5));
        return Some(client);
    }
    if !INSTALLABLE_APPS.contains(&id) {
        return None;
    }
    if manager.launch(id, LaunchMode::Background).is_err() && !running {
        return None;
    }
    let deadline = Instant::now() + arcade_link::client::LAUNCH_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(client) = Client::connect(&manager.paths.link, id, &manager_peer()) {
            let _ = client.set_timeout(Duration::from_secs(5));
            return Some(client);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    None
}

fn manager_peer() -> PeerInfo {
    crate::manager::me()
}

fn restore_tools_preferences(
    manager: &Manager,
    preferences: &Preferences,
    publish_link: &mut dyn FnMut(bool) -> Result<()>,
) -> Result<()> {
    manager.set_updates_enabled(preferences.updates.enabled)?;
    for id in INSTALLABLE_APPS {
        manager.set_auto_update(
            id,
            preferences
                .updates
                .auto_update
                .get(*id)
                .copied()
                .unwrap_or(false),
        )?;
    }
    for id in INSTALLABLE_APPS {
        manager.set_channel(
            id,
            preferences
                .channels
                .get(*id)
                .copied()
                .unwrap_or(Channel::Stable),
        )?;
    }
    publish_link(preferences.link_enabled)
}

fn restore_shortcut_labels(manager: &Manager, labels: &BTreeMap<String, String>) -> Result<()> {
    let path = manager.paths.config.join(LABELS_PATH);
    manager.paths.guard(&path)?;
    fs::create_dir_all(path.parent().expect("shortcut labels have a parent"))?;
    arcade_link::paths::write_atomic(&path, &serde_json::to_vec_pretty(labels)?, true)?;
    Ok(())
}

fn import_app_settings(
    manager: &Manager,
    id: &str,
    settings: &[SettingsFile],
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<arcade_link::app::ImportResult>> {
    let mut client = if let Ok(client) = Client::connect(&manager.paths.link, id, &manager_peer()) {
        client
    } else {
        manager.launch(id, LaunchMode::Background)?;
        wait_for_client(manager, id).ok_or_else(|| {
            Error::new(
                "not_running",
                "The app did not become ready for settings import.",
            )
        })?
    };
    client
        .set_timeout(Duration::from_secs(5))
        .map_err(|error| Error::new("link", error.to_string()))?;
    let description = client
        .describe_full()
        .map_err(|error| Error::new("link", error.to_string()))?;
    if !description.supports("app.settings.import") {
        return Ok(None);
    }
    let handoff = Handoff::create(&manager.paths.link, ids::TOOLS)?;
    let mut inputs = Vec::with_capacity(settings.len());
    for setting in settings {
        let bytes = files
            .get(&setting.archive_path)
            .ok_or_else(|| Error::new("archive", "Validated settings data is missing."))?;
        let mut content = handoff.file(&setting.filename, bytes)?;
        // Link 0.3 accepts only `file/any` settings inputs, whatever type the
        // app exported (Lens exports `file/json`).
        content.kind = "file/any".into();
        inputs.push(content);
    }
    client
        .settings_import(&inputs)
        .map(Some)
        .map_err(|error| Error::new("link", error.to_string()))
}

fn wait_for_client(manager: &Manager, id: &str) -> Option<Client> {
    let deadline = Instant::now() + arcade_link::client::LAUNCH_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(client) = Client::connect(&manager.paths.link, id, &manager_peer()) {
            return Some(client);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    None
}

fn read_shortcut_labels(paths: &Paths) -> Result<BTreeMap<String, String>> {
    let path = paths.config.join(LABELS_PATH);
    match read_bounded_regular(&path, 1024 * 1024) {
        Ok(bytes) => {
            let labels: BTreeMap<String, String> = serde_json::from_slice(&bytes)?;
            validate_saved_labels(&labels)?;
            Ok(labels)
        }
        Err(error) if error.code == "io" && !path.exists() => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

fn validate_saved_labels(labels: &BTreeMap<String, String>) -> Result<()> {
    if labels.len() > 4096 {
        return Err(Error::new("shortcut_labels", "Too many shortcut labels."));
    }
    for (key, value) in labels {
        if !valid_label_key(key) || value.trim().is_empty() || value.chars().count() > 120 {
            return Err(Error::new(
                "shortcut_labels",
                "Shortcut labels are invalid.",
            ));
        }
        if crate::diagnostics_archive::has_secret_markers(value) {
            return Err(Error::new(
                "privacy",
                "A shortcut label resembles a credential; remove it before backing up.",
            ));
        }
    }
    Ok(())
}

fn valid_label_key(key: &str) -> bool {
    key.len() <= 512
        && key.matches('|').count() == 1
        && key
            .split_once('|')
            .is_some_and(|(_, chord)| !chord.trim().is_empty())
}

/// Resolves an app's settings export to a file inside the handoff root.
/// `app_id` is the app Tools asked for the export. Some apps (Wheel) omit
/// `owner`; the export is then attributed to the app that answered the call.
/// An owner naming a different app is always rejected.
fn handoff_source(root: &Path, content: &Content, app_id: &str) -> Option<PathBuf> {
    if content
        .owner
        .as_deref()
        .is_some_and(|owner| owner != app_id)
        || !content.kind.starts_with("file/")
        || content.kind.ends_with("[]")
        || !content.paths.is_empty()
        || content.text.is_some()
        || content.html.is_some()
        || content.data.is_some()
    {
        return None;
    }
    let path = PathBuf::from(content.path.as_deref()?);
    if !path.is_absolute() || contains_symlink_component(&path).ok()? {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    if !path.starts_with(root) || !fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let size = fs::metadata(&path).ok()?.len();
    if content.size.is_some_and(|expected| expected != size) {
        return None;
    }
    Some(path)
}

fn read_bounded_regular(path: &Path, max: usize) -> Result<Vec<u8>> {
    if contains_symlink_component(path)? {
        return Err(Error::new("paths", "Refusing to read a symlink."));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max as u64 {
        return Err(Error::new(
            "size",
            "File is not regular or exceeds its size limit.",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(max.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::new("size", "File exceeds its size limit."));
    }
    Ok(bytes)
}

fn read_archive_file(path: &Path) -> Result<Vec<u8>> {
    if !path.is_absolute() || contains_symlink_component(path)? {
        return Err(Error::new(
            "path",
            "Choose a regular, non-symlink backup file.",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            "path",
            "The selected backup is not a regular file.",
        ));
    }
    if metadata.len() as usize > MAX_ARCHIVE_BYTES {
        return Err(Error::new(
            "archive_size",
            "The selected backup is too large.",
        ));
    }
    read_bounded_regular(path, MAX_ARCHIVE_BYTES)
}

fn contains_symlink_component(path: &Path) -> Result<bool> {
    if !path.is_absolute() {
        return Ok(false);
    }
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
            Component::ParentDir => return Ok(true),
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

fn contains_excluded_data(value: &Value, app_id: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            let normalized: String = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            let generic_exclusion = [
                "token",
                "secret",
                "password",
                "credential",
                "authorization",
                "apikey",
                "accesskey",
                "privatekey",
                "deviceid",
                "deviceidentity",
                "keyring",
                "identity",
            ]
            .iter()
            .any(|needle| normalized.contains(needle))
                // A boolean, number or null under a secret-sounding name is a
                // preference such as Lens's `privacy.guard_secrets`, not
                // secret material. Strings, arrays (bytes) and objects stay
                // excluded.
                && holds_material(child);
            let app_exclusion = app_id == ids::CLIPBOARD
                && ["key", "keys", "history", "device"].contains(&normalized.as_str())
                || app_id == ids::FIND
                    && ["index", "cache", "caches"].contains(&normalized.as_str());
            generic_exclusion || app_exclusion || contains_excluded_data(child, app_id)
        }),
        Value::Array(items) => items
            .iter()
            .any(|child| contains_excluded_data(child, app_id)),
        Value::String(text) => crate::diagnostics_archive::has_secret_markers(text),
        _ => false,
    }
}

/// Classifies an app's settings export: `Some(false)` for JSON that passed the
/// excluded-data scan, `Some(true)` for a non-JSON app-defined format (Shelf's
/// binary export) and `None` for JSON holding excluded data. Tools cannot
/// interpret opaque formats, so their privacy boundary is the app's own export
/// contract; Tools only bounds, hashes and returns the bytes unchanged.
fn export_is_opaque(bytes: &[u8], app_id: &str) -> Option<bool> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(value) if contains_excluded_data(&value, app_id) => None,
        Ok(_) => Some(false),
        Err(_) => Some(true),
    }
}

fn holds_material(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Array(_) | Value::Object(_))
}

fn valid_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

fn installed_apps_count(document: &BackupDocument) -> usize {
    document.installed_apps.len()
}

fn settings_summary(files: &[SettingsFile]) -> Vec<RestoreSettings> {
    let mut counts = BTreeMap::<String, usize>::new();
    for file in files {
        *counts.entry(file.app_id.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(app_id, files)| RestoreSettings { app_id, files })
        .collect()
}

fn group_settings_files(files: &[SettingsFile]) -> BTreeMap<String, Vec<SettingsFile>> {
    let mut grouped = BTreeMap::<String, Vec<SettingsFile>>::new();
    for file in files {
        grouped
            .entry(file.app_id.clone())
            .or_default()
            .push(file.clone());
    }
    grouped
}

fn store_plan(plan_id: &str, backup: ValidatedBackup) {
    let plans = PENDING_PLANS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut plans = plans
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    plans.retain(|_, plan| plan.created.elapsed() < MAX_PLAN_AGE);
    if plans.len() >= MAX_PENDING_PLANS {
        if let Some(oldest) = plans
            .iter()
            .min_by_key(|(_, plan)| plan.created)
            .map(|(key, _)| key.clone())
        {
            plans.remove(&oldest);
        }
    }
    plans.insert(
        plan_id.into(),
        PendingPlan {
            created: Instant::now(),
            backup,
        },
    );
}

fn take_plan(plan_id: &str) -> Result<ValidatedBackup> {
    if plan_id.len() != 64 || !plan_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(
            "restore_plan",
            "The restore confirmation expired.",
        ));
    }
    let plans = PENDING_PLANS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut plans = plans
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(plan) = plans.remove(plan_id) else {
        return Err(Error::new(
            "restore_plan",
            "The restore confirmation expired.",
        ));
    };
    if plan.created.elapsed() >= MAX_PLAN_AGE {
        return Err(Error::new(
            "restore_plan",
            "The restore confirmation expired.",
        ));
    }
    Ok(plan.backup)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        Err(Error::new("cancelled", "Restore was cancelled."))
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn expansion_is_allowed(uncompressed: u64, compressed: u64) -> Result<()> {
    if uncompressed > 0
        && (compressed == 0 || uncompressed > compressed.saturating_mul(MAX_EXPANSION_RATIO))
    {
        return Err(Error::new(
            "archive_size",
            "The backup expansion ratio is unsafe.",
        ));
    }
    Ok(())
}

fn validate_archive(bytes: &[u8]) -> Result<ValidatedBackup> {
    preflight_zip(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| Error::new("archive", error.to_string()))?;
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::new(
            "archive_entries",
            "The backup has an invalid entry count.",
        ));
    }
    let mut entries = BTreeMap::new();
    let mut total_size = 0usize;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| Error::new("archive", error.to_string()))?;
        let name = file.name().to_owned();
        if !valid_archive_name(&name)
            || file.is_dir()
            || file.is_symlink()
            || file.encrypted()
            || file
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
        {
            return Err(Error::new(
                "archive_path",
                "The backup contains an unsafe entry.",
            ));
        }
        if file.compression() != CompressionMethod::Stored {
            return Err(Error::new(
                "archive_compression",
                "The backup uses an unsupported compression method.",
            ));
        }
        let declared_size = usize::try_from(file.size())
            .map_err(|_| Error::new("archive_size", "A backup entry is too large."))?;
        let compressed_size = file.compressed_size();
        if declared_size > MAX_ENTRY_BYTES
            || total_size.saturating_add(declared_size) > MAX_TOTAL_UNCOMPRESSED
            || (declared_size > 0
                && (compressed_size == 0
                    || declared_size as u64 > compressed_size.saturating_mul(MAX_EXPANSION_RATIO)))
        {
            return Err(Error::new(
                "archive_size",
                "The backup exceeds size limits.",
            ));
        }
        if entries.contains_key(&name) {
            return Err(Error::new(
                "archive_duplicate",
                "The backup has duplicate paths.",
            ));
        }
        let mut payload = Vec::with_capacity(declared_size);
        file.by_ref()
            .take(MAX_ENTRY_BYTES as u64 + 1)
            .read_to_end(&mut payload)
            .map_err(|error| Error::new("archive", error.to_string()))?;
        if payload.len() != declared_size {
            return Err(Error::new(
                "archive_size",
                "A backup entry size did not match.",
            ));
        }
        total_size += declared_size;
        entries.insert(name, payload);
    }
    let manifest = entries
        .remove(BACKUP_DOCUMENT)
        .ok_or_else(|| Error::new("archive_manifest", "The backup manifest is missing."))?;
    if manifest.len() > 256 * 1024 {
        return Err(Error::new(
            "archive_manifest",
            "The backup manifest is too large.",
        ));
    }
    let document: BackupDocument = serde_json::from_slice(&manifest)
        .map_err(|error| Error::new("archive_manifest", error.to_string()))?;
    validate_document(&document, &entries)?;
    Ok(ValidatedBackup {
        document,
        files: entries,
    })
}

fn preflight_zip(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::new(
            "archive_size",
            "The selected backup is too large.",
        ));
    }
    const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    let search_start = bytes.len().saturating_sub(65_557);
    let eocd = bytes[search_start..]
        .windows(4)
        .rposition(|window| window == EOCD)
        .map(|offset| search_start + offset)
        .ok_or_else(|| Error::new("archive", "The ZIP directory is missing."))?;
    if eocd + 22 > bytes.len() {
        return Err(Error::new("archive", "The ZIP directory is truncated."));
    }
    let comment_size = read_u16(bytes, eocd + 20)? as usize;
    if eocd + 22 + comment_size != bytes.len()
        || read_u16(bytes, eocd + 4)? != 0
        || read_u16(bytes, eocd + 6)? != 0
    {
        return Err(Error::new("archive", "The ZIP directory is malformed."));
    }
    let disk_entries = read_u16(bytes, eocd + 8)?;
    let total_entries = read_u16(bytes, eocd + 10)?;
    let directory_size = read_u32(bytes, eocd + 12)?;
    let directory_offset = read_u32(bytes, eocd + 16)?;
    if disk_entries == u16::MAX
        || total_entries == u16::MAX
        || directory_size == u32::MAX
        || directory_offset == u32::MAX
        || disk_entries != total_entries
        || total_entries == 0
        || total_entries as usize > MAX_ARCHIVE_ENTRIES
        || directory_size as usize > MAX_ARCHIVE_BYTES / 2
        || directory_offset as usize + directory_size as usize > eocd
    {
        return Err(Error::new(
            "archive_entries",
            "The ZIP directory exceeds limits.",
        ));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let slice = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| Error::new("archive", "The ZIP directory is truncated."))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let slice = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| Error::new("archive", "The ZIP directory is truncated."))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn valid_archive_name(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with(['/', '\\'])
        || name.contains(['\\', ':', '\0'])
        || name.chars().any(char::is_control)
    {
        return false;
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(part) if part != "." && part != "..")
        })
    {
        return false;
    }
    true
}

fn validate_document(document: &BackupDocument, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if document.schema != BACKUP_SCHEMA
        || document.kind != BACKUP_KIND
        || document.tools_version.trim().is_empty()
        || document.created_at.len() > 64
    {
        return Err(Error::new(
            "archive_manifest",
            "Unsupported backup manifest.",
        ));
    }
    if document.installed_apps.len() > INSTALLABLE_APPS.len()
        || document.settings_files.len() > MAX_SETTINGS_FILES
        || document.not_included.len() > INSTALLABLE_APPS.len()
    {
        return Err(Error::new(
            "archive_manifest",
            "The backup lists too many apps or files.",
        ));
    }
    let mut app_ids = BTreeSet::new();
    for app in &document.installed_apps {
        validate_known_app(&app.id)?;
        if !app_ids.insert(app.id.as_str()) {
            return Err(Error::new("archive_manifest", "The backup repeats an app."));
        }
    }
    for id in document
        .tools_preferences
        .channels
        .keys()
        .chain(document.tools_preferences.updates.auto_update.keys())
    {
        validate_known_app(id)?;
    }
    validate_labels(&document.shortcut_labels)?;

    let mut settings_paths = BTreeSet::new();
    let mut per_app = BTreeMap::<&str, usize>::new();
    for settings in &document.settings_files {
        validate_known_app(&settings.app_id)?;
        if !app_ids.contains(settings.app_id.as_str())
            || !settings
                .archive_path
                .starts_with(&format!("apps/{}/settings-", settings.app_id))
            || !settings.archive_path.ends_with(if settings.is_opaque() {
                ".bin"
            } else {
                ".json"
            })
            || settings
                .encoding
                .as_deref()
                .is_some_and(|encoding| encoding != OPAQUE_ENCODING)
            || settings.sha256.as_deref().is_some_and(|sha256| {
                sha256.len() != 64
                    || !sha256
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            || (settings.is_opaque() && settings.sha256.is_none())
            || !valid_archive_name(&settings.archive_path)
            || !valid_filename(&settings.filename)
            || !settings.content_type.starts_with("file/")
            || settings.content_type.ends_with("[]")
            || settings.content_type.len() > 64
            || !settings_paths.insert(settings.archive_path.as_str())
        {
            return Err(Error::new(
                "archive_manifest",
                "The app settings index is invalid.",
            ));
        }
        *per_app.entry(settings.app_id.as_str()).or_default() += 1;
        if per_app[settings.app_id.as_str()] > MAX_SETTINGS_PER_APP {
            return Err(Error::new(
                "archive_manifest",
                "An app has too many settings files.",
            ));
        }
        let bytes = files
            .get(&settings.archive_path)
            .ok_or_else(|| Error::new("archive_manifest", "An app settings file is missing."))?;
        if bytes.len() > MAX_SETTINGS_BYTES
            || settings
                .sha256
                .as_deref()
                .is_some_and(|sha256| sha256 != format!("{:x}", Sha256::digest(bytes)))
        {
            return Err(Error::new(
                "archive_settings",
                "An app settings file does not match the backup index.",
            ));
        }
        let parsed = serde_json::from_slice::<Value>(bytes);
        if settings.is_opaque() {
            // Opaque payloads rely on the app's export contract. JSON is never
            // accepted as opaque, so it cannot bypass the privacy scan.
            if parsed.is_ok() {
                return Err(Error::new(
                    "archive_privacy",
                    "The backup includes settings excluded by the privacy contract.",
                ));
            }
            continue;
        }
        let value = parsed.map_err(|error| Error::new("archive_settings", error.to_string()))?;
        if contains_excluded_data(&value, &settings.app_id) {
            return Err(Error::new(
                "archive_privacy",
                "The backup includes settings excluded by the privacy contract.",
            ));
        }
    }
    if files
        .keys()
        .any(|name| name != "tools/shortcut-labels.json" && !settings_paths.contains(name.as_str()))
    {
        return Err(Error::new(
            "archive_path",
            "The backup contains an unlisted file.",
        ));
    }
    let labels_file = files
        .get("tools/shortcut-labels.json")
        .ok_or_else(|| Error::new("archive_manifest", "Shortcut labels are missing."))?;
    let zipped_labels: BTreeMap<String, String> = serde_json::from_slice(labels_file)
        .map_err(|_| Error::new("archive_manifest", "Shortcut labels are malformed."))?;
    validate_labels(&zipped_labels)?;
    if zipped_labels != document.shortcut_labels {
        return Err(Error::new(
            "archive_manifest",
            "Shortcut labels do not match.",
        ));
    }
    for item in &document.not_included {
        validate_known_app(&item.app_id)?;
        if item.reason.trim().is_empty() || item.reason.len() > 256 {
            return Err(Error::new(
                "archive_manifest",
                "A not-included reason is invalid.",
            ));
        }
    }
    Ok(())
}

fn validate_known_app(id: &str) -> Result<()> {
    if !INSTALLABLE_APPS.contains(&id) {
        return Err(Error::new(
            "archive_app",
            "The backup names an unknown Arcade app.",
        ));
    }
    Ok(())
}

fn validate_labels(labels: &BTreeMap<String, String>) -> Result<()> {
    if labels.len() > 4096 {
        return Err(Error::new("archive_labels", "Too many shortcut labels."));
    }
    for (key, value) in labels {
        if !valid_label_key(key)
            || value.trim().is_empty()
            || value.chars().count() > 120
            || crate::diagnostics_archive::has_secret_markers(value)
        {
            return Err(Error::new("archive_labels", "Shortcut labels are invalid."));
        }
    }
    Ok(())
}

fn write_zip(entries: Vec<ArchiveEntry>) -> Result<Vec<u8>> {
    if entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::new(
            "archive_entries",
            "The backup has too many files.",
        ));
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
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
    let bytes = writer
        .finish()
        .map_err(|error| Error::new("archive", error.to_string()))?
        .into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::new(
            "archive_size",
            "The backup exceeds the size limit.",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
fn preflight_test_archive(entries: Vec<ArchiveEntry>) -> Result<Vec<u8>> {
    write_zip(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_document() -> BackupDocument {
        BackupDocument {
            schema: BACKUP_SCHEMA,
            kind: BACKUP_KIND.into(),
            created_at: "2026-10-11T00:00:00Z".into(),
            tools_version: "0.1.0".into(),
            tools_preferences: Preferences::default(),
            shortcut_labels: BTreeMap::from([("|Ctrl+Alt+F".into(), "Find".into())]),
            installed_apps: vec![InstalledApp {
                id: ids::FIND.into(),
                version: Some("0.3.0".into()),
                channel: Channel::Stable,
            }],
            settings_files: vec![SettingsFile {
                app_id: ids::FIND.into(),
                archive_path: "apps/arcade.find/settings-00.json".into(),
                filename: "find-settings.json".into(),
                content_type: "file/any".into(),
                encoding: None,
                sha256: None,
            }],
            not_included: vec![],
        }
    }

    fn roundtrip_archive() -> Vec<u8> {
        let document = minimal_document();
        let mut entries = vec![ArchiveEntry {
            name: "tools/shortcut-labels.json".into(),
            bytes: serde_json::to_vec(&document.shortcut_labels).unwrap(),
        }];
        entries.push(ArchiveEntry {
            name: "apps/arcade.find/settings-00.json".into(),
            bytes: br#"{"locations":["~/Documents"]}"#.to_vec(),
        });
        entries.push(ArchiveEntry {
            name: BACKUP_DOCUMENT.into(),
            bytes: serde_json::to_vec(&document).unwrap(),
        });
        write_zip(entries).unwrap()
    }

    #[test]
    fn archive_roundtrips_valid_portable_settings_and_labels() {
        let bytes = roundtrip_archive();
        let validated = validate_archive(&bytes).unwrap();
        assert_eq!(validated.document.installed_apps[0].id, ids::FIND);
        assert!(validated
            .files
            .contains_key("apps/arcade.find/settings-00.json"));
    }

    #[test]
    fn rejects_zip_slip_symlinks_duplicates_and_oversized_entries() {
        let slip = preflight_test_archive(vec![ArchiveEntry {
            name: "../arcade-tools/preferences.json".into(),
            bytes: b"bad".to_vec(),
        }])
        .unwrap();
        assert!(validate_archive(&slip).is_err());

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let symlink_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o120777);
        writer
            .start_file("tools/shortcut-labels.json", symlink_options)
            .unwrap();
        writer.write_all(b"{}").unwrap();
        let symlink = writer.finish().unwrap().into_inner();
        assert!(validate_archive(&symlink).is_err());

        let duplicate = write_zip(vec![
            ArchiveEntry {
                name: "one.json".into(),
                bytes: vec![],
            },
            ArchiveEntry {
                name: "one.json".into(),
                bytes: vec![],
            },
        ]);
        assert!(duplicate.is_err() || validate_archive(&duplicate.unwrap()).is_err());

        let oversized = write_zip(vec![ArchiveEntry {
            name: "large.json".into(),
            bytes: vec![b'x'; MAX_ENTRY_BYTES + 1],
        }])
        .unwrap();
        assert!(validate_archive(&oversized).is_err());
    }

    #[test]
    fn rejects_bomb_ratio_and_malformed_central_directory() {
        assert!(expansion_is_allowed(MAX_EXPANSION_RATIO + 1, 1).is_err());
        let mut bytes = roundtrip_archive();
        let last = bytes.len() - 22;
        bytes[last + 10] = (MAX_ARCHIVE_ENTRIES as u8).saturating_add(1);
        bytes[last + 11] = 0;
        assert!(validate_archive(&bytes).is_err());
        assert!(validate_archive(b"not a zip").is_err());
    }

    #[test]
    fn rejects_clipboard_identities_keys_history_find_index_and_credentials() {
        assert!(contains_excluded_data(
            &json!({ "device": "phone", "keys": ["key"] }),
            ids::CLIPBOARD
        ));
        assert!(contains_excluded_data(
            &json!({ "history": [] }),
            ids::CLIPBOARD
        ));
        assert!(contains_excluded_data(&json!({ "index": {} }), ids::FIND));
        assert!(contains_excluded_data(
            &json!({ "credential": "x" }),
            ids::FIND
        ));
    }

    #[test]
    fn lens_boolean_secret_toggle_is_not_excluded() {
        let lens = json!({
            "privacy": {
                "history_enabled": false,
                "learn_action_usage": true,
                "guard_secrets": true
            },
            "secret_scan_limit": 4096,
            "token_budget": null
        });
        assert!(!contains_excluded_data(&lens, ids::LENS));
        assert_eq!(
            export_is_opaque(&serde_json::to_vec(&lens).unwrap(), ids::LENS),
            Some(false)
        );
    }

    #[test]
    fn real_secret_material_is_still_excluded() {
        for value in [
            json!({ "privacy": { "guard_secrets": "s3cr3t-value" } }),
            json!({ "api_token": "abc123" }),
            json!({ "password": "hunter2" }),
            json!({ "private_key": [1, 2, 3] }),
            json!({ "credentials": { "user": "me" } }),
            json!({ "device_identity": "phone-1" }),
            json!({ "notes": "ghp_0123456789abcdefghijklmnopqrstuvwxyz" }),
        ] {
            assert!(contains_excluded_data(&value, ids::LENS), "{value}");
            assert_eq!(
                export_is_opaque(&serde_json::to_vec(&value).unwrap(), ids::LENS),
                None
            );
        }
    }

    #[test]
    fn missing_owner_is_attributed_to_the_requested_app() {
        let root = std::env::temp_dir().join(format!("tools-handoff-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("wheel-settings.json");
        fs::write(&path, br#"{"ring":[]}"#).unwrap();
        let mut content = Content::file(&path);
        content.kind = "file/any".into();
        content.owner = None;
        assert!(handoff_source(&root, &content, ids::WHEEL).is_some());
        content.owner = Some(ids::WHEEL.into());
        assert!(handoff_source(&root, &content, ids::WHEEL).is_some());
        content.owner = Some(ids::LENS.into());
        assert!(handoff_source(&root, &content, ids::WHEEL).is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    fn shelf_archive(settings: SettingsFile, payload: &[u8]) -> Vec<u8> {
        let mut document = minimal_document();
        document.installed_apps.push(InstalledApp {
            id: ids::SHELF.into(),
            version: Some("0.2.0".into()),
            channel: Channel::Stable,
        });
        document.settings_files = vec![settings.clone()];
        write_zip(vec![
            ArchiveEntry {
                name: "tools/shortcut-labels.json".into(),
                bytes: serde_json::to_vec(&document.shortcut_labels).unwrap(),
            },
            ArchiveEntry {
                name: settings.archive_path,
                bytes: payload.to_vec(),
            },
            ArchiveEntry {
                name: BACKUP_DOCUMENT.into(),
                bytes: serde_json::to_vec(&document).unwrap(),
            },
        ])
        .unwrap()
    }

    #[test]
    fn opaque_shelf_export_roundtrips_byte_for_byte() {
        let mut payload = b"ARCADE-SHELF-PORTABLE\n".to_vec();
        payload.extend_from_slice(&[0, 0, 0, 2, b'{', b'}', 0xff, 0xfe, 0x00]);
        assert_eq!(export_is_opaque(&payload, ids::SHELF), Some(true));
        let settings = SettingsFile {
            app_id: ids::SHELF.into(),
            archive_path: "apps/arcade.shelf/settings-00.bin".into(),
            filename: "Arcade-Shelf.arcade-shelf".into(),
            content_type: "file/any".into(),
            encoding: Some(OPAQUE_ENCODING.into()),
            sha256: Some(format!("{:x}", Sha256::digest(&payload))),
        };
        let validated = validate_archive(&shelf_archive(settings.clone(), &payload)).unwrap();
        assert!(validated.document.settings_files[0].is_opaque());
        assert_eq!(validated.files[&settings.archive_path], payload);

        let mut tampered = payload.clone();
        tampered.push(1);
        assert!(validate_archive(&shelf_archive(settings.clone(), &tampered)).is_err());

        let mut unhashed = settings.clone();
        unhashed.sha256 = None;
        assert!(validate_archive(&shelf_archive(unhashed, &payload)).is_err());

        let mut wrong_extension = settings.clone();
        wrong_extension.archive_path = "apps/arcade.shelf/settings-00.json".into();
        assert!(validate_archive(&shelf_archive(wrong_extension, &payload)).is_err());

        let secret_json = br#"{"password":"hunter2"}"#;
        let mut smuggled = settings;
        smuggled.sha256 = Some(format!("{:x}", Sha256::digest(secret_json)));
        assert!(validate_archive(&shelf_archive(smuggled, secret_json)).is_err());
    }
}
