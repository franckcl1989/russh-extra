//! Remote TCP port forwarding.
//!
//! Requests the SSH server to listen on a remote port and forward connections
//! back to a local destination.
//!
//! Usage:
//! ```bash
//! cargo run --example remote_forward --features <required-features>
//! ```
//!

//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22             (optional)
//!   SSH_USER=deploy
//!   SSH_PASSWORD=...
//!   REMOTE_BIND_PORT=9000   (optional, 0 = let server choose)
//!   LOCAL_HOST=127.0.0.1    (optional, defaults to 127.0.0.1)
//!   LOCAL_PORT=3000         (optional, defaults to 3000)

use std::env;

use russh_extra::{Client, ForwardSpec, TcpEndpoint};

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

    let remote_bind_port: u16 = env::var("REMOTE_BIND_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let local_host = env::var("LOCAL_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let local_port: u16 = env::var("LOCAL_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username)
        .password(password)
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let spec = ForwardSpec::remote_tcp(
        TcpEndpoint::new("0.0.0.0", remote_bind_port),
        TcpEndpoint::new(local_host.as_str(), local_port),
    );

    let tunnel = session.tunnel(spec).start().await?;
    println!(
        "Remote {}:{} -> {}:{}",
        host,
        tunnel.bound_addr().unwrap().port(),
        local_host,
        local_port,
    );
    println!("Press Ctrl-C to stop");

    tokio::signal::ctrl_c().await.ok();
    tunnel.close().await?;
    println!("Remote tunnel closed");

    Ok(())
}
