use crate::paths::{app_id, Paths};
use crate::platform;
use crate::release::{checksum, Arch, Channel, Kind, Os};
use crate::source::{CheckedRelease, Source};
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
}
fn yes() -> bool {
    true
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            link_enabled: true,
            channels: BTreeMap::new(),
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
    pub fn set_channel(&self, id: &str, channel: Channel) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.channels.insert(id.into(), channel);
        self.save_preferences(&prefs)
    }
    pub fn set_link(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.link_enabled = enabled;
        self.save_preferences(&prefs)
    }
    fn write(&self, path: &std::path::Path, value: &impl Serialize) -> Result<()> {
        self.paths.guard(path)?;
        arcade_link::paths::write_atomic(path, &serde_json::to_vec_pretty(value)?, true)?;
        Ok(())
    }
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
    pub fn list(&self, probe: bool) -> Result<Vec<AppView>> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        ids::APPS.iter().map(|id| {
            let record = self.record(id)?;
            let peer = registry.get(id);
            let executable = record.as_ref().map(|r| self.executable(r)).or_else(|| peer.map(|m| PathBuf::from(&m.executable)));
            let running = probe && Client::connect(&self.paths.link, id, &me()).is_ok();
            let healthy = executable.as_ref().is_some_and(|p| p.is_file());
            let reason = if record.is_none() && peer.is_some() { Some("Installed outside Arcade Tools. Launch is available; updates and removal need a managed installation.".into()) }
                else if peer.is_some_and(|m| !m.settings.link_enabled) { Some("Arcade Link is disabled in this app. Close it before changing its installation.".into()) }
                else if record.is_some() && !healthy { Some("Installed executable is missing. Choose Repair.".into()) } else { None };
            Ok(AppView { id: id.to_string(), name: manifest::app_name(id).into(), pitch: manifest::app_pitch(id).into(), releases_url: manifest::releases_url(id).into(),
                version: record.as_ref().map(|r| r.version.clone()).or_else(|| peer.map(|m| m.version.clone())), managed: record.is_some(), installed: healthy,
                running, channel: prefs.channels.get(*id).copied().or(record.as_ref().map(|r| r.channel)).unwrap_or_default(), healthy,
                start_at_login: platform::autostart_enabled(&self.paths, id), login_supported: platform::login_supported(id, Os::current()), executable: executable.map(|p| p.display().to_string()),
                data_folders: self.paths.data_folders(id, Os::current()).iter().map(|p| p.display().to_string()).collect(), reason })
        }).collect()
    }
    fn checked(&self, id: &str) -> Result<CheckedRelease> {
        let prefs = self.preferences()?;
        let channel = prefs
            .channels
            .get(id)
            .copied()
            .or(self.record(id)?.map(|r| r.channel))
            .unwrap_or_default();
        self.source
            .check(id, channel, Os::current(), Arch::current()?)
    }
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
    pub fn operate(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        app_id(&request.id)?;
        let _lock = self.lock()?;
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
            let (exe, replacement) =
                platform::install(&self.paths, id, &checked.asset, downloaded.path())?;
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
            manifest::app_name(id),
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
                arcade_link::LinkError::busy().user_message(manifest::app_name(id)),
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
            manifest::app_name(id),
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
        error.user_message(manifest::app_name(id)),
    )
}
