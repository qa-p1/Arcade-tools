# Packaging and releases

`scripts/package` builds this OS's installer exactly as CI does:

1. `scripts/prepare-overlay.py` builds the overlay into `src-tauri/binaries/` (the Tauri sidecar).
2. `npm run tauri -- build --config src-tauri/tauri.bundle.conf.json` bundles an AppImage
   (Linux), an NSIS setup (Windows, hooks in `packaging/windows/`) or a universal dmg (macOS),
   with the docs snapshot and catalog sheets from `src-tauri/resources/`.
3. `scripts/verify-package.py` installs it into a disposable profile and checks the receipt,
   `--version` and uninstall.

## Version

The version lives in `[workspace.package]` in `Cargo.toml`. `package.json`,
`package-lock.json` and `src-tauri/tauri.conf.json` repeat it;
`python3 scripts/check-versions.py --set X.Y.Z` writes all of them, and the check fails on drift.
After a bump, retake the guide's screenshots, whose sidebar shows the version:
`npm run build && SCREENS_DOCS=1 node scripts/ui-checks/harness.mjs dist scripts/ui-checks/screens.mjs`.

## Release

Built once, promoted byte for byte (catalog `dev/v1/PLAN.md` § Release):

1. Set the version with `python3 scripts/check-versions.py --set X.Y.Z`, retake the guide's screenshots (above),
   and merge to main after CI is green on Linux, Windows, macOS and the desktop jobs.
2. Actions → "Tag a release" (`tag.yml`) with `vX.Y.Z-rc.N` and the main commit. It checks the commit is on
   main and carries version X.Y.Z, pushes the tag (tags are never pushed by hand) and starts "Release" on it.
3. "Release" (`release.yml`) builds the three installers with LTO and publishes a hidden prerelease on the test
   channel with `arcade-release.json`, an unsigned manifest and `SHA256SUMS.txt`. Tools follows it only with
   `ARCADE_TOOLS_CHANNEL_OVERRIDE=test`.
4. Run every exit gate against the published rc files (upgrade from the last stable release with
   `scripts/verify-upgrade.py`, the rc.N → rc.N+1 self-update, N-1, desktop and performance). Anything that fails
   is fixed and cut as rc.N+1; superseded rc's stay published.
5. Actions → "Promote" (`promote.yml`) with the verified rc tag republishes the same bytes as `vX.Y.Z` on the
   stable channel with a signed manifest. It fails, publishing nothing, while `ARCADE_UPDATE_SIGNING_KEY` is
   missing.

Release notes start from [release-notes-template.md](release-notes-template.md).
Every push to main also uploads the Linux installer as `tools-installer-ubuntu-<sha>`
(14 days) for Link's nightly ecosystem check.

PR and main CI build and verify all three installers. Release calls the same
source/overlay checks with `skip_packages: true`, then builds its production LTO
installers once in the release bundle matrix. Install/uninstall, automatic
maintenance and resident-process checks run on those exact shipping packages;
publication still requires both the source checks and every package gate.

Rust caches are restored for every run and saved only by main pushes. PR and
tag caches are scoped to their ref, so uploading them cannot warm the next main
build or a different release tag. Release Node dependencies use the npm cache.
