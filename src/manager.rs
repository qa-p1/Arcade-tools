use crate::catalog::{Catalog, CatalogClient};
use crate::installation::{self, Package, PreviousInstallation, Transaction};
use crate::paths::{app_id, Paths};
use crate::platform;
use crate::release::{checksum, Arch, Channel, Kind, Os};
use crate::source::{CheckedRelease, Source};
use crate::updates::{
    self, Clock, HistoryEntry, HistoryResult, InstalledApp, PeerProtocols, SystemClock,
    UpdateSettings,
};
use crate::{Error, Result};
use arcade_link::client::Client;
use arcade_link::manifest::{self, ids, Manifest};
use arcade_link::receipt::{InstallMethod, ManagedBy, Receipt, Store};
use arcade_link::registry::Registry;
use arcade_link::wire::PeerInfo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
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
    /// Supplemental verification data; receipt.path is authoritative.
    #[serde(default)]
    pub executable: Option<PathBuf>,
    #[serde(default)]
    pub package: Option<crate::installation::Package>,
    #[serde(default)]
    pub installed_by_app: bool,
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
    pub installation_class: InstallationClass,
    pub rollback_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstallationClass {
    Managed,
    LegacyManaged,
    SelfInstalled,
    Manual,
    Dev,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Adoption {
    pub id: String,
    pub class: InstallationClass,
    pub executable: PathBuf,
    pub version: String,
    pub can_adopt: bool,
    pub reason: Option<String>,
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
    Adopt,
    Rollback,
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
#[derive(Clone, Copy)]
struct Prior<'a> {
    record: Option<&'a Record>,
    receipt: Option<&'a Receipt>,
    peer: Option<&'a Manifest>,
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
        manager_id(id)?;
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
        let catalog = self.catalog()?;
        let apps = catalog
            .install_entry(ids::TOOLS)
            .into_iter()
            .chain(catalog.apps);
        for app in apps {
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
                    sha256: record.map(|r| r.sha256).filter(|hash| !hash.is_empty()),
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
        manager_id(id)?;
        let supplemental = match fs::read(self.paths.state().join(format!("{id}.json"))) {
            Ok(bytes) => {
                let record: Record = serde_json::from_slice(&bytes)?;
                if record.id != id {
                    return Err(Error::new(
                        "state",
                        "Installed record has a mismatched app ID.",
                    ));
                }
                Some(record)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(receipt) = self.receipt(id)? {
            if receipt.method == InstallMethod::Dev {
                return Ok(None);
            }
            let executable = installation::executable(&self.paths, &receipt);
            if let Some(mut record) = supplemental {
                if record.version != receipt.version
                    || record.channel.as_str() != receipt.channel
                    || record
                        .executable
                        .as_ref()
                        .is_some_and(|path| path != &executable)
                {
                    record.version = receipt.version.clone();
                    record.channel = receipt_channel(&receipt)?;
                    record.package = None;
                    record.sha256 = String::new();
                    record.executable_sha256 = checksum(&executable).unwrap_or_default();
                }
                record.executable = Some(executable);
                return Ok(Some(record));
            }
            return Ok(Some(self.record_from_receipt(&receipt)?));
        }
        Ok(supplemental)
    }
    /// Read a validated Link receipt on a worker; no migration or writes.
    pub fn receipt(&self, id: &str) -> Result<Option<Receipt>> {
        manager_id(id)?;
        self.paths.guard_receipt(id)?;
        Ok(Store::new(&self.paths.link).read(id)?)
    }
    fn record_from_receipt(&self, receipt: &Receipt) -> Result<Record> {
        let executable = installation::executable(&self.paths, receipt);
        let kind = match receipt.method {
            InstallMethod::Appimage => Kind::Appimage,
            InstallMethod::MacosBundle => Kind::Dmg,
            InstallMethod::Tarball => Kind::Tarball,
            InstallMethod::WindowsInstaller => {
                windows_kind(receipt.integration.uninstaller.as_deref())
            }
            _ => match Os::current() {
                Os::Linux => Kind::Appimage,
                Os::Windows => windows_kind(receipt.integration.uninstaller.as_deref()),
                Os::Macos => Kind::Dmg,
            },
        };
        Ok(Record {
            id: receipt.id.clone(),
            version: receipt.version.clone(),
            channel: receipt_channel(receipt)?,
            kind,
            sha256: String::new(),
            executable_sha256: checksum(&executable).unwrap_or_default(),
            last_mode: None,
            executable: Some(executable),
            package: None,
            installed_by_app: true,
        })
    }
    fn executable(&self, record: &Record) -> PathBuf {
        record.executable.clone().unwrap_or_else(|| {
            self.paths
                .executable(&record.id, Os::current(), record.kind == Kind::Tarball)
        })
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
    fn class(
        &self,
        id: &str,
        registry: &Registry,
        receipt: Option<&Receipt>,
    ) -> Result<InstallationClass> {
        let dev = registry.additions(id).and_then(|extra| extra.install)
            == Some(InstallMethod::Dev)
            || registry.get(id).is_some_and(|peer| {
                arcade_link::install::is_dev_build(Path::new(&peer.executable))
            })
            || receipt.is_some_and(|receipt| {
                receipt.method == InstallMethod::Dev
                    || arcade_link::install::is_dev_build(&receipt.path)
            });
        if dev {
            return Ok(InstallationClass::Dev);
        }
        Ok(match receipt.map(|r| r.managed_by) {
            Some(ManagedBy::Tools) => InstallationClass::Managed,
            Some(ManagedBy::SelfManaged) => InstallationClass::SelfInstalled,
            None if self.paths.state().join(format!("{id}.json")).is_file() => {
                InstallationClass::LegacyManaged
            }
            None => InstallationClass::Manual,
        })
    }
    /// Classify registry installs for the adoption dialog on a worker. Dev builds
    /// are launchable but never eligible for any installation mutation.
    pub fn adoptions(&self) -> Result<Vec<Adoption>> {
        let registry = Registry::load(&self.paths.link);
        let mut result = Vec::new();
        for peer in registry
            .apps()
            .iter()
            .filter(|peer| crate::apps::APPS.contains(&peer.id.as_str()))
        {
            let receipt = self.receipt(&peer.id)?;
            let class = self.class(&peer.id, &registry, receipt.as_ref())?;
            let can_adopt = matches!(
                class,
                InstallationClass::Manual | InstallationClass::SelfInstalled
            );
            result.push(Adoption {
                id: peer.id.clone(),
                class,
                executable: PathBuf::from(&peer.executable),
                version: peer.version.clone(),
                can_adopt,
                reason: if class == InstallationClass::Dev {
                    Some("Development build; Arcade Tools will never modify it.".into())
                } else {
                    None
                },
            });
        }
        Ok(result)
    }
    /// Read one-step rollback metadata on a worker, without network or mutation.
    pub fn rollback_version(&self, id: &str) -> Result<Option<String>> {
        manager_id(id)?;
        Ok(PreviousInstallation::read(&self.paths, id)?
            .filter(|previous| previous.pointer(&self.paths).is_some())
            .map(|previous| previous.record.version))
    }
    /// Disk/optional IPC work on a worker thread; no network refresh.
    pub fn list(&self, probe: bool) -> Result<Vec<AppView>> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let catalog = self.catalog()?;
        catalog.apps.iter().map(|app| {
            let id = app.id.as_str();
            let record = if app.supported_install() { self.record(id)? } else { None };
            let receipt = if app.supported_install() { self.receipt(id)? } else { None };
            let class = self.class(id, &registry, receipt.as_ref())?;
            let peer = registry.get(id);
            let executable = record.as_ref().map(|r| self.executable(r)).or_else(|| peer.map(|m| PathBuf::from(&m.executable)));
            let running = probe && Client::connect(&self.paths.link, id, &me()).is_ok();
            let healthy = executable.as_ref().is_some_and(|p| p.is_file());
            let reason = if class == InstallationClass::Dev { Some("Development build; launch is available, installation changes are disabled.".into()) }
                else if matches!(class, InstallationClass::Manual | InstallationClass::SelfInstalled) && peer.is_some() { Some("Installed outside Arcade Tools. Choose Let Arcade Tools manage this to enable updates.".into()) }
                else if peer.is_some_and(|m| !m.settings.link_enabled) { Some("Arcade Link is disabled in this app. Close it before changing its installation.".into()) }
                else if record.is_some() && !healthy { Some("Installed executable is missing. Choose Repair.".into()) } else { None };
            Ok(AppView { id: id.to_string(), name: app.name.clone(), pitch: app.pitch.clone(), releases_url: app.releases_url(),
                version: record.as_ref().map(|r| r.version.clone()).or_else(|| peer.map(|m| m.version.clone())), managed: matches!(class, InstallationClass::Managed | InstallationClass::LegacyManaged), installed: healthy,
                running, channel: prefs.channels.get(id).copied().or(record.as_ref().map(|r| r.channel)).unwrap_or_default(), healthy,
                start_at_login: platform::autostart_enabled(&self.paths, id), login_supported: app.supported_install() && platform::login_supported(id, Os::current()), executable: executable.map(|p| p.display().to_string()),
                data_folders: self.paths.data_folders(id, Os::current()).iter().map(|p| p.display().to_string()).collect(), reason,
                description: app.description.clone(), category: app.category.clone(), icon: app.icon.clone(),
                platforms: app.platforms.clone(), min_tools: app.min_tools.clone(), install_reason: app.install_reason.clone(),
                install_supported: app.supported_install(), installation_class: class, rollback_version: if app.supported_install() { self.rollback_version(id)? } else { None } })
        }).collect()
    }
    fn checked(&self, id: &str) -> Result<CheckedRelease> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
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
        if let Some(reason) = updates::held_back(&app, &check.checked.release, &peers) {
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
        platform::spawn_detached(&self.paths, &exe, &mode.args())?;
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
        self.operate_locked(request, cancel, progress, SystemClock.now())
    }
    fn operate_locked(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        time: u64,
    ) -> Result<String> {
        let old = self.record(&request.id)?;
        let mut history = HistoryEntry {
            time,
            app: request.id.clone(),
            from: old.as_ref().map(|r| r.version.clone()),
            to: None,
            result: HistoryResult::Success,
            reason: None,
        };
        let mut replaced = false;
        let result = self.operate_inner(request, cancel, progress, &mut history, &mut replaced);
        if matches!(
            request.operation,
            Operation::Update | Operation::Repair | Operation::Rollback
        ) {
            if result.is_ok() && request.operation == Operation::Rollback {
                history.result = HistoryResult::RolledBack;
                history.reason = Some("User requested rollback.".into());
            }
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
        let registry = Registry::load(&self.paths.link);
        let peer = registry.get(id);
        let receipt = self.receipt(id)?;
        let class = self.class(id, &registry, receipt.as_ref())?;
        if class == InstallationClass::Dev {
            return Err(Error::new(
                "dev_build",
                "Development builds are never modified by Arcade Tools.",
            ));
        }
        if request.operation == Operation::Adopt {
            return self.adopt_inner(request, peer, receipt, cancel, progress);
        }
        if request.operation == Operation::Rollback {
            return self.rollback_inner(
                request,
                Prior {
                    record: old.as_ref(),
                    receipt: receipt.as_ref(),
                    peer,
                },
                cancel,
                progress,
                history,
                replaced,
            );
        }
        if request.operation != Operation::Install && old.is_none() {
            return Err(Error::new(
                "unmanaged",
                "Let Arcade Tools manage this installation first.",
            ));
        }
        if matches!(request.operation, Operation::Update | Operation::Repair)
            && !matches!(
                class,
                InstallationClass::Managed | InstallationClass::LegacyManaged
            )
        {
            return Err(Error::new(
                "unmanaged",
                "Let Arcade Tools manage this installation first.",
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
            self.stop_peer(id, peer, request, false)?;
            progress(Progress::new(id, "Removing app", None, false));
            return self.uninstall(old.as_ref().unwrap(), request.remove_data);
        }
        progress(Progress::new(id, "Checking release", None, true));
        check_cancel(cancel)?;
        let checked = self.checked(id)?;
        history.to = Some(checked.release.version.clone());
        let migration = old
            .as_ref()
            .is_some_and(|r| r.kind == Kind::Tarball && checked.asset.kind == Kind::Appimage);
        if old.as_ref().is_some_and(|r| r.kind != checked.asset.kind) && !migration {
            return Err(Error::new("installer_changed", "This release changes installer format. Uninstall while keeping data, then install the new channel."));
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
        if (old.is_none() || migration) && destination.exists() {
            return Err(Error::new(
                "unmanaged",
                "The install location already contains a file. Adopt it before installing.",
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
        let package = installation::retain(&self.paths, &checked, downloaded.path())?;
        progress(Progress::new(id, "Verified SHA-256", Some(1.0), true));
        check_cancel(cancel)?;
        self.apply_package(
            request,
            &package,
            Prior {
                record: old.as_ref(),
                receipt: receipt.as_ref(),
                peer,
            },
            cancel,
            progress,
            replaced,
        )
    }

    fn apply_package(
        &self,
        request: &Request,
        package: &Package,
        prior: Prior<'_>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        replaced: &mut bool,
    ) -> Result<String> {
        package.verify(&self.paths, &request.id)?;
        self.ensure_compatible(&request.id, &package.release)?;
        let resume = self.stop_peer(&request.id, prior.peer, request, true)?;
        if let Err(error) = check_cancel(cancel) {
            if let Some(mode) = resume {
                let _ = self.launch_inner(&request.id, mode);
            }
            return Err(error);
        }
        let result = self.install_package(request, package, prior, resume, progress, replaced);
        if result.is_err() {
            if let Some(mode) = resume {
                let _ = self.launch_inner(&request.id, mode);
            }
        }
        result
    }

    fn install_package(
        &self,
        request: &Request,
        package: &Package,
        prior: Prior<'_>,
        resume: Option<LaunchMode>,
        progress: &mut dyn FnMut(Progress),
        replaced: &mut bool,
    ) -> Result<String> {
        let old = prior.record;
        let receipt = prior.receipt;
        let id = &request.id;
        let expected =
            self.paths
                .executable(id, Os::current(), package.asset.kind == Kind::Tarball);
        let prospective = Record {
            id: id.clone(),
            version: package.release.version.clone(),
            channel: package.release.channel,
            kind: package.asset.kind,
            sha256: package.asset.sha256.clone(),
            executable_sha256: String::new(),
            last_mode: resume.or(request.mode).or(old.and_then(|r| r.last_mode)),
            executable: Some(expected.clone()),
            package: Some(package.clone()),
            installed_by_app: false,
        };
        let old_target = old
            .map(|record| installation::target(&self.paths, record, receipt))
            .unwrap_or_else(|| installation::target(&self.paths, &prospective, None));
        let mut transaction = Transaction::begin(&self.paths, id, &old_target, receipt)?;
        transaction.include(&installation::target(&self.paths, &prospective, None))?;
        let previous = old.map(|record| transaction.previous(record.clone(), receipt.cloned()));
        let previous_pointer = previous
            .as_ref()
            .and_then(|previous| previous.pointer(&self.paths));
        let login = platform::autostart_enabled(&self.paths, id);
        progress(Progress::new(id, "Installing", None, false));
        *replaced = true;
        let install_args = installation::install_args(&package.asset, package.release.channel)?;
        if let Some(old) = old {
            if receipt.is_none() {
                // Bootstrap ownership for an old Tools install so Link's
                // installer can replace its existing stable AppImage safely.
                Store::new(&self.paths.link)
                    .write(&installation::legacy_receipt(&self.paths, old))?;
            }
        }
        let mut replacement = None;
        let executable = if package.asset.kind == Kind::Appimage && install_args.is_some() {
            installation::run_app_installer(
                &self.paths,
                &package.path,
                install_args.clone().unwrap(),
            )?;
            let installed = self.receipt(id)?.ok_or_else(|| {
                Error::new("receipt", "The app installer did not write its receipt.")
            })?;
            installation::validate_receipt(
                &self.paths,
                &installed,
                id,
                &package.release.version,
                package.release.channel,
            )?;
            let executable = installation::executable(&self.paths, &installed);
            if checksum(&executable)? != package.asset.sha256 {
                return Err(Error::new(
                    "checksum",
                    "The app did not install the verified AppImage bytes.",
                ));
            }
            executable
        } else {
            // Transition release only for Linux/macOS manifests without installArgs.
            // Windows always runs its verified native installer (old and new releases).
            let (executable, rollback) = platform::install_with_options(
                &self.paths,
                id,
                &package.asset,
                &package.path,
                package.release.channel,
                login,
            )?;
            replacement = Some(rollback);
            if let Some(args) = &install_args {
                installation::run_app_installer(&self.paths, &executable, args.clone())?;
            }
            executable
        };
        let app_receipt = self.receipt(id)?;
        let app_written = app_receipt.as_ref().is_some_and(|r| {
            r.version == package.release.version && r.channel == package.release.channel.as_str()
        });
        if install_args.is_some() && !app_written {
            return Err(Error::new(
                "receipt",
                "The app installer did not write a matching receipt.",
            ));
        }
        let mut record = prospective;
        record.executable = Some(executable.clone());
        record.executable_sha256 = checksum(&executable)?;
        record.installed_by_app =
            app_written && (install_args.is_some() || Os::current() == Os::Windows);
        if login && !record.installed_by_app {
            platform::set_autostart(&self.paths, id, &executable, true)?;
        }
        let mut installed_receipt = if app_written {
            app_receipt.unwrap()
        } else {
            installation::legacy_receipt(&self.paths, &record)
        };
        installation::validate_receipt(
            &self.paths,
            &installed_receipt,
            id,
            &record.version,
            record.channel,
        )?;
        if let Some(old) = receipt {
            installed_receipt.installed_at = old.installed_at.clone();
        }
        self.write(&self.paths.state().join(format!("{id}.json")), &record)?;
        // Launch before claiming: the app rewrites its own manifest/receipt. It
        // remains the only writer of capabilities, including when Link is off.
        let mode = resume.or(if request.operation == Operation::Install {
            Some(request.mode.unwrap_or(LaunchMode::Foreground))
        } else {
            None
        });
        if let Some(mode) = mode {
            progress(Progress::new(id, "Launching", None, false));
            platform::spawn_detached(&self.paths, &executable, &mode.args())?;
        }
        // Keep the app's receipt-addressed previous when it created one; shared
        // installers must retain ownership of their stable .previous filename.
        // The supplemental Tools snapshot additionally saves integration/state.
        let previous_pointer = if record.installed_by_app && Os::current() == Os::Linux {
            installed_receipt.previous.clone().or(previous_pointer)
        } else {
            previous_pointer
        };
        installation::claim(&self.paths, installed_receipt, previous_pointer)?;
        if old.is_some_and(|old| old.kind == Kind::Tarball && package.asset.kind == Kind::Appimage)
        {
            self.paths.guard(&old_target)?;
            platform::remove(&old_target)?;
        }
        transaction.commit(previous)?;
        if let Some(replacement) = replacement {
            replacement.commit();
        }
        *replaced = false;
        progress(Progress::new(id, "Done", Some(1.0), false));
        Ok(format!(
            "{} {} installed.",
            crate::apps::app_name(id),
            package.release.version
        ))
    }

    fn ensure_compatible(&self, id: &str, release: &crate::release::Release) -> Result<()> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let peers = self.update_inputs()?.1;
        if let Some(reason) = updates::held_back(&app, release, &peers) {
            return Err(Error::new("version_mismatch", reason));
        }
        Ok(())
    }

    fn adopt_inner(
        &self,
        request: &Request,
        peer: Option<&Manifest>,
        old_receipt: Option<Receipt>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        let peer = peer.ok_or_else(|| {
            Error::new(
                "not_installed",
                "There is no installed registry entry to adopt.",
            )
        })?;
        if old_receipt
            .as_ref()
            .is_some_and(|r| r.managed_by == ManagedBy::Tools)
        {
            return Ok("Already managed by Arcade Tools.".into());
        }
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(&request.id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        if let Some(reason) = app.install_reason {
            return Err(Error::new("version_mismatch", reason));
        }
        check_cancel(cancel)?;
        let source = PathBuf::from(&peer.executable);
        if arcade_link::install::is_dev_build(&source) {
            return Err(Error::new(
                "dev_build",
                "Development builds are never adopted.",
            ));
        }
        let resume = self.stop_peer(&request.id, Some(peer), request, true)?;
        let destination = self.paths.executable(&request.id, Os::current(), false);
        let target = if Os::current() == Os::Macos {
            self.paths.install_root(&request.id, Os::Macos, false)
        } else {
            destination.clone()
        };
        let transaction =
            Transaction::begin(&self.paths, &request.id, &target, old_receipt.as_ref())?;
        progress(Progress::new(
            &request.id,
            "Adopting installation",
            None,
            false,
        ));
        let result = (|| {
            let receipt = if Os::current() == Os::Linux {
                let channel = old_receipt
                    .as_ref()
                    .map(receipt_channel)
                    .transpose()?
                    .unwrap_or_default();
                if old_receipt.is_none() && source == destination {
                    // The user explicitly chose ownership of this existing file.
                    let now = manifest::now_rfc3339();
                    let receipt = Receipt {
                        schema: 1,
                        id: request.id.clone(),
                        version: peer.version.clone(),
                        channel: channel.as_str().into(),
                        method: InstallMethod::Appimage,
                        managed_by: ManagedBy::SelfManaged,
                        path: source.clone(),
                        integration: Default::default(),
                        previous: None,
                        installed_at: now.clone(),
                        updated_at: now,
                    };
                    Store::new(&self.paths.link).write(&receipt)?;
                }
                // Existing manual files are never chmod'ed, moved or deleted by
                // Tools. The app owns its explicit silent installation.
                platform::CommandSpec {
                    program: source.clone(),
                    args: vec![
                        "--install".into(),
                        "--silent".into(),
                        "--channel".into(),
                        channel.as_str().into(),
                    ],
                }
                .run(&self.paths)?;
                self.receipt(&request.id)?.ok_or_else(|| Error::new("receipt", "The app did not write an install receipt; use a release supporting --install."))?
            } else if let Some(receipt) = old_receipt.clone() {
                receipt
            } else {
                // Older native installs may predate receipts. Adoption explicitly
                // records the selected per-user file/bundle and its uninstaller.
                let record = Record {
                    id: request.id.clone(),
                    version: peer.version.clone(),
                    channel: Channel::Stable,
                    kind: if Os::current() == Os::Windows {
                        windows_kind(
                            source
                                .parent()
                                .map(|p| p.join("unins000.exe"))
                                .filter(|p| p.is_file())
                                .as_deref(),
                        )
                    } else {
                        Kind::Dmg
                    },
                    sha256: String::new(),
                    executable_sha256: checksum(&source)?,
                    last_mode: resume,
                    executable: Some(source.clone()),
                    package: None,
                    installed_by_app: true,
                };
                let mut receipt = installation::legacy_receipt(&self.paths, &record);
                if Os::current() == Os::Macos {
                    receipt.path = arcade_link::install::platform::macos::bundle_for(&source)
                        .ok_or_else(|| {
                            Error::new(
                                "receipt",
                                "The registry executable is not in an app bundle.",
                            )
                        })?;
                } else {
                    let root = source.parent().unwrap();
                    receipt.integration.uninstaller =
                        platform::windows_uninstall(record.kind, root)
                            .ok()
                            .map(|p| p.program)
                            .filter(|p| p.is_file());
                    if receipt.integration.uninstaller.is_none() {
                        return Err(Error::new("receipt", "The native uninstaller is missing; reinstall the app before adopting it."));
                    }
                }
                receipt
            };
            let channel = receipt_channel(&receipt)?;
            installation::validate_receipt(
                &self.paths,
                &receipt,
                &request.id,
                &peer.version,
                channel,
            )?;
            let mut record = self.record_from_receipt(&receipt)?;
            record.last_mode = resume.or(request.mode);
            if record.kind == Kind::Appimage {
                record.sha256 = record.executable_sha256.clone();
            }
            self.write(
                &self.paths.state().join(format!("{}.json", request.id)),
                &record,
            )?;
            let exe = self.executable(&record);
            if let Some(mode) = resume {
                platform::spawn_detached(&self.paths, &exe, &mode.args())?;
            }
            installation::claim(&self.paths, receipt.clone(), receipt.previous.clone())?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(transaction);
            if let Some(mode) = resume {
                let _ = platform::spawn_detached(&self.paths, &source, &mode.args());
            }
            return Err(error);
        }
        transaction.commit(None)?;
        progress(Progress::new(&request.id, "Done", Some(1.0), false));
        Ok(format!("{} is now managed by Arcade Tools.", peer.name))
    }

    fn rollback_inner(
        &self,
        request: &Request,
        prior: Prior<'_>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        history: &mut HistoryEntry,
        replaced: &mut bool,
    ) -> Result<String> {
        let receipt = prior.receipt;
        let peer = prior.peer;
        let old = prior.record.ok_or_else(|| {
            Error::new(
                "unmanaged",
                "There is no managed installation to roll back.",
            )
        })?;
        if receipt.is_some_and(|r| r.managed_by != ManagedBy::Tools) {
            return Err(Error::new(
                "unmanaged",
                "Let Arcade Tools manage this first.",
            ));
        }
        let previous = PreviousInstallation::read(&self.paths, &request.id)?
            .ok_or_else(|| Error::new("no_previous", "There is no retained previous version."))?;
        history.to = Some(previous.record.version.clone());
        check_cancel(cancel)?;
        if let Some(package) = &previous.record.package {
            self.apply_package(request, package, prior, cancel, progress, replaced)?;
        } else {
            let pointer = previous.pointer(&self.paths).ok_or_else(|| {
                Error::new("no_previous", "The retained previous file is missing.")
            })?;
            let exe = if previous.record.kind == Kind::Dmg {
                let original = self.paths.install_root(&request.id, Os::Macos, false);
                pointer.path.join(
                    self.executable(&previous.record)
                        .strip_prefix(original)
                        .map_err(|_| Error::new("paths", "Invalid previous bundle layout."))?,
                )
            } else {
                pointer.path.clone()
            };
            if checksum(&exe)? != previous.record.executable_sha256 {
                return Err(Error::new(
                    "checksum",
                    "The previous executable failed verification.",
                ));
            }
            if previous.record.kind == Kind::Appimage && previous.record.installed_by_app {
                let asset = crate::release::Asset {
                    os: Os::Linux,
                    arch: Arch::current()?,
                    kind: Kind::Appimage,
                    file: pointer.path.file_name().unwrap().to_string_lossy().into(),
                    sha256: previous.record.executable_sha256.clone(),
                    size: Some(fs::metadata(&pointer.path)?.len()),
                    silent: None,
                    install_args: Some(vec!["--install".into(), "--silent".into()]),
                };
                let release = crate::release::Release {
                    schema: 1,
                    id: request.id.clone(),
                    version: previous.record.version.clone(),
                    channel: previous.record.channel,
                    link_protocol: previous.protocols(&self.paths),
                    notes: String::new(),
                    assets: vec![asset.clone()],
                };
                let package = Package {
                    release,
                    asset,
                    path: pointer.path,
                };
                let retained = installation::retain(
                    &self.paths,
                    &CheckedRelease {
                        release: package.release.clone(),
                        asset: package.asset.clone(),
                        url: String::new(),
                    },
                    &package.path,
                )?;
                self.apply_package(request, &retained, prior, cancel, progress, replaced)?;
            } else {
                let release = crate::release::Release {
                    schema: 1,
                    id: request.id.clone(),
                    version: previous.record.version.clone(),
                    channel: previous.record.channel,
                    link_protocol: previous.protocols(&self.paths),
                    notes: String::new(),
                    assets: Vec::new(),
                };
                self.ensure_compatible(&request.id, &release)?;
                let resume = self.stop_peer(&request.id, peer, request, true)?;
                let transaction = Transaction::begin(
                    &self.paths,
                    &request.id,
                    &installation::target(&self.paths, old, receipt),
                    receipt,
                )?;
                let current = transaction.previous(old.clone(), receipt.cloned());
                *replaced = true;
                previous.restore(&self.paths)?;
                let mut record = previous.record.clone();
                record.last_mode = resume.or(request.mode);
                let exe = self.executable(&record);
                if record.installed_by_app && record.kind == Kind::Dmg {
                    platform::CommandSpec {
                        program: exe.clone(),
                        args: vec![
                            "--install".into(),
                            "--silent".into(),
                            "--channel".into(),
                            record.channel.as_str().into(),
                        ],
                    }
                    .run(&self.paths)?;
                }
                let restored = self
                    .receipt(&request.id)?
                    .unwrap_or_else(|| installation::legacy_receipt(&self.paths, &record));
                self.write(
                    &self.paths.state().join(format!("{}.json", request.id)),
                    &record,
                )?;
                if let Some(mode) = resume {
                    platform::spawn_detached(&self.paths, &exe, &mode.args())?;
                }
                installation::claim(&self.paths, restored, current.pointer(&self.paths))?;
                transaction.commit(Some(current))?;
                *replaced = false;
            }
        }
        progress(Progress::new(&request.id, "Rolled back", Some(1.0), false));
        Ok(format!(
            "{} rolled back to {}.",
            crate::apps::app_name(&request.id),
            previous.record.version
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
        let receipt = self.receipt(id)?;
        let previous = PreviousInstallation::read(&self.paths, id)?;
        let target = installation::target(&self.paths, record, receipt.as_ref());
        self.paths.guard(&target)?;
        if let Some(receipt) = &receipt {
            installation::owned_paths(&self.paths, receipt)?;
            if let Some(previous) = &receipt.previous {
                self.paths.guard(&previous.path)?;
            }
        }
        if remove_data {
            for folder in self.paths.data_folders(id, Os::current()) {
                self.paths.guard(&folder)?;
            }
        }
        if record.installed_by_app {
            if Os::current() == Os::Windows {
                let uninstaller = receipt
                    .as_ref()
                    .and_then(|r| r.integration.uninstaller.as_ref())
                    .ok_or_else(|| {
                        Error::new(
                            "installer",
                            "The receipt's native uninstaller is missing. Repair the app first.",
                        )
                    })?;
                self.paths.guard(uninstaller)?;
                if !uninstaller.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The native uninstaller is missing. Repair the app first.",
                    ));
                }
                let mut command = platform::windows_uninstall(
                    windows_kind(Some(uninstaller)),
                    uninstaller.parent().unwrap(),
                )?;
                command.program = uninstaller.clone();
                command.run(&self.paths)?;
            } else {
                if !exe.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The app's uninstaller is missing. Repair the app first.",
                    ));
                }
                let mut args = vec!["--uninstall".into(), "--silent".into()];
                if remove_data {
                    args.push("--remove-data".into());
                }
                platform::CommandSpec {
                    program: exe.clone(),
                    args,
                }
                .run(&self.paths)?;
            }
            if exe.exists() {
                return Err(Error::new(
                    "installer",
                    "The app's uninstaller left its executable in place.",
                ));
            }
        } else {
            // Transition releases without installArgs use the pre-v0.3 removal
            // path. Never use this fallback for an app with an own installer.
            if platform::login_supported(id, Os::current()) {
                platform::set_autostart(&self.paths, id, &exe, false)?;
            }
            #[cfg(windows)]
            {
                let uninstaller = platform::windows_uninstall(record.kind, &root)?;
                self.paths.guard(&uninstaller.program)?;
                if !uninstaller.program.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The platform uninstaller is missing. Repair the app first.",
                    ));
                }
                uninstaller.run(&self.paths)?;
            }
            platform::remove(&target)?;
        }
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
        self.paths.guard_receipt(id)?;
        Store::new(&self.paths.link).remove(id)?;
        if let Some(previous) = previous {
            previous.remove(&self.paths)?;
        }
        platform::remove(&self.paths.state().join(format!("{id}.previous.json")))?;
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

/// Pure known-install contract check; safe on any thread. Tools has its own
/// catalog entry, distinct from the seven apps shown on the Apps page.
fn manager_id(id: &str) -> Result<()> {
    if id == ids::TOOLS {
        Ok(())
    } else {
        app_id(id)
    }
}
fn receipt_channel(receipt: &Receipt) -> Result<Channel> {
    match receipt.channel.as_str() {
        "stable" => Ok(Channel::Stable),
        "nightly" => Ok(Channel::Nightly),
        _ => Err(Error::new("receipt", "Unsupported receipt channel.")),
    }
}
fn windows_kind(uninstaller: Option<&Path>) -> Kind {
    if uninstaller.and_then(Path::file_name).is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("unins")
    }) {
        Kind::Inno
    } else {
        Kind::Nsis
    }
}
