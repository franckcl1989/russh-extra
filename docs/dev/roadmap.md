# russh-extra Roadmap

This roadmap tracks accepted project direction and current implementation state.
An item being listed means the project wants the capability; it does not mean
the work is ready to implement.

The project builds high-level APIs directly on the official `russh` crate.
Adding another SSH, SFTP, shell, tunnel, or protocol abstraction crate is out of
scope. Missing protocol primitives should become upstream `russh` issues or
local layers over public `russh` APIs.

## Status Legend

- Draft: direction is accepted, but design questions remain.
- Accepted: design is ready for implementation.
- Implementing: code is in progress and should stay aligned with the design.
- Implemented: code, tests, and docs match the design.

## Current Focus

1. Prepare the 0.1.0 release candidate by keeping public claims aligned with
   implemented behavior.
2. Harden the implemented client, server, shell, known-hosts, SFTP, and
   forwarding runtime slices with additional edge-case tests, examples, and
   concurrency stress tests.
3. Keep lower-level `russh` escape-hatch gaps documented until first-class
   high-level wrappers exist.

## Foundation

Status: Implementing
Docs:
[Project Charter](project-charter.md),
[Development Constraints](constraints.md),
[AI Development Workflow](ai-workflow.md),
[Testing Strategy](testing.md),
[Development Plan](development-plan.md),
[Security Policy](security.md),
[Release Policy](release.md)
Design: [Error Taxonomy](design/error-taxonomy.md)

- Implemented: workspace skeleton, AI-agent instructions, design docs, CI,
  feature checks, and basic test support.
- Implemented: typed error taxonomy for SSH negotiation, auth, channel,
  command, SFTP, forwarding, cancellation, and remote disconnect failures.
- Draft: tracing conventions for connections, sessions, channels, and transfer
  IDs.

Done when:

- Public API work cannot land without an Accepted design or a documented
  small-change exception.
- CI covers formatting, clippy, tests, MSRV, and feature-gating combinations.
- Local networking tests do not depend on external SSH hosts.
- The current phase gate in `docs/dev/development-plan.md` is accurate.

## Client API

Status: Implementing
Design: [Client Session API](design/client-session-api.md)

- Implemented: client builder with endpoint, username, credentials, host-key
  policy, timeout, keepalive, and crypto backend configuration.
- Implemented: host-key policy types, pinned SHA256 fingerprint validation, and
  command output limit configuration.
- Implemented: `Client::connect()` runtime slice with strict host-key default,
  explicit accept-any opt-out, pinned SHA256 host keys, ordered credentials,
  typed errors, and raw `russh` handle guard.
- Implemented: buffered `Session::command()` with bounded stdout and stderr.
- Implemented: public key authentication from in-memory and file-loaded
  OpenSSH keys.
- Implemented: SSH agent authentication through `$SSH_AUTH_SOCK` on Unix
  platforms. Non-Unix platforms return `AuthenticationErrorKind::Unavailable`.
- Implemented: known-hosts store integration and trust-on-first-use in memory.
- Implemented: shell and subsystem entry points from connected sessions.
- Implementing: TCP forwarding runtime and additional forwarding tests.
- Draft: native SFTP packet/runtime layer and split streaming command handles.

Next implementation work:

- Add more known-hosts save/load and revoked-key tests.
- Add more forwarding lifecycle and remote forwarding tests.

## Server API

Status: Implementing
Design: [Server API](design/server-api.md)

- Implemented: first runtime slice with bind address, required host keys,
  password authentication, public key authentication, exact buffered exec
  routing, request rejection, session limits, and explicit shutdown.
- Implemented: connection lifecycle hooks (`on_connect`, `on_disconnect`,
  `on_auth_success`) on `ServerBuilder` and `ServerHandler`.
- Implemented: shell, PTY, subsystem, env, and window-change handler hooks.
- Implemented: forwarding authorization hooks (tcpip-forward, direct-tcpip,
  forwarded-tcpip, cancel-tcpip-forward).
- Implemented: local loopback test server helpers for client runtime tests.
- Implemented: streaming exec handlers with async stdin/stdout/stderr I/O
  (`StreamingExecContext`, `ServerBuilder::streaming_exec()`,
  `ServerHandler::streaming_exec()`), including concurrent stdin forwarding
  via `russh::server::Handle` pump loop and `channel_eof` EOF signalling.
- Implemented: `server_streaming_exec` example and loopback test server
  streaming support (`StreamingCommandConfig`, `StreamingStep`,
  `LoopbackServerConfig::streaming_command()`).
- Implemented: stdin forwarding integration test (client stdin → handler
  `read_stdin()` → handler stdout → client stdout).
- Implemented: keyboard-interactive authentication with multi-round
  challenge-response support (builder method, callback type,
  `KeyboardInteractivePrompt`, `KeyboardInteractiveResponse`,
  `KeyboardInteractiveContext`, `ServerHandler` trait method,
  `russh::server::Auth::Partial` integration).
- Implemented: keyboard-interactive streaming exec error tests
  (handler error → exit 1, panic → exit 1, explicit exit overrides fallback,
  success → exit 0).
- Implemented: keyboard-interactive integration tests (single-prompt success,
  wrong answer rejection, multi-step acceptance).
- Implemented: environment variable propagation — env vars set by client
  before exec are stored per-channel and injected into `ExecContext.env`
  and `StreamingExecContext.env` with `env()` accessor.
- Implemented: env var propagation integration tests (single var, streaming,
  empty, and multiple vars per channel).
- Implemented: SFTP server handler registration and runtime dispatch through
  `SftpServerHandler`.
- Draft: channel lifecycle hooks.

## Known Hosts

Status: Implementing
Design: [Known Hosts](design/known-hosts.md)

- Implemented: `KnownHosts::load()`, `KnownHosts::save()`,
  `KnownHosts::check()`, parse warnings, in-memory trust-on-first-use, changed
  key rejection, and `@revoked` rejection.
- Deferred: hashed hostname matching/writing, wildcard matching,
  `@cert-authority` validation, and automatic persistence.

## Channels and Shells

Status: Implemented (first runtime slice)
Design: [Channels and Shells](design/channels-shells.md)

- Implemented: `ShellBuilder`, `Shell::open()`, `ShellHandle` read/write,
  PTY request, env requests, resize, signal, close, and subsystem channel open.
- Implemented: server shell, PTY, subsystem, env, and window-change hooks.
- Deferred: split read/write halves and `AsyncRead`/`AsyncWrite` trait impls.

## Native SFTP

Status: Implemented
Design: [Native SFTP Layer](design/native-sftp.md)

- Implemented: SFTP v3 client with open, read, write, close, stat, lstat,
  opendir, readdir, remove, rename, mkdir, rmdir, realpath, readlink, symlink,
  `read_to_vec`, `write_all`, and Drop auto-close.
- Implemented: packet encoding/decoding, request pipelining via oneshot
  channels, status code mapping, and attribute round-trip.
- Implemented: server-side `SftpServerHandler` trait, packet decoder, response
  encoder, and runtime dispatch for the SFTP v3 request set.
- Implemented: loopback SFTP integration tests (open/read/write/stat/
  opendir+readdir/remove/rename/mkdir+rmdir/symlink+readlink/canonicalize/
  drop-auto-close).
- Implemented: server handler integration tests via an in-memory SFTP handler.
- Deferred: SFTP v4+ protocol extensions and OpenSSH SFTP extensions.

## Forwarding and Tunnels

Status: Implementing
Design: [Forwarding and Tunnels](design/forwarding-tunnels.md)

- Implemented: `TunnelBuilder` from `Session::tunnel()` for local and remote TCP forwarding.
- Implemented: `Tunnel::start()`, `Tunnel::close()`, `Tunnel::abort()`, `Tunnel::bound_addr()`.
- Implemented: `DirectTcpBuilder`, `DirectTcp::open()`, `TunnelStream` for channel I/O.
- Implemented: local forwarding accept loop with `direct-tcpip` channel open and bidirectional copy.
- Implemented: remote forwarding with `tcpip-forward` global request, port allocation, and `forwarded-tcpip` channel handling via `ClientHandler` callback.
- Implemented: server-side forwarding authorization hooks on `ServerBuilder` and `ServerHandler` trait.
- Implemented: `TcpipForwardContext`, `DirectTcpipContext`, `ForwardedTcpipContext` context types.
- Implemented: direct TCP, local forwarding, and remote forwarding loopback integration tests.
- Implemented: StreamLocal API and Unix-domain forwarding paths where supported by `russh`.
- Deferred: dynamic SOCKS-style forwarding.
- Pending hardening: StreamLocal loopback tests and cancellation edge cases.

## Testing

Status: Implementing
Docs: [Testing Strategy](testing.md)
Design: [Loopback Test Fixtures](design/loopback-test-fixtures.md)

- Implemented: first loopback fixture slice with ephemeral local `russh`
  server, generated in-memory host key, password auth, configurable exec
  responses, and protocol-level fixture self-test.
- Implemented: client runtime integration tests for connect, pinned host keys,
  strict host-key rejection, password rejection, connect timeout, buffered
  command output, output limits, exec rejection, and disconnect during command
  execution.
- Implemented: known-hosts accept-new and changed-key integration tests.
- Implemented: shell/PTY and subsystem open integration tests.
- Implemented: direct TCP and local forwarding integration tests.
- Draft: protocol-level SFTP packet tests.
- Draft: API compile tests for builder ergonomics and macro output.
- Implemented: CI on Linux, macOS, and Windows.

## Macros

Status: Deferred

- Deferred: optional typed server routing macros after handler traits are stable.
- Deferred: optional command declaration macros after the command API is stable.
