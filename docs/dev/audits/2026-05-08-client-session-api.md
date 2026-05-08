# Audit: Client Session API Design

Date: 2026-05-08
Auditor: AI independent audit pass
Trigger: design Implemented (post-hoc — created after implementation to satisfy ADR 0002)

## Scope

Verify that the client session API design (`docs/dev/design/client-session-api.md`)
is consistent with the project charter, features honest claims about implemented
vs deferred behavior, and satisfies its acceptance checklist.

## Input Files

- `docs/dev/design/client-session-api.md` (design, status: Implemented)
- `crates/russh-extra/src/client.rs` (implementation)
- `crates/russh-extra/src/lib.rs` (re-exports)
- `docs/dev/roadmap.md` (client API status)
- `docs/dev/design/README.md` (design index)
- `AGENTS.md` sections 9, 11, 12 (client, command, auth requirements)
- `README.md` (user-facing API examples)
- `CHANGELOG.md` (feature claims)
- `docs/dev/development-plan.md` (phase gates)

## Blocking Issues

None found.

## Resolved Issues

None.

## Remaining Risks

- The design declares "Implemented" but several deferrable open questions remain
  (connection pooling, streaming command API, async close-on-drop). These are
  correctly marked as deferrable and do not contradict the "Implemented" status
  for the first runtime slice.
- Raw handle guard semantics (`RusshHandleGuard`) are claimed to serialize
  access. Whether this is correctly implemented in concurrent scenarios requires
  runtime testing (tracked in integration tests).
- Credential redaction from `Debug` is claimed but needs runtime proof tests.

## Verification Commands

Not executed — post-hoc documentation audit. Should be verified by:

```bash
cargo test --workspace --all-features
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
```

## Conclusion

Pass with notes. The design doc accurately describes the first runtime slice
(builder-driven connect, buffered command execution, host-key policy, credential
ordering, raw handle guard). Deferred items are clearly marked. The design's
acceptance checklist is fully checked and consistent with the roadmap.
