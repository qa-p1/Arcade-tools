---
title: Features
order: 3
since: 0.2.0
---

# Features

Home shows installed versions, channels, uptime, memory, CPU, tray and Link state. Metrics refresh every two seconds while Home is visible. Health checks link to repair, adoption, engines and shortcut fixes.

Apps provides catalog details, bundles, install, update, repair, rollback, adoption, uninstall and app settings. Updates shows compatibility holds, sanitized notes, history and per-app automatic updates. Tools checks after 60 seconds, every six hours, and when an opened window has an hour-old check.

Automatic updates require each app's opt-in. Closed apps can update; running apps must report that they are not busy. When an app reports its last-used time, Tools waits ten minutes. Older apps without that timestamp use their busy state.

Tools can update its own standalone installation from Settings, including a Windows setup install or a macOS bundle dragged to Applications. Updates and rollbacks close and reopen Tools through a helper while your other apps keep running. Adoption also works when an app has only its install receipt.

On Windows, the first update after adoption has no retained installer for rollback. Tools shows this before updating. Later updates keep the previous installer in the cache.

Shortcuts combines Arcade globals and in-app keys, desktop binds and catalog sheets. Rebind supported Arcade globals after conflict review. Hyprland configuration is read-only; the Tools overlay uses a runtime bind. The native overlay starts on demand and exits when dismissed.

Learn reads cached/versioned and bundled guides offline. Connections explains advertised content flows and opens each app’s own connected-app settings. Engines provides detection and copyable system-package guidance, plus a confirmed Link Tesseract download where supported.

Settings includes tray hosting, per-app opt-outs, update notifications, theme, diagnostics, backup/restore and uninstall everything. Closing the window destroys its webview while tray hosting or shortcuts remain enabled.
