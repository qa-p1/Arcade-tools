# Changelog

## 0.4.0 — 2026-10-28

Arcade design kit; one look and one set of conventions shared with every Arcade app.

### Changed

- The interface uses the Arcade design kit from Arcade Link 0.4.0: its colours,
  type scale, icons and the Inter typeface, with a new app icon and a
  monochrome tray icon (white or black to suit the panel).
- Settings follow the shared order: General, Shortcuts, Tray, Backup and
  restore, Connected apps, About (version, channel, logs folder, diagnostics and
  "Report a problem") and Uninstall last.
- Shortcuts are set by pressing the keys. A clash with another app or the desktop
  is shown before it applies, and "Reset to default" restores this system's default.
- Onboarding has three steps, can be skipped at any step, and leaves the sidebar
  once it is done; Settings › General › "Set up apps again" brings it back.
- The sidebar footer shows whether apps are connected; the "Connect apps" switch
  lives in Settings › Connected apps.
- App cards, onboarding and the tray describe each app by its one-line tagline.
- Every page shows a placeholder while loading, an empty state, and a message
  with Retry when loading fails. Messages and buttons use the same wording as
  the other apps.
- The shortcut sheet uses the kit's palette and Inter, follows Tools' Appearance
  setting, and has a hint bar (Esc Close, ↑ ↓ Move, Tab Scope, Type to filter).
- The tray menu follows the sidebar's order, in sentence case, with "Quit Arcade
  Tools" last after a separator.
- Tools' data moves to `arcade/tools` in the system's data and cache folders.
  The 0.3 folders (`arcade-tools`, `app.arcadetools`) move on first start and are
  only removed once the copy is verified.

### Added

- Settings › About: "Open logs folder" and "Copy diagnostics" with the shared
  Arcade header. Errors that would otherwise be lost go to the log folder.
- `scripts/dev`, `scripts/check` and `scripts/package` for development, and a
  single version source checked in CI.

### Fixed

- Links in the bundled guides that pointed outside the guide now open on
  GitHub, and their images are bundled; Learn has no dead links offline.
- The Shortcuts tab builds the shortcut list when it opens, instead of asking
  you to open the overlay or restart first.
- Every push to main publishes the Linux installer that Arcade Link's nightly
  ecosystem check installs, updates, repairs and removes.

## 0.3.8 — 2026-10-25

- A calmer, consistent interface across Home, Apps, Updates, Shortcuts, Learn,
  Connections, Engines, Settings and onboarding: quieter cards, clearer actions,
  shared spacing and readable light and dark themes. Home keeps issues collapsed,
  app details use a focused layout, and Connections explains each workflow.
- Learn opens guides in a dedicated reader with bounded text, tables and images,
  and requests from other pages reliably open the selected guide.
- Opening Tools waits for the interface to be ready before showing the window,
  avoiding the brief blank flash, with a fallback if loading fails.
- Engine checks run in parallel and start quietly after launch. Engines shows
  placeholders while checking, uses the catalog's purpose labels, and offers a
  retry after a failed check instead of retrying forever.

## 0.3.7 — 2026-10-25

- The shortcut sheet is calmer and easier to scan.
  Each shortcut is one line with its keys on the right, so a column holds about
  three times as many; a detail line shows only when it says something new.
  The panel, search field and keys have rounded corners and quiet borders,
  scopes are one segmented control, headings use a real medium weight, and a
  conflict is a small amber dot. A single scope keeps a narrow column so its
  keys sit beside their titles. The app glyph is now placed correctly on
  HiDPI screens.

## 0.3.6 — 2026-10-19

- Updating Tools while it runs now switches to the new version. Running a
  newer Tools you downloaded (or `--install` from the command line) only brought
  the running Tools forward, so the update offer never showed, and an install
  kept the old version running until you logged out. The running Tools is now
  asked to quit first; a busy Tools stays, and the new version starts the next
  time Tools starts.

## 0.3.5 — 2026-10-19

- Opening the Tools window, Settings or the Uninstall review no longer crashes
  Tools when it runs from its AppImage (0.3.2 to 0.3.4). Since 0.3.2 Tools
  moved to the folder you started it from, and the web view behind every Tools
  window then could not find its helper programs, which are looked up relative
  to the AppImage. Tools now stays in the AppImage's folder; programs it starts
  still begin in your home folder.
- The package check in CI now opens the Tools window from the AppImage and fails
  if Tools quits.

## 0.3.4 — 2026-10-19

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.

## 0.3.3 — 2026-10-19

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.

## 0.3.2 — 2026-10-18

- Updating or installing from your Downloads folder, closing the window (Tools
  restarts lean in the background) and starting other Arcade apps no longer
  leave the old AppImage mounted and its runtime (about 20 MB) running. A
  program the app started inherited an open handle on its AppImage mount and
  kept it busy.
- The launcher's "Open Settings" action opens Settings instead of Home
  (`arcade-tools --settings`).

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
