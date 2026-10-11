//! Desktop selection and a common read-only report for the index worker.

use crate::{labels, BindState, ConfigKind, ConfigReport, WmBind};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct WmReport {
    pub name: String,
    pub config_kind: ConfigKind,
    pub binds: Vec<WmBind>,
    pub warnings: Vec<String>,
    /// Files read by this rebuild; callers can watch changes without polling.
    pub files: Vec<PathBuf>,
}

impl From<crate::hyprland::HyprlandReport> for WmReport {
    fn from(report: crate::hyprland::HyprlandReport) -> Self {
        Self {
            name: "hyprland".into(),
            config_kind: report.config_kind,
            binds: report.binds,
            warnings: report.warnings,
            files: report.files,
        }
    }
}

impl WmReport {
    pub fn apply_labels(&mut self, overrides: &labels::UserLabels) {
        for bind in &mut self.binds {
            if let Some(label) =
                labels::user_label(overrides, bind.submap.as_deref(), bind.chord.as_deref())
            {
                bind.label = label.trim().to_owned();
            }
        }
    }
}

pub(crate) fn configured(
    name: &str,
    config: ConfigReport,
    overrides: &labels::UserLabels,
) -> WmReport {
    let binds = config
        .binds
        .into_iter()
        .map(|bind| {
            let label = labels::for_config(&bind, &config.sources, overrides);
            WmBind {
                chord: bind.chord,
                key_text: bind.key_text,
                submap: bind.submap,
                label: label.label,
                detail: label.detail,
                category: label.category,
                action: bind.action,
                flags: bind.flags,
                origin: bind.origin,
                state: BindState::ConfiguredOnly,
            }
        })
        .collect();
    WmReport {
        name: name.into(),
        config_kind: ConfigKind::Conf,
        binds,
        warnings: config.warnings,
        files: config.sources.into_keys().collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    Hyprland,
    Sway,
    Kde,
    Gnome,
    None,
}

/// Pure selection, also used by fixture tests. Socket hints take precedence.
pub fn desktop(hyprland: bool, sway: bool, session: &str) -> Desktop {
    if hyprland {
        return Desktop::Hyprland;
    }
    if sway {
        return Desktop::Sway;
    }
    for name in session
        .split([':', ';'])
        .map(|part| part.trim().to_ascii_lowercase())
    {
        match name.as_str() {
            "hyprland" => return Desktop::Hyprland,
            "sway" => return Desktop::Sway,
            "kde" | "plasma" => return Desktop::Kde,
            "gnome" | "ubuntu" | "unity" => return Desktop::Gnome,
            _ => {}
        }
    }
    Desktop::None
}

/// No resident watcher or hotkey registration. Call once on the index worker.
pub fn read_machine(overrides: &labels::UserLabels) -> WmReport {
    #[cfg(target_os = "linux")]
    {
        let present = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        let session = std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("XDG_SESSION_DESKTOP"))
            .unwrap_or_default();
        match desktop(
            present("HYPRLAND_INSTANCE_SIGNATURE"),
            present("SWAYSOCK"),
            &session,
        ) {
            Desktop::Hyprland => crate::hyprland::read_machine(overrides).into(),
            Desktop::Sway => crate::sway::read_machine(overrides),
            Desktop::Kde => crate::kde::read_machine(overrides),
            Desktop::Gnome => crate::gnome::read_machine(overrides),
            Desktop::None => WmReport {
                name: "none".into(),
                ..Default::default()
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = overrides;
        WmReport {
            name: "none".into(),
            ..Default::default()
        }
    }
}

pub(crate) fn query(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    crate::runtime::command_output(&mut command, std::time::Duration::from_millis(500))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_and_desktop_selection() {
        assert_eq!(desktop(true, true, "GNOME"), Desktop::Hyprland);
        assert_eq!(desktop(false, true, "KDE"), Desktop::Sway);
        assert_eq!(desktop(false, false, "ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(desktop(false, false, "plasma"), Desktop::Kde);
        assert_eq!(desktop(false, false, "unknown"), Desktop::None);
    }
}
