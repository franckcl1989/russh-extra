# Changelog

All notable changes to `russh-extra` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once a stable release policy is declared. During the pre-1.0 phase, breaking
changes may occur without a new major version.

## [0.1.0] - 2026-05-09

### Changed

- **Breaking**: slimmed default features from `["client", "server", "shell",
  "tunnel", "sftp", "known-hosts", "aws-lc-rs", "flate2", "rsa"]` to
  `["client", "known-hosts", "aws-lc-rs"]`. Users needing `server`, `shell`,
  `tunnel`, `sftp`, `flate2`, or `rsa` must now enable them explicitly.
- **Breaking**: renamed `HostKeyPolicy::AcceptAny` to
  `HostKeyPolicy::InsecureAcceptAny` to make the insecure policy explicit in
  the type name.
- `sftp` feature now included in `full` feature set (client and server SFTP
  runtimes are implemented and tested).
- `async-trait` is now an optional dependency enabled by the `sftp` feature
  instead of being pulled into default client-only builds.
- License changed from MIT-only to MIT OR Apache-2.0 (added `LICENSE-APACHE`
  file, updated `Cargo.toml` workspace metadata).
- `SftpClientRuntime` no longer stores a redundant `session_id` field.

### Fixed

- Fixed `SftpFile::close()` and `SftpDir::close()` firing a duplicate close on
  drop after explicit close was called. Added `closed` flag to prevent the
  best-effort drop-based close when `close()` was already called.
- Fixed `encode_init()` to include the extension count field (previously only
  sent the version, causing the server-side `SftpServerRuntime` to reject the
  init packet as truncated).
- Removed `unreachable!()` panic in SFTP `expect_handle`; replaced with
  proper error reporting via `status_code_name`.
- Removed all `#[allow(dead_code)]` annotations in `sftp/packet.rs` (now
  all SFTP constants and functions are used by client or server).
- Timeout parameter names corrected (`_timeouts`) in tunnel forwarding functions.
- Fixed `Session::auth_banner()` so it returns the authentication banner
  captured by the active `russh` client handler during authentication.

### Removed

- `russh-extra-macros` crate removed from workspace (no runtime to ship).
- `SftpServer` reserved marker type removed; replaced by `SftpServerHandler` trait.

### Added

- Added `agent` feature flag (`agent = ["client"]`) for SSH agent
  authentication through `$SSH_AUTH_SOCK` on Unix platforms.
- Client connect API: `Client::builder()`, `ClientBuilder`, `Client::connect()`.
- Password authentication with configurable credential order.
- Host-key policy: `Strict` (default), `InsecureAcceptAny` (explicit opt-out),
  `PinnedSha256`.
- Buffered `Session::command()` with bounded stdout/stderr capture and typed
  `CommandOutput`.
- `RemoteCommand` builder with per-command output limits and stdin support.
- `RusshHandleGuard` for serialized access to the underlying `russh` client
  handle.
- Server API: `Server::builder()`, listener startup, host key loading,
  password auth callback, exact command routing, and graceful shutdown.
- `ServerHandler` trait for stateful server applications.
- `ServerHostKey` with debug redaction, passphrase support, and Unix
  permission checks.
- `AuthContext`, `AuthDecision`, `ExecContext`, `ExecCommand`, `ExecResponse`
  server types.
- `ServerHandle` for cloneable, idempotent shutdown requests.
- Loopback test fixtures (`russh-extra-test-support`) with ephemeral TCP
  addresses, in-memory host keys, configurable auth, authorized keys, and
  command responses.
- Integration tests for client connect, host-key rejection, auth rejection,
  timeout, command output, output limits, exec failure, concurrent clients,
  multiple channels, shutdown, and disconnect.
- Server unit tests for builder validation, host key redaction, auth
  decisions, and UTF-8 command handling.
- CI on Linux, macOS, and Windows with format, clippy, test, MSRV, and
  feature-gating checks.
- Project documentation: charter, constraints, development plan, roadmap,
  security policy, release policy, testing strategy, design index, and AI
  workflow.
- Design documents: error taxonomy, client session API, loopback test
  fixtures, server API, public key+agent authentication, known hosts,
  channels/shells, native SFTP, and forwarding/tunnels.
- Feature flags: `client`, `server`, `shell`, `sftp`, `tunnel`, `agent`,
  `known-hosts`, `aws-lc-rs`, `ring`, `flate2`, `rsa`, `serde`, `full`.
- Examples: `client_exec`, `client_exec_password`, `client_known_hosts`,
  `server_exec`.
- `full` feature enabling all stable features.
- Known-hosts file parser (`KnownHosts`) with plain hostname and `[host]:port`
  format support. In-memory store with `check()`, `add_entry()`, `save()`,
  and `load()` methods. Integration with `ClientBuilder::known_hosts()` and
  `ClientBuilder::known_hosts_accept_new()`. `@revoked` marker support.
  Hashed hostname entries parsed as warnings (deferred).
- Public key authentication: `Identity::load_openssh_file()`,
  `Identity::load_openssh_pem()`, client-side key file/private key auth,
  server-side `ServerBuilder::public_key_auth()` and
  `ServerHandler::auth_publickey()`.
- Interactive shell API: `ShellBuilder` with PTY configuration, environment
  variables, `Shell::open()` returning a streaming `ShellHandle` for async
  I/O, resize, signal, and exit-status observation.
- SSH subsystem API: `SubsystemBuilder`, `Session::subsystem()`,
  `Subsystem::open()` returning a `SubsystemHandle`.
- Server-side shell/PTY/subsystem handler hooks: `ServerBuilder::shell_handler()`,
  `ServerBuilder::pty_handler()`, `ServerBuilder::subsystem_handler()`,
  `ServerBuilder::env_handler()`, `ServerBuilder::window_change_handler()`.
- `ServerHandler` trait methods: `shell()`, `pty()`, `subsystem()`,
  `env()`, `window_change()` with default reject/accept implementations.
- Server context types: `ShellContext`, `PtyContext`, `PtyParams`,
  `SubsystemContext`, `EnvRequest`, `WindowChange`.
- Terminal mode to `russh::Pty` conversion for PTY requests.
- Integration tests for shell open, PTY allocation, window resize, and
  subsystem open.
- Unit tests for terminal mode conversion, PTY builder, shell/subsystem
  builder fields, and debug redaction.
- Port forwarding (tunnel) APIs: `TunnelBuilder` from `Session::tunnel()`,
  `Tunnel::start()` with local and remote TCP forwarding, `Tunnel::close()`,
  `Tunnel::abort()`, `bound_addr()`, and `TunnelStream` for bidirectional
  channel I/O.
- Direct TCP channels: `DirectTcpBuilder` from `Session::direct_tcp()`,
  `DirectTcpBuilder::open()` returning a `TunnelStream`.
- Server-side forwarding authorization hooks: `tcpip_forward_handler()`,
  `cancel_tcpip_forward_handler()`, `direct_tcpip_handler()`,
  `forwarded_tcpip_handler()` on `ServerBuilder`.
- `ServerHandler` trait forwarding methods: `tcpip_forward()`,
  `cancel_tcpip_forward()`, `channel_open_direct_tcpip()`,
  `channel_open_forwarded_tcpip()` with default deny implementations.
- Server forwarding context types: `TcpipForwardContext`,
  `DirectTcpipContext`, `ForwardedTcpipContext`.
- Client-side remote forwarding registry and `server_channel_open_forwarded_tcpip`
  handler callback for bridging forwarded connections.
- Interactive shell example (`client_shell`), subsystem example
  (`client_subsystem`), tracing instrumentation example (`tracing`), and
  public-key authentication server example (`server_public_key`).
- Loopback test server support for shell, PTY, env, subsystem, tcpip-forward,
  and direct-tcpip channel requests (`LoopbackServerConfig::accept_shell()`,
  `accept_pty()`, `accept_subsystem()`, `accept_direct_tcpip()`,
  `accept_tcpip_forward()`).
- Connection lifecycle hooks on `ServerBuilder` and `ServerHandler`:
  `on_connect()`, `on_disconnect()`, `on_auth_success()` callbacks.
- Streaming exec API: `ServerBuilder::streaming_exec()`,
  `ServerHandler::streaming_exec()`, and `StreamingExecContext` with async
  stdin/stdout/stderr I/O and exit-status signalling.
  `StreamingExecCmd` for internal command transport.
- Integration tests for streaming exec: stdout streaming, stderr streaming,
  exit status, and buffered+streaming coexistence on the same server.
- `server_streaming_exec` example with echo, progress, and late-failure
  streaming commands.
- Loopback test server streaming support: `StreamingCommandConfig`,
  `StreamingStep`, and `LoopbackServerConfig::streaming_command()` for
  step-based streaming with delays.
- Client integration tests: buffered `Session::command()` capturing
  streaming server output, including exit status propagation.
- Streaming exec stdin forwarding: `StreamingExecContext::read_stdin()`
  receives client stdin via `russh::server::Handle`-based pump loop,
  enabling concurrent I/O without blocking the `russh` handler.
  `channel_eof` drops the stdin sender to signal EOF.
- Integration test: client sends stdin to streaming exec handler,
  handler echoes back as stdout.
- Keyboard-interactive authentication: `ServerBuilder::keyboard_interactive_auth()`,
  `KeyboardInteractivePrompt`, `KeyboardInteractivePromptItem`,
  `KeyboardInteractiveResponse` (Accept/Reject/FurtherAction),
  `KeyboardInteractiveContext` (session, submethods, responses),
  and `ServerHandler::auth_keyboard_interactive()` trait method.
  Supports multi-round challenge-response with `Auth::Partial` integration.
- Streaming exec error behavior tests: handler error → exit 1, panic → exit 1,
  explicit exit overrides fallback, success → exit 0.
- Keyboard-interactive integration tests: single-prompt success, wrong-answer
  rejection, multi-step atomic-counter-based acceptance.
- Environment variable propagation: per-channel env var storage in
  `HighLevelRusshHandler`, injected into `ExecContext.env` and
  `StreamingExecContext.env` with `env()` accessor. Vars set via
  `env_request` are accumulated per channel and cleaned up on `channel_close`.
- Env var propagation integration tests: single var, streaming, empty,
  and multiple vars per channel.
- Client-side keyboard-interactive authentication: `Credential::KeyboardInteractive`,
  `ClientBuilder::keyboard_interactive()`, `ClientKeyboardInteractiveInfo`,
  `ClientKeyboardInteractivePrompt`, `KeyboardInteractiveReply`, and
  `KeyboardInteractiveHandler`. The auth loop handles the full multi-round
  challenge-response protocol with timeout support.
- Client-side keyboard-interactive integration tests: single-prompt success,
  wrong-answer rejection, multi-step atomic-counter-based acceptance.
- StreamLocal (Unix-domain) forwarding: `ForwardSpec::local_streamlocal()` and
  `ForwardSpec::remote_streamlocal()` constructors, `DirectStreamLocalBuilder`,
  `Session::direct_streamlocal()`, client-side remote StreamLocal registry,
  `TunnelBindPoint` enum, and `Tunnel::bound_path()`.
- Server-side StreamLocal forwarding: `StreamLocalForwardContext`,
  `DirectStreamLocalContext` types, callback aliases and builder methods,
  `ServerHandler` trait methods `streamlocal_forward()`,
  `cancel_streamlocal_forward()`, and `channel_open_direct_streamlocal()`.
- `ShellAsyncIo` struct implementing `tokio::io::AsyncRead` and
  `AsyncWrite` via `ShellHandle::into_async_io()`. Internally spawned
  channel bridge task routes SSH channel messages to a read stream
  and write commands to the channel.
- SFTP batch readdir: `SftpDir` buffers all entries from `SSH_FXP_NAME`
  responses and drains them one-at-a-time, reducing round-trips.
- `SftpMetadata::new(size, uid, gid, permissions, accessed, modified)`
  full public constructor and `with_accessed()`/`with_modified()` builders.
- `Tunnel::bound_addr()` now returns `Option<SocketAddr>` (changed from
  `SocketAddr`) to support `TunnelBindPoint::StreamLocal` paths.
- X11 forwarding: `ShellBuilder::x11()` and `x11_with_cookie()` for
  requesting X11 forwarding on shell/subsystem sessions. Client-side
  X11 channel handling via `ClientBuilder::x11_display()`. Server-side
  `X11RequestContext`, `X11ChannelContext`, `x11_request_handler()`,
  `x11_channel_handler()` on `ServerBuilder`, and `ServerHandler`
  trait methods `x11_request()` + `channel_open_x11()`.
- Agent forwarding tunnel: `ShellBuilder::agent_forward()` to request
  `auth-agent-req@openssh.com` on a session. Client bridges incoming
  agent channels to `$SSH_AUTH_SOCK`. Server-side `AgentRequestContext`,
  `agent_request_handler()` on `ServerBuilder`, and `ServerHandler`
  trait method `agent_request()`.
- OpenSSH certificate authentication: `CertificateCredential` type with
  `from_openssh_files(key_path, cert_path)` and
  `from_openssh_data(private_key, certificate)`.
  `ClientBuilder::certificate()`. Server-side `auth_openssh_certificate()`
  on `ServerBuilder`, `ServerHandler` trait method.
- Authentication banner: `Session::auth_banner()` returns the server
  banner text if one was sent during authentication. Server-side
  `ServerBuilder::banner()`.

[0.1.0]: https://github.com/franckcl1989/russh-extra/releases/tag/v0.1.0
