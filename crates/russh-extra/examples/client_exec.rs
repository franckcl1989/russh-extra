//! Demonstrates client connect, pinned host-key verification, password
//! authentication, and buffered command execution.
//!
//! This example runs against a local loopback SSH server so it works without
//! external SSH hosts or configuration.

use russh_extra::Client;
use russh_extra_test_support::{CommandResponse, LoopbackServer, LoopbackServerConfig};

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("whoami", CommandResponse::stdout("demo\n")),
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

    let output = session.command("whoami").await?;

    println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("exit: {:?}", output.exit);

    assert!(output.success());

    Ok(())
}
