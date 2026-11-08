use super::*;

impl Manager {
    /// Read persisted preferences on a worker thread; no network work.
    pub fn preferences(&self) -> Result<Preferences> {
        let path = self.paths.state().join("preferences.json");
        let mut prefs: Preferences = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
            Err(e) => return Err(e.into()),
        };
        prefs.schema = prefs.schema.max(PREFERENCES_SCHEMA);
        Ok(prefs)
    }
    pub(super) fn save_preferences(&self, prefs: &Preferences) -> Result<()> {
        // A restored 0.x backup carries no schema; what is written always does.
        let prefs = Preferences {
            schema: prefs.schema.max(PREFERENCES_SCHEMA),
            ..prefs.clone()
        };
        self.write(&self.paths.state().join("preferences.json"), &prefs)
    }
    pub fn validate_preferences(&self, prefs: &Preferences) -> Result<()> {
        if !matches!(prefs.theme.as_str(), "system" | "light" | "dark") {
            return Err(Error::new("preferences", "Unknown theme."));
        }
        for id in prefs
            .channels
            .keys()
            .chain(prefs.updates.auto_update.keys())
        {
            manager_id(id)?;
        }
        Ok(())
    }
    pub fn restore_preferences(&self, prefs: &Preferences) -> Result<()> {
        let _lock = self.lock()?;
        self.validate_preferences(prefs)?;
        self.save_preferences(prefs)
    }
    pub fn set_notifications(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.notifications_enabled = enabled;
        self.save_preferences(&prefs)
    }
    pub fn set_theme(&self, theme: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.theme = theme.into();
        self.validate_preferences(&prefs)?;
        self.save_preferences(&prefs)
    }
    pub fn complete_onboarding(&self) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.onboarding_complete = true;
        self.save_preferences(&prefs)
    }
    /// Persist a channel selection on a worker thread; no network work.
    pub fn set_channel(&self, id: &str, channel: Channel) -> Result<()> {
        manager_id(id)?;
        if channel == Channel::Test {
            return Err(Error::new(
                "invalid",
                "The test channel is only for release testing (ARCADE_TOOLS_CHANNEL_OVERRIDE=test).",
            ));
        }
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.channels.insert(id.into(), channel);
        self.save_preferences(&prefs)
    }
    /// Persist Link enablement on a worker thread; no IPC/network work.
    pub fn set_link(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.link_enabled = enabled;
        self.save_preferences(&prefs)
    }
    /// Persist scheduler opt-in on a worker thread. No network work is started.
    pub fn set_updates_enabled(&self, enabled: bool) -> Result<()> {
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.updates.enabled = enabled;
        self.save_preferences(&prefs)
    }
    /// Persist per-app auto-update opt-in on a worker thread. No installation.
    pub fn set_auto_update(&self, id: &str, enabled: bool) -> Result<()> {
        manager_id(id)?;
        let _lock = self.lock()?;
        let mut prefs = self.preferences()?;
        prefs.updates.auto_update.insert(id.into(), enabled);
        self.save_preferences(&prefs)
    }
}
