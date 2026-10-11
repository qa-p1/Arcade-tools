# Arcade Tools: status

Verified on Linux on 2026-10-11 in `arcade/tools-hub-core`, based on
`bdefa44`. Tools remains **0.1.0** and Arcade Link remains **v0.2.0**.
The [README](../README.md#core-development) describes the public APIs and how
the desktop layer should call them.

## Implemented

- Install, update, repair, launch and remove the seven Arcade apps per user.
  Release manifests, SHA-256 and sizes are verified before execution or quit.
  Failed replacements restore the previous files; successful updates restore
  the reported foreground/background mode. Busy apps are never force-quit.
- A schema-1 catalog client with an owner allowlist, bounded conditional HTTPS
  requests, last-good disk cache and compiled catalog/engine snapshots. Catalog
  data controls membership and presentation; compiled contracts still control
  installers and native paths. Engines and shortcut sheets load on demand.
- An injectable update scheduler with the 60-second/six-hour/one-hour window
  schedule, persisted release ETags, per-channel state, compatibility holds and
  notification flags. `updates.enabled` defaults on and is honored at every
  wake. Offline checks record a failure without requests. The desktop owns the
  single timer and notification delivery; the core starts neither.
- Per-app auto-update opt-in and a pure eligibility decision requiring a known
  non-busy status and ten minutes since use. Update/repair history is JSON lines
  in the Tools data directory, including verified rollback outcomes.
- Rust Markdown rendering with a tight HTML/URL/image allowlist. Versioned docs
  resolve from installed files, exact-version cache, then a bundled snapshot;
  explicit fetching preserves an existing cache on failure. Search is an
  in-memory inverted index with prefix matching.
- A standard-library-only docs snapshot script with local and GitHub modes,
  missing-doc records, text/images and an 8,000,000-byte budget including its
  manifest. Optional engine detection uses PATH, bounded worker processes,
  version/minimum checks and OS/distro install hints; it never installs anything.
- Synchronous disk/network/process APIs document worker-thread requirements.
  `ARCADE_HOME` isolates all roots without persisting environment changes.

## Verification

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- Workspace tests outside the isolated session, excluding the seven lifecycle
  tests using the README command: **35 passed, 7 filtered out**.
- `CARGO_HOME="$HOME/.cargo" python3 ../Arcade-link/tools/e2e.py run -- cargo test
  --workspace --locked`: **42 passed, 0 failed**. This includes fixture executable
  lifecycle/rollback/quit-timeout checks and the three Python snapshot tests.
- `python3 scripts/docs-snapshot.py --from-local /home/harneet/Work/Coding --out
  /tmp/docs-snapshot-test`: exit 0, **8 entries, 1,478/8,000,000 bytes**. All eight
  local sources currently lack the requested user/ecosystem docs; their manifest
  records `no_docs`. Separate fixtures exercise real Markdown/image copies,
  stable-tag selection, missing GitHub docs, budget rejection and cache retention.
- Catalog and release tests use the existing local HTTP server, with request and
  conditional-header assertions. Scheduler tests inject time; no tests require
  outbound network. Markdown tests cover script, event, JavaScript/data URL, SVG,
  CSS and iframe payloads. Engine tests run fake binaries in temporary directories,
  including a timeout. Docs tests check fallback order and search after files have
  been removed, proving queries perform no disk reads.

The workspace was built/tested with system Rust **1.99**. Added dependencies and
core APIs support Rust 1.88; a Rust 1.88 build was not run because this environment
has no such toolchain/rustup. The baseline lockfile already pins Tauri 2.12 and
related desktop crates declaring Rust **1.90**. Their versions must be resolved
by the desktop task before whole-workspace Rust 1.88 CI can pass.

## Limits and integration still to do

- The live development catalog and its unchanged vendored copy specify
  `minTools: 0.2.0` for all apps. This 0.1.0 core correctly displays **Update Arcade
  Tools to install this** and holds install/update/repair. Lifecycle tests supply
  a compatible 0.1.0 fixture catalog. Catalog publication and a Tools version
  change belong to the coordinated release, not this task.
- The UI/desktop task must wire the core APIs, settings, timer, notifications,
  Learn/Engines views and bundled docs resource path. No UI or Tauri source was
  changed here; no generated docs bundle is committed.
- Receipts, adoption and hosted tray behavior await Arcade Link v0.3.0. This task
  keeps the existing v0.2.0 IPC contract.
- This branch has no fresh frontend build, real-GUI ecosystem e2e, Windows/macOS
  CI or real-desktop validation. The previous status page's 74 ecosystem checks
  and CI results described the earlier implementation and were not rerun here.
  Linux Wayland remains untested.
- HTTPS and checksums verify transport/integrity; release signing remains future
  work. Nothing was pushed, merged or published from this worktree.
