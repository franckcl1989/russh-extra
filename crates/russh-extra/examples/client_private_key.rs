//! Remote command execution with OpenSSH private key authentication.
//!
//! Usage:
//! ```bash
//! cargo run --example client_private_key --features <required-features>
//! ```
//!

//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22            (optional, defaults to 22)
//!   SSH_USER=deploy
//!   SSH_KEY_PATH=~/.ssh/id_ed25519   (optional, defaults to ~/.ssh/id_ed25519)

use std::env;

use russh_extra::{Client, Identity, KnownHosts};

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("russh_extra=debug")
        .init();

    let host = env::var("SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = env::var("SSH_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22);
    let username = env::var("SSH_USER").unwrap_or_else(|_| "root".into());
    let key_path = env::var("SSH_KEY_PATH").unwrap_or_else(|_| "~/.ssh/id_ed25519".into());

    let identity = Identity::load_openssh_file(&key_path)?;
    let known_hosts = KnownHosts::load("~/.ssh/known_hosts")?;

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username)
        .identity(identity)
        .known_hosts(known_hosts)
        .build()
        .connect()
        .await?;

    let output = session.command("whoami").await?;
    println!(
        "logged in as: {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );

    Ok(())
}
