# Shortcut builder validation

Measured on the owner's Linux desktop on 2026-10-11, with Hyprland 0.56.2,
Rust 1.99.0 and vendored Lua 5.5. The crate and its dependencies declare support
for Rust 1.88; that compiler is unavailable on this machine, so the MSRV was
not exercised directly.

## Required checks

All commands completed successfully with the updated lockfile:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p arcade-shortcuts --locked -- --nocapture
  unit:     25 passed
  fixtures:  5 passed, 1 timing test ignored
cargo test --workspace --no-run --locked
  Tools core, Tauri, shortcut unit tests and integration tests compiled
```

The ignored timing test was run separately in release mode:

```text
cargo test -p arcade-shortcuts --release --locked --test fixtures \
  measure_owner_and_31_catalog_sheets -- --ignored --nocapture

20 complete builds: median 16.751 ms, p95 17.524 ms, maximum 26.064 ms
Stage medians: evaluate + merge 13.920 ms; build 2.469 ms; publish 0.339 ms
Result: passed (p95 < 150 ms)
```

Each iteration reads the owner fixture, merges the 145-bind runtime snapshot,
reads 31 catalog sheets plus Arcade docs and registry manifests, then
serializes, flushes, syncs and atomically renames the index in a temporary
directory. Buffered serialization avoids a syscall for every JSON fragment;
file sync remains enabled. Compile time is excluded.

## Runtime reconciliation

The default owner fixture records 140 config binds, all active after merging:
140 active, 0 configured-only, 5 runtime-only. Missing `hypr-vars`, `hypr-user`
and `hypremoji` produce three expected warnings. The five unmatched runtime
entries are `Ctrl+Alt+Super+Period` and two registrations each of `CapsLock`
and `NumLock`. The customized fixture installs the copied variable overrides
in a temporary HOME and records 139 binds: the screenshot binding is disabled
in those overrides and restored by the real optional `hypr-user` module.

The real config records 141 binds across 20 read files. Its runtime merge is
141 active, 0 configured-only, 4 runtime-only, with no warnings. `Super+L`
matches `keybinds.lua:67`. The four unmatched entries are the two `CapsLock`
and two `NumLock` registrations above. No explicit definitions for them were
found in the read config files; runtime JSON exposes only opaque `__lua`
callbacks. Their source cannot be recovered, so they retain the label
`Lua function (added at runtime)` and a null origin.

Both runtime lists contain 145 registrations. Collapsing identical Play/Pause
actions produces 144 displayed WM shortcuts. The real index contains all
31 supplied catalog sheets.

## Real-config dump

The debug example was run against the real config without editing it:

```sh
cargo run -p arcade-shortcuts --release --example dump --locked -- \
  --catalog /path/to/Arcade-catalog/shortcuts \
  --output target/shortcuts-real-release.json
```

Config evaluation plus the read-only `hyprctl binds -j` query took 64.545 ms.
The complete rebuild, including atomic publication, took 87.869 ms. JSON on
stdout is printed after the measured rebuild. An earlier unoptimized debug
run without publication took 121.507 ms.

Excerpt (all active):

```text
Super              Launcher             | Caelestia: launcher
Ctrl+Alt+Delete    Session              | Caelestia: session
Super+N            Show sidebar         | Caelestia: sidebar
Ctrl+Alt+C         Clear notifications  | Caelestia: clearNotifs
Super+L            Lock                 | Caelestia: lock
Alt+Super+L        Restore lock
Super+1            Go to workspace
Ctrl+Super+1       Go to workspace group
Super+Left         Focus window left
Shift+Super+Left   Move window left
Super+T            Terminal             | Run: kitty
Super+W            Browser              | Run: firefox
Ctrl+Alt+V         Audio settings       | Run: pwvucontrol
Ctrl+Super+Space   Media toggle         | Caelestia: mediaToggle
Super+Q            Close window
```

The launcher records the release flag and the first line of its multi-line
call (`~/.config/hypr/hyprland/keybinds.lua:54`). The terminal origin is line
160. Media toggle belongs to the Media group at line 180. Restore lock uses
the directly preceding function comment at line 70.

## Coverage and remaining limits

Every implementation module has unit tests. Fixtures also cover the upstream
48-bind Lua example, hyprlang sources/variables/descriptions/submaps, the full
schema-1 index, user labels, sequence-prefix conflicts, OS key selection and
JSON/atomic-write round trips. Sandbox tests exercise discarded writes,
disabled process calls, symlink escapes, missing/erroring modules, unknown
APIs, submaps, infinite loops (including protected calls), memory exhaustion
and bounded Rust-side recording.

Windows/macOS selection and display were fixture-tested; those desktops were
not exercised. OS-reserved conflicts come from the catalog's `windows-system`
and `macos-system` sheets. The four opaque runtime callbacks remain a validation
limit; CI checks the crate on Rust 1.88. The UI and Tauri integration live in
`src-tauri/` and are covered by the app's own tests.
