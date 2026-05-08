# Forwarding and Tunnels

Status: Implementing
Roadmap: `docs/dev/roadmap.md#forwarding-and-tunnels`

## Summary

`russh-extra` provides high-level tunnel APIs for local TCP forwarding, remote
TCP forwarding, and direct TCP channels built on `russh` primitives. Direct TCP
and local forwarding have loopback integration coverage; remote forwarding
runtime exists and still needs broader integration tests.

## Motivation

SSH forwarding requires coordinating local listeners, remote global requests,
channel opens, bidirectional stream copying, cancellation, and shutdown.
Applications need concise APIs for common tunnel workflows while still being
able to inspect and control lower-level SSH behavior.

## Accepted Decisions

### Public API shape

Three modes:

```rust
// 1. Local forwarding: bind local TCP → remote target via SSH
let local = session.tunnel(ForwardSpec::local_tcp(
    ("127.0.0.1", 8080),
    ("10.0.0.10", 80),
)).start().await?;
let bound = local.bound_addr();

// 2. Remote forwarding: bind remote TCP → local target via SSH
let remote = session.tunnel(ForwardSpec::remote_tcp(
    ("0.0.0.0", 0),           // port 0 = ephemeral
    ("127.0.0.1", 3000),
)).start().await?;
let remote_port = remote.bound_addr().port();

// 3. Direct TCP: one-shot channel to a remote host:port
let mut stream = session.direct_tcp(&TcpEndpoint::new("db.internal", 5432)).open().await?;
stream.write_all(sql).await?;
```

- `TunnelBuilder` is created from `Session::tunnel(spec)`. Calling `.start()` opens the tunnel.
- `Tunnel` owns the listener task. `bound_addr()` returns the bind address.
- `Tunnel::close()` gracefully shuts down. `Tunnel::abort()` forcefully stops.
- `DirectTcpBuilder::open()` returns a `TunnelStream` (read/write channel).
- All builders carry an `Option<Arc<Mutex<Handle>>>` — only connected sessions produce live tunnels.

### Tunnel lifecycle and drop behavior

- `Tunnel::close()` sends a graceful shutdown signal, waits for the accept
  loop to finish, and returns `Result<()>`.
- `Tunnel::abort()` sends a shutdown signal and does not wait for existing
  connections to finish.
- Dropping a `Tunnel` sends the shutdown signal and abandons the join handle.
  The dropped tunnel is **not** awaited; callers that need deterministic
  cleanup should call `close()`.
- The accept loop handles `TcpListener::accept()`. Between accepts it checks
  the shutdown channel. New connections are rejected after shutdown begins.
- Active forwarded connections during close or abort may receive RST or hang
  depending on timing — this is documented as best-effort.

### Remote forwarding behavior

- The client sends `tcpip-forward` global request. If `port` is 0, the server
  allocates an ephemeral port and returns it; `bound_addr()` reports it.
- The `ClientHandler` stores registered forwarding targets in an
  `Arc<Mutex<HashMap<u16, TcpEndpoint>>>` mapping remote bind ports to local
  targets.
- When the server opens a `forwarded-tcpip` channel, the handler's
  `server_channel_open_forwarded_tcpip` callback fires. It looks up the target,
  opens a local `TcpStream`, and runs bidirectional copy.
- Remote listeners are cancelled via `cancel-tcpip-forward` on close, and the
  registration is removed from the handler.

### Streamlocal

Streamlocal forwarding (`streamlocal-forward@openssh.com` and `direct-streamlocal@openssh.com`)
are deferred until direct TCP and remote TCP forwarding are stable.

### Error policy

- Bind failure -> `Error::Forwarding` with `ForwardingErrorKind::Bind`
- Global request rejection -> `Error::Forwarding` with `ForwardingErrorKind::GlobalRequest`
- Channel open failure -> `Error::Forwarding` with `ForwardingErrorKind::ChannelOpen`
- Target connect failure -> logged and the per-connection channel is closed
- Stream copy error → logged as warning, not propagated (best-effort per-connection)

### Security

- Server-side forwarding handlers authorize based on direction, bind address,
  target address, and authenticated username.
- Default server forwarding policy: deny all `tcpip-forward`, deny all
  `channel_open_direct_tcpip`, deny all `channel_open_forwarded_tcpip`.
- Tracing includes bind/target endpoints but never stream payloads.

### Feature gating

- `tunnel` feature exposes all tunnel types. Depends on `client` and `server`.
- `russh-extra --no-default-features --features tunnel,aws-lc-rs` must compile.

## Mapping to `russh`

| russh-extra concept | russh API |
|---|---|
| Local forward accept | `handle.channel_open_direct_tcpip(host, port, origin, origin_port)` → `Channel<Msg>` |
| Remote forward request | `handle.tcpip_forward(address, port)` → `u32` (allocated port) |
| Remote forward cancel | `handle.cancel_tcpip_forward(address, port)` → `()` |
| Forwarded channel callback | `client::Handler::server_channel_open_forwarded_tcpip(channel, ...)` |
| Bidirectional copy | `Channel::split()` → `(ChannelReadHalf, ChannelWriteHalf)`, `ChannelReadHalf::make_reader()` gives `AsyncRead` |
| Server auth (tcpip-forward) | `server::Handler::tcpip_forward(address, &mut port, session)` → `bool` |
| Server auth (direct-tcpip) | `server::Handler::channel_open_direct_tcpip(channel, ...)` → `bool` |
| Server auth (forwarded-tcpip) | `server::Handler::channel_open_forwarded_tcpip(channel, ...)` → `bool` |

## Edge cases

- Binding to port `0` requires reporting the actual bound port via `TcpListener::local_addr()`.
- Remote forwarding can succeed globally but individual forwarded connections can fail.
- Remote forwarding cancellation can fail if the server already removed the listener.
- One stream direction can EOF before the other — the copy loop handles half-close.
- Shutdown can race with new accepted connections — the accept loop checks shutdown signal.
- Backpressure is managed by `tokio::io::copy_bidirectional` buffer sizes and `russh` window management.
- The `tunnel` feature requires both `client` and `server` features enabled.

## Testing Plan

- Unit tests for `TunnelBuilder`/`DirectTcpBuilder` field access and debug redaction.
- Integration tests for local forwarding (connect -> forward -> verify data).
- Integration tests for remote forwarding (bind -> forward -> verify data).
- Integration tests for direct TCP (open channel -> send/receive data).
- Integration tests for `close()` and `abort()` lifecycle.
- Feature-gating checks for `tunnel`.

## Alternatives considered

- Expose only raw forwarding channels: rejected — leaves lifecycle management to applications.
- SOCKS-style dynamic forwarding: deferred after direct TCP and basic forwarding are stable.
- Streamlocal forwarding: deferred pending the direct TCP baseline.

## Out of scope

SFTP and shell channel behavior are covered by separate design documents.
Streamlocal forwarding is deferred.
Dynamic (SOCKS) forwarding is deferred.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are broadly specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Tunnel lifecycle and drop behavior are specified.
- [x] Remote forwarding cancellation behavior is specified.
- [x] Streamlocal support matrix is specified (deferred).
