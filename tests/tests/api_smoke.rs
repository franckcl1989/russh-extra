use pretty_assertions::assert_eq;
use russh_extra::{
    AuthenticationErrorKind, Client, CommandExit, CommandLimits, CommandOutput, Endpoint, Error,
    ForwardDirection, ForwardSpec, HostKeyErrorKind, HostKeyPolicy, Identity, Operation, Server,
    ServerHostKey, TcpEndpoint, TransportErrorKind,
};

fn test_host_key() -> ServerHostKey {
    let private_key = russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )
    .unwrap();

    ServerHostKey::from_private_key(private_key)
}

#[test]
fn client_builder_sets_core_configuration() {
    let client = Client::builder()
        .endpoint(("example.com", 2222))
        .username("alice")
        .password("secret")
        .identity(Identity::agent())
        .strict_host_key_checking(false)
        .build();

    assert_eq!(
        client.config().endpoint(),
        &Endpoint::new("example.com", 2222)
    );
    assert_eq!(client.config().username().unwrap().as_str(), "alice");
    assert_eq!(client.config().credentials().len(), 2);
    assert!(!client.config().strict_host_key_checking());
}

#[test]
fn server_builder_sets_listen_configuration() {
    let server = Server::builder()
        .listen(("127.0.0.1", 2022))
        .host_key(test_host_key())
        .server_id("SSH-2.0-russh-extra-test")
        .max_sessions(8)
        .build()
        .unwrap();

    assert_eq!(server.config().listen(), &Endpoint::new("127.0.0.1", 2022));
    assert_eq!(server.config().server_id(), "SSH-2.0-russh-extra-test");
    assert_eq!(server.config().max_sessions(), 8);
}

#[test]
fn forwarding_specs_keep_public_metadata() {
    let spec = ForwardSpec::local_tcp(
        TcpEndpoint::new("127.0.0.1", 8080),
        TcpEndpoint::new("10.0.0.10", 80),
    );

    let ForwardSpec::Tcp {
        direction,
        bind,
        target,
    } = spec
    else {
        panic!("expected TCP forwarding spec");
    };

    assert_eq!(direction, ForwardDirection::Local);
    assert_eq!(bind.host(), "127.0.0.1");
    assert_eq!(bind.port(), 8080);
    assert_eq!(target.host(), "10.0.0.10");
    assert_eq!(target.port(), 80);
}

#[test]
fn command_output_success_uses_typed_exit() {
    let output = CommandOutput {
        exit: CommandExit::status(0),
        stdout: Default::default(),
        stderr: Default::default(),
    };

    assert!(output.success());
}

#[test]
fn typed_errors_are_matchable_from_public_api() {
    let error = Error::transport(TransportErrorKind::TcpConnect, "connect failed");
    let Error::Transport(transport) = error else {
        panic!("expected transport error");
    };
    assert_eq!(transport.kind(), TransportErrorKind::TcpConnect);

    let error = Error::authentication_kind(AuthenticationErrorKind::Exhausted, "no credentials");
    let Error::Authentication(auth) = error else {
        panic!("expected authentication error");
    };
    assert_eq!(auth.kind(), AuthenticationErrorKind::Exhausted);

    let error = Error::host_key(HostKeyErrorKind::Changed, "host key changed");
    let Error::HostKey(host_key) = error else {
        panic!("expected host-key error");
    };
    assert_eq!(host_key.kind(), HostKeyErrorKind::Changed);

    assert!(Error::timeout(Operation::Connect, "connect timed out").is_timeout());
}

#[test]
fn client_api_exposes_host_key_policy_and_command_limits() {
    let client = Client::builder()
        .try_pinned_host_key_sha256("SHA256:abc123+/=")
        .unwrap()
        .build();

    assert!(matches!(
        client.config().host_key_policy(),
        HostKeyPolicy::PinnedSha256(_)
    ));

    let command = russh_extra::RemoteCommand::new("echo hello")
        .with_limits(CommandLimits::new(1024, 2048))
        .stderr_limit(4096);

    assert_eq!(command.limits().stdout(), 1024);
    assert_eq!(command.limits().stderr(), 4096);
}
