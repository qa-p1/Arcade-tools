---
title: Frequently asked questions
order: 8
---

# Frequently asked questions

## Do I need Arcade Tools?

No. Each app works on its own and can be installed or updated from its release
page. Tools adds a place to manage apps, channels, updates, and tray icons.

## Why do I not see another app's action?

The other app may not be installed, may have Link turned off, or may not
support that input or platform. Open **Connected apps** in both apps and check
the master switch and the per-app toggle. An unavailable action stays hidden.

## Does Arcade Link send my files to the internet?

No. Link connects apps running as your user on the same computer. Clipboard
sync is separate: it sends selected or captured clips to devices you paired.
See [Privacy and security](privacy.md).

## Can I install an app without Tools?

Yes. Use the app's release package. On Linux, run its AppImage and choose
**Just run it once** or install it. Windows has a per-user installer; macOS
uses a disk image. See [Installing and updating](installing.md).

## Will uninstalling delete my settings?

No, not by default. Tools keeps app data unless you select **Remove settings
and data**. Review the folders shown before confirming. See
[Uninstalling and your data](uninstalling.md).

## Can Tools update an app I installed myself?

Tools can launch an app it discovers outside its managed installs, but it
cannot update or remove that copy as a managed installation. Install the app
through Tools if you want Tools to manage its updates and uninstall.

## Why does Windows or macOS show a warning?

The installers and apps are unsigned or not notarized. SmartScreen and
Gatekeeper may show a warning. Arcade does not bypass these checks. See
[Installing and updating](installing.md) for the steps on each system.

## Does Arcade Tools change my window manager shortcuts?

No. Its shortcut sheet can read supported Hyprland configurations and display
their bindings, but does not edit them. You can edit a displayed label in
Tools without changing the actual binding. See [Shortcuts](shortcuts.md).

## Are all platform flows tested on real computers?

No. The catalog marks Windows and macOS app support as **ci**; those builds
and tests run in CI but not on real desktops. Linux apps marked **verified**
have had a real desktop run. Arcade Find is marked **ci** on Linux too.
