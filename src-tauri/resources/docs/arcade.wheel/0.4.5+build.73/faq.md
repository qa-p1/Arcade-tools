---
title: Arcade Wheel questions
order: 5
since: 0.3.0
---

# Arcade Wheel questions

## Why does F8 open a wheel only while I hold it?

Wheel uses press and release as one gesture: press opens the overlay, pointer direction selects a slot, and release runs it. A release in the center cancels. You can assign a hold threshold or choose a different trigger in **Settings → Trigger**.

## Can I tap the trigger to toggle the wheel?

The default interaction is hold-to-select. A tap can open and close without running an action. Use `arcade-wheel --show` for a click-to-select overlay, or `arcade-wheel --preview` to inspect the overlay without running actions.

## How many actions can I put in a deck?

Each deck has up to eight slots, and you can create as many decks as you need. A center-gesture group can hold up to 16 actions.

## Does Wheel run shell commands?

No. A command action starts a program directly with its arguments. If you need shell syntax, set the program to `sh` and pass `-c` and the script as arguments.

## Which platforms have been exercised interactively?

Linux Wayland is used daily on Hyprland, and the project has Linux Xvfb checks. Windows and macOS are built, tested and packaged in CI, but have **not been run interactively on real Windows or macOS desktops**. Other Wayland desktops also need more real-desktop checking. See [project status](https://github.com/qa-p1/Arcade-wheel/blob/main/docs/STATUS.md).

## Can I keep using Wheel without Tools?

Yes. Tools hosts only the tray. Wheel's actions and settings stay in Wheel. If Tools quits or crashes, Wheel restores its own tray immediately; during a declared Tools restart it waits up to five seconds to avoid flashing icons. Turning off connections also restores Wheel's tray.

## What goes into a settings backup?

Preferences, decks, slots and center gestures. Credential, token, secret and device-identity fields are omitted. Commands and paths you save as actions remain part of those slots. Import saves a private copy of your current configuration first.
