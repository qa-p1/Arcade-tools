use crate::apps::app_name;
use crate::paths::Paths;
use crate::release::{Asset, Kind, Os};
use crate::{Error, Result};
use arcade_link::manifest::ids;
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
    /// Blocking child-process work on a worker thread. Environment overrides
    /// stay scoped to this process and never reach persistent config files.
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
        child_environment(&mut command, paths);
        command.stdin(std::process::Stdio::null());
        // A freshly staged installer can be briefly held open for writing by a
        // child another thread forked before it exec'd (ETXTBSY); retry it.
        let mut attempts = 0;
        let out = loop {
            match command.output() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 40 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                result => break result?,
            }
        };
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

pub(crate) fn child_environment(command: &mut Command, paths: &Paths) {
    // Apps and installers get the user's environment, not the variables and
    // mount paths of Tools' own AppImage (wrong libraries, forced theme,
    // Tools' desktop files in XDG_DATA_DIRS, a working directory in the mount).
    arcade_link::childenv::sanitize(command);
    command
        .env("HOME", &paths.home)
        .env("USERPROFILE", &paths.home)
        .env("LOCALAPPDATA", &paths.local)
        .env("APPDATA", &paths.roaming)
        .env("XDG_CONFIG_HOME", &paths.config)
        .env("XDG_DATA_HOME", &paths.data)
        .env("XDG_CACHE_HOME", &paths.cache)
        .env("ARCADE_NO_INSTALL_PROMPT", "1");
    if paths.isolated {
        command.env("ARCADE_HOME", &paths.home);
    }
}

/// Launch on a worker with process-scoped paths and no inherited stdio. The
/// reaper blocks on child exit; there is no timer or idle polling.
pub fn spawn_detached(paths: &Paths, executable: &Path, args: &[String]) -> Result<()> {
    let mut command = Command::new(executable);
    child_environment(&mut command, paths);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200);
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
/// Pure silent command planning, safe on any thread. Shared v0.3 installer
/// tasks preserve the caller's login preference without creating shortcuts here.
pub fn windows_install_options(
    asset: &Asset,
    file: &Path,
    destination: &Path,
    channel: crate::release::Channel,
    login: bool,
) -> Result<CommandSpec> {
    let mut command = windows_install(asset, file, destination)?;
    let destination = command.args.pop().unwrap();
    command
        .args
        .push(format!("/ARCADECHANNEL={}", channel.as_str()));
    command.args.push("/ARCADEMANAGEDBY=tools".into());
    if asset.kind == Kind::Inno {
        command.args.push(format!(
            "/MERGETASKS={},!arcade_desktop",
            if login {
                "arcade_startup"
            } else {
                "!arcade_startup"
            }
        ));
    } else {
        command
            .args
            .push(format!("/STARTATLOGIN={}", u8::from(login)));
        command.args.push("/DESKTOPSHORTCUT=0".into());
    }
    command.args.push(destination); // NSIS's /D must remain the final raw tail.
    Ok(command)
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
    /// Commit and remove the backup on a worker thread; Drop performs disk work.
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

/// Installer/process/disk work on a worker thread. Keep/drop `Replacement` on
/// that worker too: its destructor restores files on failure.
pub fn install(
    paths: &Paths,
    id: &str,
    asset: &Asset,
    verified: &Path,
) -> Result<(PathBuf, Replacement)> {
    install_with_options(
        paths,
        id,
        asset,
        verified,
        crate::release::Channel::Stable,
        autostart_enabled(paths, id),
        None,
    )
}
/// The folder a Windows update replaces: where the user installed the app, as
/// its installer recorded in the receipt. `None` selects the default folder.
pub fn windows_install_root(receipt: Option<&arcade_link::receipt::Receipt>) -> Option<PathBuf> {
    receipt
        .filter(|r| r.method == arcade_link::receipt::InstallMethod::WindowsInstaller)
        .and_then(|r| r.path.parent())
        .map(Path::to_path_buf)
}
/// Native updates keep the destination selected by the standalone installer
/// or by dragging a macOS bundle into Applications.
pub fn native_install_root(receipt: Option<&arcade_link::receipt::Receipt>) -> Option<PathBuf> {
    match receipt {
        Some(receipt) if receipt.method == arcade_link::receipt::InstallMethod::MacosBundle => {
            Some(receipt.path.clone())
        }
        receipt => windows_install_root(receipt),
    }
}

pub fn native_install_paths(
    paths: &Paths,
    id: &str,
    os: Os,
    tarball: bool,
    chosen: Option<&Path>,
) -> (PathBuf, PathBuf) {
    let root = paths.install_root(id, os, tarball);
    let executable = paths.executable(id, os, tarball);
    match chosen.filter(|_| matches!(os, Os::Windows | Os::Macos)) {
        Some(chosen) => (
            chosen.to_path_buf(),
            chosen.join(executable.strip_prefix(root).unwrap()),
        ),
        None => (root, executable),
    }
}
/// Verified native/transition installer work on a worker. The app's own
/// installer receives the selected channel and existing login preference.
/// `native_root` keeps native updates in the folder the user chose.
pub fn install_with_options(
    paths: &Paths,
    id: &str,
    asset: &Asset,
    verified: &Path,
    channel: crate::release::Channel,
    login: bool,
    native_root: Option<&Path>,
) -> Result<(PathBuf, Replacement)> {
    let tarball = asset.kind == Kind::Tarball;
    let (root, executable) = native_install_paths(paths, id, Os::current(), tarball, native_root);
    let destination = if asset.kind == Kind::Appimage {
        &executable
    } else {
        &root
    };
    paths.guard(destination)?;
    // Prepare contents before replacing the old installation.
    let prepared = if tarball {
        Some(if id == crate::apps::SHELF {
            extract_shelf(verified, &paths.downloads())?
        } else {
            extract_clipboard(verified, &paths.downloads())?
        })
    } else {
        None
    };
    let replacement = Replacement::begin(paths, destination)?;
    let _ = (channel, login); // Native Windows arguments; no extra Linux policy.
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
        // Transition release only: retain Shelf/Clipboard tarballs until the
        // coordinated AppImage release has shipped. New manifests use installArgs.
        Kind::Tarball if id == crate::apps::SHELF => {
            // The bundle is data, never an executable installation script.
            let (_, package) = prepared.as_ref().unwrap();
            copy_shelf_bundle(package, &root)?;
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
        windows_install_options(asset, &temp_path, &root, channel, login)?.run(paths)?;
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

/// Extract a Shelf runtime bundle without following links or running scripts.
/// Filesystem/archive work on a worker thread.
pub fn extract_shelf(file: &Path, cache: &Path) -> Result<(tempfile::TempDir, PathBuf)> {
    fs::create_dir_all(cache)?;
    let directory = tempfile::Builder::new()
        .prefix("shelf-package-")
        .tempdir_in(cache)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(file)?));
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
                "Shelf package exceeds extraction limits.",
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
                "Shelf archive contains an unsafe path, link or special file.",
            ));
        }
        if !entry.unpack_in(directory.path())? {
            return Err(Error::new(
                "installer",
                "Shelf archive escaped its extraction directory.",
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
            p.join("arcade-shelf").is_file()
                && p.join("bin/arcade-shelf").is_file()
                && p.join("bin/qt.conf").is_file()
        })
        .collect();
    if packages.len() != 1 {
        return Err(Error::new(
            "installer",
            "Shelf bundle must contain arcade-shelf, bin/arcade-shelf and bin/qt.conf.",
        ));
    }
    Ok((directory, packages[0].clone()))
}

#[cfg(target_os = "linux")]
fn copy_shelf_bundle(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source = entry.path();
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_shelf_bundle(&source, &target)?;
        } else if kind.is_file() {
            fs::copy(&source, &target)?;
        } else {
            return Err(Error::new(
                "installer",
                "Unexpected special file in validated Shelf bundle.",
            ));
        }
    }
    Ok(())
}

/// Filesystem/archive work on a worker thread; no scripts are executed here.
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
/// Filesystem mutation on a worker thread; missing files are tolerated.
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
/// Generated login-entry disk/process work on a worker thread; reads and backs
/// up existing entries and validates the saved result.
pub fn set_autostart(paths: &Paths, id: &str, executable: &Path, enabled: bool) -> Result<()> {
    set_autostart_entry(paths, id, executable, enabled)?;
    #[cfg(not(windows))]
    crate::installation::record_autostart(
        paths,
        id,
        enabled.then(|| paths.autostart(id, Os::current())),
    )?;
    Ok(())
}
fn set_autostart_entry(paths: &Paths, id: &str, executable: &Path, enabled: bool) -> Result<()> {
    // An app installed by its own Windows installer owns a receipt-recorded
    // Startup shortcut. Remove the transition entries first so the app is
    // never started twice, then toggle that shortcut.
    #[cfg(windows)]
    if crate::windows_login::state(paths, id).is_some() {
        if id == crate::apps::SHELF {
            remove_managed_shelf_entry(paths)?;
        } else {
            delete_run_value(paths, id)?;
        }
        return crate::windows_login::set(paths, id, enabled, crate::windows_login::write_shortcut);
    }
    if id == crate::apps::SHELF {
        return set_shelf_autostart(paths, executable, enabled);
    }
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
fn set_shelf_autostart(paths: &Paths, executable: &Path, enabled: bool) -> Result<()> {
    let path = paths.autostart(crate::apps::SHELF, Os::current());
    paths.guard(&path)?;
    if enabled {
        paths.check_persistent_executable(executable)?;
    }
    let previous = match fs::read(&path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let owned = text.contains("Arcade Shelf managed login entry")
                || (Os::current() == Os::Linux && desktop_owned(&text, crate::apps::SHELF));
            if !owned {
                return Err(Error::new(
                    "autostart",
                    "An unrelated login entry already uses Shelf's name.",
                ));
            }
            Some(bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(bytes) = &previous {
        let backup = path.with_file_name(format!(
            "{}.arcade-tools.bak",
            path.file_name().unwrap().to_string_lossy()
        ));
        arcade_link::paths::write_atomic(&backup, bytes, true)?;
    }
    if !enabled {
        return remove(&path);
    }
    let marker = "Arcade Shelf managed login entry";
    let proposed = match Os::current() {
        Os::Linux => format!(
            "# {marker}\n{}",
            desktop_entry(crate::apps::SHELF, executable)?
        ),
        Os::Macos => launch_agent(crate::apps::SHELF, executable)
            .replace("<plist", &format!("<!-- {marker} -->\n<plist")),
        Os::Windows => {
            let exe = executable.to_string_lossy();
            if exe.chars().any(char::is_control) {
                return Err(Error::new(
                    "paths",
                    "Executable contains a control character.",
                ));
            }
            let escaped = exe.replace('"', "\"\"");
            format!("' {marker}\r\nSet shell = CreateObject(\"WScript.Shell\")\r\nshell.Run Chr(34) & \"{escaped}\" & Chr(34) & \" --background\", 0, False\r\n")
        }
    };
    arcade_link::paths::write_atomic(&path, proposed.as_bytes(), true)?;
    if fs::read_to_string(&path)? != proposed {
        return Err(Error::new(
            "autostart",
            "Could not verify Shelf's login entry.",
        ));
    }
    Ok(())
}

pub fn desktop_owned(text: &str, id: &str) -> bool {
    let mut main = false;
    text.lines().any(|line| {
        if line.starts_with('[') {
            main = line.trim() == "[Desktop Entry]";
        }
        main && line.trim().strip_prefix("X-Arcade-Id=") == Some(id)
    })
}

#[cfg(windows)]
fn delete_run_value(paths: &Paths, id: &str) -> Result<()> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let name = windows_run_name(id);
    if Command::new("reg.exe")
        .args(["query", key, "/v", name])
        .output()?
        .status
        .success()
    {
        CommandSpec {
            program: "reg.exe".into(),
            args: ["delete", key, "/v", name, "/f"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
        .run(paths)?;
    }
    Ok(())
}
#[cfg(windows)]
fn remove_managed_shelf_entry(paths: &Paths) -> Result<()> {
    let path = paths.autostart(crate::apps::SHELF, Os::Windows);
    paths.guard(&path)?;
    match fs::read(&path) {
        Ok(bytes)
            if String::from_utf8_lossy(&bytes).contains("Arcade Shelf managed login entry") =>
        {
            remove(&path)
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn windows_run_name(id: &str) -> &'static str {
    match id {
        ids::BOX => "Arcade Box",
        ids::LENS => "ArcadeLens",
        ids::LOOK => "ArcadeLook",
        ids::WHEEL => "ArcadeWheel",
        ids::CLIPBOARD => "ArcadeClipboard",
        crate::apps::SHELF => "ArcadeShelf",
        crate::apps::FIND => "ArcadeFind",
        _ => "ArcadeTools",
    }
}
/// Filesystem/Windows registry-process read on a worker thread.
pub fn autostart_enabled(paths: &Paths, id: &str) -> bool {
    #[cfg(windows)]
    if let Some(enabled) = crate::windows_login::state(paths, id) {
        return enabled;
    }
    if id == crate::apps::SHELF {
        return paths.autostart(id, Os::current()).is_file();
    }
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
