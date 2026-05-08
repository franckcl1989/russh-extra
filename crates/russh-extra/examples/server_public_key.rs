//! Standalone SSH server with public-key authentication.
//!
//! Starts a server that accepts one authorized public key and runs an
//! `exec` handler. The server runs for 30 seconds then shuts down.
//!
//! Usage:
//! ```bash
//! cargo run --example server_public_key --features server,aws-lc-rs
//! ```
//!
//! Connect with a matching private key:
//! ```bash
//! ssh -i /path/to/private_key admin@127.0.0.1 -p 2222 whoami
//! ```

use russh_extra::{AuthDecision, ExecResponse, Server, ServerHostKey};

#[tokio::main]
async fn main() -> Result<(), russh_extra::BoxError> {
    let host_key = {
        let private_key = russh_extra::russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh_extra::russh::keys::Algorithm::Ed25519,
        )?;
        ServerHostKey::from_private_key(private_key)
    };

    // Generate a client key pair for the example.
    let client_private = russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )?;
    let client_public = client_private.public_key().clone();

    println!(
        "client public key fingerprint: {}",
        client_public.fingerprint(russh_extra::russh::keys::HashAlg::Sha256)
    );

    let server = Server::builder()
        .listen(("127.0.0.1", 2222))
        .host_key(host_key)
        .public_key_auth(move |ctx, key| {
            let authorized = key == client_public;
            async move {
                if authorized && ctx.username().as_str() == "admin" {
                    Ok(AuthDecision::accept())
                } else {
                    Ok(AuthDecision::reject())
                }
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(format!("{}\n", ctx.username()))
                .exit_status(0))
        })
        .build()?;

    println!("server listening on 127.0.0.1:2222");
    println!("accepts public key authentication for user 'admin'");

    let handle = server.handle();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        handle.shutdown("example server timeout");
    });

    server.run().await?;

    Ok(())
}
