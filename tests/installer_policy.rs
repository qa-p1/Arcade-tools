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
