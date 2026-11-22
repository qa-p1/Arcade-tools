#![cfg(target_os = "linux")]
//! "Download with Arcade" for Tesseract runs curl and the downloaded AppImage with the user's environment, not with
//! Tools' AppImage libraries: Tools' bundled libnghttp2 broke the system curl ("symbol lookup error", D37). Link 1.0.15's download does this;
//! Tools calls it directly again.

#[test]
fn the_tesseract_download_runs_curl_without_the_tools_image_libraries() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let dump = root.path().join("curl-env.txt");
    let curl = bin.join("curl");
    std::fs::write(
        &curl,
        format!(
            "#!/bin/sh\nenv > '{}'\necho 'fake curl stops here' >&2\nexit 22\n",
            dump.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // SAFETY: this test binary runs only this test.
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("ARCADE_HOME", root.path().join("home"));
        std::env::set_var("APPDIR", "/tmp/.mount_ArcadeT00ls");
        std::env::set_var(
            "LD_LIBRARY_PATH",
            "/tmp/.mount_ArcadeT00ls/usr/lib:/opt/keep/lib",
        );
    }
    let Err(error) = arcade_link::engines::download_tesseract() else {
        panic!("the fake curl fails the download");
    };
    assert!(error.contains("fake curl stops here"), "{error}");
    let env = std::fs::read_to_string(&dump).unwrap();
    assert!(!env.contains(".mount_ArcadeT00ls"), "{env}");
    assert!(env.contains("LD_LIBRARY_PATH=/opt/keep/lib"), "{env}");
    // The partial download folder is removed.
    assert!(!arcade_link::engines::engines_dir()
        .join(".tesseract-download")
        .exists());
}
