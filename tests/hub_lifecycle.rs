#![cfg(target_os = "linux")]
mod support;
use arcade_link::manifest::{Manifest, ManifestAdditions, ManifestDocument};
use arcade_link::receipt::{ManagedBy, Store};
use arcade_link::server::{Handler, InvokeContext, Reply, Server, ServerConfig};
use arcade_link::wire::{InvokeRequest, PeerInfo};
use arcade_link::{Action, LinkError};
use arcade_tools_core::health::{FixAction, HealthKind};
use arcade_tools_core::manager::{InstallationClass, LaunchMode, Manager, Operation, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::{checksum, Os};
use arcade_tools_core::self_update::{SelfUpdatePlan, StagedSelfUpdate};
use arcade_tools_core::source::Source;
use arcade_tools_core::updates::{Clock, HistoryEntry, HistoryResult};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::FakeRelease;

fn fixture() -> (FakeRelease, Manager) {
    assert!(
        std::env::var("HOME").unwrap().starts_with("/tmp/")
            && std::env::var_os("ARCADE_HOME").is_some(),
        "Use the Link isolated runner"
    );
    let server = FakeRelease::new();
    let manager = Manager::new(
        Paths::under(&server.root.path().join("user")),
        Source::test_server(&server.url).unwrap(),
    );
    (server, manager)
}
fn request(id: &str, operation: Operation) -> Request {
    Request {
        id: id.into(),
        operation,
        mode: Some(LaunchMode::Background),
        remove_data: false,
        app_closed: false,
    }
}
fn operate(manager: &Manager, id: &str, operation: Operation) -> arcade_tools_core::Result<String> {
    manager.operate(
        &request(id, operation),
        &AtomicBool::new(false),
        &mut |_| {},
    )
}
fn fake_app(paths: &Paths, id: &str, version: &str, fail: bool) -> Vec<u8> {
    let exe = paths.executable(id, Os::Linux, false);
    let manifest = Manifest::new(id, version, exe.to_str().unwrap());
    let source = r#"#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
ID, VERSION, FAIL, DOC, DATA = __CONSTANTS__
home = pathlib.Path(os.environ['ARCADE_HOME'])
assert pathlib.Path(os.environ['HOME']) == home
args = sys.argv[1:]
log = home / 'installer-events'
home.mkdir(parents=True, exist_ok=True)
with log.open('a') as f: f.write(json.dumps({'id': ID, 'version': VERSION, 'args': args}) + '\n')
receipt_file = home / 'installs' / (ID + '.json')
registry = home / 'apps' / (ID + '.json')
dest = pathlib.Path(DOC['executable'])
desktop = home / '.local/share/applications' / (ID.replace('.', '-') + '.desktop')
cli = home / '.local/bin' / ID.replace('.', '-')
stamp = '2026-10-11T10:00:00Z'
def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value)); path.chmod(0o600)
def remove(path):
    if path.is_symlink() or path.is_file(): path.unlink()
    elif path.is_dir(): shutil.rmtree(path)
if '--install' in args:
    # A real resident such as Find execs itself unless Tools suppresses launch.
    # Refuse here so a missing --no-launch fails promptly instead of hanging.
    assert '--no-launch' in args, 'Managed installer would launch a resident'
    old = json.loads(receipt_file.read_text()) if receipt_file.exists() else None
    previous = old.get('previous') if old else None
    source = pathlib.Path(sys.argv[0]).resolve()
    dest.parent.mkdir(parents=True, exist_ok=True)
    if source != dest.resolve():
        if dest.exists():
            assert old and pathlib.Path(old['path']) == dest
            prev = dest.parent / '.previous' / dest.name
            prev.parent.mkdir(parents=True, exist_ok=True)
            if prev.exists(): assert old.get('previous') and pathlib.Path(old['previous']['path']) == prev
            shutil.copy2(dest, prev)
            previous = {'version': old['version'], 'path': str(prev)}
        shutil.copy2(source, dest)
    dest.chmod(0o755)
    desktop.parent.mkdir(parents=True, exist_ok=True)
    desktop.write_text('[Desktop Entry]\nType=Application\nExec=' + str(dest) + '\nX-Arcade-Id=' + ID + '\n')
    cli.parent.mkdir(parents=True, exist_ok=True)
    if cli.is_symlink(): cli.unlink()
    cli.symlink_to(dest)
    channel = args[args.index('--channel')+1] if '--channel' in args else 'stable'
    write(receipt_file, {'schema': 1, 'id': ID, 'version': VERSION, 'channel': channel, 'method': 'appimage', 'managedBy': 'self',
        'path': str(dest), 'integration': {'desktopEntry': str(desktop), 'icons': [], 'cli': str(cli), 'autostart': None, 'uninstaller': None},
        'previous': previous, 'installedAt': old['installedAt'] if old else stamp, 'updatedAt': stamp})
    if source != dest.resolve(): source.unlink() # Find's default remove_download
    if FAIL: sys.exit(9)
elif '--uninstall' in args:
    receipt = json.loads(receipt_file.read_text())
    remove(pathlib.Path(receipt['path']))
    remove(desktop); remove(cli)
    if receipt.get('previous'): remove(pathlib.Path(receipt['previous']['path']))
    if '--remove-data' in args:
        for folder in DATA: remove(pathlib.Path(folder))
    remove(receipt_file); remove(registry)
else:
    DOC['install'] = 'appimage'
    write(registry, DOC)
"#;
    let constants = format!(
        "{}, {}, {}, json.loads({}), json.loads({})",
        json!(id),
        json!(version),
        if fail { "True" } else { "False" },
        json!(manifest.to_json()),
        json!(serde_json::to_string(&paths.data_folders(id, Os::Linux)).unwrap())
    );
    source.replace("__CONSTANTS__", &constants).into_bytes()
}
fn publish(server: &FakeRelease, manager: &Manager, id: &str, version: &str, fail: bool) {
    server.publish(
        id,
        version,
        "stable",
        &[(
            "App_x64.AppImage",
            &fake_app(&manager.paths, id, version, fail),
        )],
        "nsis",
    );
    server.amend_manifest(id, version, "stable", |manifest| {
        manifest["assets"][0]["installArgs"] = json!(["--install", "--silent"]);
    });
}
fn wait_for(path: &Path, check: impl Fn(&str) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if fs::read_to_string(path).is_ok_and(|text| check(&text)) {
            break;
        }
        assert!(
            Instant::now() < until,
            "Timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn installed(server: &FakeRelease, manager: &Manager, id: &str) {
    publish(server, manager, id, "1.0.0", false);
    operate(manager, id, Operation::Install).unwrap();
    wait_for(
        &manager.paths.link.registry.join(format!("{id}.json")),
        |text| text.contains("1.0.0"),
    );
}
fn history(manager: &Manager) -> Vec<HistoryEntry> {
    fs::read_to_string(manager.paths.state().join("update-history.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn hub_lifecycle_install_update_rollback_repeated_updates_and_uninstall() {
    let (server, manager) = fixture();
    installed(&server, &manager, "arcade.look");
    let receipt = manager.receipt("arcade.look").unwrap().unwrap();
    assert_eq!(receipt.managed_by, ManagedBy::Tools);
    assert!(
        manager
            .record("arcade.look")
            .unwrap()
            .unwrap()
            .installed_by_app
    );
    assert!(
        fs::read_to_string(manager.paths.home.join("installer-events"))
            .unwrap()
            .contains("\"--install\", \"--silent\", \"--channel\", \"stable\", \"--no-launch\"")
    );
    let retained = manager
        .record("arcade.look")
        .unwrap()
        .unwrap()
        .package
        .unwrap();
    retained.verify(&manager.paths, "arcade.look").unwrap();
    let data = manager.paths.data_folders("arcade.look", Os::Linux)[0].clone();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("keep"), "settings").unwrap();
    publish(&server, &manager, "arcade.look", "2.0.0", false);
    operate(&manager, "arcade.look", Operation::Update).unwrap();
    assert_eq!(
        manager.rollback_version("arcade.look").unwrap().as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        manager
            .receipt("arcade.look")
            .unwrap()
            .unwrap()
            .previous
            .unwrap()
            .version,
        "1.0.0"
    );
    operate(&manager, "arcade.look", Operation::Rollback).unwrap();
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().version,
        "1.0.0"
    );
    assert!(matches!(
        history(&manager).last().unwrap().result,
        HistoryResult::RolledBack
    ));
    retained.verify(&manager.paths, "arcade.look").unwrap();
    publish(&server, &manager, "arcade.look", "3.0.0", false);
    operate(&manager, "arcade.look", Operation::Update).unwrap();
    assert_eq!(
        manager.rollback_version("arcade.look").unwrap().as_deref(),
        Some("1.0.0")
    );
    operate(&manager, "arcade.look", Operation::Uninstall).unwrap();
    assert!(data.join("keep").exists());
    assert!(manager.receipt("arcade.look").unwrap().is_none());
    assert!(!receipt.path.exists());
    installed(&server, &manager, "arcade.look");
    let mut removal = request("arcade.look", Operation::Uninstall);
    removal.remove_data = true;
    manager
        .operate(&removal, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(!data.exists());
    assert!(
        fs::read_to_string(manager.paths.home.join("installer-events"))
            .unwrap()
            .contains("\"--uninstall\", \"--silent\", \"--remove-data\"")
    );
}

#[test]
fn hub_lifecycle_installer_failure_restores_receipt_integration_and_history() {
    let (server, manager) = fixture();
    installed(&server, &manager, "arcade.look");
    let before = manager.receipt("arcade.look").unwrap().unwrap();
    let old_bytes = fs::read(&before.path).unwrap();
    let old_desktop = fs::read(before.integration.desktop_entry.as_ref().unwrap()).unwrap();
    publish(&server, &manager, "arcade.look", "2.0.0", true);
    assert_eq!(
        operate(&manager, "arcade.look", Operation::Update)
            .unwrap_err()
            .code,
        "installer"
    );
    assert_eq!(fs::read(&before.path).unwrap(), old_bytes);
    assert_eq!(manager.receipt("arcade.look").unwrap().unwrap(), before);
    assert_eq!(
        fs::read(before.integration.desktop_entry.as_ref().unwrap()).unwrap(),
        old_desktop
    );
    assert_eq!(
        fs::read_link(before.integration.cli.as_ref().unwrap()).unwrap(),
        before.path
    );
    assert!(matches!(
        history(&manager).last().unwrap().result,
        HistoryResult::RolledBack
    ));
}

#[test]
fn hub_lifecycle_adopt_manual_self_and_refuse_dev() {
    let (server, manager) = fixture();
    for (id, self_install) in [("arcade.look", false), ("arcade.lens", true)] {
        let manual = manager
            .paths
            .home
            .join("Downloads")
            .join(format!("{id}.AppImage"));
        fs::create_dir_all(manual.parent().unwrap()).unwrap();
        fs::write(&manual, fake_app(&manager.paths, id, "1.0.0", false)).unwrap();
        fs::set_permissions(&manual, fs::Permissions::from_mode(0o755)).unwrap();
        if self_install {
            arcade_tools_core::installation::run_app_installer(
                &manager.paths,
                &manual,
                vec!["--install".into(), "--silent".into(), "--no-launch".into()],
            )
            .unwrap();
        }
        let exe = if self_install {
            manager.paths.executable(id, Os::Linux, false)
        } else {
            manual.clone()
        };
        arcade_link::manifest::write_manifest(
            &manager.paths.link,
            &Manifest::new(id, "1.0.0", exe.to_str().unwrap()),
        )
        .unwrap();
        let adoption = manager
            .adoptions()
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap();
        assert_eq!(
            adoption.class,
            if self_install {
                InstallationClass::SelfInstalled
            } else {
                InstallationClass::Manual
            }
        );
        operate(&manager, id, Operation::Adopt).unwrap();
        assert_eq!(
            manager.receipt(id).unwrap().unwrap().managed_by,
            ManagedBy::Tools
        );
        assert!(manual.exists());
    }
    let dev = manager.paths.home.join("source/target/debug/app");
    fs::create_dir_all(dev.parent().unwrap()).unwrap();
    fs::write(&dev, "development").unwrap();
    fs::create_dir_all(manager.paths.home.join("source/.git")).unwrap();
    let doc = ManifestDocument {
        manifest: Manifest::new("arcade.find", "1.0.0", dev.to_str().unwrap()),
        additions: ManifestAdditions {
            install: Some(arcade_link::receipt::InstallMethod::Dev),
            ..Default::default()
        },
    };
    arcade_link::manifest::write_document(&manager.paths.link, &doc).unwrap();
    assert_eq!(
        operate(&manager, "arcade.find", Operation::Adopt)
            .unwrap_err()
            .code,
        "dev_build"
    );
    assert_eq!(fs::read_to_string(&dev).unwrap(), "development");
    assert!(manager.receipt("arcade.find").unwrap().is_none());
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(route, _)| !route.contains("releases")));
}

struct Time(u64);
impl Clock for Time {
    fn now(&self) -> u64 {
        self.0
    }
}
#[test]
fn hub_lifecycle_auto_update_executes_only_opted_in_idle_compatible_apps() {
    let (server, manager) = fixture();
    installed(&server, &manager, "arcade.look");
    publish(&server, &manager, "arcade.look", "2.0.0", false);
    let calls = server.requests.lock().unwrap().len();
    let run = |last_used| {
        manager
            .auto_update(
                "arcade.look",
                last_used,
                &Time(1000),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap()
    };
    assert!(!run(Some(0)).decision.eligible);
    manager.set_auto_update("arcade.look", true).unwrap();
    assert!(!run(Some(500)).decision.eligible);
    manager.set_updates_enabled(false).unwrap();
    assert!(!run(Some(0)).decision.eligible);
    assert_eq!(server.requests.lock().unwrap().len(), calls);
    manager.set_updates_enabled(true).unwrap();
    let result = run(Some(0));
    assert!(result.decision.eligible && result.message.is_some());
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().version,
        "2.0.0"
    );
    let peer = manager.paths.home.join("Applications/peer");
    fs::write(&peer, "peer").unwrap();
    let mut manifest = Manifest::new("arcade.lens", "1.0.0", peer.to_str().unwrap());
    manifest.link.protocol = vec![2];
    arcade_link::manifest::write_manifest(&manager.paths.link, &manifest).unwrap();
    publish(&server, &manager, "arcade.look", "3.0.0", false);
    let outcome = run(Some(0));
    assert!(!outcome.decision.eligible && outcome.decision.reason.contains("protocol"));
    assert_eq!(
        manager.record("arcade.look").unwrap().unwrap().version,
        "2.0.0"
    );
}

#[test]
fn hub_lifecycle_health_and_reviewed_uninstall_all() {
    let (server, manager) = fixture();
    installed(&server, &manager, "arcade.look");
    let stale = manager.paths.home.join("Applications/gone");
    arcade_link::manifest::write_manifest(
        &manager.paths.link,
        &Manifest::new("arcade.lens", "1.0.0", stale.to_str().unwrap()),
    )
    .unwrap();
    let download = manager.paths.home.join("Downloads/Find.AppImage");
    fs::create_dir_all(download.parent().unwrap()).unwrap();
    fs::write(&download, "manual").unwrap();
    let mut manifest = Manifest::new("arcade.find", "1.0.0", download.to_str().unwrap());
    manifest.settings.link_enabled = false;
    manifest.link.protocol = vec![2];
    arcade_link::manifest::write_manifest(&manager.paths.link, &manifest).unwrap();
    let issues = manager.health().unwrap();
    for kind in [
        HealthKind::StaleRegistry,
        HealthKind::TemporaryExecutable,
        HealthKind::UnsupportedVersion,
        HealthKind::LinkOff,
    ] {
        assert!(issues.iter().any(|issue| issue.kind == kind));
    }
    manifest.link.protocol = vec![1];
    arcade_link::manifest::write_manifest(&manager.paths.link, &manifest).unwrap();
    manager
        .fix_health(
            "arcade.lens",
            FixAction::RemoveStaleRegistry,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let receipt = manager.receipt("arcade.look").unwrap().unwrap();
    fs::remove_file(&receipt.path).unwrap();
    assert!(manager
        .health()
        .unwrap()
        .iter()
        .any(|issue| issue.kind == HealthKind::MissingReceiptFile
            && issue.fix_action == FixAction::Repair));
    manager
        .fix_health(
            "arcade.look",
            FixAction::Repair,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    let plan = manager.plan_uninstall_all(true).unwrap();
    assert!(plan
        .apps
        .iter()
        .any(|app| app.id == "arcade.find" && !app.can_uninstall));
    assert!(plan
        .apps
        .iter()
        .find(|app| app.id == "arcade.look")
        .unwrap()
        .data_folders
        .contains(&manager.paths.data_folders("arcade.look", Os::Linux)[0]));
    let mut changed = plan.clone();
    changed.apps[0]
        .folders
        .push(manager.paths.home.join("unreviewed"));
    assert_eq!(
        manager
            .uninstall_all(&changed, &AtomicBool::new(false), &mut |_| {})
            .unwrap_err()
            .code,
        "state_changed"
    );
    let outcome = manager
        .uninstall_all(&plan, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert!(outcome
        .results
        .iter()
        .any(|result| result.app == "arcade.look" && result.removed));
    assert!(download.exists());
}

#[test]
fn hub_lifecycle_tools_self_update_handoff_verification_and_helper_flow() {
    let (server, manager) = fixture();
    let file = manager.paths.executable("arcade.tools", Os::Linux, false);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        &file,
        fake_app(&manager.paths, "arcade.tools", "1.0.0", false),
    )
    .unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    arcade_tools_core::platform::CommandSpec {
        program: file.clone(),
        args: vec!["--install".into(), "--silent".into(), "--no-launch".into()],
    }
    .run(&manager.paths)
    .unwrap();
    let mut receipt = manager.receipt("arcade.tools").unwrap().unwrap();
    receipt.managed_by = ManagedBy::Tools;
    Store::new(&manager.paths.link).write(&receipt).unwrap();
    publish(&server, &manager, "arcade.tools", "2.0.0", false);
    let mut called = false;
    let staged = manager
        .prepare_self_update(
            LaunchMode::Background,
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |staged: &StagedSelfUpdate| {
                assert_eq!(
                    SelfUpdatePlan::read(&manager.paths, &staged.file)
                        .unwrap()
                        .expected_sha256,
                    checksum(&file).unwrap()
                );
                called = true;
                Ok(())
            },
        )
        .unwrap();
    assert!(called && staged.file.exists());
    assert_eq!(
        manager
            .finish_self_update(&staged.plan, &AtomicBool::new(false), &mut |_| {})
            .unwrap_err()
            .code,
        "self_update"
    );
    let mut plan = staged.plan.clone();
    plan.parent_pid = 2_000_000_000; // nonexistent parent represents the exited helper handoff
    manager
        .finish_self_update(&plan, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert_eq!(
        manager.receipt("arcade.tools").unwrap().unwrap().version,
        "2.0.0"
    );
    wait_for(
        &manager.paths.link.registry.join("arcade.tools.json"),
        |text| text.contains("2.0.0"),
    );
    let mut removal = manager.prepare_self_uninstall(false).unwrap();
    removal.parent_pid = 2_000_000_000;
    manager
        .finish_self_uninstall(&removal, &AtomicBool::new(false))
        .unwrap();
    assert!(!file.exists());
    assert!(history(&manager)
        .iter()
        .any(|entry| entry.app == "arcade.tools"));
}

struct RunningApp {
    server: Mutex<Option<Server>>,
    busy: AtomicBool,
    quit: AtomicBool,
}
impl Handler for RunningApp {
    fn describe(&self) -> Vec<Action> {
        vec![]
    }
    fn invoke(&self, _: InvokeRequest, _: &InvokeContext) -> Result<Reply, LinkError> {
        Err(LinkError::busy())
    }
    fn status(&self) -> Value {
        json!({ "busy": self.busy.load(Ordering::SeqCst), "background": true })
    }
    fn activate(&self) -> Result<(), LinkError> {
        Ok(())
    }
    fn quit(&self) -> Result<(), LinkError> {
        self.quit.store(true, Ordering::SeqCst);
        self.server.lock().unwrap().take();
        Ok(())
    }
}
fn running(manager: &Manager) -> Arc<RunningApp> {
    let app = Arc::new(RunningApp {
        server: Mutex::new(None),
        busy: AtomicBool::new(true),
        quit: AtomicBool::new(false),
    });
    let server = Server::start(
        ServerConfig {
            app: PeerInfo {
                id: "arcade.look".into(),
                version: "1.0.0".into(),
            },
            locations: manager.paths.link.clone(),
        },
        app.clone(),
    )
    .unwrap();
    *app.server.lock().unwrap() = Some(server);
    app
}

#[test]
fn hub_lifecycle_busy_holds_verification_before_quit_and_same_mode_restart() {
    let (server, manager) = fixture();
    installed(&server, &manager, "arcade.look");
    let app = running(&manager);
    publish(&server, &manager, "arcade.look", "2.0.0", false);
    assert_eq!(
        operate(&manager, "arcade.look", Operation::Update)
            .unwrap_err()
            .code,
        "busy"
    );
    assert!(!app.quit.load(Ordering::SeqCst));
    manager.set_auto_update("arcade.look", true).unwrap();
    assert!(
        !manager
            .auto_update(
                "arcade.look",
                Some(0),
                &Time(1000),
                &AtomicBool::new(false),
                &mut |_| {}
            )
            .unwrap()
            .decision
            .eligible
    );
    let mut held: Value = serde_json::from_str(arcade_tools_core::catalog::SNAPSHOT).unwrap();
    for entry in held["apps"].as_array_mut().unwrap() {
        entry["minTools"] = json!("9.0.0");
    }
    // Reuse the catalog client to populate its real envelope, not a synthetic
    // manager setting; minTools is checked before download/quit.
    server.routes.lock().unwrap().insert(
        "/held/catalog.json".into(),
        serde_json::to_vec(&held).unwrap(),
    );
    arcade_tools_core::catalog::CatalogClient::with_url(
        manager.paths.clone(),
        Source::test_server(&server.url).unwrap(),
        &format!("{}held/catalog.json", server.url),
    )
    .unwrap()
    .refresh_catalog();
    assert_eq!(
        operate(&manager, "arcade.look", Operation::Update)
            .unwrap_err()
            .code,
        "version_mismatch"
    );
    assert!(!app.quit.load(Ordering::SeqCst));
    arcade_tools_core::catalog::CatalogClient::with_url(
        manager.paths.clone(),
        Source::test_server(&server.url).unwrap(),
        &format!("{}catalog.json", server.url),
    )
    .unwrap()
    .refresh_catalog();
    app.busy.store(false, Ordering::SeqCst);
    let mut verified = false;
    manager
        .operate(
            &request("arcade.look", Operation::Update),
            &AtomicBool::new(false),
            &mut |progress| {
                if progress.phase == "Verified SHA-256" {
                    verified = true;
                    assert!(!app.quit.load(Ordering::SeqCst));
                }
                if progress.phase == "Installing" {
                    assert!(verified && app.quit.load(Ordering::SeqCst));
                }
            },
        )
        .unwrap();
    wait_for(&manager.paths.home.join("installer-events"), |text| {
        text.lines().any(|line| {
            serde_json::from_str::<Value>(line).is_ok_and(|event| {
                event["version"] == "2.0.0" && event["args"] == json!(["--background"])
            })
        })
    });
}

#[test]
fn hub_lifecycle_refuses_corrupt_rollback_and_cleans_first_install_failure() {
    let (server, manager) = fixture();
    publish(&server, &manager, "arcade.look", "1.0.0", true);
    assert_eq!(
        operate(&manager, "arcade.look", Operation::Install)
            .unwrap_err()
            .code,
        "installer"
    );
    assert!(manager.receipt("arcade.look").unwrap().is_none());
    assert!(!manager
        .paths
        .executable("arcade.look", Os::Linux, false)
        .exists());
    assert!(!manager
        .paths
        .data
        .join("applications/arcade-look.desktop")
        .exists());
    assert!(fs::symlink_metadata(manager.paths.home.join(".local/bin/arcade-look")).is_err());
    installed(&server, &manager, "arcade.look");
    publish(&server, &manager, "arcade.look", "2.0.0", false);
    operate(&manager, "arcade.look", Operation::Update).unwrap();
    let record: Value = serde_json::from_slice(
        &fs::read(manager.paths.state().join("arcade.look.previous.json")).unwrap(),
    )
    .unwrap();
    let package = record["record"]["package"]["path"].as_str().unwrap();
    fs::write(package, "corrupted").unwrap();
    let before = manager.receipt("arcade.look").unwrap().unwrap();
    assert_eq!(
        operate(&manager, "arcade.look", Operation::Rollback)
            .unwrap_err()
            .code,
        "checksum"
    );
    assert_eq!(manager.receipt("arcade.look").unwrap().unwrap(), before);
}
