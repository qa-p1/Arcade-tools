# Arcade Tools v1 decisions

Decisions the plan left open, made by the v1 session. Each one cites the part of `dev/v1/README.md` it follows.
Newest last.

## D1 — Stable promotion waits for the signing secret (blocker B1)

The `ARCADE_UPDATE_SIGNING_KEY` Actions secret does not exist and this session cannot create it (Link
`docs/v1/DECISIONS.md` D1). Tools still gets the full verifier: it checks `manifest.json.sig` against the built-in keys,
and the tests sign with throwaway keys made inside each test run. rc releases on the hidden `test` channel need no
signature to be built and published, so all rc work and every rc exit gate continue. `promote.yml` refuses to run
without the secret, so `1.0.0` is promoted only after the owner runs step 1 of catalog `dev/v1/LOCAL.md` and the
public key is committed here as the current key. README: "never publish unsigned stable".

## D2 — Spec and audit copied at catalog `2f2e19b`

`docs/v1/SPEC.md` is `specs/TOOLS.md`; `docs/v1/audit/` holds `AUDIT/tools.md` with its evidence folder,
`FINDINGS.md`, `memory.md` (with the Tools measurement files in `memory-tools/`) and `real-desktop.md`, all from
catalog commit `2f2e19b`. Line references in the spec are for Tools 0.4.6 (`60f92ea`).

## D3 — Known-bug tests, and TL-11 re-measured

A behaviour-lock test for a known bug is `#[should_panic(expected = "KNOWN BUG <id>")]` and its failing assertion
names the finding. It passes while the bug is there; once the fix lands the test stops panicking and fails, so the
marker has to be removed with the fix (PLAN.md step 1: "marked expected-to-fail until step 3 fixes them").

TL-11 said every download is capped at 120 s in total. `tests/lock_downloads.rs` shows otherwise for the blocking
client Tools uses: a 3.4 s body with 200 ms gaps completes under a 1 s timeout, and a 1.5 s gap fails it. reqwest's
blocking `Response` applies the timeout to each read, so it already acts as an idle timeout once the body starts. The
other half of TL-11 holds: there is no Range request, so an interrupted download restarts at byte 0
(`an_interrupted_download_resumes_where_it_stopped`, known bug). Step 3 still sets an explicit 30 s read-idle
timeout as the spec asks, and adds resume.
