---
title: Fix common Arcade Wheel issues
order: 6
since: 0.3.0
---

# Fix common Arcade Wheel issues

## The AppImage does not start

Make sure the downloaded file is executable. If your system does not have FUSE available, run the AppImage with `--appimage-extract-and-run`. Keep it in a permanent location before enabling start at login.

## The trigger does not open the wheel

Open **Settings → Trigger** and check the status below the shortcut field. Approve the desktop's shortcut request if prompted. If F8 is reserved by the keyboard or desktop, record another shortcut; some Mac keyboards need **Fn+F8** to send F8. Check that no other Arcade app or desktop binding owns the same shortcut.

On Linux Wayland, the desktop needs an XDG GlobalShortcuts portal and the package needs its LayerShellQt overlay support. If the portal is unavailable, install or enable the desktop portal backend and try again. On Hyprland, older portal versions may ask you to bind Wheel's global shortcut target in compositor settings.

On Linux X11, most desktops offer no global shortcut service that Wheel can register with. Bind the command `arcade-wheel --show` to a key in your desktop's keyboard settings instead; it opens the wheel in click mode.

## The overlay opens but pointer selection feels wrong

Hyprland supplies Wheel with the pointer position and Wheel moves the pointer to the center when the overlay appears. Other Wayland desktops may provide only a best-effort position or may not move the pointer. Adjust **Selection sensitivity** or the **Cancel dead zone** in **Settings → Behaviour**. Multi-monitor and fullscreen handling can depend on the compositor.

## A center click does not run my group

Center gestures start unassigned. Enable and add actions to a gesture in **Settings → Center gestures**. A single click may wait for the configured click interval when a double- or triple-click can follow. Releasing the trigger completes a pending click; moving outside the center, changing decks or right-clicking cancels it.

## An Arcade action is missing or unavailable

Open **Settings → Connected apps** and check the master switch and the individual peer toggle. The picker hides input modes that the target action or its selection resolver does not support. Look file selection is not available on Linux in the current build. A saved action whose peer is missing remains in the deck with its unavailable reason.

## The overlay is blocked by a full-screen app

Some full-screen games and applications with exclusive input can intercept the shortcut or prevent the overlay from appearing. Try another trigger or leave exclusive full-screen mode. This depends on the desktop and platform.

## Windows or macOS behavior differs from the guide

Windows and macOS builds pass CI, but Arcade Wheel has **not been run interactively on real Windows or macOS desktops**. The platform backends have different limits: macOS currently supports screenshot and sleep system actions, while other system, media and compositor actions may not be available. Check the action picker and Trigger status on your platform.
