//! Process metrics sampled only by requests from the visible Home page.
use crate::manager::{me, Manager};
use crate::Result;
use arcade_link::client::Client;
use arcade_link::registry::Registry;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetrics {
    pub id: String,
    pub pid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
    pub uptime_seconds: Option<u64>,
    pub tray: Option<String>,
    pub link_enabled: bool,
    pub reachable: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub apps: Vec<AppMetrics>,
    pub total_memory_bytes: u64,
    pub total_cpu_percent: f32,
}
#[derive(Default)]
pub struct Metrics {
    system: System,
    sampled: bool,
}
impl Metrics {
    pub fn sample(&mut self, manager: &Manager) -> Result<Dashboard> {
        let registry = Registry::load(&manager.paths.link);
        let entries = manager
            .list(false)?
            .into_iter()
            .filter(|app| app.installed)
            .map(|app| {
                let endpoint = arcade_link::endpoint::read(&manager.paths.link, &app.id).ok();
                (app.id, endpoint)
            })
            .collect::<Vec<_>>();
        let pids = entries
            .iter()
            .filter_map(|(_, endpoint)| endpoint.as_ref().map(|e| Pid::from_u32(e.pid)))
            .collect::<Vec<_>>();
        if !pids.is_empty() {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                true,
                ProcessRefreshKind::nothing()
                    .with_cpu()
                    .with_memory()
                    .without_tasks(),
            );
        }
        #[cfg(target_os = "linux")]
        let table = linux::table();
        let mut total_memory_bytes = 0;
        let mut total_cpu_percent = 0.0;
        let apps = entries
            .into_iter()
            .map(|(id, endpoint)| {
                let status = Client::connect_with(
                    &manager.paths.link,
                    &id,
                    &me(),
                    Duration::from_millis(200),
                )
                .and_then(|mut client| client.status())
                .ok();
                let process = endpoint
                    .as_ref()
                    .and_then(|e| self.system.process(Pid::from_u32(e.pid)));
                // TD1: the app's whole process tree (WebKit helpers, workers) and its AppImage's FUSE helper, by
                // PSS, so shared libraries count once across the fleet. Elsewhere the main process's resident size.
                #[cfg(target_os = "linux")]
                let memory_bytes = process.map(|_| {
                    let image = registry
                        .get(&id)
                        .map(|m| std::path::PathBuf::from(&m.executable));
                    linux::tree_pss(
                        endpoint.as_ref().map_or(0, |e| e.pid),
                        image.as_deref(),
                        &table,
                    )
                });
                #[cfg(not(target_os = "linux"))]
                let memory_bytes = process.map(|p| p.memory());
                let cpu_percent = self
                    .sampled
                    .then(|| process.map(|p| p.cpu_usage()))
                    .flatten();
                total_memory_bytes += memory_bytes.unwrap_or(0);
                total_cpu_percent += cpu_percent.unwrap_or(0.0);
                AppMetrics {
                    link_enabled: registry.get(&id).is_some_and(|m| m.settings.link_enabled),
                    id,
                    pid: process.and_then(|_| endpoint.as_ref().map(|e| e.pid)),
                    memory_bytes,
                    cpu_percent,
                    uptime_seconds: process.map(|p| p.run_time()),
                    tray: status
                        .as_ref()
                        .and_then(|s| s.pointer("/status/tray"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    reachable: status.is_some(),
                }
            })
            .collect();
        self.sampled = true;
        Ok(Dashboard {
            apps,
            total_memory_bytes,
            total_cpu_percent,
        })
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    /// One process: pid, parent pid and executable.
    pub type Table = Vec<(u32, u32, Option<PathBuf>)>;

    pub fn table() -> Table {
        std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let pid: u32 = e.file_name().to_str()?.parse().ok()?;
                let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
                Some((
                    pid,
                    parent(&stat)?,
                    std::fs::read_link(e.path().join("exe")).ok(),
                ))
            })
            .collect()
    }

    /// The parent pid from `/proc/<pid>/stat` (after the parenthesised name, which may hold spaces).
    fn parent(stat: &str) -> Option<u32> {
        stat[stat.rfind(')')? + 1..]
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    /// `root` and its descendants, plus the AppImage runtime processes of `image` (re-parented, so not
    /// descendants).
    pub fn members(root: u32, image: Option<&Path>, table: &Table) -> Vec<u32> {
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            let parent = out[i];
            out.extend(
                table
                    .iter()
                    .filter(|(pid, ppid, _)| *ppid == parent && !out.contains(pid))
                    .map(|(pid, _, _)| *pid)
                    .collect::<Vec<_>>(),
            );
            i += 1;
        }
        if let Some(image) = image.filter(|i| i.extension().is_some_and(|e| e == "AppImage")) {
            let helpers: Vec<u32> = table
                .iter()
                .filter(|(pid, _, exe)| exe.as_deref() == Some(image) && !out.contains(pid))
                .map(|(pid, _, _)| *pid)
                .collect();
            out.extend(helpers);
        }
        out
    }

    /// `Pss:` from `smaps_rollup`, in bytes.
    fn pss(rollup: &str) -> Option<u64> {
        let kib: u64 = rollup
            .lines()
            .find_map(|l| l.strip_prefix("Pss:"))?
            .trim()
            .strip_suffix("kB")?
            .trim()
            .parse()
            .ok()?;
        Some(kib * 1024)
    }

    pub fn tree_pss(root: u32, image: Option<&Path>, table: &Table) -> u64 {
        members(root, image, table)
            .into_iter()
            .filter_map(|pid| {
                pss(&std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?)
            })
            .sum()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_tree_includes_children_grandchildren_and_the_appimage_helper() {
            let image = PathBuf::from("/home/u/Applications/Arcade-Look.AppImage");
            let table: Table = vec![
                (
                    10,
                    1,
                    Some(PathBuf::from("/data/arcade/runtime/look-1/usr/bin/look")),
                ),
                (
                    11,
                    10,
                    Some(PathBuf::from("/usr/lib/webkit/WebKitWebProcess")),
                ),
                (12, 11, None),
                (13, 1, Some(image.clone())), // the FUSE helper, re-parented to init
                (20, 1, Some(PathBuf::from("/usr/bin/other"))),
            ];
            let mut got = members(10, Some(&image), &table);
            got.sort();
            assert_eq!(got, [10, 11, 12, 13]);
        }

        #[test]
        fn reads_pss_and_the_parent_after_a_name_with_spaces() {
            assert_eq!(
                pss("Rss:  2048 kB\nPss:  1536 kB\nPss_Anon: 3 kB\n"),
                Some(1536 * 1024)
            );
            assert_eq!(parent("42 (Web Content) S 7 42 42 0"), Some(7));
        }
    }
}
