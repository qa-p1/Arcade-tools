//! Receipt ownership, verified retained packages and reversible installation
//! snapshots. All filesystem/process APIs are synchronous worker work. Receipts
//! describe ownership/layout; Tools records only add hashes and release metadata.
use crate::manager::Record;
use crate::paths::Paths;
use crate::platform::{self, CommandSpec};
use crate::release::{self, Asset, Channel, Kind, Os, Release};
use crate::{Error, Result};
use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Previous, Receipt, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub release: Release,
    pub asset: Asset,
    pub path: PathBuf,
}
impl Package {
    /// Validate retained release metadata, SHA-256 and size on a worker thread.
    pub fn verify(&self, paths: &Paths, id: &str) -> Result<()> {
        paths.guard(&self.path)?;
        let release = Release::parse(&serde_json::to_vec(&self.release)?)?;
        release.compatible(id, self.release.channel)?;
        if release.select(Os::current(), release::Arch::current()?)? != self.asset {
            return Err(Error::new(
                "manifest",
                "Retained installer metadata does not match its release.",
            ));
        }
        install_args(&self.asset, self.release.channel)?;
        release::verify(&self.path, &self.asset)
    }
}

/// Retain a verified package in the normal cache, on a worker thread. Only a
/// checksum-addressed, schema-checked package can become a rollback installer.
pub fn retain(
    paths: &Paths,
    checked: &crate::source::CheckedRelease,
    file: &Path,
) -> Result<Package> {
    release::verify(file, &checked.asset)?;
    install_args(&checked.asset, checked.release.channel)?;
    let extension = match checked.asset.kind {
        Kind::Appimage => "AppImage",
        Kind::Nsis | Kind::Inno => "exe",
        Kind::Dmg => "dmg",
        Kind::Tarball => "tar.gz",
        _ => return Err(Error::new("unavailable", "Unsupported installer.")),
    };
    let dir = paths
        .cache
        .join("arcade/tools/installers")
        .join(&checked.release.id);
    let path = dir.join(format!("{}.{}", checked.asset.sha256, extension));
    paths.guard(&path)?;
    fs::create_dir_all(&dir)?;
    if !path.exists() || release::verify(&path, &checked.asset).is_err() {
        let mut temporary = tempfile::NamedTempFile::new_in(&dir)?;
        std::io::copy(&mut fs::File::open(file)?, &mut temporary)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).map_err(|e| e.error)?;
    }
    let package = Package {
        release: checked.release.clone(),
        asset: checked.asset.clone(),
        path,
    };
    package.verify(paths, &checked.release.id)?;
    Ok(package)
}

/// Pure argument policy, safe on any thread. A present installArgs opts into the
/// shared self-install contract. Unsafe/destructive flags never reach a process.
pub fn install_args(asset: &Asset, channel: Channel) -> Result<Option<Vec<String>>> {
    let Some(args) = &asset.install_args else {
        return Ok(None);
    };
    if !matches!(asset.kind, Kind::Appimage | Kind::Dmg)
        || args.iter().map(String::as_str).collect::<Vec<_>>() != ["--install", "--silent"]
    {
        return Err(Error::new(
            "manifest",
            "installArgs must be --install --silent for an AppImage or macOS bundle.",
        ));
    }
    Ok(Some(vec![
        "--install".into(),
        "--silent".into(),
        "--channel".into(),
        channel.as_str().into(),
        "--no-launch".into(),
    ]))
}

/// Run the app's own installer from a verified disposable copy on a worker.
/// Self installers may remove their source; retained packages and adopted files
/// must survive. --no-launch leaves relaunch/receipt ownership to the manager.
/// Input bytes must already match the release hash, or the explicitly adopted
/// current executable. This rechecks the staged copy before executing it.
pub fn run_app_installer(paths: &Paths, executable: &Path, args: Vec<String>) -> Result<()> {
    if !args.iter().any(|arg| arg == "--no-launch") {
        return Err(Error::new(
            "installer",
            "Managed installation requires --no-launch.",
        ));
    }
    // A macOS executable must remain inside its copied bundle for resources.
    if Os::current() == Os::Macos {
        return CommandSpec {
            program: executable.into(),
            args,
        }
        .run(paths);
    }
    let sha256 = release::checksum(executable)?;
    let size = fs::metadata(executable)?.len();
    let directory = paths.downloads();
    paths.guard(&directory)?;
    fs::create_dir_all(&directory)?;
    let mut staged = tempfile::Builder::new()
        .prefix("install-")
        .suffix(".AppImage")
        .tempfile_in(&directory)?;
    std::io::copy(&mut fs::File::open(executable)?, &mut staged)?;
    staged.as_file().sync_all()?;
    if fs::metadata(staged.path())?.len() != size || release::checksum(staged.path())? != sha256 {
        return Err(Error::new(
            "checksum",
            "The staged installer differs from its verified source.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))?;
    }
    // Close the writer before exec (Linux rejects executing a writable open
    // file), while retaining deletion ownership if the app doesn't remove it.
    let staged = staged.into_temp_path();
    CommandSpec {
        program: staged.to_path_buf(),
        args,
    }
    .run(paths)
}

/// Resolve a receipt's launcher in memory, safe on any thread. A macOS receipt
/// names its bundle; the compiled contract supplies CFBundleExecutable.
pub fn executable(paths: &Paths, receipt: &Receipt) -> PathBuf {
    if receipt.method == InstallMethod::MacosBundle {
        let expected = paths.executable(&receipt.id, Os::Macos, false);
        let root = paths.install_root(&receipt.id, Os::Macos, false);
        receipt.path.join(expected.strip_prefix(root).unwrap())
    } else {
        receipt.path.clone()
    }
}

/// Validate an app-written receipt against this operation, on a worker. The
/// app owns integration, but may not redirect Tools outside per-user roots.
pub fn validate_receipt(
    paths: &Paths,
    receipt: &Receipt,
    id: &str,
    version: &str,
    channel: Channel,
) -> Result<()> {
    receipt.validate()?;
    if receipt.id != id
        || !release::versions_match(&receipt.version, version, channel)
        || receipt.channel != channel.as_str()
        || receipt.method == InstallMethod::Dev
    {
        return Err(Error::new(
            "receipt",
            "The installer receipt does not match this app/version/channel.",
        ));
    }
    paths.guard(&receipt.path)?;
    owned_paths(paths, receipt)?;
    if !executable(paths, receipt).is_file() {
        return Err(Error::new(
            "installer",
            "The installer receipt points at a missing executable.",
        ));
    }
    Ok(())
}

/// Write Tools ownership atomically through Link on a worker. Keeps the app's
/// integration and original installedAt, and updates the one-step previous.
pub fn claim(paths: &Paths, mut receipt: Receipt, previous: Option<Previous>) -> Result<Receipt> {
    paths.guard_receipt(&receipt.id)?;
    paths.guard(&receipt.path)?;
    owned_paths(paths, &receipt)?;
    if let Some(previous) = &previous {
        paths.guard(&previous.path)?;
    }
    receipt.managed_by = ManagedBy::Tools;
    receipt.previous = previous;
    receipt.updated_at = arcade_link::manifest::now_rfc3339().max(receipt.installed_at.clone());
    write_receipt(paths, &receipt)?;
    Ok(receipt)
}

/// Preserve fields introduced by installers newer than the pinned Link crate.
pub fn write_receipt(paths: &Paths, receipt: &Receipt) -> Result<()> {
    receipt.validate_native_paths()?;
    if receipt.method == InstallMethod::Dev {
        return Err(Error::new(
            "dev_build",
            "Development builds never write install receipts.",
        ));
    }
    let file = paths.guard_receipt(&receipt.id)?;
    let mut document = match fs::read(&file) {
        Ok(bytes) => {
            Store::new(&paths.link).read(&receipt.id)?;
            serde_json::from_slice(&bytes)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::Value::Null,
        Err(e) => return Err(e.into()),
    };
    fn merge(target: &mut serde_json::Value, value: serde_json::Value) {
        if let (Some(target), Some(value)) = (target.as_object_mut(), value.as_object()) {
            for (key, value) in value {
                merge(
                    target.entry(key.clone()).or_insert(serde_json::Value::Null),
                    value.clone(),
                );
            }
        } else {
            *target = value;
        }
    }
    merge(&mut document, serde_json::to_value(receipt)?);
    arcade_link::paths::ensure_private_dir(file.parent().unwrap())?;
    arcade_link::paths::write_atomic(&file, &serde_json::to_vec_pretty(&document)?, true)?;
    Ok(())
}

/// Record a successful login toggle without changing receipt ownership.
pub fn record_autostart(paths: &Paths, id: &str, path: Option<PathBuf>) -> Result<()> {
    paths.guard_receipt(id)?;
    if let Some(mut receipt) = Store::new(&paths.link).read(id)? {
        receipt.integration.autostart = path;
        owned_paths(paths, &receipt)?;
        receipt.updated_at = arcade_link::manifest::now_rfc3339().max(receipt.installed_at.clone());
        write_receipt(paths, &receipt)?;
    }
    Ok(())
}

/// A trashed bundle leaves only receipt-owned generated integration behind.
pub fn cleanup_missing_bundle(paths: &Paths, receipt: &Receipt) -> Result<()> {
    if receipt.method != InstallMethod::MacosBundle || receipt.path.exists() {
        return Ok(());
    }
    paths.guard(&receipt.path)?;
    for path in owned_paths(paths, receipt)? {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = path.parent().unwrap().join(fs::read_link(&path)?);
                let mut resolved = PathBuf::new();
                for component in target.components() {
                    match component {
                        std::path::Component::ParentDir => {
                            resolved.pop();
                        }
                        std::path::Component::CurDir => {}
                        other => resolved.push(other.as_os_str()),
                    }
                }
                if resolved.starts_with(&receipt.path) {
                    fs::remove_file(path)?;
                }
            }
            Ok(metadata)
                if metadata.is_file() && receipt.integration.cli.as_ref() != Some(&path) =>
            {
                fs::remove_file(path)?;
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    Ok(())
}

/// Transition-only receipt for a release with no self-install contract. Disk
/// inspection on a worker; no integration files are created or rewritten.
pub fn legacy_receipt(paths: &Paths, record: &Record) -> Receipt {
    let os = Os::current();
    let exe = record
        .executable
        .clone()
        .unwrap_or_else(|| paths.executable(&record.id, os, record.kind == Kind::Tarball));
    let root = paths.install_root(&record.id, os, record.kind == Kind::Tarball);
    let now = arcade_link::manifest::now_rfc3339();
    let autostart = paths.autostart(&record.id, os);
    let uninstaller = platform::windows_uninstall(record.kind, &root)
        .ok()
        .map(|p| p.program)
        .filter(|p| p.is_file());
    Receipt {
        schema: 1,
        id: record.id.clone(),
        version: record.version.clone(),
        channel: record.channel.as_str().into(),
        method: match record.kind {
            Kind::Appimage => InstallMethod::Appimage,
            Kind::Nsis | Kind::Inno => InstallMethod::WindowsInstaller,
            Kind::Dmg => InstallMethod::MacosBundle,
            _ => InstallMethod::Tarball,
        },
        managed_by: ManagedBy::Tools,
        path: if record.kind == Kind::Dmg { root } else { exe },
        integration: Integration {
            autostart: autostart.is_file().then_some(autostart),
            uninstaller,
            ..Integration::default()
        },
        previous: None,
        installed_at: now.clone(),
        updated_at: now,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Saved {
    Missing,
    Object { relative: PathBuf },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedEntry {
    path: PathBuf,
    saved: Saved,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PreviousInstallation {
    pub schema: u32,
    pub record: Record,
    pub receipt: Option<Receipt>,
    pub directory: PathBuf,
    entries: Vec<SavedEntry>,
}
impl PreviousInstallation {
    pub fn read(paths: &Paths, id: &str) -> Result<Option<Self>> {
        let file = paths.state().join(format!("{id}.previous.json"));
        let bytes = match crate::catalog::read_bounded(&file, 1024 * 1024) {
            Ok(b) => b,
            Err(e) if !file.exists() && e.code == "io" => return Ok(None),
            Err(e) => return Err(e),
        };
        let previous: Self = serde_json::from_slice(&bytes)?;
        if previous.schema != 1 || previous.record.id != id {
            return Err(Error::new(
                "schema",
                "Invalid previous installation schema or app ID.",
            ));
        }
        paths.guard(&previous.directory)?;
        if !previous.directory.starts_with(snapshot_root(paths)) {
            return Err(Error::new(
                "paths",
                "Previous installation escaped its store.",
            ));
        }
        for entry in &previous.entries {
            guard_entry(paths, &entry.path)?;
            if let Saved::Object { relative } = &entry.saved {
                crate::catalog::relative_path(&relative.to_string_lossy())?;
            }
        }
        Ok(Some(previous))
    }
    pub fn pointer(&self, paths: &Paths) -> Option<Previous> {
        let path = if self.record.kind == Kind::Nsis || self.record.kind == Kind::Inno {
            self.record.package.as_ref()?.path.clone()
        } else {
            let target = target(paths, &self.record, self.receipt.as_ref());
            let entry = self.entries.iter().find(|entry| entry.path == target)?;
            let Saved::Object { relative } = &entry.saved else {
                return None;
            };
            let backup = self.directory.join(relative);
            if self.record.kind == Kind::Tarball {
                let exe = self
                    .record
                    .executable
                    .clone()
                    .unwrap_or_else(|| paths.executable(&self.record.id, Os::current(), true));
                backup.join(exe.strip_prefix(target).ok()?)
            } else {
                backup
            }
        };
        path.exists().then_some(Previous {
            version: self.record.version.clone(),
            path,
        })
    }
    pub fn restore(&self, paths: &Paths) -> Result<()> {
        restore_entries(paths, &self.directory, &self.entries)
    }
    pub fn protocols(&self, paths: &Paths) -> Vec<u32> {
        if let Some(package) = &self.record.package {
            return package.release.link_protocol.clone();
        }
        let registry_path = paths.link.registry.join(format!("{}.json", self.record.id));
        self.entries
            .iter()
            .find(|entry| entry.path == registry_path)
            .and_then(|entry| match &entry.saved {
                Saved::Object { relative } => {
                    crate::catalog::read_bounded(&self.directory.join(relative), 1024 * 1024).ok()
                }
                Saved::Missing => None,
            })
            .and_then(|bytes| {
                arcade_link::manifest::ManifestDocument::from_json(&String::from_utf8_lossy(&bytes))
                    .ok()
            })
            .map(|document| document.manifest.link.protocol)
            .unwrap_or_else(|| vec![1])
    }
    pub fn remove(&self, paths: &Paths) -> Result<()> {
        paths.guard(&self.directory)?;
        platform::remove(&self.directory)
    }
}

pub(crate) struct Transaction {
    directory: Option<tempfile::TempDir>,
    entries: Vec<SavedEntry>,
    paths: Paths,
    committed: bool,
}
impl Transaction {
    pub fn begin(
        paths: &Paths,
        id: &str,
        target: &Path,
        receipt: Option<&Receipt>,
    ) -> Result<Self> {
        let root = snapshot_root(paths);
        paths.guard(&root)?;
        fs::create_dir_all(&root)?;
        let directory = tempfile::Builder::new()
            .prefix(&format!("tools-{id}-"))
            .tempdir_in(&root)?;
        let mut transaction = Self {
            directory: Some(directory),
            entries: Vec::new(),
            paths: paths.clone(),
            committed: false,
        };
        let mut files = BTreeSet::from([
            target.to_path_buf(),
            paths.guard_receipt(id)?,
            paths.state().join(format!("{id}.json")),
            paths.link.registry.join(format!("{id}.json")),
        ]);
        if Os::current() == Os::Linux {
            files.insert(
                paths
                    .home
                    .join("Applications/Arcade/.previous")
                    .join(paths.executable(id, Os::Linux, false).file_name().unwrap()),
            );
        }
        files.extend(integration_candidates(paths, id));
        if let Some(receipt) = receipt {
            files.extend(owned_paths(paths, receipt)?);
            if let Some(previous) = &receipt.previous {
                files.insert(previous.path.clone());
            }
        }
        for path in files {
            transaction.include(&path)?;
        }
        Ok(transaction)
    }
    pub fn include(&mut self, path: &Path) -> Result<()> {
        if self.entries.iter().any(|entry| entry.path == path) {
            return Ok(());
        }
        let path = path.to_path_buf();
        guard_entry(&self.paths, &path)?;
        let saved = if fs::symlink_metadata(&path).is_ok() {
            let relative = PathBuf::from(format!(
                "{}-{}",
                self.entries.len(),
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            copy_object(
                &path,
                &self.directory.as_ref().unwrap().path().join(&relative),
            )?;
            Saved::Object { relative }
        } else {
            Saved::Missing
        };
        self.entries.push(SavedEntry { path, saved });
        Ok(())
    }
    /// Drop the snapshot without restoring it: for when nothing was changed.
    pub fn discard(mut self) {
        self.committed = true;
    }
    pub fn previous(&self, record: Record, receipt: Option<Receipt>) -> PreviousInstallation {
        PreviousInstallation {
            schema: 1,
            record,
            receipt,
            directory: self.directory.as_ref().unwrap().path().into(),
            entries: self.entries.clone(),
        }
    }
    pub fn commit(mut self, previous: Option<PreviousInstallation>) -> Result<()> {
        if let Some(previous) = previous {
            let file = self
                .paths
                .state()
                .join(format!("{}.previous.json", previous.record.id));
            let old = PreviousInstallation::read(&self.paths, &previous.record.id)?;
            arcade_link::paths::write_atomic(&file, &serde_json::to_vec_pretty(&previous)?, true)?;
            let _ = self.directory.take().unwrap().keep();
            self.committed = true;
            if let Some(old) = old {
                if let Err(e) = old.remove(&self.paths) {
                    eprintln!("Previous installation cleanup: {e}");
                }
            }
        } else {
            self.committed = true;
        }
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            if let Some(directory) = &self.directory {
                if let Err(e) = restore_entries(&self.paths, directory.path(), &self.entries) {
                    eprintln!("Installation rollback: {e}");
                }
            }
        }
    }
}

pub(crate) fn target(paths: &Paths, record: &Record, receipt: Option<&Receipt>) -> PathBuf {
    if record.kind == Kind::Tarball || matches!(record.kind, Kind::Nsis | Kind::Inno) {
        record
            .executable
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| {
                paths.install_root(&record.id, Os::current(), record.kind == Kind::Tarball)
            })
    } else if let Some(receipt) = receipt {
        receipt.path.clone()
    } else if record.kind == Kind::Dmg {
        paths.install_root(&record.id, Os::Macos, false)
    } else {
        record
            .executable
            .clone()
            .unwrap_or_else(|| paths.executable(&record.id, Os::current(), false))
    }
}

/// Snapshot folders no `<id>.previous.json` points at: left by a Tools that
/// was killed mid-transaction. Only call while no operation runs (startup).
pub(crate) fn remove_orphan_snapshots(paths: &Paths, ids: &[&str]) -> Result<u64> {
    let root = snapshot_root(paths);
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(0);
    };
    let mut referenced = BTreeSet::new();
    for id in ids {
        if let Ok(Some(previous)) = PreviousInstallation::read(paths, id) {
            referenced.insert(previous.directory);
        }
    }
    let mut freed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let ours = entry.file_name().to_string_lossy().starts_with("tools-");
        if ours && entry.file_type().is_ok_and(|t| t.is_dir()) && !referenced.contains(&path) {
            paths.guard(&path)?;
            freed += size_of(&path);
            platform::remove(&path)?;
        }
    }
    Ok(freed)
}
fn size_of(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::read_dir(path).map_or(0, |entries| {
            entries.flatten().map(|e| size_of(&e.path())).sum()
        }),
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

#[cfg(target_os = "linux")]
fn clone_file(source: &Path, destination: &Path) -> bool {
    use std::os::fd::AsRawFd;
    const FICLONE: libc::c_ulong = 0x4004_9409;
    let (Ok(from), Ok(to)) = (fs::File::open(source), fs::File::create(destination)) else {
        return false;
    };
    // SAFETY: both descriptors are open for the duration of the call.
    let cloned = unsafe { libc::ioctl(to.as_raw_fd(), FICLONE as _, from.as_raw_fd()) } == 0;
    if !cloned {
        let _ = fs::remove_file(destination);
    }
    cloned
}
#[cfg(not(target_os = "linux"))]
fn clone_file(_: &Path, _: &Path) -> bool {
    false
}

fn snapshot_root(paths: &Paths) -> PathBuf {
    // Link's Linux installer/uninstaller validates previous within this root.
    // Keep metadata in Tools state and receipt-addressed binary backups here.
    if Os::current() == Os::Linux {
        paths.home.join("Applications/Arcade/.previous")
    } else {
        paths.state().join("installations")
    }
}

pub(crate) fn owned_paths(paths: &Paths, receipt: &Receipt) -> Result<Vec<PathBuf>> {
    let integration = &receipt.integration;
    let mut entries = Vec::new();
    for path in integration
        .icons
        .iter()
        .chain(integration.desktop_entry.iter())
        .chain(integration.cli.iter())
        .chain(integration.autostart.iter())
        .chain(integration.uninstaller.iter())
        .chain(integration.commands.iter())
    {
        let roots = [
            paths.data.join("applications"),
            paths.data.join("icons/hicolor"),
            paths.home.join(".local/bin"),
            paths.config.join("autostart"),
            paths.home.join("Library/LaunchAgents"),
            paths.local.join("Arcade/bin"),
            paths.local.join("Programs"),
            paths.roaming.join("Microsoft/Windows/Start Menu/Programs"),
        ];
        let user_uninstaller = integration.uninstaller.as_ref() == Some(path)
            && path.starts_with(&paths.home)
            && path.parent() == receipt.path.parent()
            && path.file_name().is_some_and(|name| {
                ["uninstall.exe", "unins000.exe"]
                    .iter()
                    .any(|expected| name.to_string_lossy().eq_ignore_ascii_case(expected))
            });
        if !user_uninstaller
            && !roots
                .iter()
                .any(|root| path.starts_with(root) && path != root)
        {
            return Err(Error::new(
                "paths",
                "Receipt integration escaped Arcade's generated-file locations.",
            ));
        }
        guard_entry(paths, path)?;
        entries.push(path.clone());
    }
    Ok(entries)
}
/// Generated files an install may replace: the desktop entry, CLI link,
/// autostart entry and icons, under both the 0.3 desktop id and the
/// `dev.arcade.<App>` id apps move to (STANDARD §10), so an app that migrated
/// after this Tools was released is still covered.
fn integration_candidates(paths: &Paths, id: &str) -> Vec<PathBuf> {
    let legacy = match id {
        "arcade.box" => "dev.arcadebox.app".to_owned(),
        "arcade.wheel" => "com.arcadewheel.ArcadeWheel".to_owned(),
        "arcade.shelf" => "arcade.shelf".to_owned(),
        "arcade.clipboard" => "dev.arcade.clipboard".to_owned(),
        _ => id.replace('.', "-"),
    };
    let mut candidates = common_integration(paths, id, &legacy);
    if let Some(current) =
        arcade_link::manifest::os_identifier(id).filter(|current| *current != legacy)
    {
        for path in common_integration(paths, id, &current) {
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    candidates
}
fn common_integration(paths: &Paths, id: &str, desktop_id: &str) -> Vec<PathBuf> {
    let mut paths_out = vec![
        paths
            .data
            .join("applications")
            .join(format!("{desktop_id}.desktop")),
        paths.home.join(".local/bin").join(id.replace('.', "-")),
        paths.autostart(id, Os::current()),
        paths
            .config
            .join("autostart")
            .join(format!("{desktop_id}.desktop")),
    ];
    for size in [16, 22, 24, 32, 48, 64, 96, 128, 192, 256, 512] {
        paths_out.push(
            paths
                .data
                .join("icons/hicolor")
                .join(format!("{size}x{size}/apps/{desktop_id}.png")),
        );
    }
    paths_out.push(
        paths
            .data
            .join("icons/hicolor/scalable/apps")
            .join(format!("{desktop_id}.svg")),
    );
    paths_out
}
fn guard_entry(paths: &Paths, path: &Path) -> Result<()> {
    if path
        == Store::new(&paths.link)
            .path(
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            )
            .unwrap_or_default()
    {
        paths.guard_receipt(&path.file_stem().unwrap().to_string_lossy())?;
        return Ok(());
    }
    // A CLI symlink itself is owned; its parents must never be symlinked.
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        let parent = path
            .parent()
            .ok_or_else(|| Error::new("paths", "No integration parent."))?;
        paths.guard(&parent.join(".arcade-path-check"))
    } else {
        paths.guard(path)
    }
}
fn restore_entries(paths: &Paths, directory: &Path, entries: &[SavedEntry]) -> Result<()> {
    for entry in entries.iter().rev() {
        guard_entry(paths, &entry.path)?;
        platform::remove(&entry.path)?;
        if let Saved::Object { relative } = &entry.saved {
            fs::create_dir_all(entry.path.parent().unwrap())?;
            copy_object(&directory.join(relative), &entry.path)?;
        }
    }
    Ok(())
}
fn copy_object(source: &Path, destination: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fs::read_link(source)?, destination)?;
        }
        #[cfg(windows)]
        {
            return Err(Error::new(
                "paths",
                "Unexpected Windows installation symlink.",
            ));
        }
    } else if meta.is_dir() {
        #[cfg(target_os = "macos")]
        if source.extension().is_some_and(|e| e == "app") {
            let status = std::process::Command::new("ditto")
                .arg(source)
                .arg(destination)
                .status()?;
            if !status.success() {
                return Err(Error::new("installer", "Bundle snapshot failed."));
            }
            return Ok(());
        }
        fs::create_dir_all(destination)?;
        for child in fs::read_dir(source)? {
            let child = child?;
            copy_object(&child.path(), &destination.join(child.file_name()))?;
        }
        fs::set_permissions(destination, meta.permissions())?;
    } else if meta.is_file() {
        // A snapshot must keep its own bytes: an older installer may write
        // the executable in place. A copy-on-write clone (btrfs, XFS) keeps
        // them without a second full AppImage on disk; elsewhere, a copy.
        if !clone_file(source, destination) {
            fs::copy(source, destination)?;
        }
        fs::set_permissions(destination, meta.permissions())?;
    } else {
        return Err(Error::new("paths", "Installation contains a special file."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_candidates_cover_the_old_and_new_desktop_ids() {
        let paths = Paths::under(Path::new("/isolated"));
        let candidates = integration_candidates(&paths, "arcade.box");
        let entry = |name: &str| {
            paths
                .data
                .join("applications")
                .join(format!("{name}.desktop"))
        };
        assert!(candidates.contains(&entry("dev.arcadebox.app")));
        assert!(candidates.contains(&entry("dev.arcade.Box")));
        let icon = paths
            .data
            .join("icons/hicolor/scalable/apps/dev.arcade.Box.svg");
        assert!(candidates.contains(&icon));
        let mut unique = candidates.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), candidates.len(), "no path is listed twice");
    }
}
