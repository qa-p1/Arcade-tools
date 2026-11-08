# Arcade Tools v1 decisions

Decisions the plan left open, made by the v1 session. Each one cites the part of `dev/v1/README.md` it follows.
Newest last.

## D1 — Stable promotion waits for the signing secret (blocker B1)

The `ARCADE_UPDATE_SIGNING_KEY` Actions secret does not exist and this session cannot create it (Link
`docs/v1/DECISIONS.md` D1). Tools still gets the full verifier: it checks `manifest.json.sig` against the built-in keys,
and the tests sign with throwaway keys made inside each test run. rc releases on the hidden `test` channel need no
signature to be built and published, so all rc work and every rc exit gate continue. `promote.yml` refuses to run
without the secret, so `1.0.0` is promoted only after the owner runs step 1 of catalog `dev/v1/LOCAL.md` and the
public key is committed here as the current key. README: "never publish unsigned stable".

## D2 — Spec and audit copied at catalog `2f2e19b`

`docs/v1/SPEC.md` is `specs/TOOLS.md`; `docs/v1/audit/` holds `AUDIT/tools.md` with its evidence folder,
`FINDINGS.md`, `memory.md` (with the Tools measurement files in `memory-tools/`) and `real-desktop.md`, all from
catalog commit `2f2e19b`. Line references in the spec are for Tools 0.4.6 (`60f92ea`).

## D3 — Known-bug tests, and TL-11 re-measured

A behaviour-lock test for a known bug is `#[should_panic(expected = "KNOWN BUG <id>")]` and its failing assertion
names the finding. It passes while the bug is there; once the fix lands the test stops panicking and fails, so the
marker has to be removed with the fix (PLAN.md step 1: "marked expected-to-fail until step 3 fixes them").

TL-11 said every download is capped at 120 s in total. `tests/lock_downloads.rs` shows otherwise for the blocking
client Tools uses: a 3.4 s body with 200 ms gaps completes under a 1 s timeout, and a 1.5 s gap fails it. reqwest's
blocking `Response` applies the timeout to each read, so it already acts as an idle timeout once the body starts. The
other half of TL-11 holds: there is no Range request, so an interrupted download restarts at byte 0
(`an_interrupted_download_resumes_where_it_stopped`, known bug). Step 3 still sets an explicit 30 s read-idle
timeout as the spec asks, and adds resume.

## D4 — Untangle (skill steps 2–5; both extras chosen by the owner)

Read from the skeleton (`grep` of every item) and the module graph; git history shows `manager.rs` changes in nearly
every commit, whatever the feature. Fixes first, the problems each one solves under it.

- Split `Manager` into one file per area under `src/manager/` (`preferences`, `records`, `checks`, `operations`,
  `self_update`, `uninstall`, `maintenance`, `launch`); `mod.rs` keeps the types, the struct and the shared helpers.
  Mechanical (`impl Manager` blocks moved whole, private methods became `pub(super)`); the compiler checks it.
  - `src/manager.rs` was 3 292 lines with nine unrelated reasons to change; an update fix and an uninstall fix
    touched the same file and re-read all of it.
- Hash installed executables in one place, `Manager::cached_checksum`, keyed by size, times and inode.
  - `record()`'s mismatch branch and `record_from_receipt` called the uncached `checksum` (TL-8: 12 % CPU on Home).
- Work out an app's channel in one function that returns both the selected and the installed channel.
  - `list_catalog` and `checked` each computed `prefs.channels.get(id).or(record.channel)` on their own.
  - The UI could only show the selected one (TL-9).
- One retention rule for installers: keep the installed package and one rollback package per app, applied after
  each successful operation and at startup.
  - `installation::retain` wrote packages that nothing ever removed; `cleanup_interrupted` only knew downloads
    and snapshots (TL-7).
- One "back one step" rule for Escape, in the overlay and in the window: close a picker, then clear a filter, then
  leave a detail or reader view; only the top level closes (the overlay) or does nothing (the window).
  - The overlay's `key` returned "close" for Escape unconditionally (TL-6).
  - The window had no Escape handling at all (TL-3).
- Downloads move into the updater redesign (`specs/LINK.md` §3): one download path with an explicit read-idle
  timeout and Range resume, used by full and delta downloads alike.
  - The timeout was a module constant shared by catalog fetches and installers; nothing could resume (TL-11).

Special cases kept on purpose: the 30 s parent wait in `finish_self_update` (the old process must exit before its
file is replaced) and the per-OS installer kinds (the platforms differ).

## D5 — Updater design (SPEC.md §Updater work, Link `release` API)

- **Link 1.0.0** is the dependency (all crates, one copy), with the `release` feature for manifests and deltas.
- **Channels:** `Stable`, `Nightly`, `Test`. Discovery lists `/releases` (ETag-conditional): Stable is the newest
  published release tagged `vX.Y.Z`; Nightly the newest prerelease tagged `vX.Y.Z-nightly.YYYYMMDD.N`; Test the
  newest prerelease tagged `vX.Y.Z-rc.N`. The old moving `nightly` tag is no longer followed (its builds are
  unsigned, so they could not pass the signature rule anyway). `Test` is never stored or shown: it applies only while
  `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` is set (the release gates): `Channel::followed()` maps every app to Test
  while it is set and a stored Test (an rc's record) to Stable otherwise; `set_channel` refuses Test, backups never
  write it, and a production source refuses to check Test without the override (its manifests are unsigned). No
  hidden CLI flag: the environment variable is the one way in (SPEC allowed either). When the API's anonymous limit is hit, Stable
  falls back to `releases/latest/download/` as before; Nightly and Test report the limit and retry later.
- **Trust:** a release must carry `manifest.json`; Stable and Nightly also `manifest.json.sig`, verified against the
  keys built into Tools (`resources/update-keys.pub`: current and next). The trusted comment must name this app and
  channel. The chosen asset's size and SHA-256 must equal its manifest entry; `arcade-release.json` only supplies
  install arguments. Test manifests are unsigned (Link D4) and are reachable only through the override. Until the
  owner adds the public key (B1, `LOCAL.md` §1), the key file is empty and stable/nightly updates are refused with a
  clear message: never unsigned. Debug-only loopback test sources trust a fixed test key whose secret lives in
  `tests/support`; release builds cannot use it.
- **Chunk store:** the "shared store" is the files Tools already keeps: every installed executable and every retained
  package, each indexed by its own manifest entry (saved next to the package as `<sha256>.manifest.json`), so indexing
  costs nothing and no chunk is stored twice. A file without a saved entry (a pre-1.0 install) is chunked once on
  demand. Missing ranges are fetched with HTTP Range, neighbouring ranges less than 256 KiB apart in one request.
- **Ladder:** delta → full download with resume (`downloads/<sha256>.part`, `Range: bytes=N-`, up to 3 attempts) →
  error. Downloads use a 30 s read-idle timeout. Progress counts the bytes to download, not the file size.
- **Delta details** (`src/delta.rs`): a delta is tried only when it skips at least 20 % of the file; fetched pieces
  are split at chunk boundaries to ≤ 4 MiB and merged into Range requests of ≤ 8 MiB, so memory stays bounded. A
  host that answers a Range request with anything but the exact 206 range fails the delta, which falls back to the full
  download. A response shorter than its own Content-Length is an interruption (resumed); a complete response with the
  wrong bytes is a checksum error (the `.part` is discarded). Startup cleanup keeps `.part` files younger than 7 days.
- **Bad releases:** a catalog app (or `tools`) may list `bad: [{tag, reason}]` (catalog schema, Arcade-catalog
  7549858). Entries name release tags, not versions, because many nightlies share one base version. Discovery skips
  withdrawn tags (the next newest on the channel is offered) and never reuses a cached release that has since been
  withdrawn. Retained packages record their tag, so an installed withdrawn release shows the reason in the app's
  Manage card and in Updates › Withdrawn versions, with the rollback when one is kept.

## D6 — Nightly switch (TL-9, SPEC.md §Updater 2)

- `AppView` reports the selected `channel` and the `installedChannel` separately. Choosing a channel in the card or
  Details stores it and immediately opens the reviewed update for it; declining keeps the choice, and the card and
  Manage card say "Switching to …: not installed yet." until it is installed. An installed nightly shows a Nightly
  badge.
- A channel change counts as an update even to a lower version (`updates::is_newer` already did this; now tested).
- Nightly → Stable backs up the app's settings and data folders (not its cache) to
  `state/channel-backups/<app>/<nightly version>/` with a `backup.json` index, after the download and before the
  swap, and the result message names the folder. One backup per app; a later switch replaces it. Larger than 2 GiB
  refuses the switch rather than fill the disk. The live data is left for the app's own FS-2 downgrade rule, which
  may restore this backup.

## D7 — Memory (PLAN step 5; SPEC.md §Memory, AUDIT/memory.md)

- **U1:** a resident start (no arguments, `--background`, `--settings`) calls Link's `relaunch_unpacked` and exits;
  one-shot calls (`--status`, `--quit`, installers…) return before it, so they never pay for the ~250 MB copy, and a
  copy that is offering to install itself (Downloads) keeps running from its mount. Tools removes unpacked copies of
  every app that no process runs from (`runtime_gc.rs`), keeping each installed app's newest copy of the version
  its receipt names, at startup and after every operation. Link's `remove_unpacked_except` needs the folder key,
  which only the app can compute (it hashes the AppImage's size and time), so Tools decides by name and use instead.
- **W1 + W3b:** 3 s after the last window is destroyed (the audit's minimum before ending the network process, which
  holds only the onboarding flag's localStorage), `release_webkit` then `page_out_hidden`; also once after startup.
  This replaces `resident/memory.rs`, whose PAGEOUT of library pages the audit measured as ineffective.
- **OPEN1:** while the window is open, `runtime::trim` 2 s after the page is ready and every 20 s; WebKit's helper
  processes load `libarcade-trim.so` (Link's preload trimmer, vendored and built by `prepare-overlay.py`, placed in
  the AppImage's `usr/lib`) through `LD_PRELOAD`, set in `main` only when the library is inside `$APPDIR`. Link's
  child environment already drops entries inside `$APPDIR`, so apps Tools starts never inherit it. OPEN2 (software
  compositing) is not applied: motion is unproven and the spec doesn't list it.
- **TD1:** Home shows each app's tree PSS (descendants plus its AppImage's FUSE helper) on Linux; other platforms
  keep the main process's resident size.
- **Budgets** are Link's `tools/fleet/budgets.json` (`xvfb` section in CI: background and closed 30 PSS / 50 RSS,
  window open 225 / 310). Tools CI measures its packaged AppImage with Link's `fleet_budget.py --only tools` once it
  pins Link 1.0.1 (the version that can follow an unpacked relaunch).

## D8 — CI and releases on Link's shape (PLAN step 6, Link SPEC §4)

- **Release** (`release.yml`) runs only on a release-candidate tag `vX.Y.Z-rc.N` whose base is the app version (the
  built files already say `X.Y.Z`, so promotion needs no rebuild). It keeps the LTO build matrix and every package
  gate, then hands the installers (`release-<os>` artifacts, spaces in names replaced) to Link's reusable
  `app-publish.yml`: a hidden prerelease on the test channel with `arcade-release.json` and an unsigned manifest.
  `tag.yml` creates only rc tags and starts Release on them (a tag pushed with the workflow token starts no workflow).
- **Promote** (`promote.yml`, Link `app-promote.yml`) republishes a verified rc's bytes as `vX.Y.Z` with a signed
  manifest; it publishes nothing without `ARCADE_UPDATE_SIGNING_KEY` (B1).
- **Nightly** (`nightly.yml`, Link `app-nightly.yml`) republishes main CI's tested installers as
  `vX.Y.Z-nightly.YYYYMMDD.N`, signed. They are CI's non-LTO builds, by design: a nightly is exactly what was
  tested, and LTO only matters for the stable files, which come from Release. The old moving `nightly` release is
  no longer updated (D5 stopped following it); it is kept, never deleted.
- **CI** keeps one job per OS (lint, tests, package, package smoke). Every apt, npm and Playwright step has a timeout
  and three tries. Linux also runs `scripts/tray-resident-e2e.py` (tray host, resident window, single instance,
  crash recovery, Link-off quit) on the debug build the tests produced, with Link's cached CLI.

## D9 — Schema versions and the downgrade rule for Tools' own files

README Round 5: every settings and data file carries a schema version, with migrations tested from each released 0.x,
and Stable reads data from a newer Tools without breaking it.

| File | Schema | Notes |
|---|---|---|
| `preferences.json` | `schema` 1 (new) | 0.x wrote none: read as 0 (the same fields), stamped 1 on load and on every save, restored backups included |
| `tray-preferences.json` | `schema` 1 (new) | same rule |
| `updates.json` | `schema` 1 (since 0.2) | a higher schema was refused, which would have stopped update checks after a downgrade; now read |
| `update-history.jsonl` | none | append-only lines; unknown fields are ignored on read and the file is never rewritten |
| receipts, self-update plans, backups, channel backups | `schema` 1 | unchanged (Link receipts; Tools-written plans and archives) |
| `maintenance/latest.json`, caches | — | regenerated reports and caches, not user data |

A file from a newer Tools keeps its higher schema number and every field this version does not know (a flattened
map), so a Nightly → Stable switch never loses settings; the switch also backs up the app's data first (D6). The
three 0.x shapes (0.1.0; 0.2.0–0.2.1; 0.3.0–0.4.6), each in its own folder (`<data>/arcade-tools` until 0.3.8), are
loaded in `tests/schema_migrations.rs`; the tray file in `tray::settings` tests.

## D10 — Window-open memory on Xvfb is above the 225 PSS ratchet; no UX trade to close it

PLAN's checklist asks for every per-app budget. Link's `budgets.json` `xvfb` section gives Tools 30/50 in the
background and after close, 225/310 with the window open, and calls the open and closed caps *ratchets*: "set to
today's achievable value, lowered as items land" (AUDIT/memory.md §7). The 225 came from the audit's lab estimate for
OPEN1 (210–228), not from a run of `fleet_budget.py`.

Measured 2026-11-08 with `fleet_budget.py --only tools --section xvfb --settle 30 --open tools --close-wait 20` on the
AppImage packaged locally (Ubuntu 24.04 and its WebKitGTK, Mesa 25.2 llvmpipe, 4 CPUs like GitHub's runners; CI
packages on 22.04, so the WebKit it bundles differs, one more reason the cap below comes from CI), PSS / RSS in MB:

| state | 0.4.6 code before this session | with Link's `r--p` trim and trims after startup | budget |
|---|---|---|---|
| background | 40 / 43 | **14.6–17.6 / 15.5–18.6** | 30 / 50 |
| open, at the gate's 6 s | 350–398 | 329–334 / 363–368 | 225 / 310 |
| open, after the helpers' 20 s trim | — | 261 / 278 | 225 / 310 |
| closed +20 s | 71 | 28–45 (29 and 34 with swap, 45 without) | 30 / 50 |

CI's own run (ubuntu-22.04 runner, the WebKit Tools ships, Link v1.0.2, 2026-11-08): background **12.6 / 13.3**,
window open **267.7 / 293.2**, closed +20 s **35.8 / 38.7**. Closed was over its 30 PSS because W3b's page-out
skipped the heap and the written pages of library mappings (libLLVM's RELRO alone is 6.6 MB under llvmpipe); Link
1.0.3 pages out every private mapping of a hidden app: locally background 15 → 4 and closed 29–34 → 12 PSS, reopen
latency unchanged (mean 1.41 → 1.40 s over 8 reopens). A second CI run gave open 263.3 / 288.8 and closed 36.0 / 38.8, so Link 1.0.3's
`budgets.json` sets Tools' open cap to **281 PSS** (267.7 + 5 %), RSS unchanged at 310.

What is left with the window open is WebKitWebProcess: 211 MB anonymous, of which ~123 MB is glibc malloc in use
(two thread arenas of 40 and 38 MB plus a 45 MB heap; `malloc_trim` already returns the free pages). Under llvmpipe
that is mostly compositing layers. The audit names only two levers that remove it, and both change UX: compositing
off (OPEN2, −50/−40, unproven motion) and paging out the open window (OPEN3, swap-in on interaction). README's rule is
that UX must not get worse, so neither is applied.

**Decision.** Tools keeps the UX-neutral set (U1, W1, W3b, OPEN1 with Link 1.0.2's trim). The open-window cap stays a
ratchet: it is set from Tools' own CI perf job (the same command on `ubuntu-24.04`, task 14) to the measured value
plus at most 5 %, stated in `budgets.json` with this decision as its source, and it is only ever lowered. The
background budget is met as written, and closed is held to 30/50 as written. README's fleet-level open-window budget
(PSS ≤ 500 / RSS ≤ 680) is checked at the Fleet step. Whether to trade UX for memory (OPEN2) is the owner's call,
listed in catalog `dev/v1/LOCAL.md`.

## D11 — N-1: which peers, and how they are run

PLAN's gate is that the rc talks to the previous released versions of its peers over Link. Tools' Link traffic with
an app is the tray (own → hosted, the hosted menu, a toggle, Start from the manifest) and status. The released apps
speak the Link of the release they were built with: Look and Clipboard v0.4.4, Find and Lens v0.4.14, Box and Shelf
v0.4.15 (Wheel has no Link crate pin).

**Decision.** CI runs the tray end to end a second and third time with the mock peer served by the v0.4.4 and v0.4.15
`arcade-link` CLIs (the oldest and newest Link among published apps; between them the library changed only its design
kit, install code and peer descriptions, not the wire format, so v0.4.14 is covered), while the current CLI watches the tray and reads status. Mocks are used instead of the published apps
because they exercise the same protocol code without each app's GUI, engines and 30 s of start-up per run. When apps
ship 1.0 on Link 1.0.x, the pair moves to the Link of the previous stable release of each peer.

## D12 — Performance budgets (cold start, show, idle CPU and wakeups, disk)

README asks for idle CPU ≈ 0 % with bounded wakeups (background and with the default page open), cold start, show
latency and disk use within budget, without numbers. Link's `perf_budget.py` measures them on the package in a private
Xvfb profile, and the limits live in the `perf` block of Link's `budgets.json`, beside the memory budgets (D7, D10).

**Decision.** Tools' limits, set from a local run of the same tool (cold start 690 ms, show 614 ms, CPU 0.0 / 0.4 /
0.03 %, wakeups 0.2 / 2.2 / 0.4 per second, disk 370 MB) with headroom for CI runners and llvmpipe: cold start
1500 ms, show 1000 ms, background and closed CPU 0.5 % and 5 wakeups/s, Home open 2.0 % and 20 wakeups/s, disk
400 MB. CI fails on any of them (exit 1) and on a metric it could not measure. Like D10, they are ratchets: lowered
once CI's measurements show room, never raised without a new decision here. Disk counts the installed AppImage and
its unpacked copy (U1, ~88 + 282 MB): the unpacked copy is what makes start and show fast and memory low.

**Correction (Link 1.0.5).** The open-window wakeups above (2.2/s locally) and CI's first figure (-2.3/s) were
undercounts: `perf_budget.py` differenced whole-tree totals, so threads that exited during the window (WebKit's
workers) took their lifetime of context switches out of the count. Counted per thread (Link 1.0.5), Tools' open
window does about 11 wakeups/s locally (11.2); background 0.2 and closed 0.4 were unaffected. The budget of 20
still holds, with less headroom than recorded; it stays a ratchet.

## D13 — Design pass: what "from the kit" means, and how it stays true

README's design pass asks that fonts, icons, spacing, colours and error wording come from the Link design kit, with
the same states (empty, list, settings, error; light and dark) captured for every app. STANDARD §11 checks colours and
font sizes only; §4 also says "Nothing off-scale" for spacing, and §9 fixes the error form.

**Decision.** `scripts/verify-design.py` (run by `scripts/check`, so CI's check job on every OS) fails on a colour
literal, a px font size or a font family outside the kit's variables, an undefined `var(--token)`, a padding, margin
or gap off the kit's spacing scale (1 px hairlines allowed), and a "Could not / Failed to / Unable to" message in the UI
or in Rust outside logging. The pass snapped six off-scale gaps (22, 28, 30 → 24 or 32; the Connections indent is
now the row's own `--sp-14 + --tile + --sp-12`) and rewrote eight Rust messages to "Couldn't <verb> <thing>. <Reason>.
<What to do>." One of them sent people to a "Rebuild" button that does not exist; it now says changes show after Tools
restarts, which is what happens. `screens.mjs` with `SCREENS_STATES=1` captures the four states in both themes for
the fleet sheet (`dev/v1/fleet-screens/`). Looking at them found the error state's header still reading "Loading your
apps…" and its toast repeating the bare reason; both now say "Couldn't list your apps".

## D14 — Exit gates on the published files: the Gates workflow, and what the self-update gate proves

PLAN Release §2 runs every exit gate against the published rc files, never a local build; CI and Release test what
they build. README: "Nothing ships unverified".

**Decision.** `gates.yml` (workflow_dispatch on an rc tag) is that run. One job downloads the rc's files from its
release, refuses a draft or a non-prerelease, checks `SHA256SUMS.txt` and the unsigned manifest
(`arcade-link verify-manifest --unsigned --files`) and hands the checked files to every other job as artifacts; the
gate scripts come from the tag's commit. It covers each PLAN gate that has a published file to test: conformance on
the AppImage; `verify-package.py --package` on the AppImage, exe and dmg (the Windows and macOS smoke); the upgrade
from the previous stable release (default `v0.4.6`); maintenance and resident checks; the tray test on the AppImage
itself and with the published apps' Link as the peer (N-1); the memory and performance budgets; Tier 1 and Tier 2.
`promote.yml` is run only on an rc whose Gates run is green.

- **Delta, signature, tampered chunk.** The previous stable release (0.4.x) has no manifest, so no copy of it can
  fetch 1.0.0 with a delta through GitHub, and rc manifests are unsigned (D1, B1). The `delta` job therefore runs
  Tools' own updater code (`tests/releases.rs`, `ARCADE_TOOLS_GATE_DELTA=OLD:NEW`) on the two published AppImages:
  NEW is served by the local release host with a manifest signed by the test key, Tools checks the signature, builds
  the delta from OLD and assembles NEW byte for byte; a tampered chunk falls back to the intact file and a bad
  signature is refused. Measured on 0.4.6 → the 1.0.0 build: 15.3 of 92.7 MB fetched.
- **Self-update through GitHub.** With `update-from` set, `verify-self-update.py` installs that earlier rc in a
  private profile with automatic updates on for Tools and the test channel forced, starts it as login does and
  touches nothing. It passes only if the first check replaces it with this rc's bytes, the restarted copy runs in
  the background, the receipt and history agree, and Tools logged the delta (a successful delta now prints
  "Downloaded <file> with a delta: N of M bytes"; before, only a failed one left a trace). So PLAN's "Tools updates
  itself twice" is rc.N → rc.N+1 through the test channel, then rc.N+1 → 1.0.0 after Promote (Release §4).
- **Found by writing it.** A background start counted as use, so the auto-update policy ("not used in the last
  10 minutes") put Tools' own update off from the first check to the next one, 6 hours later; a login start now
  leaves "last used" at 0 (`desktop.rs`, test `a_background_start_is_not_use`). An AppImage relaunches from its
  unpacked copy and the launcher exits, so `tray-resident-e2e.py` follows the resident process by its private
  ARCADE_HOME instead of the PID it started; a binary extracted from the AppImage is not a substitute (WebKit's
  helper path is relative to the AppDir, and AppRun's GTK environment is missing). The macOS release job compared
  `wc -l` output as a string, which macOS pads with spaces; it compares numbers now (rc.2 failed on it).

## D15 — An rc installs as Stable; a release that fails to install is not retried automatically

The rc.3 → rc.4 self-update gate (Gates run 42506931186, then reproduced locally on the published files) found that
Tools could not install any rc: it called the installer with `--channel test`, and the install contract (Tools' own
`--install`, every app's, and `/ARCADECHANNEL` on Windows) takes stable or nightly only. Tools rolled back,
restarted, and its first check a minute later tried again, every minute. The delta itself worked: 79 KB of 94 MB.

**Decision.** D5 already says Test is never stored, so an rc installs as Stable: installers get the stored channel,
the receipt is checked against it, and at equal versions an installed rc and the next rc are compared by package
hash, not by channel (`is_newer` compares stored channels). README: a failed update must never become a loop, so a
Tools release that fails to install is marked like a rollback (`updates.rolledBack`), and Tools' own auto-update now
honours that mark as apps' already did (it ignored even a manual rollback). A newer release or an update by hand
clears it. The test installer now refuses any other channel, as the real ones do, which is why the tests had not
caught it. Since the old Tools runs the new installer, the self-update gate needs two rc's with this fix: rc.5, then
rc.6 with `update-from: v1.0.0-rc.5`.

The gate's profile also changed: it set `ARCADE_HOME`, which marks a test profile in which Tools never runs timed
update checks, so no update could have happened. It now uses a private `HOME` and XDG folders, as a user has.

## D16 — A self-installed AppImage's executable hash stands for its package hash

Gates on rc.6 (run 42520824716, `update-from: v1.0.0-rc.5`) found rc.6 on GitHub but rc.5 offered no update, and
never updated (reproduced locally on the published files). Every rc of a release carries the same version, since
the last one is promoted byte for byte as `1.0.0`, so at equal versions `is_newer` decides by the installed package
hash. Tools installed from the downloaded file has a receipt but no record of the package it came from, so that hash
was unknown and the answer was always "no update". rc.5's "offers nothing on its own release" check (PROGRESS) passed
for the same wrong reason.

**Decision.** An AppImage installs by copying itself, so its executable is the package, byte for byte (the self-update
gate asserts exactly that). An AppImage that installed itself, with a receipt and no record from Tools, takes its
executable's hash as the package hash (`record_from_receipt`; adoption already did this, and now shares it). Bytes
replaced outside Tools under a record Tools wrote still get no hash, as before: they came from an unknown package, so
an equal version is not offered over them (the Wheel lifecycle test locks that). Other kinds unpack an installer, so their package hash stays unknown until Tools
installs them, as before: an equal-version rc is not offered to a self-installed Windows or macOS Tools. That matters
only on the test channel; stable versions always differ.

Since the old Tools makes this decision, the self-update gate needs two rc's with the fix: rc.7, then rc.8 with
`update-from: v1.0.0-rc.7`.

## D17 — Commands wait 2 s for the running Tools to answer

rc.8's Release failed in the Linux bundle's resident check (run 42528082923): after `--restart`, the next `--quit`
exited 1 with "Arcade Tools didn't respond in time", 0.85 s after the last close/reopen cycle. The same check had
failed once before on PR #42 (and the rc.3 bundle hung once at a different step). The diagnostics added then show the
cause: the restarted Tools had written its endpoint and was starting GTK and WebKit, and the command's `hello` used
Link's 150 ms `HELLO_TIMEOUT`, which is the liveness probe for background checks. A person's `--quit`, `--restart` or
`--status` right after a restart, or soon after a login start, could fail the same way, and `--status` reported such a
Tools as not running. The start-up check that keeps legacy folders from moving under a running Tools used the same
probe.

**Decision.** These commands, and the folder check, connect with 2 s, the time Link's server itself waits for a hello.
A Tools that is not running still fails at once (no endpoint, or the connection is refused), so only a live but busy
Tools is waited for. A CLI test serves a fake Tools that answers `hello` after 600 ms: it failed with 150 ms and
passes with 2 s. Link's background probes keep 150 ms. Since Release and Gates run from the tag, rc.8 stays
unpublished and the fix goes into rc.9, with the self-update gate run as `update-from: v1.0.0-rc.7`.

## D18 — Window-open memory has two levels on CI; find the cause before any rc passes on it

Gates on rc.9 (run 42535663129, `update-from: v1.0.0-rc.7`) passed every gate, the self-update included (rc.7 → rc.9
with a delta of 8.5 of 94.5 MB, back in the background after 67 s), except the open-window memory: PSS 297.5 / RSS
333.3 against D10's cap of 281 / 310. rc.6 measured 301.4 / 338.3. Every other run measured 255.8–262.2 PSS: 30 CI
runs and three Gates runs, rc.7's among them. Background (1.3) and closed (11.3) were normal in both high runs, and
so were the runner image and the WebKit installed. Locally the same file measures 141 at 6 s on every run (full speed
or pinned to one CPU): the memory peaks at 2.4 s and is flat from 4.4 s, so the sample does not sit on a slope there.
The high level is about 40 MB above the normal one, about one glibc thread arena of the WebKit process (D10 found two
of 38–40 MB), which a thread gets or not depending on lock contention; this is a hypothesis, not yet a finding.

**Decision.** The cap stays at 281 (D10: it is only lowered) and a run that fails it is not re-run until it passes:
two of five Gates runs measured above it, so the budget is not met as it stands. The memory probe workflow measures a
published AppImage on eight runners at once, with the gate's own command and `scripts/open-memory.py` (PSS per
process and per kind of mapping 6 s after the window opens), then both again with `MALLOC_ARENA_MAX=2`, to find what
the high level is. What it finds decides the fix.

