# Arcade Tools 0.4.6 candidate

## Stable resident process (2026-11-03)

The owner explicitly rejected background app replacement on window close or
memory cleanup. Tools' Linux-only 20-second lean restart is removed; closing
destroys its WebView and reclaims unused pages in the same process. Only an
explicit Restart or the update flow replaces it. Packaged CI and the exact
release AppImage must keep the same PID across three close/reopen cycles,
including beyond the old timer, require zero retired renderer processes, and
still honor explicit Restart and Quit. A Linux WebView destruction hook breaks
Wry's IPC reference cycle on that view's own content manager. It leaves shared
contexts and other live windows alone; no renderer process is killed.

## Automatic version metadata repair

The live-session audit found Clipboard 0.3.1 installed correctly while its
registry cache still advertised 0.2.5. Automatic maintenance only compared ID
and executable path, so this version-only drift was missed. Compare the cached
version with the installation receipt using the existing release version policy;
recover metadata from the verified installed read-only manifest. Healthy custom
launchers, disabled login and Link-off choices remain preserved.

The new process/filesystem regression fails on 0.4.5. The real AppImage gate now
also corrupts only the registry version and requires automatic restoration with
no Fix request, app restart or executable replacement. Final packaged CI and
release acceptance must pass before this candidate is counted as shipped.

# Arcade Tools 0.4.5 candidate

## Automatic integration maintenance (2026-11-03)

The owner requested click-free repair of routine Arcade integration drift.
Tools now runs an offline startup pass and listens for OS filesystem events on
its receipt, registry, launcher, command and login-entry directories. No idle
polling or update opt-in is required. Closing the window keeps maintenance
resident even when tray hosting and shortcuts are off; explicit Quit still exits. It repairs receipt-owned missing launchers,
command links and registry metadata through the permanently installed app's
`--repair`. Missing registry metadata comes from the installed app's read-only
`--arcade-manifest`, checked against its receipt; it never downloads/reinstalls a release or restarts a running app.

Linux desktop application caches are refreshed at startup, after launcher changes
and after repairs, including healthy customized entries. Optional cache helpers
share the 20-second process limit; cache writes do not trigger a polling loop.

Affected files and receipts are backed up under `maintenance/backups/` in
Tools' data folder. Healthy customized launchers, disabled login entries and
unknown receipt fields survive. Development/transient executables, foreign
entries and changed installed hashes are not executed or overwritten. Missing
login entries remain off. Repairs time out after 20 seconds, unchanged faults
are attempted once per process, and unreadable receipts do not block other
apps. Failed watcher setup is nonfatal and retains a startup maintenance pass.

The packaged gate also exposed a release-only startup path that reapplied saved
tray login preferences. Linux startup now reads the installed login choice,
including disabled or removed entries, without rewriting it. The failure was
reproduced locally with debug assertions disabled before the fix; the same
release-path maintenance gate passes afterward. The full local check now passes
211 Rust tests and 22 UI checks, including Clippy and the startup login regression.
The final AppImage must still pass packaged CI and the release acceptance gate.

Native Windows integration that requires its setup program is reported for
explicit repair, not silently reinstalled. The actual Linux AppImage gate must
restore two separate launcher/CLI deletions while resident, without a Fix/API
request, recover missing registry metadata, and refresh a healthy custom launcher's MIME cache while preserving a disabled login choice. It must pass in CI before
release; native local and final CI evidence will be recorded here.

# Arcade Tools 0.4.4 status

## 0.4.4 candidate (2026-11-03)

Reproduced the owner's two 0.3.8 → 0.4.3 checksum rollbacks in a disposable
profile using the actual released helper. The new installer's startup migrated
its executing AppImage out of the legacy cache; Link consequently installed
the inner ELF instead of the verified AppImage (24 MB instead of 93 MB).
Installer/helper and informational commands now defer migration; normal resident
startup still migrates. The real package gate checks a legacy-cache installer
and installed SHA-256. No checksum or rollback checks are weakened.

Full CI and the packaged upgrade check are required before publishing.

# Arcade Tools 0.4.3 status

## 0.4.3 verification (2026-11-01)

- Release target: `v0.4.3`, a patch release:
  - start at login for Arcade Wheel 0.5.0 and later uses `dev.arcade.Wheel.desktop`
    (`autostart_rename`), the earlier `arcade-wheel.desktop` before; the Find onboarding
    test asserts both;
  - Arcade Link v0.4.15 (was v0.4.0) and its design kit, whose small text, badges and links
    use the readable inks (4.5:1 in both themes); the vendored release and installer files
    are byte-identical between the two tags;
  - group titles and the onboarding card captions use `muted` instead of `faint`;
  - `.editorconfig`.
- `scripts/check` on Linux: typecheck, build, UI checks, 39 vendored files, versions,
  shortcut sheet, docs snapshot, **0 broken** doc links, fmt, Clippy `-D warnings`, and the
  workspace tests in Link's disposable session **200 passed, 0 failed, 2 ignored**.
- Tools' update code fed the real published releases (temporary test, not committed):
  Box 0.3.5 → 0.4.0, Clipboard 0.2.5 → 0.3.1 and Look 0.3.5 → 0.4.0 are offered as updates,
  and every installer for Linux, Windows and both Mac architectures verifies against its
  manifest.

## 0.4.2 verification (2026-11-01)

- Release target: `v0.4.2`, a fix release: start-at-login entries use the name the
  installed app version reads (`Paths::autostart_for`); both names count as on and
  are cleared when switched off. New test
  `login_entries_move_to_the_name_the_installed_version_reads`; the Find and Shelf
  onboarding tests assert the new and earlier names.
- `cargo fmt --check` and workspace Clippy `-D warnings` clean; workspace tests in
  Link's disposable session **200 passed, 0 failed, 2 ignored**.

## 0.4.1 verification (2026-10-28)

- Release target: `v0.4.1`, a fix release: desktop entries and window classes are
  looked up under the app's Link 0.4.0 OS identifier (`dev.arcade.<App>`) as well
  as its earlier name. New unit test `integration_candidates_cover_the_old_and_new_desktop_ids`.
- Core Clippy `-D warnings` clean; core tests in Link's disposable session **110 passed, 0 failed**.

## 0.4.0 verification (2026-10-28)

- Release target: `v0.4.0`, on the Arcade design kit from Arcade Link v0.4.0. See CHANGELOG.md.
- `scripts/check` (what CI runs) on Linux: typecheck and svelte-check **0 errors, 0 warnings**;
  mock-backend Chromium UI checks **22 passed** (races, ux, layout); vendored files (39),
  versions, shortcut sheets, docs snapshot tests and the docs link check (**0 broken**, down
  from 20) passed; `cargo fmt --check` and workspace Clippy `-D warnings` clean.
- Workspace tests in Link's disposable HOME/XDG/ARCADE session with private D-Bus and Xvfb:
  **198 passed, 0 failed, 2 ignored** (the released-Find package test and the benchmark).
  The isolation checks now compare against the session's root, not `/tmp`.
- UI shell **230,463 / 245,760 bytes** raw (192,190 in 0.3.8; the budget now covers the vendored kit),
  **67,346 bytes** gzip; deferred guides and map **44,852 bytes**.
- Shortcut overlay, headless Xvfb render: Template B in both themes with Inter; window
  ready in a median **36 ms**, p95 **45 ms** (warm).
- Not verified here (needs a real desktop): the global shortcut opening the overlay on
  Hyprland, tray behaviour and icon colour on real panels, the Windows taskbar theme
  detection, moving a real 0.3.8 profile's folders, and self-update from 0.3.8 to 0.4.0.

## 0.3.8 verification (2026-10-25)

- Release target: `v0.3.8`, with the hub UI overhaul, a dedicated Learn reader,
  delayed window reveal and parallel engine probes. See CHANGELOG.md.
- Local workspace tests in Link's disposable HOME/XDG/ARCADE, private D-Bus and
  Xvfb session: **196 passed, 0 failed, 2 ignored**. The ignored tests require the
  benchmark machine and the final Find AppImage.
- Workspace Clippy with all targets and `--locked -- -D warnings`, and
  `cargo fmt --all --check`: passed on Rust 1.99.1.
- Frontend typecheck: **0 errors, 0 warnings**. Production build: **160 modules**;
  shell **192,190 / 204,800 bytes** raw, **57,935 bytes** gzip; deferred pages
  **46,102 bytes**.
- Mock-backend Chromium checks: **22 passed** across request races, UI behavior
  and layout. Dark, light and narrow-window screenshots were also checked.
- Vendored sources, shortcut validation and all four docs snapshot tests passed.
  Python 3.14 emitted the existing non-failing HTTP fixture ResourceWarnings.
- Cross-platform packaging and installer checks are required in the CI and stable
  Release workflows. Local verification does not cover real Windows/macOS desktops.

## Previous release status (0.3.7 and earlier)

- Latest release: [v0.3.7](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.3.7), published 2026-10-25 (a calmer shortcut sheet; see CHANGELOG.md; 0.3.0 was tagged but its release build could not finish). The CI and e2e notes below were recorded for the earlier v0.2.0.
- Previously released: [v0.3.6](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.3.6), published 2026-10-19 (updating while Tools runs switches to the new version).
- Previously released: [v0.3.3](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.3.3), published 2026-10-19 (released every inherited descriptor at start, which also cut off helpers from the app's own image).
- Previously released: [v0.3.2](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.3.2), published 2026-10-18 (fixes for what verifying the release before it found).
- Previously released: [v0.3.1](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.3.1), published 2026-10-18 (fixes from the October audit and stress test).
- Previously released: [v0.2.1](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.2.1), published 2026-10-16.
- Previously released: [v0.2.0](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.2.0) with a Linux self-installing AppImage, a per-user Windows installer, a macOS dmg, `arcade-release.json` and `SHA256SUMS.txt`.
- Merged to `main` from PR #2 (9d637db (backup fix cf8f0c5)) after CI passed on Linux, Windows and macOS: [run 38601196256](https://github.com/qa-p1/Arcade-tools/actions/runs/38601196256). That covers the build, tests, clippy or analyzers, installer smoke tests on each OS, and the Linux AppImage install, repair and uninstall check in a temporary profile.
- Ecosystem e2e (Arcade Link `tools/e2e.py`, real CI builds of all eight apps): install, tray hosting (hosted by Tools, own icon back within 1 s after Tools is killed, no flash on restart) and shortcuts pass. Tools hosts the tray, adopts, installs, updates, rolls back and uninstalls all seven apps; the backup fix for Lens, Wheel and Shelf settings landed in cf8f0c5 before the tag.
- Not yet verified: a real Windows or macOS desktop (CI runners only), and the owner's real-desktop checks. Builds are unsigned, so SmartScreen and Gatekeeper ask for approval.

Arcade Link is pinned to commit `3bbbc5d` on Link's main branch (after the v0.3.0 tag).

Implemented scope from plan §8 and §10–§14:

- Per-user installer integration, receipts/adoption, install/update/repair/removal,
  channel selection, protocol holds, opt-in idle updates, retained rollback and
  Tools self-update/helper handoff. Stable updates ignore build metadata in
  version precedence and require a different recorded SHA-256 at equal precedence;
  nightly comparisons retain their literal version/hash policy.
- Nine-tab Svelte hub and onboarding; resident tray hosting, app opt-outs and icon
  hand-back; closing the window destroys its webview while resident features stay.
- Native shortcut overlay, global rebinding/conflict review, app/catalog shortcuts,
  desktop readers and labels stored in Tools-owned files.
- Offline Learn/Connections, sanitized Markdown, versioned guide caching and engine
  detection/guidance.
- Visible-only process metrics, health checks, local redacted diagnostics and
  confirmed portable settings backup/restore with secrets and app data excluded.

See [the user guide](user/index.md) for behavior and [README](../README.md) for
development checks.

Local verification on Linux, 2026-10-13, using Rust 1.99.0, Node 24.21.0 and
Python 3.14.7; Cargo builds used three jobs:

- Workspace tests through Link's disposable HOME/XDG/ARCADE, private D-Bus and
  Xvfb runner: **160 passed, 0 failed, 2 ignored**. Core: 77; desktop: 23;
  shortcuts: 40; overlay: 20. The ignored checks require the release benchmark
  machine or the final Find AppImage. No real Find artifact check ran.
- Six new update-policy unit tests and two new scheduler/Wheel lifecycle tests
  passed, covering the five requested version/hash cases, reverse build metadata,
  compatibility holds, displayed versions, receipt rewrites and rollback. The
  existing Tools self-update test also passed with a build-tagged release and
  plain-version app receipt.
- Workspace Clippy, all targets, locked: **0 errors, 0 warnings**. The newer local
  compiler used command-scoped allowances for `float_literal_f32_fallback`,
  `clippy::collapsible_if`, `clippy::chunks_exact_to_as_chunks`,
  `clippy::manual_is_multiple_of` and `clippy::byte_char_slices`. Rust 1.88 remains
  the project's minimum and CI toolchain; no dependency pins changed.
- `cargo fmt --all --check`: passed across all four workspace packages.
- `npm run typecheck`: **0 errors, 0 warnings**; `npm run build`: **149 modules**.
  UI size: **203,196 / 204,800 bytes** raw, **58,757 bytes** gzip;
  deferred guides/map: **16,135 bytes**.
- Vendored verification: **5 Link v0.3.0 sources** match their hashes (now 8
  pinned files with the adapted Tauri NSIS template and its two licenses).
  Tools shortcut validation: **3 shortcuts in 2 groups**, valid.
- Docs snapshot script: **4 tests passed**. Python 3.14 emitted two non-failing
  ResourceWarnings for mocked HTTP 404 fixtures.
- Bundled docs: **9 sources, 66 files, 176,294 / 8,000,000 bytes** including the
  manifest; all sizes and SHA-256 hashes verified.

The catalog snapshot was refreshed from `main` at
`4d0b42b7b66d5ad9c131aed784b8aa0a6616fa15` on 2026-10-13: **7 apps, 3 bundles,
11 engines, 31 shortcut sheets plus their index, 9 ecosystem guides**. All 44
catalog payload files match that commit byte-for-byte; their contents already
matched before this refresh. Provenance now names `main`, and ecosystem docs use
the `main` directory/manifest reference. App guides retain the labeled `local`
fallback because published stable tags predate `docs/user/`. Release builds refresh
stable-tag docs and keep the existing labeled guides when released docs are absent.
Runtime exact-version downloads remain separate.

## Installer standard on Windows and macOS

- Windows uses an adapted Tauri 2.11.5 NSIS template (see `VENDORED`) with
  Link's shared `arcade.nsh`. A plain `/S` now installs to
  `%LOCALAPPDATA%\Programs\Arcade Tools`, the same folder as the wizard; the
  stock template used to copy files to `%LOCALAPPDATA%\Arcade Tools` first, so
  the integration step failed. The shared task page replaces the custom login
  page: start at login (on by default) and an unchecked desktop shortcut, with
  `/STARTATLOGIN` and `/DESKTOPSHORTCUT` for silent installs. Silent installs no
  longer create an unrecorded desktop shortcut. Apps & features uses the
  `arcade.tools` key with the catalog homepage and a quiet uninstall string.
- Start at login on Windows now reads the receipt-recorded Startup shortcut
  for every installed app and for Tools itself, falling back to the Run value
  only without a receipt. Updates therefore keep the user's choice. Toggling it
  in Tools edits that shortcut and the receipt, after removing the Run value or
  Tools' older VBScript entry. Windows updates install into the folder the
  receipt records instead of always the default folder.
- macOS `--install [--silent] [--no-launch] [--channel]` moves or copies the
  bundle to Applications, adds the CLI link and writes a `macos-bundle` receipt;
  `--repair` and `--uninstall` use that receipt. Self-updates, which run
  `--install --silent` from the new bundle, no longer fail at that step. A copy
  run from the dmg, Downloads or a translocated folder offers Link's shared
  "Move Arcade Tools to Applications?" dialog; a copy in Applications without a
  receipt records one on first run.
- `scripts/verify-package.py` now runs a plain `/S` install and a custom-folder
  install with explicit tasks, checks the receipt, shortcuts and Apps &
  features entry, and uninstalls with `_?=` so the removal checks wait for it.
  On macOS it mounts the dmg read-only and runs `--install --silent
  --no-launch`, `--integration-status`, `--repair` and `--uninstall` in an
  isolated home, checking that no shell profile is written. The universal
  build now prepares the overlay for each architecture as Tauri requires.
- Local checks on Linux for this round: workspace Clippy clean, formatting
  clean, the new receipt-login and folder tests passed (see the commit), and
  vendored and docs checks passed. Windows NSIS and macOS code paths compile
  only in CI; none of this ran on a real Windows or macOS desktop.

CI: pending (orchestrator fills in the run)

Windows/macOS package behavior is **CI only**, with no real desktop run verified
here. Sway/KDE/GNOME readers are **fixture-only** (six desktop reader tests), with
no real desktop run. Signing/notarization remain the owner's release decisions.
Verification used disposable profiles; real installations, login files, shell
profiles and Hyprland configuration were untouched. No packaging build ran.
