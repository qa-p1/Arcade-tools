# Changelog

## 0.3.1 — 2026-10-18

- Same app as 0.3.0, which could not be published: the release build's
  documentation snapshot now uses the workflow's token, so GitHub's anonymous
  rate limit no longer stops it.

## 0.3.0 — 2026-10-18

- Apps started from Tools get your own environment instead of the AppImage's, so
  "Start all Arcade apps" starts Clipboard again.
- The window no longer renders blank on machines without GPU acceleration.
- Idle memory: after its window closes Tools continues in the background without
  WebKit, about 68 MiB instead of 158 MiB.
- Settings backups include Arcade Lens and Arcade Shelf; restore no longer races
  a running backup or update.
- GitHub rate limits fall back to the release files, so update checks keep working.
- Apps installed before the shared installer can be adopted; apps that removed
  themselves show as not installed; uninstalling an app also removes its extra
  terminal commands (and Look's WebView storage).
- Updates: cancelling during the backup works, auto-update no longer undoes a
  rollback, a busy app is detected before the download, and old Tools images are
  unmounted.
- Uninstalling Tools asks for confirmation and removes everything it installed
  (`--uninstall --silent` removes without asking, for scripts and terminals
  without a desktop); `--help` lists the command-line options.
- Interface: late answers no longer overwrite newer ones; clearer messages and
  confirmations; "Open all" opens apps in the background; readable update history,
  release notes and onboarding; the shortcut editor shows clashes while you type
  and requires a modifier; the page you pick during startup is kept.
- The shortcuts overlay picks up shortcut changes live on Windows and macOS too.
- Built against Arcade Link commit `2fc7c91`.

## 0.2.1 — 2026-10-15

- Restore portable settings as `file/any` inputs, which Link 0.3 requires;
  restoring Arcade Lens settings from a backup was rejected with
  `invalid_inputs`.
- Stop bundling Ubuntu's `libwayland-client` in the AppImage. On newer distributions
  (Arch, CachyOS, Fedora) it broke the host Mesa EGL and the Tools window rendered blank.

## 0.2.0 — 2026-10-14

- Replace the installer list with a Svelte hub: dashboard, catalog/bundles,
  updates, shortcuts, Learn, Connections, Engines, Settings and onboarding.
- Pin Arcade Link v0.3.0; use app-owned installers and receipts, adoption,
  compatibility holds, rollback, Tools self-update and confirmed removal.
- Compare stable updates by SemVer precedence and recorded package hashes;
  accept app receipts that omit release build metadata without repeating updates.
- Host app tray menus with per-app opt-out, update badge and immediate socket
  hand-back. Closing the window drops its webview while resident features run.
- Package the native shortcut overlay on Linux, Windows and macOS; add native
  or runtime registration, conflict checks, global rebinding and desktop readers.
- Bundle offline docs and catalog shortcut sheets, refresh exact-version guides,
  and render Markdown with a Rust sanitizer.
- Refresh catalog snapshot provenance and ecosystem guides from catalog `main`,
  while retaining labeled development guides for unreleased app documentation.
- Add visible-only process metrics, health fixes, local redacted diagnostics,
  portable settings backup and confirmed restore across isolated profiles.
- Share Windows PATH/receipt integration and canonical release metadata, with
  v0.3.0 provenance hashes. Test all three OSes on Rust 1.88 and package both binaries.

## 0.1.0

- Verified per-user install, update, repair, launch and uninstall for Arcade apps.
