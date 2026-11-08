//! The backup taken when an app moves from Nightly back to Stable (ARCADE_APP.md FS-2, SPEC.md §Updater 2): a
//! nightly may have migrated its settings and data forward, so the older stable build gets a copy to fall back on.
//! One backup per app is kept: the newest switch replaces the last. Worker thread, under the manager lock.
use crate::paths::Paths;
use crate::release::Os;
use crate::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// A backup larger than this is refused, so a switch never fills the disk; the switch then doesn't happen.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn root(paths: &Paths, id: &str) -> PathBuf {
    paths.state().join("channel-backups").join(id)
}

/// Copies the app's settings and data folders (not its cache) into `root/<version>`. Returns where, or `None` when
/// the app has no data yet.
pub fn back_up(paths: &Paths, id: &str, version: &str) -> Result<Option<PathBuf>> {
    let folders: Vec<PathBuf> = paths
        .data_folders(id, Os::current())
        .into_iter()
        .filter(|f| f.is_dir() && !f.starts_with(&paths.cache))
        .collect();
    if folders.is_empty() {
        return Ok(None);
    }
    let total: u64 = folders.iter().map(|f| size(f)).sum();
    if total > MAX_BYTES {
        return Err(Error::new(
            "too_large",
            format!(
                "The app's data is {} MiB, too large to back up before switching to Stable. Nothing was changed.",
                total / (1024 * 1024)
            ),
        ));
    }
    let root = root(paths, id);
    paths.guard(&root)?;
    let staging = root.with_file_name(format!("{id}.partial"));
    let _ = fs::remove_dir_all(&staging);
    let target = staging.join(safe(version));
    fs::create_dir_all(&target)?;
    let mut index = Vec::new();
    for (n, folder) in folders.iter().enumerate() {
        let name = format!(
            "{n}-{}",
            folder
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("data")
        );
        copy_tree(folder, &target.join(&name))?;
        index.push(serde_json::json!({"copy": name, "from": folder}));
    }
    fs::write(
        target.join("backup.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"app": id, "version": version, "folders": index}),
        )?,
    )?;
    let _ = fs::remove_dir_all(&root);
    fs::rename(&staging, &root)?;
    Ok(Some(root.join(safe(version))))
}

fn safe(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// Files and folders are copied; symbolic links are kept as links, never followed.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&source)?, &target)?;
        } else if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}
