# Native SFTP Layer

Status: Implemented
Roadmap: `docs/dev/roadmap.md#native-sftp`

## Summary

`russh-extra` implements SFTP directly on top of `russh` session channels and
the SSH `sftp` subsystem. The crate does not depend on community SFTP helper
crates.

The client SFTP runtime is implemented and stable behind the `sftp` feature.
The server SFTP handler trait (`SftpServerHandler`) is implemented behind
`features = ["sftp", "server"]`. Both are included in the `full` feature set.

## Motivation

The project exists because the ecosystem lacks a complete, ergonomic high-level
API around `russh`. Pulling in a separate SFTP abstraction would make SFTP
behavior depend on another project instead of making it a coherent part of
`russh-extra`.

SFTP is the single largest missing feature. Users need file upload, download,
listing, and metadata operations without leaving the `russh-extra` API surface.

## Accepted Decisions

### Public API shape

Users open SFTP from a connected `Session`. The current marker type is replaced
with a fully functional `SftpClient`:

```rust
let session = client.connect().await?;
let sftp = session.sftp().await?;

// Low-level file operations.
let file = sftp.open("/etc/hostname", SftpOpenMode::Read).await?;
let data = file.read(0, 4096).await?;
file.close().await?;

let metadata = sftp.metadata("/var/log/app.log").await?;

// Convenience helpers.
sftp.write_all("/tmp/release.tar.gz", &release_bytes).await?;
let content = sftp.read_to_vec("/etc/hostname").await?;

// Streaming upload/download.
sftp.upload("./local.tar.gz", "/remote/target.tar.gz").await?;
sftp.download("/remote/data.bin", "./local.bin").await?;

// Directory operations.
let mut dir = sftp.opendir("/etc").await?;
while let Some(entry) = dir.read().await? {
    println!("{}", entry.filename());
}
dir.close().await?;

sftp.symlink("/var/log/app.log", "/tmp/app-link").await?;
let target = sftp.readlink("/tmp/app-link").await?;
```

The `SftpClient` is `Clone` and `Send + Sync` — it shares the underlying channel
I/O tasks via `Arc`.

### Channel I/O ownership model

SFTP runs over a single SSH session channel opened with the `"sftp"` subsystem
request. The channel is split into read and write halves:

1. Open session channel via `handle.channel_open_session()`.
2. Request `"sftp"` subsystem on the channel.
3. Exchange `FXP_INIT` / `FXP_VERSION` to negotiate protocol version.
4. Split `Channel<Msg>` into `(ChannelReadHalf, ChannelWriteHalf)`.
5. Spawn a background **read task** that reads raw bytes from the read half,
   reassembles SFTP packets (4-byte big-endian length prefix), and dispatches
   responses to waiting requestors via oneshot channels keyed by request ID.
6. Spawn a background **write task** that receives serialized SFTP requests
   from an mpsc channel and writes them to the channel write half.
7. Public API methods create a request struct, assign a unique request ID,
   register a oneshot sender in a pending map, send through the write mpsc,
   and await the oneshot receiver.

```
Session session ──open session channel──→ Channel<Msg>
  └─ request_subsystem("sftp")
       └──split──→ (ChannelReadHalf, ChannelWriteHalf)
                        │                    │
                   read task           write task
                   (packet reassembly)  (packet serialization)
                        │                    │
                   pending: HashMap        mpsc::Receiver
                   <u32, oneshot::Sender>      ▲
                        │                    │
                        ▼                    │
              dispatch response     public API methods
              by request ID         (open, read, write, ...)
```

### Request pipeline concurrency

Each SFTP request carries a unique `u32` request ID (monotonic, wrapping).
Responses carry the matching ID. Multiple requests may be in flight
simultaneously.

The implementation maintains a `HashMap<u32, oneshot::Sender<SftpResponse>>` for
pending requests behind a `tokio::sync::Mutex`. Public API methods insert a
sender, send the request through the write mpsc, and await the receiver.

Default max concurrent requests is 64. The `open()` method waits for the
`FXP_HANDLE` response before returning the `SftpFile` handle. Reads and writes
on that handle may proceed concurrently with other requests.

### Protocol version and extensions

Target SFTP protocol version **3** (draft-ietf-secsh-filexfer-02) as the
baseline. Version 3 is universally supported by OpenSSH, Dropbear, and
enterprise SSH servers.

Extension negotiation flow:

1. Client sends `FXP_INIT { version: 3 }`.
2. Server responds with `FXP_VERSION { version, extensions }`.
3. If server version < 3, fail with `Error::Sftp(SftpErrorKind::UnsupportedVersion)`.
4. If server version >= 3, operate in v3 mode. Server-supported extensions
   (`extensions`) are recorded for future use but not used in the first runtime
   slice.

### Error policy

SFTP errors flow through the existing `Error::Sftp(SftpErrorKind)` variant in
`russh-extra-core`. The `SftpErrorKind` enum distinguishes:

| Kind | Meaning |
|------|---------|
| `Protocol` | Malformed packet, unexpected type, length mismatch |
| `RemoteStatus` | Server returned `SSH_FX_*` status code (e.g. `SSH_FX_NO_SUCH_FILE`) |
| `ChannelIo` | SSH channel read/write failure |
| `UnexpectedResponse` | Response received for unknown request ID |
| `UnsupportedVersion` | Server does not support SFTP v3 |
| `Unsupported` | Feature not yet implemented |

The `Error::Sftp` variant carries a `SftpError` with a `kind()` accessor and an
optional `status_code: u32` field for `RemoteStatus` kinds. Server status codes
are mapped to readable names:

| Code | Name | Error message example |
|------|------|----------------------|
| 0 | SSH_FX_OK | (not an error) |
| 1 | SSH_FX_EOF | "end of file" |
| 2 | SSH_FX_NO_SUCH_FILE | "no such file: /path/to/file" |
| 3 | SSH_FX_PERMISSION_DENIED | "permission denied" |
| 4 | SSH_FX_FAILURE | "operation failed" |
| 5 | SSH_FX_BAD_MESSAGE | "bad message" |
| 6 | SSH_FX_NO_CONNECTION | "no connection" |
| 7 | SSH_FX_CONNECTION_LOST | "connection lost" |
| 8 | SSH_FX_OP_UNSUPPORTED | "operation unsupported" |

### Cancellation and shutdown

- Dropping `SftpClient` closes the write mpsc channel, which signals the write
  task to close the SSH channel. The read task terminates when the channel
  closes.
- Pending requests receive a cancellation error when the client is dropped.
- `SftpClient::close()` sends `FXP_CLOSE` on the SSH channel and shuts down
  both tasks gracefully.
- Individual `SftpFile` handles send `FXP_CLOSE` for their handle on drop
  (best-effort; the close is spawned as a fire-and-forget task so Drop
  does not block).

### Feature flags

- `sftp` depends on `client` (`sftp = ["client"]`).
- SFTP types require the `sftp` feature. `Session::sftp()` also requires
  `client` (always true when `sftp` is active).
- Server SFTP handler (`SftpServerHandler`) requires `features = ["sftp", "server"]`.
- `sftp` is included in `full`; both client and server runtimes are
  implemented and tested end-to-end.

### Escape hatches to russh

- SFTP runs over the same connected session model as the rest of the client API.
- `SftpClient::russh_channel()` (expert) exposes the underlying SSH channel for
  raw SFTP packet access.
- `SftpFile::russh_handle()` returns the SFTP file handle string for custom
  operations.

## User-facing API

### Open and negotiate

```rust
let sftp: SftpClient = session.sftp().await?;

// Versions and extensions that were negotiated.
let version: u32 = sftp.protocol_version();
let extensions: &HashMap<String, String> = sftp.extensions();
```

### File operations

```rust
let mut file = sftp.open("/home/deploy/config.toml", SftpOpenMode::Read).await?;
let chunk = file.read(0, 4096).await?;
// chunk is Vec<u8>; empty Vec signals EOF.
file.close().await?;

let file = sftp.open("/tmp/new.log", SftpOpenMode::Write | SftpOpenMode::Create | SftpOpenMode::Truncate).await?;
file.write(0, b"hello\n").await?;
file.close().await?;
```

`SftpOpenMode` is a bitflag type:

```rust
pub struct SftpOpenMode(u32);

impl SftpOpenMode {
    pub const Read: SftpOpenMode = SftpOpenMode(0x00000001);
    pub const Write: SftpOpenMode = SftpOpenMode(0x00000002);
    pub const Append: SftpOpenMode = SftpOpenMode(0x00000004);
    pub const Create: SftpOpenMode = SftpOpenMode(0x00000008);
    pub const Truncate: SftpOpenMode = SftpOpenMode(0x00000010);
    pub const Exclusive: SftpOpenMode = SftpOpenMode(0x00000020);
}
```

### Metadata

```rust
let meta = sftp.metadata("/var/log/app.log").await?;
// meta.size(), meta.permissions(), meta.accessed(), meta.modified()

sftp.set_metadata("/var/log/app.log", SftpFileAttrs::new().permissions(0o644)).await?;
```

### Directory listing

```rust
let mut dir = sftp.opendir("/etc").await?;
while let Some(entry) = dir.read().await? {
    println!("{}  ({})", entry.filename(), entry.longname());
    // entry.metadata() returns SftpMetadata.
}
dir.close().await?;
```

### Symlink operations

```rust
sftp.symlink("/var/log/app.log", "/tmp/app-link").await?;
let target = sftp.readlink("/tmp/app-link").await?;
let resolved = sftp.canonicalize("/tmp/app-link").await?; // realpath
```

### Rename, remove, mkdir, rmdir

```rust
sftp.rename("/tmp/old.txt", "/tmp/new.txt").await?;
sftp.remove("/tmp/junk.txt").await?;
sftp.create_dir("/tmp/new-dir", SftpFileAttrs::default()).await?;
sftp.remove_dir("/tmp/old-dir").await?;
```

### Convenience helpers

```rust
// Read entire file.
let content: Vec<u8> = sftp.read_to_vec("/etc/hostname").await?;

// Write entire file.
sftp.write_all("/tmp/data.bin", &bytes).await?;

// Streaming upload (reads local file in chunks).
sftp.upload("./local.tar.gz", "/remote/target.tar.gz").await?;

// Streaming download (writes local file in chunks).
sftp.download("/remote/data.bin", "./local.bin").await?;
```

## Behavior

### Happy path

1. `Session::sftp()` constructs an `SftpClient` builder from the session handle.
2. `SftpClient::open()` opens a session channel, requests `"sftp"` subsystem,
   splits the channel, and spawns I/O tasks.
3. Client sends `FXP_INIT { version: 3 }` and waits for `FXP_VERSION`.
4. Negotiation succeeds → `SftpClient` is ready. The `SftpClient` instance
   shares the underlying I/O tasks via `Arc`.
5. Each public API call (open, read, write, stat, etc.) creates a numbered
   request, sends it through the write mpsc, and awaits the oneshot response.
6. The write task serializes the request to an SFTP packet and writes it to
   the channel write half.
7. The read task reads raw bytes from the channel read half, buffers them,
   reassembles complete packets, and dispatches them by request ID.
8. Response arrives → public API returns the result.

### Error cases

| Scenario | Error |
|----------|-------|
| Server rejects `"sftp"` subsystem | `Error::Channel(ChannelErrorKind::Request)` |
| Server returns version < 3 | `Error::Sftp(SftpErrorKind::UnsupportedVersion)` |
| Malformed packet (truncated length, bad type byte) | `Error::Sftp(SftpErrorKind::Protocol)` |
| Server returns `SSH_FX_*` status | `Error::Sftp(SftpErrorKind::RemoteStatus)` with status code |
| Channel read error | `Error::Sftp(SftpErrorKind::ChannelIo)` |
| Response for unknown request ID | `Error::Sftp(SftpErrorKind::UnexpectedResponse)` |
| Write task mpsc closed (client dropped) | `Error::Sftp(SftpErrorKind::ChannelIo)` |
| Pending request timeout | `Error::Timeout` |

### Defaults

- SFTP protocol v3.
- Max concurrent requests: 64.
- Read chunk size for streaming helpers: 32 KiB.
- No operation-level timeout by default (uses session timeouts).
- `SftpFile` sends `FXP_CLOSE` on drop (fire-and-forget).

### Cancellation and shutdown

- `SftpClient::close()` sends a shutdown signal to the write task, which flushes
  and closes the SSH channel. The read task exits when the channel closes.
- Dropping `SftpClient` without `close()` triggers the same shutdown
  through the write mpsc drop.
- `SftpFile` handles: closing a file explicitly via `close()` awaits the
  `FXP_CLOSE` response. Dropping a file without `close()` spawns a
  fire-and-forget close.
- Pending requests when the client shuts down receive `Error::Cancelled`.

## Security

- SFTP methods read and write local files. APIs that create local files
  document their overwrite behavior.
- File paths appearing in SFTP request/response packets are not masked in
  tracing. Users sensitive to path logging should control log verbosity.
- Local file I/O follows OS permission semantics. Remote file permissions
  are set by the server.
- SFTP packet serialization does not use `unsafe`.
- `SftpClient` does not cache credentials or authentication state.
- Channel I/O does not log packet payloads at `INFO` level or above.

## Mapping to `russh`

| russh-extra concept | russh API |
|---|---|
| Open SFTP subsystem | `handle.channel_open_session()` → `Channel<Msg>` → `channel.request_subsystem(false, "sftp")` |
| Split channel | `channel.split()` → `(ChannelReadHalf, ChannelWriteHalf)` |
| Read raw bytes | `ChannelReadHalf::wait()` → `ChannelMsg::Data { data: Bytes }` |
| Write raw bytes | `Handle::data(id, bytes)` (via `ChannelWriteHalf`) |
| Close channel | `channel.close()` |
| Session handle access | `Session::russh_handle()` → `RusshHandleGuard` |

The SFTP layer does **not** require new `russh` APIs. Channel read/write
primitives exposed by `russh 0.60` are sufficient for SFTP packet I/O.

## Feature Flags and Compatibility

- `sftp` exposes `SftpClient`, `SftpFile`, `SftpDir`, `SftpMetadata`,
  `SftpDirEntry`, `SftpOpenMode`, `SftpFileAttrs`, and `SftpErrorKind`.
- `client,sftp` exposes `Session::sftp()`.
- `server,sftp` exposes `SftpServerHandler` trait and building integration.
- `russh-extra --no-default-features --features sftp,aws-lc-rs` compiles.
- `sftp` is included in `full`; both client and server runtimes are
  implemented and tested end-to-end.
- Breaking changes to SFTP types are allowed during pre-1.0 development.

## Edge cases

- **Short reads/writes**: The read task buffers partial packets. The write task
  writes complete SFTP packets in a single `data()` call. Large writes from
  the public API are split into chunks at the SFTP packet level.
- **Concurrent file handles**: Multiple `SftpFile` handles may be open
  simultaneously. Each gets a unique SFTP handle string from the server.
- **Server-closed handles**: If the server closes a handle unexpectedly, the
  next operation receives an error. `SftpFile::close()` on an already-closed
  handle is a no-op.
- **Large file offsets**: Offsets are `u64`.
- **Filename encoding**: Filenames are UTF-8 `String`s. Non-UTF-8 remote
  filenames produce `Error::Sftp(SftpErrorKind::Protocol)`.
- **Extension negotiation**: Server extensions are stored but not used in the
  first runtime slice. Unknown extensions do not cause errors.
- **Remote disconnect**: If the SSH connection drops, the read task terminates
  and pending awaiters receive `Error::Sftp(SftpErrorKind::ChannelIo)`.
- **Packet reassembly**: The read task maintains a buffer. If a received chunk
  ends mid-packet, the partial packet is held until more data arrives. Packets
  larger than 256 KiB are rejected as malformed.
- **Request ID wrapping**: `u32` request IDs wrap at `u32::MAX`. The pending
  map is cleared of old entries; wrapping is safe as long as fewer than
  `u32::MAX` requests are in flight simultaneously (guaranteed by the
  concurrency limit of 64).

## Testing Plan

### Unit tests

- SFTP packet encoding and decoding: all v3 packet types.
- Length-prefix framing: complete packet, partial packet (buffering), oversized
  packet rejection.
- Request ID assignment and wrapping.
- `SftpOpenMode` bitflag behavior.
- `SftpFileAttrs` builder and serialization.
- Status code to `SftpErrorKind::RemoteStatus` mapping.
- Malformed packet rejection (truncated length, bad type byte, missing fields).

### Integration tests

- SFTP subsystem open and version negotiation against loopback server with
  a mock SFTP subsystem handler.
- File open/read/write/close round-trip through mock.
- Metadata retrieval (stat, lstat, fstat).
- Directory open/read/close through mock.
- Create/remove directory and file operations.
- Symlink and readlink through mock.
- Rename, remove, rmdir.
- Realpath / canonicalize.
- `read_to_vec` and `write_all` convenience methods.
- Streaming `upload` and `download` helpers.
- Concurrent requests (interleaved reads on multiple files).
- Handle close on `SftpFile` drop.
- `SftpClient::close()` shutdown.
- Server status error mapping (no such file, permission denied, etc.).
- Malformed server response handling.
- Remote disconnect during operation.
- Feature-gating checks for `sftp`.

### Feature-gating checks

- `--no-default-features --features sftp,aws-lc-rs`
- `--no-default-features --features client,sftp,aws-lc-rs`

## Alternatives considered

Use `russh-sftp` community crate. Rejected — adding another SSH/SFTP
abstraction violates the project constraint against third-party protocol
helpers. SFTP protocol framing is well-specified and implementable.

Delegate SFTP to a future, separate crate. Rejected — SFTP is fundamental to
the `russh-extra` value proposition. Users should not need an extra dependency
for basic file transfer.

Implement SFTP v5 or v6. Rejected for the first slice — v3 is universally
supported. Higher versions add extensions that can be negotiated later.

## Out of scope

- SCP (separate protocol, not SFTP).
- SFTP server-side handler (deferred until client SFTP is stable).
- Recursive directory sync helpers (deferred until basic file ops are stable).
- `fsetstat`, `fstat` operations with raw byte attributes (rarely used).
- Vendor-specific SFTP extensions (OpenSSH statvfs, posix-rename, etc.).

## Acceptance Checklist

- [x] User-facing API examples are illustrative and cover all major operations.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Target SFTP protocol version and extension policy are specified (v3, extensions recorded not used).
- [x] Channel read/write ownership model is specified (split channel, background tasks, mpsc+oneshot).
- [x] Request pipeline limits are specified (64 concurrent, u32 request IDs, HashMap+oneshot).
