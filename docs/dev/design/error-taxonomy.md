# Error Taxonomy

Status: Implemented
Roadmap: `docs/dev/roadmap.md#foundation`

## Summary

`russh-extra` uses a typed error taxonomy so users can distinguish local
configuration problems, SSH transport failures, authentication failures,
channel failures, command results, SFTP failures, forwarding failures,
timeouts, cancellation, and remote disconnects.

## Motivation

High-level SSH workflows fail for different reasons that require different
user responses. Retrying a TCP connection failure is different from accepting a
new host key, prompting for a password, handling a non-zero command exit, or
closing a failed SFTP handle. A single opaque error string would make the API
ergonomic only on the happy path.

## Accepted Decisions

- Public API shape: all crates use `russh_extra_core::Error` and
  `russh_extra_core::Result` unless a narrower feature-specific error type is
  explicitly documented.
- Error policy: user-actionable categories are represented by typed variants
  and public subcategory kind enums.
- Cancellation and shutdown policy: cancellation and timeout behavior are
  distinguishable when the caller can react differently.
- Feature flags: feature-specific variants compile without forcing unrelated
  runtime features.
- Escape hatches to `russh`: lower-level `russh` errors may be preserved as
  sources, but public APIs classify them when a stable category is known.
- Exact enum shape: `Error` has top-level variants for invalid configuration,
  transport, host key, authentication, channel, command exit, SFTP, forwarding,
  timeout, cancellation, disconnect, unsupported operation, local I/O, and
  unclassified lower-level SSH errors.
- Category details: transport, host-key, authentication, channel, SFTP,
  forwarding, timeout, cancellation, disconnect, and SSH variants carry a
  `CategoryError<K>` value. The `K` kind enum is public so user code can match
  on stable subcategories without parsing strings.
- Source preservation: constructors that receive lower-level source errors
  store them behind `Box<dyn std::error::Error + Send + Sync + 'static>`.
  Source errors are preserved for diagnostics but are not part of stable match
  behavior.

## User-facing API

Users match the top-level category first, then inspect the public kind enum
when they need a more specific decision:

```rust
match session.command("deploy").await {
    Ok(output) if output.success() => {}
    Ok(output) => eprintln!("remote command failed: {:?}", output.exit),
    Err(russh_extra::Error::Authentication(error))
        if error.kind() == russh_extra::AuthenticationErrorKind::Rejected =>
    {
        prompt_for_credentials()
    }
    Err(russh_extra::Error::HostKey(error))
        if error.kind() == russh_extra::HostKeyErrorKind::Changed =>
    {
        review_host_key()
    }
    Err(error) if error.is_timeout() => retry_later(),
    Err(error) => return Err(error),
}
```

Feature-specific helpers expose narrower matching without string parsing:

```rust
if let russh_extra::Error::Sftp(error) = error {
    eprintln!("remote SFTP failure: {:?}", error.kind());
}
```

## Behavior

The base taxonomy is:

- `InvalidConfig(Cow<'static, str>)` for local builder, parser, and
  incompatible option failures.
- `Transport(TransportError)` for DNS, TCP connect, SSH negotiation, keepalive,
  encryption, I/O, and other transport failures.
- `HostKey(HostKeyError)` for unknown, changed, rejected, unsupported, or
  unavailable host keys.
- `Authentication(AuthenticationError)` for rejected credentials, exhausted
  credentials, partial authentication, unsupported auth methods, and unavailable
  authentication mechanisms.
- `Channel(ChannelError)` for channel open, request, read, write, EOF, close,
  and protocol-ordering failures.
- `CommandExit { exit: CommandExit }` for convenience APIs that choose to treat
  non-success remote command exits as errors.
- `Sftp(SftpError)` for remote status responses, malformed packets,
  unsupported versions, unsupported extensions, request ID mismatches, local
  I/O while handling SFTP, and remote disconnect during SFTP work.
- `Forwarding(ForwardingError)` for bind, listen, accept, connect, global
  request, channel open, stream copy, cancel, and shutdown failures.
- `Timeout(TimeoutError)` for configured operation timeouts.
- `Cancelled(CancelledError)` for caller-driven or shutdown-driven
  cancellation.
- `Disconnected(DisconnectedError)` for remote disconnects that are not better
  classified.
- `Unsupported(Cow<'static, str>)` for intentionally unsupported or not-yet
  implemented operations.
- `Io(std::io::Error)` for local I/O failures that are not better classified.
- `Ssh(SshError)` for lower-level `russh` errors that cannot be classified
  into a more stable public category.

The accepted kind enums are:

- `TransportErrorKind`: `Dns`, `TcpConnect`, `Negotiation`, `Keepalive`,
  `Encryption`, `Io`, `Other`.
- `HostKeyErrorKind`: `Unknown`, `Changed`, `Rejected`, `Unsupported`,
  `Unavailable`.
- `AuthenticationErrorKind`: `Rejected`, `Exhausted`, `Partial`,
  `UnsupportedMethod`, `Unavailable`.
- `ChannelErrorKind`: `Open`, `Request`, `Read`, `Write`, `Eof`, `Close`,
  `Protocol`.
- `SftpErrorKind`: `Status`, `MalformedPacket`, `UnsupportedVersion`,
  `UnsupportedExtension`, `RequestIdMismatch`, `LocalIo`, `RemoteDisconnect`.
- `ForwardingErrorKind`: `Bind`, `Listen`, `Accept`, `Connect`,
  `GlobalRequest`, `ChannelOpen`, `StreamCopy`, `Cancel`, `Shutdown`.
- `Operation`: `Connect`, `Authentication`, `ChannelOpen`, `Command`, `Shell`,
  `Sftp`, `Forwarding`, `Server`, `Shutdown`, `Other`.
- `SshErrorKind`: `Russh`, `Other`.

Remote command non-zero exits do not automatically become `Err` for the base
buffered command API. That API returns `CommandOutput` so callers keep stdout,
stderr, and exit details.

`Error::is_timeout()` returns true for `Error::Timeout(_)` and local I/O errors
with `std::io::ErrorKind::TimedOut`. `Error::is_cancelled()` and
`Error::is_disconnected()` match only their top-level categories.

## Security

Errors and their `Debug` output must not expose passwords, passphrases, private
key material, command stdin, or full command output. Host-key errors may expose
algorithm, fingerprint, endpoint, and policy names, but should not log private
material.

`CategoryError<K>` debug output includes the kind, message, and whether a source
exists. It does not print the source error's debug representation. Secret
handling still depends on callers not putting secrets into public error
messages.

## Mapping to `russh`

The taxonomy wraps and classifies errors from official `russh` APIs. The
implementation preserves source errors when useful while mapping them to stable
`russh-extra` categories.

If `russh` exposes an unstable or overly broad error type for a case users need
to handle, `russh-extra` adds a local classification layer over public `russh`
results.

## Feature Flags and Compatibility

`russh-extra-core` owns the public error type. It compiles without enabling the
`client`, `server`, `sftp`, `shell`, or `tunnel` features.

Adding variants before the first stable release is allowed by
`docs/dev/release.md`. After a stable release, compatibility rules must be
documented before changing match behavior.

## Edge cases

- One failure may cross layers, such as a disconnect while an SFTP request is
  in flight.
- Authentication can partially succeed before failing.
- A timeout can race with a remote disconnect.
- A command can fail remotely while stdout and stderr contain useful output.
- SFTP status codes can be protocol-version-specific.
- Forwarding failures can occur before or after a listener is advertised.

## Testing Plan

- Unit tests for constructors, display text, source preservation, and redaction.
- Unit tests for `is_timeout()`, `is_cancelled()`, and `is_disconnected()`.
- Unit tests for `Password`, private-key identity, and category-error debug
  redaction.
- User-level smoke tests that match public error variants and subcategory
  kinds.
- Integration tests that assert typed categories for host-key rejection,
  authentication failure, channel request rejection, remote disconnect, SFTP
  status response, and forwarding bind failure once those runtimes exist.
- Feature-gating checks with `--no-default-features`.
- Negative tests that ensure secret values do not appear in `Debug`.

## Alternatives considered

Use one opaque error enum with string payloads. This keeps the first
implementation small, but it prevents user code from handling SSH failure modes
cleanly.

Expose raw `russh` errors only. This leaks lower-level implementation details
and fails to classify errors from local configuration, SFTP packet handling, and
high-level forwarding helpers.

## Open questions

- Deferrable: stable compatibility policy for error variants after 1.0. The
  current pre-release rule lives in `docs/dev/release.md`.
- Deferrable: runtime-specific classifiers for exact `russh` error values.
  These should be added while implementing the runtime features that observe
  those values.

## Out of scope

This design does not define every feature's runtime behavior. Feature-specific
design docs own the cases they create and should map them into this taxonomy.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Open questions are either resolved or marked deferrable.
- [x] Exact enum shape is specified.
- [x] Source preservation policy is specified.
- [x] Redaction test matrix is specified.
