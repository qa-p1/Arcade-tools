#[path = "hub.rs"]
mod hub;
use crate::tray::hosting::Presence;
use crate::{hotkey, index_service, resident, tray};
use arcade_link::client::Client;
use arcade_link::manifest::ids;
use arcade_link::registry::{Registry, SharedRegistry};
use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::wire::{InvokeRequest, InvokeResult};
use arcade_link::LinkError;
use arcade_tools_core::catalog::Bundle;
use arcade_tools_core::catalog::Catalog;
use arcade_tools_core::docs::{DocSource, DocsResolver, SearchHit, SearchIndex};
use arcade_tools_core::engines::EngineCatalog;
use arcade_tools_core::engines::EngineUse;
use arcade_tools_core::manager::{AppView, CheckView, LaunchMode, Manager, Preferences, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::{Channel, Os};
use arcade_tools_core::source::Source;
use arcade_tools_core::updates::UpdateState;
use arcade_tools_core::{link, Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager as TauriManager, State};

struct Desktop {
    manager: OnceLock<Arc<Manager>>,
    presence: Mutex<Option<Presence>>,
    registry: Mutex<Option<SharedRegistry>>,
    init: Mutex<()>,
    active: AtomicBool,
    cancellable: AtomicBool,
    cancel: AtomicBool,
    background: AtomicBool,
    link_enabled: AtomicBool,
    selected: Mutex<Option<String>>,
    resident: Mutex<resident::Resident>,
    restarting: AtomicBool,
    docs_index: Mutex<Option<Arc<SearchIndex>>>,
    metrics: Mutex<arcade_tools_core::dashboard::Metrics>,
    update_wake: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    update_running: AtomicBool,
    #[cfg(debug_assertions)]
    audit: Mutex<()>,
}
impl Desktop {
    fn new(background: bool) -> Self {
        Self {
            manager: OnceLock::new(),
            presence: Mutex::new(None),
            registry: Mutex::new(None),
            init: Mutex::new(()),
            active: AtomicBool::new(false),
            cancellable: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            background: AtomicBool::new(background),
            link_enabled: AtomicBool::new(true),
            selected: Mutex::new(None),
            resident: Mutex::new(resident::Resident::new(background)),
            restarting: AtomicBool::new(false),
            docs_index: Mutex::new(None),
            metrics: Mutex::new(Default::default()),
            update_wake: Mutex::new(None),
            update_running: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            audit: Mutex::new(()),
        }
    }
    fn manager(&self) -> Result<Arc<Manager>> {
        self.manager
            .get()
            .cloned()
            .ok_or_else(|| Error::new("initializing", "Arcade Tools is starting."))
    }
}
fn desktop(app: &AppHandle) -> Arc<Desktop> {
    app.state::<Arc<Desktop>>().inner().clone()
}
fn show(app: &AppHandle, state: &Desktop) {
    state.background.store(false, Ordering::SeqCst);
    state.resident.lock().unwrap().open();
    let app = app.clone();
    let handle = app.clone();
    let _ = handle.run_on_main_thread(move || {
        let window = app.get_webview_window("main").or_else(|| {
            app.config().app.windows.first().and_then(|config| {
                tauri::WebviewWindowBuilder::from_config(&app, config)
                    .ok()?
                    .build()
                    .ok()
                    .inspect(|window| {
                        #[cfg(target_os = "linux")]
                        let _ = window.with_webview(|webview| {
                            use webkit2gtk::{SettingsExt, WebViewExt};
                            if let Some(settings) = webview.inner().settings() {
                                // This document UI needs no GPU compositor or
                                // retained page cache after its window closes.
                                settings.set_hardware_acceleration_policy(
                                    webkit2gtk::HardwareAccelerationPolicy::Never,
                                );
                                settings.set_enable_page_cache(false);
                            }
                        });
                        #[cfg(not(target_os = "linux"))]
                        let _ = window;
                    })
            })
        });
        if let Some(window) = window {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
}

fn reclaim_resident_memory(app: AppHandle) {
    // Let GTK/WebKit finish deferred destruction, then release unused
    // allocator and library pages once. No periodic idle timer.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if app.get_webview_window("main").is_none() {
            resident::release_unused_runtime_pages();
        }
    });
}
fn audit(app: &AppHandle, event: Value) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let state = desktop(app);
        let _audit = state.audit.lock().unwrap();
        if let (Some(path), Ok(manager)) =
            (std::env::var_os("ARCADE_TOOLS_SMOKE_LOG"), state.manager())
        {
            let path = std::path::PathBuf::from(path);
            // The Link e2e runner places its report beside ARCADE_HOME, while
            // Paths::discover() roots Tools under ARCADE_HOME itself.
            let runner_root = std::env::var_os("ARCADE_E2E_ROOT")
                .map(std::path::PathBuf::from)
                .filter(|root| root.starts_with("/tmp"));
            let inside_isolated_root = path.starts_with(manager.paths.home.parent().unwrap());
            let inside_runner_root = runner_root.is_some_and(|root| path.starts_with(root));
            if manager.paths.isolated && (inside_isolated_root || inside_runner_root) {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(file, "{event}");
                }
            }
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = (app, event);
}
struct LinkHandler {
    app: AppHandle,
    state: Arc<Desktop>,
}
impl Handler for LinkHandler {
    fn methods(&self) -> Vec<String> {
        vec!["app.settings".into()]
    }
    fn settings(&self) -> std::result::Result<(), LinkError> {
        if let Ok(service) = tray::service(&self.app) {
            service.navigate("settings");
        }
        show(&self.app, &self.state);
        Ok(())
    }
    fn describe(&self) -> Vec<arcade_link::Action> {
        if self.state.link_enabled.load(Ordering::SeqCst) {
            link::actions()
        } else {
            vec![]
        }
    }
    fn invoke(
        &self,
        request: InvokeRequest,
        _: &InvokeContext,
    ) -> std::result::Result<Reply, LinkError> {
        if !self.state.link_enabled.load(Ordering::SeqCst) {
            return Err(LinkError::denied("disabled"));
        }
        let id = link::install_target(&request)?;
        if self.state.active.load(Ordering::SeqCst) {
            return Err(LinkError::busy());
        }
        *self.state.selected.lock().unwrap() = Some(id.clone());
        show(&self.app, &self.state);
        let _ = self.app.emit("install-request", &id);
        audit(&self.app, json!({"event":"handoff", "id":id}));
        Ok(Reply::Done(InvokeResult {
            message: Some("Review the installation in Arcade Tools.".into()),
            data: Some(json!({"app": id, "confirmationRequired": true})),
            ..Default::default()
        }))
    }
    fn status(&self) -> Value {
        let background = self.state.background.load(Ordering::SeqCst);
        json!({"mode": if background { "background" } else { "foreground" }, "background": background, "busy": self.state.active.load(Ordering::SeqCst)})
    }
    fn activate(&self) -> std::result::Result<(), LinkError> {
        show(&self.app, &self.state);
        Ok(())
    }
    fn quit(&self) -> std::result::Result<(), LinkError> {
        if self.state.active.load(Ordering::SeqCst) {
            return Err(LinkError::busy());
        }
        request_quit(&self.app);
        Ok(())
    }
}
async fn worker<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| Error::new("worker", e.to_string()))?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    apps: Vec<AppView>,
    preferences: Preferences,
    updates: UpdateState,
    selected: Option<String>,
    diagnostics: Vec<(String, String)>,
    testing: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPayload {
    catalog: Catalog,
    engines: EngineCatalog,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnAppView {
    id: String,
    name: String,
    installed_version: Option<String>,
    docs_version: Option<String>,
    available: bool,
    source: Option<DocSource>,
    needs_fetch: bool,
    docs_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineView {
    id: String,
    name: String,
    homepage: String,
    binaries: Vec<String>,
    version_args: Vec<String>,
    minimum_version: Option<String>,
    used_by: Vec<EngineUse>,
    status: Value,
    install_hint: Option<String>,
    link_download: bool,
    downloaded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionActionView {
    id: String,
    title: String,
    verb: String,
    available: bool,
    reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionPeerView {
    id: String,
    name: String,
    version: String,
    link_enabled: bool,
    reachable: bool,
    busy: Option<bool>,
    protocols: Vec<u32>,
    actions: Vec<ConnectionActionView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionsView {
    link_enabled: bool,
    peers: Vec<ConnectionPeerView>,
}

fn docs_resource_dir(app: &AppHandle) -> PathBuf {
    let root = app
        .path()
        .resource_dir()
        .map(|root| root.join("docs"))
        .unwrap_or_else(|_| PathBuf::from("__arcade_docs_snapshot_missing__"));
    #[cfg(debug_assertions)]
    if !root.join("manifest.json").exists() {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/docs");
    }
    root
}

fn docs_url(repo: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    let path = if path == "README.md" {
        "docs/user"
    } else {
        path
    };
    let route = if path.to_ascii_lowercase().ends_with(".md") {
        "blob"
    } else {
        "tree"
    };
    format!("https://github.com/{repo}/{route}/main/{path}")
}

fn build_docs_index(app: &AppHandle, manager: &Manager) -> Result<SearchIndex> {
    let catalog = manager.catalog()?;
    let versions = manager
        .list(false)?
        .into_iter()
        .map(|entry| (entry.id, entry.version))
        .collect::<std::collections::BTreeMap<_, _>>();
    let resolver = DocsResolver::new(manager.paths.clone(), docs_resource_dir(app));
    let mut locations = Vec::new();
    for entry in &catalog.apps {
        if let Some(location) = resolver.resolve(
            &entry.id,
            versions.get(&entry.id).and_then(Option::as_deref),
            None,
        )? {
            locations.push(location);
        }
    }
    if let Some(location) = resolver.resolve("arcade.ecosystem", None, None)? {
        locations.push(location);
    }
    SearchIndex::build(&locations)
}

fn cached_docs_index(
    app: &AppHandle,
    state: &Desktop,
    manager: &Manager,
) -> Result<Arc<SearchIndex>> {
    if let Some(index) = state.docs_index.lock().unwrap().clone() {
        return Ok(index);
    }
    let index = Arc::new(build_docs_index(app, manager)?);
    let mut cached = state.docs_index.lock().unwrap();
    Ok(cached.get_or_insert_with(|| index.clone()).clone())
}

#[tauri::command]
async fn initialize(app: AppHandle) -> Result<Bootstrap> {
    worker(move || {
        let state = desktop(&app);
        let manager = initialize_desktop(&app)?;
        if !state.background.load(Ordering::SeqCst) {
            show(&app, &state);
        }
        let selected = state.selected.lock().unwrap().take();
        let diagnostics = state
            .presence
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.diagnostics())
            .unwrap_or_default();
        Ok(Bootstrap {
            apps: manager.list(true)?,
            preferences: manager.preferences()?,
            updates: manager.update_state()?,
            selected,
            diagnostics,
            testing: cfg!(debug_assertions) && manager.paths.isolated,
        })
    })
    .await
}
fn initialize_desktop(app: &AppHandle) -> Result<Arc<Manager>> {
    let state = desktop(app);
    let _init = state.init.lock().unwrap();
    if state.manager.get().is_none() {
        let paths = Paths::discover()?;
        #[cfg(debug_assertions)]
        let mut paths = paths;
        #[cfg(debug_assertions)]
        let source = if let Ok(url) = std::env::var("ARCADE_TOOLS_TEST_SOURCE") {
            let root = paths
                .home
                .parent()
                .ok_or_else(|| Error::new("isolation", "Test HOME has no parent."))?;
            if !root.starts_with("/tmp")
                || ![
                    &paths.config,
                    &paths.data,
                    &paths.cache,
                    &paths.link.registry,
                ]
                .iter()
                .all(|p| p.starts_with(root))
            {
                return Err(Error::new(
                    "isolation",
                    "A test source requires HOME, XDG and ARCADE_HOME inside one /tmp test root.",
                ));
            }
            paths.isolated = true;
            Source::test_server(&url)?
        } else {
            Source::github()?
        };
        #[cfg(not(debug_assertions))]
        let source = Source::github()?;
        let manager = Arc::new(Manager::new(paths, source));
        let prefs = manager.preferences()?;
        state
            .link_enabled
            .store(prefs.link_enabled, Ordering::SeqCst);
        state
            .manager
            .set(manager.clone())
            .map_err(|_| Error::new("initializing", "Initialization already finished."))?;
        let resources = app.path().resource_dir().ok();
        let options = index_service::Options::discover(&manager.paths, resources.as_deref())?;
        let handle = app.clone();
        let index = index_service::IndexService::start(options, move |status| {
            let _ = handle.emit("shortcut-index-built", status);
        })?;
        app.manage(index.clone());
        let tools_handle = app.clone();
        let settings_handle = app.clone();
        let registry_handle = app.clone();
        let tray = tray::Service::start(
            app.clone(),
            manager.clone(),
            move |action| match action {
                tray::model::Action::OpenTools => show(&tools_handle, &desktop(&tools_handle)),
                tray::model::Action::Updates => {
                    if let Ok(tray) = tray::service(&tools_handle) {
                        tray.navigate("apps");
                    }
                    show(&tools_handle, &desktop(&tools_handle));
                }
                tray::model::Action::QuitTools => request_quit(&tools_handle),
                _ => {}
            },
            move |prefs| {
                let state = desktop(&settings_handle);
                {
                    let mut resident = state.resident.lock().unwrap();
                    resident.hosting = prefs.host_icons;
                    resident.shortcuts = prefs.shortcuts_enabled;
                }
                if let Some(presence) = state.presence.lock().unwrap().as_mut() {
                    presence.update(state.link_enabled.load(Ordering::SeqCst), &prefs);
                };
                hotkey::configure(&settings_handle, &prefs);
            },
            move || {
                index.changed();
                let _ = registry_handle.emit("registry-changed", ());
            },
        )?;
        let tray_prefs = tray.preferences();
        {
            let mut resident = state.resident.lock().unwrap();
            resident.hosting = tray_prefs.host_icons;
            resident.shortcuts = tray_prefs.shortcuts_enabled;
        }
        let presence = Presence::start(
            manager.paths.link.clone(),
            tray.host(),
            Arc::new(LinkHandler {
                app: app.clone(),
                state: state.clone(),
            }),
            prefs.link_enabled,
            &tray_prefs,
        );
        app.manage(tray);
        *state.presence.lock().unwrap() = Some(presence);
        hotkey::configure(app, &tray_prefs);
        let registry = SharedRegistry::load(&manager.paths.link);
        let handle = app.clone();
        let locations = manager.paths.link.clone();
        let initial = watch_state(&registry.snapshot(), &locations);
        let last = Mutex::new(initial);
        let docs_state = state.clone();
        if !registry.watch(move |registry| {
            // SharedRegistry also reports endpoint reads. Probing those files
            // must not trigger another probe or cost anything while idle.
            let current = watch_state(registry, &locations);
            let mut previous = last.lock().unwrap();
            if *previous != current {
                *previous = current;
                docs_state.docs_index.lock().unwrap().take();
                let _ = handle.emit("registry-changed", ());
            }
        }) {
            return Err(Error::new("watch", "Could not watch the Arcade registry."));
        }
        *state.registry.lock().unwrap() = Some(registry);
        hub::start_updates(app.clone(), state.clone(), manager)?;
    }
    state.manager()
}
fn watch_state(
    registry: &Registry,
    locations: &arcade_link::Locations,
) -> (String, Vec<Option<(std::time::SystemTime, u64)>>) {
    let manifests = serde_json::to_string(registry.apps()).unwrap_or_default();
    let endpoints = arcade_tools_core::apps::APPS
        .iter()
        .chain(std::iter::once(&"arcade.tools"))
        .map(|id| {
            std::fs::metadata(locations.endpoint(id))
                .ok()
                .and_then(|m| m.modified().ok().map(|time| (time, m.len())))
        })
        .collect();
    (manifests, endpoints)
}
#[tauri::command]
async fn list_apps(app: AppHandle) -> Result<Vec<AppView>> {
    worker(move || desktop(&app).manager()?.list(true)).await
}
#[tauri::command]
async fn check_release(app: AppHandle, id: String) -> Result<CheckView> {
    worker(move || desktop(&app).manager()?.check(&id)).await
}
#[tauri::command]
async fn update_state(app: AppHandle) -> Result<UpdateState> {
    worker(move || desktop(&app).manager()?.update_state()).await
}
#[tauri::command]
async fn set_update_checks(app: AppHandle, enabled: bool) -> Result<()> {
    worker(move || {
        let state = desktop(&app);
        state.manager()?.set_updates_enabled(enabled)?;
        hub::wake_updates(&state);
        Ok(())
    })
    .await
}
#[tauri::command]
async fn set_auto_update(app: AppHandle, id: String, enabled: bool) -> Result<()> {
    worker(move || desktop(&app).manager()?.set_auto_update(&id, enabled)).await
}
#[tauri::command]
async fn list_bundles(app: AppHandle) -> Result<Vec<Bundle>> {
    worker(move || Ok(desktop(&app).manager()?.catalog()?.bundles)).await
}
#[tauri::command]
async fn list_engines(app: AppHandle, refresh: bool) -> Result<Vec<EngineView>> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        let catalog = manager.engines(refresh)?;
        let os = Os::current();
        let os_release = if os == Os::Linux {
            std::fs::read_to_string("/etc/os-release").unwrap_or_default()
        } else {
            String::new()
        };
        catalog
            .engines
            .into_iter()
            .map(|engine| {
                let install_hint = engine.install_hint(os, &os_release).map(str::to_owned);
                let status = match engine.detect_current() {
                    Ok(status) => serde_json::to_value(status)?,
                    Err(error) => json!({"status":"error", "message":error.message}),
                };
                let link_download = engine.link_download
                    && engine.id == "tesseract"
                    && arcade_link::engines::tesseract_source().is_some();
                let downloaded = status
                    .get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|path| {
                        PathBuf::from(path).starts_with(arcade_link::engines::engines_dir())
                    });
                Ok(EngineView {
                    id: engine.id,
                    name: engine.name,
                    homepage: engine.homepage,
                    binaries: engine.detect.binaries,
                    version_args: engine.detect.version_args,
                    minimum_version: engine.detect.min_version,
                    used_by: engine.used_by,
                    status,
                    install_hint,
                    link_download,
                    downloaded,
                })
            })
            .collect()
    })
    .await
}
#[tauri::command]
async fn list_connections(app: AppHandle) -> Result<ConnectionsView> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        let link_enabled = manager.preferences()?.link_enabled;
        let registry = Registry::load(&manager.paths.link);
        let me = arcade_tools_core::manager::me();
        let peers = registry
            .peers(ids::TOOLS)
            .map(|manifest| {
                let (reachable, busy) = match Client::connect_with(
                    &manager.paths.link,
                    &manifest.id,
                    &me,
                    Duration::from_millis(500),
                ) {
                    Ok(mut client) => match client.status() {
                        Ok(status) => (
                            true,
                            status
                                .pointer("/status/busy")
                                .or_else(|| status.get("busy"))
                                .and_then(Value::as_bool),
                        ),
                        Err(_) => (false, None),
                    },
                    Err(_) => (false, None),
                };
                ConnectionPeerView {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    link_enabled: manifest.settings.link_enabled,
                    reachable,
                    busy,
                    protocols: manifest.link.protocol.clone(),
                    actions: manifest
                        .actions
                        .iter()
                        .map(|action| ConnectionActionView {
                            id: action.id.clone(),
                            title: action.title.clone(),
                            verb: action.verb.clone(),
                            available: action.available,
                            reason: action.reason.clone(),
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(ConnectionsView {
            link_enabled,
            peers,
        })
    })
    .await
}
#[tauri::command]
async fn list_learn_apps(app: AppHandle) -> Result<Vec<LearnAppView>> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        let catalog = manager.catalog()?;
        let versions = manager
            .list(false)?
            .into_iter()
            .map(|entry| (entry.id, entry.version))
            .collect::<std::collections::BTreeMap<_, _>>();
        let resolver = DocsResolver::new(manager.paths.clone(), docs_resource_dir(&app));
        let ecosystem = resolver.resolve("arcade.ecosystem", None, None)?;
        let mut apps = vec![LearnAppView {
            id: "arcade.ecosystem".into(),
            name: "Arcade guides".into(),
            installed_version: None,
            docs_version: ecosystem.as_ref().map(|location| location.version.clone()),
            available: ecosystem.is_some(),
            source: ecosystem.as_ref().map(|location| location.source),
            needs_fetch: false,
            docs_url: "https://github.com/qa-p1/Arcade-catalog/tree/main/docs".into(),
        }];
        let guides = catalog
            .apps
            .into_iter()
            .map(|entry| {
                let installed_version = versions.get(&entry.id).cloned().flatten();
                let location = resolver.resolve(&entry.id, installed_version.as_deref(), None)?;
                Ok(LearnAppView {
                    id: entry.id,
                    name: entry.name,
                    docs_version: location.as_ref().map(|item| item.version.clone()),
                    available: location.is_some(),
                    source: location.as_ref().map(|item| item.source),
                    needs_fetch: installed_version.is_some()
                        && location.as_ref().is_none_or(|item| item.needs_fetch),
                    docs_url: docs_url(&entry.repo, &entry.docs),
                    installed_version,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        apps.extend(guides);
        Ok(apps)
    })
    .await
}
#[tauri::command]
async fn search_docs(app: AppHandle, query: String) -> Result<Vec<SearchHit>> {
    worker(move || {
        let state = desktop(&app);
        let manager = state.manager()?;
        let index = cached_docs_index(&app, &state, &manager)?;
        Ok(index.search(&query, 30))
    })
    .await
}
#[tauri::command]
async fn fetch_docs(app: AppHandle, id: String, version: String) -> Result<()> {
    let state = desktop(&app);
    worker(move || {
        state.manager()?.fetch_docs(&id, &version)?;
        state.docs_index.lock().unwrap().take();
        Ok(())
    })
    .await
}
#[tauri::command]
async fn open_docs(app: AppHandle, id: String) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        let manager = desktop(&app).manager()?;
        if id == "arcade.ecosystem" {
            return app
                .opener()
                .open_url(
                    "https://github.com/qa-p1/Arcade-catalog/tree/main/docs",
                    None::<&str>,
                )
                .map_err(|e| Error::new("open", e.to_string()));
        }
        let catalog = manager.catalog()?;
        let entry = catalog
            .app(&id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        app.opener()
            .open_url(docs_url(&entry.repo, &entry.docs), None::<&str>)
            .map_err(|error| Error::new("open", error.to_string()))
    })
    .await
}
struct Active(Arc<Desktop>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
        self.0.cancellable.store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
async fn operate(app: AppHandle, request: Request) -> Result<String> {
    let state = desktop(&app);
    if state
        .active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(Error::new("busy", "Another manager operation is running."));
    }
    state.cancel.store(false, Ordering::SeqCst);
    state.cancellable.store(true, Ordering::SeqCst);
    worker(move || {
        let _active = Active(state.clone());
        let result = state.manager()?.operate(&request, &state.cancel, &mut |progress| {
            state.cancellable.store(progress.cancellable, Ordering::SeqCst);
            let _ = app.emit("operation-progress", &progress);
        });
        audit(&app, json!({"event":"operation", "id":request.id, "operation":request.operation, "ok":result.is_ok()}));
        let _ = app.emit("registry-changed", ()); result
    }).await
}
#[tauri::command]
fn cancel_operation(state: State<'_, Arc<Desktop>>) -> Result<()> {
    if !state.active.load(Ordering::SeqCst) || !state.cancellable.load(Ordering::SeqCst) {
        return Err(Error::new(
            "unavailable",
            "The installer is committing changes. Wait for it to finish.",
        ));
    }
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
async fn set_channel(app: AppHandle, id: String, channel: Channel) -> Result<()> {
    worker(move || desktop(&app).manager()?.set_channel(&id, channel)).await
}
#[tauri::command]
async fn launch_app(app: AppHandle, id: String, mode: LaunchMode) -> Result<()> {
    worker(move || desktop(&app).manager()?.launch(&id, mode)).await
}
#[tauri::command]
async fn set_login(app: AppHandle, id: String, enabled: bool) -> Result<()> {
    worker(move || desktop(&app).manager()?.start_at_login(&id, enabled)).await
}
#[tauri::command]
async fn set_link(app: AppHandle, enabled: bool) -> Result<()> {
    worker(move || {
        let state = desktop(&app);
        state.manager()?.set_link(enabled)?;
        state.link_enabled.store(enabled, Ordering::SeqCst);
        if let Some(presence) = state.presence.lock().unwrap().as_mut() {
            presence.update(enabled, &tray::service(&app)?.preferences());
        }
        Ok(())
    })
    .await
}
#[tauri::command]
async fn open_releases(app: AppHandle, id: String) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        arcade_tools_core::paths::app_id(&id)?;
        app.opener()
            .open_url(arcade_link::manifest::releases_url(&id), None::<&str>)
            .map_err(|e| Error::new("open", e.to_string()))
    })
    .await
}
fn catalog_payload(manager: &Manager, refresh: bool) -> Result<CatalogPayload> {
    let catalog = if refresh {
        manager.refresh_catalog()?
    } else {
        manager.catalog()?
    };
    let engines = manager.engines(refresh)?;
    Ok(CatalogPayload { catalog, engines })
}
#[tauri::command]
async fn catalog_data(app: AppHandle) -> Result<CatalogPayload> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        catalog_payload(&manager, false)
    })
    .await
}
#[tauri::command]
async fn refresh_catalog(app: AppHandle) -> Result<CatalogPayload> {
    worker(move || {
        let manager = desktop(&app).manager()?;
        catalog_payload(&manager, true)
    })
    .await
}
#[tauri::command]
async fn open_catalog_docs(app: AppHandle, id: String) -> Result<()> {
    worker(move || {
        use tauri_plugin_opener::OpenerExt;
        let manager = desktop(&app).manager()?;
        let catalog = manager.catalog()?;
        let entry = catalog
            .app(&id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resource_kind = if std::path::Path::new(&entry.docs).extension().is_some() {
            "blob"
        } else {
            "tree"
        };
        let url = format!(
            "https://github.com/{}/{}/{}/{}",
            entry.repo, resource_kind, "main", entry.docs
        );
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| Error::new("open", error.to_string()))
    })
    .await
}
#[tauri::command]
async fn render_markdown(markdown: String) -> Result<String> {
    worker(move || {
        if markdown.len() > 1024 * 1024 {
            return Err(Error::new(
                "too_large",
                "Release notes exceed the rendering limit.",
            ));
        }
        Ok(arcade_tools_core::markdown::render(&markdown, &[]))
    })
    .await
}
#[tauri::command]
async fn ui_rendered(app: AppHandle, report: Value) -> Result<()> {
    worker(move || {
        audit(&app, json!({"event":"rendered", "report":report}));
        Ok(())
    })
    .await
}

fn request_close(app: &AppHandle) {
    let state = desktop(app);
    let close = state
        .resident
        .lock()
        .unwrap()
        .close(state.active.load(Ordering::SeqCst));
    if close == resident::Close::DestroyWindow {
        state.background.store(true, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.destroy();
        }
    } else if close == resident::Close::Busy {
        let _ = app.emit(
            "manager-message",
            "Wait for the current operation to finish before closing Arcade Tools.",
        );
    } else {
        request_quit(app);
    }
}

fn request_quit(app: &AppHandle) {
    let state = desktop(app);
    if state.active.load(Ordering::SeqCst) {
        let _ = app.emit(
            "manager-message",
            "Wait for the current operation before quitting Arcade Tools.",
        );
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        state.presence.lock().unwrap().take();
        if let Ok(service) = tray::service(&app) {
            service.stop(state.restarting.load(Ordering::SeqCst));
        }
        if let Some(index) = app.try_state::<Arc<index_service::IndexService>>() {
            index.stop();
        }
        app.exit(0);
    });
}

/// Core self-update/restart calls this on its worker before stopping Tools.
#[expect(dead_code, reason = "Core self-update/restart integration hook")]
pub fn announce_restarting(app: &AppHandle) {
    desktop(app).restarting.store(true, Ordering::SeqCst);
    if let Some(presence) = desktop(app).presence.lock().unwrap().as_mut() {
        presence.announce_restarting();
    }
}

/// Core calls this with its cached count, without checking the network here.
#[expect(dead_code, reason = "Core update scheduling integration hook")]
pub fn set_update_count(app: &AppHandle, count: usize) {
    if let Ok(service) = tray::service(app) {
        service.set_updates(count);
    }
}
pub fn run(background: bool, quit: bool) {
    tauri::Builder::default()
        .manage(Arc::new(Desktop::new(background)))
        .manage(Arc::new(hotkey::Service::default()))
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if args.iter().any(|a| a == "--quit") {
                request_quit(app);
            } else if !args.iter().any(|a| a == "--background") {
                show(app, &desktop(app));
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|window, payload| {
            audit(window.app_handle(), json!({"event":"page-load", "url":payload.url().as_str(), "stage":format!("{:?}", payload.event())}));
        })
        .setup(move |app| {
            if quit {
                // No existing instance: quit is a no-op, without initializing
                // the manager or writing a manifest.
                app.handle().exit(0);
                return Ok(());
            }
            let handle = app.handle().clone();
            let state = desktop(&handle);
            if !state.background.load(Ordering::SeqCst) {
                show(&handle, &state);
            }
            // Background presence must not depend on a hidden webview loading.
            std::thread::spawn(move || {
                if let Err(error) = initialize_desktop(&handle) {
                    let _ = handle.emit("manager-message", &error.message);
                    audit(
                        &handle,
                        json!({"event":"initialization-error", "message":error.message}),
                    );
                } else {
                    reclaim_resident_memory(handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            initialize,
            list_apps,
            check_release,
            operate,
            cancel_operation,
            set_channel,
            launch_app,
            set_login,
            set_link,
            open_releases,
            catalog_data,
            refresh_catalog,
            open_catalog_docs,
            render_markdown,
            update_state,
            set_update_checks,
            set_auto_update,
            list_bundles,
            list_engines,
            list_connections,
            list_learn_apps,
            search_docs,
            fetch_docs,
            open_docs,
            hub::check_updates,
            hub::check_updates_window_open,
            hub::preferences,
            hub::set_notifications,
            hub::set_theme,
            hub::complete_onboarding,
            hub::tools_app,
            hub::update_history,
            hub::dashboard,
            hub::health,
            hub::fix_health,
            hub::open_app_settings,
            hub::restart_app,
            hub::create_diagnostics,
            hub::export_backup,
            hub::plan_restore,
            hub::restore_backup,
            hub::reveal_archive,
            hub::open_issue,
            hub::docs_pages,
            hub::read_doc,
            hub::install_catalog_bundle,
            hub::plan_uninstall_all,
            hub::uninstall_all,
            hub::self_update,
            hub::install_tools,
            hub::download_engine,
            ui_rendered,
            crate::shortcuts::shortcuts_index,
            crate::shortcuts::shortcut_labels,
            crate::shortcuts::set_shortcut_label,
            tray::tray_get_settings,
            tray::tray_set_settings,
            tray::tray_ui_intent,
            tray::rebuild_shortcut_index,
            tray::shortcut_index_status,
            tray::configure_shortcut_sources
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                request_close(window.app_handle());
            }
            if let tauri::WindowEvent::Destroyed = event {
                desktop(window.app_handle()).resident.lock().unwrap().destroyed();
                reclaim_resident_memory(window.app_handle().clone());
            }
        })
        .build(tauri::generate_context!())
        .expect("could not build Arcade Tools")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event { hotkey::stop(); }
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                if desktop(app).resident.lock().unwrap().keeps_running() { api.prevent_exit(); }
            }
        });
}
