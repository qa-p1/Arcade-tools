//! On-demand launcher. The Windows GUI subsystem follows Arcade Find
//! (MIT OR Apache-2.0), avoiding a console window when Tools opens the sheet.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Instant;

use arcade_tools_overlay::{
    focus, index,
    instance::{Command, Instance},
    model::Model,
    timing::Timing,
    ui,
};

fn main() {
    let timing = Timing::start();
    if let Err(error) = run(timing) {
        eprintln!("arcade-tools-overlay: {error}");
        std::process::exit(1);
    }
}

fn run(timing: Timing) -> Result<(), String> {
    let mut command = None;
    let mut path = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let next = match arg.to_str() {
            Some("--toggle") => Some(Command::Toggle),
            Some("--show") => Some(Command::Show),
            Some("--hide") => Some(Command::Hide),
            Some("--index") => {
                path = Some(PathBuf::from(args.next().ok_or("--index needs a path")?));
                None
            }
            Some("--version") => {
                println!("arcade-tools-overlay {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--help") | Some("-h") => {
                println!("arcade-tools-overlay --toggle | --show | --hide [--index PATH]\narcade-tools-overlay --version");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        };
        if let Some(next) = next {
            if command.replace(next).is_some() {
                return Err("choose only one of --toggle, --show or --hide".into());
            }
        }
    }
    let command = command.ok_or("use --toggle, --show or --hide (see --help)")?;
    let began = Instant::now();
    let directory =
        arcade_tools_overlay::instance::runtime_directory().map_err(|e| e.to_string())?;
    let instance = match Instance::acquire(directory.clone()).map_err(|e| e.to_string())? {
        Some(instance) => instance,
        None => {
            return arcade_tools_overlay::instance::send(&directory, command)
                .map_err(|e| e.to_string())
        }
    };
    timing.phase("instance", began);
    if command == Command::Hide {
        return Ok(());
    }
    let path = path
        .map(Ok)
        .unwrap_or_else(index::default_path)
        .map_err(|e| e.to_string())?;
    let load_timing = timing.clone();
    let loader = std::thread::Builder::new()
        .name("overlay-index".into())
        .spawn(move || {
            let began = Instant::now();
            let index = match index::Index::load(&path) {
                Ok(mut index) => {
                    index::apply_saved_labels(&mut index);
                    Some(index)
                }
                Err(error) => {
                    eprintln!("arcade-tools-overlay: index {}: {error}", path.display());
                    None
                }
            };
            let dark = ui::dark_preference();
            load_timing.phase("index-theme", began);
            (index, dark)
        })
        .map_err(|e| e.to_string())?;
    let began = Instant::now();
    let focus = focus::detect();
    timing.phase("focus", began);
    let (index, dark) = loader.join().map_err(|_| "index loader failed")?;
    let app = index.as_ref().and_then(|index| {
        focus
            .identity
            .as_ref()
            .and_then(|identity| index.match_app(identity, focus.kind))
    });
    ui::run(Model::new(index, app), focus, &instance, timing, dark)
}
