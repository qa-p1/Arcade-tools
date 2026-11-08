# Arcade Tools v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

## 2026-11-08

- D5 download ladder done: `src/delta.rs` (local chunk index from retained packages + installed file, bounded pieces and
  Range requests), `Source::download_from` (delta → whole file resumed from `<sha256>.part`, 3 attempts, 30 s idle),
  saved `<sha256>.manifest.json` beside retained packages and kept by pruning. TL-11's resume test is now a normal test;
  new hub test checks an update fetches < ¼ of the file and that a Range-ignoring host falls back to a full download.
- Next: catalog `bad` flag + rollback notice, then TL-9.

## 2026-11-07 — paused for the usage-limit reset

Done on the branch (all pushed, core suite green under `tools/e2e.py run`):
- Lock (step 1), manager split (D4), fixes TL-8, TL-4, CLI-2, TL-6, TL-2, TL-3, TL-10, RD-TOOLS-1, TL-7.
- Link v1.0.0 dependency and `Channel::Test` (0fc0b47).
- D5 discovery from the `/releases?per_page=30` listing per channel (b34b615).
- D5 signed manifests (2580fb2): `src/trust.rs`, `resources/update-keys.pub` (empty until B1), test key secret in
  `tests/support` (`TEST_SIGNING_KEY`), FakeRelease signs every release; refusal test in `hub_lifecycle.rs`.
- Fake server honours single `Range: bytes=a-b` / `a-` requests (206 + Content-Range), for the next step.

Next, in order:
1. Download ladder (D5) in `Source::download`: delta from local chunks (installed executable + retained packages in
   `installation::installers`, saved `<sha256>.manifest.json` next to each retained package), Fetch pieces split at
   chunk boundaries to ≤ 4 MiB and coalesced across gaps < 256 KiB into ≤ 8 MiB Range requests, any failure → full
   download; full download resumes from `downloads/<sha256>.part` (`Range: bytes=N-`, 200 restarts, 3 attempts);
   `DOWNLOAD_TIMEOUT` 30 s idle. Then rewrite `an_interrupted_download_resumes_where_it_stopped` (one call, retry
   inside; trickle server must honour Range) and drop its `should_panic`.
2. Catalog `bad` flag + rollback notice; TL-9 (`installedChannel`, badge, backup before Nightly→Stable).
3. TL-1, TL-5, RD-TOOLS-2/3/6, TL-12; T3 after Link 1.0.1. Then PLAN steps 4–8 and rc releases.


Branch `claude/tender-feynman-3vt7dg` from main `60f92ea` (Tools 0.4.6).

- Step 0 done: spec and audit copied into `docs/v1` (D2); signing blocker recorded (D1).
- Next: PLAN.md loop step 1, the behaviour lock (CLI, `--status`, Link messages, data formats, every audit finding as
  an expected-to-fail regression test, fleet rules).
