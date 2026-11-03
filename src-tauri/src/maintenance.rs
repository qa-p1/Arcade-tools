//! One event-driven worker for integration maintenance; no idle polling.
use arcade_tools_core::{manager::Manager, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Listener, Manager as _};

struct Service {
    _watcher: RecommendedWatcher,
}

pub(super) fn start(app: AppHandle, manager: Arc<Manager>) -> Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    let events = tx.clone();
    let cache_dirty = Arc::new(AtomicBool::new(true));
    let dirty = cache_dirty.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if event.paths.iter().any(|path| {
                path.extension().is_some_and(|ext| ext == "desktop")
                    || (matches!(event.kind, EventKind::Remove(_))
                        && path
                            .file_name()
                            .is_some_and(|name| name == "mimeinfo.cache"))
            }) {
                dirty.store(true, Ordering::Relaxed);
            }
            let _ = events.try_send(());
        }
    })
    .map_err(|error| arcade_tools_core::Error::new("watch", error.to_string()))?;
    let receipt = manager.paths.guard_receipt("arcade.tools")?;
    for folder in [
        manager.paths.data.join("applications"),
        manager.paths.home.join(".local/bin"),
        manager.paths.config.join("autostart"),
        manager.paths.link.registry.clone(),
        receipt.parent().unwrap().to_path_buf(),
    ] {
        manager.paths.guard(&folder)?;
        std::fs::create_dir_all(&folder)?;
        watcher
            .watch(&folder, RecursiveMode::NonRecursive)
            .map_err(|error| arcade_tools_core::Error::new("watch", error.to_string()))?;
    }
    app.manage(Service { _watcher: watcher });
    // A file event may arrive while an install owns the mutation lock. The
    // operation's completion event retries maintenance after releasing it,
    // without polling or requiring the user to open Home or click Repair.
    let completed = tx.clone();
    app.listen("registry-changed", move |_| {
        let _ = completed.try_send(());
    });
    let _ = tx.try_send(()); // Startup also repairs faults left while Tools was closed.
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // Coalesce rename/write bursts, including the repair's own events.
            while rx.recv_timeout(Duration::from_millis(250)).is_ok() {}
            match manager.maintain_integrations() {
                Ok(report) => {
                    if cache_dirty.swap(false, Ordering::Relaxed) {
                        arcade_tools_core::maintenance::refresh_desktop_caches(&manager.paths);
                    }
                    for id in &report.repaired {
                        eprintln!("Automatically repaired {id} integration");
                    }
                    for (id, reason) in &report.pending {
                        eprintln!("Automatic integration repair for {id}: {reason}");
                        let _ = app.emit(
                            "manager-message",
                            format!(
                                "Couldn't repair {} automatically. {reason}",
                                arcade_tools_core::apps::app_name(id)
                            ),
                        );
                    }
                    if !report.repaired.is_empty() {
                        let _ = app.emit("registry-changed", ());
                    }
                }
                Err(error) if error.code == "busy" => {} // The active operation owns its receipt changes.
                Err(error) => eprintln!("Automatic integration maintenance: {error}"),
            }
        }
    });
    Ok(())
}
