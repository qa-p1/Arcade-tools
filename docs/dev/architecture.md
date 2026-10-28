# Architecture

Arcade Tools is one Tauri 2 desktop process plus an on-demand native overlay.

| Part | Where | What it does |
|---|---|---|
| Core | `src/` (`arcade-tools-core`) | Catalog, releases, verified installs and updates, receipts, rollback, backup and restore, uninstall plans, engines, docs and diagnostics. No UI code; every effect goes through `Manager` (`src/manager.rs`). |
| Desktop shell | `src-tauri/` (`arcade-tools`) | Window, Tauri commands (`hub.rs`, `installation.rs`, `shortcuts.rs`), tray (`tray/`), resident lifetime (`resident.rs`), global shortcut (`hotkey.rs`), shortcut index service (`index_service.rs`). |
| UI | `src-ui/` | Svelte 5 pages on the vendored design kit in `src-ui/design/` (do not edit; run `scripts/sync-design.py`). Learn and Connections load lazily; the shell has a size budget (`scripts/ui-size.py`). |
| Shortcut sheets | `crates/arcade-shortcuts` | Reads app, desktop and Hyprland bindings into the schema-1 index. |
| Overlay | `crates/arcade-tools-overlay` | The native shortcut sheet: reads the prebuilt index, renders with tiny-skia and cosmic-text on the kit palette and Inter, exits on close. See its README. |

## Data flow

- **Catalog**: GitHub (`src/source.rs`) → `src/catalog.rs` → cached under the Tools cache folder,
  with the bundled snapshot in `resources/catalog-snapshot/` as the offline fallback.
- **Install and update**: release manifest (`arcade-release.json`, validated in `src/release.rs`) →
  download → SHA-256 check → Link's installer transaction (`src/installation.rs`) → receipt.
  Tools updates itself the same way through a helper (`src/self_update.rs`).
- **Folders**: everything Tools owns comes from Link's `paths` module (`src/paths.rs`): data in
  `arcade/tools`, cache in the cache folder's `arcade/tools`, logs in `<data>/logs`, the webview's
  storage in `<cache>/webview`. 0.3 folders (`arcade-tools`, `app.arcadetools`) move on first start.

## Threads

Tauri commands never block the UI thread: network, file and Link work runs on
`spawn_blocking` workers. The tray model is cached and shared by both tray
renderers (`tray/model.rs`).
