# Arcade Tools

[Getting started](docs/user/getting-started.md) · [Features](docs/user/features.md) · [Shortcuts](docs/user/shortcuts.md) · [Status](docs/STATUS.md)

Arcade Tools **0.2.0** is the optional home for Box, Lens, Look, Wheel, Clipboard,
Find and Shelf. Browse the catalog, install bundles, adopt existing installations,
update and roll back apps, host their tray menus, find shortcuts, and read guides
without leaving Tools. Every app also works independently.

The Svelte 5 interface has Home, Apps, Updates, Shortcuts, Learn, Connections,
Engines, Settings and Onboarding. Tools asks before installs, adoption, updates,
uninstall, restore and shortcut changes. Automatic updates are opt-in per app,
respect busy state and ten minutes since use, and announce completed changes.

Closing the window destroys its webview. Tray hosting or the cheat-sheet shortcut
keeps the native process resident; Quit Arcade Tools exits it and hands icons back
to apps. App actions use peer-to-peer Arcade Link, pinned to **v0.3.0**.

The separate native `arcade-tools-overlay` ships in every package and starts on
demand. Its default shortcut is Super+/ on Linux, Ctrl+Alt+/ on Windows, and
Ctrl+Option+/ on macOS. Hyprland uses a runtime bind, X11/Windows/macOS use native
registration, and other Wayland desktops offer a manual keyboard-settings hint.
Tools reads desktop configurations and stores labels in its own files.

Downloads are restricted to trusted GitHub HTTPS hosts and verified against
release SHA-256 and size metadata before execution. Diagnostics stays in a local
redacted ZIP for you to attach to an issue. Backup uses app-owned portable settings
handoffs and includes Tools preferences, tray settings, labels and app channels;
restore reviews missing installations before importing settings. Secrets, device
identity, Clipboard history, indexes and caches are excluded.

Packages are per user: Linux AppImage, Windows NSIS, universal macOS dmg. The
Windows installer uses checksum-pinned Link integration for its receipt and PATH
shim. Linux/macOS integration uses Link's installer. No shell profile is changed;
SmartScreen and quarantine protections remain in place.

```sh
arcade-tools --version
arcade-tools --arcade-manifest
arcade-tools --background
arcade-tools --show-shortcuts
arcade-tools --install --silent --no-launch
arcade-tools --integration-status
arcade-tools --repair
arcade-tools --quit
```

The native desktop uses Tauri 2's system webview. The shell budget is 200 KB;
Markdown is rendered and sanitized in Rust, and Learn/Connections load lazily.
Docs snapshots are limited to 8 MB. Local development snapshots are labeled
`local`; the release workflow refreshes stable-tag docs and retains the labeled
bundle if those tags do not yet contain user guides.

For development, use Rust **1.88** or newer, Node 24, and the Linux Tauri/Wayland/X11
libraries listed in `.github/workflows/ci.yml`. Keep a Link v0.3.0 checkout beside
Tools for the isolated test runner. Temporary environments stay scoped to tests.

```sh
npm ci
npm run typecheck
npm run build
python3 scripts/ui-size.py
cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_HOME="$HOME/.cargo" python3 ../Arcade-link-wt-core/tools/e2e.py run -- cargo test --workspace --locked
cargo build --locked -p arcade-tools --features custom-protocol
cargo build --locked -p arcade-tools-overlay
CARGO_HOME="$HOME/.cargo" python3 ../Arcade-link-wt-core/tools/e2e.py run -- python3 scripts/tray-resident-e2e.py --check-budgets
python3 ../Arcade-link-wt-core/tools/e2e.py --only tools-hub
python3 scripts/verify-vendored.py
python3 scripts/test-docs-snapshot.py
```

CI checks the full workspace and frontend on Rust 1.88 for Linux, Windows and
macOS, builds both binaries into actual installers, and runs disposable package
smokes. Overlay render checks use private Xvfb. These are CI/isolated checks;
Windows/macOS and non-Hyprland Wayland are not verified on a real desktop here.
No live Arcade installation is changed by these tests.

Shared sources and their hashes are recorded in [VENDORED](VENDORED). Version tags
publish stable packages.
