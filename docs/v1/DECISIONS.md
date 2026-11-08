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
  `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` is set (the release gates). When the API's anonymous limit is hit, Stable
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
