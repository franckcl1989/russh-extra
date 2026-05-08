//! Interactive shell with PTY allocation.
//!
//! Opens a remote shell, sends a command, reads the response, then closes.
//!
//! Requires:
//!   SSH_HOST=example.com
//!   SSH_PORT=22            (optional)
//!   SSH_USER=deploy
//!   SSH_PASSWORD=...

use std::env;

use russh_extra::{Client, Pty};

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

    let session = Client::builder()
        .endpoint((host.as_str(), port))
        .username(username)
        .password(password)
        .accept_any_host_key()
        .build()
        .connect()
        .await?;

    let mut shell = session
        .shell()
        .pty(Pty::new("xterm-256color", 120, 40))
        .env("LANG", "C.UTF-8")
        .build()
        .open()
        .await?;

    shell.write_all(b"uname -a\nexit\n").await?;

    let mut buf = vec![0u8; 4096];
    loop {
        let n = shell.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        eprint!("{}", String::from_utf8_lossy(&buf[..n]));
    }

    shell.close().await?;
    Ok(())
}
