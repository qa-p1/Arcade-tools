use super::*;

impl Manager {
    /// Execute an opted-in update on a worker. Disabled checks, unknown use,
    /// recent use, disconnected/busy apps and compatibility holds return a
    /// decision without modifying the installation. No timer is started here.
    /// The caller supplies its latest last-used timestamp (Unix seconds).
    pub fn auto_update(
        &self,
        id: &str,
        last_used: Option<u64>,
        clock: &dyn Clock,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<updates::AutoUpdateOutcome> {
        app_id(id)?;
        let _lock = self.lock()?;
        let mut decision = self.auto_eligibility(id, last_used, clock.now())?;
        if decision.eligible {
            if let Some(skipped) = self.skipped_after_rollback(id)? {
                decision = updates::AutoUpdateDecision {
                    eligible: false,
                    reason: format!(
                        "You rolled back from {skipped}; auto-update waits for a newer release."
                    ),
                };
            }
        }
        if !decision.eligible {
            return Ok(updates::AutoUpdateOutcome {
                decision,
                message: None,
            });
        }
        let request = Request {
            id: id.into(),
            operation: Operation::Update,
            mode: None,
            remove_data: false,
            app_closed: false,
            channel: None,
            version: None,
        };
        match self.operate_locked(&request, cancel, progress, clock.now()) {
            Ok(message) => Ok(updates::AutoUpdateOutcome {
                decision,
                message: Some(message),
            }),
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "busy"
                        | "disconnected"
                        | "version_mismatch"
                        | "dev_build"
                        | "unmanaged"
                        | "mode_required"
                ) =>
            {
                Ok(updates::AutoUpdateOutcome {
                    decision: updates::AutoUpdateDecision {
                        eligible: false,
                        reason: error.message,
                    },
                    message: None,
                })
            }
            Err(error) => Err(error),
        }
    }
    /// A rollback marks the version it left so auto-update doesn't reinstall
    /// it; a later update (by hand, or to a newer release) clears the mark.
    pub(super) fn remember_rollback(&self, id: &str, operation: Operation, from: Option<&str>) {
        let Ok(mut prefs) = self.preferences() else {
            return;
        };
        let changed = match (operation, from) {
            (Operation::Rollback, Some(from)) => {
                prefs.updates.rolled_back.insert(id.into(), from.into()) != Some(from.into())
            }
            (Operation::Rollback, None) => false,
            _ => prefs.updates.rolled_back.remove(id).is_some(),
        };
        if changed {
            if let Err(error) = self.save_preferences(&prefs) {
                eprintln!("Rollback preference: {error}");
            }
        }
    }
    /// The rolled-back version, when the available update is not newer than it.
    pub(super) fn skipped_after_rollback(&self, id: &str) -> Result<Option<String>> {
        let Some(skipped) = self.preferences()?.updates.rolled_back.get(id).cloned() else {
            return Ok(None);
        };
        let state = self.update_state()?;
        let latest = state.apps.get(id).and_then(|app| {
            app.latest
                .get(app.channel.as_str())
                .and_then(|channel| channel.latest_version.clone())
        });
        let newer = match (
            latest.as_deref().map(semver::Version::parse),
            semver::Version::parse(&skipped),
        ) {
            (Some(Ok(latest)), Ok(skipped)) => latest > skipped,
            (Some(_), _) => latest.as_deref() != Some(skipped.as_str()),
            (None, _) => false,
        };
        Ok((!newer).then_some(skipped))
    }
    pub(super) fn auto_eligibility(
        &self,
        id: &str,
        last_used: Option<u64>,
        now: u64,
    ) -> Result<updates::AutoUpdateDecision> {
        let settings = self.preferences()?.updates;
        let deny = |reason: &str| updates::AutoUpdateDecision {
            eligible: false,
            reason: reason.into(),
        };
        if !settings.enabled {
            return Ok(deny("Update checks are off."));
        }
        if !settings.auto_update.get(id).copied().unwrap_or(false) {
            return Ok(deny("Auto-update is off for this app."));
        }
        let registry = Registry::load(&self.paths.link);
        let receipt = self.receipt(id)?;
        let class = self.class(id, &registry, receipt.as_ref())?;
        let updatable = matches!(
            class,
            InstallationClass::Managed | InstallationClass::LegacyManaged
        ) || (id == ids::TOOLS && class == InstallationClass::SelfInstalled);
        if !updatable {
            return Ok(deny(
                "Only managed, non-development installations can auto-update.",
            ));
        }
        if registry
            .get(id)
            .is_some_and(|peer| !peer.settings.link_enabled)
        {
            return Ok(deny(
                "Arcade Link is disabled; close the app and update manually.",
            ));
        }
        let mut status = match Client::connect(&self.paths.link, id, &me()) {
            Ok(mut client) => client.status().map_err(|error| link_error(id, error))?,
            Err(error) if error.code == arcade_link::ErrorCode::NotRunning => {
                if arcade_link::endpoint::read(&self.paths.link, id)
                    .is_ok_and(|endpoint| process_alive(endpoint.pid))
                {
                    return Ok(deny("A running app has an unavailable Link endpoint."));
                }
                json!({"busy": false})
            }
            Err(error) => return Ok(deny(&link_error(id, error).message)),
        };
        if self.own_process(id) {
            status["busy"] = false.into();
            if let Some(nested) = status.get_mut("status") {
                nested["busy"] = false.into();
            }
        }
        Ok(updates::auto_update_policy(true, &status, last_used, now))
    }

    /// Read current policy/status on a worker, without network or installation.
    pub fn auto_update_eligibility(
        &self,
        id: &str,
        last_used: Option<u64>,
        clock: &dyn Clock,
    ) -> Result<updates::AutoUpdateDecision> {
        manager_id(id)?;
        self.auto_eligibility(id, last_used, clock.now())
    }
    /// Read managed records and installed peers' registry manifests on a worker
    /// thread. Does not probe IPC or connect to the network.
    pub fn update_inputs(&self) -> Result<(Vec<InstalledApp>, Vec<PeerProtocols>)> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let peers = registry
            .apps()
            .iter()
            .filter(|m| m.id != ids::TOOLS)
            .map(|m| PeerProtocols {
                id: m.id.clone(),
                protocols: m.link.protocol.clone(),
            })
            .collect();
        let mut installed = Vec::new();
        let catalog = self.catalog()?;
        let apps = catalog
            .install_entry(ids::TOOLS)
            .into_iter()
            .chain(catalog.apps);
        for app in apps {
            let record = if app.supported_install() {
                self.record(&app.id)?
            } else {
                None
            };
            let peer = registry.get(&app.id);
            if let Some(version) = record
                .as_ref()
                .map(|r| r.version.clone())
                .or_else(|| peer.map(|p| p.version.clone()))
            {
                installed.push(InstalledApp {
                    id: app.id.clone(),
                    version,
                    channel: record.as_ref().map(|r| r.channel).unwrap_or_default(),
                    selected_channel: prefs
                        .channels
                        .get(&app.id)
                        .copied()
                        .or(record.as_ref().map(|r| r.channel))
                        .unwrap_or_default(),
                    sha256: record.map(|r| r.sha256).filter(|hash| !hash.is_empty()),
                });
            }
        }
        Ok((installed, peers))
    }
    /// Read the cached catalog or built-in snapshot on a worker thread.
    /// No network I/O is performed.
    pub fn catalog(&self) -> Result<Catalog> {
        Ok(CatalogClient::new(self.paths.clone(), self.source.clone())?.cached_catalog())
    }
    /// Explicit catalog network refresh/cache write on a worker thread.
    pub fn refresh_catalog(&self) -> Result<Catalog> {
        Ok(CatalogClient::new(self.paths.clone(), self.source.clone())?.refresh_catalog())
    }
    /// [`Self::refresh_catalog`], with why it fell back to the saved copy.
    pub fn refresh_catalog_reporting(&self) -> Result<(Catalog, Option<Error>)> {
        Ok(
            CatalogClient::new(self.paths.clone(), self.source.clone())?
                .refresh_catalog_reporting(),
        )
    }
    /// Read cached engine metadata or explicitly refresh it on a worker thread.
    pub fn engines(&self, refresh: bool) -> Result<EngineCatalog> {
        CatalogClient::new(self.paths.clone(), self.source.clone())?.engines(refresh)
    }
    /// Read persisted update results and reconcile them with the current
    /// registry/catalog snapshot. This does not make network requests.
    pub fn update_state(&self) -> Result<UpdateState> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let mut guard = self.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(updates::Scheduler::new(
                self.paths.clone(),
                SystemClock,
                self.source.clone(),
            )?);
        }
        let scheduler = guard.as_mut().unwrap();
        scheduler.reconcile(&catalog, &installed, &peers);
        Ok(scheduler.state().clone())
    }
    /// Run a user-requested update check through the core scheduler and persist
    /// its state. The caller must keep this synchronous work off the UI thread.
    pub fn check_updates(&self) -> Result<UpdateState> {
        self.check_updates_for(Trigger::Manual)
    }
    /// Run a scheduled or window-open update check through the core scheduler.
    /// The caller must keep this synchronous work off the UI thread.
    pub fn check_updates_for(&self, trigger: Trigger) -> Result<UpdateState> {
        self.check_updates_with(trigger, true)
            .map(|(state, _)| state)
    }
    pub fn check_updates_with(
        &self,
        trigger: Trigger,
        online: bool,
    ) -> Result<(UpdateState, Vec<updates::UpdateNotification>)> {
        let catalog = self.catalog()?;
        let (installed, peers) = self.update_inputs()?;
        let settings = self.preferences()?.updates;
        let mut guard = self.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(updates::Scheduler::new(
                self.paths.clone(),
                SystemClock,
                self.source.clone(),
            )?);
        }
        let scheduler = guard.as_mut().unwrap();
        let outcome = scheduler.run(
            trigger,
            CheckContext {
                settings: &settings,
                catalog: &catalog,
                installed: &installed,
                peers: &peers,
                // A manual request treats connectivity as unknown/online; the
                // first failed transport is recorded by the scheduler.
                online,
                os: Os::current(),
                arch: Arch::current()?,
            },
        )?;
        Ok((scheduler.state().clone(), outcome.notifications))
    }
    pub fn update_deadline(&self) -> Result<Option<u64>> {
        self.update_state()?;
        Ok(self
            .scheduler
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .next_deadline(&self.preferences()?.updates))
    }
    pub fn last_used(&self, id: &str) -> Option<u64> {
        let status = Client::connect_with(&self.paths.link, id, &me(), Duration::from_millis(250))
            .ok()?
            .status()
            .ok()?;
        status
            .pointer("/status/lastUsed")
            .or_else(|| status.pointer("/status/lastUsedAt"))
            .or_else(|| status.get("lastUsed"))
            .or_else(|| status.get("lastUsedAt"))
            .and_then(Value::as_u64)
    }
    /// Fetch and cache one app's docs for the selected installed version. This
    /// method never runs unless Learn explicitly requests the download.
    pub fn fetch_docs(&self, id: &str, version: &str) -> Result<()> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resolver = DocsResolver::new(self.paths.clone(), PathBuf::new());
        resolver.fetch_version(&app, version, &self.source)?;
        Ok(())
    }
    pub fn fetch_latest_docs(&self, id: &str) -> Result<String> {
        let app = self
            .catalog()?
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let resolver = DocsResolver::new(self.paths.clone(), PathBuf::new());
        Ok(resolver.fetch_latest(&app, &self.source)?.version)
    }
    pub(super) fn checked(&self, id: &str) -> Result<CheckedRelease> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        if let Some(reason) = &app.install_reason {
            return Err(Error::new("version_mismatch", reason.clone()));
        }
        let prefs = self.preferences()?;
        let channel = prefs
            .channels
            .get(id)
            .copied()
            .or(self.record(id)?.map(|r| r.channel))
            .unwrap_or_default();
        let path = self
            .paths
            .cache
            .join("arcade/tools/releases")
            .join(id)
            .join(format!("{}.json", channel.as_str()));
        let cache: Option<crate::release::ReleaseCache> =
            crate::catalog::read_bounded(&path, 6 * 1024 * 1024)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        let check = self.source.check_repo_conditional(
            id,
            &app.repo,
            channel,
            Os::current(),
            Arch::current()?,
            cache.as_ref(),
            &app.withdrawn(),
        )?;
        self.write(&path, &check.cache)?;
        check.checked.release.compatible(id, channel)?;
        let peers = Registry::load(&self.paths.link)
            .apps()
            .iter()
            .filter(|m| m.id != ids::TOOLS)
            .map(|m| PeerProtocols {
                id: m.id.clone(),
                protocols: m.link.protocol.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(reason) = updates::held_back(&app, &check.checked.release, &peers) {
            return Err(Error::new("version_mismatch", reason));
        }
        Ok(check.checked)
    }
    /// Explicit conditional release request and disk/registry reads, on a worker.
    pub fn check(&self, id: &str) -> Result<CheckView> {
        let checked = self.checked(id)?;
        let record = self.record(id)?;
        Ok(CheckView {
            id: id.into(),
            version: checked.release.version.clone(),
            channel: checked.release.channel,
            file: checked.asset.file.clone(),
            notes: self
                .release_body(id, checked.release.channel)
                .unwrap_or_else(|| checked.release.notes.clone()),
            update_available: record.is_none_or(|r| r.update_available(&checked)),
        })
    }
    pub(super) fn release_body(&self, id: &str, channel: Channel) -> Option<String> {
        let path = self
            .paths
            .cache
            .join("arcade/tools/releases")
            .join(id)
            .join(format!("{}.json", channel.as_str()));
        let bytes = crate::catalog::read_bounded(&path, 6 * 1024 * 1024).ok()?;
        let cache: crate::release::ReleaseCache = serde_json::from_slice(&bytes).ok()?;
        Some(cache.body)
    }

    pub fn release_history(&self, id: &str) -> Result<serde_json::Value> {
        let entry = self
            .catalog()?
            .install_entry(id)
            .ok_or_else(|| Error::new("unsupported_input", "Unknown catalog app."))?;
        let raw = self.source.release_history(&entry.repo)?;
        let items=raw.as_array().unwrap().iter().filter(|release|release["draft"]!=true).take(20).map(|release| {
            serde_json::json!({"version":release["tag_name"],"publishedAt":release["published_at"],"prerelease":release["prerelease"],"html":crate::markdown::render(release["body"].as_str().unwrap_or_default(),&[])})
        }).collect::<Vec<_>>();
        Ok(serde_json::json!(items))
    }
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        let path = self.paths.state().join("update-history.jsonl");
        let bytes = match crate::catalog::read_bounded(&path, 8 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(_) if !path.exists() => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        String::from_utf8_lossy(&bytes)
            .lines()
            .rev()
            .take(200)
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }
}
