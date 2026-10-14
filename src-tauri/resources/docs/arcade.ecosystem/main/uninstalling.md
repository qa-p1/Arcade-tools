---
title: Uninstalling and your data
order: 7
---

# Uninstalling and your data

In Arcade Tools, remove one app or choose **Uninstall everything** for all
managed apps. Without Tools, use an app's arcade-x command with
--uninstall or its **Uninstall Arcade <App>** launcher action when available.
The app command is named for the app, for example arcade-box or arcade-find.

- **Linux:** the app command is installed in ~/.local/bin. You can also choose
  the app's uninstall action from its application launcher.
- **Windows:** open **Settings → Apps → Installed apps** and choose
  **Uninstall**.
- **macOS:** quit the app and move it from Applications to the Trash.

Tools keeps settings and data by default. Select **Remove settings and data**
to see the app's data folders and remove them with the app. The confirmation
lists the exact folders before anything is deleted. Files you saved elsewhere
are not removed.

## App data folders

These are the folders Tools offers to remove. On Linux, XDG_DATA_HOME defaults
to ~/.local/share and XDG_CONFIG_HOME defaults to ~/.config. On Windows,
APPDATA and LOCALAPPDATA are your per-user roaming and local application data
folders.

### Linux

- **Box:** $XDG_DATA_HOME/dev.arcadebox.app; $XDG_DATA_HOME/arcadebox
- **Clipboard:** $XDG_DATA_HOME/dev.arcade.clipboard; $XDG_DATA_HOME/clipboard
- **Find:** $XDG_CONFIG_HOME/arcade-find; $XDG_DATA_HOME/arcade-find
- **Lens:** $XDG_CONFIG_HOME/arcadelens; $XDG_DATA_HOME/arcadelens
- **Look:** $XDG_CONFIG_HOME/arcade-look
- **Shelf:** $XDG_DATA_HOME/qa-p1/ArcadeShelf
- **Wheel:** $XDG_CONFIG_HOME/Arcade Wheel/Arcade Wheel

### Windows

- **Box:** %APPDATA%\dev.arcadebox.app; %APPDATA%\Arcade Box\Arcade Box\data
- **Clipboard:** %APPDATA%\dev.arcade\clipboard
- **Find:** %APPDATA%\Arcade\Arcade Find; %LOCALAPPDATA%\Arcade\Arcade Find
- **Lens:** %APPDATA%\Arcade\Arcade Lens\config; %APPDATA%\Arcade\Arcade Lens\data
- **Look:** %APPDATA%\arcade-look
- **Shelf:** %LOCALAPPDATA%\qa-p1\ArcadeShelf
- **Wheel:** %LOCALAPPDATA%\Arcade Wheel\Arcade Wheel

### macOS

- **Box:** ~/Library/Application Support/dev.arcadebox.app; ~/Library/Application Support/dev.Arcade-Box.Arcade-Box
- **Clipboard:** ~/Library/Application Support/dev.arcade.clipboard
- **Find:** ~/Library/Application Support/Arcade Find
- **Lens:** ~/Library/Application Support/dev.Arcade.Arcade-Lens
- **Look:** ~/Library/Application Support/arcade-look
- **Shelf:** ~/Library/Application Support/qa-p1/ArcadeShelf
- **Wheel:** ~/Library/Preferences/Arcade Wheel/Arcade Wheel

Arcade Link also keeps shared app manifests outside those folders: under
$XDG_DATA_HOME/arcade/apps on Linux (default ~/.local/share/arcade/apps),
~/Library/Application Support/Arcade/apps on macOS, and
%LOCALAPPDATA%\Arcade\apps on Windows. Uninstall removes the
app's manifest. Shared helper engines may remain because other Arcade apps
can use them.

See [Installing and updating](installing.md), or read
[Privacy and security](privacy.md) before removing data.
