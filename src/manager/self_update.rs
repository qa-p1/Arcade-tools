use super::*;

impl Manager {
    pub(super) fn own_process(&self, id: &str) -> bool {
        id == ids::TOOLS
            && arcade_link::endpoint::read(&self.paths.link, id)
                .is_ok_and(|endpoint| endpoint.pid == std::process::id())
    }

    /// Download and verify Tools itself on a worker, persist a handoff plan, then
    /// call the tray/desktop hook. Tools cannot replace its running Windows
    /// executable; the hook starts a helper and owns shutdown. The helper calls
    /// finish_self_update after this process exits, using the same installer API.
    pub fn prepare_self_update(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<crate::self_update::StagedSelfUpdate> {
        let _lock = self.lock()?;
        self.prepare_self_update_locked(mode, cancel, progress, hook)
    }

    /// Stage rollback for the same separate helper used by self updates.
    pub fn prepare_self_rollback(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        hook: &mut dyn FnMut(&Path) -> Result<()>,
    ) -> Result<PathBuf> {
        let _lock = self.lock()?;
        check_cancel(cancel)?;
        let receipt = self.receipt(ids::TOOLS)?;
        if !matches!(
            self.class(
                ids::TOOLS,
                &Registry::load(&self.paths.link),
                receipt.as_ref()
            )?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "unmanaged",
                "Install Arcade Tools before rolling it back.",
            ));
        }
        if self.rollback_version(ids::TOOLS)?.is_none() {
            return Err(Error::new(
                "no_previous",
                "There is no retained previous Tools version.",
            ));
        }
        let old = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Tools is not installed."))?;
        let plan = crate::self_update::SelfRollbackPlan {
            schema: 1,
            expected_version: old.version.clone(),
            expected_path: self.executable(&old),
            expected_sha256: checksum(&self.executable(&old))?,
            mode,
            parent_pid: std::process::id(),
        };
        let file = self.paths.state().join("self-update").join(format!(
            "rollback-{}.json",
            arcade_link::endpoint::new_token()?
        ));
        self.write(&file, &plan)?;
        if let Err(error) = hook(&file) {
            platform::remove(&file)?;
            return Err(error);
        }
        Ok(file)
    }

    /// Revalidate the current installation after shutdown, then roll back.
    pub fn finish_self_rollback(
        &self,
        plan: &crate::self_update::SelfRollbackPlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        if plan.schema != 1 || plan.parent_pid == std::process::id() {
            return Err(Error::new(
                "self_update",
                "Tools rollback requires a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Old Arcade Tools did not exit; rollback left it unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let old = self
            .record(ids::TOOLS)?
            .ok_or_else(|| Error::new("unmanaged", "Tools is not installed."))?;
        self.paths.guard(&plan.expected_path)?;
        if old.version != plan.expected_version
            || self.executable(&old) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after staging; request rollback again.",
            ));
        }
        let result = self.operate_locked(
            &Request {
                id: ids::TOOLS.into(),
                operation: Operation::Rollback,
                mode: Some(plan.mode),
                remove_data: false,
                app_closed: true,
                channel: None,
                version: None,
            },
            cancel,
            progress,
            SystemClock.now(),
        );
        let launch = platform::spawn_detached(&self.paths, &plan.expected_path, &plan.mode.args());
        if result.is_ok() {
            launch?;
        }
        result
    }
    pub(super) fn prepare_self_update_locked(
        &self,
        mode: LaunchMode,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<crate::self_update::StagedSelfUpdate> {
        let id = ids::TOOLS;
        check_cancel(cancel)?;
        let old = self.record(id)?.ok_or_else(|| {
            Error::new(
                "unmanaged",
                "Install or adopt Arcade Tools before self-updating.",
            )
        })?;
        let receipt = self.receipt(id)?;
        let registry = Registry::load(&self.paths.link);
        if !matches!(
            self.class(id, &registry, receipt.as_ref())?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "unmanaged",
                "Only managed non-development Tools installs can self-update.",
            ));
        }
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            let status = client.status().map_err(|error| link_error(id, error))?;
            if !self.own_process(id)
                && (status.get("busy").and_then(Value::as_bool) == Some(true)
                    || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true))
            {
                return Err(Error::new(
                    "busy",
                    "Arcade Tools is busy; finish its operation before self-updating.",
                ));
            }
        }
        progress(Progress::new(id, "Checking release", None, true));
        let checked = self.checked(id)?;
        self.paths.guard(&self.paths.downloads())?;
        let download = self.source.download_from(
            &checked,
            &self.paths.downloads(),
            &self.local_chunks(id),
            cancel,
            &mut |count, total| {
                progress(Progress::new(
                    id,
                    "Downloading",
                    total.filter(|n| *n > 0).map(|n| count as f64 / n as f64),
                    true,
                ));
            },
        )?;
        let package = installation::retain(&self.paths, &checked, download.path())?;
        check_cancel(cancel)?;
        let plan = crate::self_update::SelfUpdatePlan {
            schema: 1,
            package,
            expected_version: old.version.clone(),
            expected_path: self.executable(&old),
            expected_sha256: checksum(&self.executable(&old))?,
            mode,
            parent_pid: std::process::id(),
        };
        let file = self
            .paths
            .state()
            .join("self-update")
            .join(format!("{}.json", arcade_link::endpoint::new_token()?));
        self.write(&file, &plan)?;
        let staged = crate::self_update::StagedSelfUpdate { plan, file };
        if let Err(error) = hook.on_before_self_restart(&staged) {
            platform::remove(&staged.file)?;
            return Err(error);
        }
        Ok(staged)
    }
    /// Apply the opted-in idle policy before staging a Tools update on a worker.
    /// None means ineligible; use auto_update_eligibility for the UI's reason.
    pub fn prepare_auto_self_update(
        &self,
        last_used: Option<u64>,
        clock: &dyn Clock,
        mode: LaunchMode,
        cancel: &AtomicBool,
        hook: &mut dyn crate::self_update::SelfUpdateHook,
    ) -> Result<Option<crate::self_update::StagedSelfUpdate>> {
        let _lock = self.lock()?;
        if !self
            .auto_eligibility(ids::TOOLS, last_used, clock.now())?
            .eligible
        {
            return Ok(None);
        }
        // As for apps (`auto_update`): a release Tools rolled back from, or failed to install, waits for a newer one.
        if self.skipped_after_rollback(ids::TOOLS)?.is_some() {
            return Ok(None);
        }
        self.prepare_self_update_locked(mode, cancel, &mut |_| {}, hook)
            .map(Some)
    }
    /// Run in a helper on a worker after the parent exits. Active, bounded
    /// process-exit checks last at most 30 seconds, then leave files unchanged.
    /// The plan's hashes and expected installation are rechecked before mutation.
    /// On installer failure the shared transaction restores/relaunches old Tools.
    pub fn finish_self_update(
        &self,
        plan: &crate::self_update::SelfUpdatePlan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        if plan.schema != 1
            || plan.package.release.id != ids::TOOLS
            || plan.parent_pid == std::process::id()
        {
            return Err(Error::new(
                "self_update",
                "A Tools self-update must run in a separate helper after shutdown.",
            ));
        }
        let _lock = self.lock()?;
        plan.package.verify(&self.paths, ids::TOOLS)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process_alive(plan.parent_pid) {
            check_cancel(cancel)?;
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "timeout",
                    "Old Arcade Tools did not exit; self-update left it unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let old = self.record(ids::TOOLS)?.ok_or_else(|| {
            Error::new("unmanaged", "Tools installation disappeared after staging.")
        })?;
        if old.version != plan.expected_version
            || self.executable(&old) != plan.expected_path
            || checksum(&plan.expected_path)? != plan.expected_sha256
        {
            return Err(Error::new(
                "state_changed",
                "Tools changed after staging; check for updates again.",
            ));
        }
        let receipt = self.receipt(ids::TOOLS)?;
        if !matches!(
            self.class(
                ids::TOOLS,
                &Registry::load(&self.paths.link),
                receipt.as_ref()
            )?,
            InstallationClass::Managed
                | InstallationClass::LegacyManaged
                | InstallationClass::SelfInstalled
        ) {
            return Err(Error::new(
                "dev_build",
                "Development or unmanaged Tools installations are never replaced.",
            ));
        }
        self.ensure_compatible(ids::TOOLS, &plan.package.release)?;
        check_cancel(cancel)?;
        let request = Request {
            id: ids::TOOLS.into(),
            operation: Operation::Update,
            mode: Some(plan.mode),
            remove_data: false,
            app_closed: true,
            channel: None,
            version: None,
        };
        let mut replaced = false;
        let result = self.install_package(
            &request,
            &plan.package,
            Prior {
                record: Some(&old),
                receipt: receipt.as_ref(),
                peer: None,
            },
            Some(plan.mode),
            cancel,
            progress,
            &mut replaced,
        );
        let history = HistoryEntry {
            time: SystemClock.now(),
            app: ids::TOOLS.into(),
            from: Some(old.version),
            to: Some(plan.package.release.version.clone()),
            result: if result.is_ok() {
                HistoryResult::Success
            } else if replaced
                && checksum(&plan.expected_path).is_ok_and(|hash| hash == plan.expected_sha256)
            {
                HistoryResult::RolledBack
            } else {
                HistoryResult::Failed
            },
            reason: result.as_ref().err().map(ToString::to_string),
        };
        if result.is_err() {
            // Automatic updates leave this release alone from now on (a newer one, or an update by hand, clears the
            // mark): the restarted Tools checks a minute after it starts, so a release that fails to install was
            // downloaded and tried again every minute.
            self.remember_rollback(
                ids::TOOLS,
                Operation::Rollback,
                Some(&plan.package.release.version),
            );
            let _ = platform::spawn_detached(&self.paths, &plan.expected_path, &plan.mode.args());
        }
        if let Err(error) = updates::append_history(&self.paths, &history) {
            if result.is_ok() {
                return Err(error);
            }
            eprintln!("Self-update history: {error}");
        }
        result
    }
}
