//! Tracing instrumentation example.
//!
//! Initializes a tracing subscriber and runs a simple connect-and-exec
//! workflow to demonstrate tracing spans.
//!
//! The example emits TRACE-level events; adjust `RUST_LOG` to control
//! verbosity (e.g. `RUST_LOG=russh_extra=info cargo run --example tracing
//! --features client,aws-lc-rs`).
//!
//! Usage:
//! ```bash
//! cargo run --example tracing --features client,aws-lc-rs
//! ```

use russh_extra::Client;
use russh_extra_test_support::{self as _, CommandResponse, LoopbackServer, LoopbackServerConfig};

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    russh_extra_test_support::init_tracing();

    tracing::info!("starting loopback server");

    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("whoami", CommandResponse::stdout("demo\n")),
    )
    .await?;

    tracing::info!(endpoint = %server.endpoint(), "connecting");

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    tracing::info!("running command");

    let output = session.command("whoami").await?;

    tracing::info!(
        stdout = %String::from_utf8_lossy(&output.stdout),
        exit = ?output.exit,
        "command completed"
    );

    println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}
