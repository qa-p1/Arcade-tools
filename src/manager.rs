use crate::catalog::{Catalog, CatalogClient};
use crate::docs::DocsResolver;
use crate::engines::EngineCatalog;
use crate::installation::{self, Package, PreviousInstallation, Transaction};
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
impl Record {
    fn update_available(&self, checked: &CheckedRelease) -> bool {
        updates::is_newer(
            &InstalledApp {
                id: self.id.clone(),
                version: self.version.clone(),
                channel: self.channel,
                selected_channel: checked.release.channel,
                sha256: (!self.sha256.is_empty()).then(|| self.sha256.clone()),
            },
            &checked.release,
            &checked.asset.sha256,
        )
    }
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
    #[serde(default = "yes")]
    pub notifications_enabled: bool,
    #[serde(default = "system_theme")]
    pub theme: String,
    #[serde(default)]
    pub onboarding_complete: bool,
}
fn system_theme() -> String {
    "system".into()
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
            notifications_enabled: true,
            theme: system_theme(),
            onboarding_complete: false,
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
    scheduler: Mutex<Option<updates::Scheduler>>,
}
impl Manager {
    /// Construct without I/O; safe on any thread. All manager I/O methods below
    /// are synchronous and must be invoked on worker threads.
    pub fn new(paths: Paths, source: Source) -> Self {
        Self {
            paths,
            source,
            mutation: Mutex::new(()),
            scheduler: Mutex::new(None),
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
    pub fn validate_preferences(&self, prefs: &Preferences) -> Result<()> {
        if !matches!(prefs.theme.as_str(), "system" | "light" | "dark") {
            return Err(Error::new("preferences", "Unknown theme."));
        }
        for id in prefs
            .channels
            .keys()
            .chain(prefs.updates.auto_update.keys())
        {
            manager_id(id)?;
        }
        Ok(())
    }
    pub fn restore_preferences(&self, prefs: &Preferences) -> Result<()> {
        let _lock = self.lock()?;
        self.validate_preferences(prefs)?;
        self.save_preferences(prefs)
    }
    pub fn set_notifications(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.notifications_enabled = enabled;
        self.save_preferences(&prefs)
    }
    pub fn set_theme(&self, theme: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.theme = theme.into();
        self.validate_preferences(&prefs)?;
        self.save_preferences(&prefs)
    }
    pub fn complete_onboarding(&self) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.onboarding_complete = true;
        self.save_preferences(&prefs)
    }
    /// Persist a channel selection on a worker thread; no network work.
    pub fn set_channel(&self, id: &str, channel: Channel) -> Result<()> {
        manager_id(id)?;
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
    /// Execute an opted-in update on a worker. Disabled checks, unknown use,
    /// recent use, disconnected/busy apps and compatibility holds return a
    /// decision without modifying the installation. No timer is started here.
    /// The caller supplies its latest last-used timestamp (Unix seconds).
    pub fn auto_update(
        &self,
        id: &str,
        last_used: Option<u64>,
        clock: &dyn Clock,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<updates::AutoUpdateOutcome> {
        app_id(id)?;
        let _lock = self.lock()?;
        let decision = self.auto_eligibility(id, last_used, clock.now())?;
        if !decision.eligible {
            return Ok(updates::AutoUpdateOutcome {
                decision,
                message: None,
            });
        }
        let request = Request {
            id: id.into(),
            operation: Operation::Update,
            mode: None,
            remove_data: false,
            app_closed: false,
        };
        match self.operate_locked(&request, cancel, progress, clock.now()) {
            Ok(message) => Ok(updates::AutoUpdateOutcome {
                decision,
                message: Some(message),
            }),
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "busy"
                        | "disconnected"
                        | "version_mismatch"
                        | "dev_build"
                        | "unmanaged"
                        | "mode_required"
                ) =>
            {
                Ok(updates::AutoUpdateOutcome {
                    decision: updates::AutoUpdateDecision {
                        eligible: false,
                        reason: error.message,
                    },
                    message: None,
                })
            }
            Err(error) => Err(error),
        }
    }
    fn auto_eligibility(
        &self,
        id: &str,
        last_used: Option<u64>,
        now: u64,
    ) -> Result<updates::AutoUpdateDecision> {
        let settings = self.preferences()?.updates;
        let deny = |reason: &str| updates::AutoUpdateDecision {
            eligible: false,
            reason: reason.into(),
        };
        if !settings.enabled {
            return Ok(deny("Update checks are off."));
        }
        if !settings.auto_update.get(id).copied().unwrap_or(false) {
            return Ok(deny("Auto-update is off for this app."));
        }
        let registry = Registry::load(&self.paths.link);
        let receipt = self.receipt(id)?;
        let class = self.class(id, &registry, receipt.as_ref())?;
        let updatable = matches!(
            class,
            InstallationClass::Managed | InstallationClass::LegacyManaged
        ) || (id == ids::TOOLS && class == InstallationClass::SelfInstalled);
        if !updatable {
            return Ok(deny(
                "Only managed, non-development installations can auto-update.",
            ));
        }
        if registry
            .get(id)
            .is_some_and(|peer| !peer.settings.link_enabled)
        {
            return Ok(deny(
                "Arcade Link is disabled; close the app and update manually.",
            ));
        }
        let mut status = match Client::connect(&self.paths.link, id, &me()) {
            Ok(mut client) => client.status().map_err(|error| link_error(id, error))?,
            Err(error) if error.code == arcade_link::ErrorCode::NotRunning => {
                if arcade_link::endpoint::read(&self.paths.link, id)
                    .is_ok_and(|endpoint| process_alive(endpoint.pid))
                {
                    return Ok(deny("A running app has an unavailable Link endpoint."));
                }
                json!({"busy": false})
            }
            Err(error) => return Ok(deny(&link_error(id, error).message)),
        };
        if self.own_process(id) {
            status["busy"] = false.into();
            if let Some(nested) = status.get_mut("status") {
                nested["busy"] = false.into();
            }
        }
        Ok(updates::auto_update_policy(true, &status, last_used, now))
    }

    fn own_process(&self, id: &str) -> bool {
        id == ids::TOOLS
            && arcade_link::endpoint::read(&self.paths.link, id)
                .is_ok_and(|endpoint| endpoint.pid == std::process::id())
    }

    /// Download and verify Tools itself on a worker, persist a handoff plan, then
    /// call the tray/desktop hook. Tools cannot replace its running Windows
    /// executable; the hook starts a helper and owns shutdown. The helper calls
    /// finish_self_update after this process exits, using the same installer API.
    pub fn prepare_self_update(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<crate::self_update::StagedSelfUpdate> {
        let _lock = self.lock()?;
        self.prepare_self_update_locked(mode, cancel, progress, hook)
    }

    /// Stage rollback for the same separate helper used by self updates.
    pub fn prepare_self_rollback(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        hook: &mut dyn FnMut(&Path) -> Result<()>,
    ) -> Result<PathBuf> {
        let _lock = self.lock()?;
        check_cancel(cancel)?;
        let receipt = self.receipt(ids::TOOLS)?;
        if !matches!(
            self.class(
                ids::TOOLS,
                &Registry::load(&self.paths.link),
                receipt.as_ref()
            )?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "unmanaged",
                "Install Arcade Tools before rolling it back.",
            ));
        }
        if self.rollback_version(ids::TOOLS)?.is_none() {
            return Err(Error::new(
                "no_previous",
                "There is no retained previous Tools version.",
            ));
        }
        let old = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Tools is not installed."))?;
        let plan = crate::self_update::SelfRollbackPlan {
            schema: 1,
            expected_version: old.version.clone(),
            expected_path: self.executable(&old),
            expected_sha256: checksum(&self.executable(&old))?,
            mode,
            parent_pid: std::process::id(),
        };
        let file = self.paths.state().join("self-update").join(format!(
            "rollback-{}.json",
            arcade_link::endpoint::new_token()?
        ));
        self.write(&file, &plan)?;
        if let Err(error) = hook(&file) {
            platform::remove(&file)?;
            return Err(error);
        }
        Ok(file)
    }

    /// Revalidate the current installation after shutdown, then roll back.
    pub fn finish_self_rollback(
        &self,
        plan: &crate::self_update::SelfRollbackPlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        if plan.schema != 1 || plan.parent_pid == std::process::id() {
            return Err(Error::new(
                "self_update",
                "Tools rollback requires a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Old Arcade Tools did not exit; rollback left it unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let old = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Tools is not installed."))?;
        self.paths.guard(&plan.expected_path)?;
        if old.version != plan.expected_version
            || self.executable(&old) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after staging; request rollback again.",
            ));
        }
        let result = self.operate_locked(
            &Request {
                id: ids::TOOLS.into(),
                operation: Operation::Rollback,
                mode: Some(plan.mode),
                remove_data: false,
                app_closed: true,
            },
            cancel,
            progress,
            SystemClock.now(),
        );
        let launch = platform::spawn_detached(&self.paths, &plan.expected_path, &plan.mode.args());
        if result.is_ok() {
            launch?;
        }
        result
    }
    fn prepare_self_update_locked(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<crate::self_update::StagedSelfUpdate> {
        let id = ids::TOOLS;
        check_cancel(cancel)?;
        let old = self.record(id)?.ok_or_else(|| {
            Error::new(
                "unmanaged",
                "Install or adopt Arcade Tools before self-updating.",
            )
        })?;
        let receipt = self.receipt(id)?;
        let registry = Registry::load(&self.paths.link);
        if !matches!(
            self.class(id, &registry, receipt.as_ref())?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "unmanaged",
                "Only managed non-development Tools installs can self-update.",
            ));
        }
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            let status = client.status().map_err(|error| link_error(id, error))?;
            if !self.own_process(id)
                && (status.get("busy").and_then(Value::as_bool) == Some(true)
                    || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true))
            {
                return Err(Error::new(
                    "busy",
                    "Arcade Tools is busy; finish its operation before self-updating.",
                ));
            }
        }
        progress(Progress::new(id, "Checking release", None, true));
        let checked = self.checked(id)?;
        self.paths.guard(&self.paths.downloads())?;
        let download = self.source.download(
            &checked,
            &self.paths.downloads(),
            cancel,
            &mut |count, total| {
                progress(Progress::new(
                    id,
                    "Downloading",
                    total.filter(|n| *n > 0).map(|n| count as f64 / n as f64),
                    true,
                ));
            },
        )?;
        let package = installation::retain(&self.paths, &checked, download.path())?;
        check_cancel(cancel)?;
        let plan = crate::self_update::SelfUpdatePlan {
            schema: 1,
            package,
            expected_version: old.version.clone(),
            expected_path: self.executable(&old),
            expected_sha256: checksum(&self.executable(&old))?,
            mode,
            parent_pid: std::process::id(),
        };
        let file = self
            .paths
            .state()
            .join("self-update")
            .join(format!("{}.json", arcade_link::endpoint::new_token()?));
        self.write(&file, &plan)?;
        let staged = crate::self_update::StagedSelfUpdate { plan, file };
        if let Err(error) = hook.on_before_self_restart(&staged) {
            platform::remove(&staged.file)?;
            return Err(error);
        }
        Ok(staged)
    }
    /// Apply the opted-in idle policy before staging a Tools update on a worker.
    /// None means ineligible; use auto_update_eligibility for the UI's reason.
    pub fn prepare_auto_self_update(
        &self,
        last_used: Option<u64>,
        clock: &dyn Clock,
        mode: LaunchMode,
        cancel: &AtomicBool,
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<Option<crate::self_update::StagedSelfUpdate>> {
        let _lock = self.lock()?;
        if !self
            .auto_eligibility(ids::TOOLS, last_used, clock.now())?
            .eligible
        {
            return Ok(None);
        }
        self.prepare_self_update_locked(mode, cancel, &mut |_| {}, hook)
            .map(Some)
    }
    /// Read current policy/status on a worker, without network or installation.
    pub fn auto_update_eligibility(
        &self,
        id: &str,
        last_used: Option<u64>,
        clock: &dyn Clock,
    ) -> Result<updates::AutoUpdateDecision> {
        manager_id(id)?;
        self.auto_eligibility(id, last_used, clock.now())
    }
    /// Run in a helper on a worker after the parent exits. Active, bounded
    /// process-exit checks last at most 30 seconds, then leave files unchanged.
    /// The plan's hashes and expected installation are rechecked before mutation.
    /// On installer failure the shared transaction restores/relaunches old Tools.
    pub fn finish_self_update(
        &self,
        plan: &crate::self_update::SelfUpdatePlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        if plan.schema != 1
            || plan.package.release.id != ids::TOOLS
            || plan.parent_pid == std::process::id()
        {
            return Err(Error::new(
                "self_update",
                "A Tools self-update must run in a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        plan.package.verify(&self.paths, ids::TOOLS)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Old Arcade Tools did not exit; self-update left it unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let old = self.record(ids::TOOLS)?.ok_or_else(|| {
            Error::new("unmanaged", "Tools installation disappeared after staging.")
        })?;
        if old.version != plan.expected_version
            || self.executable(&old) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after staging; check for updates again.",
            ));
        }
        let receipt = self.receipt(ids::TOOLS)?;
        if !matches!(
            self.class(
                ids::TOOLS,
                &Registry::load(&self.paths.link),
                receipt.as_ref()
            )?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "dev_build",
                "Development or unmanaged Tools installations are never replaced.",
            ));
        }
        self.ensure_compatible(ids::TOOLS, &plan.package.release)?;
        check_cancel(cancel)?;
        let request = Request {
            id: ids::TOOLS.into(),
            operation: Operation::Update,
            mode: Some(plan.mode),
            remove_data: false,
            app_closed: true,
        };
        let mut replaced = false;
        let result = self.install_package(
            &request,
            &plan.package,
            Prior {
                record: Some(&old),
                receipt: receipt.as_ref(),
                peer: None,
            },
            Some(plan.mode),
            progress,
            &mut replaced,
        );
        let history = HistoryEntry {
            time: SystemClock.now(),
            app: ids::TOOLS.into(),
            from: Some(old.version),
            to: Some(plan.package.release.version.clone()),
            result: if result.is_ok() {
                HistoryResult::Success
            } else if replaced
                && checksum(&plan.expected_path).is_ok_and(|hash| hash == plan.expected_sha256)
            {
                HistoryResult::RolledBack
            } else {
                HistoryResult::Failed
            },
            reason: result.as_ref().err().map(ToString::to_string),
        };
        if result.is_err() {
            let _ = platform::spawn_detached(&self.paths, &plan.expected_path, &plan.mode.args());
        }
        if let Err(error) = updates::append_history(&self.paths, &history) {
            if result.is_ok() {
                return Err(error);
            }
            eprintln!("Self-update history: {error}");
        }
        result
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
                // Keep release hashes/rollback metadata when the app rewrites
                // only its build suffix, but never trust changed executable bytes.
                let same_bytes = record.version == receipt.version
                    || checksum(&executable).is_ok_and(|hash| hash == record.executable_sha256);
                if !crate::release::versions_match(
                    &record.version,
                    &receipt.version,
                    record.channel,
                ) || !same_bytes
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
    /// A bundle moved to Trash no longer owns an installed app. Run once on
    /// the startup worker; missing Linux files remain repairable installations.
    pub fn cleanup_stale_bundles(&self) -> Result<Vec<String>> {
        let _lock = self.lock()?;
        let mut removed = Vec::new();
        for id in crate::apps::APPS.iter().copied().chain([ids::TOOLS]) {
            let Some(receipt) = self.receipt(id)? else {
                continue;
            };
            if receipt.method != InstallMethod::MacosBundle || receipt.path.exists() {
                continue;
            }
            installation::cleanup_missing_bundle(&self.paths, &receipt)?;
            Store::new(&self.paths.link).remove(id)?;
            for file in [
                self.paths.state().join(format!("{id}.json")),
                self.paths.state().join(format!("{id}.previous.json")),
            ] {
                self.paths.guard(&file)?;
                platform::remove(&file)?;
            }
            let registry = Registry::load(&self.paths.link);
            if registry
                .get(id)
                .is_some_and(|peer| !peer.executable_exists())
            {
                manifest::remove_manifest(&self.paths.link, id)?;
            }
            removed.push(id.into());
        }
        Ok(removed)
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
    /// Read cached engine metadata or explicitly refresh it on a worker thread.
    pub fn engines(&self, refresh: bool) -> Result<EngineCatalog> {
        CatalogClient::new(self.paths.clone(), self.source.clone())?.engines(refresh)
    }
    /// Read persisted update results and reconcile them with the current
    /// registry/catalog snapshot. This does not make network requests.
    pub fn update_state(&self) -> Result<UpdateState> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let mut guard = self.scheduler.lock().unwrap();
        if guard.is_none() {
            *guard = Some(updates::Scheduler::new(
                self.paths.clone(),
                SystemClock,
                self.source.clone(),
            )?);
        }
        let scheduler = guard.as_mut().unwrap();
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
        self.check_updates_with(trigger, true)
            .map(|(state, _)| state)
    }
    pub fn check_updates_with(
        &self,
        trigger: Trigger,
        online: bool,
    ) -> Result<(UpdateState, Vec<updates::UpdateNotification>)> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let settings = self.preferences()?.updates;
        let mut guard = self.scheduler.lock().unwrap();
        if guard.is_none() {
            *guard = Some(updates::Scheduler::new(
                self.paths.clone(),
                SystemClock,
                self.source.clone(),
            )?);
        }
        let scheduler = guard.as_mut().unwrap();
        let outcome = scheduler.run(
            trigger,
            CheckContext {
                settings: &settings,
                catalog: &catalog,
                installed: &installed,
                peers: &peers,
                // A manual request treats connectivity as unknown/online; the
                // first failed transport is recorded by the scheduler.
                online,
                os: Os::current(),
                arch: Arch::current()?,
            },
        )?;
        Ok((scheduler.state().clone(), outcome.notifications))
    }
    pub fn update_deadline(&self) -> Result<Option<u64>> {
        self.update_state()?;
        Ok(self
            .scheduler
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .next_deadline(&self.preferences()?.updates))
    }
    pub fn last_used(&self, id: &str) -> Option<u64> {
        let status = Client::connect_with(&self.paths.link, id, &me(), Duration::from_millis(250))
            .ok()?
            .status()
            .ok()?;
        status
            .pointer("/status/lastUsed")
            .or_else(|| status.pointer("/status/lastUsedAt"))
            .or_else(|| status.get("lastUsed"))
            .or_else(|| status.get("lastUsedAt"))
            .and_then(Value::as_u64)
    }
    /// Fetch and cache one app's docs for the selected installed version. This
    /// method never runs unless Learn explicitly requests the download.
    pub fn fetch_docs(&self, id: &str, version: &str) -> Result<()> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resolver = DocsResolver::new(self.paths.clone(), PathBuf::new());
        resolver.fetch_version(&app, version, &self.source)?;
        Ok(())
    }
    pub fn fetch_latest_docs(&self, id: &str) -> Result<String> {
        let app = self
            .catalog()?
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resolver = DocsResolver::new(self.paths.clone(), PathBuf::new());
        Ok(resolver.fetch_latest(&app, &self.source)?.version)
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
        for id in crate::apps::APPS {
            let peer = registry.get(id);
            let receipt = self.receipt(id)?;
            let Some((executable, version)) = peer
                .map(|peer| (PathBuf::from(&peer.executable), peer.version.clone()))
                .or_else(|| {
                    receipt.as_ref().map(|receipt| {
                        (
                            installation::executable(&self.paths, receipt),
                            receipt.version.clone(),
                        )
                    })
                })
            else {
                continue;
            };
            let class = self.class(id, &registry, receipt.as_ref())?;
            let can_adopt = matches!(
                class,
                InstallationClass::Manual | InstallationClass::SelfInstalled
            );
            result.push(Adoption {
                id: id.to_string(),
                class,
                executable,
                version,
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
        self.list_catalog(probe, false)
    }
    pub fn tools_view(&self) -> Result<Option<AppView>> {
        Ok(self
            .list_catalog(false, true)?
            .into_iter()
            .find(|app| app.id == ids::TOOLS))
    }
    fn list_catalog(&self, probe: bool, include_tools: bool) -> Result<Vec<AppView>> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let mut catalog = self.catalog()?;
        if include_tools {
            if let Some(tools) = catalog.install_entry(ids::TOOLS) {
                catalog.apps.push(tools);
            }
        }
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
                else if id != ids::TOOLS && matches!(class, InstallationClass::Manual | InstallationClass::SelfInstalled) && (peer.is_some() || receipt.is_some()) { Some("Installed outside Arcade Tools. Choose Let Arcade Tools manage this to enable updates.".into()) }
                else if peer.is_some_and(|m| !m.settings.link_enabled) { Some("Arcade Link is disabled in this app. Close it before changing its installation.".into()) }
                else if record.is_some() && !healthy { Some("Installed executable is missing. Choose Repair.".into()) }
                else { record.as_ref().and_then(first_update_rollback_notice).map(str::to_owned) };
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
            notes: self
                .release_body(id, checked.release.channel)
                .unwrap_or_else(|| checked.release.notes.clone()),
            update_available: record.is_none_or(|r| r.update_available(&checked)),
        })
    }
    fn release_body(&self, id: &str, channel: Channel) -> Option<String> {
        let path = self
            .paths
            .cache
            .join("arcade/tools/releases")
            .join(id)
            .join(format!("{}.json", channel.as_str()));
        let bytes = crate::catalog::read_bounded(&path, 6 * 1024 * 1024).ok()?;
        let cache: crate::release::ReleaseCache = serde_json::from_slice(&bytes).ok()?;
        Some(cache.body)
    }

    pub fn release_history(&self, id: &str) -> Result<serde_json::Value> {
        let entry = self
            .catalog()?
            .install_entry(id)
            .ok_or_else(|| Error::new("unsupported_input", "Unknown catalog app."))?;
        let raw = self.source.release_history(&entry.repo)?;
        let items=raw.as_array().unwrap().iter().filter(|release|release["draft"]!=true).take(20).map(|release| {
            serde_json::json!({"version":release["tag_name"],"publishedAt":release["published_at"],"prerelease":release["prerelease"],"html":crate::markdown::render(release["body"].as_str().unwrap_or_default(),&[])})
        }).collect::<Vec<_>>();
        Ok(serde_json::json!(items))
    }
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        let path = self.paths.state().join("update-history.jsonl");
        let bytes = match crate::catalog::read_bounded(&path, 8 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(_) if !path.exists() => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        String::from_utf8_lossy(&bytes)
            .lines()
            .rev()
            .take(200)
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
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
        let registry = Registry::load(&self.paths.link);
        if self.class(id, &registry, self.receipt(id)?.as_ref())? == InstallationClass::Dev {
            return Err(Error::new(
                "dev_build",
                "Development builds keep their own integration.",
            ));
        }
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
    /// List every catalog/registry installation and its exact owned/data paths
    /// on a worker before an Uninstall everything confirmation. No mutation.
    pub fn plan_uninstall_all(&self, remove_data: bool) -> Result<crate::removal::UninstallPlan> {
        let _lock = self.lock()?;
        self.uninstall_plan_inner(remove_data)
    }
    fn uninstall_plan_inner(&self, remove_data: bool) -> Result<crate::removal::UninstallPlan> {
        use crate::removal::{UninstallEntry, UninstallPlan};
        use sha2::{Digest, Sha256};
        let registry = Registry::load(&self.paths.link);
        let catalog = self.catalog()?;
        let mut ids: std::collections::BTreeSet<String> =
            catalog.apps.iter().map(|app| app.id.clone()).collect();
        ids.extend(crate::apps::APPS.into_iter().map(String::from));
        ids.extend(registry.apps().iter().map(|peer| peer.id.clone()));
        ids.remove(ids::TOOLS);
        let mut apps = Vec::new();
        let mut fingerprint = Sha256::new();
        for id in ids.into_iter().chain([ids::TOOLS.into()]) {
            let known = manager_id(&id).is_ok();
            let record = if known { self.record(&id)? } else { None };
            let receipt = if known { self.receipt(&id)? } else { None };
            let peer = registry.get(&id);
            if record.is_none() && receipt.is_none() && peer.is_none() {
                continue;
            }
            let class = self.class(&id, &registry, receipt.as_ref())?;
            let mut folders = Vec::new();
            if let Some(record) = &record {
                folders.push(installation::target(&self.paths, record, receipt.as_ref()));
                folders.push(self.paths.state().join(format!("{id}.json")));
                if let Some(previous) = PreviousInstallation::read(&self.paths, &id)? {
                    folders.push(previous.directory);
                }
                folders.push(self.paths.state().join(format!("{id}.previous.json")));
            } else if let Some(peer) = peer {
                folders.push(PathBuf::from(&peer.executable));
            }
            if let Some(receipt) = &receipt {
                folders.extend(installation::owned_paths(&self.paths, receipt)?);
                folders.push(self.paths.guard_receipt(&id)?);
                if let Some(previous) = &receipt.previous {
                    folders.push(previous.path.clone());
                }
            }
            folders.push(self.paths.link.registry.join(format!("{id}.json")));
            let mut data_folders = self.paths.data_folders(&id, Os::current());
            if id == ids::TOOLS {
                data_folders.push(self.paths.cache.join("arcade/tools"));
            }
            let can_uninstall = known && class != InstallationClass::Dev && record.is_some();
            if can_uninstall {
                for path in folders.iter().chain(if remove_data {
                    data_folders.iter()
                } else {
                    [].iter()
                }) {
                    if receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.integration.cli.as_ref() == Some(path))
                    {
                        installation::owned_paths(&self.paths, receipt.as_ref().unwrap())?;
                    } else if path == &self.paths.guard_receipt(&id)? {
                        self.paths.guard_receipt(&id)?;
                    } else {
                        self.paths.guard(path)?;
                    }
                }
            }
            folders.sort();
            folders.dedup();
            fingerprint.update(serde_json::to_vec(&(
                record.as_ref(),
                receipt.as_ref(),
                peer,
            ))?);
            if let Some(record) = &record {
                fingerprint.update(checksum(&self.executable(record)).unwrap_or_default());
            }
            apps.push(UninstallEntry {
                id: id.clone(),
                version: record
                    .as_ref()
                    .map(|record| record.version.clone())
                    .or_else(|| peer.map(|peer| peer.version.clone())),
                class,
                folders,
                data_folders,
                can_uninstall,
                requires_self_shutdown: id == ids::TOOLS,
                reason: if can_uninstall {
                    None
                } else {
                    Some(
                        if class == InstallationClass::Dev {
                            "Development builds are never removed."
                        } else {
                            "Adopt this manual installation before uninstalling it with Tools."
                        }
                        .into(),
                    )
                },
            });
        }
        Ok(UninstallPlan {
            schema: 1,
            remove_data,
            apps,
            fingerprint: format!("{:x}", fingerprint.finalize()),
        })
    }
    /// Execute a previously confirmed plan on a worker. Any changed installation
    /// rejects the entire plan before app.quit. Individual busy/disconnected
    /// failures are returned. Tools removal is handed to a post-shutdown helper.
    pub fn uninstall_all(
        &self,
        plan: &crate::removal::UninstallPlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<crate::removal::UninstallOutcome> {
        let _lock = self.lock()?;
        if plan.schema != 1 || &self.uninstall_plan_inner(plan.remove_data)? != plan {
            return Err(Error::new(
                "state_changed",
                "Installations changed; review the uninstall list again.",
            ));
        }
        let mut outcome = crate::removal::UninstallOutcome {
            results: Vec::new(),
            self_uninstall: None,
        };
        for app in &plan.apps {
            check_cancel(cancel)?;
            if !app.can_uninstall {
                outcome.results.push(crate::removal::RemovalResult {
                    app: app.id.clone(),
                    removed: false,
                    reason: app.reason.clone().unwrap_or_default(),
                });
                continue;
            }
            if app.requires_self_shutdown {
                outcome.self_uninstall = Some(self.self_uninstall_plan(plan.remove_data)?);
                continue;
            }
            let request = Request {
                id: app.id.clone(),
                operation: Operation::Uninstall,
                mode: None,
                remove_data: plan.remove_data,
                app_closed: false,
            };
            let result = self.operate_locked(&request, cancel, progress, SystemClock.now());
            outcome.results.push(crate::removal::RemovalResult {
                app: app.id.clone(),
                removed: result.is_ok(),
                reason: match result {
                    Ok(message) => message,
                    Err(error) => error.to_string(),
                },
            });
        }
        Ok(outcome)
    }
    /// Prepare removal of Tools itself after user confirmation, on a worker.
    /// The desktop starts a helper and exits; no running executable is deleted.
    pub fn prepare_self_uninstall(
        &self,
        remove_data: bool,
    ) -> Result<crate::self_update::SelfUninstallPlan> {
        let _lock = self.lock()?;
        self.self_uninstall_plan(remove_data)
    }
    fn self_uninstall_plan(
        &self,
        remove_data: bool,
    ) -> Result<crate::self_update::SelfUninstallPlan> {
        let record = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Arcade Tools has no install receipt."))?;
        let receipt = self.receipt(ids::TOOLS)?;
        if self.class(
            ids::TOOLS,
            &Registry::load(&self.paths.link),
            receipt.as_ref(),
        )? == InstallationClass::Dev
        {
            return Err(Error::new(
                "dev_build",
                "Development builds are never uninstalled.",
            ));
        }
        let path = self.executable(&record);
        self.paths.guard(&path)?;
        Ok(crate::self_update::SelfUninstallPlan {
            schema: 1,
            expected_version: record.version,
            expected_sha256: checksum(&path)?,
            expected_path: path,
            parent_pid: std::process::id(),
            remove_data,
        })
    }
    /// Run in a helper on a worker after Tools exits; bounded process-exit checks
    /// and fresh state/hash checks precede the app's own silent uninstaller.
    pub fn finish_self_uninstall(
        &self,
        plan: &crate::self_update::SelfUninstallPlan,
        cancel: &AtomicBool,
    ) -> Result<String> {
        if plan.schema != 1 || plan.parent_pid == std::process::id() {
            return Err(Error::new(
                "self_update",
                "Tools removal must run in a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Arcade Tools did not exit; nothing was removed.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.paths.guard(&plan.expected_path)?;
        let record = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("state_changed", "Tools is already uninstalled."))?;
        if self.class(
            ids::TOOLS,
            &Registry::load(&self.paths.link),
            self.receipt(ids::TOOLS)?.as_ref(),
        )? == InstallationClass::Dev
            || record.version != plan.expected_version
            || self.executable(&record) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after confirmation; nothing was removed.",
            ));
        }
        check_cancel(cancel)?;
        let result = self.uninstall(&record, plan.remove_data)?;
        if plan.remove_data {
            let cache = self.paths.cache.join("arcade/tools");
            self.paths.guard(&cache)?;
            platform::remove(&cache)?;
        }
        Ok(result)
    }

    /// Read Home diagnostics on a worker; no IPC/network or automatic cleanup.
    pub fn health(&self) -> Result<Vec<crate::health::HealthIssue>> {
        crate::health::inspect(&self.paths, &self.catalog()?)
    }
    /// Apply a user-selected fix on a worker after rechecking the issue. The UI
    /// supplies the usual confirmation for adoption/repair, and separately asks
    /// before any data removal. This API never changes development builds.
    pub fn fix_health(
        &self,
        id: &str,
        action: crate::health::FixAction,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        manager_id(id)?;
        let _lock = self.lock()?;
        if !self
            .health()?
            .iter()
            .any(|issue| issue.app == id && issue.fix_action == action && issue.can_fix)
        {
            return Err(Error::new(
                "state_changed",
                "This health issue is no longer present or cannot be fixed automatically.",
            ));
        }
        use crate::health::FixAction;
        match action {
            FixAction::Repair | FixAction::Adopt => {
                let request = Request {
                    id: id.into(),
                    operation: if action == FixAction::Repair {
                        Operation::Repair
                    } else {
                        Operation::Adopt
                    },
                    mode: None,
                    remove_data: false,
                    app_closed: false,
                };
                self.operate_locked(&request, cancel, progress, SystemClock.now())
            }
            FixAction::RemoveStaleRegistry => {
                let file = self.paths.link.registry.join(format!("{id}.json"));
                self.paths.guard(&file)?;
                let document = arcade_link::manifest::ManifestDocument::from_json(
                    &String::from_utf8_lossy(&crate::catalog::read_bounded(&file, 1024 * 1024)?),
                )
                .map_err(|error| Error::new("manifest", error))?;
                if document.manifest.executable_exists() {
                    return Err(Error::new(
                        "state_changed",
                        "The executable is present; its registry entry was kept.",
                    ));
                }
                manifest::remove_manifest(&self.paths.link, id)?;
                Ok("Removed the stale registry entry.".into())
            }
            FixAction::RemoveStaleReceipt => {
                let receipt = self
                    .receipt(id)?
                    .ok_or_else(|| Error::new("state_changed", "The receipt is already gone."))?;
                if installation::executable(&self.paths, &receipt).exists() {
                    return Err(Error::new(
                        "state_changed",
                        "The receipt's executable is present; adoption is required.",
                    ));
                }
                installation::cleanup_missing_bundle(&self.paths, &receipt)?;
                Store::new(&self.paths.link).remove(id)?;
                Ok("Removed the stale receipt; no app or data files were removed.".into())
            }
            FixAction::OpenSettings => self
                .settings_inner(id)
                .map(|()| "Opened the app's settings.".into()),
            FixAction::CheckUpdates => self
                .checked(id)
                .map(|checked| format!("Latest release: {}.", checked.release.version)),
            FixAction::OpenEngines | FixAction::OpenShortcuts => Err(Error::new(
                "guidance",
                "Open the corresponding Tools page to review this issue.",
            )),
        }
    }
    /// Open app-owned settings using a feature-detected Link helper, or the
    /// standard --settings CLI when Link is off/unsupported, on a worker.
    pub fn open_settings(&self, id: &str) -> Result<()> {
        manager_id(id)?;
        let _lock = self.lock()?;
        self.settings_inner(id)
    }
    fn settings_inner(&self, id: &str) -> Result<()> {
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            let description = client
                .describe_full()
                .map_err(|error| link_error(id, error))?;
            if description.supports("app.settings") {
                match client.settings() {
                    Ok(()) => return Ok(()),
                    Err(error) if error.is_unsupported() => {}
                    Err(error) => return Err(link_error(id, error)),
                }
            }
        }
        let registry = Registry::load(&self.paths.link);
        let exe = self
            .record(id)?
            .as_ref()
            .map(|record| self.executable(record))
            .or_else(|| registry.get(id).map(|peer| PathBuf::from(&peer.executable)))
            .filter(|path| path.is_file())
            .ok_or_else(|| Error::new("not_installed", "The app's executable is missing."))?;
        platform::spawn_detached(&self.paths, &exe, &["--settings".into()])
    }
    /// Request app-owned restart with feature detection on a worker. Old servers
    /// use the same busy-safe quit and launch fallback, preserving window mode.
    pub fn restart(&self, id: &str, mode: Option<LaunchMode>) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            if mode.is_none()
                && client
                    .describe_full()
                    .map_err(|error| link_error(id, error))?
                    .supports("app.restart")
            {
                match client.restart(None) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.is_unsupported() => {}
                    Err(error) => return Err(link_error(id, error)),
                }
            }
        }
        let registry = Registry::load(&self.paths.link);
        let request = Request {
            id: id.into(),
            operation: Operation::Update,
            mode,
            remove_data: false,
            app_closed: false,
        };
        let resume = self.stop_peer(id, registry.get(id), &request, true)?;
        self.launch_inner(id, resume.or(mode).unwrap_or(LaunchMode::Foreground))
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
            && old.as_ref().is_some_and(|r| !r.update_available(&checked))
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
        let native_root = if matches!(Os::current(), Os::Windows | Os::Macos) {
            platform::native_install_root(receipt)
        } else {
            None
        };
        let (_, expected) = platform::native_install_paths(
            &self.paths,
            id,
            Os::current(),
            package.asset.kind == Kind::Tarball,
            native_root.as_deref(),
        );
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
                native_root.as_deref(),
            )?;
            replacement = Some(rollback);
            if let Some(args) = &install_args {
                installation::run_app_installer(&self.paths, &executable, args.clone())?;
            }
            executable
        };
        let app_receipt = self.receipt(id)?;
        let app_written = app_receipt.as_ref().is_some_and(|r| {
            crate::release::versions_match(
                &r.version,
                &package.release.version,
                package.release.channel,
            ) && r.channel == package.release.channel.as_str()
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
        let receipt_peer = old_receipt.as_ref().map(|receipt| {
            Manifest::new(
                &request.id,
                &receipt.version,
                &installation::executable(&self.paths, receipt).to_string_lossy(),
            )
        });
        let peer = peer.or(receipt_peer.as_ref()).ok_or_else(|| {
            Error::new(
                "not_installed",
                "There is no installation receipt or registry entry to adopt.",
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
                // Stage an explicit adoption too: self installers may remove
                // their source; the user's current manual file stays untouched.
                installation::run_app_installer(
                    &self.paths,
                    &source,
                    vec![
                        "--install".into(),
                        "--silent".into(),
                        "--channel".into(),
                        channel.as_str().into(),
                        "--no-launch".into(),
                    ],
                )?;
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
        if receipt.is_some_and(|r| r.managed_by != ManagedBy::Tools) && request.id != ids::TOOLS {
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
                let original =
                    installation::target(&self.paths, &previous.record, previous.receipt.as_ref());
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
                let result = (|| -> Result<()> {
                    let mut transaction = Transaction::begin(
                        &self.paths,
                        &request.id,
                        &installation::target(&self.paths, old, receipt),
                        receipt,
                    )?;
                    transaction.include(&installation::target(
                        &self.paths,
                        &previous.record,
                        previous.receipt.as_ref(),
                    ))?;
                    let current = transaction.previous(old.clone(), receipt.cloned());
                    *replaced = true;
                    previous.restore(&self.paths)?;
                    let mut record = previous.record.clone();
                    record.last_mode = resume.or(request.mode);
                    let exe = self.executable(&record);
                    // The snapshot includes the receipt and integration. An
                    // adopted older bundle may not support --install; invoking
                    // it again can also move a restored /Applications bundle.
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
                    Ok(())
                })();
                if let Err(error) = result {
                    if let Some(mode) = resume {
                        let _ = self.launch_inner(&request.id, mode);
                    }
                    return Err(error);
                }
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

fn first_update_rollback_notice(record: &Record) -> Option<&'static str> {
    (matches!(record.kind, Kind::Nsis | Kind::Inno) && record.package.is_none()).then_some(
        "Rollback is unavailable for the first Windows update after adoption because the previous installer is not cached. Later updates retain an installer for rollback.",
    )
}

/// Bounded transient successor wait; never used by idle services.
pub fn wait_for_exit(pid: u32) -> Result<()> {
    if pid == 0 || pid == std::process::id() {
        return Err(Error::new("usage", "Invalid predecessor process."));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while process_alive(pid) {
        if Instant::now() >= deadline {
            return Err(Error::new("timeout", "The predecessor did not exit."));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod native_rollback_tests {
    use super::*;
    use arcade_link::receipt::Integration;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn adopted_bundle_rollback_restores_integration_without_requiring_install_flags() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let manager = Manager::new(paths.clone(), Source::github().unwrap());
        let bundle = paths.home.join("Applications/Arcade Look.app");
        let executable = bundle.join("Contents/MacOS/arcade-look");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        // Older standalone versions did not implement --install. Executing
        // this during rollback must fail the test even if it is later restored.
        let marker = paths.home.join("unexpected-installer");
        let old_bytes = format!("#!/bin/sh\ntouch '{}'\nexit 99\n", marker.display());
        fs::write(&executable, &old_bytes).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let cli = paths.home.join(".local/bin/arcade-look");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        symlink(&executable, &cli).unwrap();
        let mut receipt = Receipt {
            schema: 1,
            id: ids::LOOK.into(),
            version: "0.1.0".into(),
            channel: "stable".into(),
            method: InstallMethod::MacosBundle,
            managed_by: ManagedBy::Tools,
            path: bundle.clone(),
            integration: Integration {
                cli: Some(cli.clone()),
                ..Default::default()
            },
            previous: None,
            installed_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let store = Store::new(&paths.link);
        store.write(&receipt).unwrap();
        let record = manager.record_from_receipt(&receipt).unwrap();
        let transaction = Transaction::begin(&paths, ids::LOOK, &bundle, Some(&receipt)).unwrap();
        let previous = transaction.previous(record, Some(receipt.clone()));
        fs::write(&executable, b"new bundle").unwrap();
        receipt.version = "0.2.0".into();
        store.write(&receipt).unwrap();
        let current = manager.record_from_receipt(&receipt).unwrap();
        manager
            .write(&paths.state().join("arcade.look.json"), &current)
            .unwrap();
        transaction.commit(Some(previous)).unwrap();
        let request = Request {
            id: ids::LOOK.into(),
            operation: Operation::Rollback,
            mode: None,
            remove_data: false,
            app_closed: true,
        };
        manager
            .operate(&request, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
        assert_eq!(fs::read_to_string(executable).unwrap(), old_bytes);
        assert_eq!(store.read(ids::LOOK).unwrap().unwrap().version, "0.1.0");
        assert_eq!(
            fs::read_link(cli).unwrap(),
            bundle.join("Contents/MacOS/arcade-look")
        );
        assert!(!marker.exists(), "rollback executed the old installer");
    }
}
