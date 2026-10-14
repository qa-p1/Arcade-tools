//! Update discovery and policies. The UI owns one process-scoped timer set to
//! `next_deadline`; it invokes `run` on a worker thread and drops the scheduler
//! when Tools exits. No background threads, polling, notification transport or
//! installer are started here. Settings are supplied on every wake so disabling
//! checks takes effect without restarting Tools.
use crate::catalog::{read_bounded, Catalog, CatalogApp};
use crate::paths::Paths;
use crate::release::{Arch, Channel, Os, Release, ReleaseCache};
use crate::source::{ReleaseCheck, Source};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FIRST_CHECK: u64 = 60;
pub const CHECK_INTERVAL: u64 = 6 * 60 * 60;
pub const WINDOW_MAX_AGE: u64 = 60 * 60;
pub const AUTO_UPDATE_IDLE: u64 = 10 * 60;

/// Injectable clock. Values are Unix seconds, suitable for persisted timestamps.
pub trait Clock {
    /// Pure clock read, safe on any thread.
    fn now(&self) -> u64;
}
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_update: BTreeMap<String, bool>,
}
fn yes() -> bool {
    true
}
impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_update: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub id: String,
    pub version: String,
    pub channel: Channel,
    pub selected_channel: Channel,
    pub sha256: Option<String>,
}
#[derive(Debug, Clone)]
pub struct PeerProtocols {
    pub id: String,
    pub protocols: Vec<u32>,
}
/// Synchronous injectable release source; implementations perform I/O on workers.
pub trait ReleaseSource {
    /// Check with a validator on a worker thread. Implementations must validate
    /// app/channel/platform and manifest schema before returning a new cache.
    fn check_release(&self, request: ReleaseRequest<'_>) -> Result<ReleaseCheck>;
}
pub struct ReleaseRequest<'a> {
    pub app: &'a CatalogApp,
    pub channel: Channel,
    pub os: Os,
    pub arch: Arch,
    pub cached: Option<&'a ReleaseCache>,
}
impl ReleaseSource for Source {
    fn check_release(&self, request: ReleaseRequest<'_>) -> Result<ReleaseCheck> {
        self.check_repo_conditional(
            &request.app.id,
            &request.app.repo,
            request.channel,
            request.os,
            request.arch,
            request.cached,
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelUpdate {
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub held_back: bool,
    pub reason: Option<String>,
    pub error: Option<String>,
    pub cache: Option<ReleaseCache>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdate {
    pub installed_version: Option<String>,
    pub channel: Channel,
    pub latest: BTreeMap<String, ChannelUpdate>,
    pub update_available: bool,
    pub held_back: bool,
    pub reason: Option<String>,
    #[serde(default)]
    pub notified: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub schema: u32,
    pub last_check: Option<u64>,
    pub last_success: Option<u64>,
    pub last_failure: Option<CheckFailure>,
    pub apps: BTreeMap<String, AppUpdate>,
}
impl Default for UpdateState {
    fn default() -> Self {
        Self {
            schema: 1,
            last_check: None,
            last_success: None,
            last_failure: None,
            apps: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckFailure {
    pub time: u64,
    pub reason: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Timer,
    WindowOpen,
    Manual,
}
pub struct CheckContext<'a> {
    pub settings: &'a UpdateSettings,
    pub catalog: &'a Catalog,
    pub installed: &'a [InstalledApp],
    pub peers: &'a [PeerProtocols],
    /// Known offline state prevents all requests. Unknown connectivity may be
    /// treated as online; the first transport failure is recorded and stops work.
    pub online: bool,
    pub os: Os,
    pub arch: Arch,
}
#[derive(Debug, Clone, Serialize)]
pub struct UpdateNotification {
    pub app: String,
    pub version: String,
    pub channel: Channel,
}
#[derive(Debug, Clone, Default)]
pub struct CheckOutcome {
    pub checked: bool,
    pub notifications: Vec<UpdateNotification>,
    pub failure: Option<String>,
}
pub struct Scheduler<C = SystemClock, S = Source> {
    paths: Paths,
    clock: C,
    source: S,
    state: UpdateState,
    next_check: u64,
}
impl<C: Clock, S: ReleaseSource> Scheduler<C, S> {
    /// Load state from disk on a worker thread. Starts no timers or network work.
    /// Every new process delays its timer's first check by 60 seconds.
    pub fn new(paths: Paths, clock: C, source: S) -> Result<Self> {
        let path = paths.state().join("updates.json");
        let state = match read_bounded(&path, 16 * 1024 * 1024) {
            Ok(bytes) => {
                let state: UpdateState = serde_json::from_slice(&bytes)?;
                if state.schema != 1 {
                    return Err(Error::new(
                        "schema",
                        "Only update state schema 1 is supported.",
                    ));
                }
                state
            }
            Err(e) if !path.exists() && e.code == "io" => UpdateState::default(),
            Err(e) => return Err(e),
        };
        let next_check = clock.now().saturating_add(FIRST_CHECK);
        Ok(Self {
            paths,
            clock,
            source,
            state,
            next_check,
        })
    }
    /// In-memory view, safe on any thread. Hold/release state is refreshed on a check.
    pub fn state(&self) -> &UpdateState {
        &self.state
    }
    /// Pure recalculation from supplied installation/registry snapshots, safe on
    /// any thread. Call after an install or registry event to keep the displayed
    /// state current without a network check. Persisted at the next `run`.
    pub fn reconcile(
        &mut self,
        catalog: &Catalog,
        installed: &[InstalledApp],
        peers: &[PeerProtocols],
    ) {
        for state in self.state.apps.values_mut() {
            state.installed_version = None;
            state.update_available = false;
            state.held_back = false;
            state.reason = None;
        }
        for installed in installed {
            let Some(app) = catalog.install_entry(&installed.id) else {
                continue;
            };
            let state = self.state.apps.entry(installed.id.clone()).or_default();
            state.installed_version = Some(installed.version.clone());
            state.channel = installed.selected_channel;
            for latest in state.latest.values_mut() {
                if let Some(cache) = &latest.cache {
                    latest.update_available =
                        is_newer(installed, &cache.release, &cache.asset.sha256);
                    latest.reason = held_back(&app, &cache.release, peers);
                    latest.held_back = latest.reason.is_some();
                }
            }
            sync_selected(state);
        }
    }
    /// Pure deadline calculation, safe on any thread. `None` means cancel the
    /// caller's single timer while update checks are disabled.
    pub fn next_deadline(&self, settings: &UpdateSettings) -> Option<u64> {
        settings.enabled.then_some(self.next_check)
    }
    /// Pure scheduling decision, safe on any thread. Exact 1-hour age is still fresh.
    pub fn due(&self, trigger: Trigger, settings: &UpdateSettings) -> bool {
        if !settings.enabled {
            return false;
        }
        let now = self.clock.now();
        match trigger {
            Trigger::Timer => now >= self.next_check,
            Trigger::WindowOpen => self
                .state
                .last_check
                .is_none_or(|t| now.saturating_sub(t) > WINDOW_MAX_AGE),
            Trigger::Manual => true,
        }
    }
    fn persist(&self) -> Result<()> {
        let path = self.paths.state().join("updates.json");
        self.paths.guard(&path)?;
        arcade_link::paths::write_atomic(&path, &serde_json::to_vec_pretty(&self.state)?, true)?;
        Ok(())
    }
    /// Conditional network checks and disk persistence, on a worker thread.
    /// Returns data for the caller to notify; never sends a desktop notification
    /// or runs an installer. A returned version is persisted as notified once.
    pub fn run(&mut self, trigger: Trigger, context: CheckContext<'_>) -> Result<CheckOutcome> {
        if !self.due(trigger, context.settings) {
            return Ok(CheckOutcome::default());
        }
        let previous = self.state.clone();
        let result = self.run_due(context);
        if result.is_err() {
            self.state = previous;
        }
        result
    }
    fn run_due(&mut self, context: CheckContext<'_>) -> Result<CheckOutcome> {
        let now = self.clock.now();
        self.next_check = now.saturating_add(CHECK_INTERVAL);
        self.state.last_check = Some(now);
        self.state.last_failure = None;
        let mut outcome = CheckOutcome {
            checked: true,
            ..Default::default()
        };
        self.reconcile(context.catalog, context.installed, context.peers);
        if !context.online {
            let reason = "Offline; no update requests were sent.".to_owned();
            self.state.last_failure = Some(CheckFailure {
                time: now,
                reason: reason.clone(),
            });
            outcome.failure = Some(reason);
            self.persist()?;
            return Ok(outcome);
        }
        for installed in context.installed {
            let Some(app) = context.catalog.install_entry(&installed.id) else {
                continue;
            };
            let state = self.state.apps.entry(installed.id.clone()).or_default();
            state.installed_version = Some(installed.version.clone());
            state.channel = installed.selected_channel;
            let mut transport_failed = false;
            for channel in [Channel::Stable, Channel::Nightly] {
                let latest = state.latest.entry(channel.as_str().into()).or_default();
                match self.source.check_release(ReleaseRequest {
                    app: &app,
                    channel,
                    os: context.os,
                    arch: context.arch,
                    cached: latest.cache.as_ref(),
                }) {
                    Ok(check) => {
                        let release = &check.checked.release;
                        latest.latest_version = Some(release.version.clone());
                        latest.update_available =
                            is_newer(installed, release, &check.checked.asset.sha256);
                        latest.reason = held_back(&app, release, context.peers);
                        latest.held_back = latest.reason.is_some();
                        latest.error = None;
                        latest.cache = Some(check.cache);
                    }
                    Err(error) => {
                        latest.error = Some(error.to_string());
                        outcome.failure = Some(error.to_string());
                        // HTTP errors (e.g. no nightly release) don't imply offline.
                        // A transport failure stops further outbound requests.
                        if error.code == "network" {
                            transport_failed = true;
                            break;
                        }
                    }
                }
            }
            sync_selected(state);
            if let Some(selected) = state.latest.get(installed.selected_channel.as_str()) {
                state.update_available = selected.update_available;
                state.held_back = selected.held_back;
                state.reason = selected.reason.clone();
                if selected.error.is_none() && state.update_available && !state.held_back {
                    if let Some(version) = &selected.latest_version {
                        let key = format!("{}:{version}", installed.selected_channel.as_str());
                        if state.notified.insert(key) {
                            outcome.notifications.push(UpdateNotification {
                                app: app.id.clone(),
                                version: version.clone(),
                                channel: installed.selected_channel,
                            });
                        }
                    }
                }
            }
            if transport_failed {
                break;
            }
        }
        if let Some(reason) = &outcome.failure {
            self.state.last_failure = Some(CheckFailure {
                time: now,
                reason: reason.clone(),
            });
        } else {
            self.state.last_success = Some(now);
        }
        self.persist()?;
        Ok(outcome)
    }
}
fn sync_selected(state: &mut AppUpdate) {
    if let Some(selected) = state.latest.get(state.channel.as_str()) {
        state.update_available = selected.update_available;
        state.held_back = selected.held_back;
        state.reason = selected.reason.clone();
    }
}
pub(crate) fn is_newer(installed: &InstalledApp, release: &Release, sha256: &str) -> bool {
    if installed.channel != release.channel {
        return true;
    }
    if release.channel == Channel::Stable {
        if let (Ok(old), Ok(new)) = (
            semver::Version::parse(&installed.version),
            semver::Version::parse(&release.version),
        ) {
            match new.cmp_precedence(&old) {
                std::cmp::Ordering::Greater => return true,
                std::cmp::Ordering::Less => return false,
                // App receipts may omit release build metadata. Only a known
                // different package hash proves an update at equal precedence.
                std::cmp::Ordering::Equal => {
                    return installed
                        .sha256
                        .as_deref()
                        .is_some_and(|hash| hash != sha256);
                }
            }
        }
    }
    installed.version != release.version
        || installed
            .sha256
            .as_deref()
            .is_some_and(|hash| hash != sha256)
}
/// Pure compatibility policy, safe on any thread. The replaced app's previous
/// manifest is excluded; every remaining installed peer must share a protocol.
pub fn held_back(app: &CatalogApp, release: &Release, peers: &[PeerProtocols]) -> Option<String> {
    if let Some(reason) = &app.install_reason {
        return Some(reason.clone());
    }
    if !app.supported_install() {
        return Some("This version of Arcade Tools has no install contract for this app.".into());
    }
    if !release.link_protocol.contains(&1) {
        return Some("Update Arcade Tools first; the release needs a newer Link protocol.".into());
    }
    for peer in peers.iter().filter(|peer| peer.id != app.id) {
        if !peer
            .protocols
            .iter()
            .any(|p| release.link_protocol.contains(p))
        {
            return Some(format!(
                "Update {} first; its Link protocols do not intersect with this release.",
                peer.id
            ));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoUpdateDecision {
    pub eligible: bool,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct AutoUpdateOutcome {
    pub decision: AutoUpdateDecision,
    /// Present after execution. The desktop may notify once this is returned.
    pub message: Option<String>,
}
/// Pure opt-in/idle policy, safe on any thread. Older apps without last-used
/// status can update when explicitly not busy; known use still waits 10 minutes.
/// Recheck this and compatibility immediately before calling `Manager::operate`.
pub fn auto_update_policy(
    opted_in: bool,
    status: &serde_json::Value,
    last_used: Option<u64>,
    now: u64,
) -> AutoUpdateDecision {
    let busy_states = [
        status.pointer("/status/busy").and_then(|v| v.as_bool()),
        status.get("busy").and_then(|v| v.as_bool()),
    ];
    let busy = if busy_states.contains(&Some(true)) {
        Some(true)
    } else {
        busy_states.into_iter().flatten().next()
    };
    let reason = if !opted_in {
        Some("Auto-update is off for this app.")
    } else if busy == Some(true) {
        Some("The app is busy; finish its job first.")
    } else if busy.is_none() {
        Some("The app's busy state is unknown.")
    } else if last_used.is_some_and(|used| now.saturating_sub(used) < AUTO_UPDATE_IDLE) {
        Some("The app was used within the last 10 minutes.")
    } else {
        None
    };
    AutoUpdateDecision {
        eligible: reason.is_none(),
        reason: reason
            .unwrap_or("The app is idle and auto-update is enabled.")
            .into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResult {
    Success,
    Failed,
    RolledBack,
    RollbackFailed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub time: u64,
    pub app: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub result: HistoryResult,
    #[serde(default)]
    pub reason: Option<String>,
}
/// Append and sync one JSON-lines history record on a worker thread. No network.
pub fn append_history(paths: &Paths, entry: &HistoryEntry) -> Result<()> {
    let path = paths.state().join("update-history.jsonl");
    paths.guard(&path)?;
    std::fs::create_dir_all(paths.state())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut bytes = serde_json::to_vec(entry)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.sync_data()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(old: &str, new: &str, hash: Option<&str>, channel: Channel) -> bool {
        is_newer(
            &InstalledApp {
                id: "arcade.wheel".into(),
                version: old.into(),
                channel,
                selected_channel: channel,
                sha256: hash.map(String::from),
            },
            &Release {
                schema: 1,
                id: "arcade.wheel".into(),
                version: new.into(),
                channel,
                link_protocol: vec![1],
                notes: String::new(),
                assets: vec![],
            },
            "same",
        )
    }

    #[test]
    fn stable_build_metadata_with_same_hash_is_not_an_update() {
        for (old, new) in [
            ("0.3.0", "0.3.0+build.40"),
            ("0.3.0+build.40", "0.3.0"),
            ("0.3.0+build.40", "0.3.0+build.41"),
        ] {
            assert!(!newer(old, new, Some("same"), Channel::Stable));
        }
    }

    #[test]
    fn stable_build_metadata_with_changed_hash_is_an_update() {
        for (old, new) in [
            ("0.3.0", "0.3.0+build.40"),
            ("0.3.0+build.40", "0.3.0"),
            ("0.3.0+build.40", "0.3.0+build.39"),
        ] {
            assert!(newer(old, new, Some("different"), Channel::Stable));
        }
    }

    #[test]
    fn stable_build_metadata_without_recorded_hash_is_not_an_update() {
        for (old, new) in [
            ("0.3.0", "0.3.0+build.40"),
            ("0.3.0+build.40", "0.3.0"),
            ("0.3.0+build.40", "0.3.0+build.41"),
        ] {
            assert!(!newer(old, new, None, Channel::Stable));
        }
    }

    #[test]
    fn stable_higher_version_after_build_metadata_is_an_update() {
        assert!(newer(
            "0.3.0+build.40",
            "0.4.0",
            Some("same"),
            Channel::Stable
        ));
    }

    #[test]
    fn stable_lower_version_with_build_metadata_is_not_an_update() {
        assert!(!newer(
            "0.3.0",
            "0.2.9+build.99",
            Some("different"),
            Channel::Stable
        ));
    }

    #[test]
    fn nightly_keeps_literal_version_and_hash_comparisons() {
        assert!(newer(
            "0.3.0",
            "0.3.0+build.40",
            Some("same"),
            Channel::Nightly
        ));
        assert!(newer("0.3.0", "0.3.0", Some("different"), Channel::Nightly));
        assert!(!newer("0.3.0", "0.3.0", None, Channel::Nightly));
    }
}
