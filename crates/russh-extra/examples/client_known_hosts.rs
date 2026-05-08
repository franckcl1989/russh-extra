//! Demonstrates client connect with known-hosts verification.
//!
//! This example shows both strict verification (rejects unknown hosts) and
//! trust-on-first-use (accepts and persists new keys) against a loopback server.
//!
//! Usage:
//! ```bash
//! cargo run --example client_known_hosts --features client,known-hosts,aws-lc-rs
//! ```

use russh_extra::{Client, KnownHosts};
use russh_extra_test_support::{CommandResponse, LoopbackServer, LoopbackServerConfig};

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("whoami", CommandResponse::stdout("demo\n")),
    )
    .await?;

    // Strict mode: an empty known-hosts store rejects unknown host keys.
    let known_hosts = KnownHosts::new();
    let result = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .known_hosts(known_hosts.clone())
        .build()
        .connect()
        .await;

    match result {
        Err(e) => println!("strict mode rejected unknown host: {e}"),
        Ok(_) => println!("strict mode unexpectedly accepted unknown host"),
    }

    // Trust-on-first-use: accept the unknown key and add it to the store.
    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .known_hosts_accept_new(known_hosts)
        .build()
        .connect()
        .await?;

    let output = session.command("whoami").await?;
    println!(
        "trust-on-first-use succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    println!("exit: {:?}", output.exit);

    Ok(())
}
