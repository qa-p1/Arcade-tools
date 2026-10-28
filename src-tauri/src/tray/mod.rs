mod assets;
pub mod hosting;
pub mod model;
mod render;
pub mod settings;
mod transport;

use arcade_link::{
    client::{Client, ConnectionControl},
    manifest::ids,
    registry::SharedRegistry,
    trayhost::TrayHostServer,
};
use arcade_tools_core::{
    manager::{me, Manager},
    Error, Result,
};
#[cfg(not(windows))]
use arcade_tools_core::{platform, release::Os};
use model::{Action, PeerState, TrayModel};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager as _};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

enum Event {
    Refresh,
    Changed(String),
    Disconnected { id: String, token: String },
    Click(Action),
    Stop,
}
struct PeerWatch {
    token: String,
    control: ConnectionControl,
}

pub struct Service {
    app: AppHandle,
    manager: Arc<Manager>,
    prefs: Mutex<settings::Preferences>,
    registry: SharedRegistry,
    host: TrayHostServer,
    peers: Mutex<BTreeMap<String, PeerWatch>>,
    states: Mutex<BTreeMap<String, PeerState>>,
    renderer: Mutex<Option<render::Renderer>>,
    tx: mpsc::Sender<Event>,
    updates: AtomicUsize,
    navigation: Mutex<Option<String>>,
    on_tools: Arc<dyn Fn(Action) + Send + Sync>,
    on_settings: Arc<dyn Fn(settings::Preferences) + Send + Sync>,
    on_registry: Arc<dyn Fn() + Send + Sync>,
}

impl Service {
    pub fn start(
        app: AppHandle,
        manager: Arc<Manager>,
        on_tools: impl Fn(Action) + Send + Sync + 'static,
        on_settings: impl Fn(settings::Preferences) + Send + Sync + 'static,
        on_registry: impl Fn() + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        let mut prefs = settings::Preferences::load(&manager.paths)?;
        if settings::login_available(&manager.paths) {
            match crate::resident::login::installer_state(&manager.paths) {
                // The installer's "Start at login" choice stands until changed here.
                Ok(Some(enabled)) => prefs.start_at_login = enabled,
                Ok(None) => {
                    if let Err(error) = prefs.apply_login(
                        &manager.paths,
                        std::path::Path::new(&arcade_link::manifest::current_executable()),
                    ) {
                        prefs.start_at_login = false;
                        let _ = app.emit("manager-message", error.message);
                    }
                }
                Err(error) => {
                    let _ = app.emit("manager-message", error.message);
                }
            }
        }
        let registry = SharedRegistry::load(&manager.paths.link);
        let (tx, rx) = mpsc::channel();
        let service = Arc::new(Self {
            app,
            manager,
            prefs: Mutex::new(prefs),
            registry,
            host: TrayHostServer::new(false),
            peers: Mutex::new(BTreeMap::new()),
            states: Mutex::new(BTreeMap::new()),
            renderer: Mutex::new(None),
            tx,
            updates: AtomicUsize::new(0),
            navigation: Mutex::new(None),
            on_tools: Arc::new(on_tools),
            on_settings: Arc::new(on_settings),
            on_registry: Arc::new(on_registry),
        });
        let clicks = service.tx.clone();
        let renderer = render::Renderer::start(
            &service.app,
            TrayModel::default(),
            Arc::new(move |action| {
                let _ = clicks.send(Event::Click(action));
            }),
        )
        .map_err(|e| Error::new("tray", e))?;
        *service.renderer.lock().unwrap_or_else(|e| e.into_inner()) = Some(renderer);
        let changes = service.tx.clone();
        if !service.registry.watch(move |_| {
            let _ = changes.send(Event::Refresh);
        }) {
            return Err(Error::new(
                "tray_watch",
                "Could not watch installed Arcade apps.",
            ));
        }
        let weak = Arc::downgrade(&service);
        std::thread::Builder::new()
            .name("tools-tray".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    let Some(service) = weak.upgrade() else { break };
                    match event {
                        Event::Stop => break,
                        Event::Refresh => {
                            service.refresh();
                            (service.on_registry)();
                        }
                        Event::Changed(id) => {
                            service.registry.refresh();
                            service.refresh();
                            (service.on_registry)();
                            let _ = service.app.emit("app-changed", id);
                        }
                        Event::Disconnected { id, token } => {
                            let mut peers = service.peers.lock().unwrap_or_else(|e| e.into_inner());
                            if peers.get(&id).is_some_and(|peer| peer.token == token) {
                                peers.remove(&id);
                            }
                            drop(peers);
                            service.refresh();
                        }
                        Event::Click(action) => {
                            if let Err(error) = service.click(action) {
                                let _ = service.app.emit("manager-message", error.message);
                            }
                        }
                    }
                }
            })?;
        let _ = service.tx.send(Event::Refresh);
        Ok(service)
    }

    pub fn host(&self) -> TrayHostServer {
        self.host.clone()
    }
    pub fn preferences(&self) -> settings::Preferences {
        self.prefs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn request_index_refresh(&self) {
        (self.on_registry)();
    }
    pub fn request_shortcuts(&self) {
        let _ = self.tx.send(Event::Click(Action::Shortcuts));
    }

    pub fn apply_preferences(
        &self,
        mut prefs: settings::Preferences,
    ) -> Result<settings::Preferences> {
        let mut slot = self.prefs.lock().unwrap_or_else(|e| e.into_inner());
        prefs.normalize(&slot)?;
        if prefs.start_at_login != slot.start_at_login
            && settings::login_available(&self.manager.paths)
        {
            prefs.apply_login(
                &self.manager.paths,
                std::path::Path::new(&arcade_link::manifest::current_executable()),
            )?;
        }
        prefs.save(&self.manager.paths)?;
        *slot = prefs.clone();
        drop(slot);
        (self.on_settings)(prefs.clone());
        self.refresh_model();
        let _ = self.app.emit("tray-settings-changed", ());
        Ok(prefs)
    }

    /// Core update scheduling calls this only when its cached count changes.
    pub fn set_updates(&self, count: usize) {
        self.updates.store(count, Ordering::Relaxed);
        let _ = self.tx.send(Event::Refresh);
    }

    pub fn navigate(&self, page: &str) {
        *self.navigation.lock().unwrap_or_else(|e| e.into_inner()) = Some(page.into());
        let _ = self.app.emit("tray-navigation", page);
    }

    pub fn stop(&self, restarting: bool) {
        if !restarting {
            self.host.shutdown();
        }
        let _ = self.tx.send(Event::Stop);
        for peer in self
            .peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            peer.control.close();
        }
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.renderer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }

    fn refresh(&self) {
        let registry = self.registry.snapshot();
        let mut states = BTreeMap::new();
        for app in registry
            .apps()
            .iter()
            .filter(|a| ids::APPS.contains(&a.id.as_str()))
        {
            let mut state = PeerState::default();
            #[cfg(not(windows))]
            if platform::login_supported(&app.id, Os::current()) {
                state.login = Some(platform::autostart_enabled(&self.manager.paths, &app.id));
            }
            if app.settings.link_enabled {
                if let Ok(mut client) = Client::connect_with(
                    &self.manager.paths.link,
                    &app.id,
                    &me(),
                    Duration::from_millis(300),
                ) {
                    state.running = true;
                    if let Ok(status) = client.status() {
                        state.busy = status
                            .pointer("/status/busy")
                            .and_then(|b| b.as_bool())
                            .unwrap_or(false);
                    }
                    if let Ok(description) = client.describe_full() {
                        state.methods = description.methods;
                    }
                    self.watch_peer(app.id.clone(), client);
                } else if let Some(old) = self
                    .peers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&app.id)
                {
                    old.control.close();
                }
            }
            states.insert(app.id.clone(), state);
        }
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        peers.retain(|id, peer| {
            let keep = registry.get(id).is_some();
            if !keep {
                peer.control.close();
            }
            keep
        });
        drop(peers);
        *self.states.lock().unwrap_or_else(|e| e.into_inner()) = states;
        self.refresh_model();
    }

    fn watch_peer(&self, id: String, mut client: Client) {
        let token = arcade_link::endpoint::read(&self.manager.paths.link, &id)
            .ok()
            .map(|e| e.token)
            .unwrap_or_default();
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        if peers.get(&id).is_some_and(|p| p.token == token) {
            return;
        }
        if let Some(old) = peers.remove(&id) {
            old.control.close();
        }
        if client.subscribe(&["app.changed"]).is_err() {
            return;
        }
        peers.insert(
            id.clone(),
            PeerWatch {
                token: token.clone(),
                control: client.connection_control(),
            },
        );
        let changes = self.tx.clone();
        std::thread::spawn(move || {
            while let Ok(message) = client.next_notification(None) {
                if message.method.as_deref() == Some("app.changed") {
                    let _ = changes.send(Event::Changed(id.clone()));
                }
            }
            let _ = changes.send(Event::Disconnected { id, token });
        });
    }

    fn refresh_model(&self) {
        let registry = self.registry.snapshot();
        let catalog = self
            .manager
            .catalog()
            .unwrap_or_else(|_| arcade_tools_core::catalog::Catalog::snapshot());
        let documents: Vec<_> = catalog
            .apps
            .iter()
            .filter_map(|entry| registry.document(&entry.id))
            .collect();
        let model = TrayModel::build(
            &documents,
            &self.states.lock().unwrap_or_else(|e| e.into_inner()),
            &self.preferences(),
            self.updates.load(Ordering::Relaxed),
            crate::hotkey::overlay_path().is_some(),
        );
        if let Some(renderer) = self
            .renderer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            renderer.update(model);
        }
    }

    fn click(&self, action: Action) -> Result<()> {
        match action {
            Action::Peer { app, command } => {
                let registry = self.registry.snapshot();
                let manifest = registry
                    .get(&app)
                    .ok_or_else(|| Error::new("not_installed", "This app was removed."))?;
                transport::call(&self.manager.paths.link, manifest, &command)
                    .map_err(|e| Error::new("link", e.user_message(&manifest.name)))?;
            }
            Action::Login { app, enabled } => self.manager.start_at_login(&app, enabled)?,
            Action::StartAll => {
                let registry = self.registry.snapshot();
                for app in registry
                    .apps()
                    .iter()
                    .filter(|a| ids::APPS.contains(&a.id.as_str()))
                {
                    // Start all keeps already-running apps in their current mode.
                    if Client::connect_with(
                        &self.manager.paths.link,
                        &app.id,
                        &me(),
                        Duration::from_millis(300),
                    )
                    .is_err()
                    {
                        arcade_link::client::spawn_detached(
                            &app.executable,
                            &app.launch.background,
                        )?;
                    }
                }
            }
            Action::QuitAll => self.quit_all()?,
            Action::Shortcuts => {
                crate::hotkey::launch_overlay()?;
            }
            action => (self.on_tools)(action),
        }
        self.refresh();
        Ok(())
    }

    fn quit_all(&self) -> Result<()> {
        self.refresh();
        let registry = self.registry.snapshot();
        let states = self
            .states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        transport::quit_all(&self.manager.paths.link, registry.apps(), &states, |busy| {
            self.app.dialog().message(format!("{} are busy. Ask all running Arcade apps to quit? Apps that cannot quit safely will keep running.", busy.join(", "))).title("Quit Arcade apps").buttons(MessageDialogButtons::OkCancelCustom("Quit apps".into(), "Cancel".into())).blocking_show()
        }).map_err(|error| Error::new("quit_apps", error))
    }
}

pub fn service(app: &AppHandle) -> Result<Arc<Service>> {
    app.try_state::<Arc<Service>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| Error::new("initializing", "Arcade Tools is starting."))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub preferences: settings::Preferences,
    pub apps: Vec<AppPreference>,
    pub login_available: bool,
    pub shortcut_status: crate::hotkey::Status,
    /// This OS's default overlay shortcut, for "Reset to default".
    pub default_shortcut: &'static str,
}
#[derive(Serialize)]
pub struct AppPreference {
    id: String,
    name: String,
    excluded: bool,
}

#[tauri::command]
pub async fn tray_get_settings(app: AppHandle) -> Result<View> {
    let service = service(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let preferences = service.preferences();
        let registry = service.registry.snapshot();
        let apps = ids::APPS
            .iter()
            .filter(|id| registry.get(id).is_some())
            .map(|id| AppPreference {
                id: (*id).into(),
                name: arcade_link::manifest::app_name(id).into(),
                excluded: preferences.excluded.iter().any(|e| e == id),
            })
            .collect();
        Ok(View {
            preferences,
            apps,
            login_available: settings::login_available(&service.manager.paths),
            shortcut_status: crate::hotkey::status(&service.app),
            default_shortcut: arcade_tools_overlay::registration::default_accelerator(),
        })
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}

#[tauri::command]
pub async fn tray_set_settings(
    app: AppHandle,
    preferences: settings::Preferences,
) -> Result<settings::Preferences> {
    let service = service(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let previous = service.preferences();
        if preferences.shortcuts_enabled
            && (preferences.shortcut != previous.shortcut || !previous.shortcuts_enabled)
        {
            if let Some(index) = crate::shortcuts::read_index()? {
                arcade_tools_core::shortcut_control::validate(
                    &index,
                    ids::TOOLS,
                    "overlay",
                    &preferences.shortcut,
                )?;
            }
        }
        service.apply_preferences(preferences)
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}

#[tauri::command]
pub fn tray_ui_intent(app: AppHandle) -> Result<Option<String>> {
    Ok(service(&app)?
        .navigation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take())
}

#[tauri::command]
pub async fn rebuild_shortcut_index(app: AppHandle) -> Result<crate::index_service::Status> {
    let service = app
        .try_state::<Arc<crate::index_service::IndexService>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| Error::new("initializing", "The shortcut worker is starting."))?;
    tauri::async_runtime::spawn_blocking(move || {
        service
            .rebuild_index()
            .map_err(|e| Error::new("shortcut_index", e))
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}

#[tauri::command]
pub fn shortcut_index_status(app: AppHandle) -> Result<crate::index_service::Status> {
    let service = app
        .try_state::<Arc<crate::index_service::IndexService>>()
        .ok_or_else(|| Error::new("initializing", "The shortcut worker is starting."))?;
    Ok(service.status())
}

#[tauri::command]
pub fn configure_shortcut_sources(app: AppHandle, catalog: PathBuf, docs: PathBuf) -> Result<()> {
    let service = app
        .try_state::<Arc<crate::index_service::IndexService>>()
        .ok_or_else(|| Error::new("initializing", "The shortcut worker is starting."))?;
    service.set_sources(catalog, docs);
    Ok(())
}
