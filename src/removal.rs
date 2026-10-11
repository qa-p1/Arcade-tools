//! Reviewable uninstall plans. UI confirmation precedes execution; the core
//! rechecks the complete plan before quitting any app. Manual/dev installs are
//! listed with a reason and left intact. Tools itself finishes in a helper.
use crate::manager::InstallationClass;
use crate::self_update::SelfUninstallPlan;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallEntry {
    pub id: String,
    pub version: Option<String>,
    pub class: InstallationClass,
    pub folders: Vec<PathBuf>,
    pub data_folders: Vec<PathBuf>,
    pub can_uninstall: bool,
    pub requires_self_shutdown: bool,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPlan {
    pub schema: u32,
    pub remove_data: bool,
    pub apps: Vec<UninstallEntry>,
    pub fingerprint: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct RemovalResult {
    pub app: String,
    pub removed: bool,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallOutcome {
    pub results: Vec<RemovalResult>,
    /// The desktop dispatches this only after showing the reviewed plan and
    /// shutting Tools down. finish_self_uninstall performs the remaining step.
    pub self_uninstall: Option<SelfUninstallPlan>,
}
