//! Tools-owned login entries. All reads and writes run on a worker thread.
use arcade_link::manifest::ids;
use arcade_tools_core::{paths::Paths, platform, release::Os, Error, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MARKER: &str = "Arcade Tools managed login entry";

fn path(paths: &Paths, os: Os) -> PathBuf {
    let path = paths.autostart(ids::TOOLS, os);
    if os == Os::Windows {
        path.with_extension("vbs")
    } else {
        path
    }
}

fn entry(executable: &Path, os: Os) -> Result<String> {
    let executable_text = executable.to_string_lossy();
    if executable_text.chars().any(char::is_control) {
        return Err(Error::new(
            "autostart",
            "Executable path contains a control character.",
        ));
    }
    Ok(match os {
        Os::Linux => format!(
            "# {MARKER}\n{}",
            platform::desktop_entry(ids::TOOLS, executable)?
        ),
        Os::Macos => platform::launch_agent(ids::TOOLS, executable)
            .replace("<plist", &format!("<!-- {MARKER} -->\n<plist")),
        Os::Windows => {
            let escaped = executable_text.replace('"', "\"\"");
            format!("' {MARKER}\r\nSet shell = CreateObject(\"WScript.Shell\")\r\nshell.Run Chr(34) & \"{escaped}\" & Chr(34) & \" --background\", 0, False\r\n")
        }
    })
}

fn validate(path: &Path, expected: &str, os: Os) -> Result<()> {
    if fs::read_to_string(path)? != expected {
        return Err(Error::new(
            "autostart",
            "Could not verify the saved login entry.",
        ));
    }
    if os == Os::current() {
        let command = match os {
            Os::Linux => Some(("desktop-file-validate", vec![path.as_os_str()])),
            Os::Macos => Some((
                "plutil",
                vec![std::ffi::OsStr::new("-lint"), path.as_os_str()],
            )),
            Os::Windows => None,
        };
        if let Some((program, args)) = command {
            match std::process::Command::new(program).args(args).output() {
                Ok(output) if !output.status.success() => {
                    return Err(Error::new("autostart", "Saved login entry is invalid."))
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into())
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub fn set(paths: &Paths, executable: &Path, enabled: bool) -> Result<()> {
    // Installed on Windows: the installer's receipt-recorded Startup shortcut
    // is the one login entry. Drop the older VBScript entry so Tools never
    // starts twice.
    #[cfg(windows)]
    if arcade_tools_core::windows_login::state(paths, ids::TOOLS).is_some() {
        remove_managed(paths)?;
        return arcade_tools_core::windows_login::set(
            paths,
            ids::TOOLS,
            enabled,
            arcade_tools_core::windows_login::write_shortcut,
        );
    }
    set_for(paths, executable, enabled, Os::current())
}

/// On Windows, a Tools installer receipt owns start at login: remove Tools'
/// older VBScript entry and report the receipt's state. `None` elsewhere, or
/// without that receipt, where the Tools-managed entry applies.
pub fn installer_state(paths: &Paths) -> Result<Option<bool>> {
    #[cfg(windows)]
    if let Some(enabled) = arcade_tools_core::windows_login::state(paths, ids::TOOLS) {
        remove_managed(paths)?;
        return Ok(Some(enabled));
    }
    let _ = paths;
    Ok(None)
}

#[cfg(windows)]
fn remove_managed(paths: &Paths) -> Result<()> {
    let path = path(paths, Os::Windows);
    paths.guard(&path)?;
    match fs::read(&path) {
        Ok(bytes) if String::from_utf8_lossy(&bytes).contains(MARKER) => {
            fs::remove_file(path)?;
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn set_for(paths: &Paths, executable: &Path, enabled: bool, os: Os) -> Result<()> {
    let path = path(paths, os);
    paths.guard(&path)?;
    if enabled {
        paths.check_persistent_executable(executable)?;
    }
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let previous_text = previous
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|error| Error::new("autostart", error.to_string()))?;
    if previous_text.is_some_and(|text| {
        !(text.contains(MARKER) || (os == Os::Linux && platform::desktop_owned(text, ids::TOOLS)))
    }) {
        return Err(Error::new(
            "autostart",
            "An unrelated login entry uses Tools' name; it was left unchanged.",
        ));
    }
    let mut proposed = entry(executable, os)?;
    if enabled && os == Os::Linux {
        if let Some(text) = previous_text {
            proposed = platform::update_desktop(text, &proposed)?;
        }
    }
    if enabled && previous_text == Some(proposed.as_str()) {
        validate(&path, &proposed, os)?;
        return arcade_tools_core::installation::record_autostart(paths, ids::TOOLS, Some(path));
    }
    if let Some(bytes) = &previous {
        let backup = path.with_extension(format!(
            "{}.arcade-tools.bak",
            path.extension().unwrap().to_string_lossy()
        ));
        arcade_link::paths::write_atomic(&backup, bytes, true)?;
    }
    if !enabled {
        if previous.is_some() {
            fs::remove_file(&path)?;
        }
        return arcade_tools_core::installation::record_autostart(paths, ids::TOOLS, None);
    }
    arcade_link::paths::write_atomic(&path, proposed.as_bytes(), true)?;
    if let Err(error) = validate(&path, &proposed, os) {
        if let Some(bytes) = previous {
            arcade_link::paths::write_atomic(&path, &bytes, true)?;
        } else {
            fs::remove_file(path)?;
        }
        return Err(error);
    }
    arcade_tools_core::installation::record_autostart(paths, ids::TOOLS, Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_installer_entries_are_owned_and_the_receipt_follows_login_changes() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let executable = paths.home.join("Applications/Arcade-Tools.AppImage");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "fixture").unwrap();
        let receipt: arcade_link::receipt::Receipt = serde_json::from_value(serde_json::json!({
            "schema":1, "id":ids::TOOLS, "version":"0.2.0", "channel":"stable",
            "method":"appimage", "managedBy":"self", "path":executable, "integration":{},
            "installedAt":"2026-10-01T00:00:00Z", "updatedAt":"2026-10-01T00:00:00Z"
        }))
        .unwrap();
        let store = arcade_link::receipt::Store::new(&paths.link);
        store.write(&receipt).unwrap();
        let login = path(&paths, Os::Linux);
        fs::create_dir_all(login.parent().unwrap()).unwrap();
        let linked = platform::desktop_entry(ids::TOOLS, &executable).unwrap()
            + "X-Arcade-Id=arcade.tools\nOnlyShowIn=GNOME;\n";
        fs::write(&login, linked).unwrap();
        set_for(&paths, &executable, true, Os::Linux).unwrap();
        assert!(fs::read_to_string(&login)
            .unwrap()
            .contains("OnlyShowIn=GNOME;"));
        assert_eq!(
            store
                .read(ids::TOOLS)
                .unwrap()
                .unwrap()
                .integration
                .autostart,
            Some(login.clone())
        );
        set_for(&paths, &executable, false, Os::Linux).unwrap();
        assert!(!login.exists());
        assert!(store
            .read(ids::TOOLS)
            .unwrap()
            .unwrap()
            .integration
            .autostart
            .is_none());
        fs::write(&login, "[Desktop Entry]\nX-Arcade-Id=arcade.look\n").unwrap();
        assert!(set_for(&paths, &executable, true, Os::Linux).is_err());
    }

    #[test]
    fn isolated_entries_enable_backup_replace_and_disable_on_every_platform() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let executable = paths.home.join("Apps/a & b");
        let moved = paths.home.join("Apps/moved");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "fixture").unwrap();
        fs::write(&moved, "fixture").unwrap();
        for os in [Os::Linux, Os::Windows, Os::Macos] {
            set_for(&paths, &executable, true, os).unwrap();
            let login = path(&paths, os);
            let previous = fs::read(&login).unwrap();
            set_for(&paths, &moved, true, os).unwrap();
            assert_ne!(fs::read(&login).unwrap(), previous);
            let backup = login.with_extension(format!(
                "{}.arcade-tools.bak",
                login.extension().unwrap().to_string_lossy()
            ));
            assert_eq!(fs::read(&backup).unwrap(), previous);
            set_for(&paths, &moved, false, os).unwrap();
            assert!(!login.exists());
            fs::write(&login, "unrelated entry").unwrap();
            assert!(set_for(&paths, &moved, true, os).is_err());
            assert!(set_for(&paths, &moved, false, os).is_err());
            assert_eq!(fs::read_to_string(login).unwrap(), "unrelated entry");
        }
    }

    #[test]
    fn linux_preserves_options_and_live_profiles_reject_temporary_executables() {
        let root = tempfile::tempdir().unwrap();
        let mut paths = Paths::under(root.path());
        let executable = paths.home.join("Apps/tools");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "fixture").unwrap();
        set_for(&paths, &executable, true, Os::Linux).unwrap();
        let login = path(&paths, Os::Linux);
        let previous =
            fs::read_to_string(&login).unwrap() + "OnlyShowIn=GNOME;\nX-Custom=preserve\n";
        fs::write(&login, &previous).unwrap();
        set_for(&paths, &executable, true, Os::Linux).unwrap();
        assert_eq!(fs::read_to_string(&login).unwrap(), previous);
        paths.isolated = false;
        assert!(set_for(&paths, &executable, true, Os::Linux).is_err());
        assert_eq!(fs::read_to_string(login).unwrap(), previous);
        let windows = entry(Path::new("C:/Apps/it's 100%/tools.exe"), Os::Windows).unwrap();
        assert!(windows.contains("it's 100%"));
        assert!(windows.contains(", 0, False"));
        assert!(entry(Path::new("/bad\npath"), Os::Macos).is_err());
    }
}
