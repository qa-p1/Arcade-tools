//! Offline integration repair. It never installs an update or restarts an app.
use crate::{catalog::read_bounded, installation, paths::Paths, release, Error, Result};
use arcade_link::receipt::{InstallMethod, Receipt, Store};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub repaired: Vec<String>,
    pub pending: Vec<(String, String)>,
}

fn launcher_targets(paths: &Paths, receipt: &Receipt, text: &str) -> bool {
    let executable = installation::executable(paths, receipt);
    let generated = crate::platform::desktop_entry(&receipt.id, &executable).ok();
    let quoted = generated.as_deref().and_then(|text| {
        text.lines().find_map(|line| {
            line.strip_prefix("Exec=")
                .map(|value| value.trim_end_matches(" --background"))
        })
    });
    let raw = executable.to_string_lossy();
    let mut main = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
        }
        if main {
            if let Some(value) = line.strip_prefix("Exec=") {
                return quoted.is_some_and(|token| value.contains(token))
                    || value == raw
                    || value.starts_with(&format!("{raw} "));
            }
        }
    }
    false
}

/// Only receipt-owned paths are eligible. A missing login entry is an off
/// choice, not damage; an existing disabled entry must stay disabled.
pub(crate) fn drift(paths: &Paths, receipt: &Receipt) -> Vec<PathBuf> {
    let mut broken = Vec::new();
    if let Some(path) = &receipt.integration.desktop_entry {
        if !path.is_file() {
            broken.push(path.clone());
        } else if path.extension().is_some_and(|ext| ext == "desktop") {
            if let Ok(bytes) = read_bounded(path, 128 * 1024) {
                let text = String::from_utf8_lossy(&bytes);
                let owned = text
                    .lines()
                    .any(|line| line == format!("X-Arcade-Id={}", receipt.id));
                if owned && !launcher_targets(paths, receipt, &text) {
                    broken.push(path.clone());
                }
            }
        }
    }
    if let Some(path) = &receipt.integration.cli {
        if !path.exists() {
            broken.push(path.clone());
        }
    }
    let registry = paths.link.registry.join(format!("{}.json", receipt.id));
    let registered = read_bounded(&registry, 1024 * 1024)
        .ok()
        .and_then(|bytes| {
            arcade_link::manifest::ManifestDocument::from_json(&String::from_utf8_lossy(&bytes))
                .ok()
        })
        .is_some_and(|doc| {
            doc.manifest.id == receipt.id
                && Path::new(&doc.manifest.executable) == installation::executable(paths, receipt)
        });
    if !registered {
        broken.push(registry);
    }
    broken
}

/// Bounded local child: a broken repair command cannot wedge the maintenance
/// worker indefinitely. Output goes to a disposable file, never a full pipe.
fn run_repair(paths: &Paths, executable: &Path, limit: Duration) -> Result<()> {
    run_local(paths, executable, "--repair", limit).map(|_| ())
}

fn run_local(paths: &Paths, executable: &Path, argument: &str, limit: Duration) -> Result<Vec<u8>> {
    run_command(paths, executable, &[argument.as_ref()], limit)
}

fn run_command(
    paths: &Paths,
    executable: &Path,
    arguments: &[&std::ffi::OsStr],
    limit: Duration,
) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut output = tempfile::tempfile()?;
    let mut errors = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    crate::platform::child_environment(&mut command, paths);
    let mut child = command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(errors.try_clone()?)
        .spawn()?;
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::new(
                "timeout",
                "The app's integration repair timed out.",
            ));
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    if !status.success() {
        errors.seek(SeekFrom::Start(0))?;
        let mut message = String::new();
        errors.take(4096).read_to_string(&mut message)?;
        return Err(Error::new(
            "repair",
            format!("Local repair failed ({status}): {message}"),
        ));
    }
    output.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    output.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::new(
            "limit",
            "The app's repair response is too large.",
        ));
    }
    Ok(bytes)
}

/// Refresh desktop discovery at startup and after launcher changes. Optional
/// system utilities stay bounded and never change the user's startup policy.
pub fn refresh_desktop_caches(paths: &Paths) {
    #[cfg(target_os = "linux")]
    {
        let applications = paths.data.join("applications");
        if paths.guard(&applications).is_ok() && applications.is_dir() {
            let _ = run_command(
                paths,
                Path::new("update-desktop-database"),
                &[applications.as_os_str()],
                Duration::from_secs(20),
            );
        }
        let icons = paths.data.join("icons/hicolor");
        if paths.guard(&icons).is_ok() && icons.join("index.theme").is_file() {
            let _ = run_command(
                paths,
                Path::new("gtk-update-icon-cache"),
                &["-t".as_ref(), icons.as_os_str()],
                Duration::from_secs(20),
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = paths;
}

fn restore_manifest(
    paths: &Paths,
    receipt: &Receipt,
    executable: &Path,
    backup: &Path,
) -> Result<()> {
    let file = paths.link.manifest(&receipt.id);
    paths.guard(&file)?;
    if file.is_file() {
        let bytes = read_bounded(&file, 1024 * 1024)?;
        arcade_link::paths::write_atomic(&backup.join("registry.json"), &bytes, true)?;
    }
    // The app is the authority for its actions and Link-off preference. Ask its
    // read-only CLI, rather than inventing metadata or starting its GUI.
    let bytes = run_local(
        paths,
        executable,
        "--arcade-manifest",
        Duration::from_secs(20),
    )?;
    let text =
        std::str::from_utf8(&bytes).map_err(|error| Error::new("manifest", error.to_string()))?;
    let doc = arcade_link::manifest::ManifestDocument::from_json(text)
        .map_err(|error| Error::new("manifest", error))?;
    if doc.manifest.id != receipt.id
        || Path::new(&doc.manifest.executable) != executable
        || !release::versions_match(
            &doc.manifest.version,
            &receipt.version,
            release::Channel::Stable,
        )
    {
        return Err(Error::new(
            "manifest",
            "The installed app returned metadata for a different installation.",
        ));
    }
    arcade_link::paths::ensure_private_dir(&paths.link.registry)?;
    arcade_link::paths::write_atomic(&file, &bytes, true)?;
    Ok(())
}

/// Caller holds the installation mutation lock. Attempt each unchanged fault
/// once per process, avoiding loops when a repair emits its own file events.
pub(crate) fn maintain(
    paths: &Paths,
    entries: Vec<(Receipt, Option<String>)>,
    attempted: &mut HashSet<String>,
) -> Report {
    let mut report = Report::default();
    for (receipt, expected_hash) in entries {
        let broken = drift(paths, &receipt);
        if broken.is_empty() {
            attempted.retain(|key| !key.starts_with(&format!("{}:", receipt.id)));
            continue;
        }
        let signature = format!(
            "{}:{}:{}:{broken:?}",
            receipt.id,
            receipt.version,
            receipt.path.display()
        );
        if !attempted.insert(signature.clone()) {
            continue;
        }
        let result = repair(paths, &receipt, expected_hash.as_deref(), &broken);
        match result {
            Ok(()) => {
                // Successful repairs must remain repeatable even when the OS
                // coalesces our own writes with the next external deletion.
                attempted.remove(&signature);
                report.repaired.push(receipt.id);
            }
            Err(error) => report.pending.push((receipt.id, error.to_string())),
        }
    }
    report
}

fn repair(
    paths: &Paths,
    receipt: &Receipt,
    expected_hash: Option<&str>,
    broken: &[PathBuf],
) -> Result<()> {
    let executable = installation::executable(paths, receipt);
    if receipt.method == InstallMethod::Dev
        || arcade_link::install::is_dev_build(&receipt.path)
        || crate::health::transient(paths, &receipt.path)
        || !executable.is_file()
    {
        return Err(Error::new(
            "unavailable",
            "Automatic repair needs a permanent installed app; its files were kept.",
        ));
    }
    paths.guard(&executable)?;
    installation::owned_paths(paths, receipt)?;
    if let Some(expected) = expected_hash {
        if release::checksum(&executable)? != expected {
            return Err(Error::new(
                "checksum",
                "The installed executable changed; automatic repair was not run.",
            ));
        }
    }
    // Back up every file the app's --repair can touch before changing a login
    // entry. Healthy/customized entries are restored byte-for-byte afterward.
    let backup = paths
        .state()
        .join("maintenance/backups")
        .join(&receipt.id)
        .join(arcade_link::endpoint::new_token()?);
    paths.guard(&backup)?;
    arcade_link::paths::ensure_private_dir(&backup)?;
    let registry = paths.link.manifest(&receipt.id);
    if broken.iter().all(|path| *path == registry) {
        return restore_manifest(paths, receipt, &executable, &backup);
    }
    if receipt.method == InstallMethod::WindowsInstaller {
        return Err(Error::new(
            "native_installer",
            "This Windows integration needs its verified setup program.",
        ));
    }
    let mut preserved = Vec::new();
    for (index, path) in receipt
        .integration
        .desktop_entry
        .iter()
        .chain(receipt.integration.autostart.iter())
        .enumerate()
    {
        if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(Error::new(
                "ownership",
                "A desktop or login entry is a symbolic link; it was kept.",
            ));
        }
        if path.is_file() {
            let bytes = read_bounded(path, 128 * 1024)?;
            if path.extension().is_some_and(|ext| ext == "desktop") {
                let text = String::from_utf8_lossy(&bytes);
                if !text
                    .lines()
                    .any(|line| line == format!("X-Arcade-Id={}", receipt.id))
                {
                    return Err(Error::new(
                        "ownership",
                        "An unmarked desktop or login entry was kept for review.",
                    ));
                }
            }
            arcade_link::paths::write_atomic(&backup.join(format!("entry-{index}")), &bytes, true)?;
            if !broken.contains(path) {
                preserved.push((path.clone(), bytes));
            }
        }
    }
    let receipt_path = Store::new(&paths.link).path(&receipt.id)?;
    let original = read_bounded(&receipt_path, 1024 * 1024)?;
    arcade_link::paths::write_atomic(&backup.join("receipt.json"), &original, true)?;
    let mut policy = receipt.clone();
    if policy
        .integration
        .autostart
        .as_ref()
        .is_some_and(|path| !path.exists())
    {
        policy.integration.autostart = None;
        installation::write_receipt(paths, &policy)?;
    }
    let result = run_repair(paths, &executable, Duration::from_secs(20));
    // Keep customized launchers and disabled login choices even if an older
    // app's repair implementation rewrites its whole integration document.
    for (path, bytes) in preserved {
        if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            arcade_link::paths::write_atomic(&path, &bytes, true)?;
        }
    }
    if let Err(error) = result {
        arcade_link::paths::write_atomic(&receipt_path, &original, true)?;
        return Err(error);
    }
    let updated = Store::new(&paths.link)
        .read(&receipt.id)?
        .ok_or_else(|| Error::new("receipt", "Repair removed its installation receipt."))?;
    if updated.path != receipt.path
        || updated.version != receipt.version
        || updated.method != receipt.method
        || updated.managed_by != receipt.managed_by
    {
        arcade_link::paths::write_atomic(&receipt_path, &original, true)?;
        return Err(Error::new(
            "receipt",
            "Repair changed the installation contract; the original receipt was restored.",
        ));
    }
    // Older app repair code may know fewer fields than the current receipt.
    arcade_link::paths::write_atomic(&receipt_path, &original, true)?;
    installation::write_receipt(paths, &updated)?;
    if drift(paths, receipt).contains(&registry) {
        restore_manifest(paths, receipt, &executable, &backup)?;
    }
    // Recheck the original contract, not a new receipt that dropped a conflict.
    if !drift(paths, receipt).is_empty() {
        arcade_link::paths::write_atomic(&receipt_path, &original, true)?;
        return Err(Error::new(
            "repair",
            "Integration is still incomplete after the app's repair; the backup was kept.",
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_stalled_repair_is_terminated_instead_of_wedging_the_worker() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("repair");
        fs::write(
            &program,
            "#!/usr/bin/env python3\nimport time\ntime.sleep(60)\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let error = run_repair(
            &Paths::under(root.path()),
            &program,
            Duration::from_millis(75),
        )
        .unwrap_err();
        assert_eq!(error.code, "timeout");
    }
}
