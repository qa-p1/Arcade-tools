use arcade_link::registry::{Registry, SharedRegistry};
use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::wire::{InvokeRequest, InvokeResult};
use arcade_link::{LinkError, Presence};
use arcade_tools_core::manager::{AppView, CheckView, LaunchMode, Manager, Preferences, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::Channel;
use arcade_tools_core::source::Source;
use arcade_tools_core::{link, Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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
    let app = app.clone();
    let handle = app.clone();
    let _ = handle.run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
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
            if manager.paths.isolated && path.starts_with(manager.paths.home.parent().unwrap()) {
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
        self.state.presence.lock().unwrap().take();
        self.app.exit(0);
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
    selected: Option<String>,
    diagnostics: Vec<(String, String)>,
    testing: bool,
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
        let diagnostics = arcade_link::presence::diagnostics(
            state.presence.lock().unwrap().as_ref(),
            &manager.paths.link,
        );
        Ok(Bootstrap {
            apps: manager.list(true)?,
            preferences: manager.preferences()?,
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
        let presence = Presence::start(
            manager.paths.link.clone(),
            link::manifest(prefs.link_enabled),
            Arc::new(LinkHandler {
                app: app.clone(),
                state: state.clone(),
            }),
        );
        *state.presence.lock().unwrap() = Some(presence);
        let registry = SharedRegistry::load(&manager.paths.link);
        let handle = app.clone();
        let locations = manager.paths.link.clone();
        let initial = watch_state(&registry.snapshot(), &locations);
        let last = Mutex::new(initial);
        if !registry.watch(move |registry| {
            // SharedRegistry also reports endpoint reads. Probing those files
            // must not trigger another probe or cost anything while idle.
            let current = watch_state(registry, &locations);
            let mut previous = last.lock().unwrap();
            if *previous != current {
                *previous = current;
                let _ = handle.emit("registry-changed", ());
            }
        }) {
            return Err(Error::new("watch", "Could not watch the Arcade registry."));
        }
        *state.registry.lock().unwrap() = Some(registry);
    }
    state.manager()
}
fn watch_state(
    registry: &Registry,
    locations: &arcade_link::Locations,
) -> (String, Vec<Option<(std::time::SystemTime, u64)>>) {
    let manifests = serde_json::to_string(registry.apps()).unwrap_or_default();
    let endpoints = arcade_link::manifest::ids::APPS
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
        if let Some(presence) = state.presence.lock().unwrap().as_ref() {
            presence.update(link::manifest(enabled));
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
#[tauri::command]
async fn ui_rendered(app: AppHandle, report: Value) -> Result<()> {
    worker(move || {
        audit(&app, json!({"event":"rendered", "report":report}));
        Ok(())
    })
    .await
}

pub fn run(background: bool) {
    tauri::Builder::default()
        .manage(Arc::new(Desktop::new(background)))
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|a| a == "--background") {
                show(app, &desktop(app));
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .on_page_load(|window, payload| {
            audit(window.app_handle(), json!({"event":"page-load", "url":payload.url().as_str(), "stage":format!("{:?}", payload.event())}));
        })
        .setup(|app| {
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
            ui_rendered
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                let state = desktop(&app);
                if state.active.load(Ordering::SeqCst) {
                    let _ = app.emit(
                        "manager-message",
                        "Wait for the current operation to finish before closing Arcade Tools.",
                    );
                } else {
                    std::thread::spawn(move || {
                        state.presence.lock().unwrap().take();
                        app.exit(0);
                    });
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("could not run Arcade Tools");
}
