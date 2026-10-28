use super::{desktop, worker, Active, Desktop};
use arcade_link::manifest::ids;
use arcade_tools_core::catalog::BundleApps;
use arcade_tools_core::manager::{LaunchMode, Manager, Operation, Preferences, Progress, Request};
use arcade_tools_core::updates::{Clock, HistoryEntry, SystemClock, Trigger, UpdateState};
use arcade_tools_core::{backup, dashboard, diagnostics, docs, health, removal, Error, Result};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

fn begin(app: &AppHandle) -> Result<Arc<Desktop>> {
    let state = desktop(app);
    if state
        .active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(Error::new("busy", "Another manager operation is running."));
    }
    state.cancel.store(false, Ordering::SeqCst);
    state.cancellable.store(true, Ordering::SeqCst);
    Ok(state)
}
fn progress(app: &AppHandle, state: &Desktop, progress: Progress) {
    state
        .cancellable
        .store(progress.cancellable, Ordering::SeqCst);
    let _ = app.emit("operation-progress", progress);
}
fn notify(app: &AppHandle, manager: &Manager, message: &str) {
    if manager
        .preferences()
        .is_ok_and(|prefs| prefs.notifications_enabled)
    {
        let _ = app
            .notification()
            .builder()
            .title("Arcade Tools")
            .body(message)
            .show();
    }
}
pub(super) fn wake_updates(state: &Desktop) {
    if let Some(sender) = state
        .update_wake
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = sender.send(());
    }
}
pub(super) fn start_updates(
    app: AppHandle,
    state: Arc<Desktop>,
    manager: Arc<Manager>,
) -> Result<()> {
    let updates = manager.update_state()?;
    super::set_update_count(
        &app,
        updates
            .apps
            .values()
            .filter(|update| update.update_available && !update.held_back)
            .count(),
    );
    if manager.paths.isolated {
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    *state.update_wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    std::thread::spawn(move || loop {
        let wait = manager
            .update_deadline()
            .ok()
            .flatten()
            .map(|deadline| Duration::from_secs(deadline.saturating_sub(SystemClock.now())));
        let timeout = match wait {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(()) => false,
                Err(mpsc::RecvTimeoutError::Timeout) => true,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => {
                if rx.recv().is_err() {
                    break;
                }
                false
            }
        };
        if timeout {
            if let Err(error) = check(&app, &state, &manager, Trigger::Timer, true) {
                let _ = app.emit("manager-message", error.message);
            }
        }
    });
    Ok(())
}
fn check(
    app: &AppHandle,
    state: &Arc<Desktop>,
    manager: &Arc<Manager>,
    trigger: Trigger,
    online: bool,
) -> Result<UpdateState> {
    if state.update_running.swap(true, Ordering::SeqCst) {
        return manager.update_state();
    }
    let result = (|| {
        let (updates, notifications) = manager.check_updates_with(trigger, online)?;
        for update in notifications {
            notify(
                app,
                manager,
                &format!(
                    "{} {} is available on {}.",
                    arcade_tools_core::apps::app_name(&update.app),
                    update.version,
                    update.channel.as_str()
                ),
            );
        }
        super::set_update_count(
            app,
            updates
                .apps
                .values()
                .filter(|update| update.update_available && !update.held_back)
                .count(),
        );
        let _ = app.emit("updates-changed", &updates);
        // Known use waits ten minutes; older apps still honor their busy state.
        for (id, update) in &updates.apps {
            if !update.update_available || update.held_back {
                continue;
            }
            if !manager
                .preferences()?
                .updates
                .auto_update
                .get(id)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            if state
                .active
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                break;
            }
            let _active = Active(state.clone());
            state.cancel.store(false, Ordering::SeqCst);
            if id == ids::TOOLS {
                let mode = if state.background.load(Ordering::SeqCst) {
                    LaunchMode::Background
                } else {
                    LaunchMode::Foreground
                };
                let mut hook = |staged: &arcade_tools_core::self_update::StagedSelfUpdate| {
                    crate::helpers::launch(&manager.paths, "--finish-self-update", &staged.file)?;
                    super::announce_restarting(app);
                    app.exit(0);
                    Ok(())
                };
                manager.prepare_auto_self_update(
                    manager.last_used(id),
                    &SystemClock,
                    mode,
                    &state.cancel,
                    &mut hook,
                )?;
                continue;
            }
            let outcome = manager.auto_update(
                id,
                manager.last_used(id),
                &SystemClock,
                &state.cancel,
                &mut |p| progress(app, state, p),
            );
            match outcome {
                Ok(outcome) => {
                    if let Some(message) = outcome.message {
                        notify(app, manager, &message);
                        let _ = app.emit("registry-changed", ());
                    }
                }
                Err(error) => {
                    let _ = app.emit("manager-message", error.message);
                }
            }
        }
        manager.update_state()
    })();
    state.update_running.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub async fn check_updates(app: AppHandle, online: Option<bool>) -> Result<UpdateState> {
    worker(move || {
        let state = desktop(&app);
        let manager = state.manager()?;
        check(
            &app,
            &state,
            &manager,
            Trigger::Manual,
            online.unwrap_or(true),
        )
    })
    .await
}
#[tauri::command]
pub async fn check_updates_window_open(
    app: AppHandle,
    online: Option<bool>,
) -> Result<UpdateState> {
    worker(move || {
        let state = desktop(&app);
        let manager = state.manager()?;
        check(
            &app,
            &state,
            &manager,
            Trigger::WindowOpen,
            online.unwrap_or(true),
        )
    })
    .await
}
#[tauri::command]
pub async fn preferences(app: AppHandle) -> Result<Preferences> {
    worker(move || desktop(&app).manager()?.preferences()).await
}
#[tauri::command]
pub async fn set_notifications(app: AppHandle, enabled: bool) -> Result<()> {
    worker(move || desktop(&app).manager()?.set_notifications(enabled)).await
}
#[tauri::command]
pub async fn set_theme(app: AppHandle, theme: String) -> Result<()> {
    worker(move || desktop(&app).manager()?.set_theme(&theme)).await
}
#[tauri::command]
pub async fn complete_onboarding(app: AppHandle) -> Result<()> {
    worker(move || desktop(&app).manager()?.complete_onboarding()).await
}
#[tauri::command]
pub async fn tools_app(app: AppHandle) -> Result<Option<arcade_tools_core::manager::AppView>> {
    worker(move || desktop(&app).manager()?.tools_view()).await
}
#[tauri::command]
pub async fn update_history(app: AppHandle) -> Result<Vec<HistoryEntry>> {
    worker(move || desktop(&app).manager()?.history()).await
}
#[tauri::command]
pub async fn dashboard(app: AppHandle) -> Result<dashboard::Dashboard> {
    worker(move || {
        let state = desktop(&app);
        let manager = state.manager()?;
        let result = state
            .metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sample(&manager);
        result
    })
    .await
}
#[tauri::command]
pub async fn health(app: AppHandle) -> Result<Vec<health::HealthIssue>> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        let mut issues = manager.health()?;
        let installed = manager.list(false)?;
        for engine in manager.engines(false)?.engines {
            let affected = engine
                .used_by
                .iter()
                .filter(|usage| {
                    installed
                        .iter()
                        .any(|app| app.id == usage.app && app.installed)
                })
                .collect::<Vec<_>>();
            if affected.is_empty() {
                continue;
            }
            let status = engine.detect_current();
            if matches!(
                status,
                Ok(arcade_tools_core::engines::EngineStatus::Found { .. })
            ) {
                continue;
            }
            for usage in affected {
                issues.push(health::HealthIssue {
                    app: usage.app.clone(),
                    kind: health::HealthKind::MissingEngine,
                    path: None,
                    reason: format!(
                        "{} is unavailable or too old for {}.",
                        engine.name,
                        // Catalog purposes are sentences of their own.
                        usage.purpose.trim_end_matches(['.', ' '])
                    ),
                    fix_action: health::FixAction::OpenEngines,
                    can_fix: true,
                });
            }
        }
        if let Some(index) = crate::shortcuts::read_index()? {
            for conflict in index["conflicts"].as_array().into_iter().flatten() {
                issues.push(health::HealthIssue {
                    app: "arcade.tools".into(),
                    kind: health::HealthKind::ShortcutConflict,
                    path: None,
                    reason: format!(
                        "{} is claimed by {} and {}.",
                        conflict["chord"].as_str().unwrap_or("A shortcut"),
                        conflict["a"]["label"].as_str().unwrap_or("an app"),
                        conflict["b"]["label"].as_str().unwrap_or("another app")
                    ),
                    fix_action: health::FixAction::OpenShortcuts,
                    can_fix: true,
                });
            }
        }
        Ok(issues)
    })
    .await
}
#[tauri::command]
pub async fn fix_health(app: AppHandle, id: String, action: health::FixAction) -> Result<String> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let result = state
            .manager()?
            .fix_health(&id, action, &state.cancel, &mut |p| {
                progress(&app, &state, p)
            });
        let _ = app.emit("registry-changed", ());
        result
    })
    .await
}
#[tauri::command]
pub async fn open_app_settings(app: AppHandle, id: String) -> Result<()> {
    worker(move || desktop(&app).manager()?.open_settings(&id)).await
}
#[tauri::command]
pub async fn restart_app(app: AppHandle, id: String) -> Result<()> {
    worker(move || desktop(&app).manager()?.restart(&id, None)).await
}
#[tauri::command]
pub async fn create_diagnostics(app: AppHandle, id: String) -> Result<diagnostics::Report> {
    worker(move || diagnostics::create(desktop(&app).manager()?.as_ref(), &id)).await
}
#[tauri::command]
pub async fn export_backup(app: AppHandle) -> Result<backup::BackupReport> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        backup::export(state.manager()?.as_ref(), &state.cancel)
    })
    .await
}
#[tauri::command]
pub async fn plan_restore(app: AppHandle, path: PathBuf) -> Result<backup::RestorePlan> {
    worker(move || backup::plan_restore(desktop(&app).manager()?.as_ref(), &path)).await
}
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    path: PathBuf,
    sha256: String,
) -> Result<Vec<backup::RestoreResult>> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        let result = backup::restore(&manager, &path, &sha256, &state.cancel);
        let prefs = manager.preferences()?;
        state
            .link_enabled
            .store(prefs.link_enabled, Ordering::SeqCst);
        if let Some(presence) = state
            .presence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            let tray = crate::tray::service(&app)?;
            presence.update(prefs.link_enabled, &tray.preferences());
        }
        let tray = crate::tray::service(&app)?;
        tray.apply_preferences(crate::tray::settings::Preferences::load(&manager.paths)?)?;
        wake_updates(&state);
        let _ = app.emit("registry-changed", ());
        result
    })
    .await
}
#[tauri::command]
pub async fn reveal_archive(app: AppHandle, path: PathBuf) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        let manager = desktop(&app).manager()?;
        manager.paths.guard(&path)?;
        if !path.starts_with(manager.paths.state().join("exports")) || !path.is_file() {
            return Err(Error::new("paths", "Choose a Tools export file."));
        }
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| Error::new("open", e.to_string()))
    })
    .await
}
/// Opens Tools' log folder (stderr goes there when it would be lost).
#[tauri::command]
pub async fn open_logs_folder(app: AppHandle) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        let folder = desktop(&app).manager()?.paths.logs();
        std::fs::create_dir_all(&folder).map_err(|e| Error::new("open", e.to_string()))?;
        app.opener()
            .open_path(folder.display().to_string(), None::<&str>)
            .map_err(|e| Error::new("open", e.to_string()))
    })
    .await
}
/// "Copy diagnostics": Link's shared header (STANDARD §10), then Tools' own
/// lines. No paths inside the user's documents, no secrets.
#[tauri::command]
pub async fn diagnostics_text(app: AppHandle) -> Result<String> {
    worker(move || {
        let state = desktop(&app);
        let manager = state.manager()?;
        let preferences = manager.preferences()?;
        let channel = manager
            .tools_view()?
            .filter(|tools| tools.managed)
            .map_or("development".to_string(), |tools| format!("{:?}", tools.channel).to_lowercase());
        let mut header = arcade_link::diagnostics::Header::new(ids::TOOLS, env!("CARGO_PKG_VERSION"), &channel);
        header.link_enabled = preferences.link_enabled;
        header.peers_from(&manager.paths.link);
        let mut text = header.render();
        let lines = state
            .presence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|presence| presence.diagnostics())
            .unwrap_or_default();
        for (name, value) in lines {
            text += &format!("{name}: {value}\n");
        }
        Ok(text)
    })
    .await
}
#[tauri::command]
pub async fn open_issue(app: AppHandle, id: String, issue_url: Option<String>) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        let entry = desktop(&app)
            .manager()?
            .catalog()?
            .install_entry(&id)
            .ok_or_else(|| Error::new("unsupported_input", "Unknown app."))?;
        let expected = format!("https://github.com/{}/issues/new", entry.repo);
        let url = issue_url.unwrap_or_else(|| format!("{expected}?template=bug_report.yml"));
        if !url.starts_with(&format!("{expected}?")) || url.contains(['\n', '\r']) {
            return Err(Error::new("issue", "Choose this app's issue page."));
        }
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| Error::new("open", e.to_string()))
    })
    .await
}
fn location(
    app: &AppHandle,
    manager: &Manager,
    id: &str,
    latest: bool,
) -> Result<docs::DocLocation> {
    let version = if id == "arcade.ecosystem" {
        None
    } else {
        manager
            .list(false)?
            .into_iter()
            .find(|app| app.id == id)
            .and_then(|app| app.version)
    };
    if id != "arcade.ecosystem" && manager.catalog()?.install_entry(id).is_none() {
        return Err(Error::new("docs", "Unknown guide."));
    }
    let resolver = docs::DocsResolver::new(manager.paths.clone(), super::docs_resource_dir(app));
    (if latest { resolver.resolve_latest(id) } else { resolver.resolve(id, version.as_deref(), None) })?
        .ok_or_else(|| Error::new("docs", "No offline guide is available; fetch the installed version or open the repository guide."))
}
#[tauri::command]
pub async fn docs_pages(
    app: AppHandle,
    id: String,
    latest: Option<bool>,
) -> Result<Vec<docs::PageInfo>> {
    worker(move || {
        docs::pages(&location(
            &app,
            desktop(&app).manager()?.as_ref(),
            &id,
            latest.unwrap_or(false),
        )?)
    })
    .await
}
#[tauri::command]
pub async fn read_doc(
    app: AppHandle,
    id: String,
    path: String,
    latest: Option<bool>,
) -> Result<docs::Page> {
    worker(move || {
        docs::read_page(
            &location(
                &app,
                desktop(&app).manager()?.as_ref(),
                &id,
                latest.unwrap_or(false),
            )?,
            &path,
        )
    })
    .await
}
#[tauri::command]
pub async fn fetch_latest_docs(app: AppHandle, id: String) -> Result<String> {
    worker(move || {
        let state = desktop(&app);
        let version = state.manager()?.fetch_latest_docs(&id)?;
        state
            .docs_index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        crate::tray::service(&app)?.request_index_refresh();
        Ok(version)
    })
    .await
}
#[tauri::command]
pub async fn install_catalog_bundle(
    app: AppHandle,
    bundle_id: String,
    app_ids: Vec<String>,
) -> Result<Vec<String>> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        let catalog = manager.catalog()?;
        let bundle = catalog
            .bundles
            .iter()
            .find(|b| b.id == bundle_id)
            .ok_or_else(|| Error::new("unsupported_input", "Unknown bundle."))?;
        let allowed = match &bundle.apps {
            BundleApps::All(_) => catalog.apps.iter().map(|a| &a.id).collect::<Vec<_>>(),
            BundleApps::Ids(ids) => ids.iter().collect(),
        };
        let mut selected = std::collections::BTreeSet::new();
        if app_ids.is_empty()
            || app_ids
                .iter()
                .any(|id| !allowed.contains(&id) || !selected.insert(id))
        {
            return Err(Error::new(
                "unsupported_input",
                "Choose distinct apps from the bundle.",
            ));
        }
        let mut results = Vec::new();
        for id in app_ids {
            let request = Request {
                id,
                operation: Operation::Install,
                mode: Some(LaunchMode::Background),
                remove_data: false,
                app_closed: false,
                channel: None,
                version: None,
            };
            results.push(
                manager.operate(&request, &state.cancel, &mut |p| progress(&app, &state, p))?,
            );
            let _ = app.emit("registry-changed", ());
        }
        Ok(results)
    })
    .await
}
#[tauri::command]
pub async fn plan_uninstall_all(
    app: AppHandle,
    remove_data: bool,
) -> Result<removal::UninstallPlan> {
    worker(move || desktop(&app).manager()?.plan_uninstall_all(remove_data)).await
}
#[tauri::command]
pub async fn uninstall_all(
    app: AppHandle,
    plan: removal::UninstallPlan,
) -> Result<removal::UninstallOutcome> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        let outcome =
            manager.uninstall_all(&plan, &state.cancel, &mut |p| progress(&app, &state, p))?;
        if let Some(plan) = &outcome.self_uninstall {
            let file = manager.paths.state().join("self-update/uninstall.json");
            manager.paths.guard(&file)?;
            arcade_link::paths::write_atomic(&file, &serde_json::to_vec_pretty(plan)?, true)?;
            crate::helpers::launch(&manager.paths, "--finish-self-uninstall", &file)?;
            app.exit(0);
        }
        let _ = app.emit("registry-changed", ());
        Ok(outcome)
    })
    .await
}
/// Remove only Tools: the apps stay installed and go back to updating
/// themselves. A helper removes Tools after this process exits.
#[tauri::command]
pub async fn uninstall_tools_only(app: AppHandle, remove_data: bool) -> Result<()> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        let plan = manager.prepare_self_uninstall(remove_data)?;
        let file = manager.paths.state().join("self-update/uninstall.json");
        manager.paths.guard(&file)?;
        arcade_link::paths::write_atomic(&file, &serde_json::to_vec_pretty(&plan)?, true)?;
        crate::helpers::launch(&manager.paths, "--finish-self-uninstall", &file)?;
        app.exit(0);
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn self_update(app: AppHandle) -> Result<()> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        let mut hook = |staged: &arcade_tools_core::self_update::StagedSelfUpdate| {
            crate::helpers::launch(&manager.paths, "--finish-self-update", &staged.file)?;
            super::announce_restarting(&app);
            let _ = app.emit("tools-self-update-staged", &staged.file);
            app.exit(0);
            Ok(())
        };
        manager.prepare_self_update(
            LaunchMode::Foreground,
            &state.cancel,
            &mut |p| {
                state.active.store(true, Ordering::SeqCst);
                progress(&app, &state, p);
            },
            &mut hook,
        )?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn self_rollback(app: AppHandle) -> Result<()> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        manager.prepare_self_rollback(LaunchMode::Foreground, &state.cancel, &mut |file| {
            crate::helpers::launch(&manager.paths, "--finish-self-rollback", file)?;
            super::announce_restarting(&app);
            app.exit(0);
            Ok(())
        })?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn install_tools(app: AppHandle) -> Result<()> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        // This process keeps running from its image, so the image is kept.
        let receipt = crate::installation::install("stable", false, true)?;
        arcade_tools_core::installation::claim(&manager.paths, receipt, None)?;
        let _ = app.emit("registry-changed", ());
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn download_engine(app: AppHandle, id: String) -> Result<String> {
    let state = begin(&app)?;
    worker(move || {
        let _active = Active(state.clone());
        let manager = state.manager()?;
        if id != "tesseract"
            || !manager
                .engines(false)?
                .engines
                .iter()
                .any(|engine| engine.id == id && engine.link_download)
            || arcade_link::engines::tesseract_source().is_none()
        {
            return Err(Error::new(
                "unavailable",
                "Use the platform's install guidance for this engine.",
            ));
        }
        manager.paths.guard(&arcade_link::engines::engines_dir())?;
        state.cancellable.store(false, Ordering::SeqCst);
        match arcade_link::engines::download_tesseract().map_err(|e| Error::new("engine", e))? {
            arcade_link::engines::Download::Ready(path) => {
                Ok(format!("Tesseract is ready at {}.", path.display()))
            }
            arcade_link::engines::Download::InstallerOpened => Ok(
                "The verified Tesseract installer is open; finish it and refresh detection.".into(),
            ),
        }
    })
    .await
}

#[tauri::command]
pub async fn release_history(app: AppHandle, id: String) -> Result<serde_json::Value> {
    worker(move || desktop(&app).manager()?.release_history(&id)).await
}
#[tauri::command]
pub async fn refresh_shortcut_sheets(app: AppHandle) -> Result<usize> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        let client = arcade_tools_core::catalog::CatalogClient::new(
            manager.paths.clone(),
            arcade_tools_core::source::Source::github()?,
        )?;
        let index = client.shortcut_index(true)?;
        let mut count = 0;
        for sheet in &index.sheets {
            client.shortcut_sheet(sheet, true)?;
            count += 1;
        }
        crate::tray::service(&app)?.request_index_refresh();
        Ok(count)
    })
    .await
}
