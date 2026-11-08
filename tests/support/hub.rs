//! The hub fixture shared by the lifecycle tests: a fake release server, a manager under it, and a fake app whose
//! installer behaves like a real Arcade AppImage.
use super::FakeRelease;
use arcade_link::manifest::Manifest;
use arcade_tools_core::manager::{LaunchMode, Manager, Operation, Request};
use arcade_tools_core::paths::Paths;
use arcade_tools_core::release::Os;
use arcade_tools_core::source::Source;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub fn fixture() -> (FakeRelease, Manager) {
    super::require_isolated_session();
    let server = FakeRelease::new();
    let manager = Manager::new(
        Paths::under(&server.root.path().join("user")),
        Source::test_server(&server.url).unwrap(),
    );
    (server, manager)
}
pub fn request(id: &str, operation: Operation) -> Request {
    Request {
        id: id.into(),
        operation,
        mode: Some(LaunchMode::Background),
        remove_data: false,
        app_closed: false,
        channel: None,
        version: None,
    }
}
pub fn operate(
    manager: &Manager,
    id: &str,
    operation: Operation,
) -> arcade_tools_core::Result<String> {
    manager.operate(
        &request(id, operation),
        &AtomicBool::new(false),
        &mut |_| {},
    )
}
pub fn fake_app(paths: &Paths, id: &str, version: &str, fail: bool) -> Vec<u8> {
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
    # The shared install contract (Tools' own --install too) takes stable or nightly, nothing else.
    if '--channel' in args and args[args.index('--channel')+1] not in ('stable', 'nightly'):
        print('usage: Choose stable or nightly.', file=sys.stderr)
        sys.exit(1)
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
pub fn publish(server: &FakeRelease, manager: &Manager, id: &str, version: &str, fail: bool) {
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
pub fn wait_for(path: &Path, check: impl Fn(&str) -> bool) {
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
pub fn installed(server: &FakeRelease, manager: &Manager, id: &str) {
    publish(server, manager, id, "1.0.0", false);
    operate(manager, id, Operation::Install).unwrap();
    wait_for(
        &manager.paths.link.registry.join(format!("{id}.json")),
        |text| text.contains("1.0.0"),
    );
}
