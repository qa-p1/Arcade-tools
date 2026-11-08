use super::*;

impl Manager {
    /// List every catalog/registry installation and its exact owned/data paths
    /// on a worker before an Uninstall everything confirmation. No mutation.
    pub fn plan_uninstall_all(&self, remove_data: bool) -> Result<crate::removal::UninstallPlan> {
        let _lock = self.lock()?;
        self.uninstall_plan_inner(remove_data)
    }
    pub(super) fn uninstall_plan_inner(
        &self,
        remove_data: bool,
    ) -> Result<crate::removal::UninstallPlan> {
        use crate::removal::{UninstallEntry, UninstallPlan};
        use sha2::{Digest, Sha256};
        let registry = Registry::load(&self.paths.link);
        let catalog = self.catalog()?;
        let mut ids: std::collections::BTreeSet<String> =
            catalog.apps.iter().map(|app| app.id.clone()).collect();
        ids.extend(crate::apps::APPS.into_iter().map(String::from));
        ids.extend(registry.apps().iter().map(|peer| peer.id.clone()));
        ids.remove(ids::TOOLS);
        let mut apps = Vec::new();
        let mut fingerprint = Sha256::new();
        for id in ids.into_iter().chain([ids::TOOLS.into()]) {
            let known = manager_id(&id).is_ok();
            let record = if known { self.record(&id)? } else { None };
            let receipt = if known { self.receipt(&id)? } else { None };
            let peer = registry.get(&id);
            if record.is_none() && receipt.is_none() && peer.is_none() {
                continue;
            }
            let class = self.class(&id, &registry, receipt.as_ref())?;
            let mut folders = Vec::new();
            if let Some(record) = &record {
                folders.push(installation::target(&self.paths, record, receipt.as_ref()));
                folders.push(self.paths.state().join(format!("{id}.json")));
                if let Some(previous) = PreviousInstallation::read(&self.paths, &id)? {
                    folders.push(previous.directory);
                }
                folders.push(self.paths.state().join(format!("{id}.previous.json")));
            } else if let Some(peer) = peer {
                folders.push(PathBuf::from(&peer.executable));
            }
            if let Some(receipt) = &receipt {
                folders.extend(installation::owned_paths(&self.paths, receipt)?);
                folders.push(self.paths.guard_receipt(&id)?);
                if let Some(previous) = &receipt.previous {
                    folders.push(previous.path.clone());
                }
            }
            folders.push(self.paths.link.registry.join(format!("{id}.json")));
            let data_folders = self.paths.data_folders(&id, Os::current());
            let can_uninstall = known && class != InstallationClass::Dev && record.is_some();
            if can_uninstall {
                for path in folders.iter().chain(if remove_data {
                    data_folders.iter()
                } else {
                    [].iter()
                }) {
                    if receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.integration.cli.as_ref() == Some(path))
                    {
                        installation::owned_paths(&self.paths, receipt.as_ref().unwrap())?;
                    } else if path == &self.paths.guard_receipt(&id)? {
                        self.paths.guard_receipt(&id)?;
                    } else {
                        self.paths.guard(path)?;
                    }
                }
            }
            folders.sort();
            folders.dedup();
            fingerprint.update(serde_json::to_vec(&(
                record.as_ref(),
                receipt.as_ref(),
                peer,
            ))?);
            if let Some(record) = &record {
                fingerprint.update(checksum(&self.executable(record)).unwrap_or_default());
            }
            apps.push(UninstallEntry {
                id: id.clone(),
                version: record
                    .as_ref()
                    .map(|record| record.version.clone())
                    .or_else(|| peer.map(|peer| peer.version.clone())),
                class,
                folders,
                data_folders,
                can_uninstall,
                requires_self_shutdown: id == ids::TOOLS,
                reason: if can_uninstall {
                    None
                } else {
                    Some(
                        if class == InstallationClass::Dev {
                            "Development builds are never removed."
                        } else {
                            "Adopt this manual installation before uninstalling it with Tools."
                        }
                        .into(),
                    )
                },
            });
        }
        Ok(UninstallPlan {
            schema: 1,
            remove_data,
            apps,
            fingerprint: format!("{:x}", fingerprint.finalize()),
        })
    }
    /// Execute a previously confirmed plan on a worker. Any changed installation
    /// rejects the entire plan before app.quit. Individual busy/disconnected
    /// failures are returned. Tools removal is handed to a post-shutdown helper.
    pub fn uninstall_all(
        &self,
        plan: &crate::removal::UninstallPlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<crate::removal::UninstallOutcome> {
        let _lock = self.lock()?;
        if plan.schema != 1 || &self.uninstall_plan_inner(plan.remove_data)? != plan {
            return Err(Error::new(
                "state_changed",
                "Installations changed; review the uninstall list again.",
            ));
        }
        let mut outcome = crate::removal::UninstallOutcome {
            results: Vec::new(),
            self_uninstall: None,
        };
        for app in &plan.apps {
            check_cancel(cancel)?;
            if !app.can_uninstall {
                outcome.results.push(crate::removal::RemovalResult {
                    app: app.id.clone(),
                    removed: false,
                    reason: app.reason.clone().unwrap_or_default(),
                });
                continue;
            }
            if app.requires_self_shutdown {
                // Never leave apps behind without their manager: keep Tools if
                // any app it was asked to remove is still installed.
                let failed: Vec<&str> = outcome
                    .results
                    .iter()
                    .filter(|r| {
                        !r.removed && plan.apps.iter().any(|a| a.id == r.app && a.can_uninstall)
                    })
                    .map(|r| r.app.as_str())
                    .collect();
                if failed.is_empty() {
                    outcome.self_uninstall = Some(self.self_uninstall_plan(plan.remove_data)?);
                } else {
                    outcome.results.push(crate::removal::RemovalResult {
                        app: app.id.clone(),
                        removed: false,
                        reason: format!(
                            "Arcade Tools was kept because {} could not be removed.",
                            failed
                                .iter()
                                .map(|id| crate::apps::app_name(id))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                continue;
            }
            let request = Request {
                id: app.id.clone(),
                operation: Operation::Uninstall,
                mode: None,
                remove_data: plan.remove_data,
                app_closed: false,
                channel: None,
                version: None,
            };
            let result = self.operate_locked(&request, cancel, progress, SystemClock.now());
            outcome.results.push(crate::removal::RemovalResult {
                app: app.id.clone(),
                removed: result.is_ok(),
                reason: match result {
                    Ok(message) => message,
                    Err(error) => error.to_string(),
                },
            });
        }
        Ok(outcome)
    }
    /// Prepare removal of Tools itself after user confirmation, on a worker.
    /// The desktop starts a helper and exits; no running executable is deleted.
    pub fn prepare_self_uninstall(
        &self,
        remove_data: bool,
    ) -> Result<crate::self_update::SelfUninstallPlan> {
        let _lock = self.lock()?;
        self.self_uninstall_plan(remove_data)
    }
    pub(super) fn self_uninstall_plan(
        &self,
        remove_data: bool,
    ) -> Result<crate::self_update::SelfUninstallPlan> {
        let record = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Arcade Tools has no install receipt."))?;
        let receipt = self.receipt(ids::TOOLS)?;
        if self.class(
            ids::TOOLS,
            &Registry::load(&self.paths.link),
            receipt.as_ref(),
        )? == InstallationClass::Dev
        {
            return Err(Error::new(
                "dev_build",
                "Development builds are never uninstalled.",
            ));
        }
        let path = self.executable(&record);
        self.paths.guard(&path)?;
        Ok(crate::self_update::SelfUninstallPlan {
            schema: 1,
            expected_version: record.version,
            expected_sha256: checksum(&path)?,
            expected_path: path,
            parent_pid: std::process::id(),
            remove_data,
        })
    }
    /// Run in a helper on a worker after Tools exits; bounded process-exit checks
    /// and fresh state/hash checks precede the app's own silent uninstaller.
    pub fn finish_self_uninstall(
        &self,
        plan: &crate::self_update::SelfUninstallPlan,
        cancel: &AtomicBool,
    ) -> Result<String> {
        if plan.schema != 1 || plan.parent_pid == std::process::id() {
            return Err(Error::new(
                "self_update",
                "Tools removal must run in a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Arcade Tools did not exit; nothing was removed.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.paths.guard(&plan.expected_path)?;
        let record = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("state_changed", "Tools is already uninstalled."))?;
        if self.class(
            ids::TOOLS,
            &Registry::load(&self.paths.link),
            self.receipt(ids::TOOLS)?.as_ref(),
        )? == InstallationClass::Dev
            || record.version != plan.expected_version
            || self.executable(&record) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after confirmation; nothing was removed.",
            ));
        }
        check_cancel(cancel)?;
        let result = self.uninstall(&record, plan.remove_data)?;
        if plan.remove_data {
            let cache = self.paths.cache.join("arcade/tools");
            self.paths.guard(&cache)?;
            platform::remove(&cache)?;
        }
        if let Err(error) = self.release_managed_apps() {
            eprintln!("Releasing apps from Tools: {error}");
        }
        Ok(result)
    }

    /// Apps that stay installed when Tools is removed go back to managing
    /// themselves (their own update checks and installers), and Tools'
    /// rollback snapshots for them, which nothing else can use, are removed.
    pub fn release_managed_apps(&self) -> Result<Vec<String>> {
        let store = Store::new(&self.paths.link);
        let mut released = Vec::new();
        for id in crate::apps::APPS.into_iter().filter(|id| *id != ids::TOOLS) {
            if let Some(previous) = PreviousInstallation::read(&self.paths, id)? {
                previous.remove(&self.paths)?;
            }
            platform::remove(&self.paths.state().join(format!("{id}.previous.json")))?;
        }
        installation::remove_orphan_snapshots(&self.paths, &[])?;
        for id in crate::apps::APPS.into_iter().filter(|id| *id != ids::TOOLS) {
            let Some(mut receipt) = store.read(id)? else {
                continue;
            };
            if receipt.managed_by != ManagedBy::Tools {
                continue;
            }
            receipt.managed_by = ManagedBy::SelfManaged;
            if receipt
                .previous
                .as_ref()
                .is_some_and(|previous| !previous.path.exists())
            {
                receipt.previous = None;
            }
            store.write(&receipt)?;
            released.push(id.to_string());
        }
        Ok(released)
    }
    pub(super) fn uninstall(&self, record: &Record, remove_data: bool) -> Result<String> {
        let id = &record.id;
        let exe = self.executable(record);
        let root = self
            .paths
            .install_root(id, Os::current(), record.kind == Kind::Tarball);
        let receipt = self.receipt(id)?;
        let previous = PreviousInstallation::read(&self.paths, id)?;
        let target = installation::target(&self.paths, record, receipt.as_ref());
        self.paths.guard(&target)?;
        if let Some(receipt) = &receipt {
            installation::owned_paths(&self.paths, receipt)?;
            if let Some(previous) = &receipt.previous {
                self.paths.guard(&previous.path)?;
            }
        }
        if remove_data {
            for folder in self.paths.data_folders(id, Os::current()) {
                self.paths.guard(&folder)?;
            }
        }
        if record.installed_by_app {
            if Os::current() == Os::Windows {
                let uninstaller = receipt
                    .as_ref()
                    .and_then(|r| r.integration.uninstaller.as_ref())
                    .ok_or_else(|| {
                        Error::new(
                            "installer",
                            "The receipt's native uninstaller is missing. Repair the app first.",
                        )
                    })?;
                self.paths.guard(uninstaller)?;
                if !uninstaller.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The native uninstaller is missing. Repair the app first.",
                    ));
                }
                let mut command = platform::windows_uninstall(
                    windows_kind(Some(uninstaller)),
                    uninstaller.parent().unwrap(),
                )?;
                command.program = uninstaller.clone();
                command.run(&self.paths)?;
            } else {
                if !exe.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The app's uninstaller is missing. Repair the app first.",
                    ));
                }
                let mut args = vec!["--uninstall".into(), "--silent".into()];
                if remove_data {
                    args.push("--remove-data".into());
                }
                platform::CommandSpec {
                    program: exe.clone(),
                    args,
                }
                .run(&self.paths)?;
            }
            if exe.exists() {
                return Err(Error::new(
                    "installer",
                    "The app's uninstaller left its executable in place.",
                ));
            }
        } else {
            // Transition releases without installArgs use the pre-v0.3 removal
            // path. Never use this fallback for an app with an own installer.
            if platform::login_supported(id, Os::current()) {
                platform::set_autostart(&self.paths, id, &exe, false)?;
            }
            #[cfg(windows)]
            {
                let uninstaller = platform::windows_uninstall(record.kind, &root)?;
                self.paths.guard(&uninstaller.program)?;
                if !uninstaller.program.is_file() {
                    return Err(Error::new(
                        "installer",
                        "The platform uninstaller is missing. Repair the app first.",
                    ));
                }
                uninstaller.run(&self.paths)?;
            }
            platform::remove(&target)?;
        }
        // Remove launch entries only when their Exec references the managed installation.
        if Os::current() == Os::Linux {
            let applications = self.paths.data.join("applications");
            if let Ok(entries) = fs::read_dir(&applications) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "desktop")
                        && fs::read_to_string(&p).is_ok_and(|s| {
                            s.lines().any(|l| {
                                l.starts_with("Exec=")
                                    && (l.contains(&exe.to_string_lossy().to_string())
                                        || record.kind == Kind::Tarball
                                            && l.contains(&root.to_string_lossy().to_string()))
                            })
                        })
                    {
                        self.paths.guard(&p)?;
                        platform::remove(&p)?;
                    }
                }
            }
        }
        if remove_data {
            for folder in self.paths.data_folders(id, Os::current()) {
                platform::remove(&folder)?;
            }
        }
        manifest::remove_manifest(&self.paths.link, id)?;
        self.paths.guard_receipt(id)?;
        Store::new(&self.paths.link).remove(id)?;
        if let Some(previous) = previous {
            previous.remove(&self.paths)?;
        }
        platform::remove(&self.paths.state().join(format!("{id}.previous.json")))?;
        platform::remove(&self.paths.state().join(format!("{id}.json")))?;
        Ok(format!(
            "{} removed. {}",
            crate::apps::app_name(id),
            if remove_data {
                "Settings and data removed."
            } else {
                "Settings and data kept."
            }
        ))
    }
}
