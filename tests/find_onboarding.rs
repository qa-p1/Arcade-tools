//! Arcade Find's install, data and login locations, as its packages and its
//! own start-at-login code use them (Arcade-Find feature/find-v0.1:
//! packaging/, crates/find-core/src/paths.rs, crates/arcade-find/src/autostart.rs).
mod support;
use arcade_tools_core::{
    apps,
    paths::Paths,
    platform,
    release::{Arch, Channel, Kind, Os},
    source::Source,
};
use std::path::Path;

#[test]
fn find_metadata_install_data_and_login_paths_match_the_app() {
    assert!(apps::APPS.contains(&apps::FIND));
    assert_eq!(apps::app_name(apps::FIND), "Arcade Find");
    assert!(!apps::app_pitch(apps::FIND).is_empty());
    assert_eq!(
        apps::releases_url(apps::FIND),
        "https://github.com/qa-p1/Arcade-Find/releases"
    );
    let paths = Paths::under(Path::new("/isolated"));
    // AppImage, per-user Inno installer ({localappdata}\Programs\Arcade Find),
    // and the DMG's "Arcade Find.app" with CFBundleExecutable arcade-find.
    assert!(paths
        .executable(apps::FIND, Os::Linux, false)
        .ends_with("home/Applications/Arcade/Arcade-Find.AppImage"));
    assert!(paths
        .executable(apps::FIND, Os::Windows, false)
        .ends_with("local/Programs/Arcade Find/arcade-find.exe"));
    assert!(paths
        .executable(apps::FIND, Os::Macos, false)
        .ends_with("Applications/Arcade Find.app/Contents/MacOS/arcade-find"));
    assert_eq!(
        paths.data_folders(apps::FIND, Os::Linux),
        [
            paths.config.join("arcade-find"),
            paths.data.join("arcade-find")
        ]
    );
    assert_eq!(
        paths.data_folders(apps::FIND, Os::Windows),
        [
            paths.roaming.join("Arcade/Arcade Find"),
            paths.local.join("Arcade/Arcade Find")
        ]
    );
    assert_eq!(
        paths.data_folders(apps::FIND, Os::Macos),
        [paths.home.join("Library/Application Support/Arcade Find")]
    );
    // The same entries Find's own Settings toggle writes.
    assert!(paths
        .autostart(apps::FIND, Os::Linux)
        .ends_with("autostart/arcade-find.desktop"));
    assert!(paths
        .autostart(apps::FIND, Os::Macos)
        .ends_with("LaunchAgents/arcade.find.plist"));
    assert_eq!(platform::windows_run_name(apps::FIND), "ArcadeFind");
    assert!(platform::login_supported(apps::FIND, Os::Macos));
    let request = arcade_link::InvokeRequest::new("tools.install", "arcade.find")
        .options(serde_json::json!({"app": apps::FIND}));
    assert_eq!(
        arcade_tools_core::link::install_target(&request).unwrap(),
        apps::FIND
    );
}

#[test]
fn find_release_selects_appimage_inno_and_universal_dmg() {
    let server = support::FakeRelease::new();
    server.publish(
        apps::FIND,
        "0.1.0",
        "stable",
        &[
            ("Arcade-Find-x86_64.AppImage", b"appimage"),
            ("Arcade-Find-0.1.0-x64-setup.exe", b"inno"),
            ("Arcade-Find-0.1.0-universal.dmg", b"dmg"),
        ],
        "inno",
    );
    let source = Source::test_server(&server.url).unwrap();
    let linux = source
        .check(apps::FIND, Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    assert_eq!(linux.asset.kind, Kind::Appimage);
    let windows = source
        .check(apps::FIND, Channel::Stable, Os::Windows, Arch::X64)
        .unwrap();
    assert_eq!(windows.asset.kind, Kind::Inno);
    let mac = source
        .check(apps::FIND, Channel::Stable, Os::Macos, Arch::Arm64)
        .unwrap();
    assert_eq!(mac.asset.kind, Kind::Dmg);
}

#[cfg(target_os = "linux")]
#[test]
fn find_login_entry_matches_finds_own() {
    let temp = tempfile::tempdir().unwrap();
    let paths = Paths::under(temp.path());
    let exe = paths.home.join("Applications/Arcade/Arcade-Find.AppImage");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "appimage").unwrap();
    // An entry Find wrote itself is updated in place, keeping its extra keys.
    let login = paths.autostart(apps::FIND, Os::Linux);
    std::fs::create_dir_all(login.parent().unwrap()).unwrap();
    std::fs::write(
        &login,
        format!(
            "[Desktop Entry]\nType=Application\nName=Arcade Find\nExec=\"{}\" --background\nIcon=arcade-find\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        ),
    )
    .unwrap();
    assert!(platform::autostart_enabled(&paths, apps::FIND));
    platform::set_autostart(&paths, apps::FIND, &exe, true).unwrap();
    let entry = std::fs::read_to_string(&login).unwrap();
    assert!(
        entry.contains(&format!("Exec=\"{}\" --background", exe.display())),
        "{entry}"
    );
    assert!(entry.contains("Icon=arcade-find"), "{entry}");
    platform::set_autostart(&paths, apps::FIND, &exe, false).unwrap();
    assert!(!platform::autostart_enabled(&paths, apps::FIND));
}
