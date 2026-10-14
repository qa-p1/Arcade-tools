use arcade_tools_core::paths::Paths;
use arcade_tools_core::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const INDEX_PATH: &str = "arcade/tools/shortcuts-index.json";
const LABELS_PATH: &str = "arcade/tools/shortcut-labels.json";

fn cache_file() -> Result<PathBuf> {
    let cache = match std::env::var_os("ARCADE_HOME").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root).join("cache"),
        None => Paths::discover()?.cache,
    };
    Ok(cache.join(INDEX_PATH))
}

fn labels_file() -> Result<PathBuf> {
    Ok(Paths::discover()?.config.join(LABELS_PATH))
}

pub(crate) fn read_index() -> Result<Option<Value>> {
    match fs::read(cache_file()?) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| Error::new("shortcuts_index", error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_labels(path: &std::path::Path) -> Result<BTreeMap<String, String>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| Error::new("shortcut_labels", error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}

fn validate_label_key(key: &str) -> Result<()> {
    let Some((_, chord)) = key.split_once('|') else {
        return Err(Error::new(
            "unsupported_input",
            "A shortcut label key must have the form submap|chord.",
        ));
    };
    if chord.trim().is_empty() || key.len() > 512 || key.matches('|').count() != 1 {
        return Err(Error::new(
            "unsupported_input",
            "The shortcut label key is invalid.",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn shortcuts_index() -> Result<Option<Value>> {
    tauri::async_runtime::spawn_blocking(read_index)
        .await
        .map_err(|error| Error::new("worker", error.to_string()))?
}

#[tauri::command]
pub async fn current_shortcut_app() -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(|| {
        let Some(value) = read_index()? else {
            return Ok(None);
        };
        let index = arcade_tools_overlay::index::Index::parse(value.to_string().as_bytes())
            .map_err(|error| Error::new("shortcuts_index", error))?;
        let focus = arcade_tools_overlay::focus::detect();
        Ok(focus
            .identity
            .as_deref()
            .and_then(|identity| index.match_app(identity, focus.kind))
            .and_then(|position| index.apps.get(position))
            .map(|app| app.id.clone()))
    })
    .await
    .map_err(|error| Error::new("worker", error.to_string()))?
}

#[tauri::command]
pub async fn shortcut_labels() -> Result<BTreeMap<String, String>> {
    tauri::async_runtime::spawn_blocking(|| read_labels(&labels_file()?))
        .await
        .map_err(|error| Error::new("worker", error.to_string()))?
}

#[tauri::command]
pub async fn set_shortcut_label(key: String, label: Option<String>) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_label_key(&key)?;
        let path = labels_file()?;
        let mut labels = read_labels(&path)?;
        match label.map(|value| value.trim().to_owned()) {
            Some(value) if !value.is_empty() => {
                if value.chars().count() > 120 {
                    return Err(Error::new(
                        "unsupported_input",
                        "Shortcut labels must be 120 characters or fewer.",
                    ));
                }
                labels.insert(key, value);
            }
            _ => {
                labels.remove(&key);
            }
        }
        let bytes = serde_json::to_vec_pretty(&labels)?;
        arcade_link::paths::write_atomic(&path, &bytes, true)?;
        Ok(())
    })
    .await
    .map_err(|error| Error::new("worker", error.to_string()))?
}

#[tauri::command]
pub async fn set_app_shortcut(
    app: tauri::AppHandle,
    id: String,
    shortcut: String,
    accelerator: String,
) -> Result<arcade_link::app::ShortcutSetResult> {
    use tauri::Manager as _;
    tauri::async_runtime::spawn_blocking(move || {
        let manager = crate::desktop::manager(&app)?;
        let index = read_index()?
            .ok_or_else(|| Error::new("shortcuts_index", "Rebuild the shortcut index first."))?;
        let result = arcade_tools_core::shortcut_control::set(
            &manager,
            &index,
            &id,
            &shortcut,
            &accelerator,
        )?;
        if let Some(service) = app.try_state::<std::sync::Arc<crate::index_service::IndexService>>()
        {
            service.changed();
        }
        Ok(result)
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}
