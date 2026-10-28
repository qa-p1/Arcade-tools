# Testing

`scripts/check` runs what CI runs on the current OS. On Linux it needs
Playwright's Chromium (`npx playwright install chromium`) and an Arcade-Link
checkout at the pinned tag (`../Arcade-link` or `$ARCADE_LINK`).

| Check | Command |
|---|---|
| Types and build | `npm run typecheck`, `npm run build`, `python3 scripts/ui-size.py` |
| UI behaviour (mock backend) | `npm run ui-checks`; see `scripts/ui-checks/README.md` |
| Vendored files, versions | `python3 scripts/verify-vendored.py`, `python3 scripts/check-versions.py` |
| Docs | `python3 scripts/test-docs-snapshot.py`, `python3 scripts/docs-snapshot.py --check-links docs/user src-tauri/resources/docs` |
| Rust | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` |
| Overlay | `cargo test -p arcade-tools-overlay`, `python3 crates/arcade-tools-overlay/tests/render_headless.py` (private Xvfb) |

On Linux the Rust tests run inside Link's disposable desktop session
(`python3 ../Arcade-link/tools/e2e.py run -- cargo test --workspace --locked`):
a private HOME, XDG folders, `ARCADE_HOME`, D-Bus and Xvfb. Tests never touch a
real Arcade installation, and isolation checks compare against the session's root,
not a fixed `/tmp`.

The real-package test in `tests/find_artifact.rs` is ignored here and runs in
Link's e2e job against a released Find AppImage. Link's nightly e2e also installs,
updates, repairs and removes Tools from the main-branch installer artifact.
