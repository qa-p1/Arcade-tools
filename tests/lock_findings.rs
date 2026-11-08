//! Behaviour lock for audit findings that live in the core crate (docs/v1/audit/tools.md). A `KNOWN BUG` test passes
//! while the bug is present and must lose its marker with the fix (DECISIONS.md D3).
use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt, Store};
use arcade_tools_core::{manager::Manager, paths::Paths, release::Channel, source::Source};
use std::fs;

struct Fixture {
    root: tempfile::TempDir,
    manager: Manager,
}

/// Arcade Shelf 0.4.1 on Stable, installed by itself (a receipt, no Tools record).
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let app = paths.home.join("Applications/Arcade/Arcade-Shelf.AppImage");
    fs::create_dir_all(app.parent().unwrap()).unwrap();
    fs::write(&app, b"shelf 0.4.1").unwrap();
    Store::new(&paths.link)
        .write(&Receipt {
            schema: 1,
            id: "arcade.shelf".into(),
            version: "0.4.1".into(),
            channel: "stable".into(),
            method: InstallMethod::Appimage,
            managed_by: ManagedBy::SelfManaged,
            path: app,
            integration: Integration::default(),
            previous: None,
            installed_at: "2026-11-03T00:00:00Z".into(),
            updated_at: "2026-11-03T00:00:00Z".into(),
        })
        .unwrap();
    let manager = Manager::new(paths, Source::test_server("http://127.0.0.1:9/").unwrap());
    Fixture { root, manager }
}

#[test]
fn startup_cleanup_bounds_the_installer_cache() {
    let f = fixture();
    let dir = Paths::under(f.root.path())
        .cache
        .join("arcade/tools/installers/arcade.shelf");
    fs::create_dir_all(&dir).unwrap();
    for n in 0..3 {
        fs::write(dir.join(format!("{n:064x}.AppImage")), vec![0u8; 1024]).unwrap();
    }
    f.manager.cleanup_interrupted().unwrap();
    let left = fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
    assert_eq!(
        left, 0,
        "{left} installers that no install or rollback needs were kept (TL-7)"
    );
}

#[test]
fn the_app_list_tells_the_installed_channel_apart_from_the_selected_one() {
    let f = fixture();
    f.manager
        .set_channel("arcade.shelf", Channel::Nightly)
        .unwrap();
    let view = f
        .manager
        .list(false)
        .unwrap()
        .into_iter()
        .find(|v| v.id == "arcade.shelf")
        .unwrap();
    let value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["channel"], "nightly");
    assert_eq!(
        value["installedChannel"], "stable",
        "TL-9: the list shows only the selected channel: {value}"
    );
}

#[test]
fn choosing_a_channel_is_remembered() {
    let f = fixture();
    f.manager
        .set_channel("arcade.shelf", Channel::Nightly)
        .unwrap();
    assert_eq!(
        f.manager
            .preferences()
            .unwrap()
            .channels
            .get("arcade.shelf"),
        Some(&Channel::Nightly)
    );
}

/// T3, fixed in Link 1.0.1: a legacy file whose name the current folder already holds with other contents is
/// archived under `archived-legacy/`, so the legacy folder is retired on the first start, never kept forever.
#[test]
fn legacy_folders_are_retired_once_the_current_ones_exist() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let legacy = legacy_state(&paths);
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("preferences.json"), br#"{"theme":"dark"}"#).unwrap();
    fs::create_dir_all(paths.state()).unwrap();
    // The owner's case: the current folder already holds newer, different settings.
    fs::write(
        paths.state().join("preferences.json"),
        br#"{"theme":"light"}"#,
    )
    .unwrap();
    let first = paths.migrate_own_folders();
    assert!(first.kept.is_empty(), "kept {:?}", first.kept);
    assert!(!legacy.exists());
    // The current settings win; the legacy copy is archived, not lost.
    assert_eq!(
        fs::read(paths.state().join("preferences.json")).unwrap(),
        br#"{"theme":"light"}"#
    );
    let archived: Vec<_> = walkdir(&paths.state().join("archived-legacy"))
        .into_iter()
        .filter(|p| p.ends_with("preferences.json"))
        .collect();
    assert_eq!(archived.len(), 1, "{archived:?}");
    assert_eq!(fs::read(&archived[0]).unwrap(), br#"{"theme":"dark"}"#);
    assert!(paths.migrate_own_folders().kept.is_empty());
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walkdir(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Tools 0.1–0.3's state folder on this platform (`arcade-tools` under the user's data, roaming or Application
/// Support folder), as Link's migration table records it.
fn legacy_state(paths: &Paths) -> std::path::PathBuf {
    use arcade_link::paths::{current_os, legacy_dirs_in, Kind};
    legacy_dirs_in(
        &paths.user_dirs(),
        current_os(),
        arcade_link::manifest::ids::TOOLS,
    )
    .into_iter()
    .find(|dir| dir.kind == Kind::Data && dir.path.ends_with("arcade-tools"))
    .expect("Link knows Tools' legacy state folder")
    .path
}
