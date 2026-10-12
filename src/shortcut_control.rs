//! Confirmed global rebindings. Network, IPC and launch waits run on a worker.
use crate::manager::{me, LaunchMode, Manager};
use crate::{Error, Result};
use arcade_link::{accelerator, client::Client, registry::Registry};
use serde_json::Value;
use std::time::{Duration, Instant};

pub fn validate(index: &Value, app: &str, id: &str, accelerator: &str) -> Result<String> {
    if app != "arcade.tools" {
        crate::paths::app_id(app)?;
    }
    let normalized =
        accelerator::normalize(accelerator).map_err(|e| Error::new("shortcut", e.to_string()))?;
    let shortcuts = index["arcade"].as_array().into_iter().flatten();
    for shortcut in shortcuts {
        if shortcut["app"] == app && shortcut["id"] == id {
            continue;
        }
        for key in shortcut["keys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if accelerator::conflicts(&normalized, key).unwrap_or(false) {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "Used by {}: {}.",
                        shortcut["name"].as_str().unwrap_or("another app"),
                        shortcut["title"].as_str().unwrap_or("global shortcut")
                    ),
                ));
            }
        }
    }
    for group in index["wm"]["groups"].as_array().into_iter().flatten() {
        for shortcut in group["shortcuts"].as_array().into_iter().flatten() {
            if shortcut["state"] == "configuredOnly" {
                continue;
            }
            if app == "arcade.tools" && shortcut["label"] == "Arcade Tools shortcuts" {
                continue;
            }
            if shortcut["chord"]
                .as_str()
                .is_some_and(|key| accelerator::conflicts(&normalized, key).unwrap_or(false))
            {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "Used by the window manager: {}.",
                        shortcut["label"].as_str().unwrap_or("system binding")
                    ),
                ));
            }
        }
    }
    for sheet in index["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|sheet| {
            matches!(
                sheet["id"].as_str(),
                Some("windows-system" | "macos-system")
            )
        })
    {
        for shortcut in sheet["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["shortcuts"].as_array().into_iter().flatten())
        {
            for key in shortcut["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if accelerator::conflicts(&normalized, key).unwrap_or(false) {
                    return Err(Error::new(
                        "conflict",
                        format!(
                            "Reserved by {}: {}.",
                            sheet["name"].as_str().unwrap_or("the operating system"),
                            shortcut["title"].as_str().unwrap_or("system shortcut")
                        ),
                    ));
                }
            }
        }
    }
    Ok(normalized)
}

pub fn set(
    manager: &Manager,
    index: &Value,
    app: &str,
    id: &str,
    accelerator: &str,
) -> Result<arcade_link::app::ShortcutSetResult> {
    crate::paths::app_id(app)?;
    let normalized = validate(index, app, id, accelerator)?;
    let registry = Registry::load(&manager.paths.link);
    let manifest = registry
        .get(app)
        .ok_or_else(|| Error::new("not_installed", "The app is not registered."))?;
    if !manifest.settings.link_enabled {
        return Err(Error::new(
            "disabled",
            "Enable Arcade Link in this app's settings first.",
        ));
    }
    if !manifest.shortcuts.iter().any(|shortcut| shortcut.id == id) {
        return Err(Error::new(
            "shortcut",
            "This app does not advertise that global shortcut.",
        ));
    }
    let mut client = match Client::connect(&manager.paths.link, app, &me()) {
        Ok(client) => client,
        Err(_) => {
            manager.launch(app, LaunchMode::Background)?;
            let deadline = Instant::now() + arcade_link::client::LAUNCH_TIMEOUT;
            loop {
                if let Ok(client) = Client::connect_with(
                    &manager.paths.link,
                    app,
                    &me(),
                    Duration::from_millis(250),
                ) {
                    break client;
                }
                if Instant::now() >= deadline {
                    return Err(Error::new(
                        "timeout",
                        "The app did not become ready. Open its settings and try again.",
                    ));
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    };
    if !client.describe_full()?.supports("app.shortcuts.set") {
        return Err(Error::new("unsupported", "This version does not support rebinding from Tools. Change this shortcut in the app's settings."));
    }
    client.shortcuts_set(id, &normalized).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reserved_system_keys_are_rejected_and_app_keys_do_not_block_globals() {
        let index = json!({"apps":[{"id":"windows-system","name":"Windows","groups":[{"shortcuts":[{"title":"Lock the PC","keys":["Super+L"]}]}]},{"id":"firefox","groups":[{"shortcuts":[{"keys":["Ctrl+L"]}]}]}]});
        assert_eq!(
            validate(&index, "arcade.find", "toggle", "Super+L")
                .unwrap_err()
                .code,
            "conflict"
        );
        assert!(validate(&index, "arcade.find", "toggle", "Ctrl+L").is_ok());
        assert!(validate(&index, "arcade.tools", "overlay", "Super+L").is_err());
    }
    #[test]
    fn rejects_peer_prefixes_and_active_system_binds_but_allows_own_binding() {
        let index = json!({"arcade":[{"app":"arcade.find","id":"toggle","keys":["Ctrl+F"]},{"app":"arcade.look","id":"open","keys":["Ctrl+K Ctrl+O"]}],"wm":{"groups":[{"shortcuts":[{"chord":"Super+X","state":"active","label":"Terminal"},{"chord":"Super+Y","state":"configuredOnly"}]}]}});
        assert_eq!(
            validate(&index, "arcade.find", "toggle", "Ctrl+F").unwrap(),
            "Ctrl+F"
        );
        assert_eq!(
            validate(&index, "arcade.find", "toggle", "Ctrl+K")
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(
            validate(&index, "arcade.find", "toggle", "Super+X")
                .unwrap_err()
                .code,
            "conflict"
        );
        assert!(validate(&index, "arcade.find", "toggle", "Super+Y").is_ok());
    }
}
