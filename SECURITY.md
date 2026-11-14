# Security policy

## Reporting

Report a vulnerability privately through GitHub's "Report a vulnerability" (Security tab of this repository), not in
a public issue. Expect an answer within a week. Fixes ship as a patch release through the normal rc → promote path
(target: under a day for a serious issue). A release found to be unsafe after it shipped gets the catalog's `bad`
flag: Tools stops offering it and tells installed copies to roll back.

## What Arcade Tools guards, and how

| Surface | Protection | Tested by |
|---|---|---|
| Updates and installs | A release is trusted only through its `manifest.json`, signed with ed25519 (minisign format) and bound to the app, channel and tag; the keys are built into Tools (`resources/update-keys.pub`, current and next), the private key exists only as an Actions secret. The chosen file's size and SHA-256 must match the manifest before anything runs. Release candidates (unsigned) are followed only while `ARCADE_TOOLS_CHANNEL_OVERRIDE=test` is set for the release gates; a stored or restored "test" channel is refused. | `tests/hub_lifecycle.rs` (signatures, tampered and reordered manifests), `tests/channel_override.rs`, `src/trust.rs` |
| Downloads | HTTPS to GitHub hosts only; size limits on every response; delta downloads assemble from local chunks and verify every chunk and the whole file, falling back to a full download on any mismatch; an interrupted download resumes, and the finished file's SHA-256 is checked before use. | `tests/hub_lifecycle.rs` (changed chunks, range fallback), `tests/lock_downloads.rs`, `tests/lifecycle.rs` |
| Running installers | Only the verified bytes run, from a private copy, with `--no-launch`; a failed install restores the previous version, receipt and launcher entries. | `tests/lifecycle.rs`, `tests/hub_lifecycle.rs` |
| Backups and restore | Archives are bounded (32 MiB, 128 entries, expansion limited while reading), refuse traversal, symlinks and duplicate names, and a restore applies only the file that was reviewed (hash-checked). App settings exports are screened: secrets, keys, identities and app-private data (Clipboard history, Find's index) are never written into a backup. | `src/archive.rs`, `src/settings_export.rs`, `tests/maintenance.rs` |
| Diagnostics | Built locally, never uploaded; home paths, tokens and secret-looking strings are redacted before the file is written. The user attaches it to an issue themselves. | `src/diagnostics_archive.rs`, `tests/maintenance.rs` |
| Window | The WebView loads only Tools' own files under a strict Content-Security-Policy (no remote scripts or connections); it has only Tauri's core permissions, and all network, file and process work happens in Rust. | `src-tauri/tauri.conf.json` (`csp`), `src-tauri/capabilities/default.json` |
| Link IPC | Arcade Link's sockets (0600, peer UID check, per-endpoint token); Tools rejects malformed requests and app ids. | Link's own tests, `tests/link_contract.rs` |

## Dependencies

`cargo deny check` (advisories, licences, bans, sources) runs in CI; Dependabot watches Cargo, npm and GitHub
Actions. Accepted advisories are listed with a reason in `deny.toml`: today only "unmaintained" notices (no known
vulnerability) for crates that come in through Tauri, gtk-rs, zbus 5 and cosmic-text with no maintained
replacement yet. Third-party licences are in
`THIRD_PARTY_NOTICES.md`, also shown in Settings → About.
