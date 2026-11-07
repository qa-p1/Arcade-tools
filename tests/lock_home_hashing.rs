//! TL-8 / T1: Home refreshes the app list every few seconds, so listing an app that installed itself (a receipt but
//! no Tools record) must not re-read its whole AppImage each time. Own test binary: it reads a process-wide counter.
use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt, Store};
use arcade_tools_core::{manager::Manager, paths::Paths, release::checksum_reads, source::Source};
use std::fs;

#[test]
#[should_panic(expected = "KNOWN BUG TL-8")]
fn listing_an_unmanaged_app_again_does_not_rehash_it() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let app = paths.home.join("Applications/Arcade/Arcade-Look.AppImage");
    fs::create_dir_all(app.parent().unwrap()).unwrap();
    fs::write(&app, vec![7u8; 1 << 20]).unwrap();
    Store::new(&paths.link)
        .write(&Receipt {
            schema: 1,
            id: "arcade.look".into(),
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
    let first = manager.record("arcade.look").unwrap().unwrap();
    let before = checksum_reads();
    for _ in 0..5 {
        let again = manager.record("arcade.look").unwrap().unwrap();
        assert_eq!(again.executable_sha256, first.executable_sha256);
    }
    let reads = checksum_reads() - before;
    assert_eq!(
        reads, 0,
        "KNOWN BUG TL-8: 5 unchanged refreshes hashed the AppImage {reads} times"
    );
}
