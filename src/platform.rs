use crate::paths::Paths;
use crate::release::{Asset, Kind, Os};
use crate::{Error, Result};
use arcade_link::manifest::{app_name, ids};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}
impl CommandSpec {
    /// NSIS consumes its final path as an unquoted command-line tail, even with
    /// spaces. Inno and the other commands use ordinary argument quoting.
    pub fn nsis_tail(&self) -> Option<&str> {
        self.args
            .last()
            .map(String::as_str)
            .filter(|arg| arg.starts_with("/D=") || arg.starts_with("_?="))
    }
    pub fn run(&self, paths: &Paths) -> Result<()> {
        let mut command = Command::new(&self.program);
        #[cfg(windows)]
        if let Some(tail) = self.nsis_tail() {
            use std::os::windows::process::CommandExt;
            command
                .args(&self.args[..self.args.len() - 1])
                .raw_arg(tail);
        } else {
            command.args(&self.args);
        }
        #[cfg(not(windows))]
        command.args(&self.args);
        let out = command
            .env("HOME", &paths.home)
            .env("XDG_CONFIG_HOME", &paths.config)
            .env("XDG_DATA_HOME", &paths.data)
            .env("XDG_CACHE_HOME", &paths.cache)
            .output()?;
        if !out.status.success() {
            return Err(Error::new(
                "installer",
                format!(
                    "{} failed ({}): {}",
                    self.program.display(),
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                ),
            ));
        }
        Ok(())
    }
}
pub fn windows_install(asset: &Asset, file: &Path, destination: &Path) -> Result<CommandSpec> {
    let expected: &[&str] = match asset.kind {
        Kind::Nsis => &["/S"],
        Kind::Inno => &[
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CURRENTUSER",
        ],
        _ => {
            return Err(Error::new(
                "unavailable",
                "Only per-user NSIS and Inno installers are supported on Windows.",
            ))
        }
    };
    if asset
        .silent
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        != expected
    {
        return Err(Error::new(
            "manifest",
            "Release installer flags do not match the per-user installer contract.",
        ));
    }
    let mut args = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // https://nsis.sourceforge.io/Docs/Chapter3.html#installerusage
    args.push(if asset.kind == Kind::Nsis {
        nsis_path_arg("/D=", destination)?
    } else {
        format!("/DIR={}", destination.display())
    });
    Ok(CommandSpec {
        program: file.into(),
        args,
    })
}
pub fn windows_uninstall(kind: Kind, root: &Path) -> Result<CommandSpec> {
    match kind {
        Kind::Nsis => Ok(CommandSpec {
            program: root.join("uninstall.exe"),
            // Prevent NSIS from detaching a temporary copy: wait for it before
            // removing the installation directory and registry manifest.
            args: vec!["/S".into(), nsis_path_arg("_?=", root)?],
        }),
        Kind::Inno => Ok(CommandSpec {
            program: root.join("unins000.exe"),
            args: ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }),
        _ => Err(Error::new(
            "unavailable",
            "No supported per-user uninstaller.",
        )),
    }
}
fn nsis_path_arg(prefix: &str, path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::new("paths", "Installer destination is not a Unicode path."))?;
    if text.contains('"') || text.chars().any(char::is_control) {
        return Err(Error::new(
            "paths",
            "Installer destination contains a quote or control character.",
        ));
    }
    Ok(format!("{prefix}{text}"))
}
pub fn dmg_attach(file: &Path, mount: &Path) -> CommandSpec {
    CommandSpec {
        program: "hdiutil".into(),
        args: vec![
            "attach".into(),
            "-readonly".into(),
            "-nobrowse".into(),
            "-mountpoint".into(),
            mount.display().to_string(),
            file.display().to_string(),
        ],
    }
}

/// Holds the previous installation until the new files and record are committed.
pub struct Replacement {
    destination: PathBuf,
    backup: tempfile::TempDir,
    had_old: bool,
    committed: bool,
}
impl Replacement {
    fn begin(paths: &Paths, destination: &Path) -> Result<Self> {
        paths.guard(destination)?;
        let parent = destination
            .parent()
            .ok_or_else(|| Error::new("paths", "Install path has no parent."))?;
        fs::create_dir_all(parent)?;
        let backup = tempfile::Builder::new()
            .prefix(".arcade-backup-")
            .tempdir_in(parent)?;
        let had_old = destination.exists();
        if had_old {
            fs::rename(destination, backup.path().join("previous"))?;
        }
        Ok(Self {
            destination: destination.into(),
            backup,
            had_old,
            committed: false,
        })
    }
    pub fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for Replacement {
    fn drop(&mut self) {
        if !self.committed {
            let _ = remove(&self.destination);
            if self.had_old {
                let _ = fs::rename(self.backup.path().join("previous"), &self.destination);
            }
        }
    }
}

pub fn install(
    paths: &Paths,
    id: &str,
    asset: &Asset,
    verified: &Path,
) -> Result<(PathBuf, Replacement)> {
    let tarball = asset.kind == Kind::Tarball;
    let executable = paths.executable(id, Os::current(), tarball);
    let root = paths.install_root(id, Os::current(), tarball);
    let destination = if asset.kind == Kind::Appimage {
        &executable
    } else {
        &root
    };
    paths.guard(destination)?;
    // Prepare contents before replacing the old installation.
    let prepared = if tarball {
        Some(extract_clipboard(verified, &paths.downloads())?)
    } else {
        None
    };
    let replacement = Replacement::begin(paths, destination)?;
    #[cfg(target_os = "linux")]
    match asset.kind {
        Kind::Appimage => {
            use std::os::unix::fs::PermissionsExt;
            let mut staged = tempfile::NamedTempFile::new_in(&root)?;
            std::io::copy(&mut fs::File::open(verified)?, &mut staged)?;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o755))?;
            staged.as_file().sync_all()?;
            staged
                .persist(&executable)
                .map_err(|e| Error::from(e.error))?;
        }
        Kind::Tarball if id == ids::CLIPBOARD => {
            let (dir, package) = prepared.as_ref().unwrap();
            let script = package.join("scripts/install-linux.sh");
            CommandSpec {
                program: "bash".into(),
                args: vec![script.display().to_string()],
            }
            .run(paths)?;
            let _keep_alive = dir;
        }
        _ => {
            return Err(Error::new(
                "unavailable",
                "Unsupported Linux per-user installer.",
            ))
        }
    }
    #[cfg(windows)]
    {
        let _ = &prepared;
        // A verified installer is named .exe so Windows treats it as an executable.
        let temp = tempfile::Builder::new()
            .prefix("arcade-")
            .suffix(".exe")
            .tempfile_in(paths.downloads())?;
        fs::copy(verified, temp.path())?;
        let temp_path = temp.into_temp_path();
        windows_install(asset, &temp_path, &root)?.run(paths)?;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = &prepared;
        if asset.kind != Kind::Dmg {
            return Err(Error::new("unavailable", "A macOS disk image is required."));
        }
        let mount = tempfile::Builder::new()
            .prefix("arcade-mount-")
            .tempdir_in(paths.downloads())?;
        dmg_attach(verified, mount.path()).run(paths)?;
        let result = (|| {
            let bundle = mount.path().join(format!("{}.app", app_name(id)));
            if !bundle.is_dir() {
                return Err(Error::new(
                    "installer",
                    "Disk image does not contain the expected app bundle.",
                ));
            }
            // ditto preserves quarantine and extended attributes; never run xattr -d.
            CommandSpec {
                program: "ditto".into(),
                args: vec![bundle.display().to_string(), root.display().to_string()],
            }
            .run(paths)
        })();
        let detached = CommandSpec {
            program: "hdiutil".into(),
            args: vec!["detach".into(), mount.path().display().to_string()],
        }
        .run(paths);
        result?;
        detached?;
    }
    if !executable.is_file() {
        return Err(Error::new(
            "installer",
            format!("Installer did not create {}", executable.display()),
        ));
    }
    Ok((executable, replacement))
}

pub fn extract_clipboard(file: &Path, cache: &Path) -> Result<(tempfile::TempDir, PathBuf)> {
    fs::create_dir_all(cache)?;
    let directory = tempfile::Builder::new()
        .prefix("clipboard-package-")
        .tempdir_in(cache)?;
    let decoder = flate2::read::GzDecoder::new(fs::File::open(file)?);
    let mut archive = tar::Archive::new(decoder);
    let mut bytes = 0u64;
    let mut count = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let kind = entry.header().entry_type();
        bytes = bytes
            .checked_add(entry.size())
            .ok_or_else(|| Error::new("too_large", "Archive size overflow."))?;
        count += 1;
        if bytes > 2 * 1024 * 1024 * 1024 || count > 100000 {
            return Err(Error::new(
                "too_large",
                "Clipboard package exceeds extraction limits.",
            ));
        }
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            || !(kind.is_file() || kind.is_dir())
        {
            return Err(Error::new(
                "installer",
                "Archive contains an unsafe path, link, or special file.",
            ));
        }
        if !entry.unpack_in(directory.path())? {
            return Err(Error::new(
                "installer",
                "Archive entry escaped its extraction directory.",
            ));
        }
    }
    let mut roots = vec![directory.path().to_path_buf()];
    roots.extend(
        fs::read_dir(directory.path())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir()),
    );
    let packages: Vec<_> = roots
        .into_iter()
        .filter(|p| {
            p.join("scripts/install-linux.sh").is_file()
                && p.join("apps/flutter_app/build/linux/x64/release/bundle/clipboard")
                    .is_file()
        })
        .collect();
    if packages.len() != 1 {
        return Err(Error::new("installer", "Clipboard tarball must include scripts/install-linux.sh and apps/flutter_app/build/linux/x64/release/bundle/clipboard."));
    }
    Ok((directory, packages[0].clone()))
}
pub fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(())
}

pub fn desktop_entry(id: &str, executable: &Path) -> Result<String> {
    let text = executable.to_string_lossy();
    if text.chars().any(char::is_control) {
        return Err(Error::new(
            "paths",
            "Executable path contains a control character.",
        ));
    }
    let quoted = text
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%");
    Ok(format!("[Desktop Entry]\nType=Application\nName={}\nExec=\"{}\" --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n{}", app_name(id), quoted, if id == ids::CLIPBOARD { "X-ArcadeClipboard-Managed=true\n" } else { "" }))
}
pub fn launch_agent(id: &str, executable: &Path) -> String {
    let quoted = executable
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{id}</string><key>ProgramArguments</key><array><string>{quoted}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>\n")
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn backup(path: &Path) -> Result<()> {
    match fs::read(path) {
        Ok(bytes) => {
            let backup = path.with_file_name(format!(
                "{}.arcade-tools.bak",
                path.file_name().unwrap().to_string_lossy()
            ));
            arcade_link::paths::write_atomic(&backup, &bytes, true)?;
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(())
}
pub fn update_desktop(existing: &str, proposed: &str) -> Result<String> {
    if !existing.lines().any(|l| l.trim() == "[Desktop Entry]") {
        return Err(Error::new(
            "autostart",
            "Existing login entry is invalid; it was left unchanged.",
        ));
    }
    let replacements: std::collections::BTreeMap<_, _> = proposed
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(key, _)| {
            matches!(
                *key,
                "Exec" | "X-GNOME-Autostart-enabled" | "X-ArcadeClipboard-Managed"
            )
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut lines = Vec::new();
    let mut in_group = false;
    for line in existing.lines() {
        if line.starts_with('[') {
            if in_group {
                for (key, value) in &replacements {
                    if !seen.contains(key) {
                        lines.push(format!("{key}={value}"));
                    }
                }
            }
            in_group = line.trim() == "[Desktop Entry]";
        }
        if in_group {
            if let Some((key, _)) = line.split_once('=') {
                if matches!(key, "Hidden") {
                    continue;
                }
                if let Some(value) = replacements.get(key) {
                    if seen.insert(key) {
                        lines.push(format!("{key}={value}"));
                    }
                    continue;
                }
            }
        }
        lines.push(line.to_string());
    }
    if in_group {
        for (key, value) in replacements {
            if !seen.contains(key) {
                lines.push(format!("{key}={value}"));
            }
        }
    }
    Ok(lines.join("\n") + "\n")
}
pub fn set_autostart(paths: &Paths, id: &str, executable: &Path, enabled: bool) -> Result<()> {
    if !login_supported(id, Os::current()) {
        return Err(Error::new(
            "unavailable",
            "Change start at login inside this app; its macOS login item belongs to the app.",
        ));
    }
    let path = paths.autostart(id, Os::current());
    paths.guard(&path)?;
    if enabled {
        paths.check_persistent_executable(executable)?;
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut proposed = if cfg!(target_os = "linux") {
            desktop_entry(id, executable)?
        } else {
            launch_agent(id, executable)
        };
        let previous = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if cfg!(target_os = "linux") && enabled {
            if let Some(bytes) = &previous {
                proposed = update_desktop(
                    std::str::from_utf8(bytes)
                        .map_err(|e| Error::new("autostart", e.to_string()))?,
                    &proposed,
                )?;
            }
        }
        if enabled && fs::read_to_string(&path).is_ok_and(|s| s == proposed) {
            return Ok(());
        }
        backup(&path)?;
        if enabled {
            arcade_link::paths::write_atomic(&path, proposed.as_bytes(), true)?;
            if fs::read_to_string(&path)? != proposed {
                return Err(Error::new(
                    "autostart",
                    "Could not verify the saved login entry.",
                ));
            }
            #[cfg(target_os = "linux")]
            if Command::new("desktop-file-validate")
                .arg(&path)
                .output()
                .is_ok_and(|r| !r.status.success())
            {
                if let Some(bytes) = &previous {
                    arcade_link::paths::write_atomic(&path, bytes, true)?;
                } else {
                    remove(&path)?;
                }
                return Err(Error::new("autostart", "Saved desktop entry is invalid."));
            }
            #[cfg(target_os = "macos")]
            CommandSpec {
                program: "plutil".into(),
                args: vec!["-lint".into(), path.display().to_string()],
            }
            .run(paths)?;
        } else {
            remove(&path)?;
        }
    }
    #[cfg(windows)]
    {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let name = windows_run_name(id);
        let previous = Command::new("reg.exe")
            .args(["query", key, "/v", name])
            .output()?;
        fs::create_dir_all(paths.state())?;
        fs::write(
            paths.state().join(format!("{id}.login-backup.txt")),
            &previous.stdout,
        )?;
        let quoted = format!("\"{}\" --background", executable.display());
        let args = if enabled {
            vec!["add", key, "/v", name, "/t", "REG_SZ", "/d", &quoted, "/f"]
        } else {
            vec!["delete", key, "/v", name, "/f"]
        };
        if enabled || previous.status.success() {
            CommandSpec {
                program: "reg.exe".into(),
                args: args.iter().map(|s| s.to_string()).collect(),
            }
            .run(paths)?;
        }
        let saved = Command::new("reg.exe")
            .args(["query", key, "/v", name])
            .output()?;
        if saved.status.success() != enabled
            || enabled && !String::from_utf8_lossy(&saved.stdout).contains(&quoted)
        {
            return Err(Error::new(
                "autostart",
                "Could not verify the saved login entry.",
            ));
        }
    }
    Ok(())
}
pub fn windows_run_name(id: &str) -> &'static str {
    match id {
        ids::BOX => "Arcade Box",
        ids::LENS => "ArcadeLens",
        ids::LOOK => "ArcadeLook",
        ids::WHEEL => "ArcadeWheel",
        ids::CLIPBOARD => "ArcadeClipboard",
        _ => "ArcadeTools",
    }
}
pub fn autostart_enabled(paths: &Paths, id: &str) -> bool {
    #[cfg(windows)]
    {
        let _ = paths;
        Command::new("reg.exe")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                windows_run_name(id),
            ])
            .output()
            .is_ok_and(|r| r.status.success())
    }
    #[cfg(not(windows))]
    paths.autostart(id, Os::current()).is_file()
}
pub fn login_supported(id: &str, os: Os) -> bool {
    !(os == Os::Macos && matches!(id, ids::WHEEL | ids::CLIPBOARD))
}
