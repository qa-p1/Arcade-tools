use super::*;

/// How long an interrupted download is kept for resuming.
const PARTIAL_DOWNLOAD_KEEP: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

impl Manager {
    /// Local housekeeping, independent of update opt-ins and network access.
    /// Receipts establish app ownership; installation/update work keeps priority.
    pub fn maintain_integrations(&self) -> Result<crate::maintenance::Report> {
        let _lock = self.lock()?;
        let mut entries = Vec::new();
        let mut unreadable = Vec::new();
        for id in crate::apps::APPS.into_iter().chain([ids::TOOLS]) {
            let read = (|| -> Result<_> {
                let Some(receipt) = self.receipt(id)? else {
                    return Ok(None);
                };
                let expected = self.record(id)?.map(|record| record.executable_sha256);
                Ok(Some((receipt, expected)))
            })();
            match read {
                Ok(Some(entry)) => entries.push(entry),
                Ok(None) => {}
                Err(error) => unreadable.push((id.to_owned(), error.to_string())),
            }
        }
        let mut attempted = self
            .maintenance_attempts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut report = crate::maintenance::maintain(&self.paths, entries, &mut attempted);
        unreadable.retain(|(id, reason)| attempted.insert(format!("{id}:unreadable:{reason}")));
        report.pending.extend(unreadable);
        if !report.repaired.is_empty() || !report.pending.is_empty() {
            self.write(&self.paths.state().join("maintenance/latest.json"), &report)?;
        }
        Ok(report)
    }
    /// Partial downloads older than a week, transaction snapshots left by a Tools that was killed mid-operation, and retained
    /// installers that neither the installed version nor its rollback needs. Startup worker only.
    pub fn cleanup_interrupted(&self) -> Result<u64> {
        let _lock = self.lock()?;
        let mut freed = 0;
        let downloads = self.paths.downloads();
        if let Ok(entries) = fs::read_dir(&downloads) {
            for entry in entries.flatten() {
                let path = entry.path();
                // A recent `<sha256>.part` is an interrupted download the next attempt resumes (D5).
                let recent_part = path.extension().is_some_and(|e| e == "part")
                    && entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age < PARTIAL_DOWNLOAD_KEEP);
                if recent_part {
                    continue;
                }
                self.paths.guard(&path)?;
                freed += entry.metadata().map_or(0, |m| m.len());
                platform::remove(&path)?;
            }
        }
        let ids: Vec<&str> = crate::apps::APPS
            .iter()
            .copied()
            .chain([ids::TOOLS])
            .collect();
        freed += installation::remove_orphan_snapshots(&self.paths, &ids)?;
        for id in &ids {
            freed += installation::prune_installers(&self.paths, id)?;
        }
        Ok(freed)
    }
    /// Tools records left behind when an app uninstalled itself. Run on the
    /// startup worker, like `cleanup_stale_bundles`.
    pub fn cleanup_orphan_records(&self) -> Result<Vec<String>> {
        let _lock = self.lock()?;
        let mut removed = Vec::new();
        for id in crate::apps::APPS.iter().copied() {
            let file = self.paths.state().join(format!("{id}.json"));
            let Ok(bytes) = fs::read(&file) else { continue };
            let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
                continue;
            };
            if record.id != id || self.receipt(id)?.is_some() || !self.uninstalled_by_app(&record) {
                continue;
            }
            if let Some(previous) = PreviousInstallation::read(&self.paths, id)? {
                previous.remove(&self.paths)?;
            }
            for file in [file, self.paths.state().join(format!("{id}.previous.json"))] {
                self.paths.guard(&file)?;
                platform::remove(&file)?;
            }
            removed.push(id.into());
        }
        Ok(removed)
    }
    /// A bundle moved to Trash no longer owns an installed app. Run once on
    /// the startup worker; missing Linux files remain repairable installations.
    pub fn cleanup_stale_bundles(&self) -> Result<Vec<String>> {
        let _lock = self.lock()?;
        let mut removed = Vec::new();
        for id in crate::apps::APPS.iter().copied().chain([ids::TOOLS]) {
            let Some(receipt) = self.receipt(id)? else {
                continue;
            };
            if receipt.method != InstallMethod::MacosBundle || receipt.path.exists() {
                continue;
            }
            installation::cleanup_missing_bundle(&self.paths, &receipt)?;
            Store::new(&self.paths.link).remove(id)?;
            for file in [
                self.paths.state().join(format!("{id}.json")),
                self.paths.state().join(format!("{id}.previous.json")),
            ] {
                self.paths.guard(&file)?;
                platform::remove(&file)?;
            }
            let registry = Registry::load(&self.paths.link);
            if registry
                .get(id)
                .is_some_and(|peer| !peer.executable_exists())
            {
                manifest::remove_manifest(&self.paths.link, id)?;
            }
            removed.push(id.into());
        }
        Ok(removed)
    }
    /// Read Home diagnostics on a worker; no IPC/network or automatic cleanup.
    pub fn health(&self) -> Result<Vec<crate::health::HealthIssue>> {
        crate::health::inspect(&self.paths, &self.catalog()?)
    }
    /// Apply a user-selected fix on a worker after rechecking the issue. The UI
    /// supplies the usual confirmation for adoption/repair, and separately asks
    /// before any data removal. This API never changes development builds.
    pub fn fix_health(
        &self,
        id: &str,
        action: crate::health::FixAction,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        manager_id(id)?;
        let _lock = self.lock()?;
        if !self
            .health()?
            .iter()
            .any(|issue| issue.app == id && issue.fix_action == action && issue.can_fix)
        {
            return Err(Error::new(
                "state_changed",
                "This health issue is no longer present or cannot be fixed automatically.",
            ));
        }
        use crate::health::FixAction;
        match action {
            FixAction::Repair | FixAction::Adopt => {
                let request = Request {
                    id: id.into(),
                    operation: if action == FixAction::Repair {
                        Operation::Repair
                    } else {
                        Operation::Adopt
                    },
                    mode: None,
                    remove_data: false,
                    app_closed: false,
                    channel: None,
                    version: None,
                };
                self.operate_locked(&request, cancel, progress, SystemClock.now())
            }
            FixAction::RemoveStaleRegistry => {
                let file = self.paths.link.registry.join(format!("{id}.json"));
                self.paths.guard(&file)?;
                let document = arcade_link::manifest::ManifestDocument::from_json(
                    &String::from_utf8_lossy(&crate::catalog::read_bounded(&file, 1024 * 1024)?),
                )
                .map_err(|error| Error::new("manifest", error))?;
                if document.manifest.executable_exists() {
                    return Err(Error::new(
                        "state_changed",
                        "The executable is present; its registry entry was kept.",
                    ));
                }
                manifest::remove_manifest(&self.paths.link, id)?;
                Ok("Removed the stale registry entry.".into())
            }
            FixAction::RemoveStaleReceipt => {
                let receipt = self
                    .receipt(id)?
                    .ok_or_else(|| Error::new("state_changed", "The receipt is already gone."))?;
                if installation::executable(&self.paths, &receipt).exists() {
                    return Err(Error::new(
                        "state_changed",
                        "The receipt's executable is present; adoption is required.",
                    ));
                }
                installation::cleanup_missing_bundle(&self.paths, &receipt)?;
                Store::new(&self.paths.link).remove(id)?;
                Ok("Removed the stale receipt; no app or data files were removed.".into())
            }
            FixAction::OpenSettings => self
                .settings_inner(id)
                .map(|()| "Opened the app's settings.".into()),
            FixAction::CheckUpdates => self
                .checked(id)
                .map(|checked| format!("Latest release: {}.", checked.release.version)),
            FixAction::OpenEngines | FixAction::OpenShortcuts => Err(Error::new(
                "guidance",
                "Open the corresponding Tools page to review this issue.",
            )),
        }
    }
}
