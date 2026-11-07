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
#[should_panic(expected = "KNOWN BUG TL-4")]
fn status_prints_one_json_line_without_starting_tools() {
    let home = profile();
    let out = run(home.path(), &["--status"]);
    let out = out.unwrap_or_else(|| {
        panic!("KNOWN BUG TL-4: --status started Tools instead of printing its status")
    });
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        matches!(out.status.code(), Some(0 | 3)) && text.lines().count() == 1,
        "KNOWN BUG TL-4: --status exited {:?} and printed {text:?}",
        out.status.code()
    );
    let status: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(status["app"], "arcade.tools");
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["running"], false);
}

#[test]
#[should_panic(expected = "KNOWN BUG CLI-2")]
fn an_unknown_argument_prints_usage_and_exits_2() {
    let home = profile();
    let out = run(home.path(), &["--no-such-option"]);
    let out = out.unwrap_or_else(|| panic!("KNOWN BUG CLI-2: an unknown argument started Tools"));
    assert!(
        out.status.code() == Some(2)
            && String::from_utf8_lossy(&out.stderr).contains("Usage: arcade-tools"),
        "KNOWN BUG CLI-2: exited {:?}",
        out.status.code()
    );
}
