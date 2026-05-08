//! Streaming exec server example.
//!
//! Starts a server with password authentication and streaming command handlers.
//! The server runs for 30 seconds then shuts down gracefully.
//!
//! Usage:
//! ```bash
//! cargo run --example server_streaming_exec --features server,aws-lc-rs
//! ```
//!
//! Connect with any SSH client:
//! ```bash
//! ssh admin@127.0.0.1 -p 2222 echo-me
//! ```
//! Password: `secret`

use russh_extra::{AuthDecision, Server, ServerHostKey};

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
        .streaming_exec("echo-me", |mut ctx| async move {
            while let Some(chunk) = ctx.read_stdin().await {
                ctx.stderr(format!("received {} bytes\n", chunk.len()))
                    .await
                    .unwrap();
                ctx.stdout(chunk).await.unwrap();
            }
            ctx.exit_status(0).await.unwrap();
            Ok(())
        })
        .streaming_exec("progress", |mut ctx| async move {
            for i in 1..=5 {
                let msg = format!("step {i}\n");
                ctx.stdout(msg).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            ctx.exit_status(0).await.unwrap();
            Ok(())
        })
        .streaming_exec("fail-late", |mut ctx| async move {
            ctx.stdout("doomed\n").await.unwrap();
            ctx.exit_status(1).await.unwrap();
            Ok(())
        })
        .build()?;

    println!("server listening on 127.0.0.1:2222");
    println!("credentials: admin / secret");
    println!("try: ssh admin@127.0.0.1 -p 2222 echo-me  (echo stdin)");
    println!("try: ssh admin@127.0.0.1 -p 2222 progress  (5-step progress)");
    println!("try: ssh admin@127.0.0.1 -p 2222 fail-late (exit 1)");

    let handle = server.handle();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        handle.shutdown("example server timeout");
    });

    server.run().await?;
    Ok(())
}
