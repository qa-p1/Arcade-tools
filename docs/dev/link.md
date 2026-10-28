# Arcade Link

Tools uses Arcade Link as a git dependency pinned by tag in both Cargo manifests
(`tag = "v0.4.0"`, features `watch`, `install`, `engines`, `trayhost`, plus `design`
for the desktop shell and the overlay). The design kit in `src-ui/design/` comes from
the same tag (`VENDORED`, checked by `scripts/verify-vendored.py`).

- **Presence**: Tools publishes its `arcade.tools` manifest and reads the other apps' presence for
  Home, Apps and Connections. With "Connect apps" off it stops its Link server, hosts no
  icons and its manifest lists no actions (`src/link.rs`, `tray/hosting.rs`).
- **Tray hosting**: `tray/hosting.rs` attaches `tray.host` before announcing that it
  hosts icons; apps fall back to their own icons when Tools quits.
- **Requests**: `tools.install` from another app opens a confirmation screen; Tools
  never installs silently (`src/link.rs`).
- **Folders and migration**: `arcade_link::paths` gives each app's folders and old
  locations, so backup, restore, uninstall and the shortcut sheet handle apps before
  and after they move.
- **Diagnostics**: "Copy diagnostics" starts with `arcade_link::diagnostics::Header`.

## Moving to a new Link tag

1. Update `tag` in `Cargo.toml` and `src-tauri/Cargo.toml`, then `cargo update -p arcade-link`.
2. Check out Link at that tag and run `python3 scripts/sync-design.py --link <checkout>`.
3. Update the `ref` of the Link checkout in `.github/workflows/ci.yml`.
4. Run `scripts/check`.
