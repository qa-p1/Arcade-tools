# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] — not yet promoted (release candidates on the test channel from 2026-11-08)

### Added
- Signed updates: a release is trusted only through its `manifest.json`, signed with ed25519 and bound to the app,
  channel and tag. Release candidates live on a hidden test channel that Tools never offers.
- Delta updates: only the changed chunks of a new version are downloaded, assembled from the installed copy and kept
  packages, and verified chunk by chunk; any mismatch falls back to a full download, which resumes after an
  interruption.
- A release the catalog marks as bad is no longer offered, and Tools offers to roll back an installed copy.
- Apps on the Nightly channel show a badge; switching back to Stable backs up the app's data first.
- Third-party notices in Settings › About, and a security policy (`SECURITY.md`).

### Changed
- Arcade Link 1.0.3: the 1.0 runtime, release manifests and update primitives. 1.0.2 trims read-only data too
  (background memory about 40 → 15–18 MB PSS) and makes uninstall leave no lock file or empty icon/desktop cache;
  1.0.3 pages out every private mapping once the window closes (about 36 → 12 MB PSS while closed). 1.0.4 runs the
  desktop end-to-end tests against the app outside its checkout and adds the performance budgets CI checks;
  1.0.5 makes the Tier 2 smoke wait for the app to report it is running and counts wakeups per thread.
- Tools trims its memory again once startup settles and after each background update check.
- Tools runs from an unpacked copy of its AppImage and trims its memory, and the WebView's, while the window is open
  and after it closes; unused unpacked copies are removed.
- Tools' Linux desktop id and window class are `dev.arcade.Tools`, with a scalable and a symbolic icon.
- Details reads "Update to vX" only when an update exists, "Up to date" when current and "Check for updates" when
  the state is unknown.

### Fixed
- Design pass: spacing is on the kit's scale everywhere, error messages read "Couldn't …" with what to do
  next, and when the app list can't load, Home says so instead of "Loading your apps…". A shortcut-watching
  warning no longer points to a Rebuild button that doesn't exist.
- Updating to a release candidate on the test channel no longer fails in the installer, and an update that fails
  to install is not retried every minute; automatic updates wait for a newer release. Tools' own automatic update
  also respects a rollback now.
- A self-installed AppImage at the same version as a published release with other bytes (the next release
  candidate) is offered that release; before, only the version was compared when Tools had not installed it.
- Tools started at login updates itself at the first check when automatic updates are on for it; a background
  start used to count as use, which put the update off by six hours.
- `--show-shortcuts` waits for the overlay, so the AppImage stays mounted while it is open.
- The packaged shortcut overlay no longer bundles libxkbcommon, which broke typing on newer X11 systems.
- Engine version checks no longer time out when GTK takes over child-process signals.
- The legacy data folder is retired once, keeping a copy of any conflicting file under `archived-legacy`, instead of
  being kept and logged on every start.
- Home names Arcade Tools, never asks for optional engines, and shows backup progress.
- `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` is honoured for release testing, and a stored or restored "test" channel is
  refused.

### Security
- Dependencies with advisories updated (rustls, rustls-webpki, ammonia, pulldown-cmark, zip 9); `cargo deny` runs in CI.

## [0.4.6] — 2026-11-05

### Fixed
- Dispatch Linux notifications on a blocking worker, avoiding the notification plugin’s nested Tokio-runtime panic.
  Native tray checks fail on any logged Rust panic.
- Release closed Linux WebViews by disconnecting their retiring IPC callback, breaking Wry’s reference cycle without
  restarting the app. Native package checks require zero renderer processes after each close/reopen cycle.
- Automatically refresh registry metadata whose cached version disagrees with the installation receipt, even when its
  executable path is correct. Recover it from the verified installed app without restarting it or rewriting healthy
  launchers/login choices.
- Gate the actual AppImage on version-only registry damage and add a process/filesystem regression for this
  live-audit finding.

### Changed
- Keep the same resident process when windows close; remove the hidden 20-second memory-cleanup restart. Destroy the
  WebView and reclaim memory in place while preserving tray hosting, shortcuts and maintenance. Explicit Restart and
  updates still work.

## [0.4.5] — 2026-11-03

### Added
- Repair missing Arcade desktop launchers, command links and registry metadata automatically at startup and after
  integration files change, using the installed app offline; recover registry metadata from its validated read-only
  manifest.
- Back up affected integration, preserve customized launchers and disabled login choices, verify installed bytes and
  the repair result, and bound repair commands to 20 seconds. An unchanged failure is not retried continuously; one
  bad receipt does not block other apps.
- Add repeated-damage, ownership, future-receipt, disabled-login and actual AppImage maintenance gates. Windows
  repairs that require the native setup program remain explicit.

### Changed
- Refresh the Linux desktop application cache at startup and after launcher changes, including healthy customized
  launchers; bound optional cache commands and avoid idle polling.

### Fixed
- Honor the installed Linux login entry at release startup, including disabled or removed entries, instead of
  reapplying an older tray preference.
- Keep Tools usable if optional filesystem watching fails, and refresh Home health after automatic repairs.

## [0.4.4] — 2026-11-03

### Fixed
- Fix self-updating from pre-overhaul Tools: defer folder migration until the resident starts, preserving the
  executing AppImage and the old helper's transaction.
- Verify actual installer bytes when staging in the legacy cache in the packaged release gate.

## [0.4.3] — 2026-11-01

### Fixed

- Start at login for Arcade Wheel 0.5.0 and later uses `dev.arcade.Wheel.desktop`,
  the entry Wheel itself reads from 0.5.0, and `arcade-wheel.desktop` for older
  versions, so Tools and Wheel's own Settings agree on whether it starts at login.

## [0.4.2] — 2026-11-01

### Fixed

- Start at login finds and writes each app's login entry under the name that app
  reads: `dev.arcade.<App>` on Linux and macOS for Find from 0.5.0 and for Lens,
  Look and Shelf from 0.4.0, the earlier name for older versions. Switching it on
  replaces the entry under the other name, so the app starts once and its own
  Settings show the same state; switching it off removes both. Launch agents are
  labelled with their file name.

## [0.4.1] — 2026-10-28

### Fixed

- Installing, repairing, uninstalling and detecting an app also covers the desktop
  entry and window class it uses from Arcade Link 0.4.0 on (`dev.arcade.<App>`, such
  as `dev.arcade.Box`), as well as its earlier name. Apps released on Link 0.4.0
  are found and removed cleanly.

## [0.4.0] — 2026-10-28

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

## [0.3.8] — 2026-10-25

### Changed

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

## [0.3.7] — 2026-10-25

### Changed

- The shortcut sheet is calmer and easier to scan.
  Each shortcut is one line with its keys on the right, so a column holds about
  three times as many; a detail line shows only when it says something new.
  The panel, search field and keys have rounded corners and quiet borders,
  scopes are one segmented control, headings use a real medium weight, and a
  conflict is a small amber dot. A single scope keeps a narrow column so its
  keys sit beside their titles. The app glyph is now placed correctly on
  HiDPI screens.

## [0.3.6] — 2026-10-19

### Fixed

- Updating Tools while it runs now switches to the new version. Running a
  newer Tools you downloaded (or `--install` from the command line) only brought
  the running Tools forward, so the update offer never showed, and an install
  kept the old version running until you logged out. The running Tools is now
  asked to quit first; a busy Tools stays, and the new version starts the next
  time Tools starts.

## [0.3.5] — 2026-10-19

### Fixed

- Opening the Tools window, Settings or the Uninstall review no longer crashes
  Tools when it runs from its AppImage (0.3.2 to 0.3.4). Since 0.3.2 Tools
  moved to the folder you started it from, and the web view behind every Tools
  window then could not find its helper programs, which are looked up relative
  to the AppImage. Tools now stays in the AppImage's folder; programs it starts
  still begin in your home folder.
- The package check in CI now opens the Tools window from the AppImage and fails
  if Tools quits.

## [0.3.4] — 2026-10-19

### Fixed

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.

## [0.3.3] — 2026-10-19

### Fixed

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.

## [0.3.2] — 2026-10-18

### Fixed

- Updating or installing from your Downloads folder, closing the window (Tools
  restarts lean in the background) and starting other Arcade apps no longer
  leave the old AppImage mounted and its runtime (about 20 MB) running. A
  program the app started inherited an open handle on its AppImage mount and
  kept it busy.
- The launcher's "Open Settings" action opens Settings instead of Home
  (`arcade-tools --settings`).

## [0.3.1] — 2026-10-18

### Fixed

- Same app as 0.3.0, which could not be published: the release build's
  documentation snapshot now uses the workflow's token, so GitHub's anonymous
  rate limit no longer stops it.

## [0.3.0] — 2026-10-18

### Changed

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

## [0.2.1] — 2026-10-15

### Fixed

- Restore portable settings as `file/any` inputs, which Link 0.3 requires;
  restoring Arcade Lens settings from a backup was rejected with
  `invalid_inputs`.
- Stop bundling Ubuntu's `libwayland-client` in the AppImage. On newer distributions
  (Arch, CachyOS, Fedora) it broke the host Mesa EGL and the Tools window rendered blank.

## [0.2.0] — 2026-10-14

### Added

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

## [0.1.0] — 2026-10-10

### Added

- Verified per-user install, update, repair, launch and uninstall for Arcade apps.
