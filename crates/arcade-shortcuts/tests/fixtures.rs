use arcade_shortcuts::{
    build_index, conf,
    hyprland::{self, ConfigKind, HyprlandReport},
    index::Index,
    labels, lua, runtime, BindState, BuildInputs, Os, ReadPolicy,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn owner() -> HyprlandReport {
    let home = fixture("hypr/caelestia/home");
    let config = home.join(".config");
    let hypr = config.join("hypr");
    let mut options = lua::SandboxOptions::new(&hypr);
    // These assertions test parsed content. Debug builds and parallel CI need
    // scheduler headroom; dedicated sandbox tests enforce the production limit.
    options.duration_limit = Duration::from_secs(2);
    options.read_policy = ReadPolicy::new([config.clone()]);
    options
        .environment
        .insert("HOME".into(), Some(home.to_string_lossy().into_owned()));
    options.environment.insert(
        "XDG_CONFIG_HOME".into(),
        Some(config.to_string_lossy().into_owned()),
    );
    let config = lua::evaluate(&hypr.join("hyprland.lua"), &options);
    assert!(
        config.binds.len() >= 140,
        "{} binds, warnings: {:?}",
        config.binds.len(),
        config.warnings
    );
    let runtime = runtime::parse_json(
        &std::fs::read_to_string(fixture("hypr/hyprctl-binds-caelestia.json")).unwrap(),
    )
    .unwrap();
    hyprland::merge(
        ConfigKind::Lua,
        config,
        Some(&runtime),
        &labels::UserLabels::new(),
    )
}

fn inputs(report: HyprlandReport) -> BuildInputs {
    BuildInputs {
        os: Os::Linux,
        catalog_dir: Some(fixture("catalog")),
        arcade_docs: vec![fixture("docs/find-shortcuts.json")],
        registry_dir: Some(fixture("registry")),
        hyprland: Some(report),
        user_labels: None,
        built_at: Some("2026-10-11T12:00:00Z".into()),
    }
}

#[test]
fn owner_config_labels_origins_and_runtime_reconciliation() {
    let report = owner();
    let bind = |chord: &str| {
        report
            .binds
            .iter()
            .find(|b| b.chord.as_deref() == Some(chord))
            .unwrap()
    };
    assert_eq!(bind("Super").label, "Launcher");
    assert_eq!(bind("Super").flags.get("release"), Some(&true));
    assert_eq!(bind("Super").origin.as_ref().unwrap().line, 54);
    assert_eq!(bind("Super+T").label, "Terminal");
    assert_eq!(bind("Super+T").detail.as_deref(), Some("Run: foot"));
    assert_eq!(bind("Ctrl+Super+Space").category.as_deref(), Some("Media"));
    assert_eq!(bind("Super+1").label, "Go to workspace");
    assert_eq!(bind("Alt+Super+L").label, "Restore lock");
    let active = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::Active)
        .count();
    let configured = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::ConfiguredOnly)
        .count();
    let runtime = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::RuntimeOnly)
        .count();
    eprintln!("Owner fixture: active={active}, configured-only={configured}, runtime-only={runtime}, warnings={:?}", report.warnings);
    for b in &report.binds {
        if b.state != BindState::Active {
            eprintln!("Mismatch: {:?} {:?} {}", b.state, b.chord, b.label);
        }
    }
    assert_eq!((active, configured, runtime), (140, 0, 5));
    assert!(report.warnings.iter().any(|w| w.contains("hypr-user")));
    assert!(report.warnings.iter().any(|w| w.contains("hypremoji")));
    assert!(
        report
            .warnings
            .iter()
            .all(|w| w.contains("module not found")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn upstream_example_lua() {
    let dir = fixture("hypr");
    let mut options = lua::SandboxOptions::new(&dir);
    options.read_policy = ReadPolicy::new([dir.clone()]);
    let config = lua::evaluate(&dir.join("example-hyprland.lua"), &options);
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    assert_eq!(config.binds.len(), 48);
    let report = hyprland::merge(ConfigKind::Lua, config, None, &labels::UserLabels::new());
    assert!(report
        .binds
        .iter()
        .all(|b| b.state == BindState::ConfiguredOnly));
    assert!(report.binds.iter().any(|b| b.label == "Go to workspace 3"));
    assert!(report
        .binds
        .iter()
        .any(|b| b.label == "Move window with the mouse"));
    let index = build_index(&BuildInputs {
        hyprland: Some(report),
        ..Default::default()
    });
    assert_eq!(
        index
            .wm
            .groups
            .iter()
            .flat_map(|g| &g.shortcuts)
            .filter(|b| b.chord == "MediaPlayPause")
            .count(),
        1
    );
}

#[test]
fn hyprlang_fixture_with_source_variables_bindd_and_submaps() {
    let dir = fixture("hypr/conf");
    let config = conf::parse(
        &dir.join("hyprland.conf"),
        &conf::ConfOptions {
            read_policy: ReadPolicy::new([dir.clone()]),
            ..Default::default()
        },
    );
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    assert_eq!(config.binds.len(), 7);
    let report = hyprland::merge(ConfigKind::Conf, config, None, &labels::UserLabels::new());
    assert_eq!(report.binds[0].label, "Terminal");
    assert_eq!(report.binds[0].detail.as_deref(), Some("Run: foot"));
    assert_eq!(report.binds[0].category.as_deref(), Some("Apps"));
    assert_eq!(report.binds[5].label, "Grow window");
    assert_eq!(report.binds[5].submap.as_deref(), Some("resize"));
    assert!(report
        .binds
        .iter()
        .all(|b| b.chord.as_deref() != Some("Super+X")));
}

#[test]
fn complete_index_catalog_docs_globals_overrides_conflicts_and_round_trip() {
    let mut inputs = inputs(owner());
    inputs.user_labels = Some(fixture("shortcut-labels.json"));
    let index = build_index(&inputs);
    assert_eq!(index.schema, 1);
    assert_eq!(
        index.apps.iter().filter(|a| a.source == "catalog").count(),
        31
    );
    assert_eq!(index.arcade.len(), 5);
    let find = index
        .arcade
        .iter()
        .find(|s| s.app == "arcade.find" && s.id == "toggle")
        .unwrap();
    assert_eq!(find.title, "Show or hide Find");
    assert!(find.rebindable);
    let firefox = index.apps.iter().find(|a| a.id == "firefox").unwrap();
    assert!(firefox.matches.contains_key("class"));
    assert_eq!(firefox.matches.len(), 1);
    let wm = |chord: &str| {
        index
            .wm
            .groups
            .iter()
            .flat_map(|g| &g.shortcuts)
            .find(|s| s.chord == chord)
            .unwrap()
    };
    assert_eq!(wm("Super+T").label, "My terminal");
    assert_eq!(wm("Super+T").detail.as_deref(), Some("Run: foot"));
    assert_eq!(wm("Ctrl+Super+Space").label, "Music controls");
    assert_eq!(index.wm.groups.last().unwrap().title, "Other");
    assert!(index
        .conflicts
        .iter()
        .any(|c| c.chord == "Ctrl+Alt+V" && c.b.kind == "wm" && c.b.label == "Audio settings"));
    assert!(index
        .conflicts
        .iter()
        .any(|c| c.chord == "Ctrl+Alt+F" && c.b.kind == "arcade"));
    assert!(index
        .conflicts
        .iter()
        .any(|c| c.chord == "Ctrl+K" && c.b.kind == "arcade"));
    let encoded = serde_json::to_value(&index).unwrap();
    assert!(encoded.get("builtAt").is_some());
    assert_eq!(encoded["wm"]["configKind"], "lua");
    assert_eq!(
        encoded["wm"]["groups"][0]["shortcuts"][0]["state"],
        "active"
    );
    assert_eq!(index, serde_json::from_value::<Index>(encoded).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.json");
    index.write_index(&path).unwrap();
    assert_eq!(
        index,
        serde_json::from_slice::<Index>(&std::fs::read(path).unwrap()).unwrap()
    );
    assert!(
        index
            .wm
            .warnings
            .iter()
            .all(|w| w.contains("module not found")),
        "{:?}",
        index.wm.warnings
    );

    inputs.os = Os::Macos;
    let mac = build_index(&inputs);
    let firefox = mac.apps.iter().find(|a| a.id == "firefox").unwrap();
    assert!(firefox.matches.contains_key("bundle"));
    let find = mac.apps.iter().find(|a| a.id == "arcade.find").unwrap();
    assert_eq!(find.groups[1].shortcuts[0].keys, ["Super+Enter"]);
    assert_eq!(find.groups[1].shortcuts[0].display, ["⌘Enter"]);
}

#[test]
fn owner_overrides_preserve_disabled_bind_and_do_not_create_optional_files() {
    fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let out = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_tree(&entry.path(), &out);
            } else {
                std::fs::copy(entry.path(), out).unwrap();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    copy_tree(&fixture("hypr/caelestia/home"), &home);
    let config = home.join(".config");
    std::fs::create_dir_all(config.join("caelestia")).unwrap();
    let overrides = config.join("caelestia/hypr-vars.lua");
    std::fs::copy(fixture("hypr/caelestia/owner-overrides.lua"), &overrides).unwrap();
    let original = std::fs::read(&overrides).unwrap();
    let mut options = lua::SandboxOptions::new(config.join("hypr"));
    options.duration_limit = Duration::from_secs(2);
    options.read_policy = ReadPolicy::new([config.clone()]);
    options
        .environment
        .insert("HOME".into(), Some(home.to_string_lossy().into_owned()));
    options.environment.insert(
        "XDG_CONFIG_HOME".into(),
        Some(config.to_string_lossy().into_owned()),
    );
    let result = lua::evaluate(&config.join("hypr/hyprland.lua"), &options);
    assert_eq!(result.binds.len(), 139);
    assert!(result
        .binds
        .iter()
        .all(|b| b.chord.as_deref() != Some("Shift+Super+S")));
    assert_eq!(std::fs::read(overrides).unwrap(), original);
    assert!(!config.join("caelestia/hypr-user.lua").exists());
}

#[test]
#[ignore = "run in release mode on the benchmark machine"]
fn measure_owner_and_31_catalog_sheets() {
    let mut timings = Vec::new();
    let mut evaluate_times = Vec::new();
    let mut build_times = Vec::new();
    let mut write_times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let report = owner();
        let evaluated = Instant::now();
        let index = build_index(&inputs(report));
        let built = Instant::now();
        // Publishing cost includes serialization, flush/fsync and rename.
        let dir = tempfile::tempdir().unwrap();
        index.write_index(&dir.path().join("index.json")).unwrap();
        evaluate_times.push(evaluated.duration_since(start));
        build_times.push(built.duration_since(evaluated));
        write_times.push(built.elapsed());
        timings.push(start.elapsed());
        assert_eq!(index.apps.len(), 32);
    }
    timings.sort();
    evaluate_times.sort();
    build_times.sort();
    write_times.sort();
    eprintln!(
        "Stage medians: evaluate+merge={:.3}ms build={:.3}ms publish={:.3}ms",
        evaluate_times[10].as_secs_f64() * 1000.0,
        build_times[10].as_secs_f64() * 1000.0,
        write_times[10].as_secs_f64() * 1000.0
    );
    eprintln!("Owner + 31 sheets + docs + registry + atomic write: median={:.3}ms p95={:.3}ms max={:.3}ms (20 builds)",
        timings[10].as_secs_f64() * 1000.0, timings[18].as_secs_f64() * 1000.0, timings[19].as_secs_f64() * 1000.0);
    assert!(timings[18] < Duration::from_millis(150));
}
