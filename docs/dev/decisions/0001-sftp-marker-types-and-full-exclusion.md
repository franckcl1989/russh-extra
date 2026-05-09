# ADR 0001: SFTP Marker Types and `full` Exclusion

Status: Superseded

Superseded by: [Native SFTP Layer](../design/native-sftp.md)

This ADR recorded the pre-runtime SFTP policy. It is retained for historical
context only. The current implementation exposes a native SFTP v3 client and
server handler, and `sftp` is included in `full`.

## Context

The `sftp` feature in `russh-extra` must provide SFTP functionality implemented
directly on `russh` subsystem channels. At the time of this decision, the
native SFTP protocol implementation is in Draft design phase. The `sftp` cargo
feature exists as a feature gate, but exposing it with no runtime behavior as
stable public API would violate the project's honesty constraint.

## Decision

1. The `sftp` feature exposes only reserved experimental marker types
   (`SftpClient`, `SftpError`, `SftpFile`, and related types).
2. All marker types are behind the `sftp` feature gate, which is **not**
   included in the `default` feature set.
3. The `sftp` feature is **excluded** from the `full` feature set.
4. Runtime entry points on marker types return a typed unsupported error
   (`SftpError::Unsupported`) rather than panicking.
5. Crate docs, README, and feature docs explicitly state that SFTP runtime
   support is not yet implemented.

## Rationale

- **Honesty**: Claiming SFTP support without implementation would mislead users
  and violate the project charter.
- **Forward compatibility**: Reserving the type names and API shape behind a
  feature gate allows the public API to be designed and reviewed before
  runtime implementation, without blocking other features.
- **Safety**: Returning typed errors instead of panicking means users who
  accidentally enable `sftp` get a clean error, not a crash.
- **`full` exclusion**: `full` means "all stable runtime functionality." Since
  SFTP has no runtime, it does not belong in `full`.

## Consequences

- The `sftp` feature cannot be enabled by default until a real runtime
  implementation exists.
- Users who read the API surface will see SFTP types but must check the docs
  to understand they are reserved.
- The `full` feature set accurately reflects what is implemented today.
- The SFTP runtime now exists, so this ADR no longer describes the active
  feature policy.

## Alternatives Considered

- **Remove the `sftp` feature entirely until implementation is ready**: This
  would prevent API design from being reviewed in parallel with other work.
  Keeping marker types allows the design doc to be concrete about what the
  public API will look like.
- **Include `sftp` in `full` with marker types**: This would make `full`
  misleading about what is actually functional.
- **Panic on marker type use**: Unacceptable for a library crate.
