# Security Policy

`russh-extra` is an SSH library. Security behavior is part of the public API
contract and must be specified before implementation.

## Default Security Posture

- Client host-key verification defaults to strict verification.
- Server authentication defaults to rejecting all users until an auth policy is
  configured.
- Passwords, passphrases, private keys, command stdin, shell stdin, and stream
  payloads are never logged.
- Secret-bearing types must redact `Debug`.
- APIs that create or overwrite local files must document their file behavior.
- Forwarding APIs must make bind and target addresses explicit.

## Threat Model

Designs and implementations should consider:

- Man-in-the-middle attacks during SSH connection setup.
- Host-key changes and unknown host keys.
- Credential disclosure through logs, errors, debug output, or serialization.
- Accidental exposure of local network services through forwarding.
- Unbounded memory growth from command output, shell output, SFTP transfers, or
  forwarded streams.
- Remote peers closing channels, disconnecting, or sending malformed protocol
  messages.
- Server handlers accepting authenticated users without authorization checks.

## Client Requirements

Client designs must specify:

- Host-key policy and default behavior.
- Whether trust-on-first-use is supported and how it is stored.
- How users provide pinned keys or verification callbacks.
- Credential attempt order.
- How authentication failures are classified.
- Which fields are allowed in tracing.

`host_key_policy(HostKeyPolicy::InsecureAcceptAny)` is allowed only as an explicit
opt-out. It must be visible in code and documented as unsafe for production
connections.

## Server Requirements

Server designs must specify:

- Host-key loading and in-memory key handling.
- Authentication policy defaults.
- Authorization hooks for commands, shells, subsystems, and forwarding.
- Limits for authentication attempts, sessions, channels, and idle time.
- Shutdown behavior for active connections and handlers.

Server examples should avoid open-by-default behavior.

## SFTP Requirements

SFTP designs must specify:

- Local file overwrite behavior.
- Local file permissions for created files.
- Whether paths are included in traces by default.
- How malformed packets, request ID mismatches, unsupported versions, and
  unsupported extensions are reported.
- How handles close on drop and on remote disconnect.

## Forwarding Requirements

Forwarding designs must specify:

- Bind address defaults and examples.
- Authorization inputs for server-side forwarding decisions.
- Close, abort, and remote cancellation behavior.
- Backpressure behavior during bidirectional stream copy.
- Platform differences for streamlocal forwarding.

## Security Testing

Tests should cover:

- Strict host-key checking defaults.
- Host-key rejection.
- Rejected authentication.
- Secret redaction in `Debug` and serialization.
- Server auth rejection by default.
- Forwarding bind behavior.
- Malformed SFTP packets.
- Output and transfer bounds where applicable.

