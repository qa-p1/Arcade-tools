use arcade_shortcuts::{
    build_index, gnome, kde, labels::UserLabels, sway, wm::WmReport, BindState, BuildInputs, Os,
    ReadPolicy,
};
use std::path::{Path, PathBuf};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn text(relative: &str) -> String {
    std::fs::read_to_string(fixture(relative)).unwrap()
}

fn overrides() -> UserLabels {
    UserLabels::from([
        ("|Ctrl+Alt+V".into(), "My audio panel".into()),
        ("resize|Ctrl+Alt+V".into(), "Modal audio panel".into()),
    ])
}

fn bind<'a>(
    report: &'a WmReport,
    chord: &str,
    submap: Option<&str>,
) -> &'a arcade_shortcuts::WmBind {
    report
        .binds
        .iter()
        .find(|bind| bind.chord.as_deref() == Some(chord) && bind.submap.as_deref() == submap)
        .unwrap()
}

fn sway_report() -> WmReport {
    let root = fixture("sway");
    let options = sway::Options {
        read_policy: ReadPolicy::new([root.clone()]),
        ..Default::default()
    };
    let config = sway::parse(&root.join("config"), &options);
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    let modes = sway::parse_modes(&text("sway/binding-modes.json")).unwrap();
    sway::report(config, Some(&modes), &overrides())
}

fn kde_report() -> WmReport {
    kde::parse(
        &text("kde/kglobalshortcutsrc"),
        &fixture("kde/kglobalshortcutsrc"),
        &overrides(),
    )
}

fn gnome_report() -> WmReport {
    let settings = text("gnome/gsettings.txt");
    let paths = gnome::custom_paths(&settings).unwrap();
    let custom = gnome::parse_custom(&paths[0], &text("gnome/custom.txt")).unwrap();
    assert_eq!(custom.name, "Owner's terminal");
    gnome::parse(&settings, &[custom], &overrides())
}

#[test]
fn sway_variables_includes_modes_flags_origins_and_labels() {
    let report = sway_report();
    assert_eq!(report.name, "sway");
    assert_eq!(report.binds.len(), 13);
    assert_eq!(report.files.len(), 3);
    assert!(report.warnings.is_empty());
    assert_eq!(bind(&report, "Super+Enter", None).label, "Terminal");
    assert_eq!(
        bind(&report, "Super+Enter", None).detail.as_deref(),
        Some("Run: foot")
    );
    assert_eq!(
        bind(&report, "Super+Enter", None)
            .origin
            .as_ref()
            .unwrap()
            .line,
        5
    );
    assert_eq!(
        bind(&report, "Super+Q", None).flags.get("release"),
        Some(&true)
    );
    assert_eq!(
        bind(&report, "VolumeUp", None).flags.get("locked"),
        Some(&true)
    );
    assert_eq!(bind(&report, "Ctrl+Alt+V", None).label, "My audio panel");
    assert_eq!(
        bind(&report, "Ctrl+Alt+V", Some("resize")).label,
        "Modal audio panel"
    );
    assert_eq!(
        bind(&report, "Super+Right", Some("resize")).label,
        "Grow window"
    );
    assert!(bind(&report, "Super+Code38", None).origin.is_some());
    assert!(bind(&report, "Super+D", None)
        .label
        .contains("literal # value"));
    assert!(report
        .binds
        .iter()
        .all(|bind| bind.state == BindState::ConfiguredOnly));
}

#[test]
fn sway_cycles_block_syntax_and_limits_preserve_valid_binds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    std::fs::write(
        &path,
        "set $mod Mod4\nbindsym {\n$mod+t exec foot\n$mod+q kill\n}\ninclude config\n",
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut options = sway::Options {
        read_policy: ReadPolicy::new([dir.path().to_owned()]),
        ..Default::default()
    };
    let parsed = sway::parse(&path, &options);
    assert_eq!(parsed.binds.len(), 2);
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("cycle")));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    options.max_bytes = 8;
    assert!(sway::parse(&path, &options).binds.is_empty());
    let config = sway::parse_text(
        "mode \"resize\" {\nbindsym Left resize shrink width 1px\n}\n",
        &path,
        &sway::Options::default(),
    );
    let report = sway::report(config, Some(&["default".into()]), &UserLabels::new());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("absent")));
    assert!(sway::parse_modes("{}").is_err());
}

#[cfg(unix)]
#[test]
fn sway_rejects_include_symlink_escape() {
    let inside = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret");
    std::fs::write(&target, "bindsym Mod4+x exec secret\n").unwrap();
    std::os::unix::fs::symlink(&target, inside.path().join("escape")).unwrap();
    let config = inside.path().join("config");
    std::fs::write(&config, "include escape\nbindsym Mod4+t exec foot\n").unwrap();
    let report = sway::parse(
        &config,
        &sway::Options {
            read_policy: ReadPolicy::new([inside.path().to_owned()]),
            ..Default::default()
        },
    );
    assert_eq!(report.binds.len(), 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("escapes")));
}

#[test]
fn kde_current_keys_alternatives_sequences_escapes_and_disabled_defaults() {
    let report = kde_report();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.binds.len(), 10);
    assert_eq!(bind(&report, "Ctrl+Alt+V", None).label, "My audio panel");
    assert_eq!(
        bind(&report, "Ctrl+Alt+V", Some("resize")).label,
        "Modal audio panel"
    );
    assert_eq!(bind(&report, "Super+PageUp", None).label, "Maximize window");
    assert_eq!(
        bind(&report, "Alt+F10", None).category.as_deref(),
        Some("KWin")
    );
    assert_eq!(
        bind(&report, "Super+K Super+C", None).label,
        "Sequence action"
    );
    assert_eq!(bind(&report, "Ctrl+Comma", None).label, "Comma key");
    assert_eq!(bind(&report, "Super+Slash", None).label, "Shortcut sheet");
    assert_eq!(
        bind(&report, "VolumeUp", None).category.as_deref(),
        Some("Audio Volume")
    );
    assert!(report
        .binds
        .iter()
        .all(|bind| bind.chord.as_deref() != Some("Super+D")
            && bind.chord.as_deref() != Some("Super+E")
            && bind.state == BindState::ConfiguredOnly));
    let bad = kde::parse(
        "[kwin]\nMalformed=Ctrl+Q\nUnknown=Hyper+Q,none,Unknown\nValid=Meta+T,none,Terminal\n",
        Path::new("fixture"),
        &UserLabels::new(),
    );
    assert_eq!(bad.binds.len(), 1);
    assert_eq!(bad.warnings.len(), 2);
}

#[test]
fn gnome_gvariants_effective_keys_custom_names_disabled_and_fallback() {
    let report = gnome_report();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.binds.len(), 8);
    assert!(report
        .binds
        .iter()
        .all(|bind| bind.state == BindState::ConfiguredOnly));
    assert_eq!(bind(&report, "Ctrl+Alt+V", None).label, "My audio panel");
    assert_eq!(
        bind(&report, "Ctrl+Super+T", None).label,
        "Owner's terminal"
    );
    assert!(bind(&report, "Ctrl+Super+T", None)
        .detail
        .as_deref()
        .unwrap()
        .contains("foot"));
    assert_eq!(
        gnome::gtk_chord("<Shift><Alt>ISO_Left_Tab").as_deref(),
        Some("Alt+Shift+Tab")
    );
    assert_eq!(
        gnome::gtk_chord("<Super>slash").as_deref(),
        Some("Super+Slash")
    );
    assert_eq!(
        gnome::gtk_chord("<Primary>plus").as_deref(),
        Some("Ctrl+Shift+Equal")
    );
    assert!(gnome::gtk_chord("<Hyper>t").is_none());
    assert_eq!(
        gnome::string_values("@as []").unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        gnome::string_values("'Owner\\'s café'").unwrap(),
        ["Owner's café"]
    );
    for invalid in [
        "[",
        "['unclosed]",
        "['one' 'two']",
        "'x' suffix",
        "['\\uZZZZ']",
    ] {
        assert!(gnome::string_values(invalid).is_err(), "{invalid}");
    }
    let bad = gnome::parse("org.gnome.desktop.wm.keybindings close ['<Alt>F4']\norg.gnome.desktop.wm.keybindings broken ['unclosed]\n",
        &[], &UserLabels::new());
    assert_eq!(bad.binds.len(), 1);
    assert_eq!(bad.warnings.len(), 1);
    let fallback = gnome::parse_dconf(&text("gnome/dconf.ini"), &overrides());
    assert_eq!(fallback.binds.len(), 4);
    assert!(fallback
        .binds
        .iter()
        .all(|bind| bind.state == BindState::ConfiguredOnly));
    assert!(!fallback
        .binds
        .iter()
        .any(|bind| bind.label == "Removed shortcut"));
    assert!(fallback
        .warnings
        .iter()
        .any(|warning| warning.contains("saved overrides only")));
    assert!(gnome::custom_paths(
        "org.gnome.settings-daemon.plugins.media-keys custom-keybindings ['/outside/']\n"
    )
    .is_err());
}

#[test]
fn all_desktops_feed_shared_wm_groups_label_overrides_and_arcade_conflicts() {
    for report in [sway_report(), kde_report(), gnome_report()] {
        let name = report.name.clone();
        let index = build_index(&BuildInputs {
            os: Os::Linux,
            wm: Some(report),
            registry_dir: Some(fixture("registry")),
            ..Default::default()
        });
        assert_eq!(index.wm.name, name);
        assert!(!index.wm.groups.is_empty());
        assert!(index
            .conflicts
            .iter()
            .any(|conflict| conflict.chord == "Ctrl+Alt+V"
                && conflict.a.owner == "arcade.clipboard"
                && conflict.b.kind == "wm"
                && conflict.b.owner == name
                && conflict.b.label == "My audio panel"));
        let json = serde_json::to_value(&index).unwrap();
        assert_eq!(json["wm"]["name"], name);
        let decoded: arcade_shortcuts::Index = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, index);
    }
}
