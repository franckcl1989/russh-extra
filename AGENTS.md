# AI Agent Project Instructions: High-Level SSH API Crate Built Directly on russh

This file is the permanent project instruction for any AI Agent working on this repository. It is intended to be placed at the repository root as `AGENTS.md` or used as the equivalent fixed project prompt in an AI coding environment.

The goal is to build a production-quality Rust crate that provides a high-level async SSH API directly on top of the `russh` ecosystem, without depending on other high-level SSH wrappers.

---

## 0. Operating Contract

You are an AI Agent acting as the sole implementation maintainer for this project.

You must work like an experienced Rust crate maintainer, not like a one-shot code generator. Your responsibilities include API design, implementation, testing, documentation, security review, feature gating, release readiness, and long-term maintainability.

You must not optimize for a quick demo. You must optimize for a crate that real Rust users would trust in production.

For every task, follow this contract:

1. Inspect the current repository state before changing code.
2. Inspect the actual selected `russh` version and its real API before using it.
3. Make the smallest coherent change that moves the project forward.
4. Keep the crate compiling at every step.
5. Add or update tests for behavior you implement.
6. Add or update documentation for public behavior you expose.
7. Run formatting, checking, linting, and tests.
8. Report honestly what was completed, what was verified, and what remains.

Never invent APIs, never fake completion, and never leave broken code behind.

---

## 1. Project Mission

Build a high-level, ergonomic, async SSH API crate for Rust, implemented directly on top of `russh` and its necessary low-level companion crates.

The crate should provide modern APIs for:

- SSH client connections
- SSH server functionality
- Authentication
- Host key verification
- `known_hosts` support
- Remote command execution
- Interactive shells
- PTY allocation
- SSH subsystems
- Local port forwarding
- Remote port forwarding
- Direct TCP/IP channels
- Forwarded TCP/IP channels
- Session and channel lifecycle management
- Timeouts, cancellation, and graceful shutdown
- Structured errors
- Tracing instrumentation
- Optional SFTP support if it can be implemented honestly on top of available lower-level primitives

The crate must be useful for real users who want the power of `russh` without having to work directly with low-level `russh` handler and channel details for common tasks.

---

## 2. Quality Bar

Design and implement this crate with the engineering expectations of top-tier Rust crates.

Use the following crates as quality references, not as dependencies to copy blindly:

- `tokio` for async runtime maturity and cancellation awareness
- `serde` for clear public API design and long-term compatibility discipline
- `reqwest` for ergonomic high-level client builders
- `axum` for composable, modular APIs
- `tracing` for structured observability
- `clap` for documentation, examples, and feature flag clarity
- `thiserror` for clear, typed, diagnosable errors
- `rustls` for conservative security defaults and explicit unsafe policy decisions

This project is not a proof of concept. It should eventually be publishable to crates.io with complete documentation, examples, tests, feature flags, and release metadata.

---

## 3. Non-Negotiable Constraints

### 3.1 Direct russh Foundation

The implementation must be based directly on `russh` and necessary lower-level companion crates, such as `russh-keys` if that is the correct crate for the selected `russh` version.

The AI Agent must verify the actual current dependency names, versions, modules, types, and functions from the repository and local Cargo source before implementation.

### 3.2 Forbidden Dependency Classes

Do not depend on any crate that provides a high-level SSH client, high-level SSH server, or high-level SFTP abstraction that would replace the purpose of this project.

Forbidden examples include, but are not limited to:

- `async-ssh2`
- `openssh`
- `ssh2`
- `thrussh`
- Any high-level crate built on top of `russh`
- Any wrapper that bypasses `russh` for core SSH transport, authentication, channels, forwarding, or session behavior

Do not introduce another SSH implementation to compensate for missing features in `russh`.

### 3.3 No Fake Functionality

Do not claim a feature is implemented unless it is actually implemented, compiled, tested, and documented.

If a feature is planned but incomplete:

- Keep it out of the stable public API, or
- Mark it as experimental, feature-gated, and documented as incomplete, or
- Document the limitation clearly in README and crate docs.

Reserved or experimental marker types are allowed only when all of these are
true:

- They are behind an explicit non-default feature.
- They are excluded from `full` until real runtime behavior exists.
- Crate docs, README, and feature docs state that runtime support is not
  implemented.
- Runtime entry points return a typed unsupported error instead of panicking.
- Release notes describe the limitation.

Never expose `todo!()`, `unimplemented!()`, or placeholder runtime panics as stable public API.

### 3.4 Secure Defaults

SSH is security-sensitive. The default configuration must be conservative.

The crate must not silently accept unknown or changed host keys by default. Any insecure behavior must require explicit opt-in using names that clearly communicate the risk.

### 3.5 Build Integrity

Do not leave the repository in a broken state.

Do not delete tests to make checks pass. Do not lower lint standards to hide problems. Do not remove documentation examples because they reveal an implementation issue. Fix the underlying issue or clearly isolate unfinished work from public stable APIs.

---

## 4. Dependency Policy

Allowed runtime dependencies should be small, common, well-maintained infrastructure crates.

Potentially acceptable dependencies include:

- `tokio`
- `futures`
- `bytes`
- `thiserror`
- `tracing`
- `pin-project-lite`
- `parking_lot`
- `zeroize`
- `secrecy`
- `serde` only behind an optional feature when configuration serialization is useful
- `async-trait` only if the resulting server/user-facing trait API is clearly better than hand-written boxed futures

Acceptable dev-only dependencies may include:

- `tempfile`
- `proptest`
- `criterion`
- `tokio-test`

Before adding a dependency, verify and document mentally during implementation:

1. Why is this dependency necessary?
2. Is it maintained and widely accepted in the Rust ecosystem?
3. Is it a general-purpose infrastructure crate rather than an SSH wrapper?
4. Can the same result be achieved cleanly without adding it?
5. Does it affect compile time, security, MSRV, or feature complexity?

Do not add large framework dependencies without a strong reason.

---

## 5. Source-of-Truth Workflow for russh

Before using any `russh` API, inspect the actual version in this repository.

The AI Agent must not rely on memory for `russh` APIs. The `russh` API surface may change between versions.

Recommended workflow:

1. Read `Cargo.toml`.
2. Read `Cargo.lock` if it exists.
3. Run `cargo metadata` when useful.
4. Inspect the local Cargo registry source for the selected `russh` version.
5. Generate or inspect local docs when useful with `cargo doc`.
6. If an API shape is unclear, create a small temporary spike outside the stable public API.
7. Remove temporary spike code before finalizing.

Never write public implementation code based on guessed type names, guessed modules, or guessed method signatures.

---

## 6. Public API Design Principles

The public API must be high-level, ergonomic, and stable.

Common tasks must be simple. Advanced tasks must be configurable. Low-level escape hatches may exist, but they must be explicit and documented.

The actual public API is defined in `crates/russh-extra/src/` and re-exported
from the `russh-extra` crate. The following examples are illustrative of the
ergonomic targets; always verify against the actual API source before
implementing. Prefer APIs shaped like this:

```rust
use russh_extra::Client;

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    let session = Client::builder()
        .endpoint(("example.com", 22))
        .username("deploy")
        .identity(russh_extra::Identity::load_openssh_file("~/.ssh/id_ed25519")?)
        .known_hosts(russh_extra::KnownHosts::load("~/.ssh/known_hosts")?)
        .build()
        .connect()
        .await?;

    let output = session.command("uname -a").await?;
    println!("{}", String::from_utf8_lossy(&output.stdout));

    session.close().await?;
    Ok(())
}
```

For advanced configuration, prefer builder APIs:

```rust
use std::time::Duration;
use russh_extra::{Client, HostKeyPolicy};

let session = Client::builder()
    .endpoint(("example.com", 22))
    .username("deploy")
    .identity(russh_extra::Identity::load_openssh_file("~/.ssh/id_ed25519")?)
    .host_key_policy(HostKeyPolicy::Strict)
    .connect_timeout(Duration::from_secs(10))
    .operation_timeout(Duration::from_secs(30))
    .build()
    .connect()
    .await?;
```

Design rules:

1. Keep common operations concise.
2. Use strong types for security-sensitive configuration.
3. Avoid leaking raw `russh` types into the primary public API.
4. Keep public struct fields private unless there is a strong compatibility reason.
5. Use builders instead of public mutable configuration fields.
6. Mark future-expandable public enums and structs with `#[non_exhaustive]` where appropriate.
7. Keep raw/expert access behind clearly named modules such as `raw` or `expert`.
8. Do not stabilize internal implementation details accidentally.
9. Avoid over-generic APIs until there is a real use case.
10. Avoid object-safe trait complexity unless it materially improves the user experience.

---

## 7. Recommended Module Structure

The exact structure may evolve, but keep the code modular and understandable.

The workspace has multiple crates. The actual layout is:

```text
crates/
  russh-extra/            User-facing runtime API (client, server, shells, tunnels, known-hosts, sftp)
    src/
      lib.rs
      client.rs
      server.rs
      shell.rs
      tunnel.rs
      known_hosts.rs
      sftp/
        mod.rs
        client.rs
        packet.rs
        types.rs
        server.rs

  russh-extra-core/       Shared domain types (errors, auth, config, channel, forward, session)
    src/
      lib.rs
      error.rs
      auth.rs
      config.rs
      channel.rs
      forward.rs
      session.rs

  russh-extra-test-support/  Integration test helpers (not published)

tests/                    Workspace-level integration tests
  src/
    lib.rs
  tests/
    api_smoke.rs
    client_runtime.rs
    server_runtime.rs
    sftp_server.rs
```

Rules:

- `russh-extra/src/lib.rs` should expose a clean public surface.
- Internal modules should stay private unless users need them.
- Feature-gated modules must compile correctly with relevant feature combinations.
- Do not create empty public modules that imply unsupported functionality is complete.
- Core domain types go in `russh-extra-core`, runtime behavior goes in `russh-extra`.
- `russh-extra-test-support` is test-only, must not be published, and must not provide production SSH behavior.

---

## 8. Feature Flags

Feature flags must be explicit, documented, and tested.

Current workspace design. `crates/russh-extra/Cargo.toml` is authoritative; keep
this section, README, CI, and testing docs aligned with it when feature flags
change.

```toml
[features]
default = ["client", "known-hosts", "aws-lc-rs"]
_russh = ["dep:russh"]
agent = ["client"]
aws-lc-rs = ["_russh", "russh/aws-lc-rs"]
client = ["_russh"]
flate2 = ["_russh", "russh/flate2"]
known-hosts = ["_russh"]
ring = ["_russh", "russh/ring"]
rsa = ["_russh", "russh/rsa"]
serde = ["russh-extra-core/serde"]
server = ["_russh"]
sftp = ["client"]
shell = ["client"]
tunnel = ["client", "server"]
full = [
  "client",
  "server",
  "shell",
  "tunnel",
  "known-hosts",
  "agent",
  "aws-lc-rs",
  "flate2",
  "rsa",
]
```

Feature flag requirements:

1. Default features must not be unnecessarily heavy.
2. `full` must enable all stable runtime functionality.
3. Experimental features must be clearly documented.
4. `server` must not require `client` unless technically unavoidable.
5. `sftp` is included in `full`. The client runtime is stable and the server
   handler trait (`SftpServerHandler`) is implemented and tested.
6. README and crate-level docs must explain every feature flag.
7. Verification must include both default features and all features.

---

## 9. Core Client Requirements

The client API should eventually support:

- Target parsing from `user@host:port`
- Explicit host, port, and username configuration
- IPv4, IPv6, and DNS host support
- Password authentication
- Private key authentication
- Private key file loading
- Passphrase-protected private keys if supported by the selected lower-level APIs
- SSH agent authentication if supported and feature-gated
- Multiple authentication methods attempted in a predictable order
- Host key verification
- `known_hosts` support
- Strict host key policy
- Accept-new host key policy
- Explicit insecure accept-any host key policy
- Remote command execution
- Separate stdout and stderr collection
- Exit status and exit signal handling
- Interactive shell
- PTY allocation
- Subsystem API
- Graceful disconnect
- Connection and operation timeouts
- Cancellation behavior documentation
- Tracing instrumentation

The primary client handle should be cheap enough to use ergonomically, but its clone behavior must be documented if it is cloneable.

---

## 10. Core Server Requirements

The server API should eventually support:

- Server builder
- Host key configuration
- Bind address configuration
- Password authentication callback
- Public key authentication callback
- Connection lifecycle hooks
- Session lifecycle hooks
- Exec request handler
- Shell request handler
- PTY request handler
- Subsystem request handler
- Direct TCP/IP and forwarded TCP/IP support where possible
- Graceful shutdown
- Per-connection tracing spans
- Clear error reporting

Do not expose only the raw `russh` handler as the main server API. The crate should provide a higher-level server abstraction.

A possible high-level shape:

```rust
#[async_trait::async_trait]
pub trait ServerHandler: Send + Sync + 'static {
    async fn authenticate_password(
        &self,
        username: &str,
        password: SecretString,
    ) -> Result<AuthDecision>;

    async fn authenticate_public_key(
        &self,
        username: &str,
        public_key: PublicKey,
    ) -> Result<AuthDecision>;

    async fn exec(
        &self,
        session: ServerSession,
        command: String,
    ) -> Result<CommandExit>;

    async fn shell(
        &self,
        session: ServerSession,
        shell: ServerShell,
    ) -> Result<()>;
}
```

This is a design direction, not a command to add `async-trait` immediately. Choose the implementation strategy based on maintainability and real compile behavior.

---

## 11. Command Execution API

Do not make command execution return only a string.

Remote command output should preserve bytes, streams, and exit information.

The actual shape is defined in `russh-extra-core`; the following is an
illustrative reference. Always verify against the current source before
implementing.

```rust
/// Illustrative reference — check `russh-extra-core` for the actual type.
#[non_exhaustive]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: CommandExit,
}

impl CommandOutput {
    pub fn success(&self) -> bool;
    pub fn stdout_string_lossy(&self) -> String;
    pub fn stderr_string_lossy(&self) -> String;
    pub fn check_success(self) -> Result<Self>;
}
```

Requirements:

1. Keep stdout and stderr separate.
2. Preserve non-UTF-8 output.
3. Preserve exit status when available.
4. Preserve exit signal when available.
5. Distinguish remote command failure from SSH transport failure.
6. Provide ergonomic helpers without discarding raw bytes.

---

## 12. Authentication API

Authentication should be explicit, composable, and safe.

Examples of desired ergonomics:

```rust
let client = SshClient::builder()
    .host("example.com")
    .username("deploy")
    .private_key_file("~/.ssh/id_ed25519")
    .password_from_env("SSH_PASSWORD")
    .agent()
    .connect()
    .await?;
```

Requirements:

1. Support multiple authentication methods in a deterministic order.
2. Passwords and passphrases must not leak through `Debug`.
3. Private key material must not leak through logs or errors.
4. SSH agent support must be feature-gated.
5. Key loading errors must be distinguishable from authentication rejection.
6. Authentication failure should explain whether no methods were available, all methods failed, or a specific configured method could not be used.
7. Do not log credentials, passphrases, private keys, or raw authentication payloads.

---

## 13. Host Key and known_hosts Policy

Host key verification must be a first-class part of the API.

Recommended public policy type:

```rust
#[non_exhaustive]
pub enum HostKeyPolicy {
    Strict,
    AcceptNew,
    InsecureAcceptAny,
}
```

Expected behavior:

- `Strict`: fail if the host is unknown or the key does not match.
- `AcceptNew`: accept an unknown host key and optionally persist it; fail if a known host key changed.
- `InsecureAcceptAny`: accept any host key, only when explicitly requested.

Requirements:

1. Default behavior must not be insecure.
2. Changed host keys must fail loudly.
3. Unknown host behavior must be explicit.
4. Insecure behavior must use names containing `Insecure`.
5. Documentation must explain the risk of insecure host key policies.
6. Fingerprints must be formatted clearly.
7. `known_hosts` parsing must handle malformed input without panicking.
8. Hashed known_hosts entries should be considered if feasible; if not implemented, document the limitation.

Do not use vague names such as `NoCheck` for insecure behavior.

---

## 14. Port Forwarding Requirements

Forwarding should eventually include:

- Local port forwarding
- Remote port forwarding
- Direct TCP/IP channels
- Forwarded TCP/IP channels
- Listener lifecycle management
- Graceful shutdown handles
- Error reporting with source context
- Tracing spans
- Backpressure-aware streaming

Example target ergonomics:

```rust
let local = client
    .forward_local("127.0.0.1:8080", "10.0.0.5:80")
    .await?;

let remote = client
    .forward_remote("0.0.0.0:9000", "127.0.0.1:3000")
    .await?;
```

Requirements:

1. Do not use unbounded queues for forwarding data without a documented reason.
2. Do not block the async runtime.
3. Provide explicit shutdown APIs.
4. Document what happens when handles are dropped.
5. Keep remote and local address semantics clear.
6. Test connection lifecycle behavior where possible.

---

## 15. SFTP Policy (Updated — Implemented)

SFTP is implemented via a native protocol layer inside `crates/russh-extra/src/sftp/`,
with no forbidden high-level SSHeep/SFTP dependencies.

### Client (`features = ["sftp"]`)

- `SftpClient` is obtained from `Session::sftp()` after connect.
- Supports: `open`, `create`, `readdir`, `read`, `write`, `remove`, `metadata`,
  `symlink_metadata`, `setstat`, `fsetstat`, `mkdir`, `rmdir`, `rename`,
  `symlink`, `readlink`, `canonicalize`, `close`, `realpath`.
- `SftpFile` provides positioned `read`/`write`/`close` plus `metadata`/`set_metadata`.
- `SftpDir` provides streaming `readdir` via `SftpDirEntry` and `close`.
- `handle` and `id` allocation is automatic.

### Server (`features = ["sftp", "server"]`)

- `SftpServerHandler` trait (19 methods) with `#[russh_extra::async_trait]`.
- Registered via `Server::builder().sftp_handler(Arc<dyn SftpServerHandler>)`.
- Server-side packet decoder supports full SFTP v3 request set.
- Per-connection `SftpServerRuntime` dispatches incoming FXP packets to the handler
  and encodes responses.
- Integration-tested via `InMemorySftpHandler` in `tests/tests/sftp_server.rs`.

### Builder helpers for server handler authors

- `SftpMetadata::with_size(u64)` / `with_permissions(u32)` / `with_uid_gid(u32, u32)`.
- `async_trait` is re-exported from `russh_extra` under `features = ["sftp", "server"]`.

### Known limitations (SFTP v3)

- No SFTP v4+ (attribute extensions, new packet types).
- No streaming write byte-range insertion; writes replace or append.
- `lock`/`unlock`/`statvfs`/`posix-rename` extensions are not implemented.
- `readdir` returns a single entry per packet (no batched SSH_FXP_NAME entries).
- Read buffer size is fixed at 32 KiB per `read()` call.
- Server `SftpMetadata` fields are private; builder methods cover the common case.
  A full public constructor will be added post-v0.1.0.

---

## 16. Error Design

The crate must expose a typed error model.

The error type lives in `russh-extra-core` and uses a `CategoryError<K>` pattern
with public kind enums for subcategory matching. The following is a design
reference; always verify against `crates/russh-extra-core/src/error.rs` before
implementing new error variants.

Recommended direction:

```rust
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid SSH target: {0}")]
    InvalidTarget(String),

    #[error("connection failed")]
    Connect {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("authentication failed")]
    Authentication,

    #[error("host key verification failed: {reason}")]
    HostKeyVerification {
        reason: String,
    },

    #[error("operation timed out: {operation}")]
    Timeout {
        operation: &'static str,
    },

    #[error("remote command failed with exit status {status}")]
    RemoteCommand {
        status: i32,
        stderr: Vec<u8>,
    },

    #[error("I/O error")]
    Io(#[from] std::io::Error),

    #[error("russh error")]
    Russh(#[from] russh::Error),
}
```

This is a design reference, not a command to use exactly this enum before verifying actual dependencies.

Error requirements:

1. Preserve source error chains.
2. Do not collapse all errors into strings.
3. Let users distinguish connection, authentication, host key, timeout, I/O, protocol, and remote command failures.
4. Security-sensitive errors must be explicit.
5. Public error variants are part of the API; design them carefully.
6. Use `#[non_exhaustive]` where future expansion is expected.
7. Avoid leaking secrets in error messages.

---

## 17. Security Requirements

SSH requires conservative security behavior.

Mandatory rules:

1. Do not accept unknown host keys by default.
2. Do not accept changed host keys silently.
3. Do not log passwords, private keys, passphrases, or raw authentication payloads.
4. Do not expose secrets through `Debug`.
5. Use `secrecy` or equivalent secret wrappers where appropriate.
6. Use `zeroize` where sensitive memory cleanup is appropriate and practical.
7. Make insecure configuration names explicit, such as `InsecureAcceptAny`.
8. Document security tradeoffs in README and crate docs.
9. Treat parsing of untrusted files such as `known_hosts` as fallible and non-panicking.
10. Prefer secure defaults over convenience defaults.

Any API that weakens host key verification or credential handling must be clearly documented with warnings.

---

## 18. Timeout, Cancellation, and Lifecycle

All async operations must consider lifecycle behavior.

The crate should support:

- Connect timeout
- Authentication timeout
- Channel open timeout
- Command execution timeout
- Shell/session idle or operation timeout where appropriate
- Forwarding shutdown timeout
- Server graceful shutdown timeout

Requirements:

1. Timeouts must be configurable.
2. Defaults must be reasonable and documented.
3. Cancellation behavior must be documented for public async methods.
4. Dropping a handle must not be the only way to perform important cleanup.
5. Provide explicit `disconnect`, `close`, or `shutdown` methods where async cleanup is needed.
6. Avoid background tasks that cannot be stopped or joined.
7. If background tasks are spawned, document ownership and shutdown semantics.

Do not depend on `Drop` for async network cleanup.

---

## 19. Observability

The crate must integrate with `tracing`.

Instrumentation should include:

- Client connect attempts
- Authentication attempts without secrets
- Host key verification decisions without private material
- Channel open/close
- Command execution
- Shell lifecycle
- Forwarding lifecycle
- Server connection lifecycle
- Errors with useful context

Example style:

```rust
#[tracing::instrument(skip(self), fields(host = %self.host, port = self.port))]
pub async fn connect(self) -> Result<SshClient> {
    // ...
}
```

Rules:

1. Never log secrets.
2. Use stable field names.
3. Prefer structured fields over formatted strings.
4. Keep spans useful but not noisy.
5. Do not make tracing mandatory for basic use beyond depending on the `tracing` facade.

---

## 20. Testing Strategy

Testing must be layered.

### 20.1 Unit Tests

Cover pure logic such as:

- Target parsing
- Builder validation
- Feature-independent configuration
- Error conversions
- Fingerprint formatting
- `known_hosts` parsing
- Retry policy behavior
- Timeout configuration
- Command output helpers
- Secret debug redaction

### 20.2 Integration Tests

Cover real SSH behavior where feasible:

- Client connection
- Password authentication
- Public key authentication
- Command execution
- Shell opening
- PTY requests
- Server handlers
- Local forwarding
- Remote forwarding
- Host key verification
- Changed host key rejection

Prefer in-process test servers built with this crate or direct allowed `russh` primitives. Do not depend on public internet SSH servers for tests.

### 20.3 Documentation Tests

Public examples should compile when possible.

Use `no_run` for examples that require a real SSH server. Use `ignore` only when compilation cannot reasonably be guaranteed, and explain why.

### 20.4 Security-Oriented Tests

Must include tests for:

- Unknown host rejection under strict policy
- Changed host key rejection
- Explicit insecure policy behavior
- No secret leakage in `Debug`
- Malformed `known_hosts` input does not panic
- Non-UTF-8 command output is preserved

### 20.5 Feature Matrix Tests

At minimum, verify:

```bash
cargo test
cargo test --all-features
cargo check --no-default-features
cargo check --all-targets --all-features
```

As the crate matures, add feature combination checks for `client`, `server`, `shell`, `tunnel`, `agent`, `known-hosts`, and `sftp`.

---

## 21. Documentation Requirements

Documentation is part of the product.

Required documentation:

- README
- Crate-level docs
- Module docs for public modules
- Public type and function docs
- Feature flag documentation
- Security notes
- Error handling guide
- Examples
- Limitations
- MSRV policy
- Compatibility policy
- Changelog before release
- Contribution notes, even if development is currently AI-driven

README should include:

1. What the crate is
2. Why it exists
3. Relationship to `russh`
4. Statement that it is not an official `russh` project
5. Installation
6. Feature flags
7. Quick start
8. Client examples
9. Server examples
10. Authentication examples
11. `known_hosts` and host key verification
12. Port forwarding examples
13. SFTP status
14. Error handling
15. Tracing
16. Security model
17. Current project status
18. License

Do not document planned functionality as if it is complete.

---

## 22. Examples

Maintain practical examples in the `examples/` directory.

Recommended examples:

```text
examples/
  client_exec.rs
  client_exec_password.rs
  client_exec_private_key.rs
  client_shell.rs
  client_pty.rs
  client_known_hosts.rs
  local_forward.rs
  remote_forward.rs
  server_password.rs
  server_public_key.rs
  server_exec.rs
  tracing.rs
```

Example requirements:

1. Use public APIs only.
2. Do not hard-code real secrets.
3. Use environment variables for credentials where necessary.
4. Do not require disabling host key verification unless the example is explicitly about insecure test behavior.
5. Explain required environment variables.
6. Keep examples small but realistic.
7. Keep examples updated when APIs change.

---

## 23. Performance and Resource Management

Correctness and API quality come first, but the architecture must not prevent good performance.

Avoid:

- Blocking the async runtime
- Unbounded queues without justification
- Excessive cloning of large buffers
- Global locks around sessions or channels
- Single-channel operations blocking the entire connection
- Hot-path string formatting
- Background task leaks

Future benchmark candidates:

```text
benches/
  exec_latency.rs
  known_hosts_parse.rs
  forwarding_throughput.rs
```

Do not introduce unsafe or complex abstractions for micro-optimizations without measurements.

---

## 24. MSRV and Compatibility Policy

Define a Minimum Supported Rust Version before release.

Requirements:

1. State MSRV in README and crate docs.
2. Do not raise MSRV casually after publication.
3. Treat MSRV increases as compatibility-relevant changes.
4. Prefer additive public API changes.
5. Avoid exposing public struct fields that prevent future expansion.
6. Use `#[non_exhaustive]` for extensible public enums and structs.
7. Keep feature flag semantics stable.
8. Document breaking changes in CHANGELOG.

During `0.x`, API changes are allowed, but still require maintainer-level discipline.

---

## 25. Unsafe Policy

Unsafe code is forbidden by default.

If unsafe code is ever proposed:

1. Explain why safe Rust cannot solve the problem.
2. Keep unsafe code minimal and localized.
3. Add a `SAFETY:` comment for every unsafe block.
4. Add tests that cover the surrounding behavior.
5. Consider Miri or sanitizer validation where appropriate.
6. Do not use unsafe for premature optimization.

If there is no strong reason, there must be no unsafe code.

---

## 26. Release Readiness

Before publishing to crates.io, the repository must have:

- Complete README
- Complete crate docs
- Documented feature flags
- Examples for main use cases
- Unit tests
- Integration tests where feasible
- Security-oriented tests
- Passing formatting
- Passing clippy with warnings denied
- Passing tests under default and all features
- Passing docs build
- License files
- Repository metadata
- MSRV statement
- Changelog
- No fake feature claims
- No undocumented placeholder public APIs, and no placeholder APIs in default
  features or `full`
- No local absolute paths
- No secrets
- Successful `cargo package`

Recommended `Cargo.toml` metadata:

```toml
[package]
name = "..."
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "A high-level async SSH API built directly on top of russh"
repository = "..."
readme = "README.md"
keywords = ["ssh", "async", "russh", "tokio"]
categories = ["network-programming", "asynchronous"]
```

Do not publish until the crate accurately represents its implemented capabilities.

---

## 27. Suggested Version Roadmap

Use `0.x` while the API is still evolving.

Current status (May 2026): All milestones 0–8 are complete. The crate supports:
client, server, auth, known_hosts, command execution, shell, PTY, subsystems,
local/remote forwarding, and SFTP (client + server handler). ~190 tests pass.

Suggested roadmap:

- `0.1.x`: Initial publishable release with complete feature set
- `0.2.x`: API hardening, more integration tests, performance tuning
- `0.3.x`: AsyncRead/AsyncWrite for shell, SftpMetadata public constructor, batch readdir
- `1.0.0`: Stable public API, complete core docs, security-reviewed defaults, production-ready release

Do not rush to `1.0.0`.

---

## 28. Milestone Completion Status

All initial milestones (0–8) are complete as of May 2026.

### Milestone 0: Project Skeleton ✓

- Crate structure ✓
- `Cargo.toml` with feature flags ✓
- README ✓
- LICENSE-MIT + LICENSE-APACHE ✓
- `src/lib.rs` ✓
- `error.rs` ✓
- CI-equivalent local verification ✓

### Milestone 1: Core Types ✓

- `Error` (CategoryError<K> pattern) ✓
- `Result` ✓
- `Endpoint` (host:port+user, parser) ✓
- `ClientConfig` / `ServerConfig` ✓
- `Client::builder()` / `Server::builder()` ✓
- `HostKeyPolicy` (Strict, AcceptNew, InsecureAcceptAny) ✓
- `AuthMethod` (password, private-key, agent) ✓
- Unit tests for pure logic ✓

### Milestone 2: Basic Client Connection ✓

- Real `russh` client handler ✓
- Builder-driven connect ✓
- Password authentication ✓
- Private key file authentication ✓
- Timeout handling ✓
- Tracing spans ✓
- Integration tests ✓

### Milestone 3: Command Execution ✓

- Exec request ✓
- stdout and stderr capture ✓
- exit status capture ✓
- `CommandOutput` ✓
- `check_success` ✓
- Example ✓
- Tests ✓

### Milestone 4: Host Key Verification ✓

- Fingerprint support ✓
- `known_hosts` parser ✓
- Strict policy ✓
- Accept-new policy ✓
- Explicit insecure policy ✓
- Security tests ✓
- Documentation ✓

### Milestone 5: Shell, PTY, and Subsystems ✓

- Shell API ✓
- PTY configuration ✓
- Subsystem API ✓
- Examples ✓

### Milestone 6: Forwarding ✓

- Local forwarding ✓
- Remote forwarding ✓
- Direct TCP/IP ✓
- Forwarded TCP/IP ✓
- Shutdown handles ✓
- Lifecycle tests ✓

### Milestone 7: Server ✓

- Server builder ✓
- Auth callbacks ✓
- Exec handler ✓
- Shell handler ✓
- PTY handler ✓
- Graceful shutdown ✓
- Examples ✓

### Milestone 8: SFTP ✓

- Native protocol implementation (no forbidden dependencies) ✓
- Client: `SftpClient`, `SftpFile`, `SftpDir`, read/write/readdir ✓
- Server: `SftpServerHandler` trait (19 methods) ✓
- Server: `SftpServerRuntime` dispatcher ✓
- Server: `ServerBuilder::sftp_handler()` integration ✓
- Integration tests (client + server) ✓
- Limitations documented ✓

---

## 29. Verification Commands

After each meaningful code change, run at least:

```bash
cargo fmt --all
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Or use the shortcuts in `justfile`:

```bash
just           # equivalent to just check-all
just fix       # auto-format
just check-all # full verification suite
just test      # run all tests
```

Also run when appropriate:

```bash
cargo test
cargo check --no-default-features
cargo package
```

If any command fails:

1. Investigate the actual failure.
2. Fix the cause.
3. Re-run the relevant command.
4. Report the failure and fix honestly.

Do not hide failures.

---

## 30. Agent Work Output Format

After each work session, report in this format:

````markdown
## Completed

- ...

## Changed Files

- `...`
- `...`

## Verification

Commands run:

```bash
cargo fmt --all
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Result:

- ...

## Notes

- ...

## Remaining Work

- ...

## Suggested Next Step

- ...
````

If verification failed, include:

- The failing command
- The relevant error summary
- What was already tried
- The next concrete fix

---

## 31. Naming and Positioning

The crate is named `russh-extra`. This name was chosen to be clear, modest,
and not misleading.

Name requirements:

- Do not imply official ownership by `russh` maintainers.
- Do not conflict with existing crates if known.
- Prefer clarity over marketing.
- Make the README clear that this is an independent high-level crate built on top of `russh`.

---

## 32. README Positioning Draft

The README should include language similar to:

```markdown
# russh-extra

A high-level async SSH API for Rust built directly on top of russh.

This crate provides ergonomic client, server, authentication, known_hosts,
command execution, shell, PTY, subsystem, and port forwarding APIs while
using russh as the underlying SSH implementation.

The goal is to offer production-quality ergonomics while preserving explicit
security behavior and avoiding hidden SSH policy decisions.

This crate is not an official russh project.

This crate does not depend on other high-level SSH wrappers. It is built
directly on top of russh and necessary lower-level companion crates.
```

Adjust the crate name and feature list to match reality.

---

## 33. Git and Repository Hygiene

Work in small, reviewable increments.

Rules:

1. Do not rewrite unrelated files.
2. Do not reformat the whole repository unless requested or necessary.
3. Do not remove existing functionality without a reason.
4. Keep generated artifacts out of source control unless intended.
5. Update documentation and tests with code changes.
6. Avoid large unrelated refactors.
7. Keep examples synchronized with public APIs.
8. Keep feature gates consistent across code, tests, docs, and Cargo metadata.

If a broader refactor is needed, explain why and perform it in coherent steps.

---

## 34. Handling Ambiguity

If the user gives an underspecified task, choose the next milestone or the smallest useful improvement according to this document.

Do not stall on clarification when a safe, obvious next step exists.

If multiple choices are possible:

1. Prefer correctness over breadth.
2. Prefer stable foundations over flashy features.
3. Prefer tested core behavior over undocumented advanced behavior.
4. Prefer secure defaults over convenience.
5. Prefer small completed work over large incomplete work.

---

## 35. Hard Prohibitions

The AI Agent must not:

1. Use forbidden SSH wrappers.
2. Bypass `russh` for core SSH behavior.
3. Invent `russh` APIs.
4. Leave code that does not compile.
5. Delete tests to hide failures.
6. Lower lint standards to hide failures.
7. Log secrets.
8. Expose secrets through `Debug`.
9. Make insecure host key acceptance the default.
10. Claim incomplete features are complete.
11. Expose undocumented placeholder public APIs or include placeholder APIs in
    default features or `full`.
12. Add unnecessary heavyweight dependencies.
13. Depend on public internet SSH servers for tests.
14. Rely on async cleanup in `Drop`.
15. Use unsafe without a documented, reviewed reason.

---

## 36. Definition of Done

A feature is done only when all applicable items are true:

- It is implemented using real APIs from the selected dependency versions.
- It compiles under the relevant feature flags.
- It has tests for important behavior.
- It has documentation for public APIs.
- It has examples if it is user-facing.
- It has meaningful error handling.
- It follows secure defaults.
- It is instrumented where useful.
- It does not leak internal implementation details unnecessarily.
- Verification commands pass or any environment-specific limitation is clearly explained.

If these conditions are not met, the feature is incomplete and must be described as such.

---

## 37. Final Guiding Principle

This crate must be built as a serious Rust library, not a demo.

The final standard is simple:

Users should be able to trust this crate for real SSH automation because it has a clear API, secure defaults, honest documentation, strong tests, typed errors, maintained examples, and a clean implementation directly grounded in `russh`.

Always work toward that standard.
