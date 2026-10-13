mod support;
use arcade_tools_core::{
    catalog::{Catalog, SNAPSHOT},
    manager::{Manager, Preferences},
    paths::Paths,
    release::{Arch, Channel, Os},
    source::Source,
    updates::*,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Default)]
struct FakeClock(Arc<AtomicU64>);
impl Clock for FakeClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
impl FakeClock {
    fn set(&self, time: u64) {
        self.0.store(time, Ordering::SeqCst);
    }
}
fn catalog() -> Catalog {
    let mut value: serde_json::Value = serde_json::from_str(SNAPSHOT).unwrap();
    for app in value["apps"].as_array_mut().unwrap() {
        app["minTools"] = json!("0.1.0");
    }
    Catalog::parse(&serde_json::to_vec(&value).unwrap(), "0.1.0").unwrap()
}
fn installed() -> Vec<InstalledApp> {
    vec![InstalledApp {
        id: "arcade.look".into(),
        version: "1.0.0".into(),
        channel: Channel::Stable,
        selected_channel: Channel::Stable,
        sha256: None,
    }]
}
fn context<'a>(
    settings: &'a UpdateSettings,
    catalog: &'a Catalog,
    installed: &'a [InstalledApp],
    peers: &'a [PeerProtocols],
) -> CheckContext<'a> {
    CheckContext {
        settings,
        catalog,
        installed,
        peers,
        online: true,
        os: Os::Linux,
        arch: Arch::X64,
    }
}
fn publish(server: &support::FakeRelease, version: &str, channel: &str) {
    server.publish(
        "arcade.look",
        version,
        channel,
        &[("Look_x64.AppImage", version.as_bytes())],
        "nsis",
    );
}
#[test]
fn stable_build_metadata_reconciles_hashes_and_preserves_display_versions() {
    let server = support::FakeRelease::new();
    publish(&server, "0.3.0+build.40", "stable");
    publish(&server, "0.3.0-nightly", "nightly");
    let mut scheduler = Scheduler::new(
        Paths::under(&server.root.path().join("scheduler")),
        FakeClock::default(),
        Source::test_server(&server.url).unwrap(),
    )
    .unwrap();
    let settings = UpdateSettings::default();
    let catalog = catalog();
    let mut installed = installed();
    installed[0].version = "0.3.0".into();
    let outcome = scheduler
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &[]),
        )
        .unwrap();
    assert!(outcome.failure.is_none() && outcome.notifications.is_empty());
    let state = &scheduler.state().apps["arcade.look"];
    assert!(!state.update_available);
    assert_eq!(state.installed_version.as_deref(), Some("0.3.0"));
    assert_eq!(
        state.latest["stable"].latest_version.as_deref(),
        Some("0.3.0+build.40")
    );
    installed[0].sha256 = Some(
        state.latest["stable"]
            .cache
            .as_ref()
            .unwrap()
            .asset
            .sha256
            .clone(),
    );
    scheduler.reconcile(&catalog, &installed, &[]);
    assert!(!scheduler.state().apps["arcade.look"].update_available);
    installed[0].sha256 = Some("a".repeat(64));
    let incompatible = [PeerProtocols {
        id: "arcade.box".into(),
        protocols: vec![2],
    }];
    scheduler.reconcile(&catalog, &installed, &incompatible);
    assert!(scheduler.state().apps["arcade.look"].update_available);
    assert!(scheduler.state().apps["arcade.look"].held_back);
    scheduler.reconcile(&catalog, &installed, &[]);
    assert!(!scheduler.state().apps["arcade.look"].held_back);
    assert_eq!(
        scheduler
            .run(
                Trigger::Manual,
                context(&settings, &catalog, &installed, &[]),
            )
            .unwrap()
            .notifications
            .len(),
        1
    );
}

#[test]
fn timer_window_offline_disabled_and_restart_notifications_are_deterministic() {
    let server = support::FakeRelease::new();
    publish(&server, "2.0.0", "stable");
    publish(&server, "2.1.0-nightly", "nightly");
    let paths = Paths::under(&server.root.path().join("scheduler"));
    let clock = FakeClock::default();
    let source = Source::test_server(&server.url).unwrap();
    let mut scheduler = Scheduler::new(paths.clone(), clock.clone(), source.clone()).unwrap();
    let settings = UpdateSettings::default();
    let catalog = catalog();
    let installed = installed();
    let count = || server.requests.lock().unwrap().len();
    assert_eq!(scheduler.next_deadline(&settings), Some(60));
    let before = count();
    clock.set(59);
    assert!(
        !scheduler
            .run(
                Trigger::Timer,
                context(&settings, &catalog, &installed, &[])
            )
            .unwrap()
            .checked
    );
    let disabled = UpdateSettings {
        enabled: false,
        ..Default::default()
    };
    clock.set(60);
    assert_eq!(scheduler.next_deadline(&disabled), None);
    assert!(
        !scheduler
            .run(
                Trigger::WindowOpen,
                context(&disabled, &catalog, &installed, &[])
            )
            .unwrap()
            .checked
    );
    let mut offline = context(&settings, &catalog, &installed, &[]);
    offline.online = false;
    assert!(scheduler
        .run(Trigger::Timer, offline)
        .unwrap()
        .failure
        .unwrap()
        .contains("Offline"));
    assert_eq!(count(), before);
    assert_eq!(scheduler.state().last_check, Some(60));
    assert_eq!(
        scheduler.next_deadline(&settings),
        Some(60 + CHECK_INTERVAL)
    );
    clock.set(60 + CHECK_INTERVAL);
    let outcome = scheduler
        .run(
            Trigger::Timer,
            context(&settings, &catalog, &installed, &[]),
        )
        .unwrap();
    assert_eq!(outcome.notifications.len(), 1);
    assert_eq!(outcome.notifications[0].version, "2.0.0");
    assert_eq!(scheduler.state().apps["arcade.look"].latest.len(), 2);
    let before = count();
    let checked_at = clock.now();
    clock.set(checked_at + WINDOW_MAX_AGE);
    assert!(
        !scheduler
            .run(
                Trigger::WindowOpen,
                context(&settings, &catalog, &installed, &[])
            )
            .unwrap()
            .checked
    );
    clock.set(checked_at + WINDOW_MAX_AGE + 1);
    assert!(scheduler
        .run(
            Trigger::WindowOpen,
            context(&settings, &catalog, &installed, &[])
        )
        .unwrap()
        .notifications
        .is_empty());
    assert_eq!(
        count() - before,
        2,
        "304 checks must skip both release manifests"
    );
    let mut restarted = Scheduler::new(paths.clone(), clock.clone(), source).unwrap();
    assert_eq!(restarted.next_deadline(&settings), Some(clock.now() + 60));
    assert!(restarted
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &[])
        )
        .unwrap()
        .notifications
        .is_empty());
    publish(&server, "3.0.0", "stable");
    assert_eq!(
        restarted
            .run(
                Trigger::Manual,
                context(&settings, &catalog, &installed, &[])
            )
            .unwrap()
            .notifications
            .len(),
        1
    );
    let persisted: UpdateState =
        serde_json::from_slice(&std::fs::read(paths.state().join("updates.json")).unwrap())
            .unwrap();
    assert_eq!(persisted.apps["arcade.look"].notified.len(), 2);
    let mut current = installed.clone();
    current[0].version = "3.0.0".into();
    let before = count();
    restarted.reconcile(&catalog, &current, &[]);
    assert_eq!(
        restarted.state().apps["arcade.look"]
            .installed_version
            .as_deref(),
        Some("3.0.0")
    );
    assert!(!restarted.state().apps["arcade.look"].update_available);
    assert_eq!(
        count(),
        before,
        "registry/install events must not check the network"
    );
}

#[test]
fn compatibility_holds_are_recomputed_on_304_and_bad_manifests_preserve_cache() {
    let server = support::FakeRelease::new();
    publish(&server, "2.0.0", "stable");
    publish(&server, "2.1.0-nightly", "nightly");
    let mut scheduler = Scheduler::new(
        Paths::under(&server.root.path().join("scheduler")),
        FakeClock::default(),
        Source::test_server(&server.url).unwrap(),
    )
    .unwrap();
    let settings = UpdateSettings::default();
    let installed = installed();
    let mut catalog = catalog();
    let peers = [PeerProtocols {
        id: "arcade.box".into(),
        protocols: vec![2],
    }];
    assert!(scheduler
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &peers)
        )
        .unwrap()
        .notifications
        .is_empty());
    assert!(scheduler.state().apps["arcade.look"]
        .reason
        .as_ref()
        .unwrap()
        .contains("arcade.box"));
    assert!(scheduler.state().apps["arcade.look"].update_available);
    assert_eq!(
        scheduler
            .run(
                Trigger::Manual,
                context(&settings, &catalog, &installed, &[])
            )
            .unwrap()
            .notifications
            .len(),
        1
    );
    catalog
        .apps
        .iter_mut()
        .find(|a| a.id == "arcade.look")
        .unwrap()
        .install_reason = Some("Update Arcade Tools to install this".into());
    scheduler
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &[]),
        )
        .unwrap();
    assert!(scheduler.state().apps["arcade.look"].held_back);
    assert_eq!(
        scheduler.state().apps["arcade.look"].latest["stable"]
            .latest_version
            .as_deref(),
        Some("2.0.0")
    );
    let cache = scheduler.state().apps["arcade.look"].latest["stable"]
        .cache
        .clone()
        .unwrap();
    publish(&server, "3.0.0", "stable");
    server.routes.lock().unwrap().insert(
        "/assets/arcade-look-3.0.0-stable/arcade-release.json".into(),
        br#"{"schema":2}"#.to_vec(),
    );
    assert!(scheduler
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &[])
        )
        .unwrap()
        .failure
        .is_some());
    assert_eq!(
        scheduler.state().apps["arcade.look"].latest["stable"]
            .cache
            .as_ref()
            .unwrap()
            .etag,
        cache.etag
    );
    assert_eq!(
        scheduler.state().apps["arcade.look"].latest["stable"]
            .latest_version
            .as_deref(),
        Some("2.0.0")
    );
}

struct OfflineSource(Arc<AtomicUsize>);
impl ReleaseSource for OfflineSource {
    fn check_release(
        &self,
        _: ReleaseRequest<'_>,
    ) -> arcade_tools_core::Result<arcade_tools_core::source::ReleaseCheck> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(arcade_tools_core::Error::new("network", "offline test"))
    }
}
#[test]
fn transport_failure_stops_the_check_and_future_state_schemas_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = FakeClock::default();
    let mut scheduler =
        Scheduler::new(paths.clone(), clock.clone(), OfflineSource(calls.clone())).unwrap();
    let settings = UpdateSettings::default();
    let catalog = catalog();
    let installed = installed();
    assert!(scheduler
        .run(
            Trigger::Manual,
            context(&settings, &catalog, &installed, &[])
        )
        .unwrap()
        .failure
        .is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(scheduler.state().last_failure.is_some());
    let mut state = serde_json::to_value(scheduler.state()).unwrap();
    state["schema"] = json!(2);
    std::fs::write(
        paths.state().join("updates.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert!(Scheduler::new(paths, clock, OfflineSource(calls)).is_err());
}

#[test]
fn auto_update_is_opt_in_requires_known_idle_state_and_ten_minutes() {
    let idle = json!({"busy":false});
    let busy = json!({"status":{"busy":true}});
    assert!(!auto_update_policy(false, &idle, Some(0), 1000).eligible);
    assert!(!auto_update_policy(true, &busy, Some(0), 1000).eligible);
    assert!(
        !auto_update_policy(
            true,
            &json!({"busy":true,"status":{"busy":false}}),
            Some(0),
            1000
        )
        .eligible
    );
    assert!(!auto_update_policy(true, &json!({}), Some(0), 1000).eligible);
    assert!(!auto_update_policy(true, &idle, None, 1000).eligible);
    assert!(!auto_update_policy(true, &idle, Some(401), 1000).eligible);
    assert!(auto_update_policy(true, &idle, Some(400), 1000).eligible);
    assert!(!auto_update_policy(true, &idle, Some(1001), 1000).eligible);
}

#[test]
fn preferences_migrate_and_update_settings_persist_without_network() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    std::fs::create_dir_all(paths.state()).unwrap();
    std::fs::write(
        paths.state().join("preferences.json"),
        br#"{"linkEnabled":false,"channels":{},"extra":true}"#,
    )
    .unwrap();
    let manager = Manager::new(paths, Source::github().unwrap());
    assert!(manager.preferences().unwrap().updates.enabled);
    assert!(!manager.preferences().unwrap().link_enabled);
    manager.set_updates_enabled(false).unwrap();
    manager.set_auto_update("arcade.look", true).unwrap();
    let prefs: Preferences = manager.preferences().unwrap();
    assert!(!prefs.updates.enabled && prefs.updates.auto_update["arcade.look"]);
}

#[test]
fn release_etag_cache_is_bound_to_source_platform_and_verified_metadata() {
    let server = support::FakeRelease::new();
    publish(&server, "2.0.0", "stable");
    let source = Source::test_server(&server.url).unwrap();
    let first = source
        .check_repo_conditional(
            "arcade.look",
            "qa-p1/arcade-look",
            Channel::Stable,
            Os::Linux,
            Arch::X64,
            None,
        )
        .unwrap();
    let before = server.requests.lock().unwrap().len();
    let check = source
        .check_repo_conditional(
            "arcade.look",
            "qa-p1/arcade-look",
            Channel::Stable,
            Os::Linux,
            Arch::X64,
            Some(&first.cache),
        )
        .unwrap();
    assert!(check.unchanged);
    assert_eq!(server.requests.lock().unwrap().len() - before, 1);
    let mut cache = first.cache.clone();
    cache.asset.sha256 = "a".repeat(64);
    let check = source
        .check_repo_conditional(
            "arcade.look",
            "qa-p1/arcade-look",
            Channel::Stable,
            Os::Linux,
            Arch::X64,
            Some(&cache),
        )
        .unwrap();
    assert!(!check.unchanged);
    assert_eq!(check.checked.asset.sha256, first.checked.asset.sha256);
    cache = first.cache;
    cache.schema = 9;
    assert!(
        !source
            .check_repo_conditional(
                "arcade.look",
                "qa-p1/arcade-look",
                Channel::Stable,
                Os::Linux,
                Arch::X64,
                Some(&cache)
            )
            .unwrap()
            .unchanged
    );
}
