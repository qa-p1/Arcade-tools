---
title: Shortcuts
order: 5
---

# Shortcuts

Open Arcade Tools' shortcut sheet with **Super+/** on Linux,
**Ctrl+Alt+/** on Windows, or **Ctrl+Option+/** on macOS. It follows the
currently focused app and shows that app's shortcut sheet, Arcade app
shortcuts, and the window manager shortcuts Tools can read.

On Linux, Tools reads Hyprland's Lua and classic configuration formats. It
does not edit window manager files or change their bindings. You can edit the
labels shown in Tools to make a binding easier to recognize; changing a label
does not change the shortcut itself.

## Default Arcade shortcuts

These are the defaults in the Arcade Link specification. Saved shortcuts are
not changed when a new default is released. On macOS, Cmd means Command and
Option means the Option key.

| App | Linux | Windows | macOS |
| --- | --- | --- | --- |
| Box | Ctrl+Alt+Space | Ctrl+Alt+Space | Cmd+Shift+Space |
| Look | None (Space in GNOME Files) | Ctrl+Alt+Shift+Space | Ctrl+Option+Space |
| Lens | Ctrl+Alt+Shift+L | Ctrl+Alt+Shift+L | Ctrl+Alt+Shift+L |
| Clipboard | Ctrl+Shift+Space | Ctrl+Alt+V | Cmd+Shift+V |
| Wheel | F8 | F8 | F8 |
| Shelf | Ctrl+Alt+S | Ctrl+Alt+S | Ctrl+Alt+S |
| Find | Ctrl+Alt+F | Ctrl+Alt+F | Ctrl+Alt+F |

## Fix a conflict

Tools marks shortcuts that overlap so you can see what may intercept a key.
If an Arcade app shortcut conflicts, change that app's global shortcut in
Tools or in the app's own settings. If the conflicting binding belongs to
your window manager, edit that configuration yourself; Tools only reads it.
Arcade apps also warn when another Arcade app already uses the shortcut.

To add or update a third-party shortcut sheet, follow
[CONTRIBUTING.md](../CONTRIBUTING.md).
For app installation, see [Installing and updating](installing.md); for the
shared action menu, see [One tray icon](tray.md).
