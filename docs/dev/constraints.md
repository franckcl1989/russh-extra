# Development Constraints

These constraints are mandatory for agents working on `russh-extra`.

## Dependency Policy

`russh-extra` builds SSH behavior directly on official `russh` APIs.

Do not add community SSH, SFTP, SCP, shell, tunnel, or protocol helper crates.
This includes crates that wrap `russh` or implement parallel SSH protocol
behavior outside this repository.

Allowed dependency categories:

- Official `russh` crate APIs for SSH transport, handlers, channels, requests,
  and crypto feature selection.
- General Rust utility crates such as async runtimes, byte buffers, error
  types, serialization, tracing, and test helpers.
- Development-only crates for assertions, fixtures, and compile tests when they
  do not provide SSH protocol behavior.

If `russh` lacks a primitive needed by a high-level API:

1. Document the gap in the relevant design doc.
2. Prefer a small local layer over public `russh` APIs.
3. Track an upstream `russh` change when a public primitive is required.
4. Do not fill the gap with another SSH protocol crate.

## Design Gate

Non-trivial public API work must follow the design flow:

1. Update `docs/dev/roadmap.md`.
2. Write or update a guide-level design doc under `docs/dev/design/`.
3. Keep the design in Draft while blocking questions remain.
4. Mark the design Accepted only after public API, behavior, errors, security,
   feature flags, and tests are specified.
5. Implement against the Accepted design.

Small fixes, docs corrections, and internal cleanup can skip a new design doc
when they do not change public behavior.

## Breaking Changes

This is a new pre-1.0 project. Breaking changes are allowed when they improve
the architecture, API coherence, security posture, or testability of the
full-featured `russh`-based SSH API.

When making a breaking change, update the relevant design doc, roadmap, tests,
and examples in the same work item. Do not preserve an early API shape only for
compatibility during the pre-1.0 phase.

## Public API Shape

High-level APIs should reduce repeated bookkeeping without hiding core SSH
concepts:

- `Client` owns connection configuration and creates connected sessions.
- `Session` opens commands, shells, subsystems, SFTP, and tunnels.
- Channel wrappers expose typed lifecycle behavior and standard async I/O.
- `Server` routes authentication and channel requests to user handlers.
- Feature-specific handles such as `Shell`, `SftpClient`, and `Tunnel` own
  feature behavior.
- Escape hatches expose lower-level `russh` handles where ownership and safety
  are clear.

## Error Policy

Errors must be typed enough for user code to distinguish:

- Invalid local configuration.
- DNS, TCP, SSH negotiation, and transport failures.
- Host-key verification failures.
- Authentication rejection and partial authentication.
- Channel open and channel request failures.
- Remote command exit status and signal results.
- SFTP status responses and malformed SFTP packets.
- Forwarding bind, accept, connect, and remote cancellation failures.
- Cancellation, timeout, and remote disconnect behavior.

Do not collapse all `russh` errors into opaque strings when user code can make
meaningful decisions from the category.

## Security Defaults

Security-sensitive behavior must be explicit in design docs:

- Host-key verification defaults to strict verification.
- Passwords, passphrases, private key material, and command stdin are never
  logged.
- Debug output for credential-bearing types must redact secrets.
- Local file creation and overwrite behavior must be documented for transfer
  APIs.
- Server APIs must make authentication and authorization decisions explicit.

## Testing Constraints

Tests must not depend on external SSH hosts. Real networking tests should use
loopback fixtures from `russh-extra-test-support`.

Public API behavior should be tested at integration level when practical. Unit
tests are appropriate for small parsing, validation, encoding, decoding, and
error-classification logic.

Feature-gating checks must include `--no-default-features` and the feature
combinations listed in `AGENTS.md`.

## Documentation Constraints

Durable decisions belong in repository files:

- Roadmap status in `docs/dev/roadmap.md`.
- Public behavior in `docs/dev/design/`.
- Project-wide constraints in this file.
- Testing strategy in `docs/dev/testing.md`.
- AI workflow and prompts in `docs/dev/ai-workflow.md`.

Do not leave a public API decision only in a chat transcript, issue comment, or
PR description.
