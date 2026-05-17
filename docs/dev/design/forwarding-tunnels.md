# Forwarding and Tunnels

Status: Implemented
Roadmap: `docs/dev/roadmap.md#forwarding-and-tunnels`

## Summary

`russh-extra` provides high-level tunnel APIs for local TCP forwarding, remote
TCP forwarding, direct TCP channels, local StreamLocal forwarding, remote
StreamLocal forwarding, and direct StreamLocal channels built on official
`russh` primitives.

The TCP runtime has loopback coverage for direct, local, and remote forwarding.
StreamLocal runtime support is implemented where the platform and `russh`
support Unix-domain forwarding, with loopback integration tests for close
socket cleanup and abort no-panic behavior.

## Motivation

SSH forwarding requires coordinating local listeners, remote global requests,
channel opens, bidirectional stream copying, cancellation, and shutdown.
Applications need concise APIs for common tunnel workflows while still being
able to inspect and control lower-level SSH behavior.

## Accepted Decisions

### Public API shape

Four main modes are exposed:

```rust
// 1. Local TCP forwarding: bind local TCP -> remote target via SSH.
let local = session.tunnel(ForwardSpec::local_tcp(
    ("127.0.0.1", 8080),
    ("10.0.0.10", 80),
)).start().await?;
let bound = local.bound_addr();

// 2. Remote TCP forwarding: bind remote TCP -> local target via SSH.
let remote = session.tunnel(ForwardSpec::remote_tcp(
    ("0.0.0.0", 0),           // port 0 = ephemeral
    ("127.0.0.1", 3000),
)).start().await?;
let remote_port = remote.bound_addr().map(|addr| addr.port());

// 3. Direct TCP: one-shot channel to a remote host:port.
let mut stream = session.direct_tcp(("db.internal", 5432)).open().await?;
stream.write_all(sql).await?;

// 4. StreamLocal: Unix-domain forwarding paths where supported.
let streamlocal = session.tunnel(ForwardSpec::local_streamlocal(
    "/tmp/app.sock",
    "/var/run/app.sock",
)).start().await?;
let bound_path = streamlocal.bound_path();
```

- `TunnelBuilder` is created from `Session::tunnel(spec)`. Calling `.start()`
  opens the tunnel.
- `Tunnel` owns the listener or cancellation task. `bound_addr()` returns
  `Some(SocketAddr)` for TCP tunnels and `None` for StreamLocal tunnels.
- `bound_path()` returns `Some(&Path)` for StreamLocal tunnels and `None` for
  TCP tunnels.
- `Tunnel::close()` gracefully shuts down. `Tunnel::abort()` forcefully stops.
- `DirectTcpBuilder::open()` and `DirectStreamLocalBuilder::open()` return a
  `TunnelStream` for bidirectional channel I/O.
- All builders carry an `Option<Arc<Mutex<Handle>>>`; only connected sessions
  produce live tunnels.

### Tunnel lifecycle and drop behavior

- `Tunnel::close()` sends a graceful shutdown signal, waits for the listener or
  cancellation task to finish, and returns `Result<()>`.
- `Tunnel::abort()` sends a shutdown signal and does not wait for existing
  connections to finish.
- Dropping a `Tunnel` sends the shutdown signal and abandons the join handle.
  The dropped tunnel is not awaited. Callers that need deterministic cleanup
  should call `close()`.
- Local accept loops handle `TcpListener::accept()` or `UnixListener::accept()`.
  Between accepts they check the shutdown channel. New connections are rejected
  after shutdown begins.
- Active forwarded connections during close or abort may receive reset, EOF, or
  a channel close depending on timing. This is best-effort cleanup.

### Remote forwarding behavior

- Remote TCP forwarding sends `tcpip-forward`. If the requested port is `0`,
  the server allocates an ephemeral port and `bound_addr()` reports it.
- Remote StreamLocal forwarding sends `streamlocal-forward@openssh.com` and
  records the bound socket path.
- The client handler stores registered TCP targets in an
  `Arc<Mutex<HashMap<u16, TcpEndpoint>>>`.
- The client handler stores registered StreamLocal targets in an
  `Arc<Mutex<HashMap<String, PathBuf>>>`.
- When the server opens a `forwarded-tcpip` or
  `forwarded-streamlocal@openssh.com` channel, the handler looks up the local
  target and runs bidirectional copy.
- Remote listeners are cancelled on `close()` with `cancel-tcpip-forward` or
  `cancel-streamlocal-forward@openssh.com`, and the registration is removed
  from the handler map.

### StreamLocal platform behavior

StreamLocal uses Unix-domain socket paths and is available on Unix platforms.
On non-Unix platforms, local StreamLocal forwarding returns a typed unsupported
error. Direct and remote StreamLocal calls still depend on `russh` support and
remote server support for the OpenSSH StreamLocal request names.

### Error policy

- Bind failure -> `Error::Forwarding` with `ForwardingErrorKind::Bind`
- Listener setup failure -> `Error::Forwarding` with `ForwardingErrorKind::Listen`
- Global request rejection -> `Error::Forwarding` with
  `ForwardingErrorKind::GlobalRequest`
- Channel open failure -> `Error::Forwarding` with
  `ForwardingErrorKind::ChannelOpen`
- Target connect failure -> logged and the per-connection channel is closed
- Stream copy error -> logged as warning/debug and not propagated to the parent
  tunnel because it is per-connection best effort

### Security

- Server-side forwarding handlers authorize based on direction, bind address or
  path, target address or path, origin metadata, and authenticated username.
- Default server forwarding policy denies all `tcpip-forward`,
  `streamlocal-forward@openssh.com`, `channel_open_direct_tcpip`,
  `channel_open_forwarded_tcpip`, and direct StreamLocal requests until user
  handlers accept them.
- Tracing includes bind and target endpoints but never stream payloads.

### Feature gating

- `tunnel` exposes TCP and StreamLocal tunnel types. It depends on `client` and
  `server`.
- `russh-extra --no-default-features --features tunnel,aws-lc-rs` must compile.
- Unix-only tests should use `#[cfg(unix)]`.

## Mapping to `russh`

| russh-extra concept | russh API |
|---|---|
| Local TCP accept | `handle.channel_open_direct_tcpip(host, port, origin, origin_port)` -> `Channel<Msg>` |
| Remote TCP request | `handle.tcpip_forward(address, port)` -> allocated port |
| Remote TCP cancel | `handle.cancel_tcpip_forward(address, port)` |
| Forwarded TCP callback | `client::Handler::server_channel_open_forwarded_tcpip(channel, ...)` |
| Local StreamLocal accept | `handle.channel_open_direct_streamlocal(path)` -> `Channel<Msg>` |
| Remote StreamLocal request | `handle.streamlocal_forward(path)` |
| Remote StreamLocal cancel | `handle.cancel_streamlocal_forward(path)` |
| Forwarded StreamLocal callback | client handler callback for forwarded StreamLocal channels |
| Bidirectional copy | `Channel::split()`, `ChannelReadHalf::make_reader()`, and `tokio` I/O copy helpers |
| Server TCP auth | `server::Handler::tcpip_forward`, `channel_open_direct_tcpip`, `channel_open_forwarded_tcpip` |
| Server StreamLocal auth | StreamLocal forwarding and direct StreamLocal handler callbacks exposed by `russh` |

## Edge cases

- Binding to TCP port `0` requires reporting the actual bound port via
  `TcpListener::local_addr()`.
- Remote TCP forwarding can succeed globally but individual forwarded
  connections can fail.
- Remote forwarding cancellation can fail if the server already removed the
  listener.
- One stream direction can EOF before the other. The copy loop handles
  half-close where the lower-level channel supports it.
- Shutdown can race with new accepted connections. Accept loops check the
  shutdown signal.
- Backpressure is managed by `tokio` copy helpers and `russh` window
  management.
- StreamLocal bind paths can already exist. The API returns a bind error
  instead of unlinking paths implicitly.
- StreamLocal support differs by platform and by remote server.
- The `tunnel` feature requires both `client` and `server` features.

## Testing Plan

- Unit tests for `TunnelBuilder`, `DirectTcpBuilder`,
  `DirectStreamLocalBuilder`, `ForwardSpec`, and debug redaction.
- Integration tests for local TCP forwarding (connect -> forward -> verify
  data).
- Integration tests for remote TCP forwarding (bind -> forward -> verify data).
- Integration tests for direct TCP (open channel -> send/receive data).
- Unix integration tests for local StreamLocal forwarding.
- Unix integration tests for direct StreamLocal channels.
- Remote StreamLocal integration tests where the current `russh` server hooks
  can model the server side reliably.
- Integration tests for `close()` and `abort()` lifecycle, including
  cancellation registry cleanup.
- Feature-gating checks for `tunnel`.

## Alternatives considered

- Expose only raw forwarding channels: rejected because it leaves lifecycle
  management to applications.
- SOCKS-style dynamic forwarding: deferred after direct TCP, StreamLocal, and
  basic local/remote forwarding are hardened.
- Implicitly unlink existing StreamLocal bind paths: rejected because deleting a
  filesystem path is surprising and can remove a socket owned by another
  process.

## Out of scope

SFTP and shell channel behavior are covered by separate design documents.
Dynamic SOCKS forwarding is deferred.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are broadly specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Tunnel lifecycle and drop behavior are specified.
- [x] Remote forwarding cancellation behavior is specified.
- [x] StreamLocal support matrix is specified.
