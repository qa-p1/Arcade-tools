# Arcade Tools

Install, update, repair, launch and remove the five Arcade desktop apps. Choose
stable or nightly for each app and manage start at login. The manager is optional.
Every Arcade app works independently and can still be installed and updated by hand.

Tools reads the Arcade Link registry and the repositories' GitHub Releases. Each
row shows installation and running state, its version and selected channel. It
doesn't list capabilities, run pipelines or manage devices. Apps installed outside
Tools can be launched; Tools only updates, repairs or removes its own installations.

Install locations are per user, without administrator access:

- **Linux:** AppImages in `~/Applications/Arcade/`. Clipboard tarballs run their
  verified `scripts/install-linux.sh`, installing to
  `${XDG_DATA_HOME:-~/.local/share}/arcade-clipboard/`.
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
No update polling runs while idle. Registry changes arrive through a directory
watch, and all discovery, network, IPC and installer work runs on worker threads.

Arcade Link comes from its `v0.1.0` git tag. The isolated test runner
(`tools/e2e.py`) lives in the Link repository, so keep a checkout beside this
one for the commands below:

```sh
npm ci
npm run typecheck
npm run build
export CARGO_BUILD_JOBS=3
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
# Lifecycle tests launch executables and must use the isolated runner.
CARGO_HOME="$HOME/.cargo" python3 ../Arcade-link/tools/e2e.py run -- cargo test --workspace
cargo build -p arcade-tools --features custom-protocol
python3 ../Arcade-link/tools/e2e.py --only tools
```

Use `npm run tauri -- dev` for the Vite development server, inside the same isolated
runner. Direct test binaries use `custom-protocol` to embed the built assets instead
of requiring that server. GUI checks use a debug-only loopback release server and
observational DOM logs inside one disposable HOME/XDG/ARCADE root; release builds
ignore those environment variables. The check driver clicks the actual native GUI
and runs a real Lens instance for busy/update/quit behavior.

Linux X11 is tested under private D-Bus/Xvfb. Linux Wayland is **not run** here.
Windows and macOS are **build only**: their installer, location and login logic is
unit tested on Linux and native CI jobs are defined; their native builds and real
desktop runs have not been performed on this machine.

CI checks fmt, clippy, Rust tests and the frontend on Linux, Windows and macOS.
The release workflow bundles NSIS, AppImage and universal macOS dmg installers,
then uses the canonical generator in [VENDORED](VENDORED) to create
`arcade-release.json` and `SHA256SUMS.txt`. Version tags publish stable releases;
manual dispatch can publish the rolling nightly. Nothing has been published from
this checkout.
