# Audit: Channels and Shells Design

Date: 2026-05-08
Auditor: AI independent audit pass
Trigger: design Implemented (first runtime slice) (post-hoc — created after implementation to satisfy ADR 0002)

## Scope

Verify that the channels and shells design (`docs/dev/design/channels-shells.md`)
describes the implemented first slice accurately, correctly defers split I/O
and `AsyncRead`/`AsyncWrite` traits, and maintains security defaults.

## Input Files

- `docs/dev/design/channels-shells.md` (design, status: Implemented, first runtime slice)
- `crates/russh-extra/src/shell.rs` (implementation)
- `crates/russh-extra/src/client.rs` (shell/subsystem entry points)
- `docs/dev/roadmap.md` (channels and shells status)
- `docs/dev/design/README.md` (design index)
- `AGENTS.md` sections 9, 14, 15 (shell, forwarding, SFTP requirements)
- `README.md` (user-facing API examples)
- `CHANGELOG.md` (feature claims)

## Blocking Issues

None found.

## Resolved Issues

None.

## Remaining Risks

- Split read/write halves and `AsyncRead`/`AsyncWrite` trait implementations are
  explicitly deferred. The monolithic `ShellHandle` is correctly described as
  the first-slice API.
- `Debug` is not derived on `ShellHandle`, consistent with security requirements
  about not logging shell stdin. This should be verified through compilation and
  runtime inspection.
- The design states that `env` request rejection is non-fatal. Runtime behavior
  should be verified against actual server implementations.
- `ShellHandle` does not spawn background tasks — claimed behavior should be
  verified in integration tests.
- The design correctly notes that stdout and stderr are interleaved in arrival
  order in `read()`, which is the pragmatic choice for interactive shells.

## Verification Commands

Not executed — post-hoc documentation audit. Should be verified by:

```bash
cargo test --workspace --all-features
cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
```

## Conclusion

Pass. The design accurately describes the monolithic `ShellHandle` API, PTY
allocation, subsystem channel support, and server-side shell/PTY/subsystem
callbacks. Deferred items (split halves, `AsyncRead`/`AsyncWrite`) are
correctly documented. The `shell` feature is not in default features, keeping
the default build minimal.
