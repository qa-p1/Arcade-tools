//! Removing Tools with the helper running leaves no menu entry behind (Gates, `scripts/verify-helper.py`, found them
//! all back: the running helper rewrote its menus when Tools removed the catalog and its card under it).
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use arcade_tools_core::everywhere;
use arcade_tools_core::paths::Paths;

fn wait(what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Every file under the data folder except Arcade's own (`<data>/arcade`, where the copy lives): the menu entries.
fn entries(data: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![data.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path == data.join("arcade") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// From the catalog change to the removal. Here the old order (catalog, `integrate --remove`, then stop) lost the
/// race every time: the scheduled rewrite ran after the entries were removed and before the helper exited.
const DELAY_MS: u64 = 250;

#[test]
fn removing_tools_with_the_helper_running_leaves_no_menu_entries() {
    let root = std::env::temp_dir().join(format!("tools-helper-remove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // This test is its own process: every root the helper and Tools look at is under `home`.
    std::env::set_var("ARCADE_HOME", &home);
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    let paths = Paths::under(&home);

    let tools_dir = root.join("Arcade Tools");
    std::fs::create_dir_all(&tools_dir).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_arcade-helper"),
        tools_dir.join("arcade-helper"),
    )
    .unwrap();
    let tools = tools_dir.join("arcade-tools");
    std::fs::write(&tools, b"").unwrap();
    std::fs::create_dir_all(paths.link.catalog().parent().unwrap()).unwrap();
    std::fs::write(paths.link.catalog(), "{}").unwrap();
    // Another app's card stays registered while Tools is removed (Tools' own goes last), so the helper has
    // something to write back.
    std::fs::create_dir_all(&paths.link.registry).unwrap();
    let card = serde_json::json!({"schema": 1, "id": "arcade.box", "name": "Arcade Box", "version": "1.0.0",
        "executable": std::env::current_exe().unwrap(),
        "actions": [{"id": "box:image.webp", "title": "Convert to WebP", "accepts": ["file/image"], "oneClick": true}]});
    let manifest = arcade_link::Manifest::from_json(&card.to_string()).unwrap();
    arcade_link::manifest::write_manifest(&paths.link, &manifest).unwrap();

    let (copy, _) = everywhere::install(&paths, &tools_dir.join("arcade-helper")).unwrap();
    let mut helper = Command::new(&copy).arg("--background").spawn().unwrap();
    let data = home.join(".local/share");
    wait("the helper to write its menus", || {
        !entries(&data).is_empty()
    });

    // As in Gates: Tools quit just before its removal and wrote the catalog, so the helper had a rewrite of its
    // menus scheduled (it coalesces changes for 300 ms) when the removal began.
    std::thread::sleep(Duration::from_millis(500));
    std::fs::write(paths.link.catalog(), "{ }").unwrap();
    std::thread::sleep(Duration::from_millis(DELAY_MS));
    everywhere::remove(&paths, &tools).unwrap();
    wait("the helper to stop", || {
        helper.try_wait().unwrap().is_some()
    });
    // Longer than the helper's 300 ms coalescing of catalog and card changes.
    std::thread::sleep(Duration::from_millis(1500));
    let left = entries(&data);
    let _ = helper.kill();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        left.is_empty(),
        "menu entries left after removing Tools: {left:?}"
    );
    assert!(!copy.exists(), "{copy:?} left after removing Tools");
}
