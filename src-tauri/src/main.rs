#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
use arcade_tools_core::link;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Arcade Tools {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--arcade-manifest") {
        println!("{}", link::manifest(true).to_json());
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
