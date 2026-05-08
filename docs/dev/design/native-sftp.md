# Native SFTP Layer

Status: Draft
Roadmap: `docs/dev/roadmap.md#native-sftp`

## Summary

`russh-extra` will implement SFTP directly on top of `russh` session channels
and the SSH `sftp` subsystem. The current `sftp` feature exposes reserved
experimental marker types only; `Session::sftp()` returns `Error::Unsupported`.
The crate does not depend on community SFTP helper crates.

## Motivation

The project exists because the ecosystem lacks a complete, ergonomic high-level
API around `russh`. Pulling in a separate SFTP abstraction would make SFTP
behavior depend on another project instead of making it a coherent part of
`russh-extra`.

## Accepted Decisions

- Public API shape: users open SFTP from a connected `Session`.
- Error policy: SSH channel failures, SFTP status responses, malformed packets,
  unsupported protocol extensions, local I/O errors, and remote disconnects must
  be distinguishable.
- Feature flags: SFTP types require the `sftp` feature. `Session::sftp()` also
  requires `client`.
- Protocol ownership: packet encoding, request multiplexing, response decoding,
  extension negotiation, and high-level file operations live in this repository.
- Escape hatches to `russh`: SFTP runs over the same connected session model as
  the rest of the client API.

## User-facing API

Users open SFTP from a connected session. The example is illustrative until the
runtime implementation lands:

```rust
let session = client.connect().await?;
let sftp = session.sftp().await?;

sftp.download("/var/log/app.log", "./app.log").await?;
sftp.upload("./release.tar.gz", "/tmp/release.tar.gz").await?;
```

The full API should include file metadata, directory reads, recursive transfer
helpers, symlink operations, permissions, and stream-based reads and writes.

## Behavior

In the planned runtime, opening SFTP starts a `sftp` subsystem over a session
channel. Requests are framed, assigned request IDs, written to the subsystem
channel, and matched with responses. The client supports multiple in-flight
requests when the remote server allows it.

Handle-owning APIs must close remote handles when dropped or when explicit close
is requested. High-level upload and download helpers must preserve local I/O
errors separately from remote SFTP status errors.

## Security

SFTP methods can read and write local files. APIs that create local files should
document whether they overwrite existing files and which permissions they use.

Tracing must include request IDs and paths only when doing so is safe for users;
future logging policy should allow users to avoid path logging for sensitive
workloads.

## Mapping to `russh`

The SFTP layer uses `russh` session channels and subsystem requests. Packet
encoding, request multiplexing, response decoding, extension negotiation, and
high-level file operations live in this repository.

If current `russh` APIs do not expose a required channel primitive, the design
must document the gap and either add a `russh` upstream issue or provide a small
local adapter over public `russh` types.

## Feature Flags and Compatibility

- `sftp` exposes reserved experimental SFTP client and server marker types.
- `client,sftp` exposes `Session::sftp()`.
- `russh-extra --no-default-features --features sftp` must compile.
- `sftp` stays out of default features and `full` until runtime SFTP behavior is
  implemented and tested.

This design is still Draft. Public API compatibility is not guaranteed until
the design becomes Accepted.

## Edge cases

- Short reads and writes.
- Concurrent reads and writes on one file handle.
- Handles closed by the server.
- Large file offsets.
- Filename encoding.
- Extension negotiation.
- Servers that return protocol-version-specific status codes.
- Remote disconnect while requests are in flight.
- Local filesystem errors during upload or download.

## Testing Plan

- Unit tests for packet encoding, decoding, malformed packets, and boundary
  values.
- Integration tests with local loopback `russh` SFTP subsystem fixtures.
- Error-path tests for SFTP status responses, malformed responses, closed
  handles, unsupported extensions, local I/O failures, and remote disconnects.
- Feature-gating checks matching `.github/workflows/ci.yml`.

## Alternatives considered

Use a community SFTP crate. This was rejected because the project goal is a
coherent, complete high-level API built directly on `russh`.

## Open questions

- Blocking acceptance: target SFTP protocol version and extension policy.
- Blocking acceptance: channel read/write ownership model.
- Blocking implementation: request pipeline concurrency limits.
- Deferrable: high-level recursive sync helpers.

## Out of scope

SCP compatibility is separate from SFTP and should have its own design if the
project adds it.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and high-level error categories are specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive path and local file behavior is identified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [ ] Target SFTP protocol version and extension policy are specified.
- [ ] Channel read/write ownership model is specified.
- [ ] Request pipeline limits are specified.
