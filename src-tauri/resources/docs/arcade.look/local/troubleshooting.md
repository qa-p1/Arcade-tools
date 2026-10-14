---
title: Fix common Arcade Look issues
order: 6
since: 0.1.2
---

# Fix common Arcade Look issues

## Space does nothing in GNOME Files

Check that GNOME Files integration is enabled in Look's settings. You can rerun setup with **Set up file manager integration** or `arcade-look --install-integration`, then restart GNOME Files if it was already open. Look's first-run install choice and file-manager integration are separate: **Just run it once** does not install the app, but integration can still be set up later.

## A different Linux file manager does not open the preview

On Wayland, Look cannot register a general global shortcut or resolve the current selection. Use **Open With → Arcade Look** or configure a custom action with the `arcade-look` command and selected file path.

## The preview opens but playback or rendering is blank

Try opening the same file in its normal application. Video and audio codecs come from the operating system; install a compatible system codec or use a conversion plugin. For image formats such as HEIC or JPEG XL, support may depend on the installed decoder. Look falls back to file information when a renderer cannot decode the content.

## A document or presentation has no preview

Legacy binary `.doc` and `.ppt` files require an optional plugin. Install and configure a trusted converter such as the LibreOffice example in `examples/plugins`, then restart Look. Other files may fall back if the OS renderer or format parser cannot read them.

## Finder or Explorer does not respond to its shortcut

Confirm the platform integration is enabled in Look's settings, then relaunch the file manager. On macOS, use **Control+Option+Space**; Finder's own Space shortcut remains Apple's Quick Look. Windows Explorer integration has been built and checked in CI but has **not been run interactively on a real Windows desktop**. macOS Finder integration likewise has **not been run interactively on a real Mac**. If it still fails, use **Open With** or open the file with Look directly.

## Arcade actions are missing

Open **Settings → Connected apps** and check the master connection and the individual peer. A peer may omit an action when it is unavailable for that file type, platform or input. Restarting the peer or Look refreshes a connection that has stopped responding.
