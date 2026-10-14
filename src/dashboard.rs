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
