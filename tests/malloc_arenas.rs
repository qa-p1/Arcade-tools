#![cfg(all(target_os = "linux", target_env = "gnu"))]
//! Tools caps glibc's malloc arenas for itself and the WebKit processes it starts (DECISIONS.md D18; Link's cap
//! since D21): they inherit `MALLOC_ARENA_MAX`, apps and installers Tools starts do not. Link's own U1 test covers
//! the unpacked copy. A test binary of its own because the setting is process-wide.
use arcade_link::runtime::cap_malloc_arenas;
use arcade_tools_core::paths::Paths;
use arcade_tools_core::platform::spawn_detached;
use std::time::{Duration, Instant};

#[test]
fn webkit_inherits_the_arena_cap_and_apps_tools_starts_do_not() {
    if std::env::var_os("MALLOC_ARENA_MAX").is_some() {
        eprintln!("MALLOC_ARENA_MAX is set by the caller; Tools keeps the user's value");
        return;
    }
    cap_malloc_arenas();
    // WebKit's processes are started by WebKit from Tools' own environment.
    assert_eq!(std::env::var("MALLOC_ARENA_MAX").as_deref(), Ok("2"));
    let inherited = std::process::Command::new("sh")
        .args(["-c", "printf %s \"$MALLOC_ARENA_MAX\""])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&inherited.stdout), "2");

    // An app or installer started by Tools gets the user's environment.
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let out = root.path().join("app-env");
    let script = root.path().join("app");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s' \"${{MALLOC_ARENA_MAX-unset}}\" > '{}'\n",
            out.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    spawn_detached(&paths, &script, &[]).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !out.exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "unset");
}
