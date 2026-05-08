//! SSH subsystem example.
//!
//! Opens a named subsystem ("echo") against a local loopback server,
//! sends data and reads the echoed response.
//!
//! Usage:
//! ```bash
//! cargo run --example client_subsystem --features client,shell,aws-lc-rs
//! ```

use russh_extra::Client;
use russh_extra_test_support::LoopbackServer;
use russh_extra_test_support::LoopbackServerConfig;

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("echo"),
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

    let mut handle = session.subsystem("echo").build().open().await?;

    handle.write_all(b"hello from subsystem\n").await?;

    let mut buf = [0u8; 4096];
    let n = handle.read(&mut buf).await?;
    println!("echoed: {}", String::from_utf8_lossy(&buf[..n]));

    handle.close().await?;
    Ok(())
}
