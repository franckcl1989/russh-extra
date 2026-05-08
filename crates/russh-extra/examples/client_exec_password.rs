//! Connects to a remote SSH server using password authentication and runs a
//! buffered command.
//!
//! Usage:
//! ```bash
//! SSH_HOST=example.com SSH_USER=deploy SSH_PASSWORD=... cargo run --example \
//!   client_exec_password --features client,aws-lc-rs
//! ```
//!
//! This example uses `accept_any_host_key()` for simplicity. For production,
//! use `try_pinned_host_key_sha256()` or configure `KnownHosts`.

use russh_extra::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::var("SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let user = std::env::var("SSH_USER").unwrap_or_else(|_| "root".into());
    let password = std::env::var("SSH_PASSWORD").ok();

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(user.as_str())
        .password(password.as_deref().unwrap_or(""))
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let output = session.command("uname -a").await?;

    println!("exit: {:?}", output.exit);
    println!("stdout: {}", String::from_utf8_lossy(&output.stdout));

    if !output.stderr.is_empty() {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}
