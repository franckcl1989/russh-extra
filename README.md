# russh-extra

High-level async SSH APIs for Rust, built directly on top of
[`russh`](https://docs.rs/russh).

`russh-extra` provides ergonomic client, server, authentication, known-hosts,
command execution, shell, subsystem, SFTP, and forwarding APIs without requiring
application code to manage low-level `russh` handlers and channel messages for
common workflows.

The 0.1 line targets the main high-level SSH workflows. It is not a complete
wrapper for every low-level `russh` hook or control method; advanced users can
use the raw `russh` handle escape hatch when a workflow is not yet represented
by the high-level API.

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
        .password(std::env::var("SSH_PASSWORD").unwrap_or_default())
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
    .password(std::env::var("SSH_PASSWORD").unwrap_or_default())
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

// For tokio::io integration, convert to AsyncRead + AsyncWrite:
let mut async_io = session
    .shell()
    .pty(russh_extra::Pty::new("xterm-256color", 120, 40))
    .build()
    .open()
    .await?
    .into_async_io();

tokio::io::copy(&mut async_io, &mut tokio::io::stdout()).await?;
```

Subsystem channels use the same streaming handle. For a higher-level SFTP
experience, enable the `sftp` feature and use `Session::sftp()` (see the
[SFTP](#sftp) section below).

## Port Forwarding

Enable the `tunnel` feature for local TCP forwarding, remote TCP forwarding,
one-shot direct TCP channels, and StreamLocal (Unix-domain) forwarding.

### Local Forwarding

```rust
let tunnel = session
    .tunnel(russh_extra::ForwardSpec::local_tcp(
        ("127.0.0.1", 8080),
        ("10.0.0.10", 80),
    ))
    .start()
    .await?;

println!("bound: {}", tunnel.bound_addr().unwrap());
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

println!("remote port: {}", tunnel.bound_addr().unwrap().port());
tunnel.close().await?;
```

### StreamLocal (Unix-Domain) Forwarding

```rust
let tunnel = session
    .tunnel(russh_extra::ForwardSpec::local_streamlocal(
        "/tmp/remote.sock",
        "/var/run/app.sock",
    ))
    .start()
    .await?;

println!("bound to: {}", tunnel.bound_path().unwrap());
tunnel.close().await?;
```

One-shot direct StreamLocal channels:

```rust
let mut stream = session
    .direct_streamlocal("/var/run/service.sock")
    .open()
    .await?;

stream.write_all(b"ping").await?;
stream.close().await?;
```

StreamLocal forwarding is available on Unix platforms when the `tunnel` feature
is enabled.

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

## SFTP

Enable the `sftp` feature for native SFTP v3 client operations over SSH
subsystem channels:

```rust
let sftp = session.sftp().await?;

let metadata = sftp.metadata("/etc/hostname").await?;
println!("size: {:?}", metadata.size());

let mut dir = sftp.opendir("/tmp").await?;
while let Some(entry) = sftp.readdir(&mut dir).await? {
    println!("{}", entry.filename());
}
dir.close().await?;

let contents = sftp.read_to_vec("/etc/hostname").await?;
```

The SFTP client supports open, read, write, close_file, metadata,
symlink_metadata, set_stat, fset_stat, opendir, readdir, remove, rename,
create_dir, remove_dir, canonicalize, readlink, symlink, read_to_vec,
and write_all operations. File and directory handles auto-close on drop.

Server-side SFTP is available via the `SftpServerHandler` trait when both
`server` and `sftp` features are enabled:

```rust
use russh_extra::{SftpServerHandler, SftpMetadata, SftpDirEntry};

struct MyFs;

#[russh_extra::async_trait]
impl SftpServerHandler for MyFs {
    async fn read(&self, _id: u32, handle: String, offset: u64, len: u32)
        -> russh_extra::Result<Vec<u8>>
    {
        // Read from a virtual file identified by handle
        Ok(vec![b'x'; len as usize])
    }
}
```

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `client` | yes | Client connect, authentication, command execution, and session APIs |
| `known-hosts` | yes | Known-hosts parser, in-memory store, and client integration |
| `aws-lc-rs` | yes | `russh` crypto backend via aws-lc-rs |
| `server` | no | Server listener, auth callbacks, exec routing, lifecycle hooks |
| `shell` | no | Interactive shell, PTY, subsystems; X11/agent forwarding needs `tunnel` |
| `tunnel` | no | TCP and StreamLocal (Unix-domain) forwarding, direct channels |
| `agent` | no | SSH agent authentication using `$SSH_AUTH_SOCK` on Unix |
| `sftp` | no | Native SFTP v3 client; add `server` for `SftpServerHandler` trait |
| `ring` | no | Alternative `russh` crypto backend via ring |
| `flate2` | no | SSH compression support from `russh` |
| `rsa` | no | RSA key algorithm support from `russh` |
| `serde` | no | Serde serialization for config types |
| `full` | no | All stable runtime features |

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
cargo check -p russh-extra --no-default-features --features server,sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features full
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

Implemented in the 0.1 line:

- Client connect with password, private-key, agent, and keyboard-interactive authentication.
- Strict, pinned SHA256, and known-hosts host-key verification.
- Trust-on-first-use in the in-memory known-hosts store.
- Changed and revoked host-key rejection for known-hosts entries.
- Buffered `Session::command()` with stdout/stderr capture, stdin, limits, and exit metadata.
- Explicit `Session::disconnect()` for graceful client-side connection teardown.
- Server listener, password auth, public-key auth, keyboard-interactive auth, exact command routing, streaming exec, env propagation, lifecycle hooks, and graceful shutdown.
- Interactive shell, PTY allocation, resize, signal, X11 forwarding, agent forwarding, and subsystem channel opening.
- Native SFTP v3 client: open, read, write, close_file, metadata, symlink_metadata, opendir, readdir, remove, rename, create_dir, remove_dir, canonicalize, readlink, symlink, read_to_vec, write_all.
- Direct TCP channels, local TCP forwarding, remote TCP forwarding, and StreamLocal (Unix-domain) forwarding.
- OpenSSH certificate authentication (certificate + private key pairs).
- Authentication banner display and server-side banner configuration.
- Structured tracing spans on connect, command, disconnect, and server run entry points.
- Typed error taxonomy and local loopback test fixtures.
- 14 example programs covering client, server, shell, subsystem, known-hosts, SFTP, and forwarding workflows.

Primary client, server, SFTP, and TCP forwarding paths are covered by local
loopback integration tests. Some advanced Unix StreamLocal paths have
implementation coverage and remain a hardening target for additional runtime
tests.

Not yet implemented:

- Hashed hostname known-hosts matching and writing.
- Wildcard hostname known-hosts matching.
- Dynamic SOCKS-style forwarding.
- SFTP v4+ extensions.
- First-class high-level wrappers for every `russh` control surface. Current
  gaps include some low-level client controls such as rekey/keepalive/ping and
  no-more-sessions requests, and lower-level server hooks such as signal and DH
  GEX group lookup.

## Examples

The `crates/russh-extra/examples/` directory contains working example programs:

| Example | Feature flags | Description |
|---|---|---|
| [`client_exec`](crates/russh-extra/examples/client_exec.rs) | `client` | Remote command execution with password auth |
| [`client_exec_password`](crates/russh-extra/examples/client_exec_password.rs) | `client` | Password auth with explicit credential |
| [`client_private_key`](crates/russh-extra/examples/client_private_key.rs) | `client`, `known-hosts` | Private key auth with known-hosts verification |
| [`client_shell`](crates/russh-extra/examples/client_shell.rs) | `client`, `shell` | Interactive shell with PTY allocation |
| [`client_subsystem`](crates/russh-extra/examples/client_subsystem.rs) | `client`, `shell` | Raw SSH subsystem channel |
| [`client_known_hosts`](crates/russh-extra/examples/client_known_hosts.rs) | `client`, `known-hosts` | Known-hosts loading, TOFU, and saving |
| [`client_sftp`](crates/russh-extra/examples/client_sftp.rs) | `client`, `sftp` | SFTP file read, upload, and directory listing |
| [`local_forward`](crates/russh-extra/examples/local_forward.rs) | `client`, `tunnel` | Local TCP port forwarding |
| [`remote_forward`](crates/russh-extra/examples/remote_forward.rs) | `client`, `tunnel` | Remote TCP port forwarding |
| [`server_exec`](crates/russh-extra/examples/server_exec.rs) | `server` | Server with exec routing |
| [`server_password`](crates/russh-extra/examples/server_password.rs) | `server` | Server with password auth and exec routing |
| [`server_public_key`](crates/russh-extra/examples/server_public_key.rs) | `server` | Server with public key authentication |
| [`server_streaming_exec`](crates/russh-extra/examples/server_streaming_exec.rs) | `server` | Server with streaming exec handlers |
| [`tracing`](crates/russh-extra/examples/tracing.rs) | `client`, `known-hosts` | Tracing instrumentation with env-filter |

Each example uses environment variables for configuration. Run with:

```bash
SSH_HOST=localhost SSH_PORT=2222 SSH_USER=test SSH_PASSWORD=secret \
  cargo run --example client_exec --features client,aws-lc-rs
```

## Workspace

| Crate | Purpose |
|---|---|
| `russh-extra` | User-facing high-level API |
| `russh-extra-core` | Shared SSH domain types and errors |
| `russh-extra-test-support` | Integration test helpers (not published) |
| `russh-extra-tests` | Workspace-level tests (not published) |

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
cargo clippy --workspace --all-targets --all-features -- -D warnings
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
- `.agents/skills/` contains local development skills.

## Project Status

`0.1.4` is the final release in the `0.1.x` series. The crate supports client,
server, authentication, known-hosts, command execution, shell, PTY, subsystems,
X11 forwarding, agent forwarding, OpenSSH certificate authentication, auth
banner, local/remote TCP and StreamLocal forwarding, and SFTP (client and
server handler). 284 tests pass on Linux, macOS, and Windows with 0 failures.

See [CHANGELOG.md](CHANGELOG.md) for the full release history.

## License

This project is licensed under either the MIT license or the Apache License,
Version 2.0.
