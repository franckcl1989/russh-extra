//! SSH server with public key authentication.
//!
//! Starts a server that accepts a single pre-authorized public key. All other
//! authenticating users are rejected.
//!
//! Usage:
//! ```bash
//! cargo run --example server_public_key --features <required-features>
//! ```
//!

//! Requires:
//!   LISTEN_ADDR=127.0.0.1:2222  (optional, defaults to 127.0.0.1:2222)
//!   AUTHORIZED_KEY_PATH=~/.ssh/id_ed25519.pub  (path to authorized public key)

use russh_extra::russh::keys::ssh_key::PublicKey;
use russh_extra::russh::keys::{Algorithm, PrivateKey};
use russh_extra::{
    AuthDecision, ExecContext, ExecResponse, Server, ServerHostKey, TransportErrorKind,
};
use std::env;

#[tokio::main]
async fn main() -> russh_extra::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("russh_extra=debug")
        .init();

    let listen = env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:2222".into());
    let authorized_key_path =
        env::var("AUTHORIZED_KEY_PATH").unwrap_or_else(|_| "~/.ssh/id_ed25519.pub".into());

    let authorized_key = read_authorized_key(&authorized_key_path)?;

    let host_key = ServerHostKey::from_private_key(
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(|e| {
            russh_extra::Error::transport_with_source(
                TransportErrorKind::Other,
                "generate host key",
                e,
            )
        })?,
    );

    let authorized_key_clone = authorized_key.clone();
    let listen_addr: std::net::SocketAddr = listen.parse().unwrap();

    let server = Server::builder()
        .listen((listen_addr.ip().to_string().as_str(), listen_addr.port()))
        .host_key(host_key)
        .public_key_auth(move |_username, key: PublicKey| {
            let authorized = authorized_key_clone.clone();
            async move {
                if key.fingerprint(Default::default()) == authorized.fingerprint(Default::default())
                {
                    Ok(AuthDecision::accept())
                } else {
                    Ok(AuthDecision::reject())
                }
            }
        })
        .exec("whoami", |ctx: ExecContext| async move {
            Ok(ExecResponse::success()
                .stdout(format!("{}\n", ctx.username()))
                .exit_status(0))
        })
        .build()?;

    println!("Server listening on {listen}");
    println!("Authorized key: {authorized_key_path}");
    server
        .run_until(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

fn read_authorized_key(path: &str) -> russh_extra::Result<PublicKey> {
    let expanded = expand_tilde(path);
    let data = std::fs::read_to_string(&expanded).map_err(|e| {
        russh_extra::Error::transport_with_source(TransportErrorKind::Io, "read authorized key", e)
    })?;
    let line = data
        .lines()
        .next()
        .ok_or_else(|| russh_extra::Error::invalid_config("authorized key file is empty"))?;
    PublicKey::from_openssh(line).map_err(|e| {
        russh_extra::Error::transport_with_source(
            TransportErrorKind::Other,
            "parse authorized key",
            e,
        )
    })
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(stripped)
    } else if path == "~" {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
    } else {
        std::path::PathBuf::from(path)
    }
}
