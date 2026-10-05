#![cfg(target_os = "linux")]
mod support;
use arcade_tools_core::manager::{LaunchMode, Manager, Operation, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::{Channel, Os};
use arcade_tools_core::source::Source;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use support::FakeRelease;

fn require_isolated_runner() {
    let home = std::env::var("HOME").unwrap();
    assert!(
        home.starts_with("/tmp/") && std::env::var_os("ARCADE_HOME").is_some(),
        "Run lifecycle tests through Arcade-link/tools/e2e.py run -- cargo test"
    );
    assert!(std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none());
}
fn request(id: &str, operation: Operation) -> Request {
    Request {
        id: id.into(),
        operation,
        mode: Some(LaunchMode::Background),
        remove_data: false,
        app_closed: false,
    }
}
fn app(server: &FakeRelease, version: &str) -> Vec<u8> {
    format!(
        "#!/bin/sh\nprintf '%s\\n' '{}:'\"$*\" >> '{}'\n",
        version,
        server.root.path().join("launches").display()
    )
    .into_bytes()
}
fn wait_for(path: &std::path::Path, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if fs::read_to_string(path).is_ok_and(|s| s.contains(text)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{} never contained {text}", path.display());
}

#[test]
fn appimage_install_update_repair_uninstall_keeps_then_removes_data() {
    require_isolated_runner();
    let server = FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("user"));
    let manager = Manager::new(paths.clone(), Source::test_server(&server.url).unwrap());
    let cancel = AtomicBool::new(false);
    let mut phases = Vec::new();
    assert!(manager
        .list(true)
        .unwrap()
        .iter()
        .all(|a| !a.installed && !a.running));
    server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[("Look_x64.AppImage", &app(&server, "1"))],
        "nsis",
    );
    manager
        .operate(
            &request("arcade.look", Operation::Install),
            &cancel,
            &mut |p| phases.push(p.phase),
        )
        .unwrap();
    let exe = paths.executable("arcade.look", Os::Linux, false);
    assert!(exe.is_file());
    wait_for(&server.root.path().join("launches"), "1:--background");
    let data = paths.data_folders("arcade.look", Os::Linux)[0].clone();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("settings.json"), "keep me").unwrap();
    manager.start_at_login("arcade.look", true).unwrap();
    let login = paths.autostart("arcade.look", Os::Linux);
    assert!(fs::read_to_string(&login)
        .unwrap()
        .contains(&exe.display().to_string()));
    manager.start_at_login("arcade.look", false).unwrap();
    assert!(!login.exists());
    assert!(login
        .with_file_name("arcade-look.desktop.arcade-tools.bak")
        .exists());
    server.publish(
        "arcade.look",
        "2.0.0",
        "stable",
        &[("Look_x64.AppImage", &app(&server, "2"))],
        "nsis",
    );
    assert!(manager.check("arcade.look").unwrap().update_available);
    manager
        .operate(
            &request("arcade.look", Operation::Update),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().version,
        "2.0.0"
    );
    fs::remove_file(&exe).unwrap();
    assert!(
        !manager
            .list(false)
            .unwrap()
            .into_iter()
            .find(|a| a.id == "arcade.look")
            .unwrap()
            .healthy
    );
    manager
        .operate(
            &request("arcade.look", Operation::Repair),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    assert!(exe.is_file());
    manager
        .operate(
            &request("arcade.look", Operation::Uninstall),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    assert!(!exe.exists() && !paths.link.manifest("arcade.look").exists());
    assert!(data.join("settings.json").exists());
    manager
        .operate(
            &request("arcade.look", Operation::Install),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    let mut remove = request("arcade.look", Operation::Uninstall);
    remove.remove_data = true;
    manager.operate(&remove, &cancel, &mut |_| {}).unwrap();
    assert!(!data.exists());
    assert_eq!(&phases[..2], ["Checking release", "Downloading"]);
    println!("local HTTP AppImage install → update → missing-file repair → uninstall (keep/remove data); isolated autostart backup/validation: PASS");
}

#[test]
fn checksum_failure_cancel_and_disabled_peer_never_replace_installed_files() {
    require_isolated_runner();
    let server = FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("user"));
    let manager = Manager::new(paths.clone(), Source::test_server(&server.url).unwrap());
    let cancel = AtomicBool::new(false);
    server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[("Look_x64.AppImage", &app(&server, "original"))],
        "nsis",
    );
    manager
        .operate(
            &request("arcade.look", Operation::Install),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    let exe = paths.executable("arcade.look", Os::Linux, false);
    let original = fs::read(&exe).unwrap();
    server.publish(
        "arcade.look",
        "2.0.0",
        "stable",
        &[("Look_x64.AppImage", &app(&server, "new"))],
        "nsis",
    );
    let route = "/assets/arcade-look-2.0.0-stable/Look_x64.AppImage";
    let good = server.routes.lock().unwrap().get(route).unwrap().clone();
    server
        .routes
        .lock()
        .unwrap()
        .insert(route.into(), b"corrupt".to_vec());
    assert_eq!(
        manager
            .operate(
                &request("arcade.look", Operation::Update),
                &cancel,
                &mut |_| {}
            )
            .unwrap_err()
            .code,
        "checksum"
    );
    assert_eq!(fs::read(&exe).unwrap(), original);
    server.routes.lock().unwrap().insert(route.into(), good);
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        manager
            .operate(
                &request("arcade.look", Operation::Update),
                &cancelled,
                &mut |_| {}
            )
            .unwrap_err()
            .code,
        "cancelled"
    );
    let mut manifest = arcade_link::Manifest::new("arcade.look", "1", exe.to_str().unwrap());
    manifest.settings.link_enabled = false;
    arcade_link::manifest::write_manifest(&paths.link, &manifest).unwrap();
    assert_eq!(
        manager
            .operate(
                &request("arcade.look", Operation::Update),
                &cancel,
                &mut |_| {}
            )
            .unwrap_err()
            .code,
        "disconnected"
    );
    assert_eq!(fs::read(&exe).unwrap(), original);
    let mut closed = request("arcade.look", Operation::Update);
    closed.app_closed = true;
    manager.operate(&closed, &cancel, &mut |_| {}).unwrap();
    println!("bad SHA, cancel and disconnected peer leave installed bytes intact; explicitly closed peer permits update: PASS");
}

#[test]
fn clipboard_tarball_runs_the_verified_install_script_and_repairs() {
    require_isolated_runner();
    let server = FakeRelease::new();
    let paths = Paths::under(&server.root.path().join("user"));
    let manager = Manager::new(paths.clone(), Source::test_server(&server.url).unwrap());
    let make_package = |version: &str, failing: bool| {
        let stage = tempfile::tempdir().unwrap();
        let package = stage.path().join("clipboard-release");
        let bundle = package.join("apps/flutter_app/build/linux/x64/release/bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::create_dir_all(package.join("scripts")).unwrap();
        fs::write(bundle.join("clipboard"), app(&server, version)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bundle.join("clipboard"), fs::Permissions::from_mode(0o755)).unwrap();
        let script = if failing {
            "#!/bin/bash\nexit 7\n".to_string()
        } else {
            "#!/bin/bash\nset -eu\nroot=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\nmkdir -p \"$XDG_DATA_HOME/arcade-clipboard\"\ncp -a \"$root/apps/flutter_app/build/linux/x64/release/bundle/.\" \"$XDG_DATA_HOME/arcade-clipboard/\"\n".to_string()
        };
        fs::write(package.join("scripts/install-linux.sh"), script).unwrap();
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        tar.append_dir_all("clipboard-release", &package).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    };
    server.publish(
        "arcade.clipboard",
        "1.0.0",
        "stable",
        &[("clipboard-linux-x64.tar.gz", &make_package("clip1", false))],
        "inno",
    );
    manager
        .operate(
            &request("arcade.clipboard", Operation::Install),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let exe = paths.executable("arcade.clipboard", Os::Linux, true);
    let original = fs::read(&exe).unwrap();
    server.publish(
        "arcade.clipboard",
        "2.0.0",
        "stable",
        &[("clipboard-linux-x64.tar.gz", &make_package("clip2", true))],
        "inno",
    );
    assert_eq!(
        manager
            .operate(
                &request("arcade.clipboard", Operation::Update),
                &AtomicBool::new(false),
                &mut |_| {}
            )
            .unwrap_err()
            .code,
        "installer"
    );
    assert_eq!(fs::read(&exe).unwrap(), original);
    assert_eq!(
        manager.record("arcade.clipboard").unwrap().unwrap().version,
        "1.0.0"
    );
    server.publish(
        "arcade.clipboard",
        "2.0.0",
        "stable",
        &[("clipboard-linux-x64.tar.gz", &make_package("clip2", false))],
        "inno",
    );
    manager
        .operate(
            &request("arcade.clipboard", Operation::Repair),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_ne!(fs::read(&exe).unwrap(), original);
    manager
        .operate(
            &request("arcade.clipboard", Operation::Uninstall),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert!(!exe.exists());
    println!("verified Clipboard tarball install script + failing-script rollback + repair/removal: PASS");
}

#[test]
fn stable_nightly_channel_switch_and_unknown_apps() {
    require_isolated_runner();
    let server = FakeRelease::new();
    let manager = Manager::new(
        Paths::under(&server.root.path().join("user")),
        Source::test_server(&server.url).unwrap(),
    );
    server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[("Look_x64.AppImage", &app(&server, "stable"))],
        "nsis",
    );
    server.publish(
        "arcade.look",
        "1.1.0-nightly",
        "nightly",
        &[("Look_x64.AppImage", &app(&server, "nightly"))],
        "nsis",
    );
    manager
        .operate(
            &request("arcade.look", Operation::Install),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    manager
        .set_channel("arcade.look", Channel::Nightly)
        .unwrap();
    assert_eq!(
        manager.check("arcade.look").unwrap().channel,
        Channel::Nightly
    );
    manager
        .operate(
            &request("arcade.look", Operation::Update),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().channel,
        Channel::Nightly
    );
    assert!(manager
        .set_channel("../../unsafe", Channel::Stable)
        .is_err());
}
