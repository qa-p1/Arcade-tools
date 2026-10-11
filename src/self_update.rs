//! Self updates use the same installer transaction in a helper after Tools
//! exits. The desktop owns helper dispatch and shutdown; it need not duplicate
//! download, receipt, rollback or history logic. No timer is created here.
use crate::installation::Package;
use crate::manager::LaunchMode;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdatePlan {
    pub schema: u32,
    pub package: Package,
    pub expected_version: String,
    pub expected_sha256: String,
    pub expected_path: PathBuf,
    pub mode: LaunchMode,
    pub parent_pid: u32,
}
impl SelfUpdatePlan {
    /// Read a staged plan on a worker. The helper must subsequently call
    /// Manager::finish_self_update, which revalidates state, size and SHA-256.
    pub fn read(paths: &crate::paths::Paths, file: &Path) -> Result<Self> {
        paths.guard(file)?;
        if !file.starts_with(paths.state().join("self-update")) {
            return Err(Error::new("paths", "Self-update plan escaped Tools state."));
        }
        let plan: Self = serde_json::from_slice(&crate::catalog::read_bounded(file, 1024 * 1024)?)?;
        if plan.schema != 1 || plan.package.release.id != "arcade.tools" {
            return Err(Error::new("schema", "Invalid self-update plan."));
        }
        plan.package.verify(paths, "arcade.tools")?;
        paths.guard(&plan.expected_path)?;
        Ok(plan)
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSelfUpdate {
    pub plan: SelfUpdatePlan,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUninstallPlan {
    pub schema: u32,
    pub expected_version: String,
    pub expected_sha256: String,
    pub expected_path: PathBuf,
    pub parent_pid: u32,
    pub remove_data: bool,
}

pub trait SelfUpdateHook {
    /// Called on the worker only after a durable verified plan exists. The
    /// desktop broadcasts tray.host {restarting:true}, starts its helper using
    /// this plan path, and arranges shutdown. On error Tools stays running.
    fn on_before_self_restart(&mut self, staged: &StagedSelfUpdate) -> Result<()>;
}
impl<F> SelfUpdateHook for F
where
    F: FnMut(&StagedSelfUpdate) -> Result<()>,
{
    fn on_before_self_restart(&mut self, staged: &StagedSelfUpdate) -> Result<()> {
        self(staged)
    }
}
