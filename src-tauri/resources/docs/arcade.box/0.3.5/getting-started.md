---
title: Getting started
order: 2
since: 0.2.0
---

# Getting started

## Install Arcade Box

You can install Box from **Arcade Tools → Apps**, where Tools can install, update, and remove it. Tools can also host the app’s tray icon in one Arcade icon; Box keeps its own icon when Tools is closed.

### Linux

Download the AppImage and run it. On first launch, Box asks **Install Arcade Box?**

- Choose **Install** to add Box to your app launcher, place it in **~/Applications/Arcade/**, add the **arcade-desktop** command in **~/.local/bin**, and choose whether it starts at login.
- Choose **Just run it once** to use Box without installing it.

To remove an installed copy, run **arcade-desktop --uninstall**, choose **Uninstall** from its launcher entry, or remove it in Arcade Tools. Your settings stay unless you choose to remove them. If **~/.local/bin** is not on PATH, Box shows the exact line to add for your shell.

### Windows

Run the installer. It installs for your user and does not need administrator access. It adds a Start menu entry, offers a desktop shortcut and start-at-login options, and adds **arcade-desktop** to PATH. If SmartScreen warns about the unsigned installer, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

### macOS

Open the dmg and drag Arcade Box to Applications. If you run it from the dmg or Downloads, Box offers to move itself to Applications. The app is not notarized, so right-click it and choose **Open** the first time.

## Open the Island

The default global shortcut is **Ctrl+Alt+Space**. On macOS it is **Cmd+Shift+Space**. Change it in Box’s Settings if another app already uses it.

Type a task such as **compress PDF**, **resize image**, or **JSON**. Choose the matching action, then select or enter its input. The first time you run a tool, Box explains any permission, external program, or network access it needs.

## Try a first task

Search for **Change Case**, enter a short sentence, choose uppercase, and run it. For a file task, search for **Image Format Converter** and choose an image.

## Use the command line

The installed command **arcade-desktop** opens Box, starts it in the background, restarts or quits it, and manages its installation. Run **arcade-desktop --help** to see its options. To run tools without opening the Island, use the terminal tool runtime **arcade-box** (alias **arcadebox**), which ships beside the desktop app. On Linux, installing also adds the **arcade-box** command to `~/.local/bin`; try **arcade-box search pdf**. It is documented in the repository README.
