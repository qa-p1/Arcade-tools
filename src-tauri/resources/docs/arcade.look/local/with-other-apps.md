---
title: Use Arcade Look with other apps
order: 4
since: 0.1.2
---

# Use Arcade Look with other apps

Arcade Link lets Look offer file preview, inspection and selection resolution to compatible Arcade apps. Look also shows actions supplied by installed peers. Manage these connections in **Settings → Connected apps**; actions appear only when the peer supports the current content and is enabled.

## Arcade Box

Send a compatible file to a Box tool, preset or saved pipeline from Look's **A** Actions menu. Box owns the transformation. Returned files open in Look, so you can inspect the result without leaving the preview flow.

## Arcade Clipboard

Send a supported file or image to your devices through a Clipboard action. The peer applies its device and privacy rules; Look does not keep a clipboard history.

## Arcade Lens

Send a supported image for analysis, or send the current PDF page when the Lens action supports it. Lens handles capture, recognition and analysis.

## Arcade Wheel

Use **Add to Wheel** on a compatible preview action to save it in a Wheel slot. Wheel can invoke Look's file-selection resolver on platforms where Look exposes that resolver. Linux Wayland currently has no selection resolver, so that Wheel input mode is not offered there.

## Arcade Shelf

Use **Add to Shelf** to keep a file reference in a Shelf collection. Shelf owns the collection and decides how it stores references or temporary content.

## Arcade Find

Find appears in Connected Apps for connection status. It does not currently contribute an action to Look's Actions menu.

## Arcade Tools

Tools can install, update or remove Look from its **Apps** tab. When Tools is running, it can host Look's tray controls inside the shared Arcade tray icon; Look keeps its own tray icon when Tools is closed.
