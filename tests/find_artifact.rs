//! Explicit real-package check. Ignored in the deterministic fixture suite;
//! pass ARCADE_TOOLS_FIND_APPIMAGE and APPIMAGE_EXTRACT_AND_RUN=1 to the Link
//! isolated runner, then run this test with --ignored. Never builds a package.
#![cfg(target_os = "linux")]
mod support;
use arcade_link::client::Client;
use arcade_link::receipt::ManagedBy;
use arcade_tools_core::manager::{me, LaunchMode, Manager, Operation, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::source::Source;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn ready(manager: &Manager) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut client) = Client::connect(&manager.paths.link, "arcade.find", &me()) {
            let status = client.status().unwrap();
            if status["busy"] == false
                && status
                    .pointer("/status/phase")
                    .is_some_and(|phase| phase == "ready")
            {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Find's installed resident did not become ready"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
struct QuitOnDrop<'a>(&'a Manager);
impl Drop for QuitOnDrop<'_> {
    fn drop(&mut self) {
        if let Ok(mut client) = Client::connect(&self.0.paths.link, "arcade.find", &me()) {
            let _ = client.call("app.quit", serde_json::json!({}));
        }
    }
}
#[test]
#[ignore = "requires the final Find AppImage and Link isolated runner"]
fn real_find_install_repair_rollback_preserves_verified_cache() {
    let session = support::require_isolated_session();
    let artifact = PathBuf::from(
        std::env::var_os("ARCADE_TOOLS_FIND_APPIMAGE").expect("Provide final Find AppImage path"),
    );
    let server = support::FakeRelease::new();
    let manager = Manager::new(
        Paths::under(&server.root.path().join("user")),
        Source::test_server(&server.url).unwrap(),
    );
    let _cleanup = QuitOnDrop(&manager);
    let find_home = PathBuf::from(std::env::var_os("ARCADE_FIND_HOME").unwrap());
    assert!(find_home.starts_with(&session));
    let roots = server.root.path().join("files");
    fs::create_dir_all(&roots).unwrap();
    fs::create_dir_all(find_home.join("config")).unwrap();
    fs::write(
        find_home.join("config/settings.json"),
        serde_json::to_vec(&serde_json::json!({"schema": 1, "roots": [roots]})).unwrap(),
    )
    .unwrap();
    let version = std::env::var("ARCADE_TOOLS_FIND_VERSION").unwrap_or_else(|_| "0.3.0".into());
    server.publish(
        "arcade.find",
        &version,
        "stable",
        &[("Find_x64.AppImage", &fs::read(&artifact).unwrap())],
        "inno",
    );
    server.amend_manifest("arcade.find", &version, "stable", |manifest| {
        manifest["assets"][0]["installArgs"] = serde_json::json!(["--install", "--silent"])
    });
    for operation in [Operation::Install, Operation::Repair, Operation::Rollback] {
        let request = Request {
            id: "arcade.find".into(),
            operation,
            mode: Some(LaunchMode::Background),
            remove_data: false,
            app_closed: false,
            channel: None,
            version: None,
        };
        manager
            .operate(&request, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
        ready(&manager);
        let receipt = manager.receipt("arcade.find").unwrap().unwrap();
        assert_eq!(receipt.managed_by, ManagedBy::Tools);
        let record = manager.record("arcade.find").unwrap().unwrap();
        assert!(record.installed_by_app);
        record
            .package
            .unwrap()
            .verify(&manager.paths, "arcade.find")
            .unwrap();
        println!("Real Find {version} {operation:?}: receipt owned by Tools, cache verifies, background resident ready");
    }
    let request = Request {
        id: "arcade.find".into(),
        operation: Operation::Uninstall,
        mode: None,
        remove_data: false,
        app_closed: false,
        channel: None,
        version: None,
    };
    manager
        .operate(&request, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(manager.receipt("arcade.find").unwrap().is_none());
    assert!(!manager
        .paths
        .executable("arcade.find", arcade_tools_core::release::Os::Linux, false)
        .exists());
    assert!(find_home.join("config/settings.json").exists());
}
