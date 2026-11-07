# Spec 2: Arcade Tools 1.0.0

**Outcome:** Tools 1.0.0, the hub that installs, updates and hosts the fleet. It delivers:
- signed, chunked delta updates from one shared chunk store;
- a working per-app Nightly switch;
- a hidden rc `test` channel;
- a bounded installer cache;
- local crash reports;
- every audit finding fixed;
- the fleet rules.

Tools must update itself through the new updater twice before 1.0.0 (`PLAN.md` §Safety).

Starting point: `qa-p1/Arcade-tools`, Tools 0.4.6. Read `AUDIT/tools.md` (with the updater walkthrough and its line
references), `AUDIT/real-desktop.md` §Tools and `AUDIT/FINDINGS.md` T1–T4.

## Findings to fix

| ID | P | What | Where (as of 0.4.6) |
|---|---|---|---|
| TL-8 / T1 | P0 | Home polls every 2 s, and each poll does an uncached full SHA-256 of every unmanaged AppImage: 12% CPU and about 4 GB read in 20 s. | `src-ui/tabs/Home.svelte:66-72`, `src/manager.rs:1163` |
| TL-11 | P0 | 120 s total HTTP timeout, so downloads slower than about 6 Mbit/s fail; no resume. | `src/source.rs:105-106` |
| TL-9 | P0 | Choosing Nightly only stores a preference: nothing switches, no badge. | `src/manager.rs:386`, Apps card/Details |
| TL-7 / T2 | P1 | The installer cache is never pruned (1.1 GB on the owner's machine). | `src/installation.rs:37-75`, `src/manager.rs:1059-1079` |
| TL-4 / T4 | P1 | `--status` opens the GUI and prints nothing. | `src-tauri/src/main.rs:138` |
| TL-1 | P1 | The CLI `--show-shortcuts` spawns the overlay from its own short-lived mount, which then dies with SIGBUS. | `src-tauri/src/main.rs:101-107`, `hotkey.rs:21-45` |
| TL-5 | P1 | The X11 overlay segfaults in the bundled `libxkbcommon` mixed with the host's `libxkbcommon-x11`. | packaging; `crates/arcade-tools-overlay/src/ui/desktop.rs` |
| TL-2 | P1 | The shortcut overlay closes on focus loss (X11; the fleet popup rule). | `crates/arcade-tools-overlay/src/ui/desktop.rs:239-241` |
| TL-6 / RD-TOOLS-5 | P1 | Escape closes the overlay even with a filter typed. | `crates/arcade-tools-overlay/src/ui/mod.rs:60` |
| TL-3 / RD-TOOLS-4 | P1 | Escape does nothing on any page, guide or details view. | `src-ui/App.svelte`, `src-ui/components/AppDetail.svelte:120` |
| RD-TOOLS-1 | P1 | Shortcuts → Conflicts shows 6 false conflicts: each app against its own Hyprland bind. | `crates/arcade-shortcuts/src/index.rs:723-742` |
| RD-TOOLS-2 | P1 | Engines page: every engine reports "version check timed out" (likely a SIGCHLD/GLib reaping race). | `src/engines.rs:110-130,212-240` |
| RD-TOOLS-3 | P2 | App details show "Latest: Not checked" with Update enabled while Home says updates were checked. | Details view |
| TL-10 | P2 | About says "Development build" for the released, self-installed Tools. | Settings › About |
| TL-12 | P2 | Health list shows the raw id `arcade.tools`; optional Look plugin engines show as "needs attention"; backup shows no progress; guides lag the running version. | Home, Engines, Backup |
| T3 | P2 | Legacy `~/.local/share/arcade-tools` and `app.arcadetools` folders are never retired, and every start logs "Left … in place". | migration code |
| RD-TOOLS-6 | P2 | Window class is `arcade-tools`, not `dev.arcade.Tools`. | Tauri config |

## Updater work (detailed design in `specs/LINK.md` §3)

1. **Channels:** `Stable`, `Nightly` and `Test`.
   - `Test` is never shown in the UI. It can be selected only with `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` or a hidden
     CLI flag, for the release gates.
   - Discovery uses `/releases` listing: an `X.Y.Z-rc.N` prerelease is a Test release.
   - Nightly becomes promoted, versioned prereleases (`X.Y.Z-nightly.YYYYMMDD.N`), not the single moving `nightly`
     tag.
2. **Nightly switch:**
   - Setting an app to Nightly installs the newest nightly right away, through the normal reviewed operation.
   - The card and Details show a "Nightly" badge.
   - Setting it back to Stable installs the newest stable even when its version is lower. Back up the app's data
     first, then run the downgrade rule from `README.md`.
   - The selected and the installed channel are shown separately while an operation is pending.
3. **Signed manifests:** check `manifest.json.sig` against the built-in keys (current and next). Without a valid
   signature, stable and nightly updates are refused. The release SHA-256 alone is no longer trusted.
4. **Chunked delta downloads:**
   - Build a chunk index of every installed and retained AppImage in a shared store (`cache/arcade/tools/chunks/`,
     content-addressed by BLAKE3). Download only missing chunks, with HTTP Range against the release asset.
   - Assemble and verify the size, SHA-256 and signature, then go through the existing stage → transaction → swap →
     rollback path.
   - Fallback ladder: delta → full download with resume → error. Show progress as bytes needed, not file size.
5. **Timeouts:** a 30 s read-idle timeout instead of a total timeout, plus resumable downloads (TL-11).
6. **Retention:** keep exactly the installed version and one rollback version per app. Prune everything else at
   startup and after each successful update. Settings shows the space used, with "Clear" (TL-7).
7. **Catalog `bad` flag:** a release marked bad is never offered. If it's installed, Tools offers a rollback with a
   clear notice.
8. **Self-update** uses the same path. Gate: rc.1 → rc.2 → 1.0.0 must each self-update with a delta, verified by
   the release gates.

## Crash reports (fleet feature, hosted in Tools)

Apps write crash reports through Link (`specs/LINK.md` §2). Tools shows "<App> crashed" with "Copy report" and
"Open GitHub issue". The issue is pre-filled only after the user has seen the full text. Nothing is ever sent
automatically.

## Memory and performance

- Home polls only while visible, every 10 s, and caches hashes by (size, mtime, inode) (TL-8).
- Apply `AUDIT/memory.md` items U1 (GC runtime folders), W1, OPEN1, W3b and TD1 (Home shows tree PSS). Budget: background 30 PSS / 50 RSS.
- Apply the Tools items from `AUDIT/memory.md`, for example releasing the WebKit network process and idle pages on
  hide, and running unpacked when installed. Measured: background goes from 232/137 to 59–75 / 38–55 MB (RSS/PSS)
  after one open and close, with the same 0.40 s reopen.
- Budgets come from `AUDIT/memory.md` and are recorded in `ARCADE_APP.md`.

## Other items

- Uninstall removes everything (`README.md` §Clean uninstall), and backup restore is tested.
- Escape navigation on every page: details → list, guide → guide list, top level does nothing. Footer and tooltips
  must match.
- Unpacked runtime: Tools may install apps as unpacked payloads next to the AppImage (measured as a 17–23 MB stub
  saving per app and half the cold start). That's only if `AUDIT/memory.md` lists it as a work item. Keep the
  AppImage as the downloaded and verified unit, and the unpacked copy as derived and verified.

## CI

Measured: main CI takes 9–36 minutes. Apply `PLAN.md` step 6.

## Acceptance

All `PLAN.md` exit gates, plus:
- Delta update rc.N → rc.N+1 for Tools itself and for one app downloads less than 20% of the file size.
- A tampered chunk and a wrong signature are both rejected, and the fallback works.
- Nightly ↔ Stable switch round trip on a populated app keeps its data.
- The installer cache stays within its bound.
- Tools idle CPU ≈ 0% with its window open on Home for 60 s.
