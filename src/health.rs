//! Read-only Home diagnostics. Read raw registry documents: Registry::load
//! intentionally drops missing executables, which health checks must report.
use crate::catalog::{read_bounded, Catalog};
use crate::installation;
use crate::paths::Paths;
use crate::{Error, Result};
use arcade_link::manifest::{ids, ManifestDocument};
use arcade_link::receipt::{InstallMethod, ManagedBy, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixAction {
    Repair,
    Adopt,
    RemoveStaleRegistry,
    RemoveStaleReceipt,
    OpenSettings,
    CheckUpdates,
    OpenEngines,
    OpenShortcuts,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthKind {
    StaleRegistry,
    TemporaryExecutable,
    MissingReceiptFile,
    UnsupportedVersion,
    LinkOff,
    InvalidReceipt,
    MissingEngine,
    ShortcutConflict,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthIssue {
    pub app: String,
    pub kind: HealthKind,
    pub path: Option<PathBuf>,
    pub reason: String,
    pub fix_action: FixAction,
    pub can_fix: bool,
}

/// Bounded registry/receipt reads on a worker, without IPC/network or mutation.
/// Every issue supplies a stable fix id; Manager::fix_health rechecks it before
/// a user-triggered fix. Development builds are reported but never changed.
pub fn inspect(paths: &Paths, catalog: &Catalog) -> Result<Vec<HealthIssue>> {
    let mut issues = Vec::new();
    let mut ids_seen = BTreeSet::new();
    let store = Store::new(&paths.link);
    let entries = match fs::read_dir(&paths.link.registry) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(entries) = entries {
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") || !entry.file_type()?.is_file() {
                continue;
            }
            paths.guard(&path)?;
            let document = match read_bounded(&path, 1024 * 1024).and_then(|bytes| {
                ManifestDocument::from_json(&String::from_utf8_lossy(&bytes))
                    .map_err(|error| Error::new("manifest", error))
            }) {
                Ok(document) => document,
                Err(error) => {
                    issues.push(HealthIssue {
                        app: path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                        kind: HealthKind::UnsupportedVersion,
                        path: Some(path),
                        reason: format!("Unreadable registry manifest: {error}"),
                        fix_action: FixAction::CheckUpdates,
                        can_fix: false,
                    });
                    continue;
                }
            };
            let peer = document.manifest;
            if path.file_stem().is_none_or(|name| name != peer.id.as_str()) {
                continue;
            }
            let native = crate::apps::APPS.contains(&peer.id.as_str()) || peer.id == ids::TOOLS;
            let dev = document.additions.install == Some(InstallMethod::Dev)
                || arcade_link::install::is_dev_build(Path::new(&peer.executable));
            let receipt = if native {
                paths.guard_receipt(&peer.id)?;
                store.read(&peer.id).ok().flatten()
            } else {
                None
            };
            ids_seen.insert(peer.id.clone());
            if !Path::new(&peer.executable).is_file() {
                issues.push(HealthIssue {
                    app: peer.id.clone(),
                    kind: HealthKind::StaleRegistry,
                    path: Some(PathBuf::from(&peer.executable)),
                    reason: "The registry executable is missing.".into(),
                    fix_action: FixAction::RemoveStaleRegistry,
                    can_fix: !dev,
                });
            } else if transient(paths, Path::new(&peer.executable)) {
                issues.push(HealthIssue {
                    app: peer.id.clone(),
                    kind: HealthKind::TemporaryExecutable,
                    path: Some(PathBuf::from(&peer.executable)),
                    reason: "This app runs from Downloads or a temporary location.".into(),
                    fix_action: FixAction::Adopt,
                    can_fix: native && !dev && peer.id != ids::TOOLS,
                });
            }
            let unsupported = if peer.schema != 1 {
                Some("This manifest schema is unsupported by Arcade Tools.".into())
            } else if !peer.link.protocol.contains(&1) {
                Some("This app requires a newer Arcade Link protocol.".into())
            } else {
                catalog.install_entry(&peer.id).map_or_else(
                    || Some("This app is absent from the catalog.".into()),
                    |app| app.install_reason,
                )
            };
            if let Some(reason) = unsupported {
                issues.push(HealthIssue {
                    app: peer.id.clone(),
                    kind: HealthKind::UnsupportedVersion,
                    path: None,
                    reason,
                    fix_action: FixAction::CheckUpdates,
                    can_fix: native && !dev,
                });
            }
            if !peer.settings.link_enabled {
                issues.push(HealthIssue {
                    app: peer.id.clone(),
                    kind: HealthKind::LinkOff,
                    path: None,
                    reason:
                        "Connect with other Arcade apps is off. Enable it in this app's settings."
                            .into(),
                    fix_action: FixAction::OpenSettings,
                    can_fix: native && Path::new(&peer.executable).is_file(),
                });
            }
            if let Some(receipt) = receipt {
                receipt_issues(paths, &receipt, dev, &mut issues);
            }
        }
    }
    for id in crate::apps::APPS.into_iter().chain([ids::TOOLS]) {
        if ids_seen.contains(id) {
            continue;
        }
        paths.guard_receipt(id)?;
        match store.read(id) {
            Ok(Some(receipt)) => receipt_issues(
                paths,
                &receipt,
                arcade_link::install::is_dev_build(&receipt.path),
                &mut issues,
            ),
            Ok(None) => {}
            Err(error) => issues.push(HealthIssue {
                app: id.into(),
                kind: HealthKind::InvalidReceipt,
                path: Some(store.path(id)?),
                reason: format!("Invalid install receipt: {error}"),
                fix_action: FixAction::RemoveStaleReceipt,
                can_fix: false,
            }),
        }
    }
    Ok(issues)
}
fn receipt_issues(
    paths: &Paths,
    receipt: &arcade_link::receipt::Receipt,
    dev: bool,
    issues: &mut Vec<HealthIssue>,
) {
    let exe = installation::executable(paths, receipt);
    for path in std::iter::once(&exe).chain(receipt.previous.iter().map(|previous| &previous.path))
    {
        if !path.exists() {
            let managed = receipt.managed_by == ManagedBy::Tools;
            issues.push(HealthIssue {
                app: receipt.id.clone(),
                kind: HealthKind::MissingReceiptFile,
                path: Some(path.clone()),
                reason: "An install receipt points at a missing file.".into(),
                fix_action: if managed {
                    FixAction::Repair
                } else {
                    FixAction::RemoveStaleReceipt
                },
                can_fix: !dev && receipt.id != ids::TOOLS && (managed || !exe.exists()),
            });
        }
    }
}
/// Pure location classification, safe on any thread. In isolated tests the
/// synthetic home itself is allowed to live under /tmp; its Downloads/tmp/cache
/// children still count as temporary.
pub fn transient(paths: &Paths, path: &Path) -> bool {
    let under_home = path.starts_with(&paths.home);
    path.starts_with(paths.home.join("Downloads"))
        || path.starts_with(paths.home.join("tmp"))
        || path.starts_with(&paths.cache)
        || path.to_string_lossy().contains("AppTranslocation")
        || ((!paths.isolated || !under_home)
            && (path.starts_with(std::env::temp_dir())
                || path.starts_with("/tmp")
                || path.starts_with("/var/tmp")
                || path.starts_with("/run")
                || path.starts_with("/Volumes")))
}
