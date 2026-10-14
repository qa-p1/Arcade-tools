# Changelog

## 0.2.0

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
