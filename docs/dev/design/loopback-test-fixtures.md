# Loopback Test Fixtures

Status: Implemented
Roadmap: `docs/dev/roadmap.md#testing`

## Summary

`russh-extra-test-support` provides local loopback SSH fixtures built directly
on official `russh` server APIs. Runtime integration tests use these fixtures
instead of external SSH hosts.

## Motivation

The client, server, channel, SFTP, and forwarding APIs need real SSH protocol
coverage. Tests should still be deterministic, local, and independent of user
SSH agents, local SSH config, private keys, or public network access.

## Accepted Decisions

- Public API shape: tests start a `LoopbackServer` from a
  `LoopbackServerConfig`, read its loopback endpoint, run client operations,
  and let drop shut the server down.
- Error policy: fixture startup returns local I/O or `russh` errors through the
  test-support API. Assertions for library behavior still use `russh-extra`
  typed errors.
- Cancellation and shutdown policy: dropping `LoopbackServer` requests server
  shutdown and aborts the background task if it is still running.
- Feature flags: fixtures are test-support-only and may depend directly on
  official `russh` with the workspace crypto backend used by tests.
- Escape hatches to `russh`: fixture internals may expose server public key
  material and configured behavior needed by tests.

## User-facing API

Integration tests create fixtures like this:

```rust
let server = russh_extra_test_support::LoopbackServer::start(
    russh_extra_test_support::LoopbackServerConfig::new()
        .password("demo", "demo")
        .command("whoami", russh_extra_test_support::CommandResponse::stdout("demo\n")),
)
.await?;

let client = russh_extra::Client::builder()
    .endpoint(server.endpoint())
    .username("demo")
    .password("demo")
    .accept_any_host_key()
    .build();
```

## Behavior

The first fixture slice supports:

- Binding an ephemeral loopback TCP address.
- Generating an ephemeral Ed25519 host key at runtime.
- Accepting one configured username/password pair.
- Rejecting all other credentials.
- Accepting session channels.
- Handling `exec` requests with configured stdout, stderr, exit status, missing
  status, request rejection, or disconnect behavior.

The fixture must not write keys to disk and must not read user SSH
configuration.

## Security

Host keys are generated in memory per fixture. Passwords are test data and must
not be logged by fixture debug output. Examples use loopback addresses and
`accept_any_host_key()` only for local tests.

## Mapping to `russh`

Fixtures use `russh::server::Config`, `russh::server::Server`,
`russh::server::Handler`, generated `russh::keys::PrivateKey`, session
channels, `exec_request`, `data`, `extended_data`, `exit_status_request`,
`eof`, and `close`.

No external SSH server, SFTP server, or protocol helper crate is involved.

## Feature Flags and Compatibility

`russh-extra-test-support` is not published and is not part of the user-facing
runtime API. It may make breaking changes whenever tests need clearer
behavior.

## Edge cases

- Fixture startup can fail if loopback bind fails.
- Tests need the actual bound port after binding to port `0`.
- A command can intentionally omit exit status.
- A command can write stderr without stdout.
- A command can reject the `exec` request.
- A command can disconnect the session mid-operation.

## Testing Plan

- Unit tests for fixture configuration builders.
- Integration tests that start and drop a loopback server.
- Integration tests that connect with official `russh::client`, authenticate,
  and execute a configured command.
- Client runtime tests will use the fixture for connect, auth, host-key,
  command, output limit, and disconnect behavior.
- Feature-gating checks must continue to pass.

## Alternatives considered

Use the user's local `sshd`. This would make tests depend on host
configuration, credentials, ports, and network policy.

Use another SSH server crate. This violates the project constraint that SSH
behavior is built directly on official `russh` APIs.

## Open questions

- Deferrable: public-key auth fixture helpers.
- Deferrable: SFTP subsystem fixture helpers.
- Deferrable: forwarding fixture helpers.

## Out of scope

This design does not define the production server API. It only defines local
test support for integration tests.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Open questions are either resolved or marked deferrable.
