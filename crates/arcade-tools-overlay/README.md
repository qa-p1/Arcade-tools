# Arcade Tools shortcut overlay

An on-demand native shortcut sheet. The process reads the prebuilt schema-1
index, captures the focused application before mapping, shows the sheet, and
exits on close. It does not build the index or evaluate shortcut configurations.

## Run

```sh
cargo build --release -p arcade-tools-overlay --locked
target/release/arcade-tools-overlay --toggle
target/release/arcade-tools-overlay --show --index /path/to/shortcuts-index.json
target/release/arcade-tools-overlay --hide
target/release/arcade-tools-overlay --version
```

`--toggle` is the normal shortcut command. A per-user lock and authenticated
local socket forward commands to the visible instance. Unix uses a private
runtime socket; Windows uses a named pipe. Closing destroys the window and exits
the process, including its IPC listener. An empty lock file may remain so its
inode can safely be reused. There is no resident overlay daemon.
The launcher confirms receipt of the IPC acknowledgement before the UI handles
a close command, so immediate process exit cannot lose the launcher's reply.

The default index is `<cache>/arcade/tools/shortcuts-index.json`, using
`XDG_CACHE_HOME` or `~/.cache` on Linux and the platform cache directory elsewhere.
`ARCADE_HOME` overrides it with
`$ARCADE_HOME/cache/arcade/tools/shortcuts-index.json` and also isolates instance
state under `$ARCADE_HOME/run/tools-overlay`. `--index` overrides only the index
path. Unknown JSON fields are ignored and `wm: null` is accepted. Missing,
corrupt, oversized, or incompatible indexes display:

> Open Arcade Tools once to build the shortcut index

## Interaction and platforms

The header names the matched application. The focused search field searches
titles, key displays, group names, and descriptions across all indexed apps,
Arcade globals, and window-manager bindings. Words match substrings or ordered
subsequences. Conflict participants receive a small `!` badge. In-app Arcade
shortcuts appear in **This app**; **Arcade** contains only global shortcuts.

| Input | Action |
| --- | --- |
| Type / Backspace / Ctrl+U | Filter / edit / clear search |
| Tab / Shift+Tab | Cycle This app → Arcade → Window manager → All |
| Arrows / PageUp / PageDown / Home / End | Scroll |
| Ctrl+P or Choose app | Open the application picker |
| Type, arrows, Enter in picker | Find and select an indexed application |
| Esc / outside click / `--hide` / `--toggle` | Close and exit |

Hyprland focus detection reads `j/activewindow` directly from its instance
socket. Sway reads the focused node from the framed `get_tree` IPC reply. X11
reads `_NET_ACTIVE_WINDOW` and `WM_CLASS` only when X11 is in use. Windows reads
the foreground process executable; macOS reads the frontmost application's
bundle identifier. Detection has a total 50 ms deadline and falls back to All.
Matching is case-insensitive, including Flatpak classes and Arcade match lists.
GNOME/KDE Wayland and unidentified apps start on All with the app picker open.

The Wayland backend uses SCTK layer shell when advertised, with exclusive
keyboard interaction while visible. It releases that interaction and destroys
the surface on close. A transparent full-output click catcher surrounds the
fully opaque sheet. Other desktops use a borderless winit/softbuffer window.
Explicit X11/Windows close restores the saved foreground window; macOS hides
the accessory application, and Wayland hands focus back when the surface is
destroyed. Losing focus by clicking another application closes without moving
focus away from that application. The renderer supports native DPI and Wayland
fractional scaling, GTK/native light and dark preferences, and has no animation.

## Tools integration

The crate exports `registration::register(accelerator)` and
`registration::unregister()`. Registration returns `{ via, hint }`, with `via`
serialized as `native`, `hyprland-runtime`, or `manual`. Defaults are `Super+/`
on Linux, `Ctrl+Alt+/` on Windows, and `Ctrl+Option+/` on macOS.

Call both functions on the Tools main thread. Keep the native event loop
running, and call `unregister()` on Tools quit. Native registration owns
global-hotkey's process-wide event handler and launches the packaged sibling
`arcade-tools-overlay --toggle` executable. Integrate this handler with any
other global-hotkey users in Tools when wiring the hub.

Hyprland registration copies Find's Lua 0.56 probe and runtime approach:
`hyprctl eval 'return hl ~= nil'`, followed by `hl.bind(...,
hl.dsp.exec_cmd(...))`, with a legacy `keyword bind` fallback. A blocking event
socket watcher restores the runtime binding after config reload. Unregister
stops the watcher and removes the binding. No configuration files are edited.
Other Wayland desktops return the manual hint:

> Bind `arcade-tools-overlay --toggle` in Settings → Keyboard → Custom Shortcuts

These helpers have pure parser/argument/hint tests. Tests never register a
shortcut on the live desktop. Wiring Tools startup/quit and shipping the second
executable in the Tauri package remain the hub's integration work; this crate
does not modify `src-tauri`.

## Dependencies and attribution

Rendering follows Arcade Find's native overlay, copied with source-file
attribution under MIT OR Apache-2.0. There is no dependency on Find.

| Dependency | Purpose |
| --- | --- |
| winit 0.30.13, softbuffer 0.4 | Native window and software presentation |
| cosmic-text 0.15, tiny-skia 0.11 | Cached text shaping and opaque software drawing |
| smithay-client-toolkit 0.19, wayland-client 0.31 | Linux layer shell, input, scaling, shared-memory buffers |
| x11rb 0.13 | X11 focused app and focus restoration |
| global-hotkey 0.8 | Native global shortcut registration |
| serde 1, serde_json 1 | Pinned index and authenticated instance messages |
| directories 6, fs2 0.4, getrandom 0.3, interprocess 2.4 | Cache/runtime paths, lock, token, local IPC |
| libc 0.2, windows-sys 0.61, objc2 0.6, objc2-app-kit/objc2-foundation 0.3 | Platform APIs |
| tempfile 3; Linux-only wayland-protocols-wlr 0.3 | Tests and isolated virtual-pointer example |

Find's dependency versions are retained where practical. A known system UI
font is loaded directly to avoid scanning the entire font database at startup.
Unusual installations without a known font fall back to a system font scan.
Icons and resvg are omitted. Windows IPC checks available bytes with
[PeekNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe)
before reading, so waiting for a reply is bounded without treating an empty
nonblocking pipe as EOF.

## Checks and measurement

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p arcade-tools-overlay --locked
cargo test --workspace --no-run --locked
cargo build --release -p arcade-tools-overlay --locked
python3 crates/arcade-tools-overlay/tests/render_headless.py
```

The Xvfb test creates its own display, HOME, XDG directories, Arcade directory,
and focused Firefox window. It checks first-frame presentation, filtering,
scope/scroll keys, app selection, theme, missing/corrupt/default index paths,
focus restoration, single-instance show/toggle/hide, outside-click close, and
process exit. It saves screenshots, phase logs, and timing samples under
`target/overlay-smoke`. It needs Xvfb, xdotool, ImageMagick, and libX11.

The optional Wayland test uses a private headless Sway compositor:

```sh
cargo build -p arcade-tools-overlay --example overlay-pointer --locked
python3 crates/arcade-tools-overlay/tests/render_wayland.py \
  --pointer target/debug/examples/overlay-pointer
```

It checks layer-shell frames, IPC close, 150% scaling, and outside-click close.
It needs Sway, swaymsg, grim, and timeout. Its generated compositor config and
all environment overrides last only for the test process/session. The pointer
helper requires `WLR_BACKENDS=headless`. Neither test contacts the live desktop.

Set `ARCADE_OVERLAY_TIMING=1` to print startup phases and
`spawn_to_first_frame_ms`. The internal clock starts at entry to `main`, so it
excludes executable loading. The Xvfb/Wayland harnesses also measure from
`Popen` until the first-frame marker, including loading and stderr delivery.
Desktop timing ends after softbuffer presentation; layer-shell timing ends at
the compositor frame callback. These are launcher-to-frame measurements;
hotkey dispatch and packaging integration need a separate end-to-end check.

The overlay tests also pass using the actual Rust 1.88 compiler. Windows GNU
and Apple ARM64 targets pass Rust 1.88 checks for all targets; their native GUI
and hotkey paths have not been run on this Linux machine. CI adds a Rust 1.88
overlay build/test/Clippy matrix on Ubuntu, Windows, and macOS plus the isolated
Linux render test. Existing workspace dependencies already include packages
requiring Rust 1.90; the full workspace check continues to use stable Rust and
the overlay job separately enforces this crate's 1.88 MSRV.

Measured on the development Linux machine on 2026-10-11, using the fixture
index and the stripped release build (5,386,712 bytes / 5.14 MiB):

| Session | Samples | Entry-to-frame | Launcher-to-frame |
| --- | --- | --- | --- |
| Quiet Xvfb, final binary | 20 | median 20.15 ms; p95 22.84 ms | p95 25.27 ms |
| Quiet headless Sway, final binary | 20 | median 27.14 ms; p95 28.52 ms | p95 30.22 ms |
| Live Hyprland, one permitted opening | 1 | 35.25 ms | 38.40 ms |
| Xvfb during concurrent Rust/npm builds | 20 | p95 157.28 ms | p95 169.33 ms |
| Headless Sway during concurrent builds | 20 | p95 198.77 ms | p95 206.77 ms |

The live Hyprland opening was closed immediately using this instance's `--hide`.
No live bindings were registered or changed. It preceded the final IPC
acknowledgement fix and Windows-only subsystem attribute; the first-frame
rendering path was unchanged by those fixes. Its focus query took 0.27 ms.
One live sample does **not** establish Hyprland p95 or keypress-to-frame latency.
The 80 ms Hyprland p95 budget still needs a repeated end-to-end check after hub
hotkey/packaging integration. Concurrent compilation materially affects timing.

Screenshots and complete sample/phase files are retained locally under
`target/overlay-smoke-final`, `target/overlay-smoke-loaded`,
`target/overlay-wayland-final`, `target/overlay-wayland-quiet`, and
`target/overlay-hyprland`; they are ignored build artifacts. The isolated Sway
run passed 20 repeated launches/closes with the acknowledgement fix, plus the
fractional-scale and outside-click checks. The quiet Sway run also passed 20
launches/closes; its slowest launcher-to-frame sample was 39.93 ms.
