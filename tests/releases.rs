mod support;
use arcade_tools_core::release::{checksum, verify, Arch, Channel, Kind, Os, Release};
use arcade_tools_core::source::Source;
use std::sync::atomic::AtomicBool;
use support::FakeRelease;

#[test]
fn canonical_generator_and_local_github_release_download() {
    let server = FakeRelease::new();
    let dir = server.publish(
        "arcade.look",
        "0.4.0",
        "stable",
        &[
            ("Look_amd64.AppImage", b"verified appimage"),
            ("Look_arm64.AppImage", b"arm"),
            ("Look_x64-setup.exe", b"nsis"),
            ("Look_universal.dmg", b"dmg"),
            ("Look_x64.deb", b"deb"),
            ("Look_x64.rpm", b"rpm"),
            ("Look_arm64.msi", b"msi"),
        ],
        "nsis",
    );
    let release = Release::parse(&std::fs::read(dir.join("arcade-release.json")).unwrap()).unwrap();
    assert_eq!(
        release.select(Os::Linux, Arch::X64).unwrap().kind,
        Kind::Appimage
    );
    assert_eq!(
        release.select(Os::Linux, Arch::Arm64).unwrap().file,
        "Look_arm64.AppImage"
    );
    assert_eq!(
        release
            .select(Os::Windows, Arch::X64)
            .unwrap()
            .silent
            .unwrap(),
        ["/S"]
    );
    assert_eq!(
        release.select(Os::Macos, Arch::Arm64).unwrap().arch,
        Arch::Universal
    );
    assert!(release.select(Os::Windows, Arch::Arm64).is_err()); // MSI isn't a per-user contract.
    let source = Source::test_server(&server.url).unwrap();
    let checked = source
        .check("arcade.look", Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    let downloaded = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    verify(downloaded.path(), &checked.asset).unwrap();
    assert_eq!(checksum(downloaded.path()).unwrap(), checked.asset.sha256);
    std::fs::write(downloaded.path(), b"tampered").unwrap();
    assert_eq!(
        verify(downloaded.path(), &checked.asset).unwrap_err().code,
        "checksum"
    );
    let cancelled = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(true),
            &mut |_, _| {},
        )
        .unwrap_err();
    assert_eq!(cancelled.code, "cancelled");
    println!(
        "canonical manifest + 3-platform asset selection + good/bad checksum + cancellation: PASS"
    );
}

#[test]
fn schema_validation_and_install_policy_are_separate() {
    let server = FakeRelease::new();
    let dir = server.publish(
        "arcade.look",
        "1.0.0",
        "stable",
        &[("Look_x64.AppImage", b"app")],
        "nsis",
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("arcade-release.json")).unwrap()).unwrap();
    for (key, invalid) in [
        ("schema", serde_json::json!(2)),
        ("id", serde_json::json!("other.look")),
        ("version", serde_json::json!("v1")),
        ("channel", serde_json::json!("beta")),
        ("assets", serde_json::json!([])),
        ("linkProtocol", serde_json::json!([0])),
    ] {
        let mut bad = value.clone();
        bad[key] = invalid;
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    for (key, invalid) in [
        ("sha256", serde_json::json!("A".repeat(64))),
        ("file", serde_json::json!("../bad")),
        ("os", serde_json::json!("android")),
        ("arch", serde_json::json!("x86")),
        ("kind", serde_json::json!("zip")),
        ("size", serde_json::Value::Null),
        ("silent", serde_json::Value::Null),
    ] {
        let mut bad = value.clone();
        bad["assets"][0][key] = invalid;
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    for key in [
        "schema",
        "id",
        "version",
        "channel",
        "linkProtocol",
        "notes",
        "assets",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(
            Release::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "missing {key}"
        );
    }
    let mut compatible = value.clone();
    compatible["extra"] = serde_json::json!(true);
    compatible["version"] = serde_json::json!("1-nightly-extra");
    compatible["assets"][0]["kind"] = serde_json::json!("deb");
    compatible["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("size");
    let parsed = Release::parse(&serde_json::to_vec(&compatible).unwrap()).unwrap();
    assert!(parsed.select(Os::Linux, Arch::X64).is_err());
    assert!(parsed.compatible("arcade.box", Channel::Stable).is_err());
    println!("canonical schema: invalid inputs rejected, optional/unknown fields accepted: PASS");
}

#[test]
fn nightly_and_clipboard_tarball_from_canonical_generator() {
    let server = FakeRelease::new();
    server.publish(
        "arcade.clipboard",
        "1.2.0-nightly",
        "nightly",
        &[
            ("clipboard-linux-x64.tar.gz", b"tar"),
            ("Clipboard_arm64-setup.exe", b"inno"),
        ],
        "inno",
    );
    let source = Source::test_server(&server.url).unwrap();
    let linux = source
        .check("arcade.clipboard", Channel::Nightly, Os::Linux, Arch::X64)
        .unwrap();
    assert_eq!(linux.asset.kind, Kind::Tarball);
    let windows = source
        .check(
            "arcade.clipboard",
            Channel::Nightly,
            Os::Windows,
            Arch::Arm64,
        )
        .unwrap();
    assert_eq!(
        windows.asset.silent.unwrap(),
        [
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CURRENTUSER"
        ]
    );
    assert!(source
        .check("arcade.clipboard", Channel::Stable, Os::Linux, Arch::X64)
        .is_err());
    assert!(Source::test_server("https://untrusted.example/").is_err());
}
