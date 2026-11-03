#![cfg(target_os = "linux")]
use arcade_link::receipt::{InstallMethod, Integration, ManagedBy, Receipt, Store};
use arcade_tools_core::{manager::Manager, paths::Paths, source::Source};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture {
    _root: tempfile::TempDir,
    manager: Manager,
    receipt: Receipt,
    count: PathBuf,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let app = paths.home.join("Applications/Arcade/Arcade-Look.AppImage");
    fs::create_dir_all(app.parent().unwrap()).unwrap();
    let count = paths.home.join("repair-count");
    let script = r#"#!/usr/bin/env python3
import json, os, pathlib, sys
assert sys.argv[1:] == ['--repair']
root = pathlib.Path(os.environ['ARCADE_HOME'])
count = root / 'repair-count'
count.write_text(count.read_text() + 'repair\n' if count.exists() else 'repair\n')
if (root / 'fail-repair').exists(): sys.exit(9)
file = root / 'installs/arcade.look.json'
r = json.loads(file.read_text())
entry = pathlib.Path(r['integration']['desktopEntry'])
entry.parent.mkdir(parents=True, exist_ok=True)
entry.write_text('[Desktop Entry]\nType=Application\nName=Arcade Look\nExec="' + r['path'] + '" %U\nX-Arcade-Id=arcade.look\n')
cli = pathlib.Path(r['integration']['cli'])
cli.parent.mkdir(parents=True, exist_ok=True)
if cli.is_symlink(): cli.unlink()
cli.symlink_to(r['path'])
if r['integration'].get('autostart'):
    login = pathlib.Path(r['integration']['autostart'])
    login.parent.mkdir(parents=True, exist_ok=True)
    login.write_text('[Desktop Entry]\nType=Application\nExec="' + r['path'] + '" --service\nX-Arcade-Id=arcade.look\nHidden=false\n')
registry = root / 'apps/arcade.look.json'
registry.parent.mkdir(parents=True, exist_ok=True)
registry.write_text(json.dumps({'schema':1,'id':'arcade.look','name':'Arcade Look','version':r['version'],'executable':r['path'],'link':{'protocol':[1]},'actions':[],'writtenAt':'2026-11-03T00:00:00Z'}))
r.pop('futureMetadata', None)
file.write_text(json.dumps(r))
"#;
    fs::write(&app, script).unwrap();
    fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
    let receipt = Receipt {
        schema: 1,
        id: "arcade.look".into(),
        version: "0.4.1".into(),
        channel: "stable".into(),
        method: InstallMethod::Appimage,
        managed_by: ManagedBy::Tools,
        path: app,
        integration: Integration {
            desktop_entry: Some(paths.data.join("applications/dev.arcade.Look.desktop")),
            cli: Some(paths.home.join(".local/bin/arcade-look")),
            ..Default::default()
        },
        previous: None,
        installed_at: "2026-11-03T00:00:00Z".into(),
        updated_at: "2026-11-03T00:00:00Z".into(),
    };
    Store::new(&paths.link).write(&receipt).unwrap();
    let manager = Manager::new(paths, Source::test_server("http://127.0.0.1:9/").unwrap());
    Fixture {
        _root: root,
        manager,
        receipt,
        count,
    }
}
fn desktop(f: &Fixture) -> PathBuf {
    f.receipt.integration.desktop_entry.clone().unwrap()
}

#[test]
fn missing_entries_are_repaired_offline_without_a_fix_action_and_can_be_repaired_again() {
    let f = fixture();
    let before = fs::read(&f.receipt.path).unwrap();
    let report = f.manager.maintain_integrations().unwrap();
    assert_eq!(report.repaired, ["arcade.look"]);
    assert!(report.pending.is_empty());
    assert!(desktop(&f).is_file());
    assert!(f.receipt.integration.cli.as_ref().unwrap().exists());
    assert_eq!(fs::read(&f.receipt.path).unwrap(), before);
    fs::remove_file(desktop(&f)).unwrap();
    assert_eq!(
        f.manager.maintain_integrations().unwrap().repaired,
        ["arcade.look"]
    );
    assert_eq!(fs::read_to_string(f.count).unwrap().lines().count(), 2);
}

#[test]
fn stale_main_launcher_is_not_hidden_by_a_valid_settings_action() {
    let f = fixture();
    f.manager.maintain_integrations().unwrap();
    fs::write(desktop(&f), format!("[Desktop Entry]\nType=Application\nExec=/missing/old-app\nX-Arcade-Id=arcade.look\n[Desktop Action Settings]\nExec=\"{}\" --settings\n", f.receipt.path.display())).unwrap();
    assert_eq!(
        f.manager.maintain_integrations().unwrap().repaired,
        ["arcade.look"]
    );
    assert!(!fs::read_to_string(desktop(&f))
        .unwrap()
        .contains("/missing/old-app"));
}

#[test]
fn custom_healthy_launcher_disabled_login_and_future_receipt_fields_survive() {
    let mut f = fixture();
    let entry = desktop(&f);
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    let custom = format!("[Desktop Entry]\nType=Application\nExec=\"{}\" %U\nX-Arcade-Id=arcade.look\nComment=My custom launcher\n", f.receipt.path.display());
    fs::write(&entry, &custom).unwrap();
    let login = f
        .manager
        .paths
        .config
        .join("autostart/dev.arcade.Look.desktop");
    fs::create_dir_all(login.parent().unwrap()).unwrap();
    let disabled = "[Desktop Entry]\nType=Application\nX-Arcade-Id=arcade.look\nHidden=true\n";
    fs::write(&login, disabled).unwrap();
    f.receipt.integration.autostart = Some(login.clone());
    let store = Store::new(&f.manager.paths.link);
    store.write(&f.receipt).unwrap();
    let file = store.path("arcade.look").unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    json["futureMetadata"] = serde_json::json!({"keep": true});
    fs::write(&file, json.to_string()).unwrap();
    assert_eq!(
        f.manager.maintain_integrations().unwrap().repaired,
        ["arcade.look"]
    );
    assert_eq!(fs::read_to_string(entry).unwrap(), custom);
    assert_eq!(fs::read_to_string(login).unwrap(), disabled);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(json["futureMetadata"]["keep"], true);
    assert!(f
        .manager
        .paths
        .state()
        .join("maintenance/backups/arcade.look")
        .is_dir());
}

#[test]
fn deleting_a_login_entry_does_not_enable_start_at_login_during_an_unrelated_repair() {
    let mut f = fixture();
    let login = f
        .manager
        .paths
        .config
        .join("autostart/dev.arcade.Look.desktop");
    f.receipt.integration.autostart = Some(login.clone());
    Store::new(&f.manager.paths.link).write(&f.receipt).unwrap();
    assert_eq!(
        f.manager.maintain_integrations().unwrap().repaired,
        ["arcade.look"]
    );
    assert!(!login.exists());
    assert!(Store::new(&f.manager.paths.link)
        .read("arcade.look")
        .unwrap()
        .unwrap()
        .integration
        .autostart
        .is_none());
}

#[test]
fn foreign_entries_are_preserved_and_failed_repairs_do_not_loop() {
    let f = fixture();
    let entry = desktop(&f);
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    fs::write(
        &entry,
        "[Desktop Entry]\nX-Arcade-Id=another.app\nExec=/bin/true\n",
    )
    .unwrap();
    assert_eq!(f.manager.maintain_integrations().unwrap().pending.len(), 1);
    assert!(!f.count.exists());
    assert!(fs::read_to_string(entry).unwrap().contains("another.app"));
    let f = fixture();
    fs::write(f.manager.paths.home.join("fail-repair"), "1").unwrap();
    assert_eq!(f.manager.maintain_integrations().unwrap().pending.len(), 1);
    assert!(f
        .manager
        .maintain_integrations()
        .unwrap()
        .pending
        .is_empty());
    assert_eq!(fs::read_to_string(f.count).unwrap().lines().count(), 1);
}

#[test]
fn one_bad_receipt_does_not_block_another_apps_automatic_repair() {
    let f = fixture();
    let file = Store::new(&f.manager.paths.link)
        .path("arcade.box")
        .unwrap();
    fs::write(file, "broken JSON").unwrap();
    let report = f.manager.maintain_integrations().unwrap();
    assert_eq!(report.repaired, ["arcade.look"]);
    assert_eq!(report.pending[0].0, "arcade.box");
}
