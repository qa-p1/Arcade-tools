---
title: With other apps
order: 5
since: 0.2.0
---

# With other apps

Tools hosts app tray menus through Arcade Link. Apps restore their own tray as soon as Tools disconnects; their actions remain peer-to-peer. Hosting is optional and each app can keep its own icon.

Open, Settings, Restart, Quit and app extras invoke the app’s advertised Link methods. Quit all asks once when an app is busy. Tools exposes a confirmed install request to peers; it never installs silently from an incoming request.

Backup uses each app’s portable settings handoff and lists unsupported apps as not included. Restore reviews missing installations and settings before importing. Clipboard identity/history, secrets, Find’s index and caches are excluded.
