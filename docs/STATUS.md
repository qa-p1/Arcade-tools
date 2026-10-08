# Arcade Tools: status

Verified 2026-10-08 on `main` (version 0.1.0, Arcade Link `v0.1.0`). This
page records what is implemented and how it was checked; the
[README](../README.md) describes how it works.

## Implemented

- Install, update, repair, launch and remove Box, Lens, Look, Wheel and
  Clipboard per user, with a stable or nightly channel per app and start at
  login, from the apps' GitHub Releases.
- Downloads are checked against `arcade-release.json` (schema 1) and the
  installer's SHA-256 and size before anything runs or any app is closed.
  Updates quit the app through the Link, replace it, roll back on failure and
  reopen it in the same mode.
- Arcade Link: `tools.install` (always confirmed in Tools), used by the
  Connected apps "Get" buttons of the other apps.

## Verification

| Check | Result |
|---|---|
| `cargo test --workspace` (through the isolated runner) | 15 passed |
| `cargo clippy --workspace --all-targets -- -D warnings`, typecheck, frontend build, UI size budget | clean |
| CI (Linux, Windows, macOS; Linux lifecycle tests in the isolated runner) | passing at `10e7699` |
| Arcade Link e2e, `tools` group (real GUI, real Lens for busy/update/quit) | passing (74/74 ecosystem checks) |

## Limits

- Windows and macOS are built and tested in CI but have not been run on a
  real desktop; Linux Wayland has not been run.
- Checksums over HTTPS protect integrity, not authorship: minisign
  verification and release signing are future work.
- No release of Tools or of the apps' `arcade/link` work has been published
  yet; the manager installs whatever the app repositories publish.
