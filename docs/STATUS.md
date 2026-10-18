# Arcade Tools 0.2.0 status

## Release status (2026-10-16)

- Latest release: [v0.2.1](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.2.1), published 2026-10-16. The CI and e2e notes below were recorded for the earlier v0.2.0.
- Previously released: [v0.2.0](https://github.com/qa-p1/Arcade-tools/releases/tag/v0.2.0) with a Linux self-installing AppImage, a per-user Windows installer, a macOS dmg, `arcade-release.json` and `SHA256SUMS.txt`.
- Merged to `main` from PR #2 (9d637db (backup fix cf8f0c5)) after CI passed on Linux, Windows and macOS: [run 38601196256](https://github.com/qa-p1/Arcade-tools/actions/runs/38601196256). That covers the build, tests, clippy or analyzers, installer smoke tests on each OS, and the Linux AppImage install, repair and uninstall check in a temporary profile.
- Ecosystem e2e (Arcade Link `tools/e2e.py`, real CI builds of all eight apps): install, tray hosting (hosted by Tools, own icon back within 1 s after Tools is killed, no flash on restart) and shortcuts pass. Tools hosts the tray, adopts, installs, updates, rolls back and uninstalls all seven apps; the backup fix for Lens, Wheel and Shelf settings landed in cf8f0c5 before the tag.
- Not yet verified: a real Windows or macOS desktop (CI runners only), and the owner's real-desktop checks. Builds are unsigned, so SmartScreen and Gatekeeper ask for approval.

Arcade Link is pinned to the released `v0.3.0` tag, commit
`33650a87b6999fbd553ac678cc7bde7a308cb852`.

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
