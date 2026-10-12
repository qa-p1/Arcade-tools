//! Tools' Link presence: attach tray.host before publishing hosting availability.
use super::settings::Preferences;
use arcade_link::{
    manifest::{self, ManifestDocument, TrayHostSettings},
    server::{Handler, Server, ServerConfig},
    trayhost::TrayHostServer,
    Locations, PeerInfo,
};
use arcade_tools_core::{link, paths::Paths};
use std::sync::Arc;

pub fn document(link_enabled: bool, prefs: &Preferences) -> ManifestDocument {
    let mut doc: ManifestDocument = link::manifest(link_enabled).into();
    doc.additions.tray_host = Some(TrayHostSettings {
        enabled: prefs.host_icons,
        excluded: prefs.excluded.clone(),
    });
    doc
}

/// CLI manifest inspection is read-only and starts no server or GUI.
pub fn manifest_from_disk() -> ManifestDocument {
    let paths = Paths::discover().ok();
    let prefs = paths
        .as_ref()
        .and_then(|p| Preferences::load(p).ok())
        .unwrap_or_default();
    let enabled = paths
        .as_ref()
        .and_then(|p| std::fs::read(p.state().join("preferences.json")).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("linkEnabled").and_then(|v| v.as_bool()))
        .unwrap_or(true);
    document(enabled, &prefs)
}

pub struct Presence {
    locations: Locations,
    handler: Arc<dyn Handler>,
    host: TrayHostServer,
    server: Option<Server>,
    last_error: Option<String>,
    restarting: bool,
}

impl Presence {
    pub fn start(
        locations: Locations,
        host: TrayHostServer,
        handler: Arc<dyn Handler>,
        enabled: bool,
        prefs: &Preferences,
    ) -> Self {
        let mut presence = Self {
            locations,
            handler,
            host,
            server: None,
            last_error: None,
            restarting: false,
        };
        presence.update(enabled, prefs);
        presence
    }

    pub fn update(&mut self, link_enabled: bool, prefs: &Preferences) {
        self.host.set_excluded(&prefs.excluded);
        self.host.set_hosted(link_enabled && prefs.host_icons);
        if link_enabled && self.server.is_none() {
            let config = ServerConfig {
                locations: self.locations.clone(),
                app: PeerInfo {
                    id: "arcade.tools".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            };
            match Server::start_with_tray_host(config, self.handler.clone(), self.host.clone()) {
                Ok(server) => {
                    self.server = Some(server);
                    self.last_error = None;
                }
                Err(e) => {
                    self.host.set_hosted(false);
                    self.last_error = Some(e.to_string());
                }
            }
        } else if !link_enabled {
            self.host.shutdown();
            self.server.take();
        }
        let mut doc = document(link_enabled, prefs);
        if link_enabled && self.server.is_none() {
            doc.additions.tray_host.as_mut().unwrap().enabled = false;
        }
        match manifest::write_document(&self.locations, &doc) {
            Ok(true) => {
                if let Some(server) = &self.server {
                    server.notify_changed();
                }
            }
            Ok(false) => {}
            Err(e) => self.last_error = Some(e.to_string()),
        }
    }

    pub fn announce_restarting(&mut self) {
        self.restarting = true;
        self.host.announce_restarting();
    }

    pub fn diagnostics(&self) -> Vec<(String, String)> {
        vec![
            (
                "Registry".into(),
                self.locations.registry.display().to_string(),
            ),
            (
                "Runtime".into(),
                self.locations.runtime.display().to_string(),
            ),
            (
                "Endpoint".into(),
                if self.server.is_some() {
                    "listening"
                } else {
                    "not listening"
                }
                .into(),
            ),
            (
                "Last error".into(),
                self.last_error.clone().unwrap_or_else(|| "none".into()),
            ),
        ]
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        if !self.restarting {
            self.host.shutdown();
        }
        self.server.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::{
        trayhost::{TrayHostWatcher, TrayState, WatcherConfig},
        Client, InvokeContext, InvokeRequest, LinkError, Reply,
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    struct HandlerMock;
    impl Handler for HandlerMock {
        fn describe(&self) -> Vec<arcade_link::Action> {
            vec![]
        }
        fn invoke(&self, _: InvokeRequest, _: &InvokeContext) -> Result<Reply, LinkError> {
            Err(LinkError::unsupported("test"))
        }
    }

    #[test]
    fn subscriptions_exclusion_restart_and_graceful_quit() {
        let root = tempfile::tempdir().unwrap();
        let loc = Locations::under(root.path());
        let host = TrayHostServer::new(false);
        let mut presence = Presence::start(
            loc.clone(),
            host.clone(),
            Arc::new(HandlerMock),
            true,
            &Preferences::default(),
        );
        let mut client = Client::connect(
            &loc,
            "arcade.tools",
            &PeerInfo {
                id: "arcade.find".into(),
                version: "1".into(),
            },
        )
        .unwrap();
        client.subscribe(&["tray.host"]).unwrap();
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["hosted"],
            true
        );
        host.set_excluded(&["arcade.find".into()]);
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["hosted"],
            false
        );
        host.set_excluded(&[]);
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["hosted"],
            true
        );
        presence.announce_restarting();
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["restarting"],
            true
        );
        presence.restarting = false;
        drop(presence);
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["hosted"],
            false
        );
        assert!(client
            .next_notification(Some(Duration::from_secs(1)))
            .is_err());
    }

    #[test]
    fn watcher_hands_back_on_abrupt_server_stop_within_one_second() {
        let root = tempfile::tempdir().unwrap();
        let loc = Locations::under(root.path());
        let mut presence = Presence::start(
            loc.clone(),
            TrayHostServer::new(false),
            Arc::new(HandlerMock),
            true,
            &Preferences::default(),
        );
        let (tx, rx) = mpsc::channel();
        let _watcher = TrayHostWatcher::start(
            WatcherConfig {
                locations: loc,
                app: PeerInfo {
                    id: "arcade.find".into(),
                    version: "1".into(),
                },
                link_enabled: true,
                has_tray: true,
            },
            move |s| {
                let _ = tx.send(s);
            },
        )
        .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TrayState::Hosted
        );
        let start = Instant::now();
        // Omit hosted:false to simulate a crash, rather than normal Drop.
        presence.server.take();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TrayState::Own
        );
        eprintln!("Tools server stop → own: {:?}", start.elapsed());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn restart_reconnects_without_an_own_transition_and_link_off_hands_back() {
        let root = tempfile::tempdir().unwrap();
        let loc = Locations::under(root.path());
        let prefs = Preferences::default();
        let mut presence = Presence::start(
            loc.clone(),
            TrayHostServer::new(false),
            Arc::new(HandlerMock),
            true,
            &prefs,
        );
        let (tx, rx) = mpsc::channel();
        let watcher = TrayHostWatcher::start(
            WatcherConfig {
                locations: loc.clone(),
                app: PeerInfo {
                    id: "arcade.find".into(),
                    version: "1".into(),
                },
                link_enabled: true,
                has_tray: true,
            },
            move |state| {
                let _ = tx.send(state);
            },
        )
        .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TrayState::Hosted
        );
        presence.announce_restarting();
        drop(presence);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        let mut replacement = Presence::start(
            loc.clone(),
            TrayHostServer::new(false),
            Arc::new(HandlerMock),
            true,
            &prefs,
        );
        let mut client = Client::connect(
            &loc,
            "arcade.tools",
            &PeerInfo {
                id: "arcade.find".into(),
                version: "1".into(),
            },
        )
        .unwrap();
        client.subscribe(&["tray.host"]).unwrap();
        assert_eq!(
            client
                .next_notification(Some(Duration::from_secs(1)))
                .unwrap()
                .params()["hosted"],
            true
        );
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert_eq!(watcher.state(), TrayState::Hosted);
        replacement.update(false, &prefs);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TrayState::Own
        );
    }
}
