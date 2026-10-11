use crate::catalog::{Catalog, CatalogClient};
use crate::docs::DocsResolver;
use crate::engines::EngineCatalog;
use crate::paths::{app_id, Paths};
use crate::platform;
use crate::release::{checksum, Arch, Channel, Kind, Os};
use crate::source::{CheckedRelease, Source};
use crate::updates::{
    self, CheckContext, Clock, HistoryEntry, HistoryResult, InstalledApp, PeerProtocols,
    SystemClock, Trigger, UpdateSettings, UpdateState,
};
use crate::{Error, Result};
use arcade_link::client::Client;
use arcade_link::manifest::{self, ids, Manifest};
use arcade_link::registry::Registry;
use arcade_link::wire::PeerInfo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchMode {
    Foreground,
    Background,
}
impl LaunchMode {
    pub fn args(self) -> Vec<String> {
        if self == Self::Background {
            vec!["--background".into()]
        } else {
            vec![]
        }
    }
}
pub fn status_mode(status: &Value) -> Option<LaunchMode> {
    let status = status.get("status").unwrap_or(status);
    if let Some(background) = status.get("background").and_then(Value::as_bool) {
        Some(if background {
            LaunchMode::Background
        } else {
            LaunchMode::Foreground
        })
    } else {
        match status.get("mode").and_then(Value::as_str) {
            Some("background") => Some(LaunchMode::Background),
            Some("foreground") => Some(LaunchMode::Foreground),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub version: String,
    pub channel: Channel,
    pub kind: Kind,
    pub sha256: String,
    pub executable_sha256: String,
    pub last_mode: Option<LaunchMode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default = "yes")]
    pub link_enabled: bool,
    #[serde(default)]
    pub channels: BTreeMap<String, Channel>,
    #[serde(default)]
    pub updates: UpdateSettings,
}
fn yes() -> bool {
    true
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            link_enabled: true,
            channels: BTreeMap::new(),
            updates: UpdateSettings::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub id: String,
    pub name: String,
    pub pitch: String,
    pub releases_url: String,
    pub version: Option<String>,
    pub managed: bool,
    pub installed: bool,
    pub running: bool,
    pub channel: Channel,
    pub healthy: bool,
    pub start_at_login: bool,
    pub login_supported: bool,
    pub executable: Option<String>,
    pub data_folders: Vec<String>,
    pub reason: Option<String>,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub platforms: BTreeMap<String, String>,
    pub min_tools: String,
    pub install_reason: Option<String>,
    pub install_supported: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    pub id: String,
    pub version: String,
    pub channel: Channel,
    pub file: String,
    pub notes: String,
    pub update_available: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub phase: String,
    pub fraction: Option<f64>,
    pub cancellable: bool,
}
impl Progress {
    fn new(id: &str, phase: &str, fraction: Option<f64>, cancellable: bool) -> Self {
        Self {
            id: id.into(),
            phase: phase.into(),
            fraction,
            cancellable,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Install,
    Update,
    Repair,
    Uninstall,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    pub operation: Operation,
    #[serde(default)]
    pub mode: Option<LaunchMode>,
    #[serde(default)]
    pub remove_data: bool,
    #[serde(default)]
    pub app_closed: bool,
}
pub struct Manager {
    pub paths: Paths,
    source: Source,
    mutation: Mutex<()>,
}
impl Manager {
    /// Construct without I/O; safe on any thread. All manager I/O methods below
    /// are synchronous and must be invoked on worker threads.
    pub fn new(paths: Paths, source: Source) -> Self {
        Self {
            paths,
            source,
            mutation: Mutex::new(()),
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.mutation
            .try_lock()
            .map_err(|_| Error::new("busy", "Another manager operation is running."))
    }
    /// Read persisted preferences on a worker thread; no network work.
    pub fn preferences(&self) -> Result<Preferences> {
        let path = self.paths.state().join("preferences.json");
        match fs::read(&path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(e) => Err(e.into()),
        }
    }
    fn save_preferences(&self, prefs: &Preferences) -> Result<()> {
        self.write(&self.paths.state().join("preferences.json"), prefs)
    }
    /// Persist a channel selection on a worker thread; no network work.
    pub fn set_channel(&self, id: &str, channel: Channel) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.channels.insert(id.into(), channel);
        self.save_preferences(&prefs)
    }
    /// Persist Link enablement on a worker thread; no IPC/network work.
    pub fn set_link(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.link_enabled = enabled;
        self.save_preferences(&prefs)
    }
    /// Persist scheduler opt-in on a worker thread. No network work is started.
    pub fn set_updates_enabled(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.updates.enabled = enabled;
        self.save_preferences(&prefs)
    }
    /// Persist per-app auto-update opt-in on a worker thread. No installation.
    pub fn set_auto_update(&self, id: &str, enabled: bool) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.updates.auto_update.insert(id.into(), enabled);
        self.save_preferences(&prefs)
    }
    /// Read managed records and installed peers' registry manifests on a worker
    /// thread. Does not probe IPC or connect to the network.
    pub fn update_inputs(&self) -> Result<(Vec<InstalledApp>, Vec<PeerProtocols>)> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let peers = registry
            .apps()
            .iter()
            .filter(|m| m.id != ids::TOOLS)
            .map(|m| PeerProtocols {
                id: m.id.clone(),
                protocols: m.link.protocol.clone(),
            })
            .collect();
        let mut installed = Vec::new();
        for app in self.catalog()?.apps {
            let record = if app.supported_install() {
                self.record(&app.id)?
            } else {
                None
            };
            let peer = registry.get(&app.id);
            if let Some(version) = record
                .as_ref()
                .map(|r| r.version.clone())
                .or_else(|| peer.map(|p| p.version.clone()))
            {
                installed.push(InstalledApp {
                    id: app.id.clone(),
                    version,
                    channel: record.as_ref().map(|r| r.channel).unwrap_or_default(),
                    selected_channel: prefs
                        .channels
                        .get(&app.id)
                        .copied()
                        .or(record.as_ref().map(|r| r.channel))
                        .unwrap_or_default(),
                    sha256: record.map(|r| r.sha256),
                });
            }
        }
        Ok((installed, peers))
    }
    fn write(&self, path: &std::path::Path, value: &impl Serialize) -> Result<()> {
        self.paths.guard(path)?;
        arcade_link::paths::write_atomic(path, &serde_json::to_vec_pretty(value)?, true)?;
        Ok(())
    }
    /// Read an installation record on a worker thread; no network work.
    pub fn record(&self, id: &str) -> Result<Option<Record>> {
        app_id(id)?;
        match fs::read(self.paths.state().join(format!("{id}.json"))) {
            Ok(bytes) => {
                let record: Record = serde_json::from_slice(&bytes)?;
                if record.id != id {
                    return Err(Error::new(
                        "state",
                        "Installed record has a mismatched app ID.",
                    ));
                }
                Ok(Some(record))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn executable(&self, record: &Record) -> PathBuf {
        self.paths
            .executable(&record.id, Os::current(), record.kind == Kind::Tarball)
    }
    /// Read the cached catalog or built-in snapshot on a worker thread.
    /// No network I/O is performed.
    pub fn catalog(&self) -> Result<Catalog> {
        Ok(CatalogClient::new(self.paths.clone(), self.source.clone())?.cached_catalog())
    }
    /// Explicit catalog network refresh/cache write on a worker thread.
    pub fn refresh_catalog(&self) -> Result<Catalog> {
        Ok(CatalogClient::new(self.paths.clone(), self.source.clone())?.refresh_catalog())
    }
    /// Read persisted update results and reconcile them with the current
    /// registry/catalog snapshot. This does not make network requests.
    pub fn update_state(&self) -> Result<UpdateState> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let mut scheduler =
            updates::Scheduler::new(self.paths.clone(), SystemClock, self.source.clone())?;
        scheduler.reconcile(&catalog, &installed, &peers);
        Ok(scheduler.state().clone())
    }
    /// Run a user-requested update check through the core scheduler and persist
    /// its state. The caller must keep this synchronous work off the UI thread.
    pub fn check_updates(&self) -> Result<UpdateState> {
        self.check_updates_for(Trigger::Manual)
    }
    /// Run a scheduled or window-open update check through the core scheduler.
    /// The caller must keep this synchronous work off the UI thread.
    pub fn check_updates_for(&self, trigger: Trigger) -> Result<UpdateState> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let settings = self.preferences()?.updates;
        let mut scheduler =
            updates::Scheduler::new(self.paths.clone(), SystemClock, self.source.clone())?;
        scheduler.run(
            trigger,
            CheckContext {
                settings: &settings,
                catalog: &catalog,
                installed: &installed,
                peers: &peers,
                // A manual request treats connectivity as unknown/online; the
                // first failed transport is recorded by the scheduler.
                online: true,
                os: Os::current(),
                arch: Arch::current()?,
            },
        )?;
        Ok(scheduler.state().clone())
    }
    /// Read the cached or bundled engine catalog; refresh is always explicit.
    pub fn engine_catalog(&self, refresh: bool) -> Result<EngineCatalog> {
        CatalogClient::new(self.paths.clone(), self.source.clone())?.engines(refresh)
    }
    /// Fetch and cache one app's docs for the selected installed version. This
    /// method never runs unless Learn explicitly requests the download.
    pub fn fetch_docs(&self, id: &str, version: &str) -> Result<()> {
        let catalog = self.catalog()?;
        let app = catalog
            .app(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resolver = DocsResolver::new(self.paths.clone(), PathBuf::new());
        resolver.fetch_version(app, version, &self.source)?;
        Ok(())
    }
    /// Disk/optional IPC work on a worker thread; no network refresh.
    pub fn list(&self, probe: bool) -> Result<Vec<AppView>> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let catalog = self.catalog()?;
        catalog.apps.iter().map(|app| {
            let id = app.id.as_str();
            let record = if app.supported_install() { self.record(id)? } else { None };
            let peer = registry.get(id);
            let executable = record.as_ref().map(|r| self.executable(r)).or_else(|| peer.map(|m| PathBuf::from(&m.executable)));
            let running = probe && Client::connect(&self.paths.link, id, &me()).is_ok();
            let healthy = executable.as_ref().is_some_and(|p| p.is_file());
            let reason = if record.is_none() && peer.is_some() { Some("Installed outside Arcade Tools. Launch is available; updates and removal need a managed installation.".into()) }
                else if peer.is_some_and(|m| !m.settings.link_enabled) { Some("Arcade Link is disabled in this app. Close it before changing its installation.".into()) }
                else if record.is_some() && !healthy { Some("Installed executable is missing. Choose Repair.".into()) } else { None };
            Ok(AppView { id: id.to_string(), name: app.name.clone(), pitch: app.pitch.clone(), releases_url: app.releases_url(),
                version: record.as_ref().map(|r| r.version.clone()).or_else(|| peer.map(|m| m.version.clone())), managed: record.is_some(), installed: healthy,
                running, channel: prefs.channels.get(id).copied().or(record.as_ref().map(|r| r.channel)).unwrap_or_default(), healthy,
                start_at_login: platform::autostart_enabled(&self.paths, id), login_supported: app.supported_install() && platform::login_supported(id, Os::current()), executable: executable.map(|p| p.display().to_string()),
                data_folders: self.paths.data_folders(id, Os::current()).iter().map(|p| p.display().to_string()).collect(), reason,
                description: app.description.clone(), category: app.category.clone(), icon: app.icon.clone(),
                platforms: app.platforms.clone(), min_tools: app.min_tools.clone(), install_reason: app.install_reason.clone(),
                install_supported: app.supported_install() })
        }).collect()
    }
    fn checked(&self, id: &str) -> Result<CheckedRelease> {
        let catalog = self.catalog()?;
        let app = catalog
            .app(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        if let Some(reason) = &app.install_reason {
            return Err(Error::new("version_mismatch", reason.clone()));
        }
        let prefs = self.preferences()?;
        let channel = prefs
            .channels
            .get(id)
            .copied()
            .or(self.record(id)?.map(|r| r.channel))
            .unwrap_or_default();
        let path = self
            .paths
            .cache
            .join("arcade/tools/releases")
            .join(id)
            .join(format!("{}.json", channel.as_str()));
        let cache: Option<crate::release::ReleaseCache> =
            crate::catalog::read_bounded(&path, 6 * 1024 * 1024)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        let check = self.source.check_repo_conditional(
            id,
            &app.repo,
            channel,
            Os::current(),
            Arch::current()?,
            cache.as_ref(),
        )?;
        self.write(&path, &check.cache)?;
        check.checked.release.compatible(id, channel)?;
        let peers = Registry::load(&self.paths.link)
            .apps()
            .iter()
            .filter(|m| m.id != ids::TOOLS)
            .map(|m| PeerProtocols {
                id: m.id.clone(),
                protocols: m.link.protocol.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(reason) = updates::held_back(app, &check.checked.release, &peers) {
            return Err(Error::new("version_mismatch", reason));
        }
        Ok(check.checked)
    }
    /// Explicit conditional release request and disk/registry reads, on a worker.
    pub fn check(&self, id: &str) -> Result<CheckView> {
        let checked = self.checked(id)?;
        let record = self.record(id)?;
        Ok(CheckView {
            id: id.into(),
            version: checked.release.version.clone(),
            channel: checked.release.channel,
            file: checked.asset.file.clone(),
            notes: checked.release.notes.clone(),
            update_available: record.is_none_or(|r| {
                r.sha256 != checked.asset.sha256
                    || r.version != checked.release.version
                    || r.channel != checked.release.channel
            }),
        })
    }
    /// IPC/process/disk work on a worker thread. Starts no update timer.
    pub fn launch(&self, id: &str, mode: LaunchMode) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        self.launch_inner(id, mode)
    }
    fn launch_inner(&self, id: &str, mode: LaunchMode) -> Result<()> {
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            if mode == LaunchMode::Foreground {
                client
                    .call("app.activate", json!({}))
                    .map_err(|e| link_error(id, e))?;
            }
            return Ok(());
        }
        let mut record = self.record(id)?;
        let manifest = Registry::load(&self.paths.link).get(id).cloned();
        let exe = record
            .as_ref()
            .map(|r| self.executable(r))
            .or_else(|| manifest.map(|m| PathBuf::from(m.executable)))
            .ok_or_else(|| Error::new("not_installed", "Install this app first."))?;
        if !exe.is_file() {
            return Err(Error::new(
                "not_installed",
                "The installed executable is missing. Choose Repair.",
            ));
        }
        arcade_link::client::spawn_detached(&exe.to_string_lossy(), &mode.args())?;
        if let Some(r) = record.as_mut() {
            r.last_mode = Some(mode);
            self.write(&self.paths.state().join(format!("{id}.json")), r)?;
        }
        Ok(())
    }
    /// Read, back up, write and validate a generated login entry on a worker.
    pub fn start_at_login(&self, id: &str, enabled: bool) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        let exe = self
            .record(id)?
            .as_ref()
            .map(|r| self.executable(r))
            .or_else(|| {
                Registry::load(&self.paths.link)
                    .get(id)
                    .map(|m| PathBuf::from(&m.executable))
            })
            .ok_or_else(|| Error::new("not_installed", "Install this app first."))?;
        platform::set_autostart(&self.paths, id, &exe, enabled)
    }
    /// User-confirmed (or per-app opted-in) installer/network/IPC/disk work,
    /// on a worker thread. Updates/repairs append their verified history outcome.
    pub fn operate(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        app_id(&request.id)?;
        let _lock = self.lock()?;
        let old = self.record(&request.id)?;
        let mut history = HistoryEntry {
            time: SystemClock.now(),
            app: request.id.clone(),
            from: old.as_ref().map(|r| r.version.clone()),
            to: None,
            result: HistoryResult::Success,
            reason: None,
        };
        let mut replaced = false;
        let result = self.operate_inner(request, cancel, progress, &mut history, &mut replaced);
        if matches!(request.operation, Operation::Update | Operation::Repair) {
            if let Err(error) = &result {
                history.result = if replaced {
                    if old.as_ref().is_some_and(|r| {
                        checksum(&self.executable(r)).is_ok_and(|hash| hash == r.executable_sha256)
                    }) && self
                        .record(&request.id)
                        .ok()
                        .flatten()
                        .is_some_and(|r| Some(r.version) == history.from)
                    {
                        HistoryResult::RolledBack
                    } else {
                        HistoryResult::RollbackFailed
                    }
                } else {
                    HistoryResult::Failed
                };
                history.reason = Some(error.to_string());
            }
            // Don't replace an installation error with a log I/O error. Return
            // log failures after success so callers know history was not saved.
            if let Err(error) = updates::append_history(&self.paths, &history) {
                if result.is_ok() {
                    return Err(error);
                }
                eprintln!("Update history: {error}");
            }
        }
        result
    }
    fn operate_inner(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        history: &mut HistoryEntry,
        replaced: &mut bool,
    ) -> Result<String> {
        let id = &request.id;
        let old = self.record(id)?;
        let peer = Registry::load(&self.paths.link).get(id).cloned();
        if request.operation != Operation::Install && old.is_none() {
            return Err(Error::new(
                "unmanaged",
                "Updates, repair and removal require an installation managed by Arcade Tools.",
            ));
        }
        if request.operation == Operation::Install && (old.is_some() || peer.is_some()) {
            return Err(Error::new(
                "already_installed",
                "This app is already installed.",
            ));
        }
        if request.operation == Operation::Uninstall {
            check_cancel(cancel)?;
            progress(Progress::new(id, "Closing app", None, false));
            self.stop_peer(id, peer.as_ref(), request, false)?;
            progress(Progress::new(id, "Removing app", None, false));
            return self.uninstall(old.as_ref().unwrap(), request.remove_data);
        }
        progress(Progress::new(id, "Checking release", None, true));
        check_cancel(cancel)?;
        let checked = self.checked(id)?;
        history.to = Some(checked.release.version.clone());
        if old.as_ref().is_some_and(|r| r.kind != checked.asset.kind) {
            return Err(Error::new("installer_changed", "This channel uses a different installer format. Uninstall while keeping data, then install the new channel."));
        }
        if request.operation == Operation::Update
            && old.as_ref().is_some_and(|r| {
                r.sha256 == checked.asset.sha256
                    && r.version == checked.release.version
                    && r.channel == checked.release.channel
            })
        {
            return Ok("Already up to date.".into());
        }
        let destination =
            self.paths
                .executable(id, Os::current(), checked.asset.kind == Kind::Tarball);
        if old.is_none() && destination.exists() {
            return Err(Error::new(
                "unmanaged",
                "The managed install location already contains a file. Move it before installing.",
            ));
        }
        progress(Progress::new(id, "Downloading", Some(0.0), true));
        self.paths.guard(&self.paths.downloads())?;
        let downloaded = self.source.download(
            &checked,
            &self.paths.downloads(),
            cancel,
            &mut |count, total| {
                progress(Progress::new(
                    id,
                    "Downloading",
                    total.filter(|n| *n > 0).map(|n| count as f64 / n as f64),
                    true,
                ))
            },
        )?;
        progress(Progress::new(id, "Verified SHA-256", Some(1.0), true));
        check_cancel(cancel)?;
        // Verification always precedes app.quit. Never force quit a busy app.
        let resume = self.stop_peer(id, peer.as_ref(), request, true)?;
        if let Err(error) = check_cancel(cancel) {
            if let Some(mode) = resume {
                let _ = self.launch_inner(id, mode);
            }
            return Err(error);
        }
        let login = platform::autostart_enabled(&self.paths, id);
        progress(Progress::new(id, "Installing", None, false));
        let result = (|| {
            let (exe, replacement) = platform::install_observed(
                &self.paths,
                id,
                &checked.asset,
                downloaded.path(),
                &mut || *replaced = true,
            )?;
            let record = Record {
                id: id.into(),
                version: checked.release.version.clone(),
                channel: checked.release.channel,
                kind: checked.asset.kind,
                sha256: checked.asset.sha256.clone(),
                executable_sha256: checksum(&exe)?,
                last_mode: resume
                    .or(request.mode)
                    .or(old.as_ref().and_then(|r| r.last_mode)),
            };
            if login {
                platform::set_autostart(&self.paths, id, &exe, true)?;
            }
            self.write(&self.paths.state().join(format!("{id}.json")), &record)?;
            replacement.commit();
            *replaced = false;
            Ok(())
        })();
        if let Err(error) = result {
            if let Some(mode) = resume {
                let _ = self.launch_inner(id, mode);
            }
            return Err(error);
        }
        let mode = resume.or(if request.operation == Operation::Install {
            Some(request.mode.unwrap_or(LaunchMode::Foreground))
        } else {
            None
        });
        if let Some(mode) = mode {
            progress(Progress::new(id, "Launching", None, false));
            self.launch_inner(id, mode)?; // App owns its manifest and first-run integration.
                                          // First-run app integration can enable autostart. No preference is silently changed here.
        }
        progress(Progress::new(id, "Done", Some(1.0), false));
        Ok(format!(
            "{} {} installed.",
            crate::apps::app_name(id),
            checked.release.version
        ))
    }
    fn stop_peer(
        &self,
        id: &str,
        peer: Option<&Manifest>,
        request: &Request,
        restore_mode: bool,
    ) -> Result<Option<LaunchMode>> {
        if peer.is_some_and(|m| !m.settings.link_enabled) && !request.app_closed {
            return Err(Error::new("disconnected", "Arcade Link is disabled in this app. Close the app and confirm it is closed, or enable Arcade Link and retry."));
        }
        let mut client = match Client::connect(&self.paths.link, id, &me()) {
            Ok(c) => c,
            Err(e) if e.code == arcade_link::ErrorCode::NotRunning => {
                #[cfg(target_os = "linux")]
                if arcade_link::endpoint::read(&self.paths.link, id)
                    .is_ok_and(|ep| std::path::Path::new(&format!("/proc/{}", ep.pid)).exists())
                    && !request.app_closed
                {
                    return Err(Error::new("unavailable", "The app has a live process but its Link endpoint is unavailable. Close it before retrying."));
                }
                return Ok(None);
            }
            Err(e) => return Err(link_error(id, e)),
        };
        let status = client.status().map_err(|e| link_error(id, e))?;
        if status.get("busy").and_then(Value::as_bool) == Some(true)
            || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true)
        {
            return Err(Error::new(
                "busy",
                arcade_link::LinkError::busy().user_message(crate::apps::app_name(id)),
            ));
        }
        let mode = status_mode(&status).or(request.mode);
        if restore_mode && mode.is_none() {
            return Err(Error::new("mode_required", "This app does not report its window mode. Choose how to reopen it before updating."));
        }
        client
            .call("app.quit", json!({}))
            .map_err(|e| link_error(id, e))?;
        drop(client);
        let pid = status
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok());
        // Bounded checks only during a user-requested operation, never while idle.
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if Client::connect(&self.paths.link, id, &me()).is_err()
                && !pid.is_some_and(|pid| pid != std::process::id() && process_alive(pid))
            {
                return Ok(mode);
            }
            if Instant::now() > deadline {
                return Err(Error::new(
                    "timeout",
                    "The app did not close. Its installation was left unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn uninstall(&self, record: &Record, remove_data: bool) -> Result<String> {
        let id = &record.id;
        let exe = self.executable(record);
        let root = self
            .paths
            .install_root(id, Os::current(), record.kind == Kind::Tarball);
        let target = if record.kind == Kind::Appimage {
            &exe
        } else {
            &root
        };
        self.paths.guard(target)?;
        if remove_data {
            for folder in self.paths.data_folders(id, Os::current()) {
                self.paths.guard(&folder)?;
            }
        }
        if platform::login_supported(id, Os::current()) {
            platform::set_autostart(&self.paths, id, &exe, false)?;
        }
        #[cfg(windows)]
        {
            let uninstaller = platform::windows_uninstall(record.kind, &root)?;
            self.paths.guard(&uninstaller.program)?;
            if uninstaller.program.is_file() {
                uninstaller.run(&self.paths)?;
            } else {
                return Err(Error::new(
                    "installer",
                    "The platform uninstaller is missing. Repair the app first.",
                ));
            }
        }
        platform::remove(target)?;
        // Remove launch entries only when their Exec references the managed installation.
        if Os::current() == Os::Linux {
            let applications = self.paths.data.join("applications");
            if let Ok(entries) = fs::read_dir(&applications) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "desktop")
                        && fs::read_to_string(&p).is_ok_and(|s| {
                            s.lines().any(|l| {
                                l.starts_with("Exec=")
                                    && (l.contains(&exe.to_string_lossy().to_string())
                                        || record.kind == Kind::Tarball
                                            && l.contains(&root.to_string_lossy().to_string()))
                            })
                        })
                    {
                        self.paths.guard(&p)?;
                        platform::remove(&p)?;
                    }
                }
            }
        }
        if remove_data {
            for folder in self.paths.data_folders(id, Os::current()) {
                platform::remove(&folder)?;
            }
        }
        manifest::remove_manifest(&self.paths.link, id)?;
        platform::remove(&self.paths.state().join(format!("{id}.json")))?;
        Ok(format!(
            "{} removed. {}",
            crate::apps::app_name(id),
            if remove_data {
                "Settings and data removed."
            } else {
                "Settings and data kept."
            }
        ))
    }
}
pub fn me() -> PeerInfo {
    PeerInfo {
        id: ids::TOOLS.into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(Error::new("cancelled", "Cancelled."))
    } else {
        Ok(())
    }
}
fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit_once(')')
                .is_none_or(|(_, fields)| fields.split_whitespace().next() != Some("Z"))
        })
    }
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // Same-user peer, signal zero only checks existence.
        unsafe { kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut exit_code = 0;
            let running = GetExitCodeProcess(handle, &mut exit_code) == 0 || exit_code == 259;
            CloseHandle(handle);
            running
        }
    }
}
fn link_error(id: &str, error: arcade_link::LinkError) -> Error {
    Error::new(
        error.code.as_str(),
        error.user_message(crate::apps::app_name(id)),
    )
}
