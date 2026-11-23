//! The Arcade helper (Link's `arcade-helper`, Arcade-catalog `dev/everywhere/PLAN.md` §4.4): Arcade's actions in
//! the file manager menus and the browser. Tools ships it next to its own program, keeps an installed copy where
//! menus can call it, starts it, replaces it only between jobs, and removes it with every menu entry it wrote.

use crate::paths::Paths;
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const HELPER_ID: &str = "arcade.helper";

fn file_name() -> &'static str {
    if cfg!(windows) {
        "arcade-helper.exe"
    } else {
        "arcade-helper"
    }
}

/// The helper Tools was shipped with: next to Tools' own program (in an AppImage, inside its mount).
pub fn bundled() -> Option<PathBuf> {
    // Debug builds only: tests stand in their own build.
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("ARCADE_TOOLS_HELPER_FOR_TESTS") {
        return Some(PathBuf::from(path)).filter(|path| path.is_file());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(file_name())))
        .filter(|path| path.is_file())
}

/// Where the menus and the browser call the helper. An AppImage's mount goes away when Tools quits, so on Linux the
/// helper is copied to Tools' own data folder; elsewhere it stays in Tools' install folder or app bundle.
pub fn installed(paths: &Paths, bundled: &Path) -> PathBuf {
    if cfg!(target_os = "linux") {
        paths.state().join("helper").join(file_name())
    } else {
        bundled.to_path_buf()
    }
}

/// Runs `f` again for up to a second while it fails with "text file busy": just after the copy is written, a fork
/// on another thread may still hold the written file open until that child starts its own program (Linux).
fn retry_busy<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    const ETXTBSY: i32 = 26;
    let mut tries = 0;
    loop {
        match f() {
            Err(e)
                if cfg!(target_os = "linux") && e.raw_os_error() == Some(ETXTBSY) && tries < 50 =>
            {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

fn sha256(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path)
        .ok()
        .map(|bytes| Sha256::digest(&bytes).to_vec())
}

fn me() -> arcade_link::PeerInfo {
    arcade_link::PeerInfo {
        id: "arcade.tools.helper-manager".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// The running helper's status, if it runs.
pub fn status(paths: &Paths) -> Option<Value> {
    let mut client = arcade_link::Client::connect(&paths.link, HELPER_ID, &me()).ok()?;
    client.status().ok()
}

fn busy(status: &Value) -> bool {
    status.get("busy").and_then(Value::as_bool) == Some(true)
        || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true)
}

/// Stops a running helper unless it has unfinished jobs. `Ok(false)`: it is busy and keeps running.
pub fn stop(paths: &Paths) -> Result<bool> {
    if status(paths).is_some_and(|s| busy(&s)) {
        return Ok(false);
    }
    arcade_link::install::quit_running(&paths.link, HELPER_ID, Duration::from_secs(10))
        .map_err(|e| Error::new("helper", format!("The Arcade helper did not stop: {e}")))?;
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Installed {
    /// The installed copy already matches the shipped one.
    Current,
    /// The shipped helper was copied (and an older running one stopped).
    Updated,
    /// The running helper has unfinished jobs; it is replaced the next time Tools starts.
    Busy,
}

/// Makes the installed copy match the shipped one (Linux; elsewhere the shipped one is the installed one).
pub fn install(paths: &Paths, bundled: &Path) -> Result<(PathBuf, Installed)> {
    let target = installed(paths, bundled);
    if target == bundled {
        return Ok((target, Installed::Current));
    }
    let shipped = sha256(bundled)
        .ok_or_else(|| Error::new("helper", "The shipped Arcade helper can't be read."))?;
    if sha256(&target).as_deref() == Some(shipped.as_slice()) {
        return Ok((target, Installed::Current));
    }
    if !stop(paths)? {
        return Ok((target, Installed::Busy));
    }
    let dir = target
        .parent()
        .ok_or_else(|| Error::new("helper", "No folder for the helper."))?;
    paths.guard(dir)?;
    std::fs::create_dir_all(dir)?;
    // Copy beside it and rename over it, so a menu never starts a half-written program.
    let mut staged = tempfile::Builder::new()
        .prefix(".arcade-helper-")
        .tempfile_in(dir)?;
    std::io::copy(&mut std::fs::File::open(bundled)?, staged.as_file_mut())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(staged.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    staged
        .persist(&target)
        .map_err(|e| Error::new("helper", e.to_string()))?;
    Ok((target, Installed::Updated))
}

/// Starts the helper in the background (it exits at once when one is already running).
pub fn start(path: &Path) -> Result<()> {
    retry_busy(|| {
        arcade_link::client::spawn_detached(&path.to_string_lossy(), &["--background".into()])
    })?;
    Ok(())
}

/// The version a helper program reports (`arcade-helper --version` prints `arcade-helper 1.1.0`).
pub fn version_of(program: &Path) -> Option<String> {
    let out = retry_busy(|| {
        std::process::Command::new(program)
            .arg("--version")
            .output()
    })
    .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    text.trim()
        .strip_prefix("arcade-helper ")
        .map(str::to_string)
}

/// The version of the helper that is running, if one is.
pub fn running_version(paths: &Paths) -> Option<String> {
    arcade_link::client::probe(&paths.link, HELPER_ID, &me()).map(|peer| peer.version)
}

/// Installs and starts the helper: what Tools does each time it starts. A helper still running from before an
/// update is stopped first (unless it is busy), or the new one would see it and step aside.
pub fn ensure_running(paths: &Paths) -> Result<Option<(PathBuf, Installed)>> {
    let Some(bundled) = bundled() else {
        // A development build of Tools without the helper beside it.
        return Ok(None);
    };
    let (path, mut installed) = install(paths, &bundled)?;
    if installed != Installed::Busy {
        if let (Some(running), Some(shipped)) = (running_version(paths), version_of(&path)) {
            if running != shipped && !stop(paths)? {
                installed = Installed::Busy;
            }
        }
    }
    start(&path)?;
    Ok(Some((path, installed)))
}

/// Before Tools is removed: every menu entry and browser file the helper wrote, its card, the running helper and
/// (Linux) its installed copy. `tools` is the installed Tools program, beside which the helper was shipped.
pub fn remove(paths: &Paths, tools: &Path) -> Result<()> {
    // Nothing else keeps the shared copy of the catalog up to date once Tools is gone; apps fall back to the list
    // they were built with.
    let _ = std::fs::remove_file(paths.link.catalog());
    let shipped = tools.parent().map(|dir| dir.join(file_name()));
    let copy = shipped.as_deref().map(|s| installed(paths, s)).or_else(|| {
        cfg!(target_os = "linux").then(|| paths.state().join("helper").join(file_name()))
    });
    let Some(program) = copy
        .iter()
        .chain(shipped.iter())
        .find(|p| p.is_file())
        .cloned()
    else {
        return Ok(());
    };
    let out = retry_busy(|| {
        std::process::Command::new(&program)
            .args(["integrate", "--remove"])
            .output()
    })
    .map_err(|e| Error::new("helper", format!("The Arcade helper didn't run: {e}")))?;
    if !out.status.success() {
        return Err(Error::new(
            "helper",
            format!(
                "The Arcade helper couldn't remove its menu entries: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    // Its card is gone and Tools is going: stop it even mid-job.
    let _ = arcade_link::install::quit_running(&paths.link, HELPER_ID, Duration::from_secs(10));
    // Only the Linux copy lives outside Tools' own install, which Tools' removal deletes.
    if cfg!(target_os = "linux") {
        if let Some(dir) = copy
            .as_deref()
            .and_then(Path::parent)
            .filter(|d| d.starts_with(paths.state()))
        {
            paths.guard(dir)?;
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    Ok(())
}

/// Keeps Link's shared copy of the app list (`<registry>/../catalog.json`) in step with the catalog Tools read, so
/// every app, the helper and the extension name apps and pairs the same way.
pub fn share_catalog(paths: &Paths, catalog_text: &str) -> Result<bool> {
    arcade_link::catalog::write_shared(&paths.link, catalog_text)
        .map_err(|e| Error::new("catalog", e.to_string()))
}

// ---- The Everywhere page --------------------------------------------------------------------------------------

/// The most actions with their own menu entry (besides "Arcade…"); the helper writes no more.
pub const MAX_PINS: usize = 8;

/// Kinds of files whose one-click actions can be pinned, most common first (the helper's default pins use these).
const PIN_KINDS: [&str; 9] = [
    "file/image", "file/video", "file/audio", "file/pdf", "file/document", "file/spreadsheet", "file/archive", "file/text", "file/any",
];

#[derive(Debug, Clone, PartialEq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HelperState {
    /// The shipped helper's version, or `None` in a development build without it.
    pub shipped: Option<String>,
    /// The running helper's version.
    pub running: Option<String>,
    pub file_managers: Vec<String>,
    pub browsers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRow {
    pub reference: String,
    pub title: String,
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRow {
    #[serde(flatten)]
    pub action: ActionRow,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRow {
    pub job: String,
    pub choices: Vec<ActionRow>,
    pub chosen: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub helper: HelperState,
    /// One-click file actions, pinned ones first in pin order. With no pins, the helper picks its own defaults.
    pub pins: Vec<PinRow>,
    pub max_pins: usize,
    pub output_folder: String,
    pub ask_where: bool,
    /// Jobs two or more installed apps do, and the user's default for each.
    pub jobs: Vec<JobRow>,
}

/// What the installed helper reports about itself (`arcade-helper status`).
pub fn helper_state(paths: &Paths) -> HelperState {
    let Some(shipped) = bundled() else { return HelperState::default() };
    let program = installed(paths, &shipped);
    let program = if program.is_file() { program } else { shipped };
    let mut state = retry_busy(|| std::process::Command::new(&program).arg("status").output())
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
        .map(|v| HelperState {
            shipped: v.get("version").and_then(Value::as_str).map(str::to_string),
            running: v.get("running").and_then(Value::as_str).map(str::to_string),
            file_managers: serde_json::from_value(v.get("fileManagers").cloned().unwrap_or_default()).unwrap_or_default(),
            browsers: serde_json::from_value(v.get("browsers").cloned().unwrap_or_default()).unwrap_or_default(),
        })
        .unwrap_or_default();
    if state.shipped.is_none() {
        state.shipped = version_of(&program);
    }
    state
}

/// The page from the installed cards, the catalog's rules and the user's choices.
pub fn page(paths: &Paths) -> Page {
    use arcade_link::offers::{self, Item, Prefs};
    let registry = arcade_link::registry::Registry::load(&paths.link);
    let catalog = arcade_link::catalog::Catalog::load(&paths.link);
    let prefs = Prefs::load(&paths.link);
    let row = |o: &offers::Offer| ActionRow { reference: o.reference(), title: o.title.clone(), app: catalog.name(&o.app) };
    let mut candidates: Vec<ActionRow> = Vec::new();
    let mut jobs: Vec<JobRow> = Vec::new();
    for kind in PIN_KINDS {
        let list = offers::offers(&[Item::new(kind)], arcade_link::manifest::surface::FILE_MENU, registry.apps(), &catalog.rules, &prefs);
        for o in list.iter().filter(|o| o.app != HELPER_ID) {
            if o.enabled && o.one_click && !candidates.iter().any(|c| c.reference == o.reference()) {
                candidates.push(row(o));
            }
            if let Some(job) = o.job.as_ref().filter(|_| o.choose || o.preferred || o.other) {
                let entry = match jobs.iter_mut().find(|j| &j.job == job) {
                    Some(entry) => entry,
                    None => {
                        jobs.push(JobRow { job: job.clone(), choices: Vec::new(), chosen: prefs.defaults.get(job).cloned() });
                        jobs.last_mut().expect("just pushed")
                    }
                };
                if !entry.choices.iter().any(|c| c.reference == o.reference()) {
                    entry.choices.push(row(o));
                }
            }
        }
    }
    let mut pins: Vec<PinRow> = prefs
        .pinned
        .iter()
        .filter_map(|r| candidates.iter().find(|c| &c.reference == r))
        .map(|c| PinRow { action: c.clone(), pinned: true })
        .collect();
    pins.extend(candidates.into_iter().filter(|c| !prefs.pinned.contains(&c.reference)).map(|action| PinRow { action, pinned: false }));
    jobs.retain(|j| j.choices.len() > 1);
    Page {
        helper: helper_state(paths),
        pins,
        max_pins: MAX_PINS,
        output_folder: prefs.output_folder,
        ask_where: prefs.ask_where,
        jobs,
    }
}

/// A change from the Everywhere page. The running helper sees `offers.json` change and rewrites its menu entries.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Change {
    /// The pinned actions in menu order (empty: the helper's defaults).
    Pins { references: Vec<String> },
    /// Where results go: a folder (empty for each action's own default), or ask every time.
    Output { folder: String, ask_where: bool },
    /// The default for a job, or `None` to show all its apps again.
    JobDefault { job: String, reference: Option<String> },
}

pub fn change(paths: &Paths, change: Change) -> Result<Page> {
    let mut prefs = arcade_link::offers::Prefs::load(&paths.link);
    match change {
        Change::Pins { references } => {
            if references.len() > MAX_PINS {
                return Err(Error::new("usage", format!("Pin at most {MAX_PINS} actions.")));
            }
            let mut unique = Vec::new();
            for r in references {
                if !unique.contains(&r) {
                    unique.push(r);
                }
            }
            prefs.pinned = unique;
        }
        Change::Output { folder, ask_where } => {
            let folder = folder.trim().to_string();
            if !folder.is_empty() && !Path::new(&folder).is_absolute() {
                return Err(Error::new("usage", "Choose a full folder path."));
            }
            prefs.output_folder = folder;
            prefs.ask_where = ask_where;
        }
        Change::JobDefault { job, reference } => match reference {
            Some(r) => {
                prefs.defaults.insert(job, r);
            }
            None => {
                prefs.defaults.remove(&job);
            }
        },
    }
    prefs.save(&paths.link).map_err(|e| Error::new("preferences", e.to_string()))?;
    Ok(page(paths))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("tools-everywhere-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A stand-in helper that records its arguments in `<its folder>/../args`.
    fn fake_helper(dir: &Path, version: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("arcade-helper");
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = --version ] && echo 'arcade-helper {version}' && exit 0\necho \"$@\" >> \"$(dirname \"$0\")/../args\"\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn the_shipped_helper_is_copied_out_of_the_appimage_once_and_again_when_it_changes() {
        let root = root("install");
        let paths = Paths::under(&root.join("home"));
        let shipped = fake_helper(&root.join("mount"), "1.1.0");
        let (path, first) = install(&paths, &shipped).unwrap();
        assert_eq!(
            (path.clone(), first),
            (
                paths.state().join("helper/arcade-helper"),
                Installed::Updated
            )
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(&shipped).unwrap()
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            install(&paths, &shipped).unwrap().1,
            Installed::Current,
            "unchanged: left alone"
        );
        let newer = fake_helper(&root.join("mount2"), "1.1.1");
        assert_eq!(install(&paths, &newer).unwrap().1, Installed::Updated);
        assert_eq!(version_of(&path).as_deref(), Some("1.1.1"));
        // Nothing else was left in the folder (the staged copy was renamed over it).
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["arcade-helper"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn removing_tools_has_the_helper_take_its_entries_away_and_deletes_the_copy() {
        let root = root("remove");
        let paths = Paths::under(&root.join("home"));
        let tools_dir = root.join("Arcade Tools");
        fake_helper(&tools_dir, "1.1.0");
        let tools = tools_dir.join("arcade-tools");
        std::fs::write(&tools, b"").unwrap();
        let (copy, _) = install(&paths, &tools_dir.join("arcade-helper")).unwrap();
        std::fs::create_dir_all(paths.link.catalog().parent().unwrap()).unwrap();
        std::fs::write(paths.link.catalog(), "{}").unwrap();
        remove(&paths, &tools).unwrap();
        assert!(!paths.link.catalog().exists(), "the shared catalog copy stays");
        // The installed copy (not the one beside Tools) ran `integrate --remove`, then was deleted.
        let ran = std::fs::read_to_string(paths.state().join("args")).unwrap();
        assert_eq!(ran.trim(), "integrate --remove");
        assert!(
            !root.join("args").exists(),
            "the copy ran, not the shipped one"
        );
        assert!(!copy.exists(), "the copy is deleted");
        assert!(!paths.state().join("helper").exists());
        // Without any helper beside Tools or installed, there is nothing to do.
        remove(&paths, &root.join("elsewhere/arcade-tools")).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stopping_with_nothing_running_is_fine_and_versions_are_read() {
        let root = root("stop");
        let paths = Paths::under(&root.join("home"));
        assert!(stop(&paths).unwrap());
        assert_eq!(running_version(&paths), None);
        let shipped = fake_helper(&root.join("mount"), "1.1.0");
        assert_eq!(version_of(&shipped).as_deref(), Some("1.1.0"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn card(paths: &Paths, id: &str, name: &str, actions: serde_json::Value) {
        let exe = std::env::current_exe().unwrap();
        let card = serde_json::json!({"schema": 1, "id": id, "name": name, "version": "1.1.0", "executable": exe, "actions": actions});
        let manifest = arcade_link::Manifest::from_json(&card.to_string()).unwrap();
        arcade_link::manifest::write_manifest(&paths.link, &manifest).unwrap();
    }

    #[test]
    fn the_page_lists_one_click_actions_to_pin_and_jobs_two_apps_do() {
        let root = root("page");
        let paths = Paths::under(&root.join("home"));
        std::fs::create_dir_all(&paths.link.registry).unwrap();
        card(&paths, "arcade.box", "Arcade Box", serde_json::json!([
            {"id": "box:image.webp", "title": "Convert to WebP", "accepts": ["file/image"], "oneClick": true, "rank": {"file/image": 80}},
            {"id": "box:image.ocr", "title": "Read text (Box)", "accepts": ["file/image"], "oneClick": true, "job": "image.read-text"},
            {"id": "box:edit", "title": "Edit image", "accepts": ["file/image"], "interactive": true}
        ]));
        card(&paths, "arcade.lens", "Arcade Lens", serde_json::json!([
            {"id": "lens.recognize", "title": "Read text", "accepts": ["file/image"], "job": "image.read-text"}
        ]));
        let page = page(&paths);
        let refs: Vec<_> = page.pins.iter().map(|p| p.action.reference.as_str()).collect();
        assert_eq!(refs, ["arcade.box/box:image.webp", "arcade.box/box:image.ocr"], "one-click only");
        assert!(page.pins.iter().all(|p| !p.pinned));
        assert_eq!(page.jobs.len(), 1);
        assert_eq!(page.jobs[0].job, "image.read-text");
        assert_eq!(page.jobs[0].choices.len(), 2);
        assert_eq!(page.jobs[0].chosen, None);

        let page = change(&paths, Change::Pins { references: vec!["arcade.box/box:image.ocr".into(), "arcade.box/box:image.ocr".into()] }).unwrap();
        assert_eq!((page.pins[0].action.reference.as_str(), page.pins[0].pinned), ("arcade.box/box:image.ocr", true));
        assert!(!page.pins[1].pinned);
        let too_many = (0..=MAX_PINS).map(|i| format!("a/{i}")).collect();
        assert!(change(&paths, Change::Pins { references: too_many }).is_err());

        let page = change(&paths, Change::JobDefault { job: "image.read-text".into(), reference: Some("arcade.lens/lens.recognize".into()) }).unwrap();
        assert_eq!(page.jobs[0].chosen.as_deref(), Some("arcade.lens/lens.recognize"));
        let page = change(&paths, Change::JobDefault { job: "image.read-text".into(), reference: None }).unwrap();
        assert_eq!(page.jobs[0].chosen, None);

        assert!(change(&paths, Change::Output { folder: "relative/dir".into(), ask_where: false }).is_err());
        let folder = root.join("Results").to_string_lossy().into_owned();
        let page = change(&paths, Change::Output { folder: folder.clone(), ask_where: true }).unwrap();
        assert_eq!((page.output_folder.as_str(), page.ask_where), (folder.as_str(), true));
        // What the helper reads.
        let prefs = arcade_link::offers::Prefs::load(&paths.link);
        assert_eq!((prefs.pinned.len(), prefs.output_folder.as_str()), (1, folder.as_str()));
        let _ = std::fs::remove_dir_all(root);
    }

}
