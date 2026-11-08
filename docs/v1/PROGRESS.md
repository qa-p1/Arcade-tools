# Arcade Tools v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

## 2026-11-08

- D5 download ladder done: `src/delta.rs` (local chunk index from retained packages + installed file, bounded pieces and
  Range requests), `Source::download_from` (delta → whole file resumed from `<sha256>.part`, 3 attempts, 30 s idle),
  saved `<sha256>.manifest.json` beside retained packages and kept by pruning. TL-11's resume test is now a normal test;
  new hub test checks an update fetches < ¼ of the file and that a Range-ignoring host falls back to a full download.
- Catalog `bad` flag done (D5): schema + validator in the catalog, discovery/cache skip, `AppView.withdrawn`, UI notice
  and Updates section, hub test + UI check.
- TL-9 done (D6): `installedChannel`, Nightly badge, pending-switch notice, switch opens the reviewed update,
  Nightly→Stable backs up data first (`src/channel_backup.rs`); lock test flipped, hub test + UI check added.
- TL-1 done: `--show-shortcuts` runs the overlay and waits (`hotkey::run_overlay`), so an AppImage mount outlives it;
  the tray keeps the detached launch. CLI test with a fake overlay fails without the fix.
- TL-5 fixed in packaging: `libxkbcommon.so*` joins `libwayland-client.so*` in `LINUXDEPLOY_EXCLUDED_LIBRARIES`
  (CI, release, `scripts/package`; linuxdeploy splits on `;`), and `verify-package.py` asserts no bundled
  libxkbcommon. Ubuntu 22.04 runners can't reproduce the crash (host = bundled), so typing in the packaged overlay
  on a newer X11 host is part of the rc gate (Tier 2 X11 smoke).
- RD-TOOLS-2 fixed: engine version checks (and the overlay's Hyprland registration, which also runs inside Tools)
  wait by polling `try_wait` instead of the SIGCHLD-based `wait-timeout` crate, which GTK/GLib's own SIGCHLD handler
  starved. Unit test replaces the SIGCHLD handler and fails without the fix (timed out).
- Draft PR qa-p1/Arcade-tools#23 opened for CI; this session watches it.
- RD-TOOLS-3 fixed: Details falls back to the scheduler's last result for the selected channel; the action reads
  "Update to vX" only when one is available, "Up to date" (disabled) when current, "Check for updates" when unknown.
  UI check in `lock.mjs`.
- RD-TOOLS-6 fixed: GTK program name `dev.arcade.Tools` before Tauri starts (window class/app_id), and the Linux
  desktop id is `dev.arcade.Tools` with `app.arcadetools` kept as a legacy id Link cleans up. The Tauri `identifier`
  stays `app.arcadetools` (it keys the Windows installer and macOS bundle). The window class itself is to be checked
  in the desktop harness (PLAN step 6).
- TL-12 fixed: Home names Arcade Tools; optional engine uses (catalog `optional`, Arcade-catalog 382e1f3) never need
  attention, and missing-engine notices read as sentences; Back up shows "Backing up…" and clears the old result;
  Tools' own bundled guide comes from the tree being built (`docs-snapshot.py --self-version`, used by release.yml).
  Other apps' bundled guides are their latest stable at build time by design (Learn fetches newer ones).
- CI's Link checkout (for `tools/e2e.py`) moved from v0.4.0 to v1.0.0.
- All audit findings for Tools are fixed except T3 (needs Link 1.0.1).
- Next: Link 1.0.1 (migration archive fix + template lockfile), then T3; then PLAN steps 4–8.

## 2026-11-07 — paused for the usage-limit reset

Done on the branch (all pushed, core suite green under `tools/e2e.py run`):
- Lock (step 1), manager split (D4), fixes TL-8, TL-4, CLI-2, TL-6, TL-2, TL-3, TL-10, RD-TOOLS-1, TL-7.
- Link v1.0.0 dependency and `Channel::Test` (0fc0b47).
- D5 discovery from the `/releases?per_page=30` listing per channel (b34b615).
- D5 signed manifests (2580fb2): `src/trust.rs`, `resources/update-keys.pub` (empty until B1), test key secret in
  `tests/support` (`TEST_SIGNING_KEY`), FakeRelease signs every release; refusal test in `hub_lifecycle.rs`.
- Fake server honours single `Range: bytes=a-b` / `a-` requests (206 + Content-Range), for the next step.

Next, in order:
1. Download ladder (D5) in `Source::download`: delta from local chunks (installed executable + retained packages in
   `installation::installers`, saved `<sha256>.manifest.json` next to each retained package), Fetch pieces split at
   chunk boundaries to ≤ 4 MiB and coalesced across gaps < 256 KiB into ≤ 8 MiB Range requests, any failure → full
   download; full download resumes from `downloads/<sha256>.part` (`Range: bytes=N-`, 200 restarts, 3 attempts);
   `DOWNLOAD_TIMEOUT` 30 s idle. Then rewrite `an_interrupted_download_resumes_where_it_stopped` (one call, retry
   inside; trickle server must honour Range) and drop its `should_panic`.
2. Catalog `bad` flag + rollback notice; TL-9 (`installedChannel`, badge, backup before Nightly→Stable).
3. TL-1, TL-5, RD-TOOLS-2/3/6, TL-12; T3 after Link 1.0.1. Then PLAN steps 4–8 and rc releases.


Branch `claude/tender-feynman-3vt7dg` from main `60f92ea` (Tools 0.4.6).

- Step 0 done: spec and audit copied into `docs/v1` (D2); signing blocker recorded (D1).
- Next: PLAN.md loop step 1, the behaviour lock (CLI, `--status`, Link messages, data formats, every audit finding as
  an expected-to-fail regression test, fleet rules).
