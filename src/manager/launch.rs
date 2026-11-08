use super::*;

impl Manager {
    /// IPC/process/disk work on a worker thread. Starts no update timer.
    pub fn launch(&self, id: &str, mode: LaunchMode) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        self.launch_inner(id, mode)
    }
    pub(super) fn launch_inner(&self, id: &str, mode: LaunchMode) -> Result<()> {
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            if mode == LaunchMode::Foreground {
                client
                    .call("app.activate", json!({}))
                    .map_err(|e| link_error(id, e))?;
            }
            return Ok(());
        }
        let mut record = self.record(id)?;
        let manifest = Registry::load(&self.paths.link).get(id).cloned();
        let exe = record
            .as_ref()
            .map(|r| self.executable(r))
            .or_else(|| manifest.map(|m| PathBuf::from(m.executable)))
            .ok_or_else(|| Error::new("not_installed", "Install this app first."))?;
        if !exe.is_file() {
            return Err(Error::new(
                "not_installed",
                "The installed executable is missing. Choose Repair.",
            ));
        }
        platform::spawn_detached(&self.paths, &exe, &mode.args())?;
        if let Some(r) = record.as_mut() {
            r.last_mode = Some(mode);
            self.write(&self.paths.state().join(format!("{id}.json")), r)?;
        }
        Ok(())
    }
    /// Read, back up, write and validate a generated login entry on a worker.
    pub fn start_at_login(&self, id: &str, enabled: bool) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        let registry = Registry::load(&self.paths.link);
        if self.class(id, &registry, self.receipt(id)?.as_ref())? == InstallationClass::Dev {
            return Err(Error::new(
                "dev_build",
                "Development builds keep their own integration.",
            ));
        }
        let exe = self
            .record(id)?
            .as_ref()
            .map(|r| self.executable(r))
            .or_else(|| {
                Registry::load(&self.paths.link)
                    .get(id)
                    .map(|m| PathBuf::from(&m.executable))
            })
            .ok_or_else(|| Error::new("not_installed", "Install this app first."))?;
        platform::set_autostart(&self.paths, id, &exe, enabled)
    }
    /// Open app-owned settings using a feature-detected Link helper, or the
    /// standard --settings CLI when Link is off/unsupported, on a worker.
    pub fn open_settings(&self, id: &str) -> Result<()> {
        manager_id(id)?;
        let _lock = self.lock()?;
        self.settings_inner(id)
    }
    pub(super) fn settings_inner(&self, id: &str) -> Result<()> {
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            let description = client
                .describe_full()
                .map_err(|error| link_error(id, error))?;
            if description.supports("app.settings") {
                match client.settings() {
                    Ok(()) => return Ok(()),
                    Err(error) if error.is_unsupported() => {}
                    Err(error) => return Err(link_error(id, error)),
                }
            }
        }
        let registry = Registry::load(&self.paths.link);
        let exe = self
            .record(id)?
            .as_ref()
            .map(|record| self.executable(record))
            .or_else(|| registry.get(id).map(|peer| PathBuf::from(&peer.executable)))
            .filter(|path| path.is_file())
            .ok_or_else(|| Error::new("not_installed", "The app's executable is missing."))?;
        platform::spawn_detached(&self.paths, &exe, &["--settings".into()])
    }
    /// Request app-owned restart with feature detection on a worker. Old servers
    /// use the same busy-safe quit and launch fallback, preserving window mode.
    pub fn restart(&self, id: &str, mode: Option<LaunchMode>) -> Result<()> {
        app_id(id)?;
        let _lock = self.lock()?;
        if let Ok(mut client) = Client::connect(&self.paths.link, id, &me()) {
            if mode.is_none()
                && client
                    .describe_full()
                    .map_err(|error| link_error(id, error))?
                    .supports("app.restart")
            {
                match client.restart(None) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.is_unsupported() => {}
                    Err(error) => return Err(link_error(id, error)),
                }
            }
        }
        let registry = Registry::load(&self.paths.link);
        let request = Request {
            id: id.into(),
            operation: Operation::Update,
            mode,
            remove_data: false,
            app_closed: false,
            channel: None,
            version: None,
        };
        let resume = self.stop_peer(id, registry.get(id), &request, true)?;
        self.launch_inner(id, resume.or(mode).unwrap_or(LaunchMode::Foreground))
    }
}
