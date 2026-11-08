# Arcade Tools v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

## 2026-11-08 (session 2)

- **Gates on rc.3** ([run 42500704532](https://github.com/qa-p1/Arcade-tools/actions/runs/42500704532)), the
  workflow's first run, on the published files: SHA256SUMS, manifest and prerelease checks pass; conformance (every
  rule), package smoke on the AppImage, exe and dmg, upgrade 0.4.6 → 1.0.0 with data kept, maintenance, resident
  (3 cycles, PID stable), tray (window in 188 ms), N-1 on Link v0.4.4 and v0.4.15, Tier 1 Hyprland and Tier 2
  sway/KWin/Mutter/X11 all pass. Budgets met: PSS background 1.2 / open 257.8 / closed 11.5 MB; cold start 564 ms,
  show 536 ms, wakeups 0.3 bg / 9.7 open / 0.7 closed, disk 371.5 MB. The delta test passed (14.1 of 94.5 MB from
  0.4.6, tampered chunk and bad signature refused), but its step failed afterwards: `tee target/delta.log` ran before
  `target/` existed. The run is red, so rc.3 cannot be promoted; the fix (log to `$RUNNER_TEMP`) goes into rc.4,
  which is needed anyway for the rc.3 → rc.4 self-update.
- **rc.3 published** ([v1.0.0-rc.3](https://github.com/qa-p1/Arcade-tools/releases/tag/v1.0.0-rc.3), from f246c5b,
  PR #40). Release run 42497501163: the first attempt's Linux bundle timed out in `verify-resident.py` (the first
  window open after a background start did not return in 25 s); macOS and Windows passed. The same step passed on
  main CI for this commit, on rc.2's bundle and 3/3 locally on main CI's AppImage, and no reader of the `last_used`
  change runs then. One re-run of the failed job passed and published. **Open:** the hang is unexplained;
  `verify-resident.py` now prints the hung launch's output and the app's processes (09e7838, for rc.4), and Gates
  runs the same check on the published AppImage. Gates on rc.3 started.
- rc.2's Release (run 42492892679, from 6238329) built and checked Linux and Windows, but the macOS job failed
  after its package smoke passed: "Stage the installer" compared `wc -l` as a string, and macOS pads it with
  spaces. Nothing was published (publish needs every bundle). The comparison is numeric now, in Release and CI.
- **Gates workflow (D14).** `gates.yml` runs every exit gate against a published rc's files: download, then
  `SHA256SUMS.txt` and manifest checks, conformance, package smoke on all three installers
  (`verify-package.py --package`), the upgrade from v0.4.6, maintenance, resident, tray and N-1 on the AppImage
  itself, budgets, Tier 1 and Tier 2, and Tools' delta, signature and tampered-chunk checks on the published
  AppImages (`tests/releases.rs` gate test; on 0.4.6 → the 1.0.0 build it fetched 15.3 of 92.7 MB). With
  `update-from`, `verify-self-update.py` lets an earlier rc update itself to this one through GitHub with a delta.
- Found while writing it: a login (background) start counted as use, so Tools' first check never auto-updated
  Tools (put off 6 h); fixed with a test. A successful delta now logs a line, so the self-update gate can prove it.
  The tray test follows the resident process, so it runs on the AppImage (which relaunches unpacked).
- Next: PR → green CI → merge → rc.3 → Gates on rc.3 → rc.4 (a real change, so its bytes differ) → Gates on rc.4
  with `update-from: v1.0.0-rc.3`. Stable stays blocked on B1.
- **Step 8 started.** PR #23 green on every job at f3a609f (version 1.0.0, guide screenshots retaken, release
  steps in `docs/dev/packaging.md` rewritten for rc → Promote) and merged as 7682ac9. `v1.0.0-rc.1` tagged through
  "Tag a release", which started "Release" on the test channel (run 42487791767). Next, against the published
  rc files: upgrade from published 0.4.6 (`verify-upgrade.py`), N-1, desktop and performance; then rc.2 for the
  rc.1 → rc.2 self-update (delta path) under `ARCADE_TOOLS_CHANNEL_OVERRIDE=test`.
- rc.1's Release (run 42487791767) failed in `prepare` before building anything: `check-versions.py` needs
  `tomllib` (Python 3.11+) and that job used ubuntu-22.04's Python 3.10; CI's jobs install 3.12 first. Nothing was
  published. Release runs the workflow from the tag's commit, so rc.1 cannot be rebuilt: the fix goes to main and
  the next candidate is rc.2 (the rc.1 tag stays, as every tag does).
- On Link v1.0.5 (601af05): the Tier 2 smoke polls `--status` instead of a fixed 8 s sleep (the sway job once
  missed Tools' first start), and `perf_budget.py` counts wakeups per thread: CI's first perf run read open-window
  wakeups as -2.3/s, which passes any budget. Measured correctly: about 11/s open (budget 20), D12 corrected.
  PR #23 on 10e29ae was green on every job (Linux, Windows, macOS, Tier 1 Hyprland, all four Tier 2 desktops).
- Design pass (D13): `scripts/verify-design.py` in `scripts/check` holds the UI to the kit (colours, type, tokens,
  spacing scale) and the kit's error wording in UI and Rust; it found and the pass fixed 6 off-scale gaps and 8
  "Could not" messages (one pointed to a Rebuild button that does not exist). `SCREENS_STATES=1` captures empty,
  list, settings and error in both themes for the fleet sheet; reviewing them fixed the error state's header and
  toast. UI checks, typecheck and rustfmt pass locally.
- On Link v1.0.4 (37ba008; Link's crate code is unchanged from 1.0.3, the design kit re-syncs with no differences):
  the desktop workflows run the app from outside the checkout, and CI's Linux job runs `perf_budget.py` against the
  package with the budgets of D12 (3913326). Waiting for PR #23's full run.
- Delta gate, tampered chunk: `hub_lifecycle_updates_download_only_the_changed_chunks` now also has a mirror serve
  wrong bytes for the delta's Range requests (the fake server's `tampered_ranges`): the chunk is rejected (Link's
  per-chunk BLAKE3, then the file's SHA-256, then Tools' own check) and the intact whole file is installed; with the
  whole file tampered too, the update fails and the previous version stays. At rc time the same path runs on the
  published files: rc.N updates itself to rc.N+1 under `ARCADE_TOOLS_CHANNEL_OVERRIDE=test`.
- Upgrade gate script `scripts/verify-upgrade.py OLD NEW` (under `dbus-run-session -- xvfb-run -a`): installs the
  published OLD from a download with Start at login, runs it on a user's settings, tray choices, update state and
  history (in OLD's own file shapes, read back after OLD ran), installs NEW over it while OLD runs, then checks NEW
  replaced the copy in place, kept all of it and Start at login, and moved launcher, command and login entry to
  itself (0.4.6's `app.arcadetools.desktop` must not remain). Dry run: published 0.4.6 (SHA256SUMS verified) → local
  build passes. At rc time it runs against the published rc file. No Tools nightly has ever been published, so the
  Nightly → Stable half of that gate uses the first nightly built from 1.0 (recorded for step 8).
- N-1 (D11): CI runs the tray test's peer checks (hosting, cached menu toggle, Start) with the mock peer served by
  the v0.4.4 and v0.4.15 Link CLIs that the published apps were built with; both pass locally against this build
  (3 s each). The full tray test also passes locally with the overlay built: its `--show-shortcuts` step expected
  the old immediate return and now waits for the overlay to close. Bundling retries twice (GitHub's 504s on
  NSIS/linuxdeploy downloads). Docs: the guide's screenshots come from `screens.mjs` (`SCREENS_DOCS=1`, retaken at
  each version bump), and Features describes channels, signed manifests, deltas and withdrawn releases (d42c210).

- On Link v1.0.3 (6e4ac67): page-out of every private mapping after the window closes (closed PSS 36 → 12) and the
  window-open cap of 281 PSS from CI measurements (D10). Tier 1 Hyprland test fixed: a layer surface has `x/y/w/h`,
  not `at/size`; the pointer now goes to a part of the other window the centred overlay does not cover; Escape goes
  to the focused surface; the app runs from a private Downloads copy (the CI artifact sits in a git checkout, which
  Link treats as a dev build that refuses to install; fixed in Link PR #51 for 1.0.4). Bundler tools are cached in
  CI (a GitHub 504 on NSIS failed a run).
- Performance budgets: Link's new `perf_budget.py` (PR #51) measured locally: cold start 690 ms, show 614 ms, idle
  CPU 0.0 / 0.4 / 0.03 % (background / Home open 60 s / closed), wakeups 0.2 / 2.2 / 0.4 per second, disk 370 MB
  (88 AppImage + 282 unpacked runtime). The CI step joins once Tools moves to Link 1.0.4.
- Done since: schema versions + migrations from every 0.x (D9, a94668e); clean uninstall leaves nothing in a fresh
  home (lifecycle test + package smoke, 67355d2); accessibility checks (axe WCAG A/AA on every tab in both themes,
  Tab reachability with a visible ring, 125 %/150 % scaling, reduced motion; a8591a3) — the kit's tab-panel focus rule
  went upstream (Link PR https://github.com/qa-p1/Arcade-Link/pull/47); native-library notices for the AppImage
  (`scripts/native-notices.py`, 4c238cb; the recorded set comes from the CI artifact).
- CI fixes (5873625): the T3 and migration tests used Linux's legacy folder on every OS (now taken from Link's table
  per platform); the updates test still expected a newer `updates.json` to be refused (D9 reads it). `scripts/check`
  runs `cargo test --no-fail-fast` so one failing binary no longer hides the rest.
- Product bug found by the clean-uninstall smoke (36aa4fa): `--repair` run from another copy (the download) repointed
  the receipt, desktop entry and CLI at that copy; a later uninstall deleted the download and left the installed
  copy. The running copy is adopted only when the recorded file is gone.
- Memory (task: budgets). Measured on Xvfb (no swap locally): background 40 PSS, open 350, closed +20 s 71 vs budgets
  30 / 225 / 30. Causes: Link's trim dropped only `r-xp`, so read-only data (`r--p`: libwebkit ~10 MB, the binary's
  .rodata ~4 MB) stayed resident; the single trim at "manager ready" ran before the tray, Link socket and first
  update check faulted code back in. Link fix in PR #47 (smaps-checked `r--p` trim; also stops reverting written
  `r-xp` mappings, a latent bug). Tools re-trims once after startup settles and after each background update check.
  Result on a local package against the patched Link: background 15–18 PSS (met), closed +20 s 28–34 with swap
  (as on CI runners; 45 without), window open 261–334 vs 225 — WebKitWebProcess keeps ~123 MB of in-use malloc for
  llvmpipe compositing, removable only by UX trades; D10 records the decision (the open cap is set from CI's perf
  job, the owner's option is in catalog `LOCAL.md` §4).
- The Linux package smoke (CI on 7802262) found uninstall leaving `arcade.tools.lock`, `icon-theme.cache` and
  `mimeinfo.cache` in a fresh home: a Link bug, fixed in Link PR #47 (52bd02a). The local package built against it
  passes the whole lifecycle; Tools' CI passes once it pins Link 1.0.2. Release-only `unused_mut` warning in
  `trust.rs` fixed.

## 2026-11-08 (later)

- Link v1.0.1 tagged (a68e801) through its "Tag a release" after the desktop harness passed on main; Link PR
  https://github.com/qa-p1/Arcade-Link/pull/46 moves the template to v1.0.1 and passes `link-ref` explicitly.
- Tools on Link v1.0.1 (a0772f1): T3 flipped to a normal test (legacy folder retired once, conflicts archived under
  `archived-legacy`); full `scripts/check` green locally.
- Step 7 so far: `cargo deny check` clean (advisory updates; zip 2.4 → 9.4, `archive::parse` adapted; crates
  `publish = false`; Link git source allowed), deny job + Dependabot (cargo, npm, actions); `SECURITY.md`; CHANGELOG
  in Keep a Changelog form; Linux identity files (`dev.arcade.Tools.desktop`, scalable + symbolic icons) in the
  AppImage and installed; `arcade-link conformance .` passes every repository rule (ID-1..3, REPO-1/2, REL-1/2,
  LINK-2). CI runs conformance on the package and the fleet memory budget on the AppImage.
- THIRD_PARTY_NOTICES.md generated by `scripts/notices.py` (670 components, licence texts grouped; `--check` in
  `scripts/check`), bundled, and shown by Settings › About › Show licences (UI check covers it and Escape) (4a3e2c4).
- PR #23 CI was red: macOS/Windows built the Linux-only SIGCHLD test, and the package smoke treated U1's relaunch
  (launcher exits 0) as a crash. Fixed in fb9ada9 (the smoke now requires Tools running from its unpacked copy).
- Next: green CI on PR #23 and its timings; remaining step 7 items (schema versions, accessibility, design pass,
  docs/screenshots, clean uninstall, N-1); wire Link's desktop-e2e/tier2 workflows for the RT/UI/PERF rules; then rc.

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
- VENDORED now records Link v1.0.0 (d60772c) for the release script and design kit; `scripts/arcade-release.py`
  gains the `test` channel and `arcade.css` the emoji font fallbacks, both copied unchanged. This fixed PR #23's red
  `verify-vendored` step ("VENDORED names Link tags ['v0.4.15'], but Cargo.toml pins v1.0.0").
- All audit findings for Tools are fixed except T3 (needs Link 1.0.1).
- Found in step 4: `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` was never read, so the rc gates could not install rc files
  through Tools, and `set_channel("test")` or a restored backup could store Test and make Tools trust unsigned rc
  manifests. Fixed (D5 Test bullet), regression test `tests/channel_override.rs` (own binary: the variable is
  process-wide). Hub test helpers moved to `tests/support/hub.rs`.
- Step 4 (dead code): `cargo machete` clean. Removed the unused second backup implementation
  (`portable_backup.rs`, ~1,300 lines; the app uses `backup.rs` + `archive.rs`), keeping its live export screening
  as `settings_export.rs`; `archive::parse` gained unit tests for symlink entries, expansion bombs and garbage (the
  removed module had tested its own parser for these). Removed `issue_apps`; engines use the bounded
  `read_os_release`. knip: 14 exports used only in their own file are no longer exported; `design/format.ts` is
  vendored. `scripts/tray-resident-e2e.py` (tray, resident, crash recovery, guide screenshot) is not in CI yet:
  step 6 wires it in.
- Step 5 (memory, D7): U1 relaunch + runtime GC, W1/W3b after close, OPEN1 (self-trim + WebKit helper preload,
  bundled `libarcade-trim.so`, checked by `verify-package.py`), TD1 tree PSS on Home. Link's `fleet_budget.py`
  learned to follow unpacked relaunches and `--only` (in Link PR #45, also fixing its dbus arguments, which would
  have stopped the fleet job). The CI budget step lands with the Link 1.0.1 bump; numbers will be recorded here.
- Step 6 (CI, D8): Release builds rc tags only and publishes on the test channel through Link's `app-publish`;
  new `promote.yml` and `nightly.yml` call Link's reusable workflows; `tag.yml` makes rc tags and starts Release;
  network steps have timeouts and retries; the tray/resident e2e runs in CI. CI timing to be measured on the PR.
- Link 1.0.1 is up for review: https://github.com/qa-p1/Arcade-Link/pull/45.
- Next: tag Link v1.0.1, bump Tools to it and flip T3; PLAN steps 5 (memory), 6 (CI), 7 (round 5), 8 (rc).

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
