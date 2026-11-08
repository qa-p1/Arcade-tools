//! D9: Tools' own state written by every released 0.x reads correctly after its folder moves (0.1–0.3 kept it in
//! `<data>/arcade-tools`), gains a schema version when rewritten, and a file from a newer Tools keeps its schema and
//! the fields this version does not know (the downgrade rule, README Round 5).
use arcade_tools_core::{
    manager::{Manager, PREFERENCES_SCHEMA},
    paths::Paths,
    release::Channel,
    source::Source,
    updates::{Clock, Scheduler},
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

/// The three shapes Tools 0.x wrote: 0.1.0; 0.2.0–0.2.1 (updates, theme, onboarding, tray, history); 0.3.0–0.4.6
/// (rolled-back pins). 0.1.0–0.3.8 kept them in `<data>/arcade-tools`, 0.4.0 on in `<data>/arcade/tools`.
const RELEASES: &[(&str, bool, &str)] = &[
    (
        "0.1.0",
        true,
        r#"{"linkEnabled":false,"channels":{"arcade.box":"nightly"}}"#,
    ),
    (
        "0.2.1",
        true,
        r#"{"linkEnabled":true,"channels":{"arcade.box":"nightly"},"updates":{"enabled":false,"autoUpdate":{"arcade.look":true}},"notificationsEnabled":false,"theme":"dark","onboardingComplete":true}"#,
    ),
    (
        "0.3.8",
        true,
        r#"{"linkEnabled":true,"channels":{"arcade.box":"nightly"},"updates":{"enabled":false,"autoUpdate":{"arcade.look":true},"rolledBack":{"arcade.look":"0.3.1"}},"notificationsEnabled":false,"theme":"dark","onboardingComplete":true}"#,
    ),
    (
        "0.4.6",
        false,
        r#"{"linkEnabled":true,"channels":{"arcade.box":"nightly"},"updates":{"enabled":false,"autoUpdate":{"arcade.look":true},"rolledBack":{"arcade.look":"0.3.1"}},"notificationsEnabled":false,"theme":"dark","onboardingComplete":true}"#,
    ),
];

const UPDATES: &str = r#"{"schema":1,"lastCheck":1760000000,"lastSuccess":1760000000,"lastFailure":null,"apps":{"arcade.look":{"installedVersion":"0.3.1","channel":"stable","latest":{"stable":{"latestVersion":"0.4.0","updateAvailable":true,"heldBack":false,"reason":null,"error":null,"cache":null}},"updateAvailable":true,"heldBack":false,"reason":null,"notified":["0.4.0"]}}}"#;
const HISTORY: &str = concat!(
    r#"{"time":1760000000,"app":"arcade.look","from":"0.3.0","to":"0.3.1","result":"success"}"#,
    "\n",
    r#"{"time":1760000100,"app":"arcade.look","from":"0.3.1","to":"0.4.0","result":"rolled_back","reason":"failed to start"}"#,
    "\n",
);

struct Never;
impl Clock for Never {
    fn now(&self) -> u64 {
        1_760_000_200
    }
}

fn manager(root: &Path) -> Manager {
    Manager::new(
        Paths::under(root),
        Source::test_server("http://127.0.0.1:9/").unwrap(),
    )
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn every_released_state_is_read_after_its_folder_moves() {
    for (version, legacy, preferences) in RELEASES {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let folder = if *legacy {
            paths.data.join("arcade-tools")
        } else {
            paths.state()
        };
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("preferences.json"), preferences).unwrap();
        if *version != "0.1.0" {
            fs::write(folder.join("updates.json"), UPDATES).unwrap();
            fs::write(folder.join("update-history.jsonl"), HISTORY).unwrap();
        }
        let moved = paths.migrate_own_folders();
        assert!(moved.kept.is_empty(), "{version}: kept {:?}", moved.kept);

        let tools = manager(root.path());
        let prefs = tools.preferences().unwrap();
        assert_eq!(prefs.schema, PREFERENCES_SCHEMA, "{version}");
        assert_eq!(
            prefs.channels.get("arcade.box"),
            Some(&Channel::Nightly),
            "{version}"
        );
        if *version == "0.1.0" {
            assert!(!prefs.link_enabled);
            assert_eq!(prefs.theme, "system");
            continue;
        }
        assert!(
            !prefs.updates.enabled && !prefs.notifications_enabled && prefs.onboarding_complete
        );
        assert_eq!(prefs.theme, "dark", "{version}");
        assert_eq!(
            prefs.updates.auto_update.get("arcade.look"),
            Some(&true),
            "{version}"
        );
        if *version != "0.2.1" {
            assert_eq!(
                prefs
                    .updates
                    .rolled_back
                    .get("arcade.look")
                    .map(String::as_str),
                Some("0.3.1")
            );
        }
        // Rewriting stamps the schema and keeps every setting.
        tools.set_theme("light").unwrap();
        let written = json_file(&paths.state().join("preferences.json"));
        assert_eq!(written["schema"], PREFERENCES_SCHEMA, "{version}");
        assert_eq!(written["channels"]["arcade.box"], "nightly", "{version}");
        assert_eq!(
            written["updates"]["autoUpdate"]["arcade.look"], true,
            "{version}"
        );

        let scheduler = Scheduler::new(
            paths.clone(),
            Never,
            Source::test_server("http://127.0.0.1:9/").unwrap(),
        )
        .unwrap_or_else(|e| panic!("{version}: updates.json: {e}"));
        let look = &scheduler.state().apps["arcade.look"];
        assert_eq!(
            look.installed_version.as_deref(),
            Some("0.3.1"),
            "{version}"
        );
        assert!(look.notified.contains("0.4.0"), "{version}");
        let history = tools.history().unwrap();
        assert_eq!(history.len(), 2, "{version}");
    }
}

#[test]
fn a_newer_preferences_file_keeps_its_schema_and_unknown_fields() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    fs::create_dir_all(paths.state()).unwrap();
    let file = paths.state().join("preferences.json");
    fs::write(
        &file,
        r#"{"schema":3,"theme":"dark","channels":{"arcade.box":"nightly"},"futureSetting":{"keep":true},"updates":{"enabled":true}}"#,
    )
    .unwrap();
    let tools = manager(root.path());
    assert_eq!(tools.preferences().unwrap().schema, 3);
    tools.set_notifications(false).unwrap();
    let written = json_file(&file);
    assert_eq!(written["schema"], 3);
    assert_eq!(written["futureSetting"], json!({"keep": true}));
    assert_eq!(written["notificationsEnabled"], false);
    assert_eq!(written["channels"]["arcade.box"], "nightly");
}

#[test]
fn a_newer_update_state_is_read_not_refused() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    fs::create_dir_all(paths.state()).unwrap();
    let mut newer: Value = serde_json::from_str(UPDATES).unwrap();
    newer["schema"] = json!(2);
    newer["futureField"] = json!([1, 2]);
    fs::write(paths.state().join("updates.json"), newer.to_string()).unwrap();
    let scheduler = Scheduler::new(
        paths,
        Never,
        Source::test_server("http://127.0.0.1:9/").unwrap(),
    )
    .unwrap();
    assert_eq!(scheduler.state().schema, 2);
    assert_eq!(scheduler.state().extra["futureField"], json!([1, 2]));
    assert!(scheduler.state().apps.contains_key("arcade.look"));
}
