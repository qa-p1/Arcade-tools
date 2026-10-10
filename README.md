# Arcade Tools

Install, update, repair, launch and remove the Arcade desktop apps (Box, Lens,
Look, Wheel, Clipboard, Shelf and Find). Choose
stable or nightly for each app and manage start at login. The manager is optional.
Every Arcade app works independently and can still be installed and updated by hand.

Tools reads the Arcade catalog, Arcade Link registry and the repositories' GitHub Releases. Each
row shows installation and running state, its version and selected channel. It
doesn't list capabilities, run pipelines or manage devices. Apps installed outside
Tools can be launched; Tools only updates, repairs or removes its own installations.

Install locations are per user, without administrator access:

- **Linux:** AppImages in `~/Applications/Arcade/`. Clipboard tarballs run their
  verified `scripts/install-linux.sh`, installing to
  `${XDG_DATA_HOME:-~/.local/share}/arcade-clipboard/`. Shelf's runtime bundle
  is extracted (no scripts, no links) to `${XDG_DATA_HOME:-~/.local/share}/arcade-shelf/`.
- **Windows:** the app's per-user NSIS or Inno installer, with the release's
  validated silent arguments, under `%LOCALAPPDATA%\Programs\<app name>\`.
- **macOS:** mount the disk image read-only with `hdiutil`, then copy its app
  bundle to `~/Applications/` with `ditto`, preserving extended attributes.

Updates download and verify first, ask the running app to quit, replace its
installation and reopen it in the same mode. Busy apps keep running: finish the
job and retry. If an app doesn't report its window mode, Tools asks how to reopen
it before closing anything. A disconnected app must be closed explicitly first.
Replacement failures restore the previous files. Cancellation is available during
download, and stops being available once installation commits changes.

Uninstall keeps settings and data by default. The **Remove settings and data**
checkbox lists the exact folders that will be removed. Start-at-login changes read
and back up existing entries, preserve unrelated Linux entry fields, validate the
result, and reject temporary executable locations. Wheel and Clipboard own native
macOS login items; their toggle is hidden here and changed in the app's settings.

Downloads come only from the app's GitHub Releases over HTTPS. Tools validates
`arcade-release.json` (schema 1), app/channel/protocol/OS/architecture, then verifies
the installer's SHA-256 and declared size before executing or closing an app.
HTTPS and checksums protect transport and integrity; they are not publisher
signatures. Minisign verification and release signing remain future hardening.
Tools never removes macOS quarantine or disables/bypasses SmartScreen. Link uses
same-user local IPC; incoming requests still require the Tools confirmation dialog.

`arcade.tools` exposes one interactive action, `tools.install`, accepting a canonical
app ID through `text/plain` or `options.app`. A "Get" button in another app can open
the install confirmation. This request never silently installs anything. Turning
off **Connect with other Arcade apps** removes the listener and exposed actions;
installation management remains available.

```sh
arcade-tools --version
arcade-tools --arcade-manifest   # JSON; no GUI or registry writes
arcade-tools --background       # hidden until activated or handed an install request
arcade-tools --quit             # refuses while an operation is running
```

Tauri 2 uses the operating system's webview with vanilla TypeScript. There is no
framework runtime or bundled Chromium. `npm run build` produces a small HTML/JS/CSS
shell; `python3 scripts/ui-size.py` checks it against a 17 KB uncompressed budget.
Registry changes arrive through a directory watch. The hub core exposes one update
deadline for the desktop layer to wire while Tools runs; it creates no background
threads. All discovery, network, IPC and installer work belongs on worker threads.

## Core development

The package stays at **0.1.0**, using Link **v0.2.0**. These hub APIs are ready for
the desktop/UI task to wire; the UI in this checkout has not gained new settings,
Learn/Engines pages, notifications or an update timer:

- `catalog`: `CatalogClient::cached_catalog` reads the last good cache or a compiled
  snapshot without networking. `refresh_catalog`, `engines`, `shortcut_index` and
  `shortcut_sheet` expose explicit conditional requests. Schema 1, a compiled
  `qa-p1` owner allowlist, HTTPS/redirect restrictions and 1 MiB metadata limits
  apply. Unknown fields are ignored. Data lives in `<cache>/arcade/tools/catalog/`.
- `manager`: catalog entries control list membership, names, pitches and display
  fields. Compiled `apps`/`paths`/`platform` contracts still control native install
  formats, executable/data/login locations and the seven supported installers.
  Future catalog IDs can be displayed but cannot be installed without a supported
  contract. `minTools` is checked before install/update/repair. The vendored catalog
  currently requires Tools **0.2.0**, so this 0.1.0 core reports **Update Arcade Tools
  to install this**; tests provide a compatible fixture catalog.
- `release`/`source`: `check_repo_conditional` binds persisted `ReleaseCache`
  validators to their API URL, app, channel and platform. A 304 avoids manifest and
  installer requests. `ReleaseCache::body` contains the GitHub release markdown;
  the manifest's `notes` URL remains intact. Downloads still verify SHA-256/size.
- `updates`: construct `Scheduler` on a worker using an injectable `Clock` and
  `ReleaseSource`. Set the caller's single process-scoped timer from
  `next_deadline`: 60 seconds after start, then six hours after each check.
  Pass `Trigger::WindowOpen` for checks older than one hour. `updates.enabled`
  defaults on; `Manager::set_updates_enabled` persists it. Known offline state
  sends no requests; transport failures stop a check and are recorded. Stable and
  nightly results, peer-protocol/minimum-Tools holds, timestamps and notification
  flags persist in `updates.json`. Returned notifications occur once per app,
  channel and version; delivery is the caller's responsibility. `reconcile`
  updates display state after registry/install events without networking.
  `set_auto_update` persists per-app opt-in; `auto_update_policy` requires a known
  non-busy status and ten minutes since use. The caller rechecks policy and
  compatibility before invoking the existing installer. Update/repair outcomes,
  including verified failure rollback, append to `update-history.jsonl` in the
  Tools data directory.
- `markdown`: `render` uses pulldown-cmark and ammonia for release/docs HTML,
  stripping scripts, SVG/MathML, frames, event attributes, inline styles and unsafe
  URLs. Links are http(s)/relative and get `rel="noopener noreferrer"`. Remote
  images require a caller-supplied exact host allowlist; use an empty list offline.
- `docs`: `DocsResolver::resolve` chooses installed docs, exact-version cache, then
  the bundled snapshot. `fetch_version` is an explicit, bounded tag fetch; failed
  refreshes leave the cache intact. The desktop supplies its bundled resource
  path and handles relative-image delivery/navigation. `SearchIndex::build` reads
  markdown on a worker; `search` uses an in-memory inverted index with prefix
  matching. Cache: `<cache>/arcade/tools/docs/<id>/<version>/`.
- `engines`: `EngineCatalog::parse` reads the exact catalog format. `Engine::detect`
  takes an explicit PATH and timeout; `detect_current` uses PATH and a two-second
  timeout. It reports found/missing/too-old and explicit process/version errors,
  considering binary alternatives. `install_hint` selects OS/distro text from
  `/etc/os-release` contents; no install commands run.

Every disk/network/process public API documents its worker-thread requirement.
`Paths::discover` honors `ARCADE_HOME` for all roots, without persisting environment
changes. `ARCADE_TOOLS_CATALOG_URL` and `CatalogClient::with_url` exist only in debug
builds; loopback HTTP additionally requires `Source::test_server` (the desktop's
existing isolated `ARCADE_TOOLS_TEST_SOURCE` mechanism). Release builds ignore the
catalog override. Built-in catalog/engine data is attributed and licensed under
`resources/catalog-snapshot/`.

Build-time docs use the standard-library-only script:

```sh
python3 scripts/docs-snapshot.py  # stable tags + catalog main, resources/docs output
python3 scripts/docs-snapshot.py --from-local /path/to/Coding --out /tmp/docs-snapshot-test
python3 scripts/test-docs-snapshot.py
```

The default output is `src-tauri/resources/docs/<id>/<version>/`, with a schema-1
`manifest.json` and an **8,000,000-byte total limit including the manifest**.
Missing user docs or stable releases are recorded and skipped. Local mode uses
working-tree files labeled `local`, not a release version. Bundle creation and
Tauri resource wiring belong to the desktop/release task; no generated docs bundle
is committed here. No docs requests happen implicitly at runtime.

Arcade Link comes from its `v0.2.0` git tag. The isolated test runner
(`tools/e2e.py`) lives in the Link repository, so keep a checkout beside this
one for the commands below:

```sh
npm ci
npm run typecheck
npm run build
export CARGO_BUILD_JOBS=3
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
# Non-lifecycle tests (lifecycle tests require the isolated runner below).
cargo test --workspace --locked -- \
  --skip appimage_install_update_repair_uninstall_keeps_then_removes_data \
  --skip checksum_failure_cancel_and_disabled_peer_never_replace_installed_files \
  --skip clipboard_tarball_runs_the_verified_install_script_and_repairs \
  --skip stable_nightly_channel_switch_and_unknown_apps \
  --skip busy_is_retryable_and_update_restores_the_reported_background_mode \
  --skip unknown_mode_requires_a_choice_and_foreground_is_preserved \
  --skip quit_timeout_leaves_the_original_installation_intact
# Lifecycle tests launch executables and must use the isolated runner.
CARGO_HOME="$HOME/.cargo" python3 ../Arcade-link/tools/e2e.py run -- cargo test --workspace --locked
cargo build -p arcade-tools --features custom-protocol
python3 ../Arcade-link/tools/e2e.py --only tools
```

Use `npm run tauri -- dev` for the Vite development server, inside the same isolated
runner. Direct test binaries use `custom-protocol` to embed the built assets instead
of requiring that server. GUI checks use a debug-only loopback release server and
observational DOM logs inside one disposable HOME/XDG/ARCADE root; release builds
ignore those environment variables. The check driver clicks the actual native GUI
and runs a real Lens instance for busy/update/quit behavior.

The baseline's Linux X11 GUI checks use private D-Bus/Xvfb. This hub branch was
verified through Linux core/fixture tests, without a fresh real-GUI run. Linux
Wayland is **not run** here. Windows/macOS CI results describe the baseline;
this branch has not run their CI or real desktops. Their installer, location and
login logic is also tested on Linux. Current status: [docs/STATUS.md](docs/STATUS.md).

CI checks fmt, clippy, Rust tests and the frontend on Linux, Windows and macOS.
The core uses Rust 1.88-compatible APIs/dependencies. The pre-existing locked
Tauri 2.12 stack declares Rust 1.90 requirements, so the whole workspace cannot
honestly claim Rust 1.88 support until the desktop task resolves those versions.
The release workflow bundles NSIS, AppImage and universal macOS dmg installers,
then uses the canonical generator in [VENDORED](VENDORED) to create
`arcade-release.json` and `SHA256SUMS.txt`. Version tags publish stable releases;
manual dispatch can publish the rolling nightly. Nothing has been published from
this checkout.
