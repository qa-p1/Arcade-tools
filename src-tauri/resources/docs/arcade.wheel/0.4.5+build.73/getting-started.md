---
title: Install and start Arcade Wheel
order: 2
since: 0.3.0
---

# Install and start Arcade Wheel

## Install with Arcade Tools

Open **Arcade Tools → Apps**, then install, update or remove Arcade Wheel there. Tools can show Wheel's tray controls inside the shared Arcade tray icon. Wheel keeps its own tray icon when Tools is not running.

## Install by hand

Download the current build from [Arcade Wheel releases](https://github.com/qa-p1/Arcade-wheel/releases).

### Linux

Download the Arcade Wheel AppImage, make it executable and run it. Before Settings opens, choose **Install** to put Wheel in `~/Applications/Arcade/Arcade-Wheel.AppImage`, add its launcher and icons, and create the `~/.local/bin/arcade-wheel` command. **Start at login** is optional and off by default; **Remove the downloaded file** is on by default. You can also choose **Just run it once** or **Don't ask again for this file**. A changed download is treated as a new file.

An older installed copy offers **Update** and keeps your settings. An equal or newer copy offers **Open installed**. Tools-managed updates go through Tools when it is installed. If your command folder is not on PATH, the dialog shows the exact line for your shell; Wheel never edits shell profiles.

For scripted installation use `./ArcadeWheel.AppImage --install --silent --no-launch`. To repair launchers or a moved AppImage, run the installed or moved copy with `--repair`. To remove Wheel, use `arcade-wheel --uninstall` or the launcher's **Uninstall** action. Settings stay unless you pass `--remove-data`. `--integration-status` prints the receipt, entries and PATH status without changing them. Development builds and background/Link launches never prompt.

Wheel's Wayland overlay needs a compositor that supports the layer-shell integration; global shortcuts use the desktop's GlobalShortcuts portal.

### Windows

Run the per-user installer; no administrator account is needed. It adds a Start menu entry, a PATH command and an install receipt, and offers optional desktop and start-at-login shortcuts. If SmartScreen appears, choose **More info → Run anyway**. Uninstall from **Settings → Apps**. The CLI `--uninstall` uses the native uninstaller recorded in the install receipt. The extra ZIP download is portable and does not install integration.

### macOS

Open the disk image and drag Arcade Wheel to Applications. When started from Downloads, a mounted disk image or a translocated location, Wheel offers **Move to Applications** before opening Settings. It copies to `/Applications` when writable, otherwise `~/Applications`, preserves quarantine and relaunches from that copy. It writes an install receipt and creates a CLI symlink when `~/.local/bin` is already on PATH; otherwise it offers the exact command. Wheel keeps its native login-item behavior. The app is not notarized; the first time, Control-click or right-click it and choose **Open**.

## First run

Start Arcade Wheel from your app launcher or run `arcade-wheel`. Settings opens with the first deck, **Applications**, populated with up to six installed graphical apps. Keep Wheel running in the background; closing Settings does not quit it.

The default global trigger is **F8**. Your desktop may ask you to approve it. To invoke an action, hold F8, move toward a slot, and release. Releasing over the center closes the wheel without running an action. Scroll while holding to switch decks; right-click or **Escape** cancels.

## Try this first

Open **Settings → Wheel**, choose an empty slot and save an action. Then hold F8 and aim toward that slot. The center label shows the selected action before you release.
