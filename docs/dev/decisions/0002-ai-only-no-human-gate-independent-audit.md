# ADR 0002: AI-Only Operating Model: No Human Gate, Independent Audit

Status: Accepted

## Context

`russh-extra` operates as an AI-only development project. The project charter
states that agents own planning, design, implementation, tests, review notes,
issues, PR text, commit text, release notes, and maintenance docs. The project
does not require human implementation, human review, or human approval gates
to move forward.

Without human gates, the project needs a mechanism to prevent single-pass
self-approval from degrading quality, introducing silent security regressions,
or allowing undocumented decisions to live only in chat context.

## Decision

1. **No human gate**: No human review, approval, or sign-off is required for
   any change. Goal-setting prompts from humans are input to the workflow, not
   durable governance.
2. **Independent AI audit required**: Before a design is marked Implemented,
   before a release checklist is completed, or before a security-sensitive
   behavior is declared production-ready, an independent AI review pass must
   audit the change from repository files alone.
3. **Audit scope**: The independent audit must check prompt compliance, design
   alignment, security behavior, test coverage, feature-gating, dependency
   policy, and documentation claims.
4. **Durable records**: Audit findings and residual risk must be recorded in
   `docs/dev/audits/` so the next agent can continue without chat context.
5. **Separation**: The audit pass should be a separate AI session or a
   separate prompt from the implementation pass when practical.

## Rationale

- **AI-only integrity**: Without a review mechanism, AI agents could
  accumulate undocumented assumptions, unreviewed security decisions, and
  untested claims that only exist in chat.
- **File-first memory**: The independent audit forces all decisions and risks
  into repository files, making the repository self-contained.
- **Separation of concerns**: The implementer and auditor having different
  perspectives (even if both are AI) catches errors that single-pass review
  would miss.
- **Practicality**: Requiring human review would defeat the AI-only operating
  model. Independent AI audit is the closest equivalent that preserves agent
  autonomy.

## Consequences

- Every `Implemented` design, release checklist, and security-sensitive claim
  requires an audit record in `docs/dev/audits/`.
- The audit template and naming conventions are defined in
  `docs/dev/audits/README.md`.
- Agility is reduced slightly (audit adds a step) in exchange for higher
  confidence in production-readiness claims.
- If no independent AI session is available, the agent must self-audit and
  note that limitation explicitly in the audit record.

## Alternatives Considered

- **Human review gate**: Rejected because it contradicts the AI-only operating
  model and introduces latency that agents cannot control.
- **No audit at all**: Rejected because it would allow undocumented
  single-pass self-approval to accumulate hidden risk.
- **Audit only for security-sensitive changes**: Rejected because design
  alignment and documentation claims can also drift without review.
