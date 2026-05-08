//! Interactive shell example with PTY allocation.
//!
//! Starts a local loopback server that echoes data, opens a shell with
//! PTY, writes a line, reads the echoed response, and closes.
//!
//! Usage:
//! ```bash
//! cargo run --example client_shell --features client,shell,aws-lc-rs
//! ```

use russh_extra::Client;
use russh_extra_test_support::LoopbackServer;
use russh_extra_test_support::LoopbackServerConfig;

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_shell()
            .accept_pty(),
    )
    .await?;

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let pty = russh_extra::Pty::new("xterm-256color", 80, 24);
    let mut handle = session.shell().pty(pty).build().open().await?;

    handle.write_all(b"hello from the shell\n").await?;

    let mut buf = [0u8; 4096];
    let n = handle.read(&mut buf).await?;
    println!("echoed: {}", String::from_utf8_lossy(&buf[..n]));

    handle.close().await?;
    Ok(())
}
