# Client Session API

Status: Implemented
Roadmap: `docs/dev/roadmap.md#client-api`

## Summary

`russh-extra` provides a high-level client API for connecting to SSH servers,
authenticating, running buffered commands, and opening shell, subsystem, SFTP,
and tunnel entry points from a connected session.

This design is implemented for `Client::connect()` and buffered
`Session::command()`. Shell/subsystem, known-hosts, authentication, forwarding,
and SFTP details are specified by their own design documents.

## Motivation

Using `russh` directly gives users the full SSH protocol surface, but common
client workflows require repeated handler, channel, stdout/stderr, exit-status,
timeout, host-key, and authentication bookkeeping. `russh-extra` should make
the first client workflow concise while keeping the underlying SSH concepts
visible and available through escape hatches.

## Accepted Decisions

- Public API shape: users create a `Client`, call `connect()`, then use a
  connected `Session` for command, shell, SFTP, subsystem, and tunnel entry
  points.
- First accepted runtime slice: `Client::connect()` and buffered
  `Session::command()`.
- Error policy: transport, host-key, authentication, timeout, disconnect, and
  channel failures return typed `Error` variants from the implemented error
  taxonomy.
- Command status policy: buffered command execution returns `Ok(CommandOutput)`
  for remote exit statuses and signals that were reported successfully. A
  separate convenience helper may later map non-successful exits into
  `Error::CommandExit`.
- Bounded buffering policy: buffered command execution enforces separate stdout
  and stderr byte limits. The default is 8 MiB per stream. Exceeding either
  limit returns `Error::Channel` with `ChannelErrorKind::Read`.
- Cancellation and shutdown policy: dropping a connect/auth/channel-open future
  cancels that high-level operation. Dropping a buffered command future must
  drop all local buffers and must not leave a `russh-extra` background task
  accumulating output. Best-effort SSH channel close is required on normal
  completion and explicit error paths. Strong async close-on-drop is deferred
  to a future cancellable command-handle design.
- Feature flags: client APIs require the `client` feature and a `russh` crypto
  backend feature such as `aws-lc-rs` or `ring`.
- Host-key policy: strict verification is the default. Unknown host keys are
  rejected unless the user configures a pinned key verifier, a known-hosts
  store, trust-on-first-use, or the explicit unsafe opt-out.
  `accept_any_host_key()` maps to `HostKeyPolicy::InsecureAcceptAny`.
- Credential order: credentials are attempted in the order configured by the
  builder. `none` auth is attempted only when explicitly configured.
- Escape hatches to `russh`: connected sessions expose an async raw-handle
  guard around the underlying `russh::client::Handle<ClientHandler>`. The guard
  serializes direct `russh` access with high-level operations.

## User-facing API

Users create a client with a builder:

```rust
let client = russh_extra::Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .try_pinned_host_key_sha256("SHA256:base64-fingerprint")?
    .agent()
    .build();

let session = client.connect().await?;
let output = session.command("uname -a").await?;

if output.success() {
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
```

Loopback tests and controlled environments may opt out of host-key checking
explicitly:

```rust
let client = russh_extra::Client::builder()
    .endpoint(("127.0.0.1", 2222))
    .username("test")
    .password("test")
    .accept_any_host_key()
    .build();
```

Buffered command limits are configurable per command:

```rust
let command = russh_extra::RemoteCommand::new("journalctl -n 1000")
    .stdout_limit(2 * 1024 * 1024)
    .stderr_limit(512 * 1024);

let output = session.command(command).await?;
```

Advanced users can drop to `russh` through a serialized raw-handle guard:

```rust
let mut raw = session.russh_handle().await?;
let mut channel = raw.channel_open_session().await?;
channel.exec(true, "printf raw").await?;
```

Shell, SFTP, subsystem, and tunnel APIs hang off the connected session:

```rust
let shell = session.shell().build().open().await?;
let sftp_result = session.sftp().await; // currently returns Error::Unsupported
let tunnel = session.tunnel(russh_extra::ForwardSpec::local_tcp(
    ("127.0.0.1", 8080),
    ("10.0.0.10", 80),
));
```

## Behavior

`Client::connect()` opens a TCP connection through `russh::client::connect`,
performs SSH negotiation, verifies the host key through the configured
`HostKeyPolicy`, and tries configured credentials in order.

Timeout behavior:

- `Timeouts::connect` wraps TCP connect and SSH negotiation.
- `Timeouts::auth` wraps each authentication attempt.
- `Timeouts::channel_open` wraps opening the session channel for buffered
  command execution.
- A timeout returns `Error::Timeout` with `Operation::Connect`,
  `Operation::Authentication`, or `Operation::ChannelOpen`.

Host-key behavior:

- `HostKeyPolicy::Strict` rejects unknown host keys. It is the default.
- `HostKeyPolicy::InsecureAcceptAny` accepts every host key and maps from
  `ClientBuilder::accept_any_host_key()` or
  `ClientBuilder::strict_host_key_checking(false)`.
- `HostKeyPolicy::PinnedSha256` accepts only matching SHA256 fingerprints.
- Rejections return `Error::HostKey` with `HostKeyErrorKind::Unknown`,
  `Changed`, `Rejected`, `Unsupported`, or `Unavailable` as appropriate.

Authentication behavior:

- No credentials configured returns `Error::Authentication` with
  `AuthenticationErrorKind::Unavailable`.
- Rejected credentials continue to the next configured credential.
- If all credentials are rejected, `Client::connect()` returns
  `AuthenticationErrorKind::Exhausted`.
- Partial authentication returns `AuthenticationErrorKind::Partial` unless a
  later configured credential completes authentication.

`Session::command()` opens a session channel, sends an `exec` request with
`want_reply = true`, sends finite stdin bytes if configured, sends EOF after
stdin, captures stdout and stderr, waits for remote exit information, and
returns `CommandOutput`.

`CommandOutput.exit` uses `CommandExit`:

- `CommandExit::Status(code)` when the server reports an exit status.
- `CommandExit::Signal(name)` when the server reports signal termination.
- `CommandExit::Missing` when the channel closes without status or signal.

Channel events for buffered commands are interpreted as:

- `Data` appends to stdout.
- `ExtendedData` with SSH extended data type `1` appends to stderr.
- Other `ExtendedData` is preserved as stderr bytes for the first slice and may
  become typed extended streams later.
- `ExitStatus` records status.
- `ExitSignal` records signal.
- `Eof` records remote EOF but does not finish until close or channel end.
- `Close` or `None` from `russh::Channel::wait()` ends collection.
- `Success` confirms the `exec` request. `Failure` returns `Error::Channel`
  with `ChannelErrorKind::Request`.

If both exit status and signal are reported, the first one observed wins and
the second is ignored. This follows the principle that user code should see one
process outcome.

## Security

Strict host-key checking is enabled by default. `accept_any_host_key()` and
`strict_host_key_checking(false)` are explicit unsafe opt-outs (mapping to
`HostKeyPolicy::InsecureAcceptAny`) and should appear only in tests or
controlled environments.

Credentials are stored in configuration for authentication attempts. Debug and
serialization must not reveal passwords, passphrases, or in-memory private key
bytes.

Tracing may include endpoint, username, session ID, command length, and command
event categories. It must not log command stdin, passwords, passphrases,
private key material, full command strings by default, or full command output.

## Mapping to `russh`

The API maps to official `russh` client APIs:

- `russh::client::Config` for client transport settings.
- `russh::client::Handler::check_server_key` for host-key policy.
- `russh::client::connect` for TCP connection, SSH negotiation, and session
  task ownership.
- `russh::client::Handle` for authentication, channel opens, disconnect,
  keepalive, and raw escape-hatch access.
- `russh::Channel` and `russh::ChannelMsg` for session channel command
  execution, stdout, stderr, EOF, close, exit status, exit signal, success, and
  failure.

No third-party SSH protocol crate is involved.

`russh` does not provide a persistent known-hosts policy at this layer.
`russh-extra` composes its `KnownHosts` store over the public
`check_server_key` callback.

## Feature Flags and Compatibility

- `client` exposes `Client`, `ClientBuilder`, `Session`, `RemoteCommand`,
  `CommandLimits`, `CommandOutput`, host-key policy types, and raw-handle guard
  types.
- `shell` depends on `client` and exposes `Session::shell()`.
- `sftp` exposes reserved experimental SFTP marker types; `Session::sftp()` is
  available when both `client` and `sftp` are enabled and currently returns
  `Error::Unsupported`.
- `tunnel` depends on `client` and `server` because forwarding has client and
  server protocol surfaces.
- `russh-extra --no-default-features` must compile.

This project is pre-1.0. Breaking changes are allowed when they improve the
full-featured `russh`-based API.

## Edge cases

- Remote commands can close stdout before stderr.
- Servers can send exit status before EOF.
- Servers can close the channel without status or signal.
- Remote stderr can continue after stdout EOF.
- Commands can produce output faster than callers consume it.
- Output can exceed configured stdout or stderr limits.
- Stdin is finite bytes in the first slice. Streaming stdin is deferred.
- Authentication can partially succeed before the server rejects all
  credentials.
- TCP connection, SSH negotiation, authentication, and channel open timeouts are
  separate user-visible failure modes.
- Raw-handle access serializes with high-level operations and can block them
  while the guard is held.

## Testing Plan

- Unit tests for endpoint parsing, credential redaction, host-key policy
  defaults, pinned fingerprint validation, command limit defaults, command
  limit validation, command exit helpers, and configuration defaults.
- Integration tests with local loopback `russh` servers for successful connect,
  failed authentication, host-key rejection, accept-any host-key policy,
  successful command, non-zero exit, signal exit, missing status,
  stdout/stderr interleaving, output-limit failure, exec request failure, and
  disconnect during command execution.
- User-level API smoke tests for builder ergonomics and command configuration.
- Feature-gating checks matching `.github/workflows/ci.yml`.
- No tests may depend on external SSH hosts.
- Error-path tests should assert typed errors.

## Alternatives considered

Expose only low-level channel wrappers. This gives maximum flexibility but does
not solve the repeated command and shell boilerplate that this crate exists to
remove.

Return `Err(Error::CommandExit)` for every non-zero exit from
`Session::command()`. This makes simple "run and fail" workflows concise, but it
hides stdout/stderr and signal details behind error handling. The base buffered
command API returns `CommandOutput`; a failing convenience helper can be added
later.

Use a persistent known-hosts store in the first slice. This is now covered by
the known-hosts design and implemented for plain hostnames, save/load, and
trust-on-first-use. Hashed hostnames remain deferred.

## Open questions

- Deferrable: connection pooling and session reuse.
- Deferrable: streaming stdin/stdout/stderr command API.
- Deferrable: async close-on-drop for cancellable command handles.

## Out of scope

Server handlers, SFTP packet APIs, tunnel lifecycle, known-hosts storage, and
streaming command APIs are covered by separate designs or future design
updates.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and base command error policy are specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Host-key verification policy is specified.
- [x] `russh` handle ownership and escape-hatch API are specified.
- [x] Buffered output bounds are specified.
