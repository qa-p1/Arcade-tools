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
                let executable = receipt.path.to_string_lossy();
                if owned
                    && !text
                        .lines()
                        .any(|line| line.starts_with("Exec=") && line.contains(executable.as_ref()))
                {
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
fn run_repair(paths: &Paths, executable: &Path) -> Result<()> {
    let mut output = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    crate::platform::child_environment(&mut command, paths);
    let mut child = command
        .arg("--repair")
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
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
        use std::io::{Read, Seek, SeekFrom};
        output.seek(SeekFrom::Start(0))?;
        let mut message = String::new();
        output.take(4096).read_to_string(&mut message)?;
        return Err(Error::new(
            "repair",
            format!("Local repair failed ({status}): {message}"),
        ));
    }
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
        if !attempted.insert(signature) {
            continue;
        }
        let result = repair(paths, &receipt, expected_hash.as_deref(), &broken);
        match result {
            Ok(()) => report.repaired.push(receipt.id),
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
    if receipt.method == InstallMethod::WindowsInstaller {
        return Err(Error::new(
            "native_installer",
            "This Windows integration needs its verified setup program.",
        ));
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
    let result = run_repair(paths, &executable);
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
