# Decisions

- **Tools is optional.** Every app works and connects to the others without it;
  Tools only installs, updates and gathers them.
- **No silent installs.** A Link request to install opens a confirmation screen.
- **One version source.** `[workspace.package] version` in `Cargo.toml`; the npm
  and Tauri copies are checked, not hand-edited (0.4.0).
- **The design kit is vendored, not linked.** `src-ui/design/` is copied from the
  pinned Link tag and hash-checked, so a build never depends on a sibling checkout (0.4.0).
- **Folders come from Link.** Tools moved its own data to `arcade/tools` in 0.4.0 and
  knows every app's old and new folders for backup, restore and uninstall.
- **The bundle identifier stays `app.arcadetools`.** Changing it would move the macOS
  bundle's preferences and break self-update from 0.3; the webview storage now lives
  in Tools' own cache folder, so nothing else depends on it.
- **The overlay is a separate native process** that starts on demand and exits on
  close, so the shortcut sheet costs no memory while hidden.
- **User guides link only inside `docs/user/`** or to absolute URLs, because Learn
  shows them offline; the snapshot rewrites any other link to its GitHub URL.
