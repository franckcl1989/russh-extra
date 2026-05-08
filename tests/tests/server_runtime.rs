use std::sync::Arc;
use std::time::Duration;

use pretty_assertions::assert_eq;
use russh::ChannelMsg;
use russh::client;
use russh_extra::{
    AuthDecision, ChannelErrorKind, Client, CommandExit, Endpoint, Error, ExecResponse, Identity,
    RemoteCommand, Server, ServerHandle, ServerHostKey, Session,
};
use russh_extra_test_support::{generate_test_key_pair, init_tracing};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

fn test_host_key() -> ServerHostKey {
    let private_key = russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )
    .unwrap();

    ServerHostKey::from_private_key(private_key)
}

struct RawAcceptAnyClient;

impl client::Handler for RawAcceptAnyClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn retry_raw_connect(
    addr: std::net::SocketAddr,
) -> russh_extra::Result<client::Handle<RawAcceptAnyClient>> {
    let mut last_error = None;

    for _ in 0..20 {
        match client::connect(
            Arc::new(client::Config::default()),
            addr,
            RawAcceptAnyClient,
        )
        .await
        {
            Ok(handle) => return Ok(handle),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }

    Err(russh_extra::Error::transport(
        russh_extra::TransportErrorKind::TcpConnect,
        format!("raw connect failed: {}", last_error.unwrap()),
    ))
}

async fn retry_client_connect(client: &Client) -> russh_extra::Result<Session> {
    let mut last_error = None;

    for _ in 0..20 {
        match client.connect().await {
            Ok(session) => return Ok(session),
            Err(error) => {
                if matches!(error, russh_extra::Error::Transport(_)) {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                } else {
                    return Err(error);
                }
            }
        }
    }

    Err(russh_extra::Error::transport(
        russh_extra::TransportErrorKind::TcpConnect,
        format!("client connect failed: {}", last_error.unwrap()),
    ))
}

async fn unused_endpoint() -> Endpoint {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    Endpoint::new(addr.ip().to_string(), addr.port())
}

async fn connect_client(endpoint: &Endpoint, password: &str) -> russh_extra::Result<Session> {
    let mut last_transport_error = None;

    for _ in 0..20 {
        let result = Client::builder()
            .endpoint(endpoint.clone())
            .username("demo")
            .password(password)
            .accept_any_host_key()
            .build()
            .connect()
            .await;

        match result {
            Ok(session) => return Ok(session),
            Err(error @ Error::Transport(_)) => {
                last_transport_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_transport_error.unwrap_or_else(|| {
        Error::transport(
            russh_extra::TransportErrorKind::TcpConnect,
            "server did not start",
        )
    }))
}

async fn stop_server(handle: ServerHandle, task: JoinHandle<russh_extra::Result<()>>) {
    handle.shutdown("test complete");
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();

    result.unwrap();
}

#[tokio::test]
async fn server_authenticates_and_routes_buffered_command() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|ctx, password| async move {
            if ctx.username().as_str() == "demo" && password.expose_secret() == "demo" {
                Ok(AuthDecision::accept())
            } else {
                Ok(AuthDecision::reject())
            }
        })
        .exec("whoami", |ctx| async move {
            assert_eq!(ctx.username().as_str(), "demo");
            assert_eq!(ctx.command().as_str(), Some("whoami"));
            Ok(ExecResponse::success()
                .stdout("demo\n")
                .stderr("warning\n")
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("whoami").await.unwrap();

    assert_eq!(output.exit, CommandExit::status(0));
    assert_eq!(output.stdout.as_ref(), b"demo\n");
    assert_eq!(output.stderr.as_ref(), b"warning\n");

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_rejects_wrong_password() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, password| async move {
            if password.expose_secret() == "demo" {
                Ok(AuthDecision::accept())
            } else {
                Ok(AuthDecision::reject())
            }
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let error = connect_client(&endpoint, "wrong").await.unwrap_err();

    let Error::Authentication(auth) = error else {
        panic!("expected authentication error");
    };
    assert_eq!(auth.kind(), russh_extra::AuthenticationErrorKind::Exhausted);

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_rejects_unknown_command() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("known", |_ctx| async {
            Ok(ExecResponse::success().stdout("ok\n").exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let error = session.command("missing").await.unwrap_err();

    let Error::Channel(channel) = error else {
        panic!("expected channel error");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Request);

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_rejects_exec_when_command_not_configured() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let error = session.command("anything").await.unwrap_err();

    let Error::Channel(channel) = error else {
        panic!("expected channel error");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Request);

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_authorization_rejects_authenticated_user() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("admin_only", |ctx| async move {
            if ctx.username().as_str() == "admin" {
                Ok(ExecResponse::success().stdout("ok\n").exit_status(0))
            } else {
                Ok(ExecResponse::reject())
            }
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let error = session.command("admin_only").await.unwrap_err();

    let Error::Channel(channel) = error else {
        panic!("expected channel error, got {error:?}");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Request);

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_handles_concurrent_clients() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("ping", |_ctx| async {
            Ok(ExecResponse::success().stdout("pong\n").exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let client_count = 4;
    let mut handles = Vec::with_capacity(client_count);

    for _ in 0..client_count {
        let endpoint = endpoint.clone();
        handles.push(tokio::spawn(async move {
            connect_client(&endpoint, "demo").await.unwrap()
        }));
    }

    let sessions: Vec<Session> = {
        let mut sessions = Vec::with_capacity(client_count);
        for handle in handles {
            sessions.push(handle.await.unwrap());
        }
        sessions
    };

    let mut command_handles = Vec::new();
    for session in sessions {
        command_handles.push(tokio::spawn(async move {
            let output = session.command("ping").await.unwrap();
            assert!(output.success());
            assert_eq!(output.stdout.as_ref(), b"pong\n");
        }));
    }

    for handle in command_handles {
        handle.await.unwrap();
    }

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_rejects_all_authentication_by_default() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let error = connect_client(&endpoint, "demo").await.unwrap_err();

    let Error::Authentication(auth) = error else {
        panic!("expected authentication error, got {error:?}");
    };
    assert_eq!(auth.kind(), russh_extra::AuthenticationErrorKind::Exhausted);

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_shutdown_while_command_in_flight() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("slow", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(ExecResponse::success().stdout("done\n").exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let command_handle = tokio::spawn(async move { session.command("slow").await });

    tokio::time::sleep(Duration::from_millis(10)).await;
    handle.shutdown("maintenance");

    let result = command_handle.await.unwrap();
    assert!(result.is_err());

    let _ = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn server_rejects_exec_request_before_authentication() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("whoami", |_ctx| async {
            Ok(ExecResponse::success().stdout("demo\n").exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = tokio::net::lookup_host(endpoint.to_string())
        .await
        .unwrap()
        .next()
        .unwrap();
    let raw = retry_raw_connect(addr).await.unwrap();

    let result = raw.channel_open_session().await;
    assert!(result.is_err());

    let _ = raw
        .disconnect(russh::Disconnect::ByApplication, "", "")
        .await;

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_handles_multiple_channels_per_connection() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .exec("id", |_ctx| async {
            Ok(ExecResponse::success().stdout("ok\n").exit_status(0))
        })
        .max_sessions(8)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = tokio::net::lookup_host(endpoint.to_string())
        .await
        .unwrap()
        .next()
        .unwrap();
    let mut raw = retry_raw_connect(addr).await.unwrap();
    assert!(
        raw.authenticate_password("demo", "demo")
            .await
            .unwrap()
            .success()
    );

    for i in 0..3 {
        let mut channel = raw.channel_open_session().await.unwrap();
        channel.exec(true, "id").await.unwrap();

        let mut stdout = Vec::new();
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::Close => break,
                _ => {}
            }
        }

        assert_eq!(stdout, b"ok\n", "channel {i}");

        let _ = channel.close().await;
    }

    let _ = raw
        .disconnect(russh::Disconnect::ByApplication, "", "")
        .await;

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_public_key_auth_callback_accepts_key() {
    init_tracing();
    let (private_key, public_key) = generate_test_key_pair();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .public_key_auth(move |_ctx, key| {
            let expected = public_key.clone();
            async move {
                if key == expected {
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
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client_with_key(&endpoint, "key-user", &private_key)
        .await
        .unwrap();

    let output = session.command("whoami").await.unwrap();

    assert!(output.success());
    assert_eq!(output.stdout.as_ref(), b"key-user\n");

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_public_key_auth_rejects_wrong_key() {
    init_tracing();
    let (_key_a, public_key_a) = generate_test_key_pair();
    let (key_b, _public_key_b) = generate_test_key_pair();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .public_key_auth(move |_ctx, key| {
            let expected = public_key_a.clone();
            async move {
                if key == expected {
                    Ok(AuthDecision::accept())
                } else {
                    Ok(AuthDecision::reject())
                }
            }
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let error = connect_client_with_key(&endpoint, "key-user", &key_b)
        .await
        .unwrap_err();

    let Error::Authentication(auth) = error else {
        panic!("expected authentication error, got {error:?}");
    };
    assert_eq!(auth.kind(), russh_extra::AuthenticationErrorKind::Exhausted);

    stop_server(handle, task).await;
}

async fn connect_client_with_key(
    endpoint: &Endpoint,
    username: &str,
    private_key: &russh::keys::PrivateKey,
) -> russh_extra::Result<Session> {
    let mut last_error = None;

    for _ in 0..20 {
        let result = Client::builder()
            .endpoint(endpoint.clone())
            .username(username)
            .identity(Identity::load_openssh_pem(serialize_private_key(
                private_key,
            )))
            .accept_any_host_key()
            .build()
            .connect()
            .await;

        match result {
            Ok(session) => return Ok(session),
            Err(error @ Error::Transport(_)) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        Error::transport(
            russh_extra::TransportErrorKind::TcpConnect,
            "server did not start",
        )
    }))
}

fn serialize_private_key(private_key: &russh::keys::PrivateKey) -> Vec<u8> {
    let pem = private_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    pem.as_bytes().to_vec()
}

#[tokio::test]
async fn server_streaming_exec_sends_stdout_and_exit_status() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("stream-cmd", |mut ctx| async move {
            ctx.stdout("line 1\n").await.unwrap();
            ctx.stdout("line 2\n").await.unwrap();
            ctx.stderr("warning\n").await.unwrap();
            ctx.exit_status(42).await.unwrap();
            Ok(())
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("stream-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"line 1\nline 2\n");
    assert_eq!(output.stderr.as_ref(), b"warning\n");
    assert_eq!(output.exit, CommandExit::status(42));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_falls_back_to_buffered_for_unknown_command() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("stream-cmd", |mut ctx| async move {
            ctx.stdout("streaming\n").await.unwrap();
            ctx.exit_status(0).await.unwrap();
            Ok(())
        })
        .exec("buffered-cmd", |_ctx| async {
            Ok(ExecResponse::success().stdout("buffered\n").exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();

    // Unknown command: should fail
    let error = session.command("missing-cmd").await.unwrap_err();
    let Error::Channel(channel) = error else {
        panic!("expected channel error");
    };
    assert_eq!(channel.kind(), ChannelErrorKind::Request);

    // Buffered command still works
    let output = session.command("buffered-cmd").await.unwrap();
    assert_eq!(output.stdout.as_ref(), b"buffered\n");

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_reads_stdin_and_echoes() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("echo", |mut ctx| async move {
            let mut collected = Vec::new();
            while let Some(chunk) = ctx.read_stdin().await {
                collected.extend_from_slice(&chunk);
            }
            ctx.stdout(collected).await.unwrap();
            ctx.exit_status(0).await.unwrap();
            Ok(())
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session
        .command(RemoteCommand::new("echo").with_stdin("hello from stdin"))
        .await
        .unwrap();

    assert_eq!(output.stdout.as_ref(), b"hello from stdin");
    assert_eq!(output.exit, CommandExit::Status(0));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_handler_error_yields_exit_1_and_stderr() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("err-cmd", |mut ctx| async move {
            ctx.stdout("partial output\n").await.unwrap();
            ctx.stderr("error: something went wrong\n").await.unwrap();
            Err(russh_extra::Error::invalid_config("oh no"))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("err-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"partial output\n");
    assert_eq!(output.stderr.as_ref(), b"error: something went wrong\n");
    assert_eq!(output.exit, CommandExit::status(1));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_handler_panic_yields_exit_1() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("panic-cmd", |_ctx| async move {
            panic!("boom");
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("panic-cmd").await.unwrap();

    assert_eq!(output.exit, CommandExit::status(1));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_explicit_exit_overrides_error_fallback() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("exit-cmd", |mut ctx| async move {
            ctx.stdout("ok\n").await.unwrap();
            ctx.exit_status(42).await.unwrap();
            Err(russh_extra::Error::invalid_config("this error is ignored"))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("exit-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"ok\n");
    assert_eq!(output.exit, CommandExit::status(42));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_streaming_exec_success_without_explicit_exit_sends_exit_0() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("ok-cmd", |mut ctx| async move {
            ctx.stdout("done\n").await.unwrap();
            Ok(())
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let output = session.command("ok-cmd").await.unwrap();

    assert_eq!(output.stdout.as_ref(), b"done\n");
    assert_eq!(output.exit, CommandExit::status(0));

    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_keyboard_interactive_single_prompt_succeeds() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth(|ctx| async move {
            if ctx.responses.is_empty() {
                Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                    russh_extra::KeyboardInteractivePrompt::new(
                        "OTP",
                        "Enter your one-time password:",
                        vec![russh_extra::KeyboardInteractivePromptItem::new(
                            "Password:",
                            false,
                        )],
                    ),
                ))
            } else {
                let resp = String::from_utf8_lossy(&ctx.responses[0]);
                if resp == "123456" {
                    Ok(russh_extra::KeyboardInteractiveResponse::Accept)
                } else {
                    Ok(russh_extra::KeyboardInteractiveResponse::Reject)
                }
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(ctx.username().as_str())
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();

    let reply = raw_handle
        .authenticate_keyboard_interactive_start("demo", None::<String>)
        .await
        .unwrap();
    assert!(
        matches!(
            reply,
            russh::client::KeyboardInteractiveAuthResponse::InfoRequest { .. }
        ),
        "expected InfoRequest, got {:?}",
        reply
    );

    let reply = raw_handle
        .authenticate_keyboard_interactive_respond(vec!["123456".to_owned()])
        .await
        .unwrap();
    assert!(
        matches!(
            reply,
            russh::client::KeyboardInteractiveAuthResponse::Success
        ),
        "expected Success, got {:?}",
        reply
    );

    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_keyboard_interactive_wrong_answer_rejects() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth(|ctx| async move {
            if ctx.responses.is_empty() {
                Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                    russh_extra::KeyboardInteractivePrompt::new(
                        "Verify",
                        "",
                        vec![russh_extra::KeyboardInteractivePromptItem::new(
                            "Code:", false,
                        )],
                    ),
                ))
            } else {
                Ok(russh_extra::KeyboardInteractiveResponse::Reject)
            }
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();

    let reply = raw_handle
        .authenticate_keyboard_interactive_start("demo", None::<String>)
        .await
        .unwrap();
    assert!(matches!(
        reply,
        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { .. }
    ));

    let reply = raw_handle
        .authenticate_keyboard_interactive_respond(vec!["wrong".to_owned()])
        .await
        .unwrap();
    assert!(matches!(
        reply,
        russh::client::KeyboardInteractiveAuthResponse::Failure { .. }
    ));

    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_keyboard_interactive_multi_step_accepts_after_two_rounds() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth({
            let step = std::sync::atomic::AtomicU8::new(0);
            move |_ctx| {
                let step = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if step == 0 {
                        Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                            russh_extra::KeyboardInteractivePrompt::new(
                                "Step 1",
                                "First prompt",
                                vec![russh_extra::KeyboardInteractivePromptItem::new("A:", false)],
                            ),
                        ))
                    } else if step == 1 {
                        Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                            russh_extra::KeyboardInteractivePrompt::new(
                                "Step 2",
                                "Second prompt",
                                vec![russh_extra::KeyboardInteractivePromptItem::new("B:", true)],
                            ),
                        ))
                    } else {
                        Ok(russh_extra::KeyboardInteractiveResponse::Accept)
                    }
                }
            }
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();

    let reply = raw_handle
        .authenticate_keyboard_interactive_start("demo", None::<String>)
        .await
        .unwrap();
    assert!(matches!(
        reply,
        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { .. }
    ));

    let reply = raw_handle
        .authenticate_keyboard_interactive_respond(vec!["answer1".to_owned()])
        .await
        .unwrap();
    assert!(matches!(
        reply,
        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { .. }
    ));

    let reply = raw_handle
        .authenticate_keyboard_interactive_respond(vec!["answer2".to_owned()])
        .await
        .unwrap();
    assert!(matches!(
        reply,
        russh::client::KeyboardInteractiveAuthResponse::Success
    ));

    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn client_keyboard_interactive_single_prompt_succeeds() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth(|ctx| async move {
            if ctx.responses.is_empty() {
                Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                    russh_extra::KeyboardInteractivePrompt::new(
                        "OTP",
                        "Enter your one-time password:",
                        vec![russh_extra::KeyboardInteractivePromptItem::new(
                            "Password:",
                            false,
                        )],
                    ),
                ))
            } else {
                let resp = String::from_utf8_lossy(&ctx.responses[0]);
                if resp == "123456" {
                    Ok(russh_extra::KeyboardInteractiveResponse::Accept)
                } else {
                    Ok(russh_extra::KeyboardInteractiveResponse::Reject)
                }
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(ctx.username().as_str())
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let client = Client::builder()
        .endpoint(endpoint.clone())
        .username("demo")
        .accept_any_host_key()
        .keyboard_interactive(|info| {
            Box::pin(async move {
                assert_eq!(info.name, "OTP");
                russh_extra::KeyboardInteractiveReply::Responses(vec!["123456".to_owned()])
            })
        })
        .build();

    let session = retry_client_connect(&client).await.unwrap();
    let cmd = session.command("whoami").await.unwrap();
    assert!(
        cmd.success(),
        "command should succeed after keyboard-interactive auth"
    );

    stop_server(handle, task).await;
}

#[tokio::test]
async fn client_keyboard_interactive_wrong_answer_rejects() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth(|ctx| async move {
            if ctx.responses.is_empty() {
                Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                    russh_extra::KeyboardInteractivePrompt::new(
                        "Verify",
                        "",
                        vec![russh_extra::KeyboardInteractivePromptItem::new(
                            "Code:", false,
                        )],
                    ),
                ))
            } else {
                Ok(russh_extra::KeyboardInteractiveResponse::Reject)
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(ctx.username().as_str())
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let client = Client::builder()
        .endpoint(endpoint.clone())
        .username("demo")
        .accept_any_host_key()
        .keyboard_interactive(|_info| {
            Box::pin(async move {
                russh_extra::KeyboardInteractiveReply::Responses(vec!["wrong".to_owned()])
            })
        })
        .build();

    let result = retry_client_connect(&client).await;
    assert!(
        result.is_err(),
        "connect should fail with wrong keyboard-interactive answer"
    );

    stop_server(handle, task).await;
}

#[tokio::test]
async fn client_keyboard_interactive_multi_step_succeeds() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .keyboard_interactive_auth({
            let step = std::sync::atomic::AtomicU8::new(0);
            let username = "demo".to_owned();
            move |_ctx| {
                let s = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _user = username.clone();
                async move {
                    if s == 0 {
                        Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                            russh_extra::KeyboardInteractivePrompt::new(
                                "Step 1",
                                "First",
                                vec![russh_extra::KeyboardInteractivePromptItem::new("A:", false)],
                            ),
                        ))
                    } else if s == 1 {
                        Ok(russh_extra::KeyboardInteractiveResponse::FurtherAction(
                            russh_extra::KeyboardInteractivePrompt::new(
                                "Step 2",
                                "Second",
                                vec![russh_extra::KeyboardInteractivePromptItem::new("B:", true)],
                            ),
                        ))
                    } else {
                        Ok(russh_extra::KeyboardInteractiveResponse::Accept)
                    }
                }
            }
        })
        .exec("whoami", |ctx| async move {
            Ok(ExecResponse::success()
                .stdout(ctx.username().as_str())
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let client = Client::builder()
        .endpoint(endpoint.clone())
        .username("demo")
        .accept_any_host_key()
        .keyboard_interactive({
            let calls = std::sync::atomic::AtomicU8::new(0);
            move |info| {
                let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move {
                    if n == 0 {
                        assert_eq!(info.name, "Step 1");
                        russh_extra::KeyboardInteractiveReply::Responses(vec!["answer1".to_owned()])
                    } else if n == 1 {
                        assert_eq!(info.name, "Step 2");
                        russh_extra::KeyboardInteractiveReply::Responses(vec!["answer2".to_owned()])
                    } else {
                        panic!("handler called more than 2 times");
                    }
                })
            }
        })
        .build();

    let session = retry_client_connect(&client).await.unwrap();
    let cmd = session.command("whoami").await.unwrap();
    assert!(
        cmd.success(),
        "command should succeed after multi-step keyboard-interactive auth"
    );

    stop_server(handle, task).await;
}

async fn raw_authenticate(raw_handle: &mut client::Handle<RawAcceptAnyClient>) {
    let reply = raw_handle.authenticate_password("demo", "x").await.unwrap();
    assert!(reply.success(), "raw password auth failed");
}

#[tokio::test]
async fn server_env_vars_received_in_exec_handler() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .exec("env-check", |ctx| async move {
            let foo = ctx
                .env()
                .get("FOO")
                .map(|v| v.as_str())
                .unwrap_or("MISSING");
            Ok(ExecResponse::success().stdout(foo).exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();
    raw_authenticate(&mut raw_handle).await;

    let mut channel = raw_handle.channel_open_session().await.unwrap();
    channel.set_env(true, "FOO", "bar").await.unwrap();
    channel.exec(true, "env-check").await.unwrap();

    let mut data = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data: d } => data.extend_from_slice(&d),
            ChannelMsg::Eof => break,
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    assert_eq!(data.as_slice(), b"bar");
    drop(channel);
    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_env_vars_received_in_streaming_exec_handler() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .streaming_exec("env-stream", |mut ctx| async move {
            let foo = ctx
                .env()
                .get("FOO")
                .map(|v| v.as_str())
                .unwrap_or("MISSING");
            ctx.stdout(format!("{foo}\n")).await.unwrap();
            ctx.exit_status(0).await.unwrap();
            Ok(())
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();
    raw_authenticate(&mut raw_handle).await;

    let mut channel = raw_handle.channel_open_session().await.unwrap();
    channel.set_env(true, "FOO", "bar").await.unwrap();
    channel.exec(true, "env-stream").await.unwrap();

    let mut data = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data: d } => data.extend_from_slice(&d),
            ChannelMsg::Eof => break,
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    assert_eq!(data.as_slice(), b"bar\n");
    drop(channel);
    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_env_vars_not_set_returns_empty() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .exec("env-check", |ctx| async move {
            let values: Vec<String> = ctx.env().iter().map(|(k, v)| format!("{k}={v}")).collect();
            Ok(ExecResponse::success()
                .stdout(values.join(","))
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();
    raw_authenticate(&mut raw_handle).await;

    let mut channel = raw_handle.channel_open_session().await.unwrap();
    channel.exec(true, "env-check").await.unwrap();

    let mut data = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data: d } => data.extend_from_slice(&d),
            ChannelMsg::Eof => break,
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    assert_eq!(data.as_slice(), b"");
    drop(channel);
    drop(raw_handle);
    stop_server(handle, task).await;
}

#[tokio::test]
async fn server_env_vars_multiple_called_once_per_channel() {
    init_tracing();
    let endpoint = unused_endpoint().await;
    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_, _| async { Ok(AuthDecision::accept()) })
        .exec("env-check", |ctx| async move {
            let values: Vec<String> = ctx.env().iter().map(|(k, v)| format!("{k}={v}")).collect();
            Ok(ExecResponse::success()
                .stdout(values.join(";"))
                .exit_status(0))
        })
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let addr = std::net::SocketAddr::new(endpoint.host().parse().unwrap(), endpoint.port());
    let mut raw_handle = retry_raw_connect(addr).await.unwrap();
    raw_authenticate(&mut raw_handle).await;

    let mut channel = raw_handle.channel_open_session().await.unwrap();
    channel.set_env(true, "A", "1").await.unwrap();
    channel.set_env(true, "B", "2").await.unwrap();
    channel.set_env(true, "C", "3").await.unwrap();
    channel.exec(true, "env-check").await.unwrap();

    let mut data = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data: d } => data.extend_from_slice(&d),
            ChannelMsg::Eof => break,
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    let result = String::from_utf8_lossy(&data);
    assert!(result.contains("A=1"));
    assert!(result.contains("B=2"));
    assert!(result.contains("C=3"));
    drop(channel);
    drop(raw_handle);
    stop_server(handle, task).await;
}
