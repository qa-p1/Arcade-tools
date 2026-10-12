---
title: Install and start Arcade Wheel
order: 2
since: 0.2.0
---

# Install and start Arcade Wheel

## Install with Arcade Tools

Open **Arcade Tools → Apps**, then install, update or remove Arcade Wheel there. Tools can show Wheel's tray controls inside the shared Arcade tray icon. Wheel keeps its own tray icon when Tools is not running.

## Install by hand

Download the current build from [Arcade Wheel releases](https://github.com/qa-p1/Arcade-wheel/releases).

### Linux

Download the Arcade Wheel AppImage and run it. On first run, Wheel asks **“Install Arcade Wheel?”** Choose **Install** to add it to the app launcher, place it in `~/Applications/Arcade/`, add the `arcade-wheel` command in `~/.local/bin`, and optionally start Wheel at login. Choose **Just run it once** to use Wheel without installing it.

If `~/.local/bin` is not on your `PATH`, Wheel shows the exact line to add for your shell. To remove the installation, run `arcade-wheel --uninstall`, use the launcher's **Uninstall** action, or remove Wheel in Arcade Tools. Your settings stay unless you choose to remove them. Wheel's Wayland overlay also needs a compositor that supports the layer-shell integration; global shortcuts use the desktop's GlobalShortcuts portal.

### Windows

Run the per-user installer; no administrator account is needed. It adds a Start menu entry, offers an optional desktop shortcut and start-at-login setting, and puts `arcade-wheel` on `PATH`. If SmartScreen appears, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

### macOS

Open the disk image and drag Arcade Wheel to Applications. If you run it from the disk image or Downloads, Wheel offers to move itself to Applications. The app is not notarized; the first time, Control-click or right-click it and choose **Open**.

## First run

Start Arcade Wheel from your app launcher or run `arcade-wheel`. Settings opens with the first deck, **Applications**, populated with up to six installed graphical apps. Keep Wheel running in the background; closing Settings does not quit it.

The default global trigger is **F8**. Your desktop may ask you to approve it. To invoke an action, hold F8, move toward a slot, and release. Releasing over the center closes the wheel without running an action. Scroll while holding to switch decks; right-click or **Escape** cancels.

## Try this first

Open **Settings → Wheel**, choose an empty slot and save an action. Then hold F8 and aim toward that slot. The center label shows the selected action before you release.
