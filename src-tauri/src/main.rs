#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
mod helpers;
mod hotkey;
mod index_service;
mod installation;
mod resident;
mod shortcuts;
mod tray;

const USAGE: &str = "Usage: arcade-tools [OPTION]

Without options, opens the Arcade Tools window.

  --background            Start in the tray without opening the window
  --settings              Open the Arcade Tools window at Settings
  --quit                  Quit the running Arcade Tools
  --restart               Restart the running Arcade Tools
  --show-shortcuts        Show the shortcuts overlay
  --install [--channel stable|nightly] [--start-at-login] [--no-launch] [--keep-download]
                          Install this copy for the current user. An image in
                          Downloads is removed afterwards unless --keep-download.
  --repair                Recreate the launcher, command and autostart entries
  --uninstall [--silent [--remove-data]]
                          Review removing Arcade Tools (and optionally its apps)
                          in the window; --silent removes Tools without asking
                          and leaves the apps installed, updating themselves
  --integration-status    Print the installation state as JSON
  --status                Print whether Arcade Tools is running, as one line of JSON
  -V, --version           Print the version
  -h, --help              Print this help
";

fn main() {
    arcade_link::childenv::on_startup();
    // Tools takes no file paths, and the Uninstall review, Settings and the
    // tray all open webviews: WebKit needs the AppImage's own directory.
    arcade_link::childenv::enter_image_dir();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return;
    }
    if let Err(arg) = check_args(&args[1..]) {
        eprint!("arcade-tools: unknown argument {arg:?}\n\n{USAGE}");
        std::process::exit(2);
    }
    if args.iter().any(|a| a == "--status") {
        std::process::exit(print_status());
    }
    if let Some(index) = args.iter().position(|arg| arg == "--wait-for-process") {
        let result = args
            .get(index + 1)
            .and_then(|pid| pid.parse::<u32>().ok())
            .ok_or_else(|| arcade_tools_core::Error::new("usage", "Provide a predecessor PID."))
            .and_then(arcade_tools_core::manager::wait_for_exit);
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    prepare_folders(&args);
    if args.iter().any(|arg| arg == "--restart") {
        let result = arcade_link::client::Client::connect(
            &arcade_link::Locations::discover(),
            "arcade.tools",
            &arcade_tools_core::manager::me(),
        )
        .and_then(|mut client| {
            client
                .call("app.restart", serde_json::json!({}))
                .map(|_| ())
        });
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = helpers::run(&args).or_else(|| installation::run(&args)) {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Arcade Tools {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--arcade-manifest") {
        println!("{}", tray::hosting::manifest_from_disk().to_json());
        return;
    }
    if args.iter().any(|a| a == "--show-shortcuts") {
        if let Err(error) = hotkey::run_overlay() {
            eprintln!("{}", error.message);
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--quit") {
        let result = arcade_link::client::Client::connect(
            &arcade_link::paths::Locations::discover(),
            "arcade.tools",
            &arcade_tools_core::manager::me(),
        )
        .and_then(|mut client| {
            if client
                .status()?
                .pointer("/status/busy")
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                return Err(arcade_link::LinkError::busy());
            }
            client.call("app.quit", serde_json::json!({})).map(|_| ())
        });
        if let Err(error) = result {
            if error.code == arcade_link::ErrorCode::NotRunning {
                // The native single-instance channel remains available when
                // the user disables Arcade Link in Tools.
                desktop::run(true, true);
                return;
            }
            eprintln!("{}", error.user_message("Arcade Tools"));
            std::process::exit(1);
        }
        return;
    }
    installation::make_way_for_offer();
    desktop::run(args.iter().any(|a| a == "--background"), false);
}

/// Every argument Tools accepts, including the ones its own helpers and updaters pass (`--finish-self-*`,
/// `--wait-for-process`); the second list takes a value.
const FLAGS: &[&str] = &[
    "--background",
    "--settings",
    "--quit",
    "--restart",
    "--show-shortcuts",
    "--install",
    "--start-at-login",
    "--no-launch",
    "--keep-download",
    "--repair",
    "--uninstall",
    "--silent",
    "--yes",
    "--remove-data",
    "--integration-status",
    "--status",
    "--version",
    "-V",
    "--arcade-manifest",
];
const VALUE_FLAGS: &[&str] = &[
    "--channel",
    "--wait-for-process",
    "--finish-self-update",
    "--finish-self-rollback",
    "--finish-self-uninstall",
];

/// ARCADE_APP.md CLI-2: anything else is a mistake, reported before Tools starts or hands it to a running copy.
fn check_args(args: &[String]) -> Result<(), String> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if VALUE_FLAGS.contains(&arg.as_str()) {
            rest.next().ok_or_else(|| arg.clone())?;
        } else if !FLAGS.contains(&arg.as_str()) && !arg.starts_with("-psn_") {
            // -psn_* is added by macOS when Finder starts an app bundle.
            return Err(arg.clone());
        }
    }
    Ok(())
}

/// ARCADE_APP.md CLI-1: one line of JSON, exit 0 when Tools runs and 3 (LSB "not running") otherwise. Asks the
/// running Tools over Link and starts or registers nothing.
fn print_status() -> i32 {
    let me = arcade_tools_core::manager::me();
    let answer = arcade_link::client::Client::connect(
        &arcade_link::Locations::discover(),
        "arcade.tools",
        &me,
    )
    .and_then(|mut client| client.status());
    let (running, status) = match answer {
        Ok(status) => (true, status),
        Err(_) => (false, serde_json::Value::Null),
    };
    let line = serde_json::json!({
        "app": "arcade.tools",
        "version": env!("CARGO_PKG_VERSION"),
        "running": running,
        "status": status,
    });
    println!("{line}");
    if running {
        0
    } else {
        3
    }
}

/// Sends otherwise-lost stderr to the log folder, then moves Tools' 0.3
/// folders (`arcade-tools`, `app.arcadetools`) into
/// `arcade/tools` before anything reads them or a webview opens. The move is
/// skipped while
/// another Tools answers, so files never move under a running older copy; the
/// next start does it. Nothing is lost either way: the move copies, verifies,
/// and only then removes.
fn prepare_folders(args: &[String]) {
    // A legacy updater may be running this installer from its old cache and
    // owns a live transaction there. Move neither until the resident starts.
    if !may_migrate_folders(args) {
        return;
    }
    let Ok(paths) = arcade_tools_core::paths::Paths::discover() else {
        return;
    };
    // Started from the launcher, at login or by an update, stderr would be lost.
    let _ = arcade_link::logs::capture_stderr(&paths.logs(), "arcade-tools");
    if arcade_link::client::Client::connect(
        &paths.link,
        "arcade.tools",
        &arcade_tools_core::manager::me(),
    )
    .is_ok()
    {
        return;
    }
    let migration = paths.migrate_own_folders();
    for (from, to) in &migration.moved {
        eprintln!("Moved {} to {}", from.display(), to.display());
    }
    for (from, reason) in &migration.kept {
        eprintln!("Left {} in place: {reason}", from.display());
    }
}

fn may_migrate_folders(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--install"
                | "--repair"
                | "--uninstall"
                | "--integration-status"
                | "--finish-self-update"
                | "--finish-self-rollback"
                | "--finish-self-uninstall"
                | "--version"
                | "-V"
                | "--arcade-manifest"
        )
    })
}

#[cfg(test)]
mod startup_tests {
    use super::may_migrate_folders;

    #[test]
    fn migration_waits_until_installers_and_helpers_finish() {
        for flag in [
            "--install",
            "--repair",
            "--uninstall",
            "--integration-status",
            "--finish-self-update",
            "--finish-self-rollback",
            "--finish-self-uninstall",
        ] {
            assert!(
                !may_migrate_folders(&["arcade-tools".into(), flag.into()]),
                "{flag}"
            );
        }
        for flag in ["--background", "--settings"] {
            assert!(may_migrate_folders(&["arcade-tools".into(), flag.into()]));
        }
        assert!(may_migrate_folders(&["arcade-tools".into()]));
    }
}
