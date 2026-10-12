#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
mod helpers;
mod hotkey;
mod index_service;
mod installation;
mod resident;
mod shortcuts;
mod tray;

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
        if let Err(error) = hotkey::launch_overlay() {
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
    desktop::run(args.iter().any(|a| a == "--background"), false);
}
