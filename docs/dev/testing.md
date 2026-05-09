# Testing Strategy

`russh-extra` tests should prove public SSH behavior without relying on
external infrastructure. Networking tests use local loopback fixtures built on
official `russh` APIs.

## Test Layers

### Core Unit Tests

Use unit tests in `russh-extra-core` for feature-neutral data behavior:

- Endpoint parsing and validation.
- Credential and secret redaction.
- Error classification helpers.
- Forwarding specification validation.
- Command exit helpers.
- SFTP packet encoding and decoding once implemented.

### Public API Smoke Tests

Use the workspace test crate for API shape and builder ergonomics:

- Builders preserve configuration.
- Feature-gated types are exported under the expected features.
- Public constructors and helpers compose in normal Rust code.
- Compile-time examples stay close to the design docs.

### Loopback Integration Tests

Use `russh-extra-test-support` for tests that need real SSH networking:

- Client connect, host-key verification, and authentication.
- Command execution, stdout/stderr ordering, exit status, signal, and missing
  status behavior.
- Shell and PTY lifecycle behavior.
- Server auth and channel routing.
- SFTP subsystem requests and malformed packet behavior.
- Local, remote, direct, and streamlocal forwarding.
- Remote disconnects and cancellation.

Fixtures should bind ephemeral loopback addresses and expose handles for
controlled server behavior. Tests must not require user SSH agents, local SSH
configuration, private keys outside the test fixture, or public network access.

### Feature-Gating Tests

CI and local verification must keep these checks working:

```bash
cargo check -p russh-extra --no-default-features
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
cargo check -p russh-extra --no-default-features --features tunnel,aws-lc-rs
cargo check -p russh-extra --no-default-features --features sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features client,ring
cargo doc --workspace --all-features --no-deps
```

When adding a feature, update CI, `AGENTS.md`, `CONTRIBUTING.md`, and this file
if the expected feature combinations change.

## Error-Path Coverage

Each public API design should list negative tests. Common categories:

- Invalid local configuration.
- TCP connection failure.
- SSH negotiation failure.
- Host-key mismatch or unknown host rejection.
- Authentication rejection.
- Channel open failure.
- Channel request rejection.
- Timeout and cancellation.
- Remote disconnect while operations are in flight.
- Missing command exit status.
- Malformed SFTP response.
- Forwarding bind, connect, accept, and cancellation failures.

## Security Coverage

Tests should assert security-sensitive defaults and redaction:

- Strict host-key checking is the default client policy.
- Secret-bearing types redact `Debug`.
- File transfer helpers document and test overwrite behavior.
- Server fixtures make authentication decisions explicit.
- Tracing fields do not include password, passphrase, private key, or command
  stdin content.

## Definition of Done

A feature is implemented when:

- The relevant design doc is Accepted or Implemented.
- Public API tests cover the user-facing examples.
- Loopback tests cover the main success path.
- Negative tests cover the error categories listed in the design.
- Feature-gate checks pass.
- CI commands from `AGENTS.md` pass or any skipped command is documented.
- Public README and crate-doc examples are either backed by compiled examples,
  included in doc tests, or explicitly marked illustrative with the reason.
