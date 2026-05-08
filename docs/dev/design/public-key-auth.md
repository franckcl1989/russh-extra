# Public Key and Agent Authentication

Status: Implemented (first runtime slice)
Roadmap: `docs/dev/roadmap.md#client-api`

## Summary

`russh-extra` adds runtime support for SSH public key authentication (from
OpenSSH key files) and SSH agent authentication via `$SSH_AUTH_SOCK` on Unix
platforms.
The `Identity` type and `Credential::Identity` variant already exist; this
design defines their runtime behavior, error handling, and server-side
counterparts.

## Motivation

Password authentication alone is insufficient for production SSH workflows.
Users need public key authentication for automated deployments, CI/CD
pipelines, and interactive use. SSH agent support eliminates the need to
store unencrypted private keys in build environments.

Using `russh` directly requires calling `russh::keys::load_secret_key`,
constructing `PrivateKeyWithHashAlg`, and managing the agent stream lifecycle
manually. `russh-extra` should make this a one-line builder call.

## Accepted Decisions

- Public API shape: existing `Identity` enum variants (`KeyFile`, `PrivateKey`,
  `Agent`) gain constructors and runtime behavior. The client authenticate
  loop processes `Credential::Identity` entries.
- Error policy: key file not found or permission-denied returns
  `Error::Io`. Key format or decryption errors return
  `Error::Authentication` with `AuthenticationErrorKind::Unavailable`.
  Key rejection by the server returns `AuthenticationErrorKind::Rejected`.
  Agent communication errors return `AuthenticationErrorKind::Unavailable`.
- Cancellation and shutdown policy: authentication is bounded by
  `Timeouts::auth`. Dropping the connect future cancels in-flight
  authentication attempts.
- Feature flags: public key auth from `KeyFile` and `PrivateKey` requires
  `client`. Agent auth requires both `client` and `agent`.
- Credential ordering: `Credential::Identity` entries are tried alongside
  `Credential::Password` entries in builder insertion order. Each
  `Credential::Identity` containing `Identity::Agent` is expanded into all
  agent identities at attempt time.
- Escape hatches to `russh`: `Identity::load_openssh_file()` returns
  `Result<Identity>`. Users who need the raw `russh::keys::PrivateKey`
  can access it through `russh_extra::russh`.

## User-facing API

### Client: Public Key from File

```rust
let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .identity(Identity::load_openssh_file("~/.ssh/id_ed25519")?)
    .known_hosts(KnownHosts::load("~/.ssh/known_hosts")?)
    .build()
    .connect()
    .await?;
```

Encrypted key with passphrase:

```rust
let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .identity(
        Identity::load_openssh_file("~/.ssh/id_ed25519")?
            .with_passphrase(std::env::var("SSH_PASSPHRASE")?)
    )
    .build()
    .connect()
    .await?;
```

### Client: Agent Authentication

```rust
let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .agent()
    .known_hosts(KnownHosts::load("~/.ssh/known_hosts")?)
    .build()
    .connect()
    .await?;
```

Explicit agent socket paths are deferred; the first runtime slice uses
`$SSH_AUTH_SOCK`.

### Client: Mixed Credentials

```rust
let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    // Try the identity first.
    .identity(Identity::load_openssh_file("~/.ssh/id_ed25519")?)
    // Fall back to agent.
    .agent()
    // Fall back to password.
    .password(std::env::var("SSH_PASSWORD")?)
    .build()
    .connect()
    .await?;
```

### Server: Public Key Authentication Callback

```rust
let server = Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key(host_key)
    .public_key_auth(|ctx, public_key| async move {
        if authorized_keys_contains(ctx.username(), &public_key) {
            Ok(AuthDecision::accept())
        } else {
            Ok(AuthDecision::reject())
        }
    })
    .build()?;
```

### ServerHandler: Public Key Authentication

```rust
impl ServerHandler for App {
    async fn auth_publickey(
        &self,
        ctx: AuthContext,
        public_key: PublicKey,
    ) -> Result<AuthDecision> {
        // Look up authorized keys.
        Ok(AuthDecision::accept())
    }
}
```

## Behavior

### Key Loading (`Identity::load_openssh_file`)

- Expands `~` to the home directory.
- On Unix, validates file permissions: group- or world-accessible key files
  are rejected with `Error::InvalidConfig`.
- Calls `russh::keys::load_secret_key(path, passphrase)`.
- Passphrase-encrypted keys loaded without a passphrase return
  `Error::Authentication` with `AuthenticationErrorKind::Unavailable`.
- Supported key algorithms: Ed25519, ECDSA (p256/p384/p521), RSA.

### Client Authentication Loop

The `authenticate_configured()` function handles `Credential::Identity`:

1. For each `Credential::Identity(identity)` credential in order:
   - `Identity::KeyFile { path, passphrase }`: loads the key from disk
     via `russh::keys::load_secret_key`. Wraps it in
     `PrivateKeyWithHashAlg` (RSA keys use SHA-256; others use their
     native hash). Calls `handle.authenticate_publickey()`.
   - `Identity::PrivateKey { data, passphrase }`: decodes OpenSSH/PEM
     bytes. Calls `handle.authenticate_publickey()`.
   - `Identity::Agent`: connects to the SSH agent socket. Lists
     identities. Tries each identity via `authenticate_publickey_with()`.
     Empty agent returns `AuthenticationErrorKind::Unavailable`.
2. If authentication succeeds (`AuthResult::Success`), return `Ok(())`.
3. If rejected, continue to the next credential.
4. If all credentials exhausted, return `AuthenticationErrorKind::Exhausted`.
5. Load failures return `AuthenticationErrorKind::Unavailable` and continue
   to the next credential.

### Agent Runtime

- Default agent socket: `$SSH_AUTH_SOCK` environment variable on Unix.
- Non-Unix platforms return `AuthenticationErrorKind::Unavailable` for agent
  authentication in this slice.
- Connection is per-connect attempt (not persistent).
- All agent identities are attempted; the first accepted wins.
- Agent timeout uses `Timeouts::auth`.

### Server Public Key Auth

- `ServerBuilder::public_key_auth()` registers a callback.
- `ServerHandler::auth_publickey()` method.
- Server `key` config includes all configured host keys (public keys in
  fingerprint form accessible to handler).
- No authorized-keys file parsing in this slice; the callback is opaque.

## Security

- Private keys, passphrases, agent stream bytes are never logged or
  included in `Debug` output. The `Identity` type's `Debug` impl already
  redacts key bytes.
- Key files with group or world permissions are rejected on Unix.
- Agent communication uses the standard SSH agent protocol; secrets
  transit the local socket only.
- Public keys may appear in tracing fields for diagnostics.

## Mapping to `russh`

- `russh::keys::load_secret_key(path, password)` — key file loading.
- `russh::keys::key::PrivateKeyWithHashAlg::new(key, hash)` — key wrapper.
- `russh::client::Handle::authenticate_publickey(user, key)` — client auth.
- `russh::keys::agent::client::AgentClient<S>::connect(stream)` — agent connection.
- `russh::keys::agent::client::AgentClient<S>::request_identities()` — list identities.
- `russh::client::Handle::authenticate_publickey_with(user, signer)` — agent-backed auth.
- `russh::server::Handler::auth_publickey(user, public_key)` — server-side check.
- `russh::keys::ssh_key::PublicKey` — public key type for server handlers.

No third-party SSH, key, or agent crate is involved.

## Feature Flags and Compatibility

- `client` exposes `Identity::load_openssh_file()` and
  `Identity::load_openssh_pem()`. Public key auth in the client loop requires
  `client`.
- `agent` (depends on `client`) exposes `ClientBuilder::agent()` runtime
  behavior.
- `server` exposes `ServerBuilder::public_key_auth()`,
  `ServerHandler::auth_publickey()`, and the `PublicKey` type re-export.
- `russh-extra --no-default-features --features client,aws-lc-rs` compiles
  with public key auth.
- `russh-extra --no-default-features --features agent,aws-lc-rs` compiles
  with public key and agent auth.
- This change is backwards-compatible with the existing public API.

## Edge cases

- Agent socket does not exist or is unreadable.
- Agent returns zero identities.
- Encrypted key file with incorrect passphrase.
- RSA keys need SHA-256 hash algorithm selection.
- Key algorithm not supported by the server.
- Disk I/O errors during key file loading.
- Multiple `Credential::Identity` entries with the same key.
- Key file loading failure should not prevent trying the next credential.
- Agent identities may overlap with explicitly configured key files.

## Testing Plan

### Unit Tests

- `Identity::load_openssh_file()` validates file permissions.
- `Identity::load_openssh_file()` with passphrase.
- `Identity::Debug` redacts key bytes (already tested).
- `Identity::agent()` returns the correct variant.
- Credential ordering preserves key-file before agent ordering.

### Integration Tests (with loopback fixture)

- Client connects with an Ed25519 key file against server with public key auth.
- Client connects using an agent identity.
- Public key rejection returns `AuthenticationErrorKind::Rejected`.
- Encrypted key with wrong passphrase returns `AuthenticationErrorKind::Unavailable`.
- Key file not found returns `Error::Io`.
- Mixed credentials: key succeeds before password is tried.
- Mixed credentials: key fails, falls back to password.
- Server with no public key callback rejects all public key auth.

### Feature-gating Checks

- `--no-default-features --features client,aws-lc-rs`
- `--no-default-features --features agent,aws-lc-rs`

### Loopback Fixture Extensions

- `LoopbackServerConfig` gains `authorized_key()` to register a public key
  for a user.
- `LoopbackServer` generates a known Ed25519 key pair that tests can load.

### Negative Tests

- Non-existent key file path.
- Key file with group permissions (on Unix).
- Agent socket connection refused.
- Server rejects agent-requested identity.

## Alternatives considered

Defer agent support. This moves the feature further from production
readiness and duplicates the implementation work of the identity loop.

Load keys eagerly at builder time. This forces the caller to handle I/O
errors before `connect()` is called and duplicates path management.
Loading in the authentication loop keeps the builder lightweight and
errors contextual.

Provide a persistent agent connection. This adds lifecycle complexity
without clear benefit for the first slice. Each connect attempt opens
its own agent session.

## Open questions

- Deferrable: `authorized_keys` file parsing for server side.
- Deferrable: public key with certificate-based authentication.
- Deferrable: persistent agent connection across multiple connect calls.
- Deferrable: `SSH_AUTH_SOCK` override via builder method.

## Out of scope

Server-side `authorized_keys` file parsing. Certificate-based
authentication. `HostbasedAuthentication`. Multiple agent connections.
Persistent agent session reuse.

## Acceptance Checklist

- [x] User-facing API examples compile or are marked as illustrative.
- [x] Runtime behavior and error policy are fully specified.
- [x] Mapping to official `russh` APIs is explicit.
- [x] Security-sensitive data handling is specified.
- [x] Feature flags and no-default behavior are specified.
- [x] Tests required for implementation are listed.
- [x] Open questions are either resolved or marked deferrable.
