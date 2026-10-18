#![cfg(target_os = "linux")]
mod support;
use arcade_link::content::Content;
use arcade_link::server::{Handler, InvokeContext, Reply, Server, ServerConfig};
use arcade_link::wire::{InvokeRequest, PeerInfo};
use arcade_link::{Action, LinkError, Manifest};
use arcade_tools_core::manager::Manager;
use arcade_tools_core::paths::Paths;
use arcade_tools_core::source::Source;
use arcade_tools_core::{archive, backup, diagnostics};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

struct SettingsPeer {
    output: std::path::PathBuf,
    kind: String,
    imported: Mutex<Option<Value>>,
    raw: Mutex<Option<Vec<u8>>>,
    methods: bool,
}
impl Handler for SettingsPeer {
    fn methods(&self) -> Vec<String> {
        if self.methods {
            vec!["app.settings.export".into(), "app.settings.import".into()]
        } else {
            vec![]
        }
    }
    fn describe(&self) -> Vec<Action> {
        vec![]
    }
    fn invoke(&self, _: InvokeRequest, _: &InvokeContext) -> Result<Reply, LinkError> {
        Err(LinkError::unavailable("unused"))
    }
    fn settings_export(&self) -> Result<Vec<Content>, LinkError> {
        let mut output = Content::file(&self.output);
        output.kind = self.kind.clone();
        Ok(vec![output])
    }
    fn settings_import(
        &self,
        inputs: &[Content],
    ) -> Result<arcade_link::app::ImportResult, LinkError> {
        let bytes = fs::read(inputs[0].path.as_ref().unwrap()).unwrap();
        *self.imported.lock().unwrap() = serde_json::from_slice(&bytes).ok();
        *self.raw.lock().unwrap() = Some(bytes);
        Ok(arcade_link::app::ImportResult {
            imported: true,
            restart_required: true,
        })
    }
}
fn fixture(root: &std::path::Path, source: &support::FakeRelease) -> Manager {
    assert!(
        std::env::var("HOME").unwrap().starts_with("/tmp/")
            && std::env::var_os("ARCADE_HOME").is_some(),
        "Use the Link isolated runner"
    );
    Manager::new(
        Paths::under(root),
        Source::test_server(&source.url).unwrap(),
    )
}
fn peer(manager: &Manager, id: &str, methods: bool, value: Value) -> (Server, Arc<SettingsPeer>) {
    peer_with(
        manager,
        id,
        methods,
        "file/any",
        &serde_json::to_vec(&value).unwrap(),
    )
}
fn peer_with(
    manager: &Manager,
    id: &str,
    methods: bool,
    kind: &str,
    bytes: &[u8],
) -> (Server, Arc<SettingsPeer>) {
    let exe = manager.paths.home.join(format!("{id}.AppImage"));
    fs::create_dir_all(&manager.paths.home).unwrap();
    fs::write(&exe, "fixture").unwrap();
    arcade_link::manifest::write_manifest(
        &manager.paths.link,
        &Manifest::new(id, "1.0.0", exe.to_str().unwrap()),
    )
    .unwrap();
    fs::create_dir_all(&manager.paths.link.handoff).unwrap();
    let output = manager
        .paths
        .link
        .handoff
        .join(format!("{id}-settings.json"));
    fs::write(&output, bytes).unwrap();
    let peer = Arc::new(SettingsPeer {
        output,
        kind: kind.into(),
        imported: Mutex::new(None),
        raw: Mutex::new(None),
        methods,
    });
    let server = Server::start(
        ServerConfig {
            locations: manager.paths.link.clone(),
            app: PeerInfo {
                id: id.into(),
                version: "1.0.0".into(),
            },
        },
        peer.clone(),
    )
    .unwrap();
    (server, peer)
}

#[test]
fn two_profiles_backup_restore_uses_app_contract_and_reports_unsupported_apps() {
    let source = support::FakeRelease::new();
    let first = fixture(&source.root.path().join("first"), &source);
    let second = fixture(&source.root.path().join("second"), &source);
    let settings = json!({"schema":1, "locations":["~/Documents"], "theme":"dark"});
    let (_a, _) = peer(&first, "arcade.find", true, settings.clone());
    let (_b, _) = peer(&first, "arcade.look", false, json!({}));
    let (_c, restored) = peer(&second, "arcade.find", true, json!({}));
    let (_d, _) = peer(&second, "arcade.look", false, json!({}));
    first.set_theme("dark").unwrap();
    let labels = first.paths.config.join("arcade/tools/shortcut-labels.json");
    arcade_link::paths::write_atomic(&labels, br#"{"|Super+X":"Open files"}"#, true).unwrap();
    let cancel = AtomicBool::new(false);
    let exported = backup::export(&first, &cancel).unwrap();
    assert_eq!(
        exported
            .apps
            .iter()
            .filter(|app| !app.settings.is_empty())
            .count(),
        1
    );
    assert!(exported
        .apps
        .iter()
        .find(|a| a.id == "arcade.look")
        .unwrap()
        .not_included
        .is_some());
    let archive = second.paths.home.join("transfer.zip");
    fs::copy(&exported.file, &archive).unwrap();
    let plan = backup::plan_restore(&second, &archive).unwrap();
    assert!(plan.apps.iter().all(|app| app.installed));
    let results = backup::restore(&second, &archive, &plan.sha256, &cancel).unwrap();
    assert_eq!(
        results
            .iter()
            .filter(|result| result.imported && result.restart_required)
            .count(),
        1
    );
    assert_eq!(*restored.imported.lock().unwrap(), Some(settings));
    assert_eq!(second.preferences().unwrap().theme, "dark");
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(
                second
                    .paths
                    .config
                    .join("arcade/tools/shortcut-labels.json")
            )
            .unwrap()
        )
        .unwrap(),
        serde_json::from_slice::<Value>(&fs::read(labels).unwrap()).unwrap()
    );
}

#[test]
fn backup_keeps_lens_code_handoffs_and_shelf_binary_settings() {
    let source = support::FakeRelease::new();
    let first = fixture(&source.root.path().join("first"), &source);
    let second = fixture(&source.root.path().join("second"), &source);
    // Lens hands over `file/code` JSON with a `guardSecrets` preference flag;
    // Shelf hands over its own binary archive.
    let lens = json!({"schema":1, "app":"arcade.lens", "privacy":{"historyEnabled":false, "guardSecrets":true}});
    let shelf = b"\x00\x01SHELF-binary\xff\xfe".to_vec();
    let (_a, _) = peer_with(
        &first,
        "arcade.lens",
        true,
        "file/code",
        &serde_json::to_vec(&lens).unwrap(),
    );
    let (_b, _) = peer_with(&first, "arcade.shelf", true, "file/any", &shelf);
    let (_c, lens_restored) = peer_with(&second, "arcade.lens", true, "file/code", b"{}");
    let (_d, shelf_restored) = peer_with(&second, "arcade.shelf", true, "file/any", b"{}");
    let cancel = AtomicBool::new(false);
    let exported = backup::export(&first, &cancel).unwrap();
    for app in &exported.apps {
        assert_eq!(
            app.settings.len(),
            1,
            "{} was left out: {:?}",
            app.id,
            app.not_included
        );
    }
    let archive = second.paths.home.join("transfer.zip");
    fs::copy(&exported.file, &archive).unwrap();
    let plan = backup::plan_restore(&second, &archive).unwrap();
    let results = backup::restore(&second, &archive, &plan.sha256, &cancel).unwrap();
    assert!(results.iter().all(|result| result.imported), "{results:?}");
    assert_eq!(*lens_restored.imported.lock().unwrap(), Some(lens));
    assert_eq!(*shelf_restored.raw.lock().unwrap(), Some(shelf));
}

#[test]
fn restore_skips_apps_without_settings_and_cancels_before_writing() {
    let source = support::FakeRelease::new();
    let first = fixture(&source.root.path().join("first"), &source);
    let second = fixture(&source.root.path().join("second"), &source);
    // Look is installed on the first profile but offers no settings; the
    // second profile doesn't have it.
    let (_a, _) = peer(&first, "arcade.look", false, json!({}));
    first.set_theme("dark").unwrap();
    let cancel = AtomicBool::new(false);
    let exported = backup::export(&first, &cancel).unwrap();
    let plan = backup::plan_restore(&second, &exported.file).unwrap();
    assert!(plan
        .apps
        .iter()
        .any(|app| app.backup.id == "arcade.look" && !app.installed));
    // Cancelled before anything is written: an error and no changes.
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        backup::restore(&second, &exported.file, &plan.sha256, &cancelled)
            .unwrap_err()
            .code,
        "cancelled"
    );
    assert_ne!(second.preferences().unwrap().theme, "dark");
    let results = backup::restore(&second, &exported.file, &plan.sha256, &cancel).unwrap();
    let look = results.iter().find(|r| r.app == "arcade.look").unwrap();
    assert!(!look.imported);
    // No install was attempted: the release server saw no Look request.
    assert!(!source
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(route, _)| route.to_ascii_lowercase().contains("look")));
    assert!(second.receipt("arcade.look").unwrap().is_none());
    assert_eq!(second.preferences().unwrap().theme, "dark");
}

#[test]
fn backup_rejects_secrets_escaping_handoffs_and_changed_archives() {
    let source = support::FakeRelease::new();
    let manager = fixture(&source.root.path().join("first"), &source);
    let (_server, peer) = peer(
        &manager,
        "arcade.clipboard",
        true,
        json!({"deviceKeys":"never-export"}),
    );
    let cancel = AtomicBool::new(false);
    let backup = backup::export(&manager, &cancel).unwrap();
    assert!(backup.apps[0].settings.is_empty());
    let files = archive::read(&manager.paths, &backup.file).unwrap();
    assert!(!files
        .values()
        .any(|bytes| String::from_utf8_lossy(bytes).contains("never-export")));
    let plan = backup::plan_restore(&manager, &backup.file).unwrap();
    fs::write(&backup.file, "tampered").unwrap();
    assert_eq!(
        backup::restore(&manager, &backup.file, &plan.sha256, &cancel)
            .unwrap_err()
            .code,
        "state_changed"
    );
    fs::remove_file(&peer.output).unwrap();
    std::os::unix::fs::symlink(manager.paths.home.join("secret.json"), &peer.output).unwrap();
    fs::write(manager.paths.home.join("secret.json"), "{}").unwrap();
    assert!(backup::export(&manager, &cancel).unwrap().apps[0]
        .settings
        .is_empty());
}

#[test]
fn diagnostics_are_local_and_redact_paths_tokens_and_device_data() {
    let source = support::FakeRelease::new();
    let manager = fixture(&source.root.path().join("first"), &source);
    let (_server, _) = peer(&manager, "arcade.find", false, json!({}));
    let path = manager.paths.link.manifest("arcade.find");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["settings"]["apiToken"] = json!("never-include-token");
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let history = manager.paths.state().join("update-history.jsonl");
    fs::create_dir_all(history.parent().unwrap()).unwrap();
    fs::write(
        &history,
        format!(
            "installed {}/app\nAuthorization: Bearer never-include-bearer\n",
            manager.paths.home.display()
        ),
    )
    .unwrap();
    let report = diagnostics::create(&manager, "arcade.find").unwrap();
    assert!(report
        .issue_url
        .starts_with("https://github.com/qa-p1/Arcade-Find/issues/new?"));
    let files = archive::read(&manager.paths, &report.file).unwrap();
    assert!(files.contains_key("system.json") && files.contains_key("endpoints/arcade.find.json"));
    let text = files
        .values()
        .map(|bytes| String::from_utf8_lossy(bytes))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !text.contains("never-include") && !text.contains(manager.paths.home.to_str().unwrap())
    );
    assert!(text.contains("~/app"));
    assert!(source
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(path, _)| path == "/catalog.json"));
}

#[test]
fn archives_reject_traversal_duplicate_names_and_unlisted_payloads() {
    let source = support::FakeRelease::new();
    let manager = fixture(&source.root.path().join("first"), &source);
    let files = BTreeMap::from([("../escape.json".into(), b"{}".to_vec())]);
    assert!(archive::write(&manager.paths, "invalid.zip", &files).is_err());
    let files = BTreeMap::from([
        (
            "backup.json".into(),
            serde_json::to_vec(
                &json!({"schema":1,"tools":manager.preferences().unwrap(),"labels":{},"apps":[]}),
            )
            .unwrap(),
        ),
        ("unlisted.json".into(), b"{}".to_vec()),
    ]);
    let path = archive::write(&manager.paths, "unlisted.zip", &files).unwrap();
    assert_eq!(
        backup::plan_restore(&manager, &path).unwrap_err().code,
        "backup"
    );
}
