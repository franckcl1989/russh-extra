# Audit: Known Hosts Design

Date: 2026-05-08
Auditor: AI independent audit pass
Trigger: design Implemented (first runtime slice) (post-hoc — created after implementation to satisfy ADR 0002)

## Scope

Verify that the known hosts design (`docs/dev/design/known-hosts.md`) correctly
describes the first runtime slice, maintains security defaults, and honestly
documents deferred behavior.

## Input Files

- `docs/dev/design/known-hosts.md` (design, status: Implemented, first runtime slice)
- `crates/russh-extra/src/known_hosts.rs` (implementation)
- `docs/dev/roadmap.md` (known hosts status)
- `docs/dev/design/README.md` (design index)
- `AGENTS.md` section 13 (host key and known_hosts policy)
- `README.md` (user-facing API examples)
- `docs/dev/decisions/0001-sftp-marker-types-and-full-exclusion.md` (honesty precedent)

## Blocking Issues

None found.

## Resolved Issues

None.

## Remaining Risks

- Hashed hostname matching/writing and wildcard matching are explicitly deferred.
  This is correctly documented in the design, roadmap, and README. Users relying
  on hashed known_hosts entries will get parse warnings rather than silent
  acceptance — this is the safe default.
- `@cert-authority` validation is deferred. Certificate-bearing entries produce
  parse warnings and are skipped. This is the safe behavior for an incomplete
  feature.
- `KnownHosts::save()` does not deduplicate entries. This is a minor UX gap
  and is correctly marked deferrable.
- Permission checks on Unix are claimed but need runtime test coverage (tracked
  in the testing plan).

## Verification Commands

Not executed — post-hoc documentation audit. Should be verified by:

```bash
cargo test -p russh-extra --features known-hosts
cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
cargo check -p russh-extra --no-default-features --features known-hosts,client,aws-lc-rs
```

## Conclusion

Pass. The design accurately documents the first slice (plain hostnames,
`[host]:port`, `@revoked` support, trust-on-first-use in memory, file save/load,
changed-key rejection). Deferred items are honestly listed and do not undermine
the security posture. The `known-hosts` feature is correctly in the default
feature set, consistent with secure-by-default requirements.
