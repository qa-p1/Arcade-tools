use super::model::{PeerCommand, PeerState};
use arcade_link::{
    client::{self, Client},
    manifest::Manifest,
    ErrorCode, LinkError, Locations,
};
use arcade_tools_core::manager::me;
use std::{collections::BTreeMap, time::Duration};

pub fn call(
    locations: &Locations,
    manifest: &Manifest,
    command: &PeerCommand,
) -> Result<(), LinkError> {
    if !manifest.settings.link_enabled {
        if *command == PeerCommand::Open {
            return client::spawn_detached(&manifest.executable, &[])
                .map_err(|e| LinkError::internal(e.to_string()));
        }
        return Err(LinkError::unavailable(
            "Enable Arcade Link inside this app first.",
        ));
    }
    let mut client =
        match Client::connect_with(locations, &manifest.id, &me(), Duration::from_millis(500)) {
            Ok(client) => client,
            Err(error) if error.code == ErrorCode::NotRunning && *command == PeerCommand::Open => {
                client::launch_and_connect(locations, manifest, &me())?
            }
            Err(error) => return Err(error),
        };
    client.set_timeout(Duration::from_secs(2))?;
    let (method, params) = command.call();
    client.call(method, params).map(|_| ())
}

pub fn quit_all(
    locations: &Locations,
    manifests: &[Manifest],
    states: &BTreeMap<String, PeerState>,
    confirm: impl FnOnce(&[String]) -> bool,
) -> Result<(), String> {
    let running: Vec<_> = manifests
        .iter()
        .filter(|app| {
            arcade_link::manifest::ids::APPS.contains(&app.id.as_str())
                && states.get(&app.id).is_some_and(|state| state.running)
        })
        .collect();
    let busy: Vec<_> = running
        .iter()
        .filter(|app| states[&app.id].busy)
        .map(|app| app.name.clone())
        .collect();
    if !busy.is_empty() && !confirm(&busy) {
        return Ok(());
    }
    let errors: Vec<_> = running
        .into_iter()
        .filter_map(|app| {
            call(locations, app, &PeerCommand::Quit)
                .err()
                .map(|error| error.user_message(&app.name))
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::{
        server::{Handler, Server, ServerConfig},
        Action, InvokeContext, InvokeRequest, PeerInfo, Reply,
    };
    use std::sync::{mpsc, Arc};
    struct Mock(mpsc::Sender<String>);
    impl Handler for Mock {
        fn methods(&self) -> Vec<String> {
            vec![
                "app.settings".into(),
                "app.restart".into(),
                "app.menu.invoke".into(),
            ]
        }
        fn describe(&self) -> Vec<Action> {
            vec![]
        }
        fn invoke(&self, _: InvokeRequest, _: &InvokeContext) -> Result<Reply, LinkError> {
            Err(LinkError::unsupported("test"))
        }
        fn activate(&self) -> Result<(), LinkError> {
            self.0.send("activate".into()).unwrap();
            Ok(())
        }
        fn settings(&self) -> Result<(), LinkError> {
            self.0.send("settings".into()).unwrap();
            Ok(())
        }
        fn restart(&self, mode: &str) -> Result<(), LinkError> {
            self.0.send(format!("restart:{mode}")).unwrap();
            Ok(())
        }
        fn menu_invoke(&self, id: &str) -> Result<(), LinkError> {
            self.0.send(format!("extra:{id}")).unwrap();
            Ok(())
        }
        fn quit(&self) -> Result<(), LinkError> {
            self.0.send("quit".into()).unwrap();
            Ok(())
        }
    }
    #[test]
    fn menu_clicks_reach_the_peer_directly() {
        let root = tempfile::tempdir().unwrap();
        let loc = Locations::under(root.path());
        let (tx, rx) = mpsc::channel();
        let _server = Server::start(
            ServerConfig {
                locations: loc.clone(),
                app: PeerInfo {
                    id: "arcade.find".into(),
                    version: "1".into(),
                },
            },
            Arc::new(Mock(tx)),
        )
        .unwrap();
        let manifest = Manifest::new(
            "arcade.find",
            "1",
            &std::env::current_exe().unwrap().to_string_lossy(),
        );
        for (command, expected) in [
            (PeerCommand::Open, "activate"),
            (PeerCommand::Settings, "settings"),
            (PeerCommand::Restart, "restart:normal"),
            (PeerCommand::Extra("private".into()), "extra:private"),
            (PeerCommand::Quit, "quit"),
        ] {
            call(&loc, &manifest, &command).unwrap();
            assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), expected);
        }
    }

    #[test]
    fn quit_all_confirms_busy_apps_once_before_sending_any_call() {
        let root = tempfile::tempdir().unwrap();
        let loc = Locations::under(root.path());
        let (tx, rx) = mpsc::channel();
        let mut servers = vec![];
        let mut manifests = vec![];
        let mut states = BTreeMap::new();
        for id in ["arcade.find", "arcade.box"] {
            servers.push(
                Server::start(
                    ServerConfig {
                        locations: loc.clone(),
                        app: PeerInfo {
                            id: id.into(),
                            version: "1".into(),
                        },
                    },
                    Arc::new(Mock(tx.clone())),
                )
                .unwrap(),
            );
            manifests.push(Manifest::new(id, "1", "/fixture/app"));
            states.insert(
                id.into(),
                PeerState {
                    running: true,
                    busy: true,
                    ..Default::default()
                },
            );
        }
        let confirmations = std::cell::Cell::new(0);
        quit_all(&loc, &manifests, &states, |busy| {
            confirmations.set(confirmations.get() + 1);
            assert_eq!(busy.len(), 2);
            false
        })
        .unwrap();
        assert_eq!(confirmations.get(), 1);
        assert!(rx.try_recv().is_err());
        quit_all(&loc, &manifests, &states, |_| {
            confirmations.set(confirmations.get() + 1);
            true
        })
        .unwrap();
        assert_eq!(confirmations.get(), 2);
        for _ in 0..2 {
            assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "quit");
        }
        for state in states.values_mut() {
            state.running = false;
        }
        quit_all(&loc, &manifests, &states, |_| {
            panic!("Stopped apps need no confirmation")
        })
        .unwrap();
        assert!(rx.try_recv().is_err());
    }
}
