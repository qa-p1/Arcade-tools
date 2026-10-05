use arcade_tools_core::paths::Paths;
use arcade_tools_core::platform::{
    desktop_entry, dmg_attach, launch_agent, windows_install, windows_uninstall,
};
use arcade_tools_core::release::{Arch, Asset, Kind, Os};
use std::path::Path;

#[test]
fn windows_installer_flags_and_paths_are_per_user_arguments() {
    let paths = Paths::under(Path::new("/tmp/windows-logic"));
    let root = paths.install_root("arcade.look", Os::Windows, false);
    assert_eq!(
        root,
        Path::new("/tmp/windows-logic/local/Programs/Arcade Look")
    );
    let mut asset = Asset {
        os: Os::Windows,
        arch: Arch::X64,
        kind: Kind::Nsis,
        file: "Look.exe".into(),
        sha256: "0".repeat(64),
        size: None,
        silent: Some(vec!["/S".into()]),
    };
    let plan = windows_install(&asset, Path::new("verified.exe"), &root).unwrap();
    assert_eq!(
        plan.args,
        ["/S", "/D=/tmp/windows-logic/local/Programs/Arcade Look"]
    );
    asset.kind = Kind::Inno;
    asset.silent = Some(
        [
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CURRENTUSER",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    );
    let plan = windows_install(&asset, Path::new("verified.exe"), &root).unwrap();
    assert_eq!(plan.args[3], "/CURRENTUSER");
    assert!(plan.args[4].starts_with("/DIR="));
    asset.silent = Some(vec!["/ALLUSERS".into()]);
    assert!(windows_install(&asset, Path::new("verified.exe"), &root).is_err());
    assert!(windows_uninstall(Kind::Inno, &root)
        .unwrap()
        .program
        .ends_with("unins000.exe"));
    assert!(windows_uninstall(Kind::Msi, &root).is_err());
}
#[test]
fn macos_plan_keeps_quarantine_and_native_login_items_are_hidden() {
    let paths = Paths::under(Path::new("/tmp/macos-logic"));
    assert!(paths
        .install_root("arcade.look", Os::Macos, false)
        .ends_with("Applications/Arcade Look.app"));
    assert!(paths
        .executable("arcade.clipboard", Os::Macos, false)
        .ends_with("Arcade Clipboard.app/Contents/MacOS/Arcade Clipboard"));
    let plan = dmg_attach(Path::new("verified.dmg"), Path::new("private-mount"));
    assert!(plan.args.contains(&"-readonly".into()) && plan.args.contains(&"-nobrowse".into()));
    let plist = launch_agent(
        "arcade.look",
        Path::new("/Applications/Look & Test.app/Contents/MacOS/look"),
    );
    assert!(plist.contains("Look &amp; Test.app"));
    assert!(!plist.contains("KeepAlive"));
    assert!(!arcade_tools_core::platform::login_supported(
        "arcade.wheel",
        Os::Macos
    ));
    assert!(!arcade_tools_core::platform::login_supported(
        "arcade.clipboard",
        Os::Macos
    ));
}
#[test]
fn traversal_symlinks_and_temporary_login_paths_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = Paths::under(root.path());
    assert!(paths.guard(&paths.home.join("../escape")).is_err());
    let exe = paths.home.join("Applications/app");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "app").unwrap();
    paths.isolated = false;
    assert!(paths.check_persistent_executable(&exe).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path(), paths.home.join("link")).unwrap();
        assert!(paths.guard(&paths.home.join("link/escape")).is_err());
    }
    let desktop = desktop_entry("arcade.look", Path::new("/home/u/Apps/a $b%\".AppImage")).unwrap();
    assert!(desktop.contains("%%"));
    assert!(!desktop.contains("/tmp/"));
    assert!(desktop_entry("arcade.look", Path::new("/bad\npath")).is_err());
}
