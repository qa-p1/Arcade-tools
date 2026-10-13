//! Start at login for Windows apps installed by their own installer. The
//! shared Inno/NSIS "Start at login" task creates `Startup\<Name>.lnk` and
//! records it in the receipt, so the receipt decides; the Run value is only
//! the fallback for copies without a receipt. The policy is plain filesystem
//! work and runs in tests on every OS; only creating the shortcut is
//! Windows-only. All of it runs on a worker thread.
use crate::apps::app_name;
use crate::paths::Paths;
use crate::{Error, Result};
use arcade_link::receipt::{InstallMethod, Receipt, Store};
use std::fs;
use std::path::{Path, PathBuf};

/// The shortcut the shared installers create when the receipt records none.
pub fn startup_shortcut(paths: &Paths, id: &str) -> PathBuf {
    paths
        .roaming
        .join("Microsoft/Windows/Start Menu/Programs/Startup")
        .join(format!("{}.lnk", app_name(id)))
}

fn installed(paths: &Paths, id: &str) -> std::io::Result<Option<Receipt>> {
    Ok(Store::new(&paths.link)
        .read(id)?
        .filter(|r| r.method == InstallMethod::WindowsInstaller))
}

/// `Some(enabled)` when a Windows installer receipt owns the login entry,
/// `None` when there is no such receipt and the Run value decides.
pub fn state(paths: &Paths, id: &str) -> Option<bool> {
    match installed(paths, id) {
        Ok(Some(receipt)) => Some(
            receipt
                .integration
                .autostart
                .is_some_and(|p| p.is_absolute() && p.is_file()),
        ),
        Ok(None) => None,
        // A damaged receipt must not silently select the Run value.
        Err(_) => Some(false),
    }
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Create or remove the receipt's Startup shortcut and record the change.
/// Callers remove the Run value first, so the two never both start the app.
/// Shortcut creation is injected so tests never touch a real Startup folder.
pub fn set(
    paths: &Paths,
    id: &str,
    on: bool,
    write_shortcut: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<()> {
    let receipt = installed(paths, id)?
        .ok_or_else(|| Error::new("autostart", "This app has no Windows installer receipt."))?;
    receipt.validate_native_paths()?;
    let recorded = receipt.integration.autostart.clone();
    let path = recorded
        .clone()
        .unwrap_or_else(|| startup_shortcut(paths, id));
    paths.guard(&path)?;
    if on && recorded.is_some() && path.is_file() || !on && recorded.is_none() {
        return Ok(());
    }
    let file = paths.guard_receipt(id)?;
    // Edit the original JSON: installers record extra fields (such as
    // desktopShortcut) that must survive a toggle.
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
    let backup = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let change = || -> Result<()> {
        if on {
            write_shortcut(&path, &receipt.path)?;
        } else {
            remove_file(&path)?;
        }
        document["integration"]["autostart"] = if on {
            serde_json::to_value(&path)?
        } else {
            serde_json::Value::Null
        };
        document["updatedAt"] = arcade_link::manifest::now_rfc3339().into();
        let bytes = serde_json::to_vec_pretty(&document)?;
        Receipt::from_json(
            std::str::from_utf8(&bytes).map_err(|e| Error::new("receipt", e.to_string()))?,
        )?
        .validate_native_paths()?;
        arcade_link::paths::write_atomic(&file, &bytes, true)?;
        Ok(())
    };
    if let Err(error) = change() {
        let restore = match backup {
            Some(bytes) => arcade_link::paths::write_atomic(&path, &bytes, false),
            None => remove_file(&path),
        };
        if let Err(restore) = restore {
            return Err(Error::new(
                "autostart",
                format!("{error}; restoring the login shortcut failed: {restore}"),
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn shortcut_script(path: &Path, executable: &Path) -> String {
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    format!(
        "$ErrorActionPreference='Stop'; $s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.Arguments='--background'; $s.WorkingDirectory={}; $s.Save()",
        quote(path),
        quote(executable),
        quote(executable.parent().unwrap_or(executable)),
    )
}

/// Windows Script Host shortcut, the same `--background` start the shared
/// installers create. Runs Windows PowerShell 5.1 without a console window.
#[cfg(windows)]
pub fn write_shortcut(path: &Path, executable: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &shortcut_script(path, executable),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::new(
            "autostart",
            format!(
                "Could not create the Windows login shortcut: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
}

/// Shortcut creation only exists on Windows; elsewhere the script is unused.
#[cfg(not(windows))]
pub fn write_shortcut(path: &Path, executable: &Path) -> Result<()> {
    let _ = shortcut_script(path, executable);
    Err(Error::new(
        "unavailable",
        "Windows login shortcuts are created only on Windows.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::manifest::ids;
    use arcade_link::receipt::{Integration, ManagedBy};

    fn fixture() -> (tempfile::TempDir, Paths, Receipt) {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let receipt = Receipt {
            schema: 1,
            id: ids::LENS.into(),
            version: "0.2.0".into(),
            channel: "stable".into(),
            method: InstallMethod::WindowsInstaller,
            managed_by: ManagedBy::Tools,
            path: paths.local.join("Programs/Arcade Lens/arcade-lens.exe"),
            integration: Integration::default(),
            previous: None,
            installed_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        (root, paths, receipt)
    }

    fn write(path: &Path, executable: &Path) -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, executable.to_string_lossy().as_bytes())?;
        Ok(())
    }

    #[test]
    fn receipt_decides_and_other_methods_fall_back_to_the_run_value() {
        let (_root, paths, mut receipt) = fixture();
        let store = Store::new(&paths.link);
        assert_eq!(state(&paths, ids::LENS), None);
        store.write(&receipt).unwrap();
        assert_eq!(state(&paths, ids::LENS), Some(false));
        let shortcut = startup_shortcut(&paths, ids::LENS);
        receipt.integration.autostart = Some(shortcut.clone());
        store.write(&receipt).unwrap();
        assert_eq!(state(&paths, ids::LENS), Some(false), "missing shortcut");
        write(&shortcut, &receipt.path).unwrap();
        assert_eq!(state(&paths, ids::LENS), Some(true));
        receipt.method = InstallMethod::Appimage;
        store.write(&receipt).unwrap();
        assert_eq!(state(&paths, ids::LENS), None);
        fs::write(store.path(ids::LENS).unwrap(), b"not a receipt").unwrap();
        assert_eq!(state(&paths, ids::LENS), Some(false));
        assert!(set(&paths, ids::LENS, true, write).is_err());
    }

    #[test]
    fn toggle_keeps_receipt_fields_and_shortcut_in_sync() {
        let (_root, paths, receipt) = fixture();
        let store = Store::new(&paths.link);
        store.write(&receipt).unwrap();
        let mut document = serde_json::to_value(&receipt).unwrap();
        document["integration"]["desktopShortcut"] = paths
            .home
            .join("Desktop/Arcade Lens.lnk")
            .to_string_lossy()
            .as_ref()
            .into();
        fs::write(
            store.path(ids::LENS).unwrap(),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        let shortcut = startup_shortcut(&paths, ids::LENS);
        set(&paths, ids::LENS, true, write).unwrap();
        assert_eq!(
            fs::read_to_string(&shortcut).unwrap(),
            receipt.path.to_string_lossy()
        );
        let saved = store.read(ids::LENS).unwrap().unwrap();
        assert_eq!(saved.integration.autostart, Some(shortcut.clone()));
        assert_eq!(saved.installed_at, receipt.installed_at);
        assert!(saved.updated_at > receipt.updated_at);
        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path(ids::LENS).unwrap()).unwrap()).unwrap();
        assert_eq!(
            raw["integration"]["desktopShortcut"],
            document["integration"]["desktopShortcut"]
        );
        assert_eq!(state(&paths, ids::LENS), Some(true));
        // Already on: no rewrite.
        set(&paths, ids::LENS, true, |_, _| panic!("unchanged")).unwrap();
        set(&paths, ids::LENS, false, write).unwrap();
        assert!(!shortcut.exists());
        assert_eq!(
            store
                .read(ids::LENS)
                .unwrap()
                .unwrap()
                .integration
                .autostart,
            None
        );
        assert_eq!(state(&paths, ids::LENS), Some(false));
        set(&paths, ids::LENS, false, |_, _| panic!("unchanged")).unwrap();
    }

    #[test]
    fn recorded_shortcut_wins_and_failures_restore_everything() {
        let (_root, paths, mut receipt) = fixture();
        let store = Store::new(&paths.link);
        let recorded = paths.roaming.join("Redirected/Lens.lnk");
        receipt.integration.autostart = Some(recorded.clone());
        store.write(&receipt).unwrap();
        let failed = set(&paths, ids::LENS, true, |path, _| {
            fs::write(path, b"partial")?;
            Err(Error::new("autostart", "shortcut creation failed"))
        });
        assert!(failed.is_err());
        assert!(!recorded.exists());
        assert_eq!(store.read(ids::LENS).unwrap(), Some(receipt.clone()));
        set(&paths, ids::LENS, true, write).unwrap();
        assert!(recorded.is_file());
        assert!(!startup_shortcut(&paths, ids::LENS).exists());
    }

    #[test]
    fn missing_receipt_is_refused_and_script_quotes_literally() {
        let (_root, paths, _) = fixture();
        assert!(set(&paths, ids::LENS, true, write).is_err());
        let script = shortcut_script(
            Path::new("C:/User's Startup/Lens.lnk"),
            Path::new("C:/User's Apps/arcade-lens.exe"),
        );
        assert!(script.contains("CreateShortcut('C:/User''s Startup/Lens.lnk')"));
        assert!(script.contains("$s.TargetPath='C:/User''s Apps/arcade-lens.exe'"));
        assert!(script.contains("$s.WorkingDirectory='C:/User''s Apps'"));
        assert!(script.contains("$s.Arguments='--background'"));
    }
}
