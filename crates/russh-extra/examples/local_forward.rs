//! Local TCP port forwarding.
//!
//! Listens on a local port and forwards connections through the SSH tunnel
//! to a remote destination. A simple TCP listener on the forwarded local port
//! pairs with stdout reporting.
//!
//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22             (optional)
//!   SSH_USER=deploy
//!   SSH_PASSWORD=...
//!   LOCAL_PORT=8080         (optional, defaults to 8080)
//!   REMOTE_HOST=127.0.0.1   (optional, defaults to 127.0.0.1)
//!   REMOTE_PORT=80           (optional, defaults to 80)

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

    let local_port: u16 = env::var("LOCAL_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    let remote_host = env::var("REMOTE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let remote_port: u16 = env::var("REMOTE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username)
        .password(password)
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let spec = ForwardSpec::local_tcp(
        TcpEndpoint::new("127.0.0.1", local_port),
        TcpEndpoint::new(remote_host.as_str(), remote_port),
    );

    let tunnel = session.tunnel(spec).start().await?;
    println!(
        "Forwarding 127.0.0.1:{} -> {}:{}",
        tunnel.bound_addr().unwrap().port(),
        remote_host,
        remote_port,
    );
    println!("Press Ctrl-C to stop");

    tokio::signal::ctrl_c().await.ok();
    tunnel.close().await?;
    println!("Tunnel closed");

    Ok(())
}
