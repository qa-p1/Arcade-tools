use arcade_tools_core::{catalog::ENGINE_SNAPSHOT, engines::*, release::Os};
use serde_json::json;
use std::time::{Duration, Instant};
fn engine() -> Engine {
    let mut engine = EngineCatalog::parse(ENGINE_SNAPSHOT.as_bytes())
        .unwrap()
        .engines
        .remove(0);
    engine.detect.binaries = vec!["fake-engine".into(), "alternative".into()];
    engine.detect.min_version = Some("5.0.0".into());
    engine
}
#[test]
fn exact_format_schema_minimum_and_distro_hints() {
    assert_eq!(
        EngineCatalog::parse(ENGINE_SNAPSHOT.as_bytes())
            .unwrap()
            .engines
            .len(),
        11
    );
    let mut catalog: serde_json::Value = serde_json::from_str(ENGINE_SNAPSHOT).unwrap();
    catalog["future"] = json!(true);
    assert!(EngineCatalog::parse(&serde_json::to_vec(&catalog).unwrap()).is_ok());
    catalog["schema"] = json!(2);
    assert!(EngineCatalog::parse(&serde_json::to_vec(&catalog).unwrap()).is_err());
    let engine = engine();
    assert_eq!(
        engine.install_hint(Os::Linux, "ID=cachyos\nID_LIKE=arch\n"),
        Some("sudo pacman -S tesseract")
    );
    assert_eq!(
        engine.install_hint(Os::Linux, "ID=ubuntu\nID_LIKE=debian\n"),
        Some("sudo apt install tesseract-ocr")
    );
    assert_eq!(
        linux_distro("ID=custom\nID_LIKE=\"fedora rhel\""),
        Some("fedora")
    );
    assert_eq!(linux_distro("ID=debian\nID_LIKE=arch"), Some("debian"));
    assert_eq!(engine.install_hint(Os::Linux, "ID=unknown"), None);
    assert_eq!(
        engine.install_hint(Os::Macos, ""),
        Some("brew install tesseract")
    );
    assert_eq!(engine.install_hint(Os::Windows, ""), None);
    for (output, expected) in [
        ("tesseract 5.3.4", "5.3.4"),
        ("ffmpeg version n8.0", "8.0.0"),
        ("pdftoppm version 25.03.0", "25.3.0"),
        ("vips-8.16.1", "8.16.1"),
    ] {
        assert_eq!(parse_output_version(output).unwrap().to_string(), expected);
    }
    assert_eq!(parse_output_version("no version"), None);
}
#[cfg(unix)]
#[test]
fn fake_binaries_found_old_missing_alternative_and_timeout() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().as_os_str();
    let engine = engine();
    assert_eq!(
        engine.detect(path, VERSION_TIMEOUT).unwrap(),
        EngineStatus::Missing
    );
    let write = |name, body| {
        let file = root.path().join(name);
        std::fs::write(&file, body).unwrap();
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    write("fake-engine", "#!/bin/sh\necho 'fake 4.9.0' >&2\n");
    assert!(
        matches!(engine.detect(path, VERSION_TIMEOUT).unwrap(), EngineStatus::TooOld {version,..} if version == "4.9.0")
    );
    write("alternative", "#!/bin/sh\necho 'fake 5.1.0'\n");
    assert!(
        matches!(engine.detect(path, VERSION_TIMEOUT).unwrap(), EngineStatus::Found {path,version} if path.ends_with("alternative") && version.as_deref() == Some("5.1.0"))
    );
    std::fs::remove_file(root.path().join("alternative")).unwrap();
    // exec replaces the shell, so the short timeout also leaves no descendant.
    write("fake-engine", "#!/bin/sh\nexec /bin/sleep 10\n");
    let start = Instant::now();
    assert_eq!(
        engine
            .detect(path, Duration::from_millis(80))
            .unwrap_err()
            .code,
        "timeout"
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}
