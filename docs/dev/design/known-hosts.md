# Known Hosts

Status: Implemented (first runtime slice)
Roadmap: `docs/dev/roadmap.md#known-hosts`

## Summary

`russh-extra` provides a known-hosts file parser, an in-memory store, and
`HostKeyPolicy` integration so users can verify SSH host keys against a
persistent registry without implementing the `check_server_key` logic
themselves.

This design covers the first runtime slice: file reading, in-memory lookup,
raw public-key comparison, trust-on-first-use in memory, file save, revoked-key
rejection, and changed-host-key detection. Hashed hostnames and wildcard
matching are deferred.

## Motivation

Using `russh` directly leaves host-key verification to the
`client::Handler::check_server_key` callback. Every application must either
accept all keys (insecure), pin fingerprints (brittle at scale), or reimplement
OpenSSH `known_hosts` parsing, hostname matching, changed-key detection, and
accept-new logic.

`russh-extra` should provide a built-in known-hosts layer so the user writes
`KnownHosts::load("~/.ssh/known_hosts")` and gets correct, safe behavior.

## Accepted Decisions

- Public API shape: `KnownHosts` is a loadable store living in the `russh-extra`
  crate behind the `known-hosts` feature. `ClientBuilder` gains `known_hosts()`
  and `known_hosts_accept_new()` methods that store a `KnownHosts` handle.
  `ClientHandler` holds an `Option<KnownHosts>` and consults it in
  `check_server_key` before falling back to the configured `HostKeyPolicy`.
  `HostKeyPolicy` enum in `russh-extra-core` is NOT modified.
- Integration model: the `ClientHandler` composes known-hosts and host-key policy.
  When `known_hosts()` is set, unknown keys are rejected (like `Strict`) unless
  `known_hosts_accept_new()` is used (trust-on-first-use). When `known_hosts()`
  is not set, `HostKeyPolicy` behaves as before.
- Error policy: file read failures use `Error::Io`. Parse failures for individual
  lines are collected in `KnownHosts::warnings()` and do not prevent loading
  valid entries. Verification failures use `Error::HostKey` with existing
  `HostKeyErrorKind::Unknown`, `Changed`, and `Rejected`.
- Cancellation and shutdown policy: known-hosts loading and saving are
  synchronous. Verification is synchronous in `check_server_key`.
- Feature flags: `known-hosts` depends on `_russh` for `PublicKey` types.
  `ClientBuilder::known_hosts()` requires both `client` and `known-hosts`.
- Concurrency model: `KnownHosts` wraps `Arc<std::sync::RwLock<KnownHostsInner>>`
  providing `Clone` and thread-safe reads. File writes acquire the write lock.
- Escape hatches to `russh`: `KnownHosts::check()` and `KnownHosts::add_entry()`
  accept `&russh::keys::ssh_key::PublicKey`. `KnownHostsEntry::key_blob()`
  returns raw public key bytes for custom serialization.

## User-facing API

Load known hosts from the default OpenSSH file:

```rust
let known_hosts = russh_extra::KnownHosts::load("~/.ssh/known_hosts")?;

let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .password(std::env::var("SSH_PASSWORD")?)
    .known_hosts(known_hosts)
    .build()
    .connect()
    .await?;
```

Trust on first use adds new entries to the in-memory store:

```rust
let known_hosts = russh_extra::KnownHosts::new();

let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .known_hosts_accept_new(known_hosts.clone())
    .build()
    .connect()
    .await?;

known_hosts.save("~/.ssh/known_hosts")?;
```

Inspect the store programmatically:

```rust
let known_hosts = russh_extra::KnownHosts::load("~/.ssh/known_hosts")?;

for warning in known_hosts.warnings() {
    eprintln!("line {}: {}", warning.line, warning.reason);
}

println!("loaded {} entries", known_hosts.entry_count());
```

Manual lookup against a received host key:

```rust
match known_hosts.check("example.com", 22, &server_public_key) {
    russh_extra::KnownHostStatus::Match => {}
    russh_extra::KnownHostStatus::NotFound => {}
    russh_extra::KnownHostStatus::Changed => {}
}
```

## Behavior

### File Format

The first slice parses OpenSSH `known_hosts` files with:

- Plain hostname entries (`example.com`). Comma-separated hostname lists are
  accepted, but only the first pattern is matched in this slice.
- `[host]:port` non-standard port markers
- Key type markers (`ssh-rsa`, `ssh-ed25519`, `ecdsa-sha2-nistp256`, etc.)
- Base64-encoded public key blobs
- Optional comments
- `@revoked` marker support
- Blank lines and `#` comments are ignored

Hashed hostname entries (`|1|salt|hash`) and `@cert-authority` entries produce
parse warnings and are skipped. Non-standard or malformed lines produce parse
warnings collected into `KnownHosts::warnings()`. The store skips malformed
lines but does not fail entirely on a single bad line.

### Hostname Matching

When checking a host key, the store matches entries against:

1. The exact hostname (e.g. `example.com`)
2. The `[host]:port` form when a non-default port is used
3. IP address entries when the connection is to an IP address

Port matching: OpenSSH `known_hosts` uses `[host]:port` notation for
non-standard ports. Standard port (22) entries match without a port marker.
The `KnownHosts` store follows this convention.

### Key Matching

The store compares received public keys against stored entries by full public
key binary comparison. A host entry with a different key is treated as changed.

### Policy Integration

`HostKeyPolicy` is not modified in this slice. `ClientHandler` composes an
optional `KnownHosts` store with the configured `HostKeyPolicy`:

- `ClientBuilder::known_hosts(store)` rejects unknown, changed, and revoked keys.
- `ClientBuilder::known_hosts_accept_new(store)` accepts unknown keys and adds
  them to the store; changed and revoked keys are rejected.
- Without a `KnownHosts` store, `HostKeyPolicy::Strict`,
  `HostKeyPolicy::PinnedSha256`, and `HostKeyPolicy::InsecureAcceptAny`
  behave as defined by the client session API.

When trust-on-first-use accepts a new key, the store is updated in memory.
Users call `known_hosts.save()` to persist. Automatic save on drop is not
provided because synchronous file I/O in `Drop` is error-prone.

### File Writing

New entries are written in OpenSSH format:

```text
example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA...
```

Hashed hostname writing is not implemented. `KnownHosts::set_hash_hostnames()`
records the preference for a future slice but current saves write plain
hostnames.

The store removes an existing matching host/port entry when `add_entry()` adds
a new key for that host. It does not perform full-file deduplication.

### Revoked Keys

Entries marked with `@revoked` are rejected. Certificate authority entries
marked with `@cert-authority` are skipped with a
parse warning in the first slice and deferred to a future certificate-based
authentication design.

## Security

### File Permissions

On Unix platforms, `KnownHosts::load()` checks file permissions. Files that are
group-writable or world-writable are rejected with `Error::InvalidConfig`.
This follows OpenSSH behavior and prevents local tampering.

On non-Unix platforms, no permission check is performed and users are
responsible for file security.

### Hostname Hashing

Hashed hostname matching and writing are not implemented in the first slice.
Hashed entries are skipped with a warning so callers can decide whether the
file needs stricter handling.

### User Install

`KnownHosts::load()` and `KnownHosts::save()` accept `~` (user home directory)
expansion through the `HOME` environment variable.

### Paths in Tracing

Known-hosts file paths appear in tracing spans for diagnostics. Hashed hostname
entries do not reveal the plaintext hostname in logs.

## Mapping to `russh`

The known-hosts feature integrates with `russh` through:

- `client::Handler::check_server_key` — the `ClientHandler` calls
  `KnownHosts::check()` when a known-hosts store is configured on the builder.
- `russh::keys::ssh_key::PublicKey` — fingerprint computation and binary key
  comparison use the public key type from `russh-keys`.
- `russh::Error::KeyChanged` and `russh::Error::WrongServerSig` — used to map
  changed and rejected keys back into typed `russh-extra` host-key errors.

If `russh` does not expose a needed key serialization primitive, the design
documents the gap and uses a local adapter over public `russh-keys` APIs.

## Feature Flags and Compatibility

- `known-hosts` exposes `KnownHosts`, `KnownHostsEntry`, `KnownHostStatus`,
  `KnownHostsParseWarning`, and related parsing types.
- `known-hosts` depends on `_russh` for `PublicKey` types.
- `ClientBuilder::known_hosts()` and `ClientBuilder::known_hosts_accept_new()`
  are available when both `client` and `known-hosts` are enabled.
- `known-hosts` does not depend on `server`, `shell`, `sftp`, or `tunnel`.
- `russh-extra --no-default-features --features known-hosts,aws-lc-rs` must
  compile.
- `russh-extra --no-default-features --features known-hosts,client,aws-lc-rs`
  must compile and expose `ClientBuilder::known_hosts()`.

The `known-hosts` feature is included in the default feature set.

This design is implemented for the first runtime slice described in this
document. The project is still pre-1.0.

## Edge cases

- A hostname has multiple entries with different key algorithms.
- A hostname has one entry and the server offers a different algorithm.
- Hashed hostname entries are skipped with parse warnings.
- Port-marked entries (`[host]:2222`) only match connections to that port.
- A known-hosts file contains malformed lines alongside valid entries.
- A known-hosts file is readable but empty.
- A write to the known-hosts file fails due to permissions or disk space.
- `~` expansion fails when the home directory is unavailable.
- Concurrent access from multiple `Client` instances sharing one `KnownHosts`
  store.
- Known-hosts files with very large numbers of entries.

## Testing Plan

### Unit Tests

- Parse valid entries: plain hostname, `[host]:port`, comma-separated
  hostname line with first-pattern matching, IP address.
- Parse entries with different key algorithms (ssh-rsa, ssh-ed25519,
  ecdsa-sha2-nistp256).
- Parse `@revoked` entries and warn/skip `@cert-authority` entries.
- Parse files with blank lines, `#` comments, and trailing whitespace.
- Reject or skip malformed lines: missing key type, invalid base64, missing
  hostname, hashed entries, and corrupt hashed entries.
- Hostname matching: exact match, port-specific match, non-matching hostname.
- Key comparison: same key, different key (same algorithm), different
  algorithm.
- File permission checks on Unix: reject group/other accessible files.
- `~` expansion to the home directory.
- `Debug` output excludes sensitive path data.

### Integration Tests

- Client connect with `ClientBuilder::known_hosts()` against a loopback server:
  known key matches, unknown key is rejected.
- `ClientBuilder::known_hosts_accept_new()` accepts an unknown key and the
  entry is available for inspection after connect.
- Changed host key returns `HostKeyErrorKind::Changed`.
- `@revoked` entries return `HostKeyErrorKind::Rejected`.
- Load from and save to a temporary file.
- `KnownHosts::warnings()` returns collected warnings.

### Feature-gating Checks

- `--no-default-features --features known-hosts,aws-lc-rs`
- `--no-default-features --features known-hosts,client,aws-lc-rs`

### Security Tests

- `Debug` of `KnownHostsEntry` does not expose private key material.
- Known-hosts files with world-writable permissions are rejected.
- Hashed entries are skipped with warnings and are not silently trusted.
- `@revoked` entries correctly reject matching keys.

## Alternatives considered

Defer known-hosts entirely and require users to call `check_server_key`
manually. This preserves flexibility but misses the most common SSH safety
behavior. Almost every real client needs known-hosts verification.

Use the `ssh-known-hosts` community crate. This violates the project
constraint against third-party SSH helper crates. The OpenSSH known-hosts
format is well-specified and a parser is straightforward to implement.

Only support `HostKeyPolicy::Strict` + `PinnedSha256`. This is safe but
impractical for users connecting to many hosts or hosts with rotating keys.

Support only plain (non-hashed) hostnames in the first runtime slice. This is
the current implementation because hashed matching needs additional parser and
test coverage before it should affect host-key security decisions.

## Open questions

- Deferrable: wildcard hostname matching (`*.example.com`).
- Deferrable: `@cert-authority` certificate validation.
- Deferrable: automatic known-hosts file write on drop.
- Deferrable: deduplication of entries on save.
- Deferrable: `HashKnownHosts` re-hashing of plain entries on load.
- Deferrable: global known-hosts (`/etc/ssh/ssh_known_hosts`).

## Out of scope

Certificate-based host authentication, server-side authorized-keys file
management, SSHFP DNS record verification, and global known-hosts
(`/etc/ssh/ssh_known_hosts`) are separate concerns.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] `HostKeyPolicy` variant naming is decided (no changes to core enum).
- [x] Crate placement for `KnownHosts` type is decided (`russh-extra`).
- [x] Concurrency model is specified (`Arc<RwLock<KnownHostsInner>`).
