#![cfg(target_os = "linux")]
//! `ARCADE_TOOLS_CHANNEL_OVERRIDE=test`, the release gates' way to install rc releases through Tools (DECISIONS.md
//! D5). A test binary of its own because the override is process-wide.
mod support;
use arcade_tools_core::manager::Operation;
use arcade_tools_core::release::{Arch, Channel, Os};
use arcade_tools_core::source::Source;
use serde_json::json;
use support::hub::{fake_app, fixture, installed, operate};

const OVERRIDE: &str = "ARCADE_TOOLS_CHANNEL_OVERRIDE";
/// The override is process-wide and the tests here run on parallel threads: one at a time.
static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_test_channel_is_followed_only_while_the_override_is_set() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (server, manager) = fixture();
    let id = "arcade.look";
    installed(&server, &manager, id);
    // Test is never stored: not from the UI or Link, and not from a backup.
    assert_eq!(
        manager.set_channel(id, Channel::Test).unwrap_err().code,
        "invalid"
    );
    let rc = "1.1.0-rc.1";
    server.publish(
        id,
        rc,
        "test",
        &[("App_x64.AppImage", &fake_app(&manager.paths, id, rc, false))],
        "nsis",
    );
    server.amend_manifest(id, rc, "test", |manifest| {
        manifest["assets"][0]["installArgs"] = json!(["--install", "--silent"]);
    });
    let check = manager.check(id).unwrap();
    assert_eq!(
        (check.version.as_str(), check.channel),
        ("1.0.0", Channel::Stable)
    );
    assert!(!check.update_available);

    std::env::set_var(OVERRIDE, "test");
    let check = manager.check(id).unwrap();
    assert_eq!((check.version.as_str(), check.channel), (rc, Channel::Test));
    assert!(check.update_available);
    operate(&manager, id, Operation::Update).unwrap();
    assert_eq!(manager.record(id).unwrap().unwrap().version, rc);
    assert!(!manager.check(id).unwrap().update_available);

    // Without the override the rc's record is followed as Stable: the rc's unsigned manifest is never trusted again.
    std::env::remove_var(OVERRIDE);
    let check = manager.check(id).unwrap();
    assert_eq!(
        (check.version.as_str(), check.channel),
        ("1.0.0", Channel::Stable)
    );

    // A production source refuses the Test channel outright unless the override is set; nothing is requested.
    let Err(error) = Source::github().unwrap().check_repo_conditional(
        id,
        "qa-p1/Arcade-look",
        Channel::Test,
        Os::Linux,
        Arch::X64,
        None,
        &[],
    ) else {
        panic!("the Test channel needs the override");
    };
    assert_eq!(error.code, "invalid");
}

/// Every rc of a release carries the same version (Tools' rcs are all "1.0.0", promoted byte for byte), so only the
/// package hash tells one from the next. An AppImage that installed itself (Tools from the downloaded file) has a
/// receipt but no record of the package it came from: its executable is that package, byte for byte, so its hash is
/// the package hash. Without it an installed rc never saw the next rc as an update (Gates on rc.6, Tools D16).
#[test]
fn a_self_installed_appimage_sees_the_next_rc_of_the_same_version() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (server, manager) = fixture();
    let id = "arcade.look";
    installed(&server, &manager, id);
    // What a self-install leaves: the receipt and the executable, no record of the package.
    std::fs::remove_file(manager.paths.state().join(format!("{id}.json"))).unwrap();
    std::env::set_var(OVERRIDE, "test");

    // The same bytes again under the test channel: nothing to do.
    let same = fake_app(&manager.paths, id, "1.0.0", false);
    server.publish(id, "1.0.0", "test", &[("App_x64.AppImage", &same)], "nsis");
    server.amend_manifest(id, "1.0.0", "test", |manifest| {
        manifest["assets"][0]["installArgs"] = json!(["--install", "--silent"]);
    });
    let check = manager.check(id).unwrap();
    assert_eq!(
        (check.version.as_str(), check.channel),
        ("1.0.0", Channel::Test)
    );
    assert!(
        !check.update_available,
        "the installed package is the published one"
    );

    // The next rc: the same version, other bytes.
    let mut next = same.clone();
    next.extend_from_slice(b"# the next rc\n");
    server.publish(id, "1.0.0", "test", &[("App_x64.AppImage", &next)], "nsis");
    server.amend_manifest(id, "1.0.0", "test", |manifest| {
        manifest["assets"][0]["installArgs"] = json!(["--install", "--silent"]);
    });
    assert!(manager.check(id).unwrap().update_available);
    let (apps, _) = manager.update_inputs().unwrap();
    let look = apps.iter().find(|app| app.id == id).unwrap();
    assert!(
        look.sha256.is_some(),
        "the executable's hash stands for the package's"
    );

    operate(&manager, id, Operation::Update).unwrap();
    assert!(!manager.check(id).unwrap().update_available);
    std::env::remove_var(OVERRIDE);
}
