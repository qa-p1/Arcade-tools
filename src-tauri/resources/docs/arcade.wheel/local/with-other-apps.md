---
title: Use Arcade Wheel with other apps
order: 4
since: 0.2.0
---

# Use Arcade Wheel with other apps

Wheel can invoke actions published by installed Arcade peers and can receive actions offered by other apps. Open **Settings → Connected apps** to enable or disable connections. A saved slot remains in place if its app is unavailable, with a reason shown in the editor.

## Arcade Box

Choose a Box tool or a saved pipeline in the Arcade action picker. When the action accepts input, Wheel can provide clipboard content, a Lens selection or a Look file selection when that resolver is available. Box owns the transformation and reports progress and results back to Wheel.

## Arcade Clipboard

Use a compatible Clipboard action from a slot, or choose clipboard input for an action that accepts it. Wheel snapshots the current clipboard only when you invoke the action; it does not alter the clipboard with returned results. Clipboard applies its own device and privacy rules.

## Arcade Find

Compatible actions published by Find appear in the Arcade action picker and can be saved into a slot. Find owns search and its results; Wheel shows the action's status and result message when it runs.

## Arcade Lens

Compatible Lens actions can be saved in slots. For another action that accepts a Lens selection, Wheel asks Lens to capture first and passes only supported returned content to the target action.

## Arcade Look

Compatible Look actions can be saved in slots. For a target that accepts files, Wheel can ask Look to resolve the current file-manager selection. The resolver is hidden on Linux because Look does not currently publish it there.

## Arcade Shelf

Compatible Shelf actions can be saved in slots, including actions that open Shelf or use a named collection. Shelf remains responsible for its items and collections.

## Arcade Tools

Tools can install, update or remove Wheel from its **Apps** tab. When Tools is running, it can host Wheel's tray controls inside the shared Arcade tray icon; Wheel keeps its own tray icon when Tools is closed. The Connected Apps page can also offer a **Get** link for an Arcade app that is not installed.
