//! Raw SSH subsystem channel opening.
//!
//! Opens the `sftp` subsystem and performs the version negotiation handshake,
//! then closes the channel.
//!
//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22            (optional)
//!   SSH_USER=deploy
//!   SSH_PASSWORD=...

use std::env;

use russh_extra::Client;

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

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username)
        .password(password)
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let sub = session.subsystem("sftp").build().open().await?;
    let _ = sub.write_all(b"raw payload").await;
    sub.close().await?;

    Ok(())
}
