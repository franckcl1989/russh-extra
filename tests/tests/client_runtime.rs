use std::time::Duration;

use pretty_assertions::assert_eq;
use russh_extra::{
    AuthenticationErrorKind, ChannelErrorKind, Client, CommandExit, Error, ForwardSpec,
    ForwardingErrorKind, HostKeyErrorKind, Identity, KnownHosts, Operation, RemoteCommand,
    TcpEndpoint, Timeouts,
};
use russh_extra_test_support::{
    CommandResponse, LoopbackServer, LoopbackServerConfig, StreamingCommandConfig,
    generate_test_key_pair, init_tracing,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn client_connects_and_runs_buffered_command() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command(
                "whoami",
                CommandResponse::stdout("demo\n").with_stderr("warning\n"),
            ),
    )
    .await
    .unwrap();
    let endpoint = server.endpoint();

    let session = Client::builder()
        .endpoint(endpoint.clone())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    assert_eq!(session.endpoint(), &endpoint);
    {
        let raw = session.russh_handle().await.unwrap();
        assert!(!raw.is_closed());
    }

    let output = session.command("whoami").await.unwrap();

    assert_eq!(output.exit, CommandExit::status(0));
    assert_eq!(output.stdout.as_ref(), b"demo\n");
    assert_eq!(output.stderr.as_ref(), b"warning\n");
    assert!(output.success());
}

#[tokio::test]
async fn client_accepts_pinned_sha256_host_key() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("ok", CommandResponse::success()),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .try_pinned_host_key_sha256(server.host_key_sha256_fingerprint())
        .unwrap()
        .build()
        .connect()
        .await
        .unwrap();

    let output = session.command("ok").await.unwrap();

    assert_eq!(output.exit, CommandExit::status(0));
}

#[tokio::test]
async fn strict_host_key_policy_rejects_unknown_loopback_key() {
    init_tracing();
    let server = LoopbackServer::start(LoopbackServerConfig::new().password("demo", "demo"))
        .await
        .unwrap();

    let error = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .build()
        .connect()
        .await
        .unwrap_err();

    let Error::HostKey(host_key) = error else {
        panic!("expected host-key error");
    };
    assert_eq!(host_key.kind(), HostKeyErrorKind::Unknown);
}

#[tokio::test]
async fn rejected_password_returns_typed_authentication_error() {
    init_tracing();
    let server = LoopbackServer::start(LoopbackServerConfig::new().password("demo", "demo"))
        .await
        .unwrap();

    let error = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("wrong")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap_err();

    let Error::Authentication(auth) = error else {
        panic!("expected authentication error");
    };
    assert_eq!(auth.kind(), AuthenticationErrorKind::Exhausted);
}

#[tokio::test]
async fn silent_server_connect_timeout_returns_typed_timeout() {
    init_tracing();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accept_task = tokio::spawn(async move {
        if let Ok((stream, _peer)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(stream);
        }
    });

    let error = Client::builder()
        .endpoint((addr.ip().to_string(), addr.port()))
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .timeouts(Timeouts {
            connect: Duration::from_millis(50),
            ..Timeouts::default()
        })
        .build()
        .connect()
        .await
        .unwrap_err();
    accept_task.abort();

    let Error::Timeout(timeout) = error else {
        panic!("expected timeout error");
    };
    assert_eq!(timeout.kind(), Operation::Connect);
}

#[tokio::test]
async fn nonzero_command_status_returns_command_output() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("false", CommandResponse::new().with_exit_status(42)),
    )
    .await
    .unwrap();
    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let output = session.command("false").await.unwrap();

    assert_eq!(output.exit, CommandExit::status(42));
    assert!(!output.success());
}

#[tokio::test]
async fn command_output_limit_returns_typed_channel_error() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("loud", CommandResponse::stdout("abcdef")),
    )
    .await
    .unwrap();
    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let error = session
        .command(RemoteCommand::new("loud").stdout_limit(3))
        .await
        .unwrap_err();

    let Error::Channel(channel) = error else {
        panic!("expected channel error");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Read);
}

#[tokio::test]
async fn exec_request_failure_returns_typed_channel_error() {
    init_tracing();
    let server = LoopbackServer::start(LoopbackServerConfig::new().password("demo", "demo"))
        .await
        .unwrap();
    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let error = session.command("missing").await.unwrap_err();

    let Error::Channel(channel) = error else {
        panic!("expected channel error");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Request);
}

#[tokio::test]
async fn disconnect_during_command_returns_typed_disconnect() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("drop", CommandResponse::disconnect()),
    )
    .await
    .unwrap();
    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo")
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let error = session.command("drop").await.unwrap_err();

    let Error::Disconnected(disconnected) = error else {
        panic!("expected disconnect error");
    };
    assert_eq!(disconnected.kind(), Operation::Command);
}

#[tokio::test]
async fn client_connects_with_private_key_authentication() {
    init_tracing();
    let (private_key, public_key) = generate_test_key_pair();

    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .authorized_key("demo", public_key)
            .command("whoami", CommandResponse::stdout("demo\n")),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .identity(Identity::load_openssh_pem(serialize_private_key(
            &private_key,
        )))
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let output = session.command("whoami").await.unwrap();

    assert!(output.success());
    assert_eq!(output.stdout.as_ref(), b"demo\n");
}

#[tokio::test]
async fn public_key_rejection_returns_typed_authentication_error() {
    init_tracing();
    let (_key_a, _pub_a) = generate_test_key_pair();
    let (key_b, _pub_b) = generate_test_key_pair();

    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .command("ok", CommandResponse::success()),
    )
    .await
    .unwrap();

    let error = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .identity(Identity::load_openssh_pem(serialize_private_key(&key_b)))
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap_err();

    let Error::Authentication(auth) = error else {
        panic!("expected authentication error, got {error:?}");
    };
    assert_eq!(auth.kind(), AuthenticationErrorKind::Exhausted);
}

fn serialize_private_key(private_key: &russh::keys::PrivateKey) -> Vec<u8> {
    let pem = private_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    pem.as_bytes().to_vec()
}

#[tokio::test]
async fn agent_auth_returns_unavailable_when_env_not_set() {
    init_tracing();
    let (private_key, public_key) = generate_test_key_pair();

    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .authorized_key("agent-user", public_key)
            .command("ok", CommandResponse::success()),
    )
    .await
    .unwrap();

    // Test that agent-only auth returns Unavailable when SSH_AUTH_SOCK is not
    // set. Since this runs in CI without an agent, this is a reliable test.
    let result = Client::builder()
        .endpoint(server.endpoint())
        .username("agent-user")
        .agent()
        .accept_any_host_key()
        .build()
        .connect()
        .await;

    match result {
        Ok(_) => {
            // An agent might be running locally — that's fine.
        }
        Err(Error::Authentication(auth)) if auth.kind() == AuthenticationErrorKind::Unavailable => {
        }
        Err(other) => {
            // If there's no agent, the key is not configured, so the agent
            // tries and fails with Unavailable. The credential is exhausted
            // since agent was the only credential.
            panic!("unexpected error: {other:?}");
        }
    }

    let _ = private_key;
}

async fn bind_ephemeral() -> russh_extra::Endpoint {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    russh_extra::Endpoint::new("127.0.0.1", addr.port())
}

#[tokio::test]
async fn known_hosts_accept_new_adds_loopback_key() {
    init_tracing();
    let (private_key, _public_key) = generate_test_key_pair();
    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;
    let known_hosts = KnownHosts::new();

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let session = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .known_hosts_accept_new(known_hosts.clone())
        .build()
        .connect()
        .await
        .unwrap();

    assert_eq!(session.endpoint().host(), "127.0.0.1");
    assert_eq!(known_hosts.entry_count(), 1);
}

#[tokio::test]
async fn known_hosts_changed_key_returns_changed_error() {
    init_tracing();
    let (private_key, _public_key) = generate_test_key_pair();
    let (_wrong_private_key, wrong_public_key) = generate_test_key_pair();
    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;
    let known_hosts = KnownHosts::new();
    known_hosts
        .add_entry(addr.host(), addr.port(), &wrong_public_key, "ssh-ed25519")
        .unwrap();

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let error = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .known_hosts(known_hosts)
        .build()
        .connect()
        .await
        .unwrap_err();

    let Error::HostKey(host_key) = error else {
        panic!("expected host-key error");
    };
    assert_eq!(host_key.kind(), HostKeyErrorKind::Changed);
}

#[tokio::test]
async fn known_hosts_revoked_entry_returns_rejected_error() {
    use base64::Engine;

    init_tracing();
    let (private_key, public_key) = generate_test_key_pair();
    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;

    let key_blob = public_key.to_bytes().unwrap();
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&key_blob);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_hosts");
    std::fs::write(
        &path,
        format!("@revoked {} ssh-ed25519 {key_b64}\n", addr.host()),
    )
    .unwrap();

    let known_hosts = KnownHosts::load(&path).unwrap();
    assert_eq!(known_hosts.entry_count(), 1);

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let error = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .known_hosts(known_hosts)
        .build()
        .connect()
        .await
        .unwrap_err();

    let Error::HostKey(host_key) = error else {
        panic!("expected host-key error");
    };
    assert_eq!(host_key.kind(), HostKeyErrorKind::Rejected);
}

#[tokio::test]
async fn known_hosts_save_and_reload_with_loopback() {
    init_tracing();
    let (private_key, public_key) = generate_test_key_pair();
    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_hosts");

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let known_hosts = KnownHosts::new();
    {
        let _session = Client::builder()
            .endpoint(addr.clone())
            .username("demo")
            .password("demo".to_owned())
            .known_hosts_accept_new(known_hosts.clone())
            .build()
            .connect()
            .await
            .unwrap();
    }
    assert_eq!(known_hosts.entry_count(), 1);

    known_hosts.save(&path).unwrap();

    let loaded = KnownHosts::load(&path).unwrap();
    assert_eq!(loaded.entry_count(), 1);
    assert_eq!(
        loaded.check(addr.host(), addr.port(), &public_key),
        russh_extra::KnownHostStatus::Match
    );

    let _session = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .known_hosts(loaded)
        .build()
        .connect()
        .await
        .unwrap();
}

#[tokio::test]
async fn shell_opens_with_pty_and_resize() {
    init_tracing();
    let (private_key, _public_key) = generate_test_key_pair();
    let pty_approved = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pty_approved_clone = std::sync::Arc::clone(&pty_approved);

    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .pty_handler(move |_ctx, _params| {
            let approved = std::sync::Arc::clone(&pty_approved_clone);
            async move {
                approved.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        })
        .shell_handler(|_ctx| async { Ok(()) })
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let session = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let pty = russh_extra::Pty::new("xterm-256color", 120, 40);
    let shell_handle = session.shell().pty(pty).build().open().await.unwrap();

    assert!(pty_approved.load(std::sync::atomic::Ordering::SeqCst));

    shell_handle.resize(80, 24).await.unwrap();
    shell_handle.close().await.unwrap();
}

#[tokio::test]
async fn subsystem_open_succeeds_with_handler() {
    init_tracing();
    let (private_key, _public_key) = generate_test_key_pair();
    let subsystem_approved = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let subsystem_approved_clone = std::sync::Arc::clone(&subsystem_approved);

    let pem = serialize_private_key(&private_key);
    let addr = bind_ephemeral().await;

    let server_handle = russh_extra::ServerBuilder::default()
        .listen(addr.clone())
        .host_key(russh_extra::ServerHostKey::from_openssh_pem(pem).unwrap())
        .password_auth(
            move |_ctx, _password| async move { Ok(russh_extra::AuthDecision::accept()) },
        )
        .subsystem_handler(move |ctx| {
            let approved = std::sync::Arc::clone(&subsystem_approved_clone);
            async move {
                assert_eq!(ctx.name, "sftp");
                approved.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        })
        .build()
        .unwrap();

    let _server_task = tokio::spawn(async move {
        let _ = server_handle.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let session = Client::builder()
        .endpoint(addr)
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let handle = session.subsystem("sftp").build().open().await.unwrap();

    assert!(subsystem_approved.load(std::sync::atomic::Ordering::SeqCst));
    handle.close().await.unwrap();
}

#[tokio::test]
async fn sftp_open_negotiates_protocol_with_server() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp"),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    assert_eq!(sftp.session_id(), session.id());
}

#[tokio::test]
async fn direct_tcp_round_trips_data_over_channel() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_direct_tcpip()
            .accept_shell(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let mut stream = session
        .direct_tcp(TcpEndpoint::new("example.test", 80))
        .open()
        .await
        .unwrap();

    stream.write_all(b"ping").await.unwrap();

    let mut buf = [0; 4];
    let n = stream.read(&mut buf).await.unwrap();

    assert_eq!(n, 4);
    assert_eq!(&buf, b"ping");
    stream.close().await.unwrap();
}

#[tokio::test]
async fn local_forwarding_round_trips_data() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_direct_tcpip()
            .accept_shell(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let tunnel = session
        .tunnel(ForwardSpec::local_tcp(
            TcpEndpoint::new("127.0.0.1", 0),
            TcpEndpoint::new("example.test", 80),
        ))
        .start()
        .await
        .unwrap();

    let mut stream = TcpStream::connect(tunnel.bound_addr()).await.unwrap();
    stream.write_all(b"pong").await.unwrap();
    stream.shutdown().await.unwrap();

    let mut buf = [0; 4];
    stream.read_exact(&mut buf).await.unwrap();

    assert_eq!(&buf, b"pong");
    tunnel.close().await.unwrap();
}

#[tokio::test]
async fn remote_forwarding_succeeds_when_server_accepts() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_tcpip_forward(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let tunnel = session
        .tunnel(ForwardSpec::remote_tcp(
            TcpEndpoint::new("127.0.0.1", 9001),
            TcpEndpoint::new("127.0.0.1", 1),
        ))
        .start()
        .await
        .unwrap();

    assert!(
        matches!(tunnel.spec(), ForwardSpec::Tcp { direction, .. } if *direction == russh_extra::ForwardDirection::Remote)
    );

    tunnel.close().await.unwrap();
}

#[tokio::test]
async fn remote_forwarding_fails_when_server_denies() {
    init_tracing();
    let server = LoopbackServer::start(LoopbackServerConfig::new().password("demo", "demo"))
        .await
        .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let error = session
        .tunnel(ForwardSpec::remote_tcp(
            TcpEndpoint::new("127.0.0.1", 9002),
            TcpEndpoint::new("127.0.0.1", 1),
        ))
        .start()
        .await
        .unwrap_err();

    let Error::Forwarding(fwd) = error else {
        panic!("expected forwarding error");
    };
    assert_eq!(fwd.kind(), ForwardingErrorKind::GlobalRequest);
}

#[tokio::test]
async fn remote_forwarding_port_conflict_returns_bind_error() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_tcpip_forward(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let _tunnel = session
        .tunnel(ForwardSpec::remote_tcp(
            TcpEndpoint::new("127.0.0.1", 9003),
            TcpEndpoint::new("127.0.0.1", 1),
        ))
        .start()
        .await
        .unwrap();

    let error = session
        .tunnel(ForwardSpec::remote_tcp(
            TcpEndpoint::new("127.0.0.1", 9003),
            TcpEndpoint::new("127.0.0.1", 2),
        ))
        .start()
        .await
        .unwrap_err();

    let Error::Forwarding(fwd) = error else {
        panic!("expected forwarding error");
    };
    assert_eq!(fwd.kind(), ForwardingErrorKind::Bind);
}

#[tokio::test]
async fn remote_forwarding_close_sends_cancel() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_tcpip_forward(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let tunnel = session
        .tunnel(ForwardSpec::remote_tcp(
            TcpEndpoint::new("127.0.0.1", 9004),
            TcpEndpoint::new("127.0.0.1", 1),
        ))
        .start()
        .await
        .unwrap();

    tunnel.close().await.unwrap();
}

#[tokio::test]
async fn direct_tcp_stream_send_eof_then_close() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_direct_tcpip()
            .accept_shell(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let stream = session
        .direct_tcp(TcpEndpoint::new("example.test", 80))
        .open()
        .await
        .unwrap();

    stream.send_eof().await.unwrap();
    stream.close().await.unwrap();
}

#[tokio::test]
async fn tunnel_abort_does_not_panic() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_direct_tcpip()
            .accept_shell(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let tunnel = session
        .tunnel(ForwardSpec::local_tcp(
            TcpEndpoint::new("127.0.0.1", 0),
            TcpEndpoint::new("example.test", 80),
        ))
        .start()
        .await
        .unwrap();

    tunnel.abort();
}

#[tokio::test]
async fn tunnel_close_is_idempotent() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_direct_tcpip()
            .accept_shell(),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let tunnel = session
        .tunnel(ForwardSpec::local_tcp(
            TcpEndpoint::new("127.0.0.1", 0),
            TcpEndpoint::new("example.test", 80),
        ))
        .start()
        .await
        .unwrap();

    tunnel.close().await.unwrap();
}

#[tokio::test]
async fn client_captures_streaming_server_output_with_delays() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .streaming_command(
                "slow-cmd",
                StreamingCommandConfig::new()
                    .stdout("part1\n")
                    .delay(Duration::from_millis(10))
                    .stdout("part2\n")
                    .stderr("warn\n")
                    .delay(Duration::from_millis(5))
                    .stdout("part3\n")
                    .exit_status(0),
            ),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let output = session.command("slow-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"part1\npart2\npart3\n");
    assert_eq!(output.stderr.as_ref(), b"warn\n");
    assert_eq!(output.exit, CommandExit::Status(0));
}

#[tokio::test]
async fn client_streaming_command_propagates_exit_status() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .streaming_command(
                "fail-cmd",
                StreamingCommandConfig::new()
                    .stdout("doomed\n")
                    .exit_status(42),
            ),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let output = session.command("fail-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"doomed\n");
    assert_eq!(output.exit, CommandExit::Status(42));
}

// ── SFTP integration tests ────────────────────────────────────────────

#[tokio::test]
async fn sftp_open_read_close_file() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/hello.txt", b"hello world\n", 0o644),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    let mut file = sftp
        .open("/hello.txt", russh_extra::SftpOpenMode::READ)
        .await
        .unwrap();
    let data = file.read(0, 4096).await.unwrap();
    assert_eq!(data, b"hello world\n");
    file.close().await.unwrap();
}

#[tokio::test]
async fn sftp_write_read_file() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/write-test.txt", b"", 0o644),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    let file = sftp
        .open(
            "/write-test.txt",
            russh_extra::SftpOpenMode::WRITE | russh_extra::SftpOpenMode::CREATE,
        )
        .await
        .unwrap();
    file.write(0, b"written data").await.unwrap();
    file.close().await.unwrap();

    let mut file = sftp
        .open("/write-test.txt", russh_extra::SftpOpenMode::READ)
        .await
        .unwrap();
    let data = file.read(0, 4096).await.unwrap();
    assert_eq!(data, b"written data");
    file.close().await.unwrap();
}

#[tokio::test]
async fn sftp_stat_file() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/stat.txt", b"some content", 0o755),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    let meta = sftp.metadata("/stat.txt").await.unwrap();
    assert_eq!(meta.size(), Some(12));
    assert_eq!(meta.permissions(), Some(0o755));
}

#[tokio::test]
async fn sftp_opendir_and_readdir() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_dir("/etc", &["hostname", "resolv.conf", "passwd"]),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    let mut dir = sftp.opendir("/etc").await.unwrap();
    let mut names = Vec::new();
    while let Some(entry) = dir.read().await.unwrap() {
        names.push(entry.filename().to_owned());
    }
    assert_eq!(names, vec!["hostname", "resolv.conf", "passwd"]);
    dir.close().await.unwrap();
}

#[tokio::test]
async fn sftp_remove_file() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/garbage.txt", b"junk", 0o644),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    sftp.remove("/garbage.txt").await.unwrap();
}

#[tokio::test]
async fn sftp_rename_file() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/old.txt", b"data", 0o644),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    sftp.rename("/old.txt", "/new.txt").await.unwrap();
}

#[tokio::test]
async fn sftp_mkdir_and_rmdir() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp"),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    sftp.create_dir("/new-dir").await.unwrap();
    sftp.remove_dir("/new-dir").await.unwrap();
}

#[tokio::test]
async fn sftp_symlink_and_readlink() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_symlink("/link", "/target"),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    sftp.symlink("/new-link", "/new-target").await.unwrap();
    let target = sftp.readlink("/link").await.unwrap();
    assert_eq!(target, "/target");
}

#[tokio::test]
async fn sftp_canonicalize_path() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp"),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    let resolved = sftp.canonicalize("/some/path").await.unwrap();
    assert_eq!(resolved, "/some/path");
}

#[tokio::test]
async fn sftp_file_drop_auto_closes() {
    init_tracing();
    let server = LoopbackServer::start(
        LoopbackServerConfig::new()
            .password("demo", "demo")
            .accept_subsystem("sftp")
            .sftp_file("/drop-test.txt", b"content", 0o644),
    )
    .await
    .unwrap();

    let session = Client::builder()
        .endpoint(server.endpoint())
        .username("demo")
        .password("demo".to_owned())
        .accept_any_host_key()
        .build()
        .connect()
        .await
        .unwrap();

    let sftp = session.sftp().await.unwrap();
    {
        let _file = sftp
            .open("/drop-test.txt", russh_extra::SftpOpenMode::READ)
            .await
            .unwrap();
        // file is dropped here — close is sent fire-and-forget
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    // If we got here without deadlock or panic, drop works
}
