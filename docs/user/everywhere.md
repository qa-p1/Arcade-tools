---
title: Everywhere
order: 6
since: 1.1.0
---

# Everywhere

Arcade's actions show up in your file manager's right-click menu, so you can compress, extract, convert or read text
from a file without opening the app first. Tools ships a small companion program, the **Arcade helper**, that does
this. Tools starts it at login and keeps it up to date; you don't install it separately.

## Using it

Right-click one or more files and choose **Arcade…** (or a pinned action, below). Arcade lists only the actions that
fit the files you picked, from the apps you have installed. Some actions ask a question or two first; the rest start
right away.

A small progress window shows each job. You can minimise it, cancel a job, and open the result or show it in its
folder when it's done. It closes by itself a few seconds after the last job finishes, unless the pointer is on it.

If an action needs an app you don't have, Arcade says which one and offers to install it through Tools. Tools always
asks first.

## Where it works

| System | Where |
|---|---|
| Linux | "Open With" in every file manager, plus Files (Nautilus), Caja, Dolphin, Nemo and Thunar menus |
| Windows | File Explorer's right-click menu (under "Show more options" on Windows 11) |
| macOS | Coming in a later update |

Browsers need the Arcade browser extension, which comes in a later update.

## The Everywhere page

Open **Everywhere** in Tools to:

- see whether the helper is running, start it if not, and see which file managers and browsers it reached;
- **pin** up to 8 actions so they show at the top level of the menu, in the order you set (everything else stays
  under "Arcade…");
- choose the folder results are saved to, or have the progress window **ask every time**;
- pick a default when two apps can do the same job. Until you pick one, the menu shows both. You can also choose
  **Always use this** in the menu itself.

## Removing it

Uninstalling Tools removes the helper, its menu entries and its browser connections. Menu entries you added yourself
(for example your own Thunar actions) are left alone.
