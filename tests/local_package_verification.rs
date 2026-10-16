#![cfg(target_os = "linux")]
#[path = "/home/harneet/Work/Coding/Rust/Arcade-tools-wt-hub/tests/support/mod.rs"]
mod support;
use arcade_tools_core::{manager::{Manager, Operation, Request}, paths::Paths, platform::CommandSpec, source::Source};
use arcade_link::receipt::ManagedBy;
use std::{fs, path::Path, sync::atomic::AtomicBool};

#[test]
fn real_appimages_standalone_adopt_update_rollback_and_uninstall() {
    assert_eq!(std::env::var("ARCADE_E2E_INNER").as_deref(), Ok("1"));
    std::env::set_var("APPIMAGE_EXTRACT_AND_RUN", "1");
    for (id, version, source) in [
        ("arcade.find", "0.3.0", "/home/harneet/Work/Coding/Rust/Arcade-Find-wt-hub/dist/Arcade-Find-x86_64.AppImage"),
        ("arcade.clipboard", "0.1.0", "/home/harneet/Work/Coding/App dev/Arcade-clipboard-wt-hub/dist/Arcade-Clipboard-0.1.0-x86_64.AppImage"),
    ] {
        let server = support::FakeRelease::new();
        let paths = Paths::under(&server.root.path().join("user"));
        let manager = Manager::new(paths.clone(), Source::test_server(&server.url).unwrap());
        let download = server.root.path().join("Download.AppImage");
        fs::copy(source, &download).unwrap();
        CommandSpec { program: download, args: ["--install", "--silent", "--no-launch"].map(String::from).to_vec() }.run(&paths).unwrap();
        let receipt = manager.receipt(id).unwrap().unwrap();
        assert_eq!(receipt.version, version);
        assert_eq!(receipt.managed_by, ManagedBy::SelfManaged);
        let original = fs::read(&receipt.path).unwrap();
        let request = |operation| Request { id: id.into(), operation, mode: None, remove_data: false, app_closed: true };
        let operate = |operation| manager.operate(&request(operation), &AtomicBool::new(false), &mut |_| {}).unwrap();
        operate(Operation::Adopt);
        assert_eq!(manager.receipt(id).unwrap().unwrap().managed_by, ManagedBy::Tools);
        let mut updated = original.clone();
        // AppImage runtime uses the filesystem's own length. Extra trailing
        // bytes provide a distinct verified package of the same build.
        updated.extend_from_slice(b"\nArcade lifecycle verification build\n");
        let release = format!("{version}+verification.1");
        server.publish(id, &release, "stable", &[("App_x64.AppImage", &updated)], "nsis");
        server.amend_manifest(id, &release, "stable", |manifest| manifest["assets"][0]["installArgs"] = serde_json::json!(["--install", "--silent"]));
        operate(Operation::Update);
        assert_eq!(fs::read(&receipt.path).unwrap(), updated);
        assert!(manager.rollback_version(id).unwrap().is_some());
        operate(Operation::Rollback);
        assert_eq!(fs::read(&receipt.path).unwrap(), original);
        let owned = manager.receipt(id).unwrap().unwrap();
        operate(Operation::Uninstall);
        assert!(manager.receipt(id).unwrap().is_none());
        assert!(!receipt.path.exists());
        for path in owned.integration.cli.iter().chain(owned.integration.autostart.iter()).chain(owned.integration.desktop_entry.iter()).chain(owned.integration.icons.iter()) {
            assert!(!path.exists() && fs::symlink_metadata(path).is_err(), "retained {}", path.display());
        }
        println!("PASS {id}: real AppImage standalone -> adoption -> verified update -> rollback -> uninstall");
        assert!(Path::new(source).is_file());
    }
}
