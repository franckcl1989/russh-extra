//! Tracing instrumentation example.
//!
//! Initializes a tracing subscriber and runs a connect-and-exec workflow to
//! demonstrate tracing spans.
//!
//! The example emits TRACE-level events; adjust `RUST_LOG` to control
//! verbosity (e.g. `RUST_LOG=russh_extra=info cargo run --example tracing
//! --features client,aws-lc-rs`).
//!
//! Usage:
//! ```bash
//! SSH_HOST=example.com SSH_USER=deploy SSH_PASSWORD=... cargo run --example \
//!   tracing --features client,aws-lc-rs
//! ```
//!
//! Set `SSH_HOST_KEY_SHA256` to pin the server host key. Without it, this
//! example uses `accept_any_host_key()` for convenience; do not use that policy
//! in production.

use std::env;

use russh_extra::Client;

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    let env_filter = env::var("RUST_LOG").unwrap_or_else(|_| "russh_extra=trace".into());
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let host = env::var("SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = env::var("SSH_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(22);
    let username = env::var("SSH_USER").unwrap_or_else(|_| "root".into());
    let command = env::var("SSH_COMMAND").unwrap_or_else(|_| "uname -a".into());

    let mut builder = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username);

    if let Ok(password) = env::var("SSH_PASSWORD") {
        builder = builder.password(password);
    }

    builder = if let Ok(fingerprint) = env::var("SSH_HOST_KEY_SHA256") {
        builder.try_pinned_host_key_sha256(fingerprint)?
    } else {
        builder.accept_any_host_key()
    };

    tracing::info!(host = %host, port, "connecting");

    let session = builder.build().connect().await?;

    tracing::info!(command = %command, "running command");

    let output = session.command(command).await?;

    tracing::info!(
        stdout = %String::from_utf8_lossy(&output.stdout),
        exit = ?output.exit,
        "command completed"
    );

    println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}
