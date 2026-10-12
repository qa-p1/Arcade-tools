---
title: Arcade Wheel questions
order: 5
since: 0.2.0
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

Linux Wayland is used daily on Hyprland, and the project has Linux Xvfb checks. Windows and macOS are built, tested and packaged in CI, but have **not been run interactively on real Windows or macOS desktops**. Other Wayland desktops also need more real-desktop checking. See [project status](../STATUS.md).
