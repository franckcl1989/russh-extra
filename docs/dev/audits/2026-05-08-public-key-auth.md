# Audit: Public Key and Agent Authentication Design

Date: 2026-05-08
Auditor: AI independent audit pass
Trigger: design Implemented (first runtime slice) (post-hoc — created after implementation to satisfy ADR 0002)

## Scope

Verify that the public key and agent authentication design
(`docs/dev/design/public-key-auth.md`) is consistent with the project's security
defaults, credential handling rules, and dependency policy.

## Input Files

- `docs/dev/design/public-key-auth.md` (design, status: Implemented, first runtime slice)
- `crates/russh-extra/src/client.rs` (implementation — credential auth loop)
- `docs/dev/roadmap.md` (client API status)
- `docs/dev/design/README.md` (design index)
- `docs/dev/security.md` (security requirements)
- `AGENTS.md` section 12 (authentication API)
- `README.md` (user-facing API examples)
- `CHANGELOG.md` (feature claims)

## Blocking Issues

None found.

## Resolved Issues

None.

## Remaining Risks

- Agent authentication uses `$SSH_AUTH_SOCK` on Unix only. Non-Unix platforms
  return `AuthenticationErrorKind::Unavailable`. This is documented but should
  be verified through platform-specific CI matrix.
- Passphrase-protected keys: the design claims encrypted key loading is
  supported via `with_passphrase()`. Runtime verification with actual encrypted
  key fixtures is needed.
- Key file permission checks on Unix are claimed but need runtime tests.
- `Identity::Debug` redaction is claimed but needs explicit test coverage to
  confirm no private key bytes leak.
- Deferred open questions (persistent agent connection, `authorized_keys`
  parsing, certificate auth) are correctly marked deferrable.

## Verification Commands

Not executed — post-hoc documentation audit. Should be verified by:

```bash
cargo test --workspace --all-features
cargo check -p russh-extra --no-default-features --features agent,aws-lc-rs
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
```

## Conclusion

Pass. The design correctly separates key-file auth (`client` feature) from
agent auth (`agent` feature, gated behind `client`). Credential ordering,
security defaults (permission checks, debug redaction), and error taxonomy
mapping are specified. No third-party SSH or agent crate is introduced.
