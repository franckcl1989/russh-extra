//! Test support utilities for `russh-extra`.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

mod sftp_mock;
use sftp_mock::MockSftpServer;

static TRACING: Once = Once::new();

/// Initializes tracing for tests.
pub fn init_tracing() {
    TRACING.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "russh_extra=trace".into()),
            )
            .with_test_writer()
            .try_init();
    });
}

/// Returns a localhost endpoint with an ephemeral-style port for tests that do
/// not bind a socket yet.
pub fn localhost_endpoint(port: u16) -> russh_extra::Endpoint {
    russh_extra::Endpoint::new("127.0.0.1", port)
}

/// Local loopback SSH server for integration tests.
pub struct LoopbackServer {
    addr: SocketAddr,
    host_key_sha256_fingerprint: String,
    handle: russh::server::RunningServerHandle,
    task: JoinHandle<std::io::Result<()>>,
}

impl LoopbackServer {
    /// Starts a loopback server on an ephemeral port.
    pub async fn start(config: LoopbackServerConfig) -> Result<Self, russh_extra::BoxError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
        let host_key_sha256_fingerprint = host_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string();

        let server_config = Arc::new(russh::server::Config {
            auth_rejection_time: Duration::from_millis(1),
            auth_rejection_time_initial: Some(Duration::from_millis(0)),
            keys: vec![host_key],
            ..Default::default()
        });

        let (handle_tx, handle_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut server = LoopbackRusshServer {
                state: Arc::new(config),
            };
            let running = server.run_on_socket(server_config, &listener);
            let handle = running.handle();
            let _ = handle_tx.send(handle);
            running.await
        });

        let handle = handle_rx.await?;

        Ok(Self {
            addr,
            host_key_sha256_fingerprint,
            handle,
            task,
        })
    }

    /// Returns the bound socket address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns the bound endpoint.
    pub fn endpoint(&self) -> russh_extra::Endpoint {
        russh_extra::Endpoint::new(self.addr.ip().to_string(), self.addr.port())
    }

    /// Returns the OpenSSH-style SHA256 host-key fingerprint.
    pub fn host_key_sha256_fingerprint(&self) -> &str {
        &self.host_key_sha256_fingerprint
    }
}

impl fmt::Debug for LoopbackServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.handle
            .shutdown("russh-extra test fixture shutting down".to_owned());
        self.task.abort();
    }
}

/// Loopback server behavior configuration.
#[derive(Clone)]
pub struct LoopbackServerConfig {
    username: String,
    password: String,
    authorized_keys: Vec<(String, russh::keys::ssh_key::PublicKey)>,
    commands: HashMap<String, CommandResponse>,
    streaming_commands: HashMap<String, StreamingCommandConfig>,
    accept_shell: bool,
    accept_pty: bool,
    accept_subsystem: String,
    accept_direct_tcpip: bool,
    accept_tcpip_forward: bool,
    sftp_mock: MockSftpServer,
}

impl LoopbackServerConfig {
    /// Creates a loopback configuration.
    pub fn new() -> Self {
        Self {
            username: "test".to_owned(),
            password: "test".to_owned(),
            authorized_keys: Vec::new(),
            commands: HashMap::new(),
            streaming_commands: HashMap::new(),
            accept_shell: false,
            accept_pty: false,
            accept_subsystem: String::new(),
            accept_direct_tcpip: false,
            accept_tcpip_forward: false,
            sftp_mock: MockSftpServer::new(),
        }
    }

    /// Sets the accepted username and password.
    pub fn password(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = username.into();
        self.password = password.into();
        self
    }

    /// Adds an authorized public key for a user.
    pub fn authorized_key(
        mut self,
        username: impl Into<String>,
        public_key: russh::keys::ssh_key::PublicKey,
    ) -> Self {
        self.authorized_keys.push((username.into(), public_key));
        self
    }

    /// Registers a command response.
    pub fn command(mut self, command: impl Into<String>, response: CommandResponse) -> Self {
        self.commands.insert(command.into(), response);
        self
    }

    /// Accept shell requests.
    pub fn accept_shell(mut self) -> Self {
        self.accept_shell = true;
        self
    }

    /// Accept PTY requests.
    pub fn accept_pty(mut self) -> Self {
        self.accept_pty = true;
        self
    }

    /// Accept a named subsystem request.
    pub fn accept_subsystem(mut self, name: impl Into<String>) -> Self {
        self.accept_subsystem = name.into();
        self
    }

    /// Accept direct-tcpip channels.
    pub fn accept_direct_tcpip(mut self) -> Self {
        self.accept_direct_tcpip = true;
        self
    }

    /// Accept tcpip-forward requests.
    pub fn accept_tcpip_forward(mut self) -> Self {
        self.accept_tcpip_forward = true;
        self
    }

    /// Adds a file to the mock SFTP server for SFTP integration tests.
    ///
    /// The file content is stored in memory and served through the SFTP
    /// subsystem when `accept_subsystem("sftp")` is also configured.
    pub fn sftp_file(self, path: impl Into<String>, data: impl Into<Vec<u8>>, perms: u32) -> Self {
        let data = data.into();
        let size = data.len() as u64;
        self.sftp_mock.add_file(&path.into(), &data, size, perms);
        self
    }

    /// Adds a directory to the mock SFTP server.
    pub fn sftp_dir(self, path: impl Into<String>, entries: &[&str]) -> Self {
        self.sftp_mock.add_dir(&path.into(), entries);
        self
    }

    /// Adds a symlink to the mock SFTP server.
    pub fn sftp_symlink(self, linkpath: impl Into<String>, targetpath: impl Into<String>) -> Self {
        self.sftp_mock
            .add_symlink(&linkpath.into(), &targetpath.into());
        self
    }

    /// Registers a streaming command with configurable steps.
    pub fn streaming_command(
        mut self,
        command: impl Into<String>,
        config: StreamingCommandConfig,
    ) -> Self {
        self.streaming_commands.insert(command.into(), config);
        self
    }

    fn response_for(&self, command: &[u8]) -> Option<&CommandResponse> {
        let command = String::from_utf8_lossy(command);
        self.commands.get(command.as_ref())
    }

    fn streaming_command_config(&self, command: &[u8]) -> Option<&StreamingCommandConfig> {
        let command = String::from_utf8_lossy(command);
        self.streaming_commands.get(command.as_ref())
    }
}

impl Default for LoopbackServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates an Ed25519 key pair for testing.
pub fn generate_test_key_pair() -> (russh::keys::PrivateKey, russh::keys::ssh_key::PublicKey) {
    let private_key =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
            .expect("test key generation should succeed");
    let public_key = private_key.public_key().clone();
    (private_key, public_key)
}

impl fmt::Debug for LoopbackServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackServerConfig")
            .field("username", &self.username)
            .field("password", &"***")
            .field("authorized_key_count", &self.authorized_keys.len())
            .field("commands", &self.commands)
            .field("streaming_commands", &self.streaming_commands)
            .field("accept_shell", &self.accept_shell)
            .field("accept_pty", &self.accept_pty)
            .field("accept_subsystem", &self.accept_subsystem)
            .field("accept_direct_tcpip", &self.accept_direct_tcpip)
            .field("accept_tcpip_forward", &self.accept_tcpip_forward)
            .finish()
    }
}

/// Configured response for an exec request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResponse {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_status: Option<u32>,
    reject_request: bool,
    disconnect: bool,
}

impl CommandResponse {
    /// Creates a successful response with stdout and exit status `0`.
    pub fn stdout(stdout: impl Into<Vec<u8>>) -> Self {
        Self::new().with_stdout(stdout).with_exit_status(0)
    }

    /// Creates an empty response with exit status `0`.
    pub fn success() -> Self {
        Self::new().with_exit_status(0)
    }

    /// Creates a response that rejects the exec request.
    pub fn reject_request() -> Self {
        Self {
            reject_request: true,
            ..Self::new()
        }
    }

    /// Creates a response that disconnects the session when executed.
    pub fn disconnect() -> Self {
        Self {
            disconnect: true,
            ..Self::new()
        }
    }

    /// Creates a response without an exit status.
    pub fn missing_status() -> Self {
        Self::new()
    }

    /// Creates an empty response.
    pub fn new() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_status: None,
            reject_request: false,
            disconnect: false,
        }
    }

    /// Sets stdout bytes.
    pub fn with_stdout(mut self, stdout: impl Into<Vec<u8>>) -> Self {
        self.stdout = stdout.into();
        self
    }

    /// Sets stderr bytes.
    pub fn with_stderr(mut self, stderr: impl Into<Vec<u8>>) -> Self {
        self.stderr = stderr.into();
        self
    }

    /// Sets the exit status.
    pub fn with_exit_status(mut self, status: u32) -> Self {
        self.exit_status = Some(status);
        self
    }
}

impl Default for CommandResponse {
    fn default() -> Self {
        Self::new()
    }
}

/// Pre-configured streaming command behavior for loopback test servers.
///
/// Each step is executed in order during an exec request, with optional
/// delays between steps to simulate real streaming output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamingCommandConfig {
    steps: Vec<StreamingStep>,
}

impl StreamingCommandConfig {
    /// Creates an empty streaming command config.
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Appends a stdout step.
    pub fn stdout(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.steps.push(StreamingStep::Stdout(data.into()));
        self
    }

    /// Appends a stderr step.
    pub fn stderr(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.steps.push(StreamingStep::Stderr(data.into()));
        self
    }

    /// Appends an exit-status step (ends the command).
    pub fn exit_status(mut self, status: u32) -> Self {
        self.steps.push(StreamingStep::ExitStatus(status));
        self
    }

    /// Inserts a delay before the next step.
    pub fn delay(mut self, duration: Duration) -> Self {
        self.steps.push(StreamingStep::Delay(duration));
        self
    }
}

impl Default for StreamingCommandConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One step in a streaming command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamingStep {
    /// Send data on stdout.
    Stdout(Vec<u8>),
    /// Send data on stderr.
    Stderr(Vec<u8>),
    /// Set the exit status (signals channel close after this step).
    ExitStatus(u32),
    /// Pause for the given duration before the next step.
    Delay(Duration),
}

#[derive(Clone)]
struct LoopbackRusshServer {
    state: Arc<LoopbackServerConfig>,
}

impl russh::server::Server for LoopbackRusshServer {
    type Handler = LoopbackHandler;

    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
        LoopbackHandler {
            state: Arc::clone(&self.state),
            sftp_channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Clone)]
struct LoopbackHandler {
    state: Arc<LoopbackServerConfig>,
    sftp_channels: Arc<Mutex<HashMap<ChannelId, ()>>>,
}

impl russh::server::Handler for LoopbackHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == self.state.username && password == self.state.password {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let authorized = self
            .state
            .authorized_keys
            .iter()
            .any(|(name, key)| name == user && key == public_key);

        if authorized {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(config) = self.state.streaming_command_config(data) {
            session.channel_success(channel)?;

            for step in &config.steps {
                match step {
                    StreamingStep::Stdout(data) => {
                        session.data(channel, data.clone())?;
                    }
                    StreamingStep::Stderr(data) => {
                        session.extended_data(channel, 1, data.clone())?;
                    }
                    StreamingStep::Delay(duration) => {
                        tokio::time::sleep(*duration).await;
                    }
                    StreamingStep::ExitStatus(status) => {
                        session.exit_status_request(channel, *status)?;
                    }
                }
            }

            session.eof(channel)?;
            session.close(channel)?;
            return Ok(());
        }

        let Some(response) = self.state.response_for(data) else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        if response.reject_request {
            session.channel_failure(channel)?;
            return Ok(());
        }

        session.channel_success(channel)?;

        if response.disconnect {
            return Err(russh::Error::Disconnect);
        }

        if !response.stdout.is_empty() {
            session.data(channel, response.stdout.clone())?;
        }

        if !response.stderr.is_empty() {
            session.extended_data(channel, 1, response.stderr.clone())?;
        }

        if let Some(status) = response.exit_status {
            session.exit_status_request(channel, status)?;
        }

        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.state.accept_shell {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.state.accept_pty {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _variable_name: &str,
        _variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == self.state.accept_subsystem {
            session.channel_success(channel)?;
            if name == "sftp" {
                self.sftp_channels.lock().unwrap().insert(channel, ());
            }
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(self.state.accept_direct_tcpip)
    }

    async fn tcpip_forward(
        &mut self,
        _address: &str,
        _port: &mut u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(self.state.accept_tcpip_forward)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let is_sftp = self.sftp_channels.lock().unwrap().contains_key(&channel);
        if is_sftp {
            let sftp = self.state.sftp_mock.clone();
            sftp.feed(channel, data, session)?;
            return Ok(());
        }
        if self.state.accept_shell || !self.state.accept_subsystem.is_empty() {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.sftp_channels.lock().unwrap().remove(&channel);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CommandResponse, LoopbackServer, LoopbackServerConfig};
    use russh::{ChannelMsg, client};

    struct AcceptAnyClient;

    impl client::Handler for AcceptAnyClient {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _server_public_key: &russh::keys::ssh_key::PublicKey,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    #[test]
    fn loopback_config_redacts_password_debug() {
        let config = LoopbackServerConfig::new().password("demo", "do-not-print");

        let debug = format!("{config:?}");

        assert!(debug.contains("***"));
        assert!(!debug.contains("do-not-print"));
    }

    #[tokio::test]
    async fn loopback_server_binds_ephemeral_localhost_port() {
        let server = LoopbackServer::start(
            LoopbackServerConfig::new().command("whoami", CommandResponse::stdout("demo\n")),
        )
        .await
        .unwrap();

        assert!(server.addr().ip().is_loopback());
        assert_ne!(server.addr().port(), 0);
        assert_eq!(server.endpoint().port(), server.addr().port());
        assert!(server.host_key_sha256_fingerprint().starts_with("SHA256:"));
    }

    #[tokio::test]
    async fn loopback_server_executes_configured_command() {
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

        let mut client = client::connect(
            Arc::new(client::Config::default()),
            server.addr(),
            AcceptAnyClient,
        )
        .await
        .unwrap();
        assert!(
            client
                .authenticate_password("demo", "demo")
                .await
                .unwrap()
                .success()
        );

        let mut channel = client.channel_open_session().await.unwrap();
        channel.exec(true, "whoami").await.unwrap();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;

        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus {
                    exit_status: status,
                } => exit_status = Some(status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }

        assert_eq!(stdout, b"demo\n");
        assert_eq!(stderr, b"warning\n");
        assert_eq!(exit_status, Some(0));
    }
}
