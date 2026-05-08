# russh-extra

High-level async SSH APIs for Rust, built directly on top of
[`russh`](https://docs.rs/russh).

`russh-extra` provides ergonomic client, server, authentication, known-hosts,
command execution, shell, subsystem, and TCP forwarding APIs without requiring
application code to manage low-level `russh` handlers and channel messages for
common workflows.

This crate is not an official russh project.

## Quick Start

Add `russh-extra` to your `Cargo.toml`:

```toml
[dependencies]
russh-extra = { version = "0.1", default-features = false, features = ["client", "known-hosts", "aws-lc-rs"] }
```

Connect to an SSH server and run a command:

```rust
use russh_extra::Client;

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    let session = Client::builder()
        .endpoint(("example.com", 22))
        .username("deploy")
        .password(std::env::var("SSH_PASSWORD")?)
        .try_pinned_host_key_sha256("SHA256:base64-fingerprint")?
        .build()
        .connect()
        .await?;

    let output = session.command("uname -a").await?;
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
```

For tests and controlled environments, explicit host-key opt-out is available
via `HostKeyPolicy::InsecureAcceptAny`:

```rust
let session = Client::builder()
    .endpoint(("127.0.0.1", 2222))
    .username("test")
    .password("test")
    .accept_any_host_key() // insecure: only for tests
    .build()
    .connect()
    .await?;
```

Advanced users can access the underlying `russh` client handle:

```rust
let mut raw = session.russh_handle().await?;
let mut channel = raw.channel_open_session().await?;
channel.exec(true, "some raw command").await?;
```

## Authentication

Credentials are attempted in the order configured by the builder. Passwords,
passphrases, and private key bytes are redacted from `Debug` output.

### Password Authentication

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

### Private Key Authentication

```rust
let known_hosts = russh_extra::KnownHosts::load("~/.ssh/known_hosts")?;

let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .identity(russh_extra::Identity::load_openssh_file("~/.ssh/id_ed25519")?)
    .known_hosts(known_hosts)
    .build()
    .connect()
    .await?;
```

### Multiple Methods

```rust
let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .identity(russh_extra::Identity::load_openssh_file("~/.ssh/id_ed25519")?)
    .agent()
    .password(std::env::var("SSH_PASSWORD").unwrap_or_default())
    .try_pinned_host_key_sha256("SHA256:base64-fingerprint")?
    .build()
    .connect()
    .await?;
```

`agent()` uses `$SSH_AUTH_SOCK` on Unix platforms when the `agent` feature is
enabled. On platforms without Unix-domain agent sockets it returns
`AuthenticationErrorKind::Unavailable`.

## Known Hosts and Host Key Verification

Host-key verification defaults to strict rejection. Unknown host keys are
rejected unless the caller configures a pinned SHA256 fingerprint, a
known-hosts store, trust-on-first-use, or the explicit insecure accept-any
policy.

### Known Hosts File

```rust
let known_hosts = russh_extra::KnownHosts::load("~/.ssh/known_hosts")?;

let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .known_hosts(known_hosts)
    .build()
    .connect()
    .await?;
```

### Trust on First Use

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

Trust-on-first-use accepts an unknown key and adds it to the in-memory store.
Changed keys are rejected. Call `KnownHosts::save()` explicitly to persist the
store.

Hashed known-hosts entries are currently skipped with parse warnings.
`@revoked` entries reject matching host keys.

## Command Execution

`Session::command()` returns bytes and exit metadata, not only a string:

```rust
let output = session.command("deploy").await?;

if output.success() {
    println!("{}", String::from_utf8_lossy(&output.stdout));
} else {
    eprintln!("exit: {:?}", output.exit);
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
}
```

Buffered stdout and stderr have configurable per-command limits.

## Shells and Subsystems

Enable the `shell` feature for interactive shells, PTY allocation, resize, and
generic subsystem channels:

```rust
let mut shell = session
    .shell()
    .pty(russh_extra::Pty::new("xterm-256color", 120, 40))
    .env("LANG", "C.UTF-8")
    .build()
    .open()
    .await?;

shell.write_all(b"echo ready\n").await?;
let mut buf = [0; 4096];
let n = shell.read(&mut buf).await?;
println!("{}", String::from_utf8_lossy(&buf[..n]));
shell.resize(80, 24).await?;
shell.close().await?;
```

Subsystem channels use the same streaming handle:

```rust
let mut subsystem = session.subsystem("sftp").build().open().await?;
subsystem.write_all(b"payload").await?;
subsystem.close().await?;
```

This is a generic subsystem channel. The high-level SFTP protocol layer is not
implemented yet.

## Port Forwarding

Enable the `tunnel` feature for local TCP forwarding, remote TCP forwarding,
and one-shot direct TCP channels. Streamlocal and dynamic SOCKS-style forwarding
are deferred.

### Local Forwarding

```rust
let tunnel = session
    .tunnel(russh_extra::ForwardSpec::local_tcp(
        ("127.0.0.1", 8080),
        ("10.0.0.10", 80),
    ))
    .start()
    .await?;

println!("bound: {}", tunnel.bound_addr());
tunnel.close().await?;
```

### Direct TCP

```rust
let mut stream = session
    .direct_tcp(("db.internal", 5432))
    .open()
    .await?;

stream.write_all(b"ping").await?;
stream.close().await?;
```

### Remote Forwarding

```rust
let tunnel = session
    .tunnel(russh_extra::ForwardSpec::remote_tcp(
        ("127.0.0.1", 0),
        ("127.0.0.1", 3000),
    ))
    .start()
    .await?;

println!("remote port: {}", tunnel.bound_addr().port());
tunnel.close().await?;
```

## Server

Servers authenticate users, route commands, and manage shutdown explicitly.

```rust
let host_key = russh_extra::ServerHostKey::from_private_key(
    russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )?,
);

let server = russh_extra::Server::builder()
    .listen(("127.0.0.1", 2222))
    .host_key(host_key)
    .password_auth(|ctx, password| async move {
        if ctx.username().as_str() == "admin" && password.expose_secret() == "secret" {
            Ok(russh_extra::AuthDecision::accept())
        } else {
            Ok(russh_extra::AuthDecision::reject())
        }
    })
    .exec("whoami", |ctx| async move {
        Ok(russh_extra::ExecResponse::success()
            .stdout(format!("{}\n", ctx.username()))
            .exit_status(0))
    })
    .build()?;

server.run_until(shutdown_signal()).await?;
```

The server API also supports public-key authentication, keyboard-interactive
authentication, streaming exec handlers, shell/PTY/subsystem hooks,
environment-variable propagation, forwarding authorization hooks, lifecycle
hooks, and graceful shutdown handles.

## SFTP Status

The `sftp` feature is reserved for the native SFTP runtime. It currently exposes
experimental marker types, and `Session::sftp()` returns `Error::Unsupported`.
A native SFTP packet layer over `russh` subsystem channels is planned, but no
high-level SFTP file operations are implemented. The feature is excluded from
default features and `full` until real runtime behavior exists.

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `client` | yes | Client connect, authentication, command execution, and session APIs |
| `known-hosts` | yes | Known-hosts parser, in-memory store, and client integration |
| `aws-lc-rs` | yes | `russh` crypto backend via aws-lc-rs |
| `server` | no | Server listener, auth callbacks, exec routing, lifecycle hooks |
| `shell` | no | Interactive shell, PTY, resize, and subsystem channels |
| `tunnel` | no | Local/remote TCP forwarding and direct TCP channels |
| `agent` | no | SSH agent authentication using `$SSH_AUTH_SOCK` on Unix |
| `sftp` | no | Reserved experimental SFTP marker types; runtime not implemented |
| `ring` | no | Alternative `russh` crypto backend via ring |
| `flate2` | no | SSH compression support from `russh` |
| `rsa` | no | RSA key algorithm support from `russh` |
| `serde` | no | Serde serialization for config types |
| `full` | no | Enables stable runtime features; excludes reserved SFTP markers |

Feature-gate checks:

```bash
cargo check -p russh-extra --no-default-features
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
cargo check -p russh-extra --no-default-features --features sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
cargo check -p russh-extra --no-default-features --features tunnel,aws-lc-rs
cargo check -p russh-extra --no-default-features --features client,ring
```

## Error Handling

`russh-extra` uses typed errors so callers can distinguish transport,
authentication, host-key, channel, command, forwarding, timeout, and
unsupported-operation failures:

```rust
match session.command("deploy").await {
    Ok(output) if output.success() => println!("deploy ok"),
    Ok(output) => eprintln!("exit: {:?}", output.exit),
    Err(russh_extra::Error::Authentication(error))
        if error.kind() == russh_extra::AuthenticationErrorKind::Rejected =>
    {
        eprintln!("bad credentials");
    }
    Err(russh_extra::Error::HostKey(error))
        if error.kind() == russh_extra::HostKeyErrorKind::Changed =>
    {
        eprintln!("host key changed");
    }
    Err(error) if error.is_timeout() => eprintln!("timed out"),
    Err(error) => eprintln!("SSH error: {error}"),
}
```

## Tracing

`russh-extra` uses the `tracing` facade for connection, authentication,
channel, command, server, shell, and forwarding lifecycle events. Secrets,
private keys, passphrases, command stdin, and stream payloads are not logged.

```rust
use tracing_subscriber::EnvFilter;

tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .init();
```

Set `RUST_LOG=russh_extra=debug` to see lifecycle events.

## Security Policy

Host-key checking defaults to strict rejection. `accept_any_host_key()` uses
`HostKeyPolicy::InsecureAcceptAny`, an explicit unsafe opt-out for tests and
controlled environments.

Passwords, passphrases, private key material, and command stdin are never
logged or exposed in `Debug` output. See [`SECURITY.md`](SECURITY.md) and
[`docs/dev/security.md`](docs/dev/security.md) for the full policy.

## Current Status

This repository is pre-1.0 and AI-driven.

Implemented and covered by local tests:

- Client connect with password authentication.
- Strict, pinned SHA256, and known-hosts host-key verification.
- Trust-on-first-use in the in-memory known-hosts store.
- Changed host-key rejection for known-hosts entries.
- Buffered `Session::command()` with stdout/stderr capture, stdin, limits, and exit metadata.
- Client private-key authentication and server public-key auth callbacks.
- Client and server keyboard-interactive authentication.
- Server listener, password auth, public-key auth, exact command routing, streaming exec, env propagation, lifecycle hooks, and shutdown.
- Interactive shell, PTY allocation, resize, and subsystem channel opening.
- Direct TCP channels and local TCP forwarding over `russh` channel primitives.
- Typed error taxonomy and local loopback test fixtures.

Implemented but still being hardened:

- Remote TCP forwarding runtime.
- Forwarding lifecycle edge cases and broader forwarding integration tests.
- Agent authentication against real user agents.
- Known-hosts save/deduplication workflows.

Not yet implemented:

- Native SFTP packet/runtime layer and high-level file operations.
- Hashed hostname known-hosts matching and writing.
- Streamlocal forwarding.
- Dynamic SOCKS-style forwarding.
- Split shell read/write halves and `AsyncRead`/`AsyncWrite` trait impls for high-level shell/tunnel handles.

## Workspace

| Crate | Purpose |
|---|---|
| `russh-extra` | User-facing high-level API |
| `russh-extra-core` | Shared SSH domain types and errors |
| `russh-extra-macros` | Future proc-macro entry points |
| `russh-extra-test-support` | Integration test helpers |
| `russh-extra-tests` | Workspace-level tests |

## MSRV

Minimum supported Rust version: **1.95**.

## Development

```bash
just check-all   # full verification suite (fmt, clippy, test, doc, feature checks)
just fix         # auto-format
just test        # run all tests
```

Or run commands directly:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

The repository is the source of truth for goals, constraints, design decisions,
and implementation status:

- `AGENTS.md` and `CLAUDE.md` define agent-facing commands and architecture.
- `docs/dev/project-charter.md` defines the project goal and operating model.
- `docs/dev/constraints.md` defines dependency, API, security, and testing constraints.
- `docs/dev/ai-workflow.md` contains reusable prompts and handoff rules.
- `docs/dev/testing.md` defines the local test strategy.
- `docs/dev/development-plan.md` defines phase gates and current work.
- `docs/dev/security.md` and `docs/dev/release.md` define security and compatibility rules.
- `docs/dev/roadmap.md` tracks accepted work.
- `docs/dev/design/` contains guide-level design docs for non-trivial public API changes.
- `.agents/skills/` and `.claude/skills/` contain local development skills.

## License

This project is licensed under the MIT license.
