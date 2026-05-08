//! Standalone SSH server example.
//!
//! Starts a server with password authentication and exact command routing.
//! The server runs for 30 seconds then shuts down gracefully.
//!
//! Usage:
//! ```bash
//! cargo run --example server_exec --features server,aws-lc-rs
//! ```
//!
//! Connect with any SSH client:
//! ```bash
//! ssh admin@127.0.0.1 -p 2222
//! ```
//! Password: `secret`

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

    let server = Server::builder()
        .listen(("127.0.0.1", 2222))
        .host_key(host_key)
        .password_auth(|ctx, password| async move {
            if ctx.username().as_str() == "admin" && password.expose_secret() == "secret" {
                Ok(AuthDecision::accept())
            } else {
                Ok(AuthDecision::reject())
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(format!("{}\n", ctx.username()))
                .exit_status(0))
        })
        .exec("uptime", |_ctx| async move {
            Ok(ExecResponse::success()
                .stdout("up 3 days, 2 hours\n")
                .exit_status(0))
        })
        .build()?;

    println!("server listening on 127.0.0.1:2222");
    println!("credentials: admin / secret");
    println!("try: ssh admin@127.0.0.1 -p 2222");

    let handle = server.handle();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        handle.shutdown("example server timeout");
    });

    server.run().await?;

    Ok(())
}
