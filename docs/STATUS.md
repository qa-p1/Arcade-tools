# Arcade Tools 0.2.0 status

Integration branch: `arcade/tools-hub`. All eight implementation lanes are merged:
core, diagnostics, metadata, overlay, shortcuts, Svelte, tabs and tray. Arcade Link
is pinned to the released `v0.3.0` tag, commit
`33650a87b6999fbd553ac678cc7bde7a308cb852`.

The complete hub, resident tray, shortcut overlay, updates/receipts/adoption,
offline Learn, engine detection, diagnostics and backup/restore are implemented.
See [the user guide](user/index.md) for behavior and
[README](../README.md) for the exact check commands.

Verification for this final integration is in progress. Previous lane checks are
not presented as evidence for the merged branch. This page will record the final
local results and Linux/Windows/macOS CI run after the full check pass.

Published stable tags currently predate `docs/user/`; the bundled nine-source
snapshot uses clearly labeled local user/ecosystem guides. Release builds refresh
stable-tag docs and keep an existing labeled snapshot when released guides are
missing. Runtime exact-version downloads remain separate.

No real application installs, login files, shell profiles or Hyprland configuration
were changed on this machine. GUI/lifecycle checks use Link's disposable
HOME/XDG/ARCADE session. Windows/macOS package behavior is CI only, not a real
desktop run. Sway/KDE/GNOME readers have fixture coverage and are not run on a real
desktop. Signing/notarization remain the owner's release decisions.
