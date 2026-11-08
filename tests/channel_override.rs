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

#[test]
fn the_test_channel_is_followed_only_while_the_override_is_set() {
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
