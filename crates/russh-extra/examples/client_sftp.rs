//! Native SFTP v3 client operations.
//!
//! Demonstrates file read, directory listing, file upload, and metadata
//! queries over the SSH subsystem.
//!
//! Usage:
//! ```bash
//! cargo run --example client_sftp --features <required-features>
//! ```
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

    let sftp = session.sftp().await?;

    // Read a remote file
    let contents = sftp.read_to_vec("/etc/hostname").await?;
    println!("hostname: {}", String::from_utf8_lossy(&contents).trim());

    // Stat a file
    let meta = sftp.metadata("/etc/hostname").await?;
    println!("size: {:?}, perms: {:?}", meta.size(), meta.permissions());

    // List a directory
    let mut dir = sftp.opendir("/tmp").await?;
    println!("\n/tmp:");
    while let Some(entry) = sftp.readdir(&mut dir).await? {
        println!("  {}", entry.filename());
    }
    let _ = dir.close().await;

    // Upload a file
    sftp.write_all(
        "/tmp/russh-extra-example.txt",
        b"Hello from russh-extra SFTP client!\n",
    )
    .await?;
    println!("\nUploaded /tmp/russh-extra-example.txt");

    // Remove the uploaded file
    sftp.remove("/tmp/russh-extra-example.txt").await?;
    println!("Removed /tmp/russh-extra-example.txt");

    Ok(())
}
