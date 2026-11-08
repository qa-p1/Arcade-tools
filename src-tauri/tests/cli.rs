//! The command line in ARCADE_APP.md (CLI-1..4), run against the built binary in an empty profile with no display.
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn run(home: &Path, args: &[&str]) -> Option<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_arcade-tools"))
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("ARCADE_HOME", home.join(".local/share/arcade"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if child.try_wait().unwrap().is_some() {
            return Some(child.wait_with_output().unwrap());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn profile() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("run")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            home.path().join("run"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    home
}

fn files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path.strip_prefix(root).unwrap().display().to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn version_and_help() {
    let home = profile();
    let version = run(home.path(), &["--version"]).expect("--version hung");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("Arcade Tools {}", env!("CARGO_PKG_VERSION"))
    );
    let help = run(home.path(), &["--help"]).expect("--help hung");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage: arcade-tools"));
}

#[test]
fn arcade_manifest_prints_json() {
    let home = profile();
    let out = run(home.path(), &["--arcade-manifest"]).expect("--arcade-manifest hung");
    assert!(out.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(manifest["id"], "arcade.tools");
}

#[test]
fn arcade_manifest_creates_nothing() {
    let home = profile();
    let before = files(home.path());
    run(home.path(), &["--arcade-manifest"]).expect("--arcade-manifest hung");
    let after = files(home.path());
    assert_eq!(before, after, "--arcade-manifest created files");
}

#[test]
fn status_prints_one_json_line_without_starting_tools() {
    let home = profile();
    let out = run(home.path(), &["--status"]);
    let out = out
        .unwrap_or_else(|| panic!("TL-4: --status started Tools instead of printing its status"));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        matches!(out.status.code(), Some(0 | 3)) && text.lines().count() == 1,
        "TL-4: --status exited {:?} and printed {text:?}",
        out.status.code()
    );
    let status: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(status["app"], "arcade.tools");
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["running"], false);
}

#[test]
fn an_unknown_argument_prints_usage_and_exits_2() {
    let home = profile();
    let out = run(home.path(), &["--no-such-option"]);
    let out = out.unwrap_or_else(|| panic!("CLI-2: an unknown argument started Tools"));
    assert!(
        out.status.code() == Some(2)
            && String::from_utf8_lossy(&out.stderr).contains("Usage: arcade-tools"),
        "CLI-2: exited {:?}",
        out.status.code()
    );
}

/// TL-1: the command line waits for the overlay, so an AppImage mount it was started from outlives it.
#[cfg(unix)]
#[test]
fn show_shortcuts_keeps_running_until_the_overlay_closes() {
    use std::os::unix::fs::PermissionsExt;
    let home = profile();
    let overlay = home.path().join("fake-overlay");
    let closed = home.path().join("overlay-closed");
    std::fs::write(
        &overlay,
        format!(
            "#!/bin/sh\n[ \"$1\" = --toggle ] || exit 3\nsleep 1\ntouch '{}'\n",
            closed.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&overlay, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_arcade-tools"));
    command
        .arg("--show-shortcuts")
        .env("HOME", home.path())
        .env("ARCADE_TOOLS_OVERLAY_FOR_TESTS", &overlay)
        .stdin(Stdio::null());
    let status = command.status().unwrap();
    assert!(status.success(), "{status}");
    assert!(
        closed.exists(),
        "--show-shortcuts returned while the overlay was still open"
    );
}

/// A Tools that is still starting (just restarted, or started at login) answers `hello` late. `--status` and
/// `--quit` must wait for it as long as Link's server waits for a hello (2 s), not fail after Link's 150 ms liveness
/// probe: `--quit` right after `--restart` exited 1 with "didn't respond in time" (rc.8 Release, D17).
#[cfg(target_os = "linux")]
#[test]
fn status_and_quit_wait_for_a_tools_that_is_still_starting() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let home = profile();
    let locations = arcade_link::Locations::under(&home.path().join(".local/share/arcade"));
    let address = arcade_link::transport::address_for(&locations, "arcade.tools").unwrap();
    let token = arcade_link::endpoint::new_token().unwrap();
    arcade_link::endpoint::write(
        &locations,
        "arcade.tools",
        &arcade_link::endpoint::EndpointInfo {
            protocol: vec![1],
            transport: address.transport.into(),
            address: address.address.clone(),
            pid: std::process::id(),
            started_at: String::new(),
            token: token.clone(),
        },
    )
    .unwrap();
    let listener = UnixListener::bind(&address.address).unwrap();
    let quit = Arc::new(AtomicBool::new(false));
    let quitting = quit.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let (token, quitting) = (token.clone(), quitting.clone());
            std::thread::spawn(move || {
                let mut writer = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { return };
                    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                    let result = match request["method"].as_str().unwrap_or_default() {
                        "hello" => {
                            assert_eq!(request["params"]["token"], token.as_str());
                            // Busy starting: well past 150 ms, well within 2 s.
                            std::thread::sleep(Duration::from_millis(600));
                            serde_json::json!({"server": {"id": "arcade.tools"}, "protocol": 1})
                        }
                        "app.status" => {
                            serde_json::json!({"app": "arcade.tools", "status": {"busy": false}})
                        }
                        "app.quit" => {
                            quitting.store(true, Ordering::SeqCst);
                            serde_json::json!({})
                        }
                        other => panic!("unexpected request {other}"),
                    };
                    let reply = serde_json::json!({"v": 1, "id": request["id"], "result": result});
                    if writeln!(writer, "{reply}").is_err() {
                        return;
                    }
                }
            });
        }
    });

    let status = run(home.path(), &["--status"]).expect("--status hung");
    let line: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        (status.status.code(), &line["running"]),
        (Some(0), &serde_json::json!(true)),
        "--status took a starting Tools for not running: {line}"
    );
    let out = run(home.path(), &["--quit"]).expect("--quit hung");
    assert!(
        out.status.success(),
        "--quit exited {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        quit.load(Ordering::SeqCst),
        "--quit never asked Tools to quit"
    );
}
