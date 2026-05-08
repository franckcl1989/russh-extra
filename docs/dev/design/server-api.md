# Server API

Status: Implementing
Roadmap: `docs/dev/roadmap.md#server-api`

## Summary

`russh-extra` provides a high-level async server API for accepting SSH
connections, authenticating users, routing buffered and streaming `exec`
requests, and shutting down predictably.

This design is implemented for listener startup, in-memory or loaded host keys,
password authentication, public-key authentication, keyboard-interactive
authentication, buffered and streaming exec, shell, PTY, subsystem, forwarding
authorization, connection lifecycle hooks, and graceful shutdown. It remains
Implementing while server hardening and deferred lifecycle/SFTP integrations are
tracked.

## Motivation

Using `russh` directly for servers requires implementing handler traits,
tracking per-connection state, mapping authentication outcomes, deciding when
to grant channel requests, sending command output and exit status in the right
order, and coordinating shutdown. The first server API should make small SSH
servers and test fixtures concise while keeping the underlying `russh` session
and channel model visible.

## Accepted Decisions

- Public API shape: users create a `Server` with `Server::builder()`, configure
  listen address, host keys, authentication, command routes (buffered or
  streaming), limits, and shutdown behavior, then run it with `run()` or
  `run_until()`.
- Exec routing: buffered routes return `ExecResponse` in one shot. Streaming
  routes receive a `StreamingExecContext` that provides stdin/stdout/stderr
  streaming through mpsc channels and exit-status signalling.
- First accepted runtime slice: TCP listener startup, password
  authentication, session channel acceptance after authentication, buffered
  `exec` request routing, request rejection, command stdout/stderr/exit
  responses, and graceful shutdown.
- Error policy: invalid configuration uses `Error::InvalidConfig`; local bind
  and listener failures use `Error::Io`; SSH negotiation and lower-level
  session failures are classified into the existing transport, authentication,
  channel, disconnected, cancelled, or SSH categories when surfaced to users.
- Cancellation and shutdown policy: explicit server shutdown stops accepting
  new connections, rejects new high-level requests, sends best-effort SSH
  disconnects to active sessions, exposes shutdown state to handlers, and waits
  for active runtime-owned tasks for a configured grace period.
- Feature flags: server APIs require the `server` feature and a `russh` crypto
  backend feature such as `aws-lc-rs` or `ring`.
- Escape hatches to `russh`: server host keys can be constructed from
  `russh::keys::PrivateKey`, and handler contexts expose lower-level `russh`
  channel/session identifiers or handles only where ownership is clear.
- Safe defaults: servers reject all authentication, all commands, all shells,
  all subsystems, and all forwarding requests until configured otherwise. A
  server without at least one host key is invalid.

## User-facing API

The first slice targets a buffered command server. The example below is the
accepted public API shape for implementation:

```rust
let host_key = russh_extra::ServerHostKey::from_openssh_file(
    "testdata/host_ed25519",
)?;

let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key(host_key)
    .password_auth(|ctx, password| async move {
        if ctx.username().as_str() == "demo" && password.expose_secret() == "demo" {
            Ok(russh_extra::AuthDecision::accept())
        } else {
            Ok(russh_extra::AuthDecision::reject())
        }
    })
    .exec("whoami", |ctx| async move {
        if ctx.username().as_str() != "demo" {
            return Ok(russh_extra::ExecResponse::reject());
        }

        Ok(russh_extra::ExecResponse::success()
            .stdout("demo\n")
            .exit(russh_extra::CommandExit::status(0)))
    })
    .build()?;

server.run_until(shutdown_signal()).await?;
```

Tests and tools can create in-memory host keys without touching disk:

```rust
let host_key = russh_extra::ServerHostKey::from_private_key(
    russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )?,
);

let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 0))
    .host_key(host_key)
    .password_auth(|_, _| async { Ok(russh_extra::AuthDecision::accept()) })
    .exec("ok", |_| async {
        Ok(russh_extra::ExecResponse::success()
            .stdout("ok\n")
            .exit_status(0))
    })
    .build()?;
```

Stateful applications can provide a handler instead of closure routes:

```rust
#[derive(Clone)]
struct App;

impl russh_extra::ServerHandler for App {
    async fn auth_password(
        &self,
        ctx: russh_extra::AuthContext,
        password: russh_extra::Password,
    ) -> russh_extra::Result<russh_extra::AuthDecision> {
        if ctx.username().as_str() == "admin" && password.expose_secret() == "secret" {
            Ok(russh_extra::AuthDecision::accept())
        } else {
            Ok(russh_extra::AuthDecision::reject())
        }
    }

    async fn exec(
        &self,
        ctx: russh_extra::ExecContext,
    ) -> russh_extra::Result<russh_extra::ExecResponse> {
        match ctx.command().as_str() {
            Some("uptime") => {
                Ok(russh_extra::ExecResponse::success().stdout("up\n").exit_status(0))
            }
            _ => Ok(russh_extra::ExecResponse::reject()),
        }
    }
}

let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key_file("testdata/host_ed25519")
    .handler(App)
    .build()?;
```

`ServerHandle` is cloneable and can request shutdown from another task:

```rust
let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key_file("testdata/host_ed25519")
    .build()?;
let handle = server.handle();

tokio::spawn(async move {
    handle.shutdown("maintenance");
});

server.run().await?;
```

### Streaming exec (closure)

For commands that produce output progressively or need to read stdin, add a
streaming route. The handler owns a `StreamingExecContext` which provides async
methods for stdout, stderr, stdin, and exit signalling:

```rust
let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key_file("testdata/host_ed25519")
    .password_auth(|_, _| async { Ok(russh_extra::AuthDecision::accept()) })
    .streaming_exec("tail -f /var/log/app.log", |mut ctx| async move {
        // Read stdin if the client sends it
        while let Some(data) = ctx.read_stdin().await {
            // process each chunk...
        }

        // Send stdout progressively
        ctx.stdout("line 1\n").await?;
        ctx.stdout("line 2\n").await?;

        // Send stderr
        ctx.stderr("warning: something\n").await?;

        // Signal exit and close the channel
        ctx.exit_status(0).await?;
        Ok(())
    })
    .build()?;
```

### Streaming exec (`ServerHandler`)

The `ServerHandler` trait provides `streaming_exec()` alongside `exec()`:

```rust
#[derive(Clone)]
struct App;

impl russh_extra::ServerHandler for App {
    async fn exec(
        &self,
        ctx: russh_extra::ExecContext,
    ) -> russh_extra::Result<russh_extra::ExecResponse> {
        match ctx.command().as_str() {
            Some("uptime") => {
                Ok(russh_extra::ExecResponse::success().stdout("up\n").exit_status(0))
            }
            _ => Ok(russh_extra::ExecResponse::reject()),
        }
    }

    async fn streaming_exec(
        &self,
        mut ctx: russh_extra::StreamingExecContext,
    ) -> russh_extra::Result<()> {
        ctx.stdout("streaming output\n").await?;
        ctx.exit_status(0).await?;
        Ok(())
    }
}

let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key_file("testdata/host_ed25519")
    .handler(App)
    .build()?;
```

## Behavior

`Server::builder().build()` validates configuration before runtime starts:

- At least one host key is required.
- The listen endpoint must contain a host and port accepted by
  `tokio::net::TcpListener`.
- Authentication defaults to reject-all.
- Command routing defaults to reject-all.
- Shutdown grace defaults to 30 seconds.
- Maximum connections, authentication attempts, and sessions per connection
  have explicit defaults and builder setters.

Host-key behavior:

- `ServerHostKey::from_private_key()` accepts an already loaded
  `russh::keys::PrivateKey`.
- `ServerHostKey::from_openssh_file()` loads an OpenSSH private key from disk.
- `ServerHostKey::from_openssh_pem()` loads key bytes held by the caller.
- Passphrases use `Password` and must be redacted from `Debug`.
- On Unix platforms, private-key files with group or world access are rejected
  by default with `Error::InvalidConfig`. A future explicit override may relax
  this for compatibility.
- Host private keys never appear in tracing, display, or debug output.

Authentication behavior:

- Password auth, public-key auth, and keyboard-interactive auth call the
  configured closure or `ServerHandler`.
- `auth_none` and certificate auth reject in this slice.
- `AuthDecision::Accept` records the authenticated username on the connection.
- `AuthDecision::Reject` maps to `russh::server::Auth::Reject`.
- Callback errors reject authentication for that attempt and are surfaced
  through the server error path without logging secret values.
- Successful authentication does not authorize any command by itself.

Command routing behavior:

- Session channels are accepted only after authentication succeeds.
- `exec` requests are matched exactly against configured UTF-8 command routes.
  Buffered routes and streaming routes are separate maps; a command can have
  either a buffered handler or a streaming handler, not both.
- If a command matches both a closure route (`.exec()`) and the
  `ServerHandler::exec()` fallback, the closure route wins.
- Invalid UTF-8 command bytes are rejected in the closure-route API. Custom
  handlers may inspect raw command bytes through `ExecCommand::as_bytes()`.
- `ExecCommand::as_str()` returns `Some(&str)` for valid UTF-8 and `None` for
  invalid UTF-8.
- Missing routes return SSH channel failure.
- `ExecResponse::reject()` returns SSH channel failure and sends no stdout,
  stderr, exit status, EOF, or close from `russh-extra`.
- Accepted buffered responses send channel success, stdout data, stderr extended
  data with SSH extended data type `1`, exit status or signal when supplied, EOF,
  and close.
- `CommandExit::Missing` sends no exit status or signal.
- Handler errors before channel success return channel failure when possible.
  Handler errors after channel success close the channel best-effort and are
  classified as channel, disconnect, or SSH errors for diagnostics.

### Streaming exec behavior

- When a streaming route matches, `exec_request` sends `channel_success` and
  then enters a loop that pumps data between the handler and the SSH session:
  - Handler calls `ctx.stdout(data)` → mpsc sender → `exec_request` loop calls
    `session.data()` on the next `await`.
  - Handler calls `ctx.stderr(data)` → mpsc sender → `exec_request` loop calls
    `session.extended_data()`.
  - Client sends stdin data → `channel_data()` handler callback → mpsc sender
    → handler `ctx.read_stdin()` returns it.
  - Handler calls `ctx.exit_status(status)` / `ctx.exit_signal(signal)` →
    `exec_request` loop writes `exit_status_request` / `exit_signal_request`
    to the session, then breaks the loop.
- When the handler future completes (returns `Ok(())` or `Err`), the
  mpsc channel closes. If the handler did not signal exit, the loop sends
  `ExitStatus(0)` for success or `ExitSignal("TERM")` for error.
- After the loop exits, `send_exec_response_finalize()` sends `eof` and
  `close` on the channel.
- If the client disconnects or the channel closes while the handler is running,
  the mpsc channel signals closure and the handler's next `await` returns an
  error.
- The handler can check server shutdown with `ctx.server().is_shutting_down()`.
- Stdin data arrived before the streaming handler reads it is buffered in the
  mpsc channel (backpressure is applied by the channel capacity).

### Channel data forwarding

- `HighLevelRusshHandler::channel_data()` inspects the per-channel streaming
  state. If no streaming exec is active for the channel, data is discarded
  silently (the default for non-shell/non-streaming-exec channels).

Authorization behavior:

- Authentication and authorization are separate decisions.
- A configured route only says the command is known. The command handler still
  decides whether the authenticated user may run it by returning an accepted or
  rejected `ExecResponse`.
- Authorization decisions have access to username, peer address, connection ID,
  channel ID, command bytes, and shutdown state.
- Shell, PTY, subsystem, and forwarding requests reject by default unless the
  caller registers the corresponding handler or implements it through
  `ServerHandler`.

Shutdown behavior:

- `Server::run()` runs until listener failure or `ServerHandle::shutdown()`.
- `Server::run_until(future)` requests shutdown when the future resolves.
- Shutdown is idempotent. The first reason string is used for best-effort SSH
  disconnect messages.
- New TCP accepts stop first. Existing connections receive disconnect requests
  through `russh::server::RunningServerHandle` or per-session handles.
- Handler contexts expose a shutdown handle so long-running auth or exec logic
  can check whether shutdown was requested.
- The runtime waits for active connection and handler tasks for the configured
  shutdown grace period. If the grace period expires, runtime-owned tasks are
  aborted and `Error::Cancelled` with `Operation::Shutdown` is returned.
- Dropping a `ServerHandle` does not shut down the server. Dropping the future
  returned by `run()` cancels the local server operation without a graceful SSH
  disconnect guarantee.

## Security

Servers must not silently allow authentication. The default auth policy rejects
all users until the user configures password authentication or a custom
handler.

Host private keys, passwords, passphrases, and command stdin must not appear in
logs, tracing fields, `Debug`, `Display`, or error messages. Server tracing may
include connection IDs, peer addresses, usernames after authentication,
request names, command route names, and byte counts. It must not include
passwords, passphrases, private key material, full command input, or full
command output.

Private-key files are security-sensitive. On Unix, group-readable,
group-writable, world-readable, or world-writable host-key files are rejected
unless a future explicit compatibility option is added.

Authorization is separate from authentication. The default command, subsystem,
and forwarding policies reject. A password match alone must never imply that a
user may run every command.

## Mapping to `russh`

The server API maps to official `russh` server APIs:

- `russh::server::Config` for SSH identification, host keys, algorithm lists,
  authentication methods, auth attempts, channel buffer sizes, keepalive, and
  inactivity timeout.
- `russh::server::Server::run_on_socket` and `run_on_address` for listener and
  accept-loop integration.
- `russh::server::Handler` for password authentication, public-key
  authentication, authentication success, channel data, session channel opens,
  channel close/EOF, `exec_request`, shell, subsystem, PTY, environment,
  signal, forwarding, and other request callbacks.
- `russh::server::Session` for `channel_success`, `channel_failure`, `data`,
  `extended_data`, `exit_status_request`, `exit_signal_request`, `eof`,
  `close`, and session handles.
- `russh::server::RunningServerHandle` for best-effort shutdown requests.
- `russh::keys::PrivateKey` for host-key material.

No third-party SSH server or protocol abstraction crate is involved.

`russh` handler methods receive many request types. The accepted path is to
reject callbacks explicitly until their own designs define public behavior.

### Streaming exec mapping

The streaming exec bridge uses `tokio::sync::mpsc` channels to connect the
async handler future (running in a `tokio::spawn`) with the synchronous
`exec_request` handler that holds `&mut server::Session`:

- Outbound channel (handler → session): `mpsc::UnboundedSender<StreamingExecCmd>`.
  The handler calls `ctx.stdout()` / `ctx.stderr()` / `ctx.exit_status()` which
  send commands through this channel. The `exec_request` handler drains the
  receiver, calling `session.data()`, `session.extended_data()`,
  `session.exit_status_request()`, or `session.exit_signal_request()` for each
  command.
- Inbound channel (session → handler): `mpsc::UnboundedSender<Bytes>`.
  `HighLevelRusshHandler::channel_data()` forwards client data into this sender.
  The handler calls `ctx.read_stdin()` which reads from the receiver.
- Per-channel streaming state is stored in
  `HashMap<ChannelId, mpsc::UnboundedSender<Bytes>>` on `HighLevelRusshHandler`,
  populated when a streaming exec starts and removed when it completes.

This avoids holding `&mut server::Session` across `.await` points while keeping
the handler future's API idiomatic (plain Rust async methods).

## Feature Flags and Compatibility

- `server` exposes `Server`, `ServerBuilder`, `ServerHandle`,
  `ServerHostKey`, `ServerHandler`, `AuthContext`, `AuthDecision`,
  `ExecContext`, `ExecCommand`, `ExecResponse`, `StreamingExecContext`,
  `StreamingExecCmd`, `CommandExit`, `SessionContext`, and `SessionId`.
- `server` depends on `_russh`; users that construct host keys from lower-level
  types can access the re-exported `russh` crate when `_russh` is enabled.
- `server,sftp` does not add SFTP server behavior until the SFTP server design
  is accepted.
- `tunnel` depends on `client` and `server` and exposes forwarding
  authorization hooks. Requests reject by default until user handlers accept
  them.
- `russh-extra --no-default-features` must compile.
- `russh-extra --no-default-features --features server,aws-lc-rs` must compile.

This project is pre-1.0. Breaking changes are allowed when they improve the
full-featured `russh`-based API and the design docs are updated in the same
work item.

## Edge cases

- A client can authenticate successfully and then request an unknown command.
- A configured command can reject one authenticated user and accept another.
- A client can request `exec` before authentication completes.
- A client can send non-UTF-8 command bytes.
- A client can close the channel while the server is preparing a buffered
  response.
- A handler can fail after the server has already sent channel success.
- Shutdown can race with accept, authentication, route lookup, or response
  delivery.
- A shutdown grace timeout can expire while user handler code is still running.
- Multiple clients can run the same route concurrently.
- Multiple channels can be active on one authenticated connection.
- Host-key files can have insecure permissions or unsupported formats.
- Binding port `0` requires exposing the actual bound address for tests.
- Streaming exec: the handler can stall without producing output (no timeout
  is imposed by the framework; users can use `tokio::time::timeout`).
- Streaming exec: the handler can send exit status and then continue writing
  stdout/stderr (these writes fail; the channel is already closing).
- Streaming exec: the client can send stdin before the handler calls
  `read_stdin()` (buffered in the mpsc channel).
- Streaming exec: the client can close their end of the channel while the
  handler is still running (handler's next I/O call returns an error).
- Streaming exec: a streaming handler error after channel success is
  surfaced as `Error::Channel` with information about the channel, keeping the
  same error policy as buffered exec.

## Testing Plan

- Unit tests for server builder validation, reject-all defaults, host-key debug
  redaction, password/passphrase redaction, host-key file permission checks,
  auth decision mapping, command route matching, and shutdown option defaults.
- Unit tests for `StreamingExecContext` send/receive behavior, mpsc channel
  exhaustion, exit-status ordering, and error propagation.
- Integration tests with local loopback `russh` clients for bind on port `0`,
  successful password authentication, rejected password authentication,
  successful command execution, non-zero command exit, stderr output, missing
  exit status, unknown command rejection, handler-level authorization
  rejection, client channel close during response, and graceful shutdown.
- Integration tests for concurrent clients and multiple session channels when
  limits permit them.
- Integration tests for streaming exec: stdout streaming, stderr streaming,
  stdin forwarding, exit status, exit signal, client disconnection during
  streaming, handler error during streaming, and concurrent streaming + buffered
  exec on the same connection.
- Negative tests for shell, subsystem, PTY, forwarding, and unauthenticated
  session-channel requests returning failure.
- Feature-gating checks for `--no-default-features`,
  `--features server,aws-lc-rs`, and default features.
- Error-path tests should assert typed errors where the public API returns an
  error. Protocol-level request rejection should be asserted through the SSH
  channel response observed by the client.

## Alternatives considered

Expose raw `russh` server handlers only. This preserves full control but does
not solve the routing, auth, limit, response-ordering, and shutdown boilerplate
that the crate is intended to remove.

Provide only macro-based routing. This would hide behavior in generated code
and make the runtime harder to debug. Macros can be added later as optional
ergonomics over normal handler APIs.

Start with streaming command handlers. Streaming is useful, but it introduces
stdin ownership, backpressure, async close-on-drop, cancellation, and output
ordering questions that should be accepted in a separate design. The first
server slice returns buffered `ExecResponse` values.

Generate a production host key by default. This would make insecure defaults
too easy. Tests can generate in-memory keys explicitly; production servers
must provide host keys.

## Open questions

- Deferrable: certificate-based and multi-factor authentication.
- Deferrable: persistent authorized-key stores and host-key rotation helpers.
- Deferrable: streaming exec stdin polling mode (`try_read_stdin()`) for
  non-blocking reads.
- Deferrable: channel lifecycle hooks (`on_channel_open`, `on_channel_close`).
- Deferrable: SFTP subsystem server integration.
- Deferrable: typed routing macros.
- Deferrable: exposing more raw `russh` session and channel handles after
  ownership rules are proven by implementation.

## Out of scope

Client session APIs, SFTP packet design, and known-hosts behavior are covered by
separate design documents or future design updates.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as target API for the
  accepted implementation.
- [x] Runtime behavior and error policy are fully specified for buffered exec.
- [x] Runtime behavior and error policy are fully specified for streaming exec.
- [x] Mapping to official `russh` APIs is explicit (including mpsc bridge).
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Handler API shape is specified (both `exec()` and `streaming_exec()`).
- [x] Host-key API is specified.
- [x] Shutdown and cancellation behavior is specified.
- [x] Authorization model is specified.
- [x] Open questions are either resolved or marked deferrable.
