mod support;
use arcade_tools_core::{
    apps,
    paths::Paths,
    platform,
    release::{Arch, Channel, Kind, Os, Release},
    source::Source,
};
use std::{io::Cursor, path::Path};

#[test]
fn shelf_metadata_install_and_data_paths_match_native_application() {
    assert!(apps::APPS.contains(&apps::SHELF));
    assert_eq!(apps::app_name(apps::SHELF), "Arcade Shelf");
    assert_eq!(
        apps::releases_url(apps::SHELF),
        "https://github.com/qa-p1/Arcade-Shelf/releases"
    );
    let paths = Paths::under(Path::new("/isolated"));
    assert!(paths
        .executable(apps::SHELF, Os::Linux, true)
        .ends_with("data/arcade-shelf/arcade-shelf"));
    assert!(paths
        .executable(apps::SHELF, Os::Windows, false)
        .ends_with("local/Programs/Arcade Shelf/arcade-shelf.exe"));
    assert!(paths
        .executable(apps::SHELF, Os::Macos, false)
        .ends_with("Applications/Arcade Shelf.app/Contents/MacOS/arcade-shelf"));
    assert_eq!(
        paths.data_folders(apps::SHELF, Os::Linux),
        [paths.data.join("qa-p1/ArcadeShelf")]
    );
    assert_eq!(
        paths.data_folders(apps::SHELF, Os::Windows),
        [paths.local.join("qa-p1/ArcadeShelf")]
    );
    assert!(paths
        .autostart(apps::SHELF, Os::Linux)
        .ends_with("autostart/arcade.shelf.desktop"));
    assert!(paths
        .autostart(apps::SHELF, Os::Windows)
        .ends_with("Startup/Arcade Shelf.vbs"));
    assert!(paths
        .autostart(apps::SHELF, Os::Macos)
        .ends_with("LaunchAgents/arcade.shelf.plist"));
    let request = arcade_link::InvokeRequest::new("tools.install", "arcade.shelf")
        .options(serde_json::json!({"app":apps::SHELF}));
    assert_eq!(
        arcade_tools_core::link::install_target(&request).unwrap(),
        apps::SHELF
    );
}

#[test]
fn shelf_release_source_selects_portable_bundle_without_changing_other_apps() {
    let server = support::FakeRelease::new();
    let dir = server.publish(
        apps::SHELF,
        "0.1.0",
        "stable",
        &[
            ("Arcade-Shelf-0.1.0-linux-x64.tar.gz", b"archive"),
            ("Arcade-Shelf-0.1.0-windows-x64.exe", b"nsis"),
            ("Arcade-Shelf-0.1.0-macos-arm64.dmg", b"dmg"),
        ],
        "nsis",
    );
    let source = Source::test_server(&server.url).unwrap();
    let checked = source
        .check(apps::SHELF, Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    assert_eq!(checked.asset.kind, Kind::Tarball);
    let manifest = std::fs::read(dir.join("arcade-release.json")).unwrap();
    let mut wrong = Release::parse(&manifest).unwrap();
    wrong.id = "arcade.look".into();
    assert!(wrong.select(Os::Linux, Arch::X64).is_err());
}

fn bundle(path: &Path, symlink: bool) {
    let file = std::fs::File::create(path).unwrap();
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (name, data) in [
        ("Arcade-Shelf/arcade-shelf", "#!/bin/sh\n"),
        ("Arcade-Shelf/bin/arcade-shelf", "binary"),
        ("Arcade-Shelf/bin/qt.conf", "[Paths]\nPrefix=..\n"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, name, Cursor::new(data))
            .unwrap();
    }
    if symlink {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("/outside").unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, "Arcade-Shelf/unsafe", Cursor::new([]))
            .unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap();
}

#[test]
fn shelf_bundle_extraction_rejects_links_and_preserves_originals() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("bundle.tar.gz");
    bundle(&archive, false);
    let (_guard, extracted) =
        platform::extract_shelf(&archive, &temp.path().join("downloads")).unwrap();
    assert_eq!(
        std::fs::read_to_string(extracted.join("bin/arcade-shelf")).unwrap(),
        "binary"
    );
    assert!(archive.is_file());
    bundle(&archive, true);
    assert_eq!(
        platform::extract_shelf(&archive, &temp.path().join("downloads"))
            .unwrap_err()
            .code,
        "installer"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn shelf_bundle_install_replacement_rollback_and_login_ownership() {
    use arcade_tools_core::release::Asset;
    let temp = tempfile::tempdir().unwrap();
    let paths = Paths::under(temp.path());
    let archive = temp.path().join("bundle.tar.gz");
    bundle(&archive, false);
    let asset = Asset {
        os: Os::Linux,
        arch: Arch::X64,
        kind: Kind::Tarball,
        file: "Arcade-Shelf-linux-x64.tar.gz".into(),
        sha256: "0".repeat(64),
        size: None,
        silent: None,
    };
    let (executable, replacement) =
        platform::install(&paths, apps::SHELF, &asset, &archive).unwrap();
    assert!(executable.is_file());
    replacement.commit();
    let original = paths.data.join("qa-p1/ArcadeShelf/original.txt");
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    std::fs::write(&original, "Keep collections").unwrap();
    let (_, replacement) = platform::install(&paths, apps::SHELF, &asset, &archive).unwrap();
    drop(replacement);
    assert!(executable.is_file());
    assert_eq!(
        std::fs::read_to_string(&original).unwrap(),
        "Keep collections"
    );
    platform::set_autostart(&paths, apps::SHELF, &executable, true).unwrap();
    let login = paths.autostart(apps::SHELF, Os::Linux);
    assert!(std::fs::read_to_string(&login)
        .unwrap()
        .contains("Arcade Shelf managed login entry"));
    platform::set_autostart(&paths, apps::SHELF, &executable, false).unwrap();
    std::fs::write(&login, "Unrelated user data").unwrap();
    assert!(platform::set_autostart(&paths, apps::SHELF, &executable, true).is_err());
    assert_eq!(
        std::fs::read_to_string(login).unwrap(),
        "Unrelated user data"
    );
}
