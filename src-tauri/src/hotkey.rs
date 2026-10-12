use arcade_tools_core::{Error, Result};
use arcade_tools_overlay::registration::{self, Via};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub accelerator: String,
    pub available: bool,
    pub via: Option<Via>,
    pub hint: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Service(Mutex<Status>);

pub fn overlay_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "arcade-tools-overlay.exe"
    } else {
        "arcade-tools-overlay"
    };
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(name)))
        .filter(|path| path.is_file())
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|path| path.join(name))
                    .find(|path| path.is_file())
            })
        })
}

pub fn launch_overlay() -> Result<()> {
    let path = overlay_path()
        .ok_or_else(|| Error::new("overlay", "The shortcut overlay is not installed."))?;
    arcade_link::client::spawn_detached(&path.to_string_lossy(), &["--toggle".into()])?;
    Ok(())
}

pub fn status(app: &AppHandle) -> Status {
    app.state::<Arc<Service>>().0.lock().unwrap().clone()
}

/// Filesystem discovery stays on the caller's worker; registration is dispatched
/// to the platform event loop, including macOS and Windows.
pub fn configure(app: &AppHandle, prefs: &crate::tray::settings::Preferences) {
    let available = overlay_path().is_some();
    let accelerator = prefs.shortcut.clone();
    let enabled = prefs.shortcuts_enabled;
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let mut status = Status {
            accelerator: accelerator.clone(),
            available,
            ..Default::default()
        };
        let result = if enabled && available {
            let pressed = handle.clone();
            registration::register_with_handler(&accelerator, move || {
                if let Ok(tray) = crate::tray::service(&pressed) {
                    tray.request_shortcuts();
                }
            })
            .map(|registration| {
                status.via = Some(registration.via);
                status.hint = registration.hint;
            })
        } else {
            registration::unregister()
        };
        if let Err(error) = result {
            status.error = Some(error);
        }
        *handle.state::<Arc<Service>>().0.lock().unwrap() = status;
        let _ = handle.emit("overlay-shortcut-changed", ());
    });
}

/// Called on the main thread when the Tools native event loop exits.
pub fn stop() {
    let _ = registration::unregister();
}
