use arcade_link::install::{Environment, Runtime, UninstallOptions};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::{Error, Result};

#[cfg(target_os = "linux")]
fn info() -> arcade_link::install::AppInfo {
    use arcade_link::install::AppInfo;
    AppInfo {
        id: "arcade.tools".into(),
        name: "Arcade Tools".into(),
        filename: "Arcade-Tools.AppImage".into(),
        desktop_id: "app.arcadetools".into(),
        cli_name: "arcade-tools".into(),
        background_args: vec!["--background".into()],
        legacy_desktop_ids: vec!["arcade-tools".into()],
        autostart_id: "arcade-tools".into(),
    }
}
pub fn install(channel: &str, start_at_login: bool) -> Result<arcade_link::receipt::Receipt> {
    install_options(channel, start_at_login, false)
}
// Linux installs the AppImage and macOS moves the app bundle to Applications.
// Windows uses the per-user setup program, which writes the receipt itself.
#[cfg(windows)]
fn install_options(
    _channel: &str,
    _start_at_login: bool,
    _remove_download: bool,
) -> Result<arcade_link::receipt::Receipt> {
    let paths = Paths::discover()?;
    windows_install_receipt(&paths, &std::env::current_exe()?)
}
#[cfg(any(windows, test))]
fn windows_install_receipt(
    paths: &Paths,
    executable: &std::path::Path,
) -> Result<arcade_link::receipt::Receipt> {
    let receipt = arcade_link::receipt::Store::new(&paths.link)
        .read("arcade.tools")?
        .filter(|r| r.method == arcade_link::receipt::InstallMethod::WindowsInstaller)
        .ok_or_else(unsupported)?;
    arcade_tools_core::installation::validate_receipt(
        paths,
        &receipt,
        "arcade.tools",
        env!("CARGO_PKG_VERSION"),
        if receipt.channel == "nightly" {
            arcade_tools_core::release::Channel::Nightly
        } else {
            arcade_tools_core::release::Channel::Stable
        },
    )?;
    if std::fs::canonicalize(&receipt.path)? != std::fs::canonicalize(executable)? {
        return Err(unsupported());
    }
    Ok(receipt)
}
#[cfg(any(windows, test))]
fn unsupported() -> Error {
    Error::new(
        "unsupported",
        "On Windows, install, repair and update Arcade Tools with its setup program; it writes the installation receipt.",
    )
}
#[cfg(not(windows))]
fn write_manifest(
    environment: &Environment,
    receipt: &arcade_link::receipt::Receipt,
) -> Result<()> {
    arcade_link::manifest::write_manifest(
        &environment.locations,
        &arcade_link::Manifest::new(
            "arcade.tools",
            env!("CARGO_PKG_VERSION"),
            &arcade_tools_core::installation::executable(&Paths::discover()?, receipt)
                .to_string_lossy(),
        ),
    )?;
    Ok(())
}
#[cfg(target_os = "macos")]
const BUNDLE: &str = "Arcade Tools.app";
#[cfg(target_os = "macos")]
const BUNDLE_EXECUTABLE: &str = "Contents/MacOS/arcade-tools";
#[cfg(target_os = "macos")]
fn macos_receipt(
    environment: &Environment,
    bundle: &std::path::Path,
    cli: Option<std::path::PathBuf>,
    channel: &str,
) -> Result<arcade_link::receipt::Receipt> {
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt};
    let old = environment.receipts().read("arcade.tools")?;
    let now = arcade_link::manifest::now_rfc3339();
    Ok(Receipt {
        schema: 1,
        id: "arcade.tools".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        channel: channel.into(),
        method: InstallMethod::MacosBundle,
        managed_by: old
            .as_ref()
            .map_or(ManagedBy::SelfManaged, |r| r.managed_by),
        path: bundle.into(),
        integration: Integration {
            cli,
            autostart: old.as_ref().and_then(|r| r.integration.autostart.clone()),
            ..Default::default()
        },
        previous: None,
        installed_at: old
            .as_ref()
            .map_or_else(|| now.clone(), |r| r.installed_at.clone()),
        updated_at: now,
    })
}
/// Moves (or, from a read-only disk image, copies) the running bundle to
/// Applications, installs the CLI link and writes the receipt. Start at login
/// on macOS is Tools' own LaunchAgent, set from its tray settings.
#[cfg(target_os = "macos")]
fn install_options(
    channel: &str,
    _start_at_login: bool,
    _remove_download: bool,
) -> Result<arcade_link::receipt::Receipt> {
    use arcade_link::install::{platform::macos, State};
    let environment = Environment::discover()?;
    let runtime = Runtime::discover()?;
    let detection = arcade_link::install::detect(&environment, "arcade.tools", &runtime)?;
    if detection.state == State::DevBuild {
        return Err(Error::new(
            "dev_build",
            "Development builds are never installed.",
        ));
    }
    let source = macos::bundle_for(&runtime.executable).ok_or_else(|| {
        Error::new(
            "unsupported",
            "--install needs the Arcade Tools app bundle.",
        )
    })?;
    let root = environment.home.join("Applications");
    std::fs::create_dir_all(&root)?;
    let target = root.join(BUNDLE);
    if std::fs::canonicalize(&source).ok() != std::fs::canonicalize(&target).ok() {
        if target.exists() && detection.receipt.as_ref().is_none_or(|r| r.path != target) {
            return Err(Error::new(
                "conflict",
                "Applications already contains Arcade Tools without its receipt; it was left unchanged.",
            ));
        }
        stage_bundle(&environment, &source, &target)?;
    }
    let cli = macos::install_cli(
        &environment,
        "arcade-tools",
        &target.join(BUNDLE_EXECUTABLE),
    )?;
    let receipt = macos_receipt(&environment, &target, Some(cli), channel)?;
    environment.receipts().write(&receipt)?;
    write_manifest(&environment, &receipt)?;
    Ok(receipt)
}
#[cfg(target_os = "macos")]
fn stage_bundle(
    environment: &Environment,
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<()> {
    let stage = tempfile::Builder::new()
        .prefix(".arcade-tools-")
        .tempdir_in(target.parent().unwrap())?;
    let staged = arcade_link::install::platform::macos::move_to_applications(
        environment,
        source,
        Some(stage.path()),
    )?;
    if target.exists() {
        std::fs::remove_dir_all(target)?;
    }
    std::fs::rename(staged, target)?;
    Ok(())
}
#[cfg(target_os = "linux")]
fn install_options(
    channel: &str,
    start_at_login: bool,
    remove_download: bool,
) -> Result<arcade_link::receipt::Receipt> {
    use arcade_link::install::{Icon, IconSize, InstallOptions};
    use arcade_link::receipt::ManagedBy;
    let environment = Environment::discover()?;
    let runtime = Runtime::discover()?;
    let staging = tempfile::tempdir()?;
    let icon = staging.path().join("tools.png");
    std::fs::write(&icon, include_bytes!("../icons/128x128.png"))?;
    let icons = [Icon {
        source: icon,
        size: IconSize::Pixels(128),
    }];
    let app = info();
    let receipt = arcade_link::install::install(
        &environment,
        InstallOptions {
            app: &app,
            runtime: &runtime,
            version: env!("CARGO_PKG_VERSION"),
            channel,
            managed_by: ManagedBy::SelfManaged,
            icons: &icons,
            start_at_login,
            remove_download,
            refresh_caches: true,
        },
    )?;
    write_manifest(&environment, &receipt)?;
    Ok(receipt)
}
#[cfg(target_os = "linux")]
fn repair(environment: &Environment) -> Result<arcade_link::receipt::Receipt> {
    let runtime = Runtime::discover()?;
    repair_at(environment, runtime.appimage.as_deref())
}
#[cfg(target_os = "linux")]
fn repair_at(
    environment: &Environment,
    moved_to: Option<&std::path::Path>,
) -> Result<arcade_link::receipt::Receipt> {
    Ok(arcade_link::install::repair(
        environment,
        &info(),
        moved_to,
    )?)
}
/// Link removes the CLI link only when it targets the receipt path, which on
/// macOS is the bundle; this link targets the executable inside it.
#[cfg(target_os = "macos")]
fn bundle_cli(environment: &Environment) -> Result<Option<std::path::PathBuf>> {
    let Some(receipt) = environment.receipts().read("arcade.tools")? else {
        return Ok(None);
    };
    Ok(receipt.integration.cli.filter(|cli| {
        std::fs::read_link(cli).is_ok_and(|target| target.starts_with(&receipt.path))
    }))
}
/// Re-creates the CLI link and refreshes the receipt for the recorded bundle.
#[cfg(target_os = "macos")]
fn repair(environment: &Environment) -> Result<arcade_link::receipt::Receipt> {
    let mut receipt = environment
        .receipts()
        .read("arcade.tools")?
        .ok_or_else(|| Error::new("not_installed", "Arcade Tools has no installation receipt."))?;
    let executable = receipt.path.join(BUNDLE_EXECUTABLE);
    if !executable.is_file() {
        return Err(Error::new(
            "not_installed",
            "The installed Arcade Tools executable is missing.",
        ));
    }
    receipt.integration.cli = Some(arcade_link::install::platform::macos::install_cli(
        environment,
        "arcade-tools",
        &executable,
    )?);
    receipt.updated_at = arcade_link::manifest::now_rfc3339();
    environment.receipts().write(&receipt)?;
    write_manifest(environment, &receipt)?;
    Ok(receipt)
}
#[cfg(windows)]
fn repair(_environment: &Environment) -> Result<arcade_link::receipt::Receipt> {
    Err(unsupported())
}
pub fn run(args: &[String]) -> Option<Result<()>> {
    let command = args.iter().find(|arg| {
        matches!(
            arg.as_str(),
            "--install" | "--uninstall" | "--repair" | "--integration-status"
        )
    })?;
    Some((|| {
        let environment = Environment::discover()?;
        match command.as_str() {
            "--install" => {
                let channel = args
                    .iter()
                    .position(|arg| arg == "--channel")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str)
                    .unwrap_or("stable");
                if !matches!(channel, "stable" | "nightly") {
                    return Err(Error::new("usage", "Choose stable or nightly."));
                }
                let receipt = install(channel, args.iter().any(|arg| arg == "--start-at-login"))?;
                println!("{}", serde_json::to_string(&receipt)?);
                if !args.iter().any(|arg| arg == "--no-launch") {
                    arcade_link::client::spawn_detached(
                        &arcade_tools_core::installation::executable(&Paths::discover()?, &receipt)
                            .to_string_lossy(),
                        &["--background".into()],
                    )?;
                }
            }
            "--repair" => println!("{}", serde_json::to_string(&repair(&environment)?)?),
            "--uninstall" => {
                let folders = Paths::discover()?
                    .data_folders("arcade.tools", arcade_tools_core::release::Os::current());
                #[cfg(target_os = "macos")]
                let cli = bundle_cli(&environment)?;
                arcade_link::install::uninstall(
                    &environment,
                    "arcade.tools",
                    UninstallOptions {
                        remove_data: args.iter().any(|arg| arg == "--remove-data"),
                        data_folders: &folders,
                    },
                )?;
                #[cfg(target_os = "macos")]
                if let Some(cli) = cli {
                    match std::fs::remove_file(cli) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                        _ => {}
                    }
                }
            }
            _ => {
                println!(
                    "{}",
                    serde_json::to_string(&arcade_link::install::integration_status(
                        &environment,
                        "arcade.tools",
                        ""
                    )?)?
                );
            }
        }
        Ok(())
    })())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    title: String,
    copy: String,
    installed: bool,
    /// Button labels: install (or move), and run this copy once.
    primary: String,
    secondary: String,
    /// Whether the start-at-login and remove-download options apply.
    options: bool,
}
fn dismissal_key(runtime: &Runtime) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    runtime.running_from().hash(&mut hash);
    format!("{:016x}", hash.finish())
}
/// The macOS first run: a copy in Applications without a receipt records one,
/// so Tools can manage and update itself; a copy run from the disk image,
/// Downloads or a translocated folder offers "Move to Applications".
#[cfg(target_os = "macos")]
fn proposal(
    environment: &Environment,
    runtime: &Runtime,
    dismissed: bool,
) -> Result<Option<Proposal>> {
    use arcade_link::install::{platform::macos, strings, State};
    let Some(bundle) = macos::bundle_for(&runtime.executable) else {
        return Ok(None);
    };
    let detection = arcade_link::install::detect(environment, "arcade.tools", runtime)?;
    if detection.state == State::DevBuild {
        return Ok(None);
    }
    if macos::in_applications(environment, &bundle) {
        if detection.receipt.is_none() {
            environment
                .receipts()
                .write(&macos_receipt(environment, &bundle, None, "stable")?)?;
        }
        return Ok(None);
    }
    let needs_move = macos::is_translocated(&bundle)
        || bundle.starts_with(environment.home.join("Downloads"))
        || bundle.starts_with("/Volumes")
        || std::fs::metadata(&bundle)?.permissions().readonly();
    let args = std::env::args().collect::<Vec<_>>();
    if !needs_move || !arcade_link::install::should_prompt(&detection, &args, false, dismissed) {
        return Ok(None);
    }
    let dialog = &strings::get().macos;
    let text = |t: &str| strings::format(t, "Arcade Tools", env!("CARGO_PKG_VERSION"), "", "");
    Ok(Some(Proposal {
        title: text(&dialog.title),
        copy: dialog.body.as_deref().map(text).unwrap_or_default(),
        installed: detection.installed_path.is_some(),
        primary: dialog.buttons[0].clone(),
        secondary: dialog.buttons[1].clone(),
        options: false,
    }))
}
/// The Linux AppImage prompt. Windows copies come from the setup program.
#[cfg(not(target_os = "macos"))]
fn proposal(
    environment: &Environment,
    runtime: &Runtime,
    dismissed: bool,
) -> Result<Option<Proposal>> {
    let detection = arcade_link::install::detect(environment, "arcade.tools", runtime)?;
    let args = std::env::args().collect::<Vec<_>>();
    if !arcade_link::install::should_prompt(&detection, &args, false, dismissed) {
        return Ok(None);
    }
    let installed = detection.installed_path.is_some();
    Ok(Some(Proposal {
        title: if installed {
            "Update or open installed Arcade Tools?"
        } else {
            "Install Arcade Tools?"
        }
        .into(),
        copy: if installed {
            format!("An installed copy is registered. This copy is {}. Install it to your apps folder or just run it once.",env!("CARGO_PKG_VERSION"))
        } else {
            "Add Tools to your app launcher and apps folder, with a command-line shortcut.".into()
        },
        installed,
        primary: "Install".into(),
        secondary: "Just run it once".into(),
        options: true,
    }))
}
#[tauri::command]
pub async fn self_install_proposal() -> Result<Option<Proposal>> {
    tauri::async_runtime::spawn_blocking(|| {
        let environment = Environment::discover()?;
        let runtime = Runtime::discover()?;
        let dismiss = Paths::discover()?
            .state()
            .join("install-dismissals")
            .join(dismissal_key(&runtime));
        proposal(&environment, &runtime, dismiss.exists())
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}
#[tauri::command]
pub async fn confirm_self_install(
    app: tauri::AppHandle,
    start_at_login: bool,
    remove_download: bool,
) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let receipt = install_options("stable", start_at_login, remove_download)?;
        let manager = crate::desktop::manager(&app)?;
        arcade_tools_core::installation::claim(&manager.paths, receipt.clone(), None)?;
        arcade_link::client::spawn_detached(
            &arcade_tools_core::installation::executable(&manager.paths, &receipt)
                .to_string_lossy(),
            &["--background".into()],
        )?;
        crate::desktop::announce_restarting(&app);
        app.exit(0);
        Ok(())
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}
#[tauri::command]
pub async fn dismiss_self_install() -> Result<()> {
    tauri::async_runtime::spawn_blocking(|| {
        let paths = Paths::discover()?;
        let file = paths
            .state()
            .join("install-dismissals")
            .join(dismissal_key(&Runtime::discover()?));
        paths.guard(&file)?;
        arcade_link::paths::write_atomic(&file, b"dismissed", true)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::new("worker", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt, Store};
    use std::fs;

    fn receipt(paths: &Paths) -> Receipt {
        Receipt {
            schema: 1,
            id: "arcade.tools".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            channel: "stable".into(),
            method: InstallMethod::WindowsInstaller,
            managed_by: ManagedBy::SelfManaged,
            path: paths.home.join("Apps/Tools/arcade-tools.exe"),
            integration: Integration::default(),
            previous: None,
            installed_at: "2026-10-01T00:00:00Z".into(),
            updated_at: "2026-10-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn windows_native_self_install_uses_its_existing_installer_receipt() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let receipt = receipt(&paths);
        fs::create_dir_all(receipt.path.parent().unwrap()).unwrap();
        fs::write(&receipt.path, "fixture").unwrap();
        let other = receipt.path.with_file_name("other.exe");
        fs::write(&other, "fixture").unwrap();
        assert!(windows_install_receipt(&paths, &receipt.path).is_err());
        Store::new(&paths.link).write(&receipt).unwrap();
        assert_eq!(
            windows_install_receipt(&paths, &receipt.path).unwrap(),
            receipt
        );
        assert!(windows_install_receipt(&paths, &other).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn repair_repoints_a_moved_appimage_and_its_cli() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let environment = Environment::under(root.path());
        let mut receipt = receipt(&paths);
        receipt.method = InstallMethod::Appimage;
        receipt.path = paths.home.join("Applications/Arcade/Arcade-Tools.AppImage");
        let moved = paths.home.join("Applications/Arcade/Moved-Tools.AppImage");
        fs::create_dir_all(moved.parent().unwrap()).unwrap();
        fs::write(&moved, "fixture").unwrap();
        let cli = paths.home.join(".local/bin/arcade-tools");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&receipt.path, &cli).unwrap();
        receipt.integration.cli = Some(cli.clone());
        environment.receipts().write(&receipt).unwrap();
        let repaired = repair_at(&environment, Some(&moved)).unwrap();
        assert_eq!(repaired.path, moved.canonicalize().unwrap());
        assert_eq!(fs::read_link(cli).unwrap(), repaired.path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_staging_leaves_no_arcade_directory() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::under(root.path());
        let source = root.path().join("Downloads/Arcade Tools.app");
        fs::create_dir_all(source.join("Contents/MacOS")).unwrap();
        fs::write(source.join(BUNDLE_EXECUTABLE), "fixture").unwrap();
        let target = root.path().join("Applications/Arcade Tools.app");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        stage_bundle(&environment, &source, &target).unwrap();
        assert!(target.join(BUNDLE_EXECUTABLE).is_file());
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    }
}
