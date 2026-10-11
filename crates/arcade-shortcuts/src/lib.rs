//! Window-manager shortcut sources for Arcade Tools' shortcut center.
//!
//! Hyprland is read three ways and merged:
//! - a Lua config (`hyprland.lua`, Hyprland ≥ 0.56) is evaluated in a sandbox
//!   with a stub `hl` table that records every `hl.bind` ([`lua`]);
//! - a hyprlang config (`hyprland.conf`) is parsed ([`conf`]);
//! - `hyprctl binds -j` gives the binds Hyprland actually holds, including
//!   ones added at runtime by Arcade apps ([`runtime`]).
//!
//! Sway, KDE and GNOME supply the same worker-side WM report ([`wm`]).
//! Nothing edits user configuration. Queries read Hyprland binds, Sway modes
//! and loaded config, or GNOME settings; KDE reads kglobalshortcutsrc.

#[cfg(feature = "builder")]
pub mod conf;
#[cfg(feature = "builder")]
pub mod gnome;
#[cfg(feature = "builder")]
pub mod hyprland;
pub mod index;
#[cfg(feature = "builder")]
pub mod kde;
pub mod keys;
pub mod labels;
#[cfg(feature = "builder")]
pub mod lua;
#[cfg(feature = "builder")]
mod read;
#[cfg(feature = "builder")]
pub mod runtime;
#[cfg(feature = "builder")]
pub mod sway;
#[cfg(feature = "builder")]
pub mod wm;

#[cfg(feature = "builder")]
pub use index::{build_index, BuildInputs};
pub use index::{default_index_path, Index, Os};
#[cfg(feature = "builder")]
pub use read::{ConfigReport, ReadPolicy};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigKind {
    Lua,
    Conf,
    Kconfig,
    Gsettings,
    #[default]
    #[serde(other)]
    None,
}

/// What a bind does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// `hl.dsp.window.move({workspace = 3})` → `name: "window.move"`, `args: [{"workspace": 3}]`.
    /// hyprlang `movetoworkspace, 3` → `name: "movetoworkspace"`, `args: ["3"]`.
    Dispatcher {
        name: String,
        args: Vec<serde_json::Value>,
    },
    /// A Lua function; where it was defined.
    LuaFunction {
        file: Option<PathBuf>,
        line: Option<u32>,
    },
    /// Known only from `hyprctl` as `__lua` (a Lua callback added at runtime).
    Unknown,
}

/// Where a bind was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub file: PathBuf,
    /// The statement in that file that created the bind (for helper functions
    /// such as Caelestia's `create_bind`, the helper's call site).
    pub line: u32,
}

/// One bind as read from a configuration file, before labeling and merging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBind {
    /// As written: `SUPER + SHIFT + left` (Lua) or `SUPER SHIFT, left` (hyprlang).
    pub key_text: String,
    /// Canonical chord, `None` when it can't be expressed.
    pub chord: Option<String>,
    pub submap: Option<String>,
    pub action: Action,
    /// Flags as booleans (`locked`, `release`, `repeating`, `mouse`, …).
    pub flags: BTreeMap<String, bool>,
    /// `desc` (Lua) / `bindd` description (hyprlang).
    pub description: Option<String>,
    pub origin: Option<Origin>,
}

/// Whether a bind is configured, active, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindState {
    /// In the configuration and held by Hyprland.
    Active,
    /// In the configuration, not held by Hyprland (conditional, failed, or
    /// Hyprland not reachable).
    ConfiguredOnly,
    /// Held by Hyprland, not in the configuration (added at runtime).
    RuntimeOnly,
}

/// A labeled, merged window-manager bind: what the shortcut center shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WmBind {
    pub chord: Option<String>,
    pub key_text: String,
    pub submap: Option<String>,
    /// Short human label ("Terminal", "Go to workspace", "Caelestia: launcher").
    pub label: String,
    /// What it runs, when the label doesn't already say it ("Run: foot").
    pub detail: Option<String>,
    /// Section from the configuration's comments ("Media", "Window actions").
    pub category: Option<String>,
    pub action: Action,
    pub flags: BTreeMap<String, bool>,
    pub origin: Option<Origin>,
    pub state: BindState,
}
