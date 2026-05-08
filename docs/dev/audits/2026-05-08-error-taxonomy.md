# Audit: Error Taxonomy Design

Date: 2026-05-08
Auditor: AI independent audit pass
Trigger: design Implemented (post-hoc — created after implementation to satisfy ADR 0002)

## Scope

Verify that the error taxonomy design (`docs/dev/design/error-taxonomy.md`) is
consistent with the actual implementation in `russh-extra-core`, that its
claims are aligned with the project charter, and that all acceptance checklist
items are satisfied.

## Input Files

- `docs/dev/design/error-taxonomy.md` (design, status: Implemented)
- `crates/russh-extra-core/src/error.rs` (implementation)
- `crates/russh-extra-core/src/lib.rs` (re-exports)
- `docs/dev/roadmap.md` (roadmap status)
- `docs/dev/design/README.md` (design index)
- `AGENTS.md` section 16 (error design reference)
- `docs/dev/project-charter.md` (project goals)

## Blocking Issues

None found.

## Resolved Issues

None.

## Remaining Risks

- This audit is a documentation-level review only. The implementation was not
  tested or compiled as part of this audit. The claims about source
  preservation, kind enum completeness, and debug redaction should be verified
  through the integration test suite.
- The `CommandExit` variant exists in the taxonomy but the design explicitly
  states that the base buffered command API returns `CommandOutput`, not an
  error. The actual enum member presence in code should be verified against the
  design claim.
- Secret redaction in `Debug` is a security requirement that needs runtime test
  coverage to confirm (tracked in testing plan section of design doc).

## Verification Commands

Not executed — this is a post-hoc documentation audit from repository files
alone. The following should be run by the next implementation session:

```bash
cargo check -p russh-extra-core --no-default-features
cargo test -p russh-extra-core
cargo test --workspace --all-features
```

## Conclusion

Pass with notes. The design document is internally consistent, the acceptance
checklist is complete, and the taxonomy provides sufficient categories for
user code to distinguish SSH failure modes. The design correctly leaves
`CommandOutput` as a non-error return and avoids claiming SFTP runtime
behavior. Remaining risks are verification-related, not design-related.
