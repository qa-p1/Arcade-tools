use arcade_tools_core::installation::install_args;
use arcade_tools_core::platform::windows_install_options;
use arcade_tools_core::release::{Arch, Asset, Channel, Kind, Os};
use std::path::Path;

#[test]
fn self_install_arguments_are_exact_and_channel_is_explicit() {
    let mut asset = Asset {
        os: Os::Linux,
        arch: Arch::X64,
        kind: Kind::Appimage,
        file: "Look.AppImage".into(),
        sha256: "0".repeat(64),
        size: Some(1),
        silent: None,
        install_args: None,
    };
    assert_eq!(install_args(&asset, Channel::Stable).unwrap(), None);
    asset.install_args = Some(vec!["--install".into(), "--silent".into()]);
    assert_eq!(
        install_args(&asset, Channel::Nightly).unwrap().unwrap(),
        [
            "--install",
            "--silent",
            "--channel",
            "nightly",
            "--no-launch"
        ]
    );
    asset
        .install_args
        .as_mut()
        .unwrap()
        .push("--remove-data".into());
    assert!(install_args(&asset, Channel::Stable).is_err());
    asset.install_args = Some(vec!["--install".into(), "--silent".into()]);
    asset.kind = Kind::Tarball;
    assert!(install_args(&asset, Channel::Stable).is_err());
}

#[test]
fn windows_silent_tasks_preserve_login_and_nsis_destination_tail() {
    let mut asset = Asset {
        os: Os::Windows,
        arch: Arch::X64,
        kind: Kind::Inno,
        file: "Look.exe".into(),
        sha256: "0".repeat(64),
        size: Some(1),
        install_args: None,
        silent: Some(
            [
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CURRENTUSER",
            ]
            .map(String::from)
            .to_vec(),
        ),
    };
    let plan = windows_install_options(
        &asset,
        Path::new("verified.exe"),
        Path::new("C:/Users/a/Apps"),
        Channel::Nightly,
        false,
    )
    .unwrap();
    assert!(plan
        .args
        .contains(&"/MERGETASKS=!arcade_startup,!arcade_desktop".into()));
    assert!(plan.args.contains(&"/ARCADECHANNEL=nightly".into()));
    asset.kind = Kind::Nsis;
    asset.silent = Some(vec!["/S".into()]);
    let plan = windows_install_options(
        &asset,
        Path::new("verified.exe"),
        Path::new("C:/Users/a/Apps"),
        Channel::Stable,
        true,
    )
    .unwrap();
    assert_eq!(plan.nsis_tail(), Some("/D=C:/Users/a/Apps"));
    assert!(plan.args.contains(&"/STARTATLOGIN=1".into()));
}

#[test]
fn windows_updates_stay_in_the_installer_recorded_folder() {
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt};
    use arcade_tools_core::platform::windows_install_root;
    use std::path::PathBuf;
    let folder = std::env::temp_dir().join("Custom Apps/Arcade Lens");
    let mut receipt = Receipt {
        schema: 1,
        id: "arcade.lens".into(),
        version: "0.2.0".into(),
        channel: "stable".into(),
        method: InstallMethod::WindowsInstaller,
        managed_by: ManagedBy::SelfManaged,
        path: folder.join("arcade-lens.exe"),
        integration: Integration::default(),
        previous: None,
        installed_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(windows_install_root(Some(&receipt)), Some(folder));
    assert_eq!(windows_install_root(None), None::<PathBuf>);
    receipt.method = InstallMethod::Appimage;
    assert_eq!(windows_install_root(Some(&receipt)), None);
}

#[test]
fn macos_updates_stay_in_the_adopted_applications_bundle() {
    use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt};
    use arcade_tools_core::paths::Paths;
    use arcade_tools_core::platform::{native_install_paths, native_install_root};
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    for folder in [
        Path::new("/Applications").to_path_buf(),
        paths.home.join("Applications"),
    ] {
        let bundle = folder.join("Arcade Look.app");
        let receipt = Receipt {
            schema: 1,
            id: "arcade.look".into(),
            version: "0.2.0".into(),
            channel: "stable".into(),
            method: InstallMethod::MacosBundle,
            managed_by: ManagedBy::Tools,
            path: bundle.clone(),
            integration: Integration::default(),
            previous: None,
            installed_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let chosen = native_install_root(Some(&receipt)).unwrap();
        let (target, executable) =
            native_install_paths(&paths, "arcade.look", Os::Macos, false, Some(&chosen));
        assert_eq!(target, bundle);
        assert_eq!(executable, bundle.join("Contents/MacOS/arcade-look"));
    }
}
