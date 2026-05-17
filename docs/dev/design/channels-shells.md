# Channels and Shells

Status: Implemented (first runtime slice)
Roadmap: `docs/dev/roadmap.md#channels-and-shells`

## Summary

`russh-extra` provides typed channel wrappers for interactive shells, PTY
allocation, and SSH subsystems. Shell API exposes streaming async I/O plus
resize, signal, and exit status observation. `ShellHandle::into_async_io()`
provides a `ShellAsyncIo` wrapper implementing `tokio::io::AsyncRead` and
`AsyncWrite` for users who need standard Tokio I/O traits. PTY configuration
uses the existing `Pty` and `TerminalMode` types from `russh-extra-core`.
Subsystem channels support generic named subsystems as well as SFTP.

## Motivation

SSH channels are the common substrate for commands, shells, subsystems, and
forwarding. Users should not have to repeatedly wire stdout, stderr, stdin,
EOF, exit-status, and close handling for every workflow. A typed channel layer
keeps shell and subsystem APIs consistent and provides streaming I/O where the
command API provides only buffered capture.

## Accepted Decisions

### Channel ownership model

`ShellHandle` is a monolithic handle wrapping `russh::Channel<Msg>`. It owns
the channel message stream and buffers unread `Data` and `ExtendedData` bytes
internally. Users call `read()` to drain buffered bytes or await new messages.
Write operations write directly to the channel via `channel.data()`.

Users who need Tokio trait integration can convert the handle with
`ShellHandle::into_async_io()`. That conversion moves channel ownership into a
background bridge task and returns `ShellAsyncIo`, which implements
`AsyncRead` and `AsyncWrite`.

Split read/write halves (`ShellStdin` / `ShellStdout` / `ShellStderr`) and
direct trait implementations on `ShellHandle` are deferred to a future design.

### Error policy

Channel open failure, exec/shell/subsystem rejection, read/write failures,
EOF semantics, remote close, and missing exit status are distinguishable
through existing error types. PTY rejection produces a `Channel` error with
`ChannelErrorKind::Request`. Shell/subystem rejection maps to the same kind.

### Cancellation and shutdown policy

`ShellHandle::close()` is available for explicit shutdown. Dropping a
`ShellHandle` drops the local channel handle; it does not perform async
cleanup. `exit()` returns `Option<&CommandExit>` once an exit status or signal
has been observed.

After conversion to `ShellAsyncIo`, `AsyncWrite::poll_shutdown()` sends EOF to
the remote channel. Dropping `ShellAsyncIo` closes the command side of the
bridge task; it does not wait for a remote close.

### Feature flags

- `shell` (depends on `client`): client-side `Shell`, `ShellBuilder`,
  `ShellHandle`, `ShellAsyncIo`, `Subsystem`, and `SubsystemBuilder`.
- Server-side `shell_request()`, `pty_request()`, `subsystem_request()` always
  available on `ServerHandler` when `server` is enabled.
- Subsystem client-side types live under the same `shell` feature (subsystem
  is a channel type, not a standalone SSH protocol).
- `shell` is not in default features.

### Escape hatches to `russh`

`ShellHandle` exposes the underlying `russh::Channel<Msg>` through a
`russh_channel()` method for users who need unsupported channel operations.

## User-facing API

### Interactive shell

```rust
use russh_extra::{Pty, Session};

let mut shell = session
    .shell()
    .pty(Pty::new("xterm-256color", 120, 40))
    .env("LANG", "C.UTF-8")
    .build()
    .open()
    .await?;

shell.write_all(b"echo ready\n").await?;
let mut buf = vec![0u8; 4096];
let n = shell.read(&mut buf).await?;
println!("{}", String::from_utf8_lossy(&buf[..n]));

// Resize the terminal
shell.resize(80, 40).await?;

// Send a signal
shell.signal(russh_extra::russh::Sig::INT).await?;

shell.close().await?;
```

### Subsystem client

```rust
let mut sftp_channel = session
    .subsystem("sftp")
    .build()
    .open()
    .await?;

// The subsystem channel uses the same handle type as ShellHandle.
sftp_channel.write_all(b"version\n").await?;
let mut buf = [0u8; 1024];
let n = sftp_channel.read(&mut buf).await?;
```

### Tokio AsyncRead and AsyncWrite

```rust
let mut io = session
    .shell()
    .pty(russh_extra::Pty::new("xterm-256color", 120, 40))
    .build()
    .open()
    .await?
    .into_async_io()
    .await?;

tokio::io::AsyncWriteExt::write_all(&mut io, b"echo ready\n").await?;
let mut out = Vec::new();
tokio::io::AsyncReadExt::read_to_end(&mut io, &mut out).await?;
```

## Behavior

### Happy path

1. User creates a `ShellBuilder` via `Session::shell()`.
2. Builder collects PTY, terminal modes, and environment variables.
3. `open()` opens a session channel, requests PTY (if configured), sends
   shell request.
4. On success, returns a `ShellHandle` for streaming I/O.
5. `read()` returns buffered stdout/stderr data or reads new channel messages.
6. `write()` writes to channel stdin.
7. `resize()`, `signal()` send window-change and signal messages.
8. `exit()` returns exit status/signal once the remote process exits.
9. `close()` closes the channel.

### Async I/O wrapper

1. User calls `ShellHandle::into_async_io()`.
2. The handle spawns a bridge task that owns the underlying `russh` channel.
3. The bridge task forwards channel data to `ShellAsyncIo` reads.
4. `AsyncWrite` calls send write commands to the bridge task.
5. `poll_shutdown()` sends channel EOF.
6. `ShellAsyncIo::resize()` and `ShellAsyncIo::signal()` send control commands
   through the same bridge.

### Subsystem opening

1. `Session::subsystem(name)` returns a `SubsystemBuilder`.
2. `open()` opens a session channel, sends `subsystem` request.
3. Returns `ShellHandle` for subsystem I/O.

### Defaults

- Default PTY: none. Shell opens without PTY if `pty()` is not called.
- `Pty::new()` callers choose terminal name and dimensions explicitly.
- No environment variables are sent unless `env()` is called. Servers may
  reject shell requests without PTY allocation.

### Error cases

| Condition | Error |
|-----------|-------|
| Channel open fails | `Error::Channel` with `ChannelErrorKind::Open` |
| Shell request rejected | `Error::Channel` with `ChannelErrorKind::Request` |
| PTY request rejected | `Error::Channel` with `ChannelErrorKind::Request` |
| Subsystem request rejected | `Error::Channel` with `ChannelErrorKind::Request` |
| Env request rejected | Non-fatal; continues shell opening |
| Read after channel close | `Ok(0)` once buffered data is drained |
| Write after remote close | Channel write error mapped through `map_shell_error` |

### Cancellation and shutdown

- `close()` is explicit and returns `Result`.
- Dropping `ShellHandle` does not block on async cleanup.
- `exit()` is available after close.
- Dropping `ShellAsyncIo` is best-effort. The bridge task exits when the
  command channel or SSH channel closes.

## Security

Shell APIs can transmit secrets through stdin and environment variables. Debug
(`#[derive(Debug)]` is not derived on `ShellHandle`) and tracing must not log
shell stdin, command stdin, or environment values by default. PTY dimensions
and terminal names may be traced.

Environment variable APIs document that remote servers may reject or ignore
variables. No environment variables are set by default.

## Mapping to `russh`

| Feature | `russh` API |
|---------|-----------|
| Channel open | `Handle::channel_open_session()` → `Channel<Msg>` |
| PTY request | `Channel::request_pty(want_reply, term, cols, rows, pix_w, pix_h, modes)` |
| Shell request | `Channel::request_shell(want_reply)` |
| Subsystem request | `Channel::request_subsystem(want_reply, name)` |
| Exec request | `Channel::exec(want_reply, command)` (existing) |
| Set environment | `Channel::set_env(want_reply, name, value)` |
| Window change | `Channel::window_change(cols, rows, pix_w, pix_h)` |
| Signal | `Channel::signal(sig)` |
| Write data | `Channel::data(bytes)` |
| Write EOF | `Channel::eof()` |
| Read messages | `Channel::wait()` → `ChannelMsg::Data`, `ExtendedData`, `Eof`, `Close`, `ExitStatus`, `ExitSignal`, `Success`, `Failure` |
| Close channel | `Channel::close()` |
| Async I/O wrapper | background task owns `Channel<Msg>` and exposes Tokio `AsyncRead`/`AsyncWrite` through mpsc channels |

### Terminal mode mapping

`russh` uses `(Pty, u32)` tuples for terminal modes where `Pty` is a russh
enum. `russh-extra-core::TerminalMode` is a local enum with the same opcode
meanings. Conversion happens at the channel boundary.

### Server handler methods

| Server-side callback | `russh::server::Handler` method |
|----------------------|--------------------------------|
| PTY request | `pty_request(channel, term, cols, rows, pix_w, pix_h, modes, session)` |
| Shell request | `shell_request(channel, session)` |
| Subsystem request | `subsystem_request(channel, name, session)` |
| Env request | `env_request(channel, name, value, session)` |
| Window change | `window_change_request(channel, cols, rows, pix_w, pix_h, session)` |
| Signal | `signal(channel, signal, session)` |

All exist on `russh::server::Handler` trait with default implementations that
do nothing.

## Feature Flags and Compatibility

- `client` exposes `Session::command()` (existing), `Session::shell()`, and
  `Session::subsystem()`.
- `shell` depends on `client` and exposes `Shell`, `ShellBuilder`,
  `ShellHandle`, `ShellAsyncIo`, `Subsystem`, and `SubsystemBuilder`.
- `server` exposes `shell_request()`, `pty_request()`, `subsystem_request()`,
  `env_request()`, `window_change_request()` on `ServerHandler`.
- `sftp` depends on `client` and exposes the native SFTP runtime. It uses
  subsystem channels and is specified in the native SFTP design.
- `russh-extra --no-default-features --features shell,aws-lc-rs` compiles.

## Edge cases

- Servers may send exit status before EOF.
- Servers may close a channel without exit status or signal → `exit()` returns
  `CommandExit::Missing`.
- Stderr can continue after stdout EOF. `read()` interleaves stdout and stderr
  in arrival order.
- Channel writes can race with remote close → `write()` returns `Io` error.
- PTY requests can be rejected while command execution would otherwise work.
- Environment variables can be ignored or rejected by the server (non-fatal).
- Shells can be long-lived; `ShellHandle` buffers unread bytes from the
  channel message stream. Users must call `read()` to advance the stream.
- `ShellHandle` does not spawn background tasks. Reading and writing happen
  synchronously within async calls.
- `ShellAsyncIo` does spawn one background bridge task. It terminates when the
  SSH channel closes or when the command channel is dropped.
- `ShellAsyncIo` interleaves normal channel data into one `AsyncRead` stream.
  It is not a split stdout/stderr abstraction.

## Testing Plan

### Unit tests

- `Pty` conversion to `russh` terminal modes tuple.
- `ShellBuilder` validation: missing host, missing session.
- `CommandExit::Missing` behavior for shell exit.
- PTY builder default validation.
- `ShellAsyncIo` debug redaction and EOF state behavior.

### Integration tests (client-side)

- Shell open + stdin write + stdout read back (echo server).
- PTY allocation with custom dimensions.
- Environment variable propagation.
- Shell resize request.
- Shell signal delivery.
- Subsystem open + basic I/O.
- PTY rejection by server.
- Shell rejection by server.
- Subsystem rejection by server.
- Read after close.
- Write after close.
- Exit status observation after shell close.
- Concurrent shell and command channels on same session.
- `ShellHandle::into_async_io()` supports Tokio copy/read/write flows.

### Integration tests (server-side)

- `ServerHandler::shell_request()` callback.
- `ServerHandler::pty_request()` callback.
- `ServerHandler::subsystem_request()` callback.
- `ServerHandler::env_request()` callback.
- `ServerHandler::window_change_request()` callback.
- Server shell I/O: read from client, write to client.
- Server shell exit status.

### Feature-gating checks

- `cargo check --no-default-features --features shell,aws-lc-rs`
- `cargo check --no-default-features --features server,aws-lc-rs`
- `cargo test --all-features`

### Negative tests

- Shell open on disconnected session.
- Subsystem request for invalid subsystem.
- Environment variable rejection (server drops env silently).
- Remote disconnect during shell I/O.
- Double close.

## Alternatives considered

### Split read/write halves

`ShellRead` + `ShellWrite` with shared channel ownership via `Arc<Mutex<>>`.
Deferred: `ShellAsyncIo` covers the standard Tokio I/O use case without
stabilizing a more complex split-stream API yet.

### Direct AsyncRead and AsyncWrite on ShellHandle

Rejected for now: `ShellHandle` also exposes shell-specific operations such as
resize, signal, exit observation, and raw-channel access. The conversion method
makes the ownership change explicit when users want Tokio trait integration.

### Separate stdout / stderr reads

`read_stdout()` + `read_stderr()` methods. Declined: most interactive shell
use cases want interleaved output. Users who need separate streams can use
command execution with buffered capture.

### Subsystem as separate feature flag

A standalone `subsystem` feature flag. Declined: subsystem is a channel type,
not a separate SSH protocol. It reuses the same channel lifecycle as shell
and command. The `shell` feature enables both shells and generic subsystems.
The `sftp` feature is reserved for the native packet layer rather than generic
subsystem transport. Runtime SFTP behavior is specified in the native SFTP
design.

## Out of scope

SFTP packet behavior and forwarding lifecycle are covered by separate designs.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Channel ownership model is specified.
- [x] `ShellAsyncIo` ownership and shutdown behavior is specified.
- [x] Split I/O and direct trait implementation API is specified (deferred).
- [x] Missing status behavior is specified.
