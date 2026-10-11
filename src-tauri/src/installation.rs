use arcade_link::install::{
    AppInfo, Environment, Icon, IconSize, InstallOptions, Runtime, UninstallOptions,
};
use arcade_link::receipt::ManagedBy;
use arcade_tools_core::paths::Paths;
use arcade_tools_core::{Error, Result};

fn info() -> AppInfo {
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
            remove_download: false,
            refresh_caches: true,
        },
    )?;
    arcade_link::manifest::write_manifest(
        &environment.locations,
        &arcade_link::Manifest::new(
            "arcade.tools",
            env!("CARGO_PKG_VERSION"),
            receipt.path.to_str().unwrap_or_default(),
        ),
    )?;
    Ok(receipt)
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
                println!(
                    "{}",
                    serde_json::to_string(&install(
                        channel,
                        args.iter().any(|arg| arg == "--start-at-login")
                    )?)?
                );
            }
            "--repair" => {
                println!(
                    "{}",
                    serde_json::to_string(&arcade_link::install::repair(
                        &environment,
                        &info(),
                        None
                    )?)?
                );
            }
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
