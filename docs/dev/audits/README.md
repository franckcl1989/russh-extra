# Independent AI Audit Records

Audit records are the durable output of the independent AI audit process
defined in `docs/dev/project-charter.md` and `docs/dev/ai-workflow.md`.

An independent audit is required before:

- Marking a design `Implemented`.
- Completing a release checklist.
- Declaring a security-sensitive behavior production-ready.

The audit must be performed by a separate AI pass from the implementation
pass. Findings and residual risk must be recorded here so the next agent can
continue without chat context.

## Record Format

Start every audit from `_template.md`. Each record must state:

- Scope of the audit.
- Input files examined.
- Blocking issues found.
- Issues resolved during the audit.
- Remaining risks and where they are tracked.
- Verification commands run.
- Conclusion (pass, pass with notes, or fail with blocking items).

Keep records self-contained. Do not assume the reader was present for the
implementation session.

## Naming Convention

```text
YYYY-MM-DD-<scope>.md
```

Use the date the audit was completed, and a short scope slug (e.g.
`client-connect`, `server-password-auth`, `release-0.1.0`).

## Records

- [2026-05-08-error-taxonomy](./2026-05-08-error-taxonomy.md) — Error taxonomy design audit (pass with notes)
- [2026-05-08-client-session-api](./2026-05-08-client-session-api.md) — Client session API design audit (pass with notes)
- [2026-05-08-loopback-test-fixtures](./2026-05-08-loopback-test-fixtures.md) — Loopback test fixtures design audit (pass)
- [2026-05-08-known-hosts](./2026-05-08-known-hosts.md) — Known hosts design audit (pass)
- [2026-05-08-public-key-auth](./2026-05-08-public-key-auth.md) — Public key and agent authentication design audit (pass)
- [2026-05-08-channels-shells](./2026-05-08-channels-shells.md) — Channels and shells design audit (pass)
- [2026-05-09-release-0.1.0](./2026-05-09-release-0.1.0.md) — 0.1.0 release candidate audit (pass with notes)
- [2026-05-15-release-0.1.1](./2026-05-15-release-0.1.1.md) — 0.1.1 hardening release audit (pass)
