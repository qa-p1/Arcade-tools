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
