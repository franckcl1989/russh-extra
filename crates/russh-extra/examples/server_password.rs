//! SSH server with password authentication and buffered exec routing.
//!
//! Starts a server that authenticates a single user and responds to a fixed
//! set of commands. Runs until Ctrl-C.
//!
//! Requires:
//!   LISTEN_ADDR=127.0.0.1:2222  (optional, defaults to 127.0.0.1:2222)

use russh_extra::russh::keys::{Algorithm, PrivateKey};
use russh_extra::{
    AuthContext, AuthDecision, ExecContext, ExecResponse, Server, ServerHostKey, TransportErrorKind,
};
use std::env;

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("russh_extra=debug")
        .init();

    let listen = env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:2222".into());

    let host_key = ServerHostKey::from_private_key(
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(|e| {
            russh_extra::Error::transport_with_source(
                TransportErrorKind::Other,
                "generate host key",
                e,
            )
        })?,
    );

    let listen_addr: std::net::SocketAddr = listen.parse().unwrap();

    let server = Server::builder()
        .listen((listen_addr.ip().to_string().as_str(), listen_addr.port()))
        .host_key(host_key)
        .password_auth(|ctx: AuthContext, password| async move {
            if ctx.username().as_str() == "admin" && password.expose_secret() == "secret" {
                Ok(AuthDecision::accept())
            } else {
                Ok(AuthDecision::reject())
            }
        })
        .exec("whoami", |ctx: ExecContext| async move {
            Ok(ExecResponse::success()
                .stdout(format!("{}\n", ctx.username()))
                .exit_status(0))
        })
        .exec("hostname", |_: ExecContext| async move {
            Ok(ExecResponse::success()
                .stdout(b"localhost\n".as_slice())
                .exit_status(0))
        })
        .build()?;

    println!("Server listening on {listen} (admin:secret)");
    server
        .run_until(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}
