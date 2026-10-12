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
// Self-install is the Linux AppImage path; Windows and macOS use the
// platform installer, which writes the receipt itself.
#[cfg(not(target_os = "linux"))]
fn install_options(
    _channel: &str,
    _start_at_login: bool,
    _remove_download: bool,
) -> Result<arcade_link::receipt::Receipt> {
    Err(unsupported())
}
#[cfg(not(target_os = "linux"))]
fn unsupported() -> Error {
    Error::new(
        "unsupported",
        "Install Arcade Tools with its Windows installer or macOS app bundle.",
    )
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
    arcade_link::manifest::write_manifest(
        &environment.locations,
        &arcade_link::Manifest::new(
            "arcade.tools",
            env!("CARGO_PKG_VERSION"),
            &arcade_tools_core::installation::executable(&Paths::discover()?, &receipt)
                .to_string_lossy(),
        ),
    )?;
    Ok(receipt)
}
#[cfg(target_os = "linux")]
fn repair(environment: &Environment) -> Result<arcade_link::receipt::Receipt> {
    Ok(arcade_link::install::repair(environment, &info(), None)?)
}
#[cfg(not(target_os = "linux"))]
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
                arcade_link::install::uninstall(
                    &environment,
                    "arcade.tools",
                    UninstallOptions {
                        remove_data: args.iter().any(|arg| arg == "--remove-data"),
                        data_folders: &folders,
                    },
                )?;
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
}
fn dismissal_key(runtime: &Runtime) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    runtime.running_from().hash(&mut hash);
    format!("{:016x}", hash.finish())
}
#[tauri::command]
pub async fn self_install_proposal() -> Result<Option<Proposal>> {
    tauri::async_runtime::spawn_blocking(|| {
        let environment=Environment::discover()?;
        let runtime=Runtime::discover()?;
        let detection=arcade_link::install::detect(&environment,"arcade.tools",&runtime)?;
        let dismiss=Paths::discover()?.state().join("install-dismissals").join(dismissal_key(&runtime));
        let args=std::env::args().collect::<Vec<_>>();
        if !arcade_link::install::should_prompt(&detection,&args,false,dismiss.exists()) { return Ok(None); }
        let installed=detection.installed_path.is_some();
        Ok(Some(Proposal { title: if installed { "Update or open installed Arcade Tools?" } else { "Install Arcade Tools?" }.into(), copy: if installed { format!("An installed copy is registered. This copy is {}. Install it to your apps folder or just run it once.",env!("CARGO_PKG_VERSION")) } else { "Add Tools to your app launcher and apps folder, with a command-line shortcut.".into() },installed }))
    }).await.map_err(|e|Error::new("worker",e.to_string()))?
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
