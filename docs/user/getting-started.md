---
title: Getting started
order: 2
since: 0.2.0
---

# Getting started

Download an installer from the [Tools releases](https://github.com/qa-p1/Arcade-tools/releases). Tools never changes shell profiles, quarantine, or SmartScreen settings.

## Install

### Linux

Run the AppImage. It offers to install itself into **~/Applications/Arcade**, with a launcher entry, icons and an **arcade-tools** command. `arcade-tools --install --silent --no-launch` does the same without the dialog.

### Windows

Run the setup program. Tools installs for your user, without administrator access, into **%LOCALAPPDATA%\Programs\Arcade Tools**. Setup adds a Start menu entry and an **arcade-tools.cmd** command in **%LOCALAPPDATA%\Arcade\bin** on your user PATH. It offers **Start at login** (on by default, because Tools hosts the apps' tray icons) and an unchecked **Desktop shortcut**. A silent install, `/S`, uses the same defaults; `/STARTATLOGIN=0|1` and `/DESKTOPSHORTCUT=0|1` change them. The installer is not signed: if SmartScreen warns, choose **More info → Run anyway**.

### macOS

Open the dmg and drag Arcade Tools to Applications. If you start Tools from the dmg or the Downloads folder, it offers to move itself to Applications. The move keeps macOS quarantine, records an installation receipt, adds an **arcade-tools** command link and relaunches Tools from Applications. `arcade-tools --install --silent --no-launch`, run from the app bundle, does the same without the dialog. The app is not notarized, so the first time, Control-click (or right-click) it and choose **Open**. Tools never removes quarantine for you.

Windows and macOS installs are checked in CI only, not yet on a real desktop.

## Start at login and uninstall

Start at login is a Tools setting. On Windows, an installed copy toggles the Startup shortcut its setup program created and records the change in its receipt, so updates keep your choice. To uninstall, use Windows **Settings → Apps → Installed apps**, or `arcade-tools --uninstall` on Linux and macOS. **Uninstall everything** in Tools' Settings removes the apps Tools manages and then Tools. Settings and data stay unless you choose to remove them.

## First steps

Open Onboarding, choose apps or a bundle, review shortcut conflicts and tray/login preferences, then confirm installs. Existing manual or self-installed apps can be adopted; development builds are left in place. Open Learn for app guides.
