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
    app.state::<Arc<Service>>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Filesystem discovery stays on the caller's worker; registration is dispatched
/// to the platform event loop, including macOS and Windows.
#[cfg(target_os = "linux")]
enum Configuration {
    Apply(AppHandle, crate::tray::settings::Preferences),
    Stop(std::sync::mpsc::Sender<()>),
}
#[cfg(target_os = "linux")]
static WORKER: std::sync::OnceLock<std::sync::mpsc::Sender<Configuration>> =
    std::sync::OnceLock::new();

pub fn configure(app: &AppHandle, prefs: &crate::tray::settings::Preferences) {
    #[cfg(target_os = "linux")]
    {
        let worker = WORKER.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Configuration>();
            std::thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        Configuration::Apply(app, prefs) => apply(&app, &prefs),
                        Configuration::Stop(done) => {
                            let _ = registration::unregister();
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            });
            tx
        });
        let _ = worker.send(Configuration::Apply(app.clone(), prefs.clone()));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let app = app.clone();
        let handle = app.clone();
        let prefs = prefs.clone();
        let _ = handle.run_on_main_thread(move || apply(&app, &prefs));
    }
}
fn apply(handle: &AppHandle, prefs: &crate::tray::settings::Preferences) {
    let available = overlay_path().is_some();
    let mut status = Status {
        accelerator: prefs.shortcut.clone(),
        available,
        ..Default::default()
    };
    let result = if prefs.shortcuts_enabled && available {
        let pressed = handle.clone();
        registration::register_with_handler(&prefs.shortcut, move || {
            if let Ok(tray) = crate::tray::service(&pressed) {
                tray.request_shortcuts();
            }
        })
        .map(|registered| {
            status.via = Some(registered.via);
            status.hint = registered.hint;
        })
    } else {
        registration::unregister()
    };
    if let Err(error) = result {
        status.error = Some(error);
    }
    *handle
        .state::<Arc<Service>>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = status;
    let _ = handle.emit("overlay-shortcut-changed", ());
}

/// Called on the main thread when the Tools native event loop exits.
pub fn stop() {
    #[cfg(target_os = "linux")]
    if let Some(worker) = WORKER.get() {
        let (done, finished) = std::sync::mpsc::channel();
        let _ = worker.send(Configuration::Stop(done));
        let _ = finished.recv_timeout(std::time::Duration::from_secs(4));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = registration::unregister();
}
