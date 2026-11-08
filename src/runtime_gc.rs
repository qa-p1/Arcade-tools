//! U1 (AUDIT/memory.md): apps relaunch from an unpacked copy of their AppImage in
//! `<data>/arcade/runtime/<app>-<version>-<key>/` (Link `runtime::relaunch_unpacked`; the key identifies one AppImage
//! file). Tools removes the copies nothing runs from, keeping each installed app's newest copy of its installed
//! version so its next start copies nothing. Linux only; worker thread.
use crate::paths::Paths;
use crate::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A copy still being written is `.<name>.tmp-<pid>`; one this old was abandoned (a copy takes about a second).
const ABANDONED_COPY: Duration = Duration::from_secs(60 * 60);

pub fn root(paths: &Paths) -> PathBuf {
    paths.data.join("arcade").join("runtime")
}

/// The folder names to remove. `entries` are the folders with their modification times, `installed` the installed
/// apps' ids and versions (as the app itself reports it: the receipt), `in_use` whether a process runs from a folder.
pub fn unused(
    entries: &[(String, SystemTime)],
    installed: &[(&str, String)],
    in_use: &dyn Fn(&str) -> bool,
    now: SystemTime,
) -> Vec<String> {
    let mut keep: Vec<&str> = Vec::new();
    for (id, version) in installed {
        let prefix = format!("{}-{version}-", id.trim_start_matches("arcade."));
        let newest = entries
            .iter()
            .filter(|(name, _)| {
                name.strip_prefix(&prefix).is_some_and(|key| {
                    key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit())
                })
            })
            .max_by_key(|(_, modified)| *modified);
        if let Some((name, _)) = newest {
            keep.push(name);
        }
    }
    entries
        .iter()
        .filter(|(name, modified)| {
            if name.starts_with('.') {
                return name.contains(".tmp-")
                    && now
                        .duration_since(*modified)
                        .is_ok_and(|age| age > ABANDONED_COPY);
            }
            !keep.contains(&name.as_str()) && !in_use(name)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Removes the unused copies under [`root`] and returns the bytes freed.
pub fn collect(paths: &Paths, installed: &[(&str, String)]) -> Result<u64> {
    let root = root(paths);
    let Ok(read) = std::fs::read_dir(&root) else {
        return Ok(0);
    };
    let entries: Vec<(String, SystemTime)> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((e.file_name().into_string().ok()?, modified))
        })
        .collect();
    if entries.is_empty() {
        return Ok(0);
    }
    let running = running_executables();
    let in_use = |name: &str| {
        let folder = root.join(name);
        running.iter().any(|exe| exe.starts_with(&folder))
    };
    let mut freed = 0;
    for name in unused(&entries, installed, &in_use, SystemTime::now()) {
        let folder = root.join(&name);
        paths.guard(&folder)?;
        let size = size(&folder);
        if std::fs::remove_dir_all(&folder).is_ok() {
            freed += size;
        }
    }
    Ok(freed)
}

/// Every process's executable this user can see.
fn running_executables() -> Vec<PathBuf> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|e| std::fs::read_link(e.path().join("exe")).ok())
        .collect()
}

fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_running_copies_and_the_newest_copy_of_each_installed_version() {
        let now = SystemTime::now();
        let at = |minutes: u64| now - Duration::from_secs(minutes * 60);
        let entries: Vec<(String, SystemTime)> = [
            ("look-1.0.0-00000000000000aa", at(30)), // installed version, older copy
            ("look-1.0.0-00000000000000bb", at(10)), // installed version, newest copy: kept
            ("look-0.9.0-00000000000000cc", at(90)), // old version, still running: kept
            ("look-0.8.0-00000000000000dd", at(99)), // old version: removed
            ("find-1.0.0-rc.2-00000000000000ee", at(5)), // installed rc: kept
            ("box-1.0.0-00000000000000ff", at(5)),   // app no longer installed: removed
            (".tools-1.0.0-0000000000000011.tmp-42", at(5)), // copy in progress: kept
            (".tools-1.0.0-0000000000000022.tmp-43", at(120)), // abandoned copy: removed
        ]
        .into_iter()
        .map(|(n, t)| (n.to_owned(), t))
        .collect();
        let installed = [
            ("arcade.look", "1.0.0".to_owned()),
            ("arcade.find", "1.0.0-rc.2".to_owned()),
        ];
        let running = |name: &str| name == "look-0.9.0-00000000000000cc";
        let mut removed = unused(&entries, &installed, &running, now);
        removed.sort();
        assert_eq!(
            removed,
            [
                ".tools-1.0.0-0000000000000022.tmp-43",
                "box-1.0.0-00000000000000ff",
                "look-0.8.0-00000000000000dd",
                "look-1.0.0-00000000000000aa",
            ]
        );
    }

    #[test]
    fn a_version_prefix_of_another_is_not_confused_with_it() {
        let now = SystemTime::now();
        let entries = vec![("look-1.0.0-rc.1-00000000000000aa".to_owned(), now)];
        // Installed 1.0.0: "look-1.0.0-" prefixes the rc's folder, but "rc.1-…" is not a key.
        let removed = unused(
            &entries,
            &[("arcade.look", "1.0.0".into())],
            &|_| false,
            now,
        );
        assert_eq!(removed, ["look-1.0.0-rc.1-00000000000000aa"]);
    }
}
