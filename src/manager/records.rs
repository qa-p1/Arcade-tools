use super::*;

impl Manager {
    pub(super) fn cached_checksum(&self, path: &Path) -> Result<String> {
        let before = Fingerprint::of(path)?;
        if let Some((seen, hash)) = self
            .hashes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
        {
            if *seen == before {
                return Ok(hash.clone());
            }
        }
        let hash = checksum(path)?;
        // Only cache when nothing changed while hashing.
        if Fingerprint::of(path).is_ok_and(|after| after == before) {
            self.hashes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(path.to_path_buf(), (before, hash.clone()));
        }
        Ok(hash)
    }
    pub(super) fn write(&self, path: &std::path::Path, value: &impl Serialize) -> Result<()> {
        self.paths.guard(path)?;
        arcade_link::paths::write_atomic(path, &serde_json::to_vec_pretty(value)?, true)?;
        Ok(())
    }
    /// Read an installation record on a worker thread; no network work.
    pub fn record(&self, id: &str) -> Result<Option<Record>> {
        manager_id(id)?;
        let supplemental = match fs::read(self.paths.state().join(format!("{id}.json"))) {
            Ok(bytes) => {
                let record: Record = serde_json::from_slice(&bytes)?;
                if record.id != id {
                    return Err(Error::new(
                        "state",
                        "Installed record has a mismatched app ID.",
                    ));
                }
                Some(record)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(receipt) = self.receipt(id)? {
            if receipt.method == InstallMethod::Dev {
                return Ok(None);
            }
            let executable = installation::executable(&self.paths, &receipt);
            if let Some(mut record) = supplemental {
                // Keep release hashes/rollback metadata when the app rewrites
                // only its build suffix, but never trust changed executable bytes.
                // Build metadata (Wheel's "0.3.1+build.56" release vs its "0.3.1"
                // receipt) reaches the hash on every list/Home refresh, so the
                // hash is cached until the file's size, times or inode change.
                let same_bytes = record.version == receipt.version
                    || self
                        .cached_checksum(&executable)
                        .is_ok_and(|hash| hash == record.executable_sha256);
                if !crate::release::versions_match(
                    &record.version,
                    &receipt.version,
                    record.channel,
                ) || !same_bytes
                    || record.channel.as_str() != receipt.channel
                    || record
                        .executable
                        .as_ref()
                        .is_some_and(|path| path != &executable)
                {
                    record.version = receipt.version.clone();
                    record.channel = receipt_channel(&receipt)?;
                    record.package = None;
                    record.sha256 = String::new();
                    record.executable_sha256 =
                        self.cached_checksum(&executable).unwrap_or_default();
                }
                record.executable = Some(executable);
                return Ok(Some(record));
            }
            return Ok(Some(self.record_from_receipt(&receipt)?));
        }
        // An app that installed itself and then uninstalled itself (its own
        // menu entry or --uninstall) removed its receipt and executable: it is
        // not installed, not "needs repair".
        if supplemental
            .as_ref()
            .is_some_and(|record| self.uninstalled_by_app(record))
        {
            return Ok(None);
        }
        Ok(supplemental)
    }
    pub(super) fn uninstalled_by_app(&self, record: &Record) -> bool {
        record.installed_by_app && !self.executable(record).exists()
    }
    /// Read a validated Link receipt on a worker; no migration or writes.
    pub fn receipt(&self, id: &str) -> Result<Option<Receipt>> {
        manager_id(id)?;
        self.paths.guard_receipt(id)?;
        Ok(Store::new(&self.paths.link).read(id)?)
    }
    pub(super) fn record_from_receipt(&self, receipt: &Receipt) -> Result<Record> {
        let executable = installation::executable(&self.paths, receipt);
        let kind = match receipt.method {
            InstallMethod::Appimage => Kind::Appimage,
            InstallMethod::MacosBundle => Kind::Dmg,
            InstallMethod::Tarball => Kind::Tarball,
            InstallMethod::WindowsInstaller => {
                windows_kind(receipt.integration.uninstaller.as_deref())
            }
            _ => match Os::current() {
                Os::Linux => Kind::Appimage,
                Os::Windows => windows_kind(receipt.integration.uninstaller.as_deref()),
                Os::Macos => Kind::Dmg,
            },
        };
        Ok(Record {
            id: receipt.id.clone(),
            version: receipt.version.clone(),
            channel: receipt_channel(receipt)?,
            kind,
            sha256: String::new(),
            executable_sha256: self.cached_checksum(&executable).unwrap_or_default(),
            last_mode: None,
            executable: Some(executable),
            package: None,
            installed_by_app: true,
        })
    }
    pub(super) fn executable(&self, record: &Record) -> PathBuf {
        record.executable.clone().unwrap_or_else(|| {
            self.paths
                .executable(&record.id, Os::current(), record.kind == Kind::Tarball)
        })
    }
    pub(super) fn class(
        &self,
        id: &str,
        registry: &Registry,
        receipt: Option<&Receipt>,
    ) -> Result<InstallationClass> {
        let dev = registry.additions(id).and_then(|extra| extra.install)
            == Some(InstallMethod::Dev)
            || registry.get(id).is_some_and(|peer| {
                arcade_link::install::is_dev_build(Path::new(&peer.executable))
            })
            || receipt.is_some_and(|receipt| {
                receipt.method == InstallMethod::Dev
                    || arcade_link::install::is_dev_build(&receipt.path)
            });
        if dev {
            return Ok(InstallationClass::Dev);
        }
        Ok(match receipt.map(|r| r.managed_by) {
            Some(ManagedBy::Tools) => InstallationClass::Managed,
            Some(ManagedBy::SelfManaged) => InstallationClass::SelfInstalled,
            None if self.paths.state().join(format!("{id}.json")).is_file()
                && self.record(id)?.is_some() =>
            {
                InstallationClass::LegacyManaged
            }
            None => InstallationClass::Manual,
        })
    }
    /// Classify registry installs for the adoption dialog on a worker. Dev builds
    /// are launchable but never eligible for any installation mutation.
    pub fn adoptions(&self) -> Result<Vec<Adoption>> {
        let registry = Registry::load(&self.paths.link);
        let mut result = Vec::new();
        for id in crate::apps::APPS {
            let peer = registry.get(id);
            let receipt = self.receipt(id)?;
            let Some((executable, version)) = peer
                .map(|peer| (PathBuf::from(&peer.executable), peer.version.clone()))
                .or_else(|| {
                    receipt.as_ref().map(|receipt| {
                        (
                            installation::executable(&self.paths, receipt),
                            receipt.version.clone(),
                        )
                    })
                })
            else {
                continue;
            };
            let class = self.class(id, &registry, receipt.as_ref())?;
            let can_adopt = matches!(
                class,
                InstallationClass::Manual | InstallationClass::SelfInstalled
            );
            result.push(Adoption {
                id: id.to_string(),
                class,
                executable,
                version,
                can_adopt,
                reason: if class == InstallationClass::Dev {
                    Some("Development build; Arcade Tools will never modify it.".into())
                } else {
                    None
                },
            });
        }
        Ok(result)
    }
    /// Read one-step rollback metadata on a worker, without network or mutation.
    pub fn rollback_version(&self, id: &str) -> Result<Option<String>> {
        manager_id(id)?;
        Ok(PreviousInstallation::read(&self.paths, id)?
            .filter(|previous| previous.pointer(&self.paths).is_some())
            .map(|previous| previous.record.version))
    }
    /// Disk/optional IPC work on a worker thread; no network refresh.
    pub fn list(&self, probe: bool) -> Result<Vec<AppView>> {
        self.list_catalog(probe, false)
    }
    pub fn tools_view(&self) -> Result<Option<AppView>> {
        Ok(self
            .list_catalog(false, true)?
            .into_iter()
            .find(|app| app.id == ids::TOOLS))
    }
    pub(super) fn list_catalog(&self, probe: bool, include_tools: bool) -> Result<Vec<AppView>> {
        let registry = Registry::load(&self.paths.link);
        let prefs = self.preferences()?;
        let mut catalog = self.catalog()?;
        if include_tools {
            if let Some(tools) = catalog.install_entry(ids::TOOLS) {
                catalog.apps.push(tools);
            }
        }
        catalog.apps.iter().map(|app| {
            let id = app.id.as_str();
            let record = if app.supported_install() { self.record(id)? } else { None };
            let receipt = if app.supported_install() { self.receipt(id)? } else { None };
            let class = self.class(id, &registry, receipt.as_ref())?;
            let peer = registry.get(id);
            let executable = record.as_ref().map(|r| self.executable(r)).or_else(|| peer.map(|m| PathBuf::from(&m.executable)));
            let running = probe && Client::connect(&self.paths.link, id, &me()).is_ok();
            let healthy = executable.as_ref().is_some_and(|p| p.is_file());
            let reason = if class == InstallationClass::Dev { Some("Development build; launch is available, installation changes are disabled.".into()) }
                else if id != ids::TOOLS && matches!(class, InstallationClass::Manual | InstallationClass::SelfInstalled) && (peer.is_some() || receipt.is_some()) { Some("Installed outside Arcade Tools. Choose “Let Tools manage it” to enable updates.".into()) }
                else if peer.is_some_and(|m| !m.settings.link_enabled) { Some("Arcade Link is disabled in this app. Close it before changing its installation.".into()) }
                else if record.is_some() && !healthy { Some("Installed executable is missing. Choose Repair.".into()) }
                else { record.as_ref().and_then(first_update_rollback_notice).map(str::to_owned) };
            Ok(AppView { id: id.to_string(), name: app.name.clone(), pitch: app.pitch.clone(), tagline: app.display_tagline(), releases_url: app.releases_url(),
                version: record.as_ref().map(|r| r.version.clone()).or_else(|| peer.map(|m| m.version.clone())), managed: matches!(class, InstallationClass::Managed | InstallationClass::LegacyManaged), installed: healthy,
                running, channel: prefs.channels.get(id).copied().or(record.as_ref().map(|r| r.channel)).unwrap_or_default(), healthy,
                start_at_login: platform::autostart_enabled(&self.paths, id), login_supported: app.supported_install() && platform::login_supported(id, Os::current()), executable: executable.map(|p| p.display().to_string()),
                data_folders: self.paths.data_folders(id, Os::current()).iter().map(|p| p.display().to_string()).collect(), reason,
                description: app.description.clone(), category: app.category.clone(), icon: app.icon.clone(),
                platforms: app.platforms.clone(), min_tools: app.min_tools.clone(), install_reason: app.install_reason.clone(),
                install_supported: app.supported_install(), installation_class: class, rollback_version: if app.supported_install() { self.rollback_version(id)? } else { None } })
        }).collect()
    }
}
