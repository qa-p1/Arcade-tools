//! A cached model shared by SNI and the native Tauri renderer.

use super::settings::Preferences;
use arcade_link::manifest::{self, ids, ManifestDocument, MenuKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerCommand {
    Open,
    Settings,
    Restart,
    Quit,
    Extra(String),
}

impl PeerCommand {
    pub fn call(&self) -> (&'static str, Value) {
        match self {
            Self::Open => ("app.activate", json!({})),
            Self::Settings => ("app.settings", json!({})),
            Self::Restart => ("app.restart", json!({"mode":"normal"})),
            Self::Quit => ("app.quit", json!({})),
            Self::Extra(id) => ("app.menu.invoke", json!({"id":id})),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    OpenTools,
    Shortcuts,
    Updates,
    Peer { app: String, command: PeerCommand },
    Login { app: String, enabled: bool },
    StartAll,
    QuitAll,
    QuitTools,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerState {
    pub running: bool,
    pub busy: bool,
    pub methods: Vec<String>,
    pub login: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Action {
        id: String,
        title: String,
        enabled: bool,
        checked: Option<bool>,
        action: Action,
    },
    Submenu {
        title: String,
        glyph: String,
        items: Vec<Item>,
    },
    Separator,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayModel {
    pub items: Vec<Item>,
    pub badge: bool,
}

fn item(id: &str, title: impl Into<String>, action: Action) -> Item {
    Item::Action {
        id: id.into(),
        title: title.into(),
        enabled: true,
        checked: None,
        action,
    }
}

fn peer(app: &str, title: impl Into<String>, command: PeerCommand) -> Item {
    let (method, _) = command.call();
    let suffix = match &command {
        PeerCommand::Extra(id) => id.as_str(),
        _ => method,
    };
    item(
        &format!("{app}/{suffix}"),
        title,
        Action::Peer {
            app: app.into(),
            command,
        },
    )
}

impl TrayModel {
    pub fn build(
        documents: &[ManifestDocument],
        states: &BTreeMap<String, PeerState>,
        prefs: &Preferences,
        updates: usize,
        overlay: bool,
    ) -> Self {
        // The sidebar's order: Home, then Updates (with its count, like the
        // sidebar badge), then Shortcuts.
        let mut items = vec![item("tools/open", "Open Arcade Tools", Action::OpenTools)];
        if updates > 0 {
            items.push(item(
                "tools/updates",
                format!("Updates ({updates})"),
                Action::Updates,
            ));
        }
        if overlay {
            items.push(item(
                "tools/shortcuts",
                if prefs.shortcuts_enabled {
                    format!("Show shortcuts    {}", prefs.shortcut)
                } else {
                    "Show shortcuts".into()
                },
                Action::Shortcuts,
            ));
        }
        items.push(Item::Separator);
        for document in documents
            .iter()
            .filter(|document| ids::APPS.contains(&document.manifest.id.as_str()))
        {
            let id = document.manifest.id.as_str();
            let state = states.get(id).cloned().unwrap_or_default();
            let name = manifest::app_name(id);
            let mut children = vec![peer(
                id,
                if state.running {
                    format!("Open {name}")
                } else {
                    format!("Start {name}")
                },
                PeerCommand::Open,
            )];
            if state.running {
                if state.methods.iter().any(|m| m == "app.settings") {
                    children.push(peer(id, "Settings", PeerCommand::Settings));
                }
                for extra in &document.additions.menu {
                    if extra.kind == MenuKind::Separator {
                        children.push(Item::Separator);
                    } else {
                        let mut entry =
                            peer(id, &extra.title, PeerCommand::Extra(extra.id.clone()));
                        if let Item::Action {
                            enabled, checked, ..
                        } = &mut entry
                        {
                            *enabled = extra.enabled
                                && state.methods.iter().any(|m| m == "app.menu.invoke");
                            *checked = (extra.kind == MenuKind::Toggle)
                                .then_some(extra.checked.unwrap_or(false));
                        }
                        children.push(entry);
                    }
                }
                children.push(Item::Separator);
                if state.methods.iter().any(|m| m == "app.restart") {
                    children.push(peer(id, format!("Restart {name}"), PeerCommand::Restart));
                }
                children.push(peer(id, format!("Quit {name}"), PeerCommand::Quit));
            } else if let Some(login) = state.login {
                let mut entry = item(
                    &format!("{id}/login"),
                    "Start at login",
                    Action::Login {
                        app: id.into(),
                        enabled: !login,
                    },
                );
                if let Item::Action { checked, .. } = &mut entry {
                    *checked = Some(login);
                }
                children.push(entry);
            }
            let mut title = name.to_owned();
            if !state.running {
                // With Arcade Link off, the app may well be running; Tools
                // just can't reach it.
                title.push_str(if document.manifest.settings.link_enabled {
                    " · Not running"
                } else {
                    " · Connections off"
                });
            }
            if prefs.excluded.iter().any(|e| e == id) {
                title.push_str(" · Own icon");
            }
            items.push(Item::Submenu {
                title,
                glyph: id.into(),
                items: children,
            });
        }
        items.extend([
            Item::Separator,
            item("tools/start-all", "Start all Arcade apps", Action::StartAll),
            item("tools/quit-all", "Quit all Arcade apps", Action::QuitAll),
            Item::Separator,
            item("tools/quit", "Quit Arcade Tools", Action::QuitTools),
        ]);
        Self {
            items,
            badge: updates > 0 && prefs.show_update_badge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_link::manifest::Manifest;

    #[test]
    fn installed_apps_in_order_running_excluded_extras_and_login() {
        let find = ManifestDocument::from_json(include_str!("../../tests/fixtures/find-menu.json"))
            .unwrap();
        let box_doc = Manifest::new(ids::BOX, "1", "/fixture/box").into();
        let prefs = Preferences {
            excluded: vec![ids::FIND.into()],
            ..Default::default()
        };
        let states = BTreeMap::from([
            (
                ids::FIND.into(),
                PeerState {
                    running: true,
                    methods: vec![
                        "app.settings".into(),
                        "app.restart".into(),
                        "app.menu.invoke".into(),
                    ],
                    ..Default::default()
                },
            ),
            (
                ids::BOX.into(),
                PeerState {
                    login: Some(false),
                    ..Default::default()
                },
            ),
        ]);
        let model = TrayModel::build(&[box_doc, find], &states, &prefs, 2, true);
        assert!(model.badge);
        let submenus: Vec<_> = model
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Submenu { title, items, .. } => Some((title, items)),
                _ => None,
            })
            .collect();
        assert_eq!(submenus[0].0, "Arcade Box · Not running");
        assert_eq!(submenus[1].0, "Arcade Find · Own icon");
        assert!(submenus[0].1.iter().any(|i| matches!(
            i,
            Item::Action {
                checked: Some(false),
                action: Action::Login { enabled: true, .. },
                ..
            }
        )));
        assert!(submenus[1].1.iter().any(|i| matches!(i, Item::Action { checked: Some(true), action: Action::Peer { command: PeerCommand::Extra(id), .. }, .. } if id == "private")));
        assert!(submenus[1].1.iter().any(|i| matches!(i, Item::Action { enabled: false, action: Action::Peer { command: PeerCommand::Extra(id), .. }, .. } if id == "offline")));
        // An app with Arcade Link off is unreachable, not necessarily stopped.
        let mut off: ManifestDocument = Manifest::new(ids::BOX, "1", "/fixture/box").into();
        off.manifest.settings.link_enabled = false;
        let model = TrayModel::build(&[off], &states, &prefs, 0, false);
        assert!(model.items.iter().any(
            |i| matches!(i, Item::Submenu { title, .. } if title == "Arcade Box · Connections off")
        ));
        let empty = TrayModel::build(&[], &BTreeMap::new(), &prefs, 0, false);
        assert!(!empty.badge);
        assert!(!empty.items.iter().any(|i| matches!(
            i,
            Item::Action {
                action: Action::Updates | Action::Shortcuts,
                ..
            }
        )));
    }

    #[test]
    fn menu_follows_the_sidebar_and_quit_comes_last_after_a_separator() {
        let model = TrayModel::build(&[], &BTreeMap::new(), &Preferences::default(), 3, true);
        let titles: Vec<&str> = model
            .items
            .iter()
            .map(|i| match i {
                Item::Action { title, .. } => title.as_str(),
                Item::Separator => "-",
                Item::Submenu { title, .. } => title.as_str(),
            })
            .collect();
        assert_eq!(&titles[..2], ["Open Arcade Tools", "Updates (3)"]);
        assert!(titles[2].starts_with("Show shortcuts"));
        assert_eq!(&titles[titles.len() - 2..], ["-", "Quit Arcade Tools"]);
    }

    #[test]
    fn calls_match_optional_link_methods() {
        assert_eq!(PeerCommand::Open.call(), ("app.activate", json!({})));
        assert_eq!(PeerCommand::Settings.call().0, "app.settings");
        assert_eq!(
            PeerCommand::Restart.call(),
            ("app.restart", json!({"mode":"normal"}))
        );
        assert_eq!(PeerCommand::Quit.call().0, "app.quit");
        assert_eq!(
            PeerCommand::Extra("private".into()).call(),
            ("app.menu.invoke", json!({"id":"private"}))
        );
    }

    #[test]
    fn old_peer_keeps_basic_controls_and_disables_unsupported_extras() {
        let find = ManifestDocument::from_json(include_str!("../../tests/fixtures/find-menu.json"))
            .unwrap();
        let states = BTreeMap::from([(
            ids::FIND.into(),
            PeerState {
                running: true,
                ..Default::default()
            },
        )]);
        let model = TrayModel::build(&[find], &states, &Preferences::default(), 0, true);
        let items = model
            .items
            .iter()
            .find_map(|item| match item {
                Item::Submenu { items, .. } => Some(items),
                _ => None,
            })
            .unwrap();
        assert!(items.iter().any(|item| matches!(
            item,
            Item::Action {
                action: Action::Peer {
                    command: PeerCommand::Open,
                    ..
                },
                enabled: true,
                ..
            }
        )));
        assert!(items.iter().any(|item| matches!(
            item,
            Item::Action {
                action: Action::Peer {
                    command: PeerCommand::Quit,
                    ..
                },
                enabled: true,
                ..
            }
        )));
        assert!(!items.iter().any(|item| matches!(
            item,
            Item::Action {
                action: Action::Peer {
                    command: PeerCommand::Settings | PeerCommand::Restart,
                    ..
                },
                ..
            }
        )));
        assert!(items.iter().all(|item| !matches!(
            item,
            Item::Action {
                action: Action::Peer {
                    command: PeerCommand::Extra(_),
                    ..
                },
                enabled: true,
                ..
            }
        )));
    }
}
