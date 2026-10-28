---
title: Troubleshooting
order: 7
since: 0.2.0
---

# Troubleshooting

Finish busy app work before retrying updates. Enable Arcade Link in an app to use hosted menus, rebinding or settings backup. Old app versions without optional methods offer their own settings instead.

On GNOME/KDE Wayland, bind `arcade-tools --show-shortcuts` through desktop keyboard settings. The overlay uses an All-app picker where focused-app detection is unavailable. Sway, KDE and GNOME shortcut readers are fixture-tested and not run on a real desktop here.

Windows/macOS behavior is built and tested in CI, not verified on a real desktop. Unsigned installers retain the platform’s normal trust prompts. Linux X11 checks use a private D-Bus/Xvfb session.

If start at login is unavailable, install to a permanent location first. Tools refuses temporary paths and backs up its generated login entries before changes. Use Report a problem in Settings for a local redacted archive.
