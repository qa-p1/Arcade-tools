use super::*;

impl Manager {
    /// User-confirmed (or per-app opted-in) installer/network/IPC/disk work,
    /// on a worker thread. Updates/repairs append their verified history outcome.
    pub fn operate(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        app_id(&request.id)?;
        let _lock = self.lock()?;
        self.operate_locked(request, cancel, progress, SystemClock.now())
    }
    pub(super) fn operate_locked(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        time: u64,
    ) -> Result<String> {
        let old = self.record(&request.id)?;
        let mut history = HistoryEntry {
            time,
            app: request.id.clone(),
            from: old.as_ref().map(|r| r.version.clone()),
            to: None,
            result: HistoryResult::Success,
            reason: None,
        };
        let mut replaced = false;
        let result = self.operate_inner(request, cancel, progress, &mut history, &mut replaced);
        if result.is_ok() {
            if let Err(error) = installation::prune_installers(&self.paths, &request.id) {
                eprintln!("Installer cache: {error}");
            }
            if let Err(error) = self.cleanup_unpacked() {
                eprintln!("Unpacked app copies: {error}");
            }
        }
        if matches!(
            request.operation,
            Operation::Update | Operation::Repair | Operation::Rollback
        ) {
            if result.is_ok() && request.operation == Operation::Rollback {
                history.result = HistoryResult::RolledBack;
                history.reason = Some("User requested rollback.".into());
            }
            if result.is_ok() && request.operation != Operation::Repair {
                self.remember_rollback(&request.id, request.operation, history.from.as_deref());
            }
            if let Err(error) = &result {
                history.result = if replaced {
                    if old.as_ref().is_some_and(|r| {
                        checksum(&self.executable(r)).is_ok_and(|hash| hash == r.executable_sha256)
                    }) && self
                        .record(&request.id)
                        .ok()
                        .flatten()
                        .is_some_and(|r| Some(r.version) == history.from)
                    {
                        HistoryResult::RolledBack
                    } else {
                        HistoryResult::RollbackFailed
                    }
                } else {
                    HistoryResult::Failed
                };
                history.reason = Some(error.to_string());
            }
            // Don't replace an installation error with a log I/O error. Return
            // log failures after success so callers know history was not saved.
            if let Err(error) = updates::append_history(&self.paths, &history) {
                if result.is_ok() {
                    return Err(error);
                }
                eprintln!("Update history: {error}");
            }
        }
        result
    }
    pub(super) fn operate_inner(
        &self,
        request: &Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        history: &mut HistoryEntry,
        replaced: &mut bool,
    ) -> Result<String> {
        let id = &request.id;
        let old = self.record(id)?;
        let registry = Registry::load(&self.paths.link);
        let peer = registry.get(id);
        let receipt = self.receipt(id)?;
        let class = self.class(id, &registry, receipt.as_ref())?;
        if class == InstallationClass::Dev {
            return Err(Error::new(
                "dev_build",
                "Development builds are never modified by Arcade Tools.",
            ));
        }
        if request.operation == Operation::Adopt {
            return self.adopt_inner(request, peer, receipt, false, cancel, progress);
        }
        // Updating an app Tools doesn't manage yet adopts it with the latest
        // release in one step, instead of asking for a separate adoption.
        if request.operation == Operation::Update
            && old.is_none()
            && Os::current() == Os::Linux
            && id != ids::TOOLS
            && matches!(
                class,
                InstallationClass::SelfInstalled | InstallationClass::Manual
            )
        {
            return self.adopt_inner(request, peer, receipt, true, cancel, progress);
        }
        if request.operation == Operation::Rollback {
            return self.rollback_inner(
                request,
                Prior {
                    record: old.as_ref(),
                    receipt: receipt.as_ref(),
                    peer,
                },
                cancel,
                progress,
                history,
                replaced,
            );
        }
        if request.operation != Operation::Install && old.is_none() {
            return Err(Error::new(
                "unmanaged",
                "Choose “Let Tools manage it” for this app first.",
            ));
        }
        if matches!(request.operation, Operation::Update | Operation::Repair)
            && !matches!(
                class,
                InstallationClass::Managed | InstallationClass::LegacyManaged
            )
        {
            return Err(Error::new(
                "unmanaged",
                "Choose “Let Tools manage it” for this app first.",
            ));
        }
        if request.operation == Operation::Install && (old.is_some() || peer.is_some()) {
            return Err(Error::new(
                "already_installed",
                "This app is already installed.",
            ));
        }
        if request.operation == Operation::Uninstall {
            check_cancel(cancel)?;
            progress(Progress::new(id, "Closing app", None, false));
            self.stop_peer(id, peer, request, false)?;
            progress(Progress::new(id, "Removing app", None, false));
            return self.uninstall(old.as_ref().unwrap(), request.remove_data);
        }
        progress(Progress::new(id, "Checking release", None, true));
        check_cancel(cancel)?;
        let checked = self.checked(id)?;
        history.to = Some(checked.release.version.clone());
        if request
            .channel
            .is_some_and(|channel| channel != checked.release.channel)
        {
            return Err(Error::new(
                "state_changed",
                "The app's release channel changed after you reviewed this; review it again.",
            ));
        }
        if request
            .version
            .as_ref()
            .is_some_and(|version| *version != checked.release.version)
        {
            return Err(Error::new(
                "state_changed",
                format!(
                    "{} is now the latest release; review it again.",
                    checked.release.version
                ),
            ));
        }
        let migration = old
            .as_ref()
            .is_some_and(|r| r.kind == Kind::Tarball && checked.asset.kind == Kind::Appimage);
        if old.as_ref().is_some_and(|r| r.kind != checked.asset.kind) && !migration {
            return Err(Error::new("installer_changed", "This release changes installer format. Uninstall while keeping data, then install the new channel."));
        }
        if request.operation == Operation::Update
            && old.as_ref().is_some_and(|r| !r.update_available(&checked))
        {
            return Ok("Already up to date.".into());
        }
        let destination =
            self.paths
                .executable(id, Os::current(), checked.asset.kind == Kind::Tarball);
        if (old.is_none() || migration) && destination.exists() {
            return Err(Error::new(
                "unmanaged",
                "The install location already contains a file. Adopt it before installing.",
            ));
        }
        // Refuse a busy or unreachable app before the download, not after it.
        self.precheck_peer(id, peer, request)?;
        let package = self.download_package(id, &checked, cancel, progress)?;
        check_cancel(cancel)?;
        // Leaving Nightly may mean an older build reads data a newer one migrated: back it up first (FS-2).
        let backup = match old.as_ref() {
            Some(record)
                if record.channel == Channel::Nightly
                    && checked.release.channel == Channel::Stable =>
            {
                progress(Progress::new(id, "Backing up app data", None, false));
                crate::channel_backup::back_up(&self.paths, id, &record.version)?
            }
            _ => None,
        };
        let done = self.apply_package(
            request,
            &package,
            Prior {
                record: old.as_ref(),
                receipt: receipt.as_ref(),
                peer,
            },
            cancel,
            progress,
            replaced,
        )?;
        Ok(match backup {
            Some(path) => format!(
                "{done} Your data from the nightly build was backed up to {}.",
                path.display()
            ),
            None => done,
        })
    }

    /// The app's retained installers and installed file, as chunk sources for a delta download (D5). Worker thread.
    pub(super) fn local_chunks(&self, id: &str) -> arcade_link::release::ChunkIndex {
        let record = self.record(id).ok().flatten();
        let installed = record
            .as_ref()
            .and_then(|r| Some((r.executable.as_deref()?, r.executable_sha256.as_str())));
        crate::delta::local_chunks(&self.paths, id, installed)
    }
    /// Download and verify a release, keeping the package for rollback.
    pub(super) fn download_package(
        &self,
        id: &str,
        checked: &CheckedRelease,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Package> {
        progress(Progress::new(id, "Downloading", Some(0.0), true));
        self.paths.guard(&self.paths.downloads())?;
        let downloaded = self.source.download_from(
            checked,
            &self.paths.downloads(),
            &self.local_chunks(id),
            cancel,
            &mut |count, total| {
                progress(Progress::new(
                    id,
                    "Downloading",
                    total.filter(|n| *n > 0).map(|n| count as f64 / n as f64),
                    true,
                ))
            },
        )?;
        let package = installation::retain(&self.paths, checked, downloaded.path())?;
        progress(Progress::new(id, "Verified SHA-256", Some(1.0), true));
        Ok(package)
    }
    /// Whether the running app's copy can install itself (`--install`): it
    /// has an install receipt, or its Link manifest says how it installs.
    /// Copies from before the Arcade installer have neither.
    pub(super) fn installs_itself(&self, id: &str, receipt: Option<&Receipt>) -> bool {
        receipt.is_some()
            || fs::read(self.paths.link.registry.join(format!("{id}.json")))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|document| document.get("install").is_some_and(|v| !v.is_null()))
    }

    pub(super) fn apply_package(
        &self,
        request: &Request,
        package: &Package,
        prior: Prior<'_>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        replaced: &mut bool,
    ) -> Result<String> {
        package.verify(&self.paths, &request.id)?;
        self.ensure_compatible(&request.id, &package.release)?;
        let resume = self.stop_peer(&request.id, prior.peer, request, true)?;
        if let Err(error) = check_cancel(cancel) {
            if let Some(mode) = resume {
                let _ = self.launch_inner(&request.id, mode);
            }
            return Err(error);
        }
        let result =
            self.install_package(request, package, prior, resume, cancel, progress, replaced);
        if result.is_err() {
            if let Some(mode) = resume {
                let _ = self.launch_inner(&request.id, mode);
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_package(
        &self,
        request: &Request,
        package: &Package,
        prior: Prior<'_>,
        resume: Option<LaunchMode>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        replaced: &mut bool,
    ) -> Result<String> {
        let old = prior.record;
        let receipt = prior.receipt;
        let id = &request.id;
        let native_root = if matches!(Os::current(), Os::Windows | Os::Macos) {
            platform::native_install_root(receipt)
        } else {
            None
        };
        let (_, expected) = platform::native_install_paths(
            &self.paths,
            id,
            Os::current(),
            package.asset.kind == Kind::Tarball,
            native_root.as_deref(),
        );
        let prospective = Record {
            id: id.clone(),
            version: package.release.version.clone(),
            channel: package.release.channel,
            kind: package.asset.kind,
            sha256: package.asset.sha256.clone(),
            executable_sha256: String::new(),
            last_mode: resume.or(request.mode).or(old.and_then(|r| r.last_mode)),
            executable: Some(expected.clone()),
            package: Some(package.clone()),
            installed_by_app: false,
        };
        let old_target = old
            .map(|record| installation::target(&self.paths, record, receipt))
            .unwrap_or_else(|| installation::target(&self.paths, &prospective, None));
        let mut transaction = Transaction::begin(&self.paths, id, &old_target, receipt)?;
        transaction.include(&installation::target(&self.paths, &prospective, None))?;
        let previous = old.map(|record| transaction.previous(record.clone(), receipt.cloned()));
        let previous_pointer = previous
            .as_ref()
            .and_then(|previous| previous.pointer(&self.paths));
        // Snapshotting a large app takes a while and is still cancellable:
        // nothing has been changed yet.
        if let Err(error) = check_cancel(cancel) {
            transaction.discard();
            return Err(error);
        }
        let login = platform::autostart_enabled(&self.paths, id);
        progress(Progress::new(id, "Installing", None, false));
        *replaced = true;
        let install_args = installation::install_args(&package.asset, package.release.channel)?;
        if let Some(old) = old {
            if receipt.is_none() {
                // Bootstrap ownership for an old Tools install so Link's
                // installer can replace its existing stable AppImage safely.
                Store::new(&self.paths.link)
                    .write(&installation::legacy_receipt(&self.paths, old))?;
            }
        }
        let mut replacement = None;
        let executable = if package.asset.kind == Kind::Appimage && install_args.is_some() {
            installation::run_app_installer(
                &self.paths,
                &package.path,
                install_args.clone().unwrap(),
            )?;
            let installed = self.receipt(id)?.ok_or_else(|| {
                Error::new("receipt", "The app installer did not write its receipt.")
            })?;
            installation::validate_receipt(
                &self.paths,
                &installed,
                id,
                &package.release.version,
                package.release.channel,
            )?;
            let executable = installation::executable(&self.paths, &installed);
            if checksum(&executable)? != package.asset.sha256 {
                return Err(Error::new(
                    "checksum",
                    "The app did not install the verified AppImage bytes.",
                ));
            }
            executable
        } else {
            // Transition release only for Linux/macOS manifests without installArgs.
            // Windows always runs its verified native installer (old and new releases).
            let (executable, rollback) = platform::install_with_options(
                &self.paths,
                id,
                &package.asset,
                &package.path,
                package.release.channel,
                login,
                native_root.as_deref(),
            )?;
            replacement = Some(rollback);
            if let Some(args) = &install_args {
                installation::run_app_installer(&self.paths, &executable, args.clone())?;
            }
            executable
        };
        let app_receipt = self.receipt(id)?;
        let app_written = app_receipt.as_ref().is_some_and(|r| {
            crate::release::versions_match(
                &r.version,
                &package.release.version,
                package.release.channel,
            ) && r.channel == package.release.channel.stored().as_str()
        });
        if install_args.is_some() && !app_written {
            return Err(Error::new(
                "receipt",
                "The app installer did not write a matching receipt.",
            ));
        }
        let mut record = prospective;
        record.executable = Some(executable.clone());
        record.executable_sha256 = checksum(&executable)?;
        record.installed_by_app =
            app_written && (install_args.is_some() || Os::current() == Os::Windows);
        if login && !record.installed_by_app {
            platform::set_autostart(&self.paths, id, &executable, true)?;
        }
        let mut installed_receipt = if app_written {
            app_receipt.unwrap()
        } else {
            installation::legacy_receipt(&self.paths, &record)
        };
        installation::validate_receipt(
            &self.paths,
            &installed_receipt,
            id,
            &record.version,
            record.channel,
        )?;
        if let Some(old) = receipt {
            installed_receipt.installed_at = old.installed_at.clone();
        }
        self.write(&self.paths.state().join(format!("{id}.json")), &record)?;
        // Launch before claiming: the app rewrites its own manifest/receipt. It
        // remains the only writer of capabilities, including when Link is off.
        let mode = resume.or(if request.operation == Operation::Install {
            Some(request.mode.unwrap_or(LaunchMode::Foreground))
        } else {
            None
        });
        if let Some(mode) = mode {
            progress(Progress::new(id, "Launching", None, false));
            platform::spawn_detached(&self.paths, &executable, &mode.args())?;
        }
        // Keep the app's receipt-addressed previous when it created one; shared
        // installers must retain ownership of their stable .previous filename.
        // The supplemental Tools snapshot additionally saves integration/state.
        let previous_pointer = if record.installed_by_app && Os::current() == Os::Linux {
            installed_receipt.previous.clone().or(previous_pointer)
        } else {
            previous_pointer
        };
        installation::claim(&self.paths, installed_receipt, previous_pointer)?;
        if old.is_some_and(|old| old.kind == Kind::Tarball && package.asset.kind == Kind::Appimage)
        {
            self.paths.guard(&old_target)?;
            platform::remove(&old_target)?;
        }
        transaction.commit(previous)?;
        if let Some(replacement) = replacement {
            replacement.commit();
        }
        *replaced = false;
        progress(Progress::new(id, "Done", Some(1.0), false));
        Ok(format!(
            "{} {} installed.",
            crate::apps::app_name(id),
            package.release.version
        ))
    }

    pub(super) fn ensure_compatible(
        &self,
        id: &str,
        release: &crate::release::Release,
    ) -> Result<()> {
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        let peers = self.update_inputs()?.1;
        if let Some(reason) = updates::held_back(&app, release, &peers) {
            return Err(Error::new("version_mismatch", reason));
        }
        Ok(())
    }

    /// `upgrade` (an update of an unmanaged app) installs the latest release
    /// while adopting; so does adopting a copy that can't install itself.
    pub(super) fn adopt_inner(
        &self,
        request: &Request,
        peer: Option<&Manifest>,
        old_receipt: Option<Receipt>,
        upgrade: bool,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String> {
        let receipt_peer = old_receipt.as_ref().map(|receipt| {
            Manifest::new(
                &request.id,
                &receipt.version,
                &installation::executable(&self.paths, receipt).to_string_lossy(),
            )
        });
        let peer = peer.or(receipt_peer.as_ref()).ok_or_else(|| {
            Error::new(
                "not_installed",
                "There is no installation receipt or registry entry to adopt.",
            )
        })?;
        if old_receipt
            .as_ref()
            .is_some_and(|r| r.managed_by == ManagedBy::Tools)
        {
            return Ok("Already managed by Arcade Tools.".into());
        }
        let catalog = self.catalog()?;
        let app = catalog
            .install_entry(&request.id)
            .ok_or_else(|| Error::new("unavailable", "This app is absent from the catalog."))?;
        if let Some(reason) = app.install_reason {
            return Err(Error::new("version_mismatch", reason));
        }
        check_cancel(cancel)?;
        let source = PathBuf::from(&peer.executable);
        if arcade_link::install::is_dev_build(&source) {
            return Err(Error::new(
                "dev_build",
                "Development builds are never adopted.",
            ));
        }
        // A copy from before the Arcade installer ignores --install: adopt it
        // by installing the current release over it instead. Downloaded before
        // the app is closed, so a failed download leaves it running.
        let migration = if Os::current() == Os::Linux
            && (upgrade || !self.installs_itself(&request.id, old_receipt.as_ref()))
        {
            let checked = self.checked(&request.id)?;
            if installation::install_args(&checked.asset, checked.release.channel)?.is_none() {
                return Err(Error::new(
                    "installer",
                    "This app's latest release can't install itself; install it from its release page.",
                ));
            }
            self.precheck_peer(&request.id, Some(peer), request)?;
            Some(self.download_package(&request.id, &checked, cancel, progress)?)
        } else {
            None
        };
        let resume = self.stop_peer(&request.id, Some(peer), request, true)?;
        let destination = self.paths.executable(&request.id, Os::current(), false);
        let target = if Os::current() == Os::Macos {
            self.paths.install_root(&request.id, Os::Macos, false)
        } else {
            destination.clone()
        };
        let transaction =
            match Transaction::begin(&self.paths, &request.id, &target, old_receipt.as_ref())
                .and_then(|transaction| check_cancel(cancel).map(|()| transaction))
            {
                Ok(transaction) => transaction,
                Err(error) => {
                    if let Some(mode) = resume {
                        let _ = self.launch_inner(&request.id, mode);
                    }
                    return Err(error);
                }
            };
        progress(Progress::new(
            &request.id,
            "Adopting installation",
            None,
            false,
        ));
        let result = (|| {
            let receipt = if Os::current() == Os::Linux {
                let channel = old_receipt
                    .as_ref()
                    .map(receipt_channel)
                    .transpose()?
                    .unwrap_or_default();
                if old_receipt.is_none() && source == destination {
                    // The user explicitly chose ownership of this existing file.
                    let now = manifest::now_rfc3339();
                    let receipt = Receipt {
                        schema: 1,
                        id: request.id.clone(),
                        version: peer.version.clone(),
                        channel: channel.as_str().into(),
                        method: InstallMethod::Appimage,
                        managed_by: ManagedBy::SelfManaged,
                        path: source.clone(),
                        integration: Default::default(),
                        previous: None,
                        installed_at: now.clone(),
                        updated_at: now,
                    };
                    Store::new(&self.paths.link).write(&receipt)?;
                }
                // Stage an explicit adoption too: self installers may remove
                // their source; the user's current manual file stays untouched.
                let (installer, channel) = match &migration {
                    Some(package) => (package.path.clone(), package.release.channel),
                    None => (source.clone(), channel),
                };
                installation::run_app_installer(
                    &self.paths,
                    &installer,
                    vec![
                        "--install".into(),
                        "--silent".into(),
                        "--channel".into(),
                        // stable or nightly only: Test is never stored (DECISIONS.md D5).
                        channel.stored().as_str().into(),
                        "--no-launch".into(),
                    ],
                )?;
                self.receipt(&request.id)?.ok_or_else(|| Error::new("receipt", "The app did not write an install receipt; use a release supporting --install."))?
            } else if let Some(receipt) = old_receipt.clone() {
                receipt
            } else {
                // Older native installs may predate receipts. Adoption explicitly
                // records the selected per-user file/bundle and its uninstaller.
                let record = Record {
                    id: request.id.clone(),
                    version: peer.version.clone(),
                    channel: Channel::Stable,
                    kind: if Os::current() == Os::Windows {
                        windows_kind(
                            source
                                .parent()
                                .map(|p| p.join("unins000.exe"))
                                .filter(|p| p.is_file())
                                .as_deref(),
                        )
                    } else {
                        Kind::Dmg
                    },
                    sha256: String::new(),
                    executable_sha256: checksum(&source)?,
                    last_mode: resume,
                    executable: Some(source.clone()),
                    package: None,
                    installed_by_app: true,
                };
                let mut receipt = installation::legacy_receipt(&self.paths, &record);
                if Os::current() == Os::Macos {
                    receipt.path = arcade_link::install::platform::macos::bundle_for(&source)
                        .ok_or_else(|| {
                            Error::new(
                                "receipt",
                                "The registry executable is not in an app bundle.",
                            )
                        })?;
                } else {
                    let root = source.parent().unwrap();
                    receipt.integration.uninstaller =
                        platform::windows_uninstall(record.kind, root)
                            .ok()
                            .map(|p| p.program)
                            .filter(|p| p.is_file());
                    if receipt.integration.uninstaller.is_none() {
                        return Err(Error::new("receipt", "The native uninstaller is missing; reinstall the app before adopting it."));
                    }
                }
                receipt
            };
            let channel = receipt_channel(&receipt)?;
            installation::validate_receipt(
                &self.paths,
                &receipt,
                &request.id,
                migration
                    .as_ref()
                    .map_or(&peer.version, |package| &package.release.version),
                channel,
            )?;
            let mut record = self.record_from_receipt(&receipt)?;
            record.last_mode = resume.or(request.mode);
            if let Some(package) = &migration {
                record.package = Some(package.clone());
                record.sha256 = package.asset.sha256.clone();
            }
            if record.kind == Kind::Appimage && migration.is_none() {
                record.sha256 = record.executable_sha256.clone();
            }
            self.write(
                &self.paths.state().join(format!("{}.json", request.id)),
                &record,
            )?;
            let exe = self.executable(&record);
            if let Some(mode) = resume {
                platform::spawn_detached(&self.paths, &exe, &mode.args())?;
            }
            installation::claim(&self.paths, receipt.clone(), receipt.previous.clone())?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(transaction);
            if let Some(mode) = resume {
                let _ = platform::spawn_detached(&self.paths, &source, &mode.args());
            }
            return Err(error);
        }
        transaction.commit(None)?;
        progress(Progress::new(&request.id, "Done", Some(1.0), false));
        Ok(match &migration {
            Some(package) => format!(
                "{} {} is installed and managed by Arcade Tools.",
                peer.name, package.release.version
            ),
            None => format!("{} is now managed by Arcade Tools.", peer.name),
        })
    }

    pub(super) fn rollback_inner(
        &self,
        request: &Request,
        prior: Prior<'_>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        history: &mut HistoryEntry,
        replaced: &mut bool,
    ) -> Result<String> {
        let receipt = prior.receipt;
        let peer = prior.peer;
        let old = prior.record.ok_or_else(|| {
            Error::new(
                "unmanaged",
                "There is no managed installation to roll back.",
            )
        })?;
        if receipt.is_some_and(|r| r.managed_by != ManagedBy::Tools) && request.id != ids::TOOLS {
            return Err(Error::new(
                "unmanaged",
                "Choose “Let Tools manage it” for this app first.",
            ));
        }
        let previous = PreviousInstallation::read(&self.paths, &request.id)?
            .ok_or_else(|| Error::new("no_previous", "There is no retained previous version."))?;
        history.to = Some(previous.record.version.clone());
        check_cancel(cancel)?;
        if let Some(package) = &previous.record.package {
            self.apply_package(request, package, prior, cancel, progress, replaced)?;
        } else {
            let pointer = previous.pointer(&self.paths).ok_or_else(|| {
                Error::new("no_previous", "The retained previous file is missing.")
            })?;
            let exe = if previous.record.kind == Kind::Dmg {
                let original =
                    installation::target(&self.paths, &previous.record, previous.receipt.as_ref());
                pointer.path.join(
                    self.executable(&previous.record)
                        .strip_prefix(original)
                        .map_err(|_| Error::new("paths", "Invalid previous bundle layout."))?,
                )
            } else {
                pointer.path.clone()
            };
            if checksum(&exe)? != previous.record.executable_sha256 {
                return Err(Error::new(
                    "checksum",
                    "The previous executable failed verification.",
                ));
            }
            if previous.record.kind == Kind::Appimage && previous.record.installed_by_app {
                let asset = crate::release::Asset {
                    os: Os::Linux,
                    arch: Arch::current()?,
                    kind: Kind::Appimage,
                    file: pointer.path.file_name().unwrap().to_string_lossy().into(),
                    sha256: previous.record.executable_sha256.clone(),
                    size: Some(fs::metadata(&pointer.path)?.len()),
                    silent: None,
                    install_args: Some(vec!["--install".into(), "--silent".into()]),
                };
                let release = crate::release::Release {
                    schema: 1,
                    id: request.id.clone(),
                    version: previous.record.version.clone(),
                    channel: previous.record.channel,
                    link_protocol: previous.protocols(&self.paths),
                    notes: String::new(),
                    assets: vec![asset.clone()],
                };
                let package = Package {
                    release,
                    asset,
                    path: pointer.path,
                    tag: previous.record.package.as_ref().and_then(|p| p.tag.clone()),
                };
                let retained = installation::retain(
                    &self.paths,
                    &CheckedRelease {
                        release: package.release.clone(),
                        asset: package.asset.clone(),
                        url: String::new(),
                        tag: package.tag.clone().unwrap_or_default(),
                        entry: None,
                    },
                    &package.path,
                )?;
                self.apply_package(request, &retained, prior, cancel, progress, replaced)?;
            } else {
                let release = crate::release::Release {
                    schema: 1,
                    id: request.id.clone(),
                    version: previous.record.version.clone(),
                    channel: previous.record.channel,
                    link_protocol: previous.protocols(&self.paths),
                    notes: String::new(),
                    assets: Vec::new(),
                };
                self.ensure_compatible(&request.id, &release)?;
                let resume = self.stop_peer(&request.id, peer, request, true)?;
                let result = (|| -> Result<()> {
                    let mut transaction = Transaction::begin(
                        &self.paths,
                        &request.id,
                        &installation::target(&self.paths, old, receipt),
                        receipt,
                    )?;
                    transaction.include(&installation::target(
                        &self.paths,
                        &previous.record,
                        previous.receipt.as_ref(),
                    ))?;
                    let current = transaction.previous(old.clone(), receipt.cloned());
                    if let Err(error) = check_cancel(cancel) {
                        transaction.discard();
                        return Err(error);
                    }
                    *replaced = true;
                    previous.restore(&self.paths)?;
                    let mut record = previous.record.clone();
                    record.last_mode = resume.or(request.mode);
                    let exe = self.executable(&record);
                    // The snapshot includes the receipt and integration. An
                    // adopted older bundle may not support --install; invoking
                    // it again can also move a restored /Applications bundle.
                    let restored = self
                        .receipt(&request.id)?
                        .unwrap_or_else(|| installation::legacy_receipt(&self.paths, &record));
                    self.write(
                        &self.paths.state().join(format!("{}.json", request.id)),
                        &record,
                    )?;
                    if let Some(mode) = resume {
                        platform::spawn_detached(&self.paths, &exe, &mode.args())?;
                    }
                    installation::claim(&self.paths, restored, current.pointer(&self.paths))?;
                    transaction.commit(Some(current))?;
                    *replaced = false;
                    Ok(())
                })();
                if let Err(error) = result {
                    if let Some(mode) = resume {
                        let _ = self.launch_inner(&request.id, mode);
                    }
                    return Err(error);
                }
            }
        }
        progress(Progress::new(&request.id, "Rolled back", Some(1.0), false));
        Ok(format!(
            "{} rolled back to {}.",
            crate::apps::app_name(&request.id),
            previous.record.version
        ))
    }
    /// The refusals [`Self::stop_peer`] would make, without stopping anything.
    /// It runs again after the download, since the app may get busy meanwhile.
    pub(super) fn precheck_peer(
        &self,
        id: &str,
        peer: Option<&Manifest>,
        request: &Request,
    ) -> Result<()> {
        if peer.is_some_and(|m| !m.settings.link_enabled) && !request.app_closed {
            return Err(Error::new("disconnected", "Arcade Link is disabled in this app. Close the app and confirm it is closed, or enable Arcade Link and retry."));
        }
        let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) else {
            return Ok(());
        };
        let Ok(status) = client.status() else {
            return Ok(());
        };
        if status.get("busy").and_then(Value::as_bool) == Some(true)
            || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true)
        {
            return Err(Error::new(
                "busy",
                arcade_link::LinkError::busy().user_message(crate::apps::app_name(id)),
            ));
        }
        Ok(())
    }
    pub(super) fn stop_peer(
        &self,
        id: &str,
        peer: Option<&Manifest>,
        request: &Request,
        restore_mode: bool,
    ) -> Result<Option<LaunchMode>> {
        if peer.is_some_and(|m| !m.settings.link_enabled) && !request.app_closed {
            return Err(Error::new("disconnected", "Arcade Link is disabled in this app. Close the app and confirm it is closed, or enable Arcade Link and retry."));
        }
        let mut client = match Client::connect(&self.paths.link, id, &me()) {
            Ok(c) => c,
            Err(e) if e.code == arcade_link::ErrorCode::NotRunning => {
                #[cfg(target_os = "linux")]
                if arcade_link::endpoint::read(&self.paths.link, id)
                    .is_ok_and(|ep| std::path::Path::new(&format!("/proc/{}", ep.pid)).exists())
                    && !request.app_closed
                {
                    return Err(Error::new("unavailable", "The app has a live process but its Link endpoint is unavailable. Close it before retrying."));
                }
                return Ok(None);
            }
            Err(e) => return Err(link_error(id, e)),
        };
        let status = client.status().map_err(|e| link_error(id, e))?;
        if status.get("busy").and_then(Value::as_bool) == Some(true)
            || status.pointer("/status/busy").and_then(Value::as_bool) == Some(true)
        {
            return Err(Error::new(
                "busy",
                arcade_link::LinkError::busy().user_message(crate::apps::app_name(id)),
            ));
        }
        let mode = status_mode(&status).or(request.mode);
        if restore_mode && mode.is_none() {
            return Err(Error::new("mode_required", "This app does not report its window mode. Choose how to reopen it before updating."));
        }
        client
            .call("app.quit", json!({}))
            .map_err(|e| link_error(id, e))?;
        drop(client);
        let pid = status
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok());
        // Bounded checks only during a user-requested operation, never while idle.
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if Client::connect(&self.paths.link, id, &me()).is_err()
                && !pid.is_some_and(|pid| pid != std::process::id() && process_alive(pid))
            {
                return Ok(mode);
            }
            if Instant::now() > deadline {
                return Err(Error::new(
                    "timeout",
                    "The app did not close. Its installation was left unchanged.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
