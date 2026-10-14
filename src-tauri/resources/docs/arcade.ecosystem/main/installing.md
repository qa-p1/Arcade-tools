---
title: Installing and updating
order: 2
---

# Installing and updating

Arcade Tools is optional. Install and update apps with Tools, or use each
app's release package directly. Apps continue to work without Tools.

## Install with Arcade Tools

Open Tools, choose an app or a catalog bundle, then choose **Install**. Tools
installs to your user account; it does not need administrator access. You can
choose the stable or nightly channel for each app. A request from another
Arcade app opens a confirmation in Tools; it never installs silently.

## Install without Tools

Choose an app's GitHub Releases page:
[Box](https://github.com/qa-p1/Arcade-box/releases),
[Clipboard](https://github.com/qa-p1/Arcade-clipboard/releases),
[Find](https://github.com/qa-p1/Arcade-Find/releases),
[Lens](https://github.com/qa-p1/Arcade-lens/releases),
[Look](https://github.com/qa-p1/arcade-look/releases),
[Shelf](https://github.com/qa-p1/Arcade-Shelf/releases), or
[Wheel](https://github.com/qa-p1/Arcade-wheel/releases).

### Linux

Download the app's self-installing AppImage and run it. The first run asks
**Install Arcade <App>?** Choose **Install** to place it in
~/Applications/Arcade/, add it to your application launcher, and install its
arcade-x command in ~/.local/bin. You can enable start at login there.
Choose **Just run it once** for a one-off run without installing it.

If the terminal says arcade-x cannot be found, add ~/.local/bin to the current
shell's PATH and open a new terminal. These examples affect only the current
shell; Arcade never edits shell startup files.

    fish:    set -gx PATH "$HOME/.local/bin" $PATH
    bash:    export PATH="$HOME/.local/bin:$PATH"
    zsh:     export PATH="$HOME/.local/bin:$PATH"

### Windows

Run the per-user installer. It needs no administrator access and adds the app
to the Start menu. The installer can also add a desktop shortcut and enable
start at login. The arcade-x command is available from
%LOCALAPPDATA%\Arcade\bin.

The installers are unsigned. If SmartScreen appears, check that you got the
installer from the Arcade release page, then choose **More info → Run anyway**
if you want to continue.

### macOS

Open the disk image and follow its **Move to Applications** offer. The app is
not notarized. To open it, Control-click or right-click the app and choose
**Open**, then confirm the macOS prompt. Arcade does not remove quarantine or
change Gatekeeper settings for you.

## Updating

Tools notifies you once per version when a newer version is available. Choose
**Update** to download and install it. Automatic updates are optional for each app. Tools
keeps the immediately previous version available for a one-step rollback.

Without Tools, run the newer AppImage or installer. It offers to update the
installed app while keeping its settings and data. On macOS, open the newer
disk image and replace the app in Applications when prompted.

The catalog marks Windows and macOS app support as **ci**, not **verified**:
the packages and code are built or tested in CI, but have not been run on real
Windows or macOS desktops. Linux entries marked **verified** have had a real
desktop run; Arcade Find is marked **ci** on Linux.

See [uninstalling and your data](uninstalling.md), the
[FAQ](faq.md), and [privacy and security](privacy.md).
