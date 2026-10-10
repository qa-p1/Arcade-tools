//! Configuration selection and registration-order runtime reconciliation.

use crate::{conf, labels, lua, read, runtime, BindState, ConfigReport, WmBind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigKind {
    Lua,
    Conf,
    #[default]
    None,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyprlandReport {
    pub config_kind: ConfigKind,
    pub binds: Vec<WmBind>,
    pub warnings: Vec<String>,
    /// Files actually read, so callers can watch them without polling.
    pub files: Vec<std::path::PathBuf>,
}

/// Lua takes precedence. No config is created if neither file exists.
pub fn select_config(config_dir: &Path) -> (ConfigKind, Option<std::path::PathBuf>) {
    for (kind, name) in [
        (ConfigKind::Lua, "hyprland.lua"),
        (ConfigKind::Conf, "hyprland.conf"),
    ] {
        let path = config_dir.join(name);
        if path.is_file() {
            return (kind, Some(path));
        }
    }
    (ConfigKind::None, None)
}

/// Full machine report. Synchronous disk/IPC work: call on an index worker.
pub fn read_machine(user_labels: &labels::UserLabels) -> HyprlandReport {
    let config_dir = read::config_home().unwrap_or_default().join("hypr");
    let (kind, path) = select_config(&config_dir);
    let config = match (kind, path) {
        (ConfigKind::Lua, Some(path)) => {
            lua::evaluate(&path, &lua::SandboxOptions::new(config_dir))
        }
        (ConfigKind::Conf, Some(path)) => conf::parse(&path, &conf::ConfOptions::default()),
        _ => ConfigReport::default(),
    };
    let (runtime, warning) = match runtime::query() {
        Ok(binds) => (binds, None),
        Err(e) => (None, Some(e)),
    };
    let mut result = merge(kind, config, runtime.as_deref(), user_labels);
    if let Some(warning) = warning {
        result.warnings.push(warning);
    }
    result
}

pub fn merge(
    kind: ConfigKind,
    config: ConfigReport,
    runtime: Option<&[runtime::RuntimeBind]>,
    user_labels: &labels::UserLabels,
) -> HyprlandReport {
    type Key = (Option<String>, String);
    let runtime = runtime.unwrap_or_default();
    let mut by_key: BTreeMap<Key, VecDeque<usize>> = BTreeMap::new();
    for (i, bind) in runtime.iter().enumerate() {
        if let Some(chord) = &bind.chord {
            by_key
                .entry((bind.submap.clone(), chord.clone()))
                .or_default()
                .push_back(i);
        }
    }
    let mut used = vec![false; runtime.len()];
    let mut binds = Vec::new();
    for mut bind in config.binds {
        let matched = bind
            .chord
            .as_ref()
            .and_then(|chord| by_key.get_mut(&(bind.submap.clone(), chord.clone())))
            .and_then(VecDeque::pop_front);
        let state = if let Some(i) = matched {
            used[i] = true;
            let runtime = &runtime[i];
            bind.flags.extend(runtime.flags.clone());
            if bind.description.is_none() {
                bind.description.clone_from(&runtime.description);
            }
            BindState::Active
        } else {
            BindState::ConfiguredOnly
        };
        let label = labels::for_config(&bind, &config.sources, user_labels);
        binds.push(WmBind {
            chord: bind.chord,
            key_text: bind.key_text,
            submap: bind.submap,
            label: label.label,
            detail: label.detail,
            category: label.category,
            action: bind.action,
            flags: bind.flags,
            origin: bind.origin,
            state,
        });
    }
    for (_, bind) in runtime.iter().enumerate().filter(|(i, _)| !used[*i]) {
        let dispatcher = labels::dispatcher_label(&bind.action);
        let label = labels::user_label(user_labels, bind.submap.as_deref(), bind.chord.as_deref())
            .map(str::to_owned)
            .or_else(|| bind.description.clone())
            .or_else(|| dispatcher.clone())
            .unwrap_or_else(|| "Lua function (added at runtime)".into());
        binds.push(WmBind {
            chord: bind.chord.clone(),
            key_text: bind.key_text.clone(),
            submap: bind.submap.clone(),
            detail: dispatcher.filter(|d| d != &label),
            label,
            category: None,
            action: bind.action.clone(),
            flags: bind.flags.clone(),
            origin: None,
            state: BindState::RuntimeOnly,
        });
    }
    HyprlandReport {
        config_kind: kind,
        binds,
        warnings: config.warnings,
        files: config.sources.into_keys().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, ConfigBind};
    use serde_json::json;

    #[test]
    fn matching_preserves_order_and_submaps_and_runtime_labels() {
        let configured = |description: &str| ConfigBind {
            key_text: "SUPER + T".into(),
            chord: Some("Super+T".into()),
            submap: None,
            action: Action::Dispatcher {
                name: "exec_cmd".into(),
                args: vec![json!("foot")],
            },
            flags: BTreeMap::new(),
            description: Some(description.into()),
            origin: None,
        };
        let runtime = runtime::parse_json(r#"[{"modmask":64,"key":"T","dispatcher":"__lua"},{"modmask":64,"key":"T","dispatcher":"__lua","release":true},{"modmask":64,"key":"T","submap":"resize","dispatcher":"__lua"},{"modmask":12,"key":"F","dispatcher":"exec","arg":"arcade-find --toggle"}]"#).unwrap();
        let report = merge(
            ConfigKind::Lua,
            ConfigReport {
                binds: vec![
                    configured("First"),
                    configured("Second"),
                    configured("Third"),
                ],
                ..Default::default()
            },
            Some(&runtime),
            &BTreeMap::new(),
        );
        assert_eq!(
            report.binds.iter().map(|b| b.state).collect::<Vec<_>>(),
            [
                BindState::Active,
                BindState::Active,
                BindState::ConfiguredOnly,
                BindState::RuntimeOnly,
                BindState::RuntimeOnly
            ]
        );
        assert_eq!(report.binds[1].flags.get("release"), Some(&true));
        assert_eq!(report.binds[3].label, "Lua function (added at runtime)");
        assert_eq!(report.binds[4].label, "Run: arcade-find --toggle");
    }

    #[test]
    fn selection_and_total_evaluation_failure_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(select_config(dir.path()).0, ConfigKind::None);
        std::fs::write(dir.path().join("hyprland.conf"), "").unwrap();
        assert_eq!(select_config(dir.path()).0, ConfigKind::Conf);
        std::fs::write(dir.path().join("hyprland.lua"), "bad lua").unwrap();
        assert_eq!(select_config(dir.path()).0, ConfigKind::Lua);
        let runtime =
            runtime::parse_json(r#"[{"key":"A","dispatcher":"__lua","description":"Runtime"}]"#)
                .unwrap();
        let result = merge(
            ConfigKind::Lua,
            ConfigReport {
                warnings: vec!["syntax error".into()],
                ..Default::default()
            },
            Some(&runtime),
            &BTreeMap::new(),
        );
        assert_eq!(result.binds[0].label, "Runtime");
        assert_eq!(result.binds[0].state, BindState::RuntimeOnly);
        assert_eq!(result.warnings, ["syntax error"]);
    }

    #[test]
    fn wall_clock_abort_surfaces_warning_and_keeps_every_runtime_bind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hyprland.lua");
        std::fs::write(&path, "hl.bind('SUPER + A',hl.dsp.no_op()); for i=1,1000000 do local x=i*i end; hl.bind('SUPER + B',hl.dsp.exit())").unwrap();
        let mut options = lua::SandboxOptions::new(dir.path());
        options.read_policy = crate::ReadPolicy::new([dir.path().to_owned()]);
        options.duration_limit = std::time::Duration::ZERO;
        let config = lua::evaluate(&path, &options);
        assert_eq!(config.binds.len(), 1);
        let runtime = runtime::parse_json(r#"[{"modmask":64,"key":"A","dispatcher":"__lua"},{"modmask":64,"key":"B","dispatcher":"__lua"},{"modmask":64,"key":"C","dispatcher":"__lua"}]"#).unwrap();
        let report = merge(ConfigKind::Lua, config, Some(&runtime), &BTreeMap::new());
        assert_eq!(report.binds.len(), 3);
        assert_eq!(report.binds[0].state, BindState::Active);
        assert!(report.binds[1..]
            .iter()
            .all(|b| b.state == BindState::RuntimeOnly));
        let index = crate::build_index(&crate::BuildInputs {
            hyprland: Some(report),
            ..Default::default()
        });
        assert_eq!(
            index
                .wm
                .groups
                .iter()
                .map(|g| g.shortcuts.len())
                .sum::<usize>(),
            3
        );
        assert!(index
            .wm
            .warnings
            .iter()
            .any(|w| w.contains("time limit exceeded") && w.contains("labels are incomplete")));
    }
}
