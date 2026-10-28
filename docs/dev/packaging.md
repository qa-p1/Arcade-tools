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

## Release

1. Merge to main after CI is green on Linux, Windows and macOS.
2. Actions → "Tag a release" (`tag.yml`) with `v<version>` and the main commit. Tags are never
   pushed by hand.
3. Actions → "Release" (`release.yml`) on the tag with channel `stable`. It refreshes the docs
   snapshot from the apps' latest stable tags, checks its links, builds the three installers with
   LTO, and publishes them with `arcade-release.json` and `SHA256SUMS.txt`.

Release notes start from [release-notes-template.md](release-notes-template.md).
Every push to main also uploads the Linux installer as `tools-installer-ubuntu-<sha>`
(14 days) for Link's nightly ecosystem check.
