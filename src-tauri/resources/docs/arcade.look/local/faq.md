---
title: Arcade Look questions
order: 5
since: 0.1.2
---

# Arcade Look questions

## Does Look open every file type?

Look attempts a built-in viewer and falls back to a file information card and hex view when no viewer can read a file. That fallback means the file can still be inspected, but it does not guarantee a rich preview for every format.

## Can I add support for an older Office file?

Legacy `.doc` and `.ppt` need a plugin, such as the optional LibreOffice example. Look handles `.xls` natively. Look does not install LibreOffice for you.

## Why does a video or audio file fail to play?

Playback uses the codecs provided by the operating system's web engine and media stack. A file can be recognized while its codec is unsupported. On Linux, installing the distribution's GStreamer codec packages may add support; an ffmpeg command plugin can be used as a conversion fallback.

## Where are settings and plugins stored?

Settings are in `config.json` in Look's per-user configuration folder. The welcome screen links to the plugin folder; exact plugin locations and the plugin format are in the [plugin reference](../PLUGINS.md).

## Does Look register a global shortcut on Linux?

No. Wayland does not let Look register a general global shortcut or read another app's selection. GNOME Files has a dedicated Space previewer integration; other Linux file managers can use **Open With** or a custom action.

## Are Windows and macOS integrations tested on a desktop?

The Windows Explorer hook and macOS Finder integration are built and tested in CI, but they have **not been run interactively on a real Windows or macOS desktop**. The Linux path has been exercised end to end. See [current project status](../STATUS.md).
