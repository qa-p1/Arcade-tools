The Hyprland example and runtime JSON were supplied with this task (Hyprland
0.56.2). `hypr/caelestia/home/.config/` is a read-only copy of the owner's Lua
tree and Caelestia variable overrides, captured 2026-10-11. Absolute home paths
and usernames are replaced with fixture placeholders. Optional `hypr-user`
and `hypremoji` modules are deliberately absent. `owner-overrides.lua` preserves
the actual override file outside the default module search path: the default
fixture has 140 binds with all optional modules missing. A separate test copies
those overrides into a temporary HOME and checks the customized 139-bind tree
(the disabled screenshot bind is restored by the real optional hypr-user).
Tests override HOME and
XDG_CONFIG_HOME inside the Lua VM, never in the process environment.

`catalog/` contains 31 schema-1 shortcut sheets from the read-only local
Arcade-catalog checkout (2026-10-11), with their source attribution retained.
The docs, registry, labels and hyprlang files are small synthetic fixtures.
# Other Linux desktop readers

The `sway/`, `kde/` and `gnome/` fixtures are synthetic configurations/settings
following the upstream formats. They cover includes, modes, source lines,
KConfig escapes/alternatives/sequences, disabled defaults, effective GNOME
values, custom names and a saved-overrides-only dconf fallback. They are tested
without executing configured actions or touching desktop settings.
These readers have not been run on a real Sway, KDE or GNOME desktop.

