---
title: Install and start Arcade Look
order: 2
since: 0.2.0
---

# Install and start Arcade Look

## Install with Arcade Tools

Open **Arcade Tools → Apps**, then install, update or remove Arcade Look there. Tools can show Look's tray controls inside the shared Arcade tray icon. Look keeps its own tray icon when Tools is not running.

## Install by hand

Download the current build from [Arcade Look releases](https://github.com/qa-p1/Arcade-look/releases/latest).

### Linux

Download the Arcade Look AppImage and run it. On first run, Look asks **“Install Arcade Look?”** Choose **Install** to add it to the app launcher, place it in `~/Applications/Arcade/`, add the `arcade-look` command in `~/.local/bin`, and optionally start Look at login. Choose **Just run it once** to use Look without installing it.

If `~/.local/bin` is not on your `PATH`, Look shows the exact line to add for your shell. To remove the installation, run `arcade-look --uninstall`, use the launcher's **Uninstall** action, or remove Look in Arcade Tools. Your settings stay unless you choose to remove them. `arcade-look --install-integration` remains an alias for setting up file-manager integration; GNOME Files' Space-key previewer registration is still supported.

### Windows

Run the per-user installer; no administrator account is needed. It adds a Start menu entry, offers an optional desktop shortcut and start-at-login setting, and puts `arcade-look` on `PATH`. If SmartScreen appears, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

### macOS

Open the disk image and drag Arcade Look to Applications. If you run it from the disk image or Downloads, Look offers to move itself to Applications. The app is not notarized; the first time, Control-click or right-click it and choose **Open**.

## First run

Look opens its welcome window. Review file-manager integration and connected-app settings there, then open a file from your file manager or drag a file into the window. You can also run `arcade-look path/to/file` from a terminal.

The default preview trigger depends on your platform:

- **GNOME Files on Linux:** select a file and press **Space**. Look registers as a GNOME Files previewer; Wayland does not provide a general global shortcut for other file managers.
- **Other Linux file managers:** use **Open With → Arcade Look** or configure that file manager to run `arcade-look` with the selected path.
- **Windows:** use **Space** in File Explorer when the Explorer integration is enabled, or use **Ctrl+Alt+Shift+Space** to preview the current selection.
- **macOS:** select a file in Finder and press **Control+Option+Space**. Finder's own Space preview remains available.

## Try this first

Open an image or a PDF, then use the title-bar Back and Forward buttons to revisit your browsing history. In a folder, **Left** and **Right** move to the previous or next file. Press **?** to see the shortcuts supported by the current viewer.
