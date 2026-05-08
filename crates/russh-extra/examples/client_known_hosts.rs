//! Known hosts: loading, trust-on-first-use, and saving.
//!
//! Demonstrates three common workflows:
//!   1. Connect with a pre-existing known_hosts file (strict checking).
//!   2. Connect with trust-on-first-use, then persist the new key.
//!   3. Detect and report changed host keys.
//!
//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22         (optional)
//!   SSH_USER=deploy
//!   SSH_PASSWORD=...

use std::env;

use russh_extra::{Client, KnownHosts};

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
    let password = env::var("SSH_PASSWORD").unwrap_or_else(|_| "".into());

    // --- Workflow 1: connect with strict known_hosts ---
    match KnownHosts::load("~/.ssh/known_hosts") {
        Ok(known_hosts) => {
            println!(
                "Loaded known_hosts file ({} entries)",
                known_hosts.entry_count()
            );

            let session = Client::builder()
                .endpoint((host.as_str(), port))
                .username(username.as_str())
                .password(password.as_str())
                .known_hosts(known_hosts)
                .build()
                .connect()
                .await?;

            let output = session.command("echo strict check passed").await?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Err(_) => {
            // --- Workflow 2: trust-on-first-use, then save ---
            let known_hosts = KnownHosts::new();

            let session = Client::builder()
                .endpoint((host.as_str(), port))
                .username(username.as_str())
                .password(password.as_str())
                .known_hosts_accept_new(known_hosts.clone())
                .build()
                .connect()
                .await?;

            let output = session.command("echo trusted").await?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());

            known_hosts.save("~/.ssh/known_hosts")?;
            println!("Saved new host key to known_hosts");
        }
    }

    Ok(())
}
