#![cfg(target_os = "linux")]
//! Apps and the new Tools after a self-update are started without the old
//! Tools image's mount paths; the update helper keeps them (it needs the
//! image's libraries).
use arcade_tools_core::paths::Paths;
use arcade_tools_core::platform::{spawn_detached, spawn_helper};
use std::time::{Duration, Instant};

fn environment_of(
    spawn: fn(&Paths, &std::path::Path, &[String]) -> arcade_tools_core::Result<()>,
) -> String {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let out = root.path().join("env.txt");
    let script = root.path().join("dump.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nenv > '{}.part' && mv '{0}.part' '{0}'\n",
            out.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    spawn(&paths, &script, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !out.exists() {
        assert!(Instant::now() < deadline, "the child never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::read_to_string(out).unwrap()
}

#[test]
fn children_leave_the_tools_image_behind_but_the_helper_keeps_it() {
    // SAFETY: this test binary runs only this test.
    unsafe {
        std::env::set_var("APPDIR", "/tmp/.mount_ArcadeT00ls");
        std::env::set_var(
            "APPIMAGE",
            "/home/u/Applications/Arcade/Arcade-Tools.AppImage",
        );
        std::env::set_var(
            "LD_LIBRARY_PATH",
            "/tmp/.mount_ArcadeT00ls/usr/lib:/opt/keep/lib",
        );
    }
    let app = environment_of(spawn_detached);
    assert!(!app.contains(".mount_ArcadeT00ls"), "{app}");
    assert!(app.contains("LD_LIBRARY_PATH=/opt/keep/lib"), "{app}");
    let helper = environment_of(spawn_helper);
    assert!(
        helper.contains("LD_LIBRARY_PATH=/tmp/.mount_ArcadeT00ls/usr/lib:/opt/keep/lib"),
        "{helper}"
    );
    assert!(
        helper.contains("APPDIR=/tmp/.mount_ArcadeT00ls"),
        "{helper}"
    );
}
