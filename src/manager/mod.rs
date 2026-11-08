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
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod checks;
mod launch;
mod maintenance;
mod operations;
mod preferences;
mod records;
mod self_update;
mod uninstall;

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
    pub tagline: String,
    pub releases_url: String,
    pub version: Option<String>,
    pub managed: bool,
    pub installed: bool,
    pub running: bool,
    /// The selected channel (what the next update follows).
    pub channel: Channel,
    /// The channel of the installed build; differs from `channel` until the switch has been installed (TL-9).
    pub installed_channel: Option<Channel>,
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
    /// Why the installed release was withdrawn (catalog `bad`), if it was: the app view offers a rollback.
    pub withdrawn: Option<String>,
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
    /// The channel and release version the user reviewed. When set, an
    /// install or update refuses to run on anything else.
    #[serde(default)]
    pub channel: Option<Channel>,
    #[serde(default)]
    pub version: Option<String>,
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
    hashes: Mutex<HashMap<PathBuf, (Fingerprint, String)>>,
    maintenance_attempts: Mutex<std::collections::HashSet<String>>,
}
/// What changes whenever a file's bytes do: a write updates mtime and ctime
/// (ctime can't be set back by the writer), a replacement changes the inode.
#[derive(Clone, PartialEq, Eq)]
struct Fingerprint(u64, Option<std::time::SystemTime>, u64, i64, i64);
impl Fingerprint {
    fn of(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        #[cfg(unix)]
        let (ino, ctime, ctime_ns) = {
            use std::os::unix::fs::MetadataExt;
            (meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        #[cfg(not(unix))]
        let (ino, ctime, ctime_ns) = (0, 0, 0);
        Ok(Self(meta.len(), meta.modified().ok(), ino, ctime, ctime_ns))
    }
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
            hashes: Mutex::new(HashMap::new()),
            maintenance_attempts: Mutex::new(std::collections::HashSet::new()),
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.mutation
            .try_lock()
            .map_err(|_| Error::new("busy", "Another manager operation is running."))
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
            channel: None,
            version: None,
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
