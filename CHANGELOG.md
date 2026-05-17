# Changelog

All notable changes to `russh-extra` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once a stable release policy is declared. During the pre-1.0 phase, breaking
changes may occur without a new major version.

## [0.1.3] - 2026-05-17

### Added

- `CommandExit::Signal` now carries a `core_dumped: bool` field alongside the
  signal name.
- `ChannelKind` now has `X11` and `AuthAgent` variants for X11 forwarding and
  agent forwarding channels. All 8 `ChannelKind` variants now have public
  constructor methods (`session()`, `direct_tcp_ip()`, `x11()`, etc.).
- `TcpEndpoint` and `StreamLocalSpec` now implement `Display` and `FromStr`,
  with IPv6 bracket notation support and tilde expansion.
- `Pty::with_term()` setter for changing the terminal type after construction.
- `StreamLocalSpec` expands tilde in paths automatically through `new()`.

### Fixed

- SFTP server `readdir` no longer hangs on handler that returns entries but
  never sends EOF. The `SftpServerHandler::readdir` trait method now has the
  correct contract: returning an empty `Vec` signals end-of-directory.
  Previously the `InMemorySftpHandler` in integration tests returned all
  entries on every call, causing an infinite loop in the client.
- Previously-ignored `sftp_server_readdir_and_fstat` integration test is now
  active and passing.
- `InMemorySftpHandler::stat()` now returns `SftpErrorKind::NoSuchFile` for
  missing files (previously returned a generic `Unsupported` error), matching
  the `open()` handler's behavior.
- `README.md` code example: `.into_async_io().await?` corrected to
  `.into_async_io()` (the method is synchronous).
- `README.md` code example: `entry.name()` corrected to `entry.filename()`.
- `docs/dev/design/error-taxonomy.md`: `SftpErrorKind` variant names updated
  to match the actual implementation (`RemoteStatus`, `Protocol`, `ChannelIo`,
  `UnexpectedResponse`; added `NoSuchFile` and `PermissionDenied`).
- `docs/dev/design/native-sftp.md`: `symlink()` example argument order fixed
  (`linkpath` first, then `targetpath`).
- `docs/dev/design/forwarding-tunnels.md`: removed stale "StreamLocal hardening
  pending" status label.
- `docs/dev/security.md`: deprecated `strict_host_key_checking(false)` replaced
  with `host_key_policy(HostKeyPolicy::InsecureAcceptAny)`.
- `docs/dev/design/client-session-api.md`: stale SFTP `Error::Unsupported` claim
  removed; `CommandExit::Signal` signature updated to 2-arg form.
- `AGENTS.md` §15 SFTP known limitations: `readdir` batching claim corrected
  (batching IS implemented via `readdir_batch()`).
- `AGENTS.md` §11 `CommandOutput` illustrative types corrected (`Vec<u8>` →
  `Bytes`; removed non-existent `stdout_string_lossy`).
- `AGENTS.md` §15 SFTP method names corrected to match actual public API
  (`set_stat`, `create_dir`, `close_file`, `canonicalize`, etc.).
- `AGENTS.md` §13 `HostKeyPolicy` variant list corrected to actual enum
  (`Strict`, `InsecureAcceptAny`, `PinnedSha256`; `AcceptNew` removed).
- `AGENTS.md` §22 example file list updated to match actual 14 examples.
- Fixed documentation drift: CHANGELOG section ordering, stale version
  references, status labels, broken intra-doc links, and missing audit/dev-plan
  links across 13 files.
- `CONTRIBUTING.md`: added `server,sftp` and `full` feature-gate checks, added
  `cargo check --workspace --all-targets`, added `--all-targets` to clippy.
- `README.md`: added `server,sftp` and `full` feature-gate checks.

### Changed

- `russh` dependency updated from 0.60.2 to 0.60.3.
- `ClientBuilder::strict_host_key_checking()` is now deprecated in favor of
  `host_key_policy()` (matching the already-deprecated
  `ClientConfig::set_strict_host_key_checking()`).
- `ServerEvent`, `StreamingExecCmd`, and `KnownHostStatus` are now marked
  `#[non_exhaustive]` for future extensibility.
- `categories.workspace = true` added to both publishable crate manifests
  for correct crates.io category listing.
- `##` doc comments added to all 16 feature flags across both Cargo.toml files.
- CI `cargo check` and `cargo clippy` now include `--all-targets` (examples
  are compiled in CI for the first time).

### Internal

- Added `SftpServerHandler` test implementations for `setstat`, `fsetstat`,
  `realpath`, `symlink`, and `readlink` in the integration test
  `InMemorySftpHandler`.
- Added 12 integration tests: large data transfers (4), ShellAsyncIo lifecycle
  (2), SFTP handler methods — symlink, readlink, realpath, setstat, fsetstat,
  readdir exhaustion, metadata builders (6).
- Added 21 unit tests for `Pty::with_term`, `ChannelKind`, `CommandExit::Signal`,
  `TcpEndpoint::Display/FromStr`, `StreamLocalSpec::Display/FromStr`.
- `InMemorySftpHandler::readdir` made stateful (cursor-based) to fix the
  previously-ignored `sftp_server_readdir_and_fstat` test.
- `sftp_server_stat_missing_file_returns_error` test strengthened to validate
  `SftpErrorKind::NoSuchFile`.
- Misleading test names corrected: `*debug_redacts_secrets` → `*_invalid_*`,
  `*shell_async_io_*` → appropriate names matching tested behavior.
- `certificate_credential_rejects_invalid_key_format` test rebuilt (old test
  had a mismatched name that implied debug testing it didn't perform).
- `SftpOpenMode` constants now have `///` doc comments.
- `.to_owned()` replaced with `.into()` in `shell.rs:90` and
  `client.rs:1605-1608`.
- Fixed documentation drift across AGENTS.md, roadmap.md, development-plan.md,
  CHANGELOG.md, README.md, and 9 design/docs files to reflect actual 0.1.2
  and 0.1.3 implementation state.
- Added 0.1.2 audit note (`docs/dev/audits/2026-05-17-release-0.1.2.md`).
- Added 0.1.3 audit note (`docs/dev/audits/2026-05-17-release-0.1.3.md`).
- Updated 0.1.2 development plan completion tracker.

## [0.1.2] - 2026-05-17

### Fixed

- **Security**: `X11Params` no longer exposes the X11 authentication cookie in
  `Debug` output. The cookie field is now redacted as `<redacted>`.
- **Security**: `KeyboardInteractiveContext` no longer exposes user responses
  (potentially passwords or 2FA codes) in `Debug` output. Responses are now
  redacted.
- **Security**: `X11RequestContext` no longer exposes the X11 authentication
  cookie in `Debug` output.
- **Data loss**: `ShellAsyncIo` no longer silently drops stderr data from the
  SSH channel. `ChannelMsg::ExtendedData` is now routed to the read stream
  (interleaved with stdout, matching `ShellHandle` behavior).
- **Resource leak**: `ShellAsyncIo` bridge task now exits cleanly when the
  `ShellAsyncIo` handle is dropped. Previously the bridge task held a
  self-referencing sender and could never shut down.
- **Correctness**: `KnownHosts::check()` no longer short-circuits on the first
  key mismatch. When multiple entries exist for the same host (e.g. different
  key types), all entries are now scanned. A match on any entry returns
  `Match`; `Changed` is only returned when no entry's key matches.
- **Correctness**: `Credential::PartialEq` for `KeyboardInteractive` variants
  now returns `true` (previously always `false`), fixing `Eq` for
  `ClientConfig` values that contain keyboard-interactive credentials.
- `set_strict_host_key_checking` is now deprecated in favor of the more
  expressive `set_host_key_policy`. The deprecated method is preserved for
  backward compatibility.
- `Endpoint::Display` now uses bracket notation for IPv6 addresses (e.g.
  `[::1]:22`) via delegation to `authority()`, fixing IPv6 round-trip parsing.

### Added

- `CommandOutput::check_success(self) -> Result<Self>` returns the output on
  success or an `Error::CommandExit` on non-zero or missing exit status.
- `Identity::key_file()` now expands tilde (`~`) in the provided path.
- `TerminalMode` now includes common SSH terminal modes: `Echo`, `EchoErase`,
  `EchoKill`, `EchoNl`, `CanonicalInput`, `SigCheck`, `CrToNlInput`,
  `NlToCrInput`, `IgnoreCrInput`, `PostProcessOutput`, `NlToCrNlOutput`,
  `CrToNlOutput`, and `NoCrOnNl`.
- `KnownHostsEntry::parse()` now produces one entry per comma-separated hostname
  in the pattern field (e.g. `host-a,host-b,host-c` creates three entries).

### Changed

- `Pty`, `TcpEndpoint`, `StreamLocalSpec`, and `TerminalMode` are now marked
  `#[non_exhaustive]` for future extensibility. Struct-literal construction of
  `TcpEndpoint` and `StreamLocalSpec` is no longer supported; use the `new()`
  constructors instead.
- `KnownHostsEntry::parse()` now returns `Vec<KnownHostsEntry>` instead of
  `Option<KnownHostsEntry>` to support multi-host patterns.

### Internal

- Added debug redaction tests for `X11Params`, `KeyboardInteractiveContext`,
  `X11RequestContext`, and `ClientConfig`.
- Added `credential_eq`, `endpoint_display_round_trip`, `tilde_expansion`,
  `terminal_mode_mapping`, `check_success`, and known-hosts `check_finds_match`
  unit tests.
- Added SFTP server handler methods (`mkdir`, `rmdir`, `rename`, `readdir`,
  `fstat`) to the integration test `InMemorySftpHandler` with corresponding
   integration tests (6 tests, 5 passing, 1 gated behind `#[ignore]`).

## [0.1.1] - 2026-05-15

### Added

- `SftpErrorKind::NoSuchFile` and `SftpErrorKind::PermissionDenied` variants for
  finer-grained SFTP error classification.
- Server-side SFTP error-to-status-code mapping: handler errors with typed
  `SftpErrorKind` values now produce the corresponding SFTP v3 status codes
  (`SSH_FX_NO_SUCH_FILE`, `SSH_FX_PERMISSION_DENIED`, `SSH_FX_OP_UNSUPPORTED`,
  `SSH_FX_BAD_MESSAGE`, `SSH_FX_FAILURE`).
- Client-side SFTP status-code-to-error-kind mapping: specific SSH_FX codes
  received from the server now produce typed `SftpErrorKind` variants.
- `sftp_error_kind_for_code()` helper in `packet.rs` for shared client/server
  SFTP status-to-kind mapping.
- Unix-only integration tests for StreamLocal tunnel close (socket path cleanup)
  and tunnel abort (no panic).
- Known-hosts edge-case tests: wildcard-looking entries do not match unrelated
  hosts, hashed entries are skipped with warnings, malformed lines mixed with
  valid entries are collected as warnings.
- Known-hosts `set_hash_hostnames()` regression test confirming plain-text
  hostname output (hashed writing not yet implemented).

### Changed

- Documentation governance: `docs/dev/design/README.md`, `docs/dev/roadmap.md`,
  and `docs/dev/development-plan.md` statuses reconciled with the post-0.1.0
  implementation state. All top-level sections now show accurate status labels.
- `AGENTS.md` updated: test count (205 → 212), edition reference (2021 → 2024),
  and version roadmap status.
- Roadmap Foundation, Client, Server, Known Hosts, and Testing sections marked
  as Implemented.
- `InMemorySftpHandler` test handler now uses `SftpErrorKind::NoSuchFile`
  instead of a generic `Unsupported` error when a file is not found.

### Fixed

- Local StreamLocal forwarding now removes the Unix-domain socket file after
  `Tunnel::close()` (previously the socket file was left behind on listener
  shutdown).
- SFTP server runtime maps typed handler errors to stable SFTP v3 status codes
  instead of collapsing all failures to `SSH_FX_FAILURE`.

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
[0.1.1]: https://github.com/franckcl1989/russh-extra/releases/tag/v0.1.1
[0.1.2]: https://github.com/franckcl1989/russh-extra/releases/tag/v0.1.2
