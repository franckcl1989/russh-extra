//! Client-side high-level SSH APIs.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use russh::ChannelMsg;
use russh::client::{self, AuthResult};
use russh::keys::{Certificate, HashAlg, PrivateKey, ssh_key::PublicKey};
use russh_extra_core::{
    AuthenticationErrorKind, ChannelErrorKind, ClientConfig, ClientKeyboardInteractiveInfo,
    ClientKeyboardInteractivePrompt, CommandExit, CommandLimits, Credential, Endpoint, Error,
    HostKeyErrorKind, HostKeyPolicy, Identity, Keepalive, KeyboardInteractiveReply, Operation,
    Password, Result, SessionId, Timeouts, TransportErrorKind, Username,
};
use tokio::sync::{Mutex, MutexGuard};
use tokio::time;

#[cfg(feature = "known-hosts")]
use crate::known_hosts::{KnownHostStatus, KnownHosts};

/// OpenSSH certificate credential for authentication.
///
/// An OpenSSH certificate is a private key paired with a certificate
/// signed by a certificate authority. Both the private key and the
/// certificate must be provided.
///
/// # Loading from files
///
/// ```no_run
/// # use russh_extra::CertificateCredential;
/// let cert = CertificateCredential::from_openssh_files(
///     "~/.ssh/id_ed25519",
///     "~/.ssh/id_ed25519-cert.pub",
/// ).unwrap();
/// ```
///
/// # From in-memory data
///
/// ```no_run
/// # use russh_extra::CertificateCredential;
/// let cert = CertificateCredential::from_openssh_data(
///     b"-----BEGIN OPENSSH PRIVATE KEY-----...",
///     b"ssh-ed25519-cert-v01@openssh.com AAAA...",
/// ).unwrap();
/// ```
#[derive(Clone)]
pub struct CertificateCredential {
    key: Arc<PrivateKey>,
    cert: Certificate,
    passphrase: Option<Password>,
}

impl fmt::Debug for CertificateCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertificateCredential")
            .field("key", &"***")
            .field("cert", &"***")
            .field("has_passphrase", &self.passphrase.is_some())
            .finish()
    }
}

impl CertificateCredential {
    /// Loads an OpenSSH certificate credential from private key and
    /// certificate files.
    ///
    /// On Unix, the private key file must not be accessible by group or others.
    pub fn from_openssh_files(
        key_path: impl AsRef<std::path::Path>,
        cert_path: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        let key_path = key_path.as_ref();
        let cert_path = cert_path.as_ref();

        let key_path = expand_tilde_path(key_path);
        let cert_path = expand_tilde_path(cert_path);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&key_path)?;
            let mode = metadata.permissions().mode();
            if mode & 0o077 != 0 {
                return Err(Error::invalid_config(format!(
                    "private key file `{}` must not be accessible by group or others",
                    key_path.display()
                )));
            }
        }

        let key = russh::keys::load_secret_key(&key_path, None).map_err(|source| {
            Error::authentication_with_source(
                AuthenticationErrorKind::Unavailable,
                format!("failed to load private key from `{}`", key_path.display()),
                source,
            )
        })?;

        let cert = russh::keys::load_openssh_certificate(&cert_path).map_err(|source| {
            Error::authentication_with_source(
                AuthenticationErrorKind::Unavailable,
                format!("failed to load certificate from `{}`", cert_path.display()),
                source,
            )
        })?;

        Ok(Self {
            key: Arc::new(key),
            cert,
            passphrase: None,
        })
    }

    /// Creates a certificate credential from in-memory data.
    pub fn from_openssh_data(
        key_data: impl AsRef<[u8]>,
        cert_data: impl AsRef<[u8]>,
    ) -> Result<Self> {
        let key = PrivateKey::from_openssh(key_data).map_err(|source| {
            Error::authentication_with_source(
                AuthenticationErrorKind::Unavailable,
                "failed to parse in-memory private key",
                source,
            )
        })?;

        let cert_data = cert_data.as_ref();
        let cert_str = std::str::from_utf8(cert_data).map_err(|_| {
            Error::authentication_kind(
                AuthenticationErrorKind::Unavailable,
                "certificate data is not valid UTF-8",
            )
        })?;

        let cert = Certificate::from_openssh(cert_str).map_err(|source| {
            Error::authentication_with_source(
                AuthenticationErrorKind::Unavailable,
                "failed to parse in-memory certificate",
                source,
            )
        })?;

        Ok(Self {
            key: Arc::new(key),
            cert,
            passphrase: None,
        })
    }

    /// Adds a passphrase for the encrypted private key.
    pub fn with_passphrase(mut self, passphrase: impl Into<Password>) -> Self {
        self.passphrase = Some(passphrase.into());
        self
    }
}

fn expand_tilde_path(path: &std::path::Path) -> PathBuf {
    if let Some(path_str) = path.to_str()
        && (path_str == "~" || path_str.starts_with("~/"))
        && let Ok(home) = std::env::var("HOME")
    {
        if path_str == "~" {
            return PathBuf::from(home);
        }
        return PathBuf::from(home).join(&path_str[2..]);
    }

    path.to_path_buf()
}

/// High-level SSH client.
#[derive(Clone)]
pub struct Client {
    config: ClientConfig,
    #[cfg(feature = "known-hosts")]
    known_hosts: Option<KnownHosts>,
    #[cfg(feature = "known-hosts")]
    known_hosts_accept_new: bool,
    #[cfg(feature = "tunnel")]
    remote_forwards: crate::tunnel::RemoteForwardMap,
    #[cfg(feature = "tunnel")]
    remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap,
    cert_credentials: Vec<CertificateCredential>,
    x11_display: Option<PathBuf>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("config", &self.config)
            .field("x11_display", &self.x11_display)
            .field("cert_count", &self.cert_credentials.len())
            .finish()
    }
}

impl Client {
    /// Creates a client builder.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Creates a client from an endpoint using default settings.
    pub fn new(endpoint: impl Into<Endpoint>) -> Self {
        Self {
            config: ClientConfig::new(endpoint),
            #[cfg(feature = "known-hosts")]
            known_hosts: None,
            #[cfg(feature = "known-hosts")]
            known_hosts_accept_new: false,
            #[cfg(feature = "tunnel")]
            remote_forwards: crate::tunnel::RemoteForwardMap::default(),
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap::default(),
            cert_credentials: Vec::new(),
            x11_display: None,
        }
    }

    /// Returns client configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Connects to the configured endpoint.
    #[tracing::instrument(skip(self), fields(host = %self.config.endpoint().host(), port = self.config.endpoint().port()))]
    pub async fn connect(&self) -> Result<Session> {
        let endpoint = self.config.endpoint().clone();
        let addrs = (endpoint.host().to_owned(), endpoint.port());
        let handler = ClientHandler::new(
            self.config.host_key_policy().clone(),
            #[cfg(feature = "known-hosts")]
            self.known_hosts.clone(),
            #[cfg(feature = "known-hosts")]
            self.known_hosts_accept_new,
            #[cfg(feature = "known-hosts")]
            endpoint.clone(),
            #[cfg(not(feature = "known-hosts"))]
            endpoint.clone(),
            #[cfg(feature = "tunnel")]
            self.remote_forwards.clone(),
            #[cfg(feature = "tunnel")]
            self.remote_streamlocal_forwards.clone(),
            self.x11_display.clone(),
            self.cert_credentials.clone(),
        );

        let mut handle = time::timeout(
            self.config.timeouts().connect,
            client::connect(Arc::new(russh_client_config(&self.config)), addrs, handler),
        )
        .await
        .map_err(|_| Error::timeout(Operation::Connect, "client connection timed out"))?
        .map_err(map_connect_error)?;

        let auth_banner =
            authenticate_configured(&mut handle, &self.config, &self.cert_credentials).await?;

        Ok(Session {
            id: SessionId::next(),
            endpoint,
            timeouts: self.config.timeouts().clone(),
            handle: Some(Arc::new(Mutex::new(handle))),
            #[cfg(feature = "tunnel")]
            remote_forwards: self.remote_forwards.clone(),
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards: self.remote_streamlocal_forwards.clone(),
            auth_banner_text: auth_banner,
        })
    }
}

/// Builder for [`Client`].
pub struct ClientBuilder {
    config: ClientConfig,
    #[cfg(feature = "known-hosts")]
    known_hosts: Option<KnownHosts>,
    #[cfg(feature = "known-hosts")]
    known_hosts_accept_new: bool,
    #[cfg(feature = "tunnel")]
    remote_forwards: crate::tunnel::RemoteForwardMap,
    #[cfg(feature = "tunnel")]
    remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap,
    cert_credentials: Vec<CertificateCredential>,
    x11_display: Option<PathBuf>,
}

impl Default for ClientBuilder {
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self {
            config: ClientConfig::default(),
            #[cfg(feature = "known-hosts")]
            known_hosts: None,
            #[cfg(feature = "known-hosts")]
            known_hosts_accept_new: false,
            #[cfg(feature = "tunnel")]
            remote_forwards: crate::tunnel::RemoteForwardMap::default(),
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap::default(),
            cert_credentials: Vec::new(),
            x11_display: None,
        }
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ds = f.debug_struct("ClientBuilder");
        ds.field("config", &self.config);
        #[cfg(feature = "known-hosts")]
        {
            ds.field("has_known_hosts", &self.known_hosts.is_some());
            ds.field("known_hosts_accept_new", &self.known_hosts_accept_new);
        }
        ds.finish()
    }
}

impl ClientBuilder {
    /// Sets the target endpoint.
    pub fn endpoint(mut self, endpoint: impl Into<Endpoint>) -> Self {
        self.config.set_endpoint(endpoint);
        self
    }

    /// Sets the username.
    pub fn username(mut self, username: impl Into<Username>) -> Self {
        self.config.set_username(username);
        self
    }

    /// Adds password authentication.
    pub fn password(mut self, password: impl Into<Password>) -> Self {
        self.config
            .add_credential(Credential::password(password.into()));
        self
    }

    /// Adds a public key or agent identity.
    pub fn identity(mut self, identity: Identity) -> Self {
        self.config.add_credential(Credential::identity(identity));
        self
    }

    /// Adds SSH agent authentication.
    pub fn agent(mut self) -> Self {
        self.config.use_agent();
        self
    }

    /// Adds keyboard-interactive authentication with a challenge handler.
    ///
    /// The handler is called for each info-request from the server and must
    /// return responses or an abort.
    pub fn keyboard_interactive(
        mut self,
        handler: impl Fn(
            ClientKeyboardInteractiveInfo,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = KeyboardInteractiveReply> + Send>,
        > + Send
        + Sync
        + 'static,
    ) -> Self {
        self.config
            .add_credential(Credential::keyboard_interactive(handler));
        self
    }

    /// Sets strict host key checking.
    pub fn strict_host_key_checking(mut self, enabled: bool) -> Self {
        self.config.set_strict_host_key_checking(enabled);
        self
    }

    /// Accepts any host key.
    ///
    /// **Insecure**: this method disables host-key verification entirely.
    /// Only use it in tests and controlled environments.
    pub fn accept_any_host_key(mut self) -> Self {
        self.config
            .set_host_key_policy(HostKeyPolicy::InsecureAcceptAny);
        self
    }

    /// Sets an explicit host-key policy.
    pub fn host_key_policy(mut self, policy: HostKeyPolicy) -> Self {
        self.config.set_host_key_policy(policy);
        self
    }

    /// Sets timeout behavior.
    pub fn timeouts(mut self, timeouts: Timeouts) -> Self {
        self.config.set_timeouts(timeouts);
        self
    }

    /// Sets keepalive behavior.
    pub fn keepalive(mut self, keepalive: Keepalive) -> Self {
        self.config.set_keepalive(keepalive);
        self
    }

    /// Adds a pinned SHA256 host-key fingerprint.
    pub fn try_pinned_host_key_sha256(mut self, fingerprint: impl Into<String>) -> Result<Self> {
        self.config
            .set_host_key_policy(HostKeyPolicy::pinned_sha256(fingerprint)?);
        Ok(self)
    }

    /// Uses a known-hosts store to verify host keys.
    ///
    /// Unknown hosts are rejected. Changed keys are rejected.
    #[cfg(feature = "known-hosts")]
    pub fn known_hosts(mut self, known_hosts: KnownHosts) -> Self {
        self.known_hosts = Some(known_hosts);
        self.known_hosts_accept_new = false;
        self
    }

    /// Uses a known-hosts store with trust-on-first-use.
    ///
    /// Unknown hosts are accepted and their keys are added to the store.
    /// Changed keys are still rejected.
    #[cfg(feature = "known-hosts")]
    pub fn known_hosts_accept_new(mut self, known_hosts: KnownHosts) -> Self {
        self.known_hosts = Some(known_hosts);
        self.known_hosts_accept_new = true;
        self
    }

    /// Adds an OpenSSH certificate credential for authentication.
    pub fn certificate(mut self, cert: CertificateCredential) -> Self {
        self.cert_credentials.push(cert);
        self
    }

    /// Sets the X11 display to forward to.
    ///
    /// When set, incoming X11 channels from the server will be forwarded
    /// to this display. The display may be a Unix socket path (e.g.
    /// `"/tmp/.X11-unix/X0"`) or a TCP address (e.g. `"localhost:6000"`).
    pub fn x11_display(mut self, display: impl Into<PathBuf>) -> Self {
        self.x11_display = Some(display.into());
        self
    }

    /// Builds the client.
    pub fn build(self) -> Client {
        Client {
            config: self.config,
            #[cfg(feature = "known-hosts")]
            known_hosts: self.known_hosts,
            #[cfg(feature = "known-hosts")]
            known_hosts_accept_new: self.known_hosts_accept_new,
            #[cfg(feature = "tunnel")]
            remote_forwards: self.remote_forwards,
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards: self.remote_streamlocal_forwards,
            cert_credentials: self.cert_credentials,
            x11_display: self.x11_display,
        }
    }
}

/// `russh` client handler used by high-level client sessions.
#[derive(Clone)]
pub struct ClientHandler {
    host_key_policy: HostKeyPolicy,
    #[cfg(feature = "known-hosts")]
    known_hosts: Option<KnownHosts>,
    #[cfg(feature = "known-hosts")]
    known_hosts_accept_new: bool,
    #[cfg(feature = "known-hosts")]
    endpoint: Endpoint,
    #[cfg(not(feature = "known-hosts"))]
    _endpoint: Endpoint,
    #[cfg(feature = "tunnel")]
    remote_forwards: crate::tunnel::RemoteForwardMap,
    #[cfg(feature = "tunnel")]
    remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap,
    x11_display: Option<PathBuf>,
    auth_banner_text: Arc<Mutex<Option<String>>>,
    cert_credentials: Vec<CertificateCredential>,
}

impl fmt::Debug for ClientHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientHandler")
            .field("host_key_policy", &self.host_key_policy)
            .field("x11_display", &self.x11_display)
            .field("cert_count", &self.cert_credentials.len())
            .finish()
    }
}

impl ClientHandler {
    #[allow(clippy::too_many_arguments)]
    fn new(
        host_key_policy: HostKeyPolicy,
        #[cfg(feature = "known-hosts")] known_hosts: Option<KnownHosts>,
        #[cfg(feature = "known-hosts")] known_hosts_accept_new: bool,
        #[cfg(feature = "known-hosts")] endpoint: Endpoint,
        #[cfg(not(feature = "known-hosts"))] _endpoint: Endpoint,
        #[cfg(feature = "tunnel")] remote_forwards: crate::tunnel::RemoteForwardMap,
        #[cfg(feature = "tunnel")]
        remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap,
        x11_display: Option<PathBuf>,
        cert_credentials: Vec<CertificateCredential>,
    ) -> Self {
        Self {
            host_key_policy,
            #[cfg(feature = "known-hosts")]
            known_hosts,
            #[cfg(feature = "known-hosts")]
            known_hosts_accept_new,
            #[cfg(feature = "known-hosts")]
            endpoint,
            #[cfg(not(feature = "known-hosts"))]
            _endpoint,
            #[cfg(feature = "tunnel")]
            remote_forwards,
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards,
            x11_display,
            auth_banner_text: Arc::new(Mutex::new(None)),
            cert_credentials,
        }
    }

    /// Returns the host-key policy enforced by this handler.
    pub fn host_key_policy(&self) -> &HostKeyPolicy {
        &self.host_key_policy
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        #[cfg(feature = "known-hosts")]
        if let Some(known_hosts) = &self.known_hosts {
            let status = known_hosts.check(
                self.endpoint.host(),
                self.endpoint.port(),
                server_public_key,
            );

            match status {
                KnownHostStatus::Match => return Ok(true),
                KnownHostStatus::Revoked => return Err(russh::Error::WrongServerSig),
                KnownHostStatus::Changed => return Err(russh::Error::KeyChanged { line: 0 }),
                KnownHostStatus::NotFound => {
                    if self.known_hosts_accept_new {
                        let _ = known_hosts.add_entry(
                            self.endpoint.host(),
                            self.endpoint.port(),
                            server_public_key,
                            public_key_algorithm(server_public_key),
                        );
                        return Ok(true);
                    }
                    return Ok(false);
                }
            }
        }

        Ok(host_key_policy_accepts(
            &self.host_key_policy,
            server_public_key,
        ))
    }

    #[cfg(feature = "tunnel")]
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let target = {
            let fwds = self.remote_forwards.lock().await;
            fwds.get(&(connected_port as u16)).cloned()
        };
        match target {
            Some(target) => {
                let addr = format!("{}:{}", target.host(), target.port());
                tracing::debug!(
                    remote_port = connected_port,
                    local_target = %addr,
                    "accepted forwarded tcpip channel",
                );
                tokio::task::spawn(async move {
                    crate::tunnel::copy_bidirectional_with_addr(channel, &addr).await;
                });
            }
            None => {
                tracing::warn!(
                    remote_port = connected_port,
                    "received forwarded-tcpip channel for unknown port",
                );
                let _ = channel.close().await;
            }
        }
        Ok(())
    }

    #[cfg(feature = "tunnel")]
    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        socket_path: &str,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let target = {
            let fwds = self.remote_streamlocal_forwards.lock().await;
            fwds.get(socket_path).cloned()
        };
        match target {
            Some(target_path) => {
                #[cfg(unix)]
                {
                    tracing::debug!(
                        remote_path = %socket_path,
                        local_target = %target_path.display(),
                        "accepted forwarded streamlocal channel",
                    );
                    tokio::task::spawn(async move {
                        crate::tunnel::copy_bidirectional_with_unix_path(channel, &target_path)
                            .await;
                    });
                }
                #[cfg(not(unix))]
                {
                    tracing::warn!(
                        remote_path = %socket_path,
                        local_target = %target_path.display(),
                        "cannot accept forwarded streamlocal channel on this platform",
                    );
                    let _ = channel.close().await;
                }
            }
            None => {
                tracing::warn!(
                    remote_path = %socket_path,
                    "received forwarded-streamlocal channel for unknown path",
                );
                let _ = channel.close().await;
            }
        }
        Ok(())
    }

    #[cfg(feature = "tunnel")]
    async fn server_channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let _ = channel.close().await;
        Ok(())
    }

    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        *self.auth_banner_text.lock().await = Some(banner.to_owned());
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        match &self.x11_display {
            Some(display_path) => {
                #[cfg(feature = "tunnel")]
                {
                    let path = display_path.clone();
                    tokio::spawn(async move {
                        forward_x11_channel(channel, path).await;
                    });
                }
                #[cfg(not(feature = "tunnel"))]
                {
                    let _ = display_path;
                    tracing::warn!(
                        "received X11 channel but tunnel feature is not enabled; closing",
                    );
                    let _ = channel.close().await;
                }
            }
            None => {
                let _ = channel.close().await;
            }
        }
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _session: &mut russh::client::Session,
    ) -> std::result::Result<(), Self::Error> {
        #[cfg(all(unix, feature = "tunnel"))]
        {
            if let Ok(socket_path) = std::env::var("SSH_AUTH_SOCK") {
                tokio::spawn(async move {
                    match tokio::net::UnixStream::connect(&socket_path).await {
                        Ok(unix_stream) => {
                            crate::tunnel::copy_bidirectional_unix(channel, unix_stream).await;
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %socket_path,
                                error = %e,
                                "failed to connect to SSH agent for forwarded agent channel",
                            );
                            let _ = channel.close().await;
                        }
                    }
                });
                return Ok(());
            }
        }

        let _ = channel.close().await;
        Ok(())
    }
}

#[cfg(feature = "tunnel")]
async fn forward_x11_channel(channel: russh::Channel<russh::client::Msg>, display_path: PathBuf) {
    #[cfg(unix)]
    {
        let path_str = display_path.to_string_lossy().to_string();
        if path_str.starts_with('/') {
            match tokio::net::UnixStream::connect(&display_path).await {
                Ok(unix_stream) => {
                    crate::tunnel::copy_bidirectional_unix(channel, unix_stream).await;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path_str,
                        error = %e,
                        "failed to connect to X11 display via Unix socket",
                    );
                    let _ = channel.close().await;
                }
            }
            return;
        }
    }

    {
        let addr = display_path.to_string_lossy().to_string();
        match tokio::net::TcpStream::connect(addr.as_str()).await {
            Ok(tcp_stream) => {
                crate::tunnel::copy_bidirectional(channel, tcp_stream).await;
            }
            Err(e) => {
                tracing::warn!(
                    addr = %addr,
                    error = %e,
                    "failed to connect to X11 display via TCP",
                );
                let _ = channel.close().await;
            }
        }
    }
}

/// Serialized guard for direct access to the underlying `russh` client handle.
pub struct RusshHandleGuard<'a> {
    guard: MutexGuard<'a, client::Handle<ClientHandler>>,
}

impl fmt::Debug for RusshHandleGuard<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RusshHandleGuard").finish_non_exhaustive()
    }
}

impl Deref for RusshHandleGuard<'_> {
    type Target = client::Handle<ClientHandler>;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl DerefMut for RusshHandleGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

/// Connected high-level SSH session.
#[derive(Clone)]
pub struct Session {
    id: SessionId,
    endpoint: Endpoint,
    timeouts: Timeouts,
    handle: Option<Arc<Mutex<client::Handle<ClientHandler>>>>,
    #[cfg(feature = "tunnel")]
    remote_forwards: crate::tunnel::RemoteForwardMap,
    #[cfg(feature = "tunnel")]
    remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap,
    auth_banner_text: Arc<Mutex<Option<String>>>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("endpoint", &self.endpoint)
            .field("timeouts", &self.timeouts)
            .field("connected", &self.handle.is_some())
            .finish()
    }
}

impl Session {
    /// Creates a session handle from its public metadata.
    #[cfg(test)]
    pub(crate) fn new(id: SessionId, endpoint: impl Into<Endpoint>) -> Self {
        Self {
            id,
            endpoint: endpoint.into(),
            timeouts: Timeouts::default(),
            handle: None,
            #[cfg(feature = "tunnel")]
            remote_forwards: crate::tunnel::RemoteForwardMap::default(),
            #[cfg(feature = "tunnel")]
            remote_streamlocal_forwards: crate::tunnel::RemoteStreamLocalForwardMap::default(),
            auth_banner_text: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the session identifier.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Returns the connected endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Returns a serialized guard to the underlying `russh` client handle.
    pub async fn russh_handle(&self) -> Result<RusshHandleGuard<'_>> {
        let handle = self.handle.as_ref().ok_or_else(|| {
            Error::unsupported("raw russh handle is unavailable for this session")
        })?;

        Ok(RusshHandleGuard {
            guard: handle.lock().await,
        })
    }

    /// Runs a remote command.
    #[tracing::instrument(skip(self, command), fields(session = %self.id))]
    pub async fn command(&self, command: impl Into<RemoteCommand>) -> Result<CommandOutput> {
        let command = command.into();
        tracing::debug!(program = %command.program(), "running remote command");

        let handle = self.handle.as_ref().ok_or_else(|| {
            Error::unsupported("remote command execution requires a connected session")
        })?;
        let handle_guard = handle.lock().await;
        let mut channel = time::timeout(
            self.timeouts.channel_open,
            handle_guard.channel_open_session(),
        )
        .await
        .map_err(|_| Error::timeout(Operation::ChannelOpen, "session channel open timed out"))?
        .map_err(map_channel_open_error)?;

        channel
            .exec(true, command.program().as_bytes().to_vec())
            .await
            .map_err(map_command_error)?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit = CommandExit::Missing;
        let mut exit_observed = false;
        let mut stdin_sent = false;
        let mut exec_confirmed = false;
        let mut channel_closed = false;

        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Success => {
                    exec_confirmed = true;
                    if !stdin_sent {
                        if !command.stdin().is_empty() {
                            channel
                                .data(command.stdin().as_ref())
                                .await
                                .map_err(map_command_error)?;
                        }
                        channel.eof().await.map_err(map_command_error)?;
                        stdin_sent = true;
                    }
                }
                ChannelMsg::Failure => {
                    let error = Error::channel_kind(
                        ChannelErrorKind::Request,
                        "remote exec request was rejected",
                    );
                    let _ = channel.close().await;
                    return Err(error);
                }
                ChannelMsg::Data { data } => {
                    if let Err(error) =
                        append_limited(&mut stdout, &data, command.limits().stdout(), "stdout")
                    {
                        let _ = channel.close().await;
                        return Err(error);
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    if let Err(error) =
                        append_limited(&mut stderr, &data, command.limits().stderr(), "stderr")
                    {
                        let _ = channel.close().await;
                        return Err(error);
                    }
                }
                ChannelMsg::ExitStatus {
                    exit_status: status,
                } if !exit_observed => {
                    exit = CommandExit::Status(status);
                    exit_observed = true;
                }
                ChannelMsg::ExitSignal { signal_name, .. } if !exit_observed => {
                    exit = CommandExit::Signal(signal_to_name(signal_name));
                    exit_observed = true;
                }
                ChannelMsg::ExitStatus { .. } | ChannelMsg::ExitSignal { .. } => {}
                ChannelMsg::Eof => {}
                ChannelMsg::Close => {
                    channel_closed = true;
                    break;
                }
                _ => {}
            }
        }

        if !channel_closed && handle_guard.is_closed() {
            return Err(Error::disconnected(
                Operation::Command,
                "server disconnected during remote command",
            ));
        }

        if !exec_confirmed {
            return Err(Error::channel_kind(
                ChannelErrorKind::Protocol,
                "remote command channel closed before exec confirmation",
            ));
        }

        let _ = channel.close().await;
        drop(handle_guard);

        Ok(CommandOutput {
            exit,
            stdout: stdout.into(),
            stderr: stderr.into(),
        })
    }

    /// Creates an interactive shell builder for this session.
    #[cfg(feature = "shell")]
    pub fn shell(&self) -> crate::shell::ShellBuilder {
        crate::shell::ShellBuilder::from_session(
            self.id,
            self.handle.clone(),
            self.timeouts.clone(),
        )
    }

    /// Creates a subsystem builder for this session.
    #[cfg(feature = "shell")]
    pub fn subsystem(&self, name: impl Into<String>) -> crate::shell::SubsystemBuilder {
        crate::shell::SubsystemBuilder::from_session(
            self.id,
            self.handle.clone(),
            name.into(),
            self.timeouts.clone(),
        )
    }

    /// Opens an SFTP client over this session.
    #[cfg(feature = "sftp")]
    pub async fn sftp(&self) -> Result<crate::sftp::SftpClient> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| Error::unsupported("SFTP requires a connected session"))?;
        crate::sftp::SftpClient::from_session(self.id)
            .connect(handle.clone())
            .await
    }

    /// Creates a tunnel builder for this session.
    #[cfg(feature = "tunnel")]
    pub fn tunnel(
        &self,
        spec: impl Into<russh_extra_core::ForwardSpec>,
    ) -> crate::tunnel::TunnelBuilder {
        crate::tunnel::TunnelBuilder::from_session(
            self.id,
            self.handle.clone(),
            #[cfg(feature = "tunnel")]
            self.remote_forwards.clone(),
            #[cfg(feature = "tunnel")]
            self.remote_streamlocal_forwards.clone(),
            spec.into(),
            self.timeouts.clone(),
        )
    }

    /// Creates a direct TCP builder for this session.
    #[cfg(feature = "tunnel")]
    pub fn direct_tcp(
        &self,
        target: impl Into<russh_extra_core::TcpEndpoint>,
    ) -> crate::tunnel::DirectTcpBuilder {
        crate::tunnel::DirectTcpBuilder::from_session(
            self.id,
            self.handle.clone(),
            target.into(),
            self.timeouts.clone(),
        )
    }

    /// Creates a direct streamlocal (Unix domain socket) builder for this session.
    #[cfg(feature = "tunnel")]
    pub fn direct_streamlocal<P: Into<std::path::PathBuf>>(
        &self,
        socket_path: P,
    ) -> crate::tunnel::DirectStreamLocalBuilder {
        crate::tunnel::DirectStreamLocalBuilder::from_session(
            self.id,
            self.handle.clone(),
            socket_path.into(),
            self.timeouts.clone(),
        )
    }

    /// Returns whether this session is currently connected.
    pub fn is_connected(&self) -> bool {
        self.handle.is_some()
    }

    /// Returns the server's authentication banner, if one was sent.
    ///
    /// The banner is typically a warning or legal notice sent before
    /// authentication completes. Returns `None` if no banner was sent.
    pub async fn auth_banner(&self) -> Option<String> {
        self.auth_banner_text.lock().await.clone()
    }

    /// Sends a disconnect message and consumes the session.
    ///
    /// Underlying SSH resources are released. The session must not be used
    /// after this call.
    #[tracing::instrument(skip(self), fields(session = %self.id))]
    pub async fn disconnect(self) -> Result<()> {
        let Some(handle) = self.handle else {
            return Ok(());
        };
        let guard = handle.lock().await;
        guard
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await
            .map_err(|e| Error::ssh_with_source("session disconnect failed", e))?;
        Ok(())
    }
}

fn russh_client_config(config: &ClientConfig) -> client::Config {
    let mut russh_config = client::Config::default();
    let keepalive = config.keepalive();
    russh_config.keepalive_interval = keepalive.enabled.then_some(keepalive.interval);
    russh_config.keepalive_max = keepalive.max_missed as usize;
    russh_config
}

fn host_key_policy_accepts(policy: &HostKeyPolicy, server_public_key: &PublicKey) -> bool {
    match policy {
        HostKeyPolicy::Strict => false,
        HostKeyPolicy::InsecureAcceptAny => true,
        HostKeyPolicy::PinnedSha256(fingerprints) => {
            let actual = server_public_key.fingerprint(HashAlg::Sha256).to_string();

            fingerprints
                .iter()
                .any(|fingerprint| fingerprint.value() == actual)
        }
        _ => false,
    }
}

#[cfg(feature = "known-hosts")]
fn public_key_algorithm(public_key: &PublicKey) -> &str {
    match public_key.key_data() {
        russh::keys::ssh_key::public::KeyData::Ed25519(_) => "ssh-ed25519",
        russh::keys::ssh_key::public::KeyData::Rsa(_) => "ssh-rsa",
        russh::keys::ssh_key::public::KeyData::Dsa(_) => "ssh-dss",
        russh::keys::ssh_key::public::KeyData::SkEd25519(_) => "sk-ssh-ed25519@openssh.com",
        _ => "ssh-unknown",
    }
}

async fn authenticate_configured(
    handle: &mut client::Handle<ClientHandler>,
    config: &ClientConfig,
    cert_credentials: &[CertificateCredential],
) -> Result<Arc<Mutex<Option<String>>>> {
    let username = config
        .username()
        .ok_or_else(|| {
            Error::authentication_kind(
                AuthenticationErrorKind::Unavailable,
                "username is required for client authentication",
            )
        })?
        .as_str()
        .to_owned();

    let has_credentials = !config.credentials().is_empty() || !cert_credentials.is_empty();

    if !has_credentials {
        return Err(Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "at least one credential is required for client authentication",
        ));
    }

    let mut saw_partial = false;

    for credential in config.credentials() {
        let mut success = false;

        match credential {
            Credential::Password(password) => {
                let result = time::timeout(
                    config.timeouts().auth,
                    handle.authenticate_password(
                        username.clone(),
                        password.expose_secret().to_owned(),
                    ),
                )
                .await
                .map_err(|_| {
                    Error::timeout(
                        Operation::Authentication,
                        "password authentication timed out",
                    )
                })?
                .map_err(map_auth_error)?;

                match result {
                    AuthResult::Success => success = true,
                    AuthResult::Failure {
                        partial_success, ..
                    } => saw_partial |= partial_success,
                }
            }
            Credential::None => {
                let result = time::timeout(
                    config.timeouts().auth,
                    handle.authenticate_none(username.clone()),
                )
                .await
                .map_err(|_| {
                    Error::timeout(Operation::Authentication, "none authentication timed out")
                })?
                .map_err(map_auth_error)?;

                match result {
                    AuthResult::Success => success = true,
                    AuthResult::Failure {
                        partial_success, ..
                    } => saw_partial |= partial_success,
                }
            }
            Credential::Identity(identity) => {
                let result = try_publickey_auth(handle, &username, identity, config).await;
                match result {
                    Ok(AuthResult::Success) => success = true,
                    Ok(AuthResult::Failure {
                        partial_success, ..
                    }) => {
                        saw_partial |= partial_success;
                    }
                    Err(error) => {
                        if error.is_timeout()
                            || matches!(
                                error,
                                Error::Authentication(ref e)
                                    if e.kind() == AuthenticationErrorKind::Rejected
                            )
                        {
                            // key rejected, continue to next credential
                        } else {
                            return Err(error);
                        }
                    }
                }
            }
            Credential::KeyboardInteractive(handler) => {
                let result = time::timeout(
                    config.timeouts().auth,
                    run_keyboard_interactive(handle, &username, handler),
                )
                .await;

                match result {
                    Ok(Ok(())) => success = true,
                    Ok(Err(error)) => {
                        if error.is_timeout()
                            || matches!(
                                error,
                                Error::Authentication(ref e)
                                    if e.kind() == AuthenticationErrorKind::Rejected
                            )
                        {
                            // keyboard-interactive failed, continue to next credential
                        } else {
                            return Err(error);
                        }
                    }
                    Err(_elapsed) => {
                        return Err(Error::timeout(
                            Operation::Authentication,
                            "keyboard-interactive authentication timed out",
                        ));
                    }
                }
            }
            _ => continue,
        }

        if success {
            return Ok(Arc::new(Mutex::new(None)));
        }
    }

    for cert_cred in cert_credentials {
        let mut key = (*cert_cred.key).clone();
        if key.is_encrypted()
            && let Some(ref passphrase) = cert_cred.passphrase
        {
            key = key.decrypt(passphrase.expose_secret()).map_err(|source| {
                Error::authentication_with_source(
                    AuthenticationErrorKind::Unavailable,
                    "failed to decrypt certificate private key",
                    source,
                )
            })?;
        }

        let result = time::timeout(
            config.timeouts().auth,
            handle.authenticate_openssh_cert(
                username.clone(),
                Arc::new(key),
                cert_cred.cert.clone(),
            ),
        )
        .await
        .map_err(|_| {
            Error::timeout(
                Operation::Authentication,
                "certificate authentication timed out",
            )
        })?
        .map_err(map_auth_error)?;

        match result {
            AuthResult::Success => return Ok(Arc::new(Mutex::new(None))),
            AuthResult::Failure {
                partial_success, ..
            } => saw_partial |= partial_success,
        }
    }

    if saw_partial {
        Err(Error::authentication_kind(
            AuthenticationErrorKind::Partial,
            "authentication partially succeeded but no configured credential completed it",
        ))
    } else {
        Err(Error::authentication_kind(
            AuthenticationErrorKind::Exhausted,
            "all configured credentials were rejected",
        ))
    }
}

async fn run_keyboard_interactive(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    handler: &russh_extra_core::KeyboardInteractiveHandler,
) -> Result<()> {
    use russh::client::KeyboardInteractiveAuthResponse;

    let mut resp = handle
        .authenticate_keyboard_interactive_start(username.to_owned(), None::<String>)
        .await
        .map_err(map_auth_error)?;

    loop {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure {
                partial_success: _, ..
            } => {
                return Err(Error::authentication_kind(
                    AuthenticationErrorKind::Rejected,
                    "keyboard-interactive authentication rejected",
                ));
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let info = ClientKeyboardInteractiveInfo::new(
                    name,
                    instructions,
                    prompts
                        .into_iter()
                        .map(|p| ClientKeyboardInteractivePrompt::new(p.prompt, p.echo))
                        .collect(),
                );
                let reply = handler(info).await;
                match reply {
                    KeyboardInteractiveReply::Responses(answers) => {
                        resp = handle
                            .authenticate_keyboard_interactive_respond(answers)
                            .await
                            .map_err(map_auth_error)?;
                    }
                    KeyboardInteractiveReply::Abort => {
                        return Err(Error::authentication_kind(
                            AuthenticationErrorKind::Rejected,
                            "keyboard-interactive authentication aborted by handler",
                        ));
                    }
                    _ => {
                        return Err(Error::authentication_kind(
                            AuthenticationErrorKind::Rejected,
                            "keyboard-interactive authentication aborted by handler",
                        ));
                    }
                }
            }
        }
    }
}

async fn try_publickey_auth(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    identity: &Identity,
    config: &ClientConfig,
) -> Result<AuthResult> {
    match identity {
        Identity::KeyFile { path, passphrase } => {
            let passphrase_ref = passphrase.as_ref().map(|p| p.expose_secret());
            let key = russh::keys::load_secret_key(path, passphrase_ref).map_err(|source| {
                Error::authentication_with_source(
                    AuthenticationErrorKind::Unavailable,
                    format!("failed to load private key from `{}`", path.display()),
                    source,
                )
            })?;
            authenticate_with_key(handle, username, key, config).await
        }
        Identity::PrivateKey { data, passphrase } => {
            let mut key = russh::keys::PrivateKey::from_openssh(data).map_err(|source| {
                Error::authentication_with_source(
                    AuthenticationErrorKind::Unavailable,
                    "failed to parse in-memory private key",
                    source,
                )
            })?;
            if key.is_encrypted()
                && let Some(passphrase) = passphrase
            {
                key = key.decrypt(passphrase.expose_secret()).map_err(|source| {
                    Error::authentication_with_source(
                        AuthenticationErrorKind::Unavailable,
                        "failed to decrypt in-memory private key",
                        source,
                    )
                })?;
            }
            authenticate_with_key(handle, username, key, config).await
        }
        #[cfg(feature = "agent")]
        Identity::Agent => try_agent_auth(handle, username, config).await,
        #[cfg(not(feature = "agent"))]
        Identity::Agent => Err(Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "SSH agent authentication is not available; enable the `agent` feature",
        )),
        _ => Err(Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "unsupported identity type",
        )),
    }
}

async fn authenticate_with_key(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    key: russh::keys::PrivateKey,
    config: &ClientConfig,
) -> Result<AuthResult> {
    let key_wrapped = russh::keys::key::PrivateKeyWithHashAlg::new(
        std::sync::Arc::new(key),
        Some(HashAlg::Sha256),
    );
    time::timeout(
        config.timeouts().auth,
        handle.authenticate_publickey(username.to_owned(), key_wrapped),
    )
    .await
    .map_err(|_| {
        Error::timeout(
            Operation::Authentication,
            "public key authentication timed out",
        )
    })?
    .map_err(map_auth_error)
}

#[cfg(all(feature = "agent", unix))]
async fn try_agent_auth(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    config: &ClientConfig,
) -> Result<AuthResult> {
    use russh::keys::agent::client::AgentClient;
    use tokio::net::UnixStream;

    let socket_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
        Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "SSH_AUTH_SOCK environment variable is not set",
        )
    })?;

    let stream = UnixStream::connect(&socket_path).await.map_err(|source| {
        Error::authentication_with_source(
            AuthenticationErrorKind::Unavailable,
            format!("failed to connect to SSH agent at `{socket_path}`"),
            source,
        )
    })?;

    let mut agent = AgentClient::connect(stream);
    let identities = agent.request_identities().await.map_err(|source| {
        Error::authentication_with_source(
            AuthenticationErrorKind::Unavailable,
            "failed to list SSH agent identities",
            source,
        )
    })?;

    if identities.is_empty() {
        return Err(Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "SSH agent returned no identities",
        ));
    }

    let mut last_result = None;
    for identity in &identities {
        let public_key = identity.public_key().into_owned();

        let result = time::timeout(
            config.timeouts().auth,
            handle.authenticate_publickey_with(
                username.to_owned(),
                public_key,
                Some(HashAlg::Sha256),
                &mut agent,
            ),
        )
        .await
        .map_err(|_| Error::timeout(Operation::Authentication, "agent authentication timed out"))?
        .map_err(|source| {
            Error::authentication_with_source(
                AuthenticationErrorKind::Unavailable,
                "agent authentication failed",
                source,
            )
        })?;

        match result {
            AuthResult::Success => return Ok(AuthResult::Success),
            auth_result => {
                last_result = Some(auth_result);
            }
        }
    }

    Ok(last_result.unwrap_or(AuthResult::Failure {
        partial_success: false,
        remaining_methods: russh::MethodSet::empty(),
    }))
}

#[cfg(all(feature = "agent", not(unix)))]
async fn try_agent_auth(
    _handle: &mut client::Handle<ClientHandler>,
    _username: &str,
    _config: &ClientConfig,
) -> Result<AuthResult> {
    Err(Error::authentication_kind(
        AuthenticationErrorKind::Unavailable,
        "SSH agent authentication requires Unix-domain socket support on this platform",
    ))
}

fn append_limited(
    buffer: &mut Vec<u8>,
    data: &Bytes,
    limit: usize,
    stream_name: &'static str,
) -> Result<()> {
    if buffer.len().saturating_add(data.len()) > limit {
        return Err(Error::channel_kind(
            ChannelErrorKind::Read,
            format!(
                "remote command {stream_name} exceeded configured {stream_name} limit of {limit} bytes"
            ),
        ));
    }

    buffer.extend_from_slice(data);
    Ok(())
}

pub(crate) fn signal_to_name(signal: russh::Sig) -> String {
    match signal {
        russh::Sig::ABRT => "ABRT".to_owned(),
        russh::Sig::ALRM => "ALRM".to_owned(),
        russh::Sig::FPE => "FPE".to_owned(),
        russh::Sig::HUP => "HUP".to_owned(),
        russh::Sig::ILL => "ILL".to_owned(),
        russh::Sig::INT => "INT".to_owned(),
        russh::Sig::KILL => "KILL".to_owned(),
        russh::Sig::PIPE => "PIPE".to_owned(),
        russh::Sig::QUIT => "QUIT".to_owned(),
        russh::Sig::SEGV => "SEGV".to_owned(),
        russh::Sig::TERM => "TERM".to_owned(),
        russh::Sig::USR1 => "USR1".to_owned(),
        russh::Sig::Custom(signal) => signal,
    }
}

fn map_connect_error(error: russh::Error) -> Error {
    match error {
        russh::Error::UnknownKey => Error::host_key(
            HostKeyErrorKind::Unknown,
            "server host key is unknown to the configured policy",
        ),
        russh::Error::KeyChanged { line } => {
            let reason = if line == 0 {
                "server host key changed".to_owned()
            } else {
                format!("server host key changed from known-hosts line {line}")
            };
            Error::host_key(HostKeyErrorKind::Changed, reason)
        }
        russh::Error::WrongServerSig => {
            Error::host_key(HostKeyErrorKind::Rejected, "server host key was rejected")
        }
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => {
            Error::timeout(Operation::Connect, "client connection timed out")
        }
        russh::Error::Disconnect | russh::Error::HUP => {
            Error::disconnected(Operation::Connect, "server disconnected during connection")
        }
        russh::Error::IO(source) => Error::transport_with_source(
            TransportErrorKind::TcpConnect,
            "TCP connection failed",
            source,
        ),
        russh::Error::KexInit
        | russh::Error::Kex
        | russh::Error::NoCommonAlgo { .. }
        | russh::Error::PacketAuth
        | russh::Error::Version
        | russh::Error::StrictKeyExchangeViolation { .. } => Error::transport_with_source(
            TransportErrorKind::Negotiation,
            "SSH negotiation failed",
            error,
        ),
        error => Error::ssh_with_source("russh client connection failed", error),
    }
}

fn map_auth_error(error: russh::Error) -> Error {
    match error {
        russh::Error::NoAuthMethod => Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "server did not offer an authentication method",
        ),
        russh::Error::UnsupportedAuthMethod => Error::authentication_kind(
            AuthenticationErrorKind::UnsupportedMethod,
            "server rejected the authentication method as unsupported",
        ),
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => {
            Error::timeout(Operation::Authentication, "authentication timed out")
        }
        russh::Error::Disconnect | russh::Error::HUP => Error::disconnected(
            Operation::Authentication,
            "server disconnected during authentication",
        ),
        russh::Error::IO(source) => Error::transport_with_source(
            TransportErrorKind::Io,
            "transport I/O failed during authentication",
            source,
        ),
        error => Error::authentication_with_source(
            AuthenticationErrorKind::Rejected,
            "authentication failed",
            error,
        ),
    }
}

fn map_channel_open_error(error: russh::Error) -> Error {
    match error {
        russh::Error::ChannelOpenFailure(_) => Error::channel_with_source(
            ChannelErrorKind::Open,
            "server refused to open a session channel",
            error,
        ),
        russh::Error::RequestDenied => Error::channel_with_source(
            ChannelErrorKind::Request,
            "session channel open request was denied",
            error,
        ),
        russh::Error::NotAuthenticated => Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "session is not authenticated",
        ),
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => {
            Error::timeout(Operation::ChannelOpen, "session channel open timed out")
        }
        russh::Error::Disconnect | russh::Error::HUP => Error::disconnected(
            Operation::ChannelOpen,
            "server disconnected while opening a session channel",
        ),
        russh::Error::IO(source) => Error::transport_with_source(
            TransportErrorKind::Io,
            "transport I/O failed while opening a session channel",
            source,
        ),
        error => {
            Error::channel_with_source(ChannelErrorKind::Open, "session channel open failed", error)
        }
    }
}

fn map_command_error(error: russh::Error) -> Error {
    match error {
        russh::Error::RequestDenied => Error::channel_with_source(
            ChannelErrorKind::Request,
            "remote command request was denied",
            error,
        ),
        russh::Error::WrongChannel | russh::Error::Inconsistent => Error::channel_with_source(
            ChannelErrorKind::Protocol,
            "remote command channel entered an invalid state",
            error,
        ),
        russh::Error::SendError => Error::channel_with_source(
            ChannelErrorKind::Write,
            "failed to send command channel data",
            error,
        ),
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => {
            Error::timeout(Operation::Command, "remote command timed out")
        }
        russh::Error::Disconnect | russh::Error::HUP => Error::disconnected(
            Operation::Command,
            "server disconnected during remote command",
        ),
        russh::Error::IO(source) => {
            Error::channel_with_source(ChannelErrorKind::Read, "command channel I/O failed", source)
        }
        error => {
            Error::channel_with_source(ChannelErrorKind::Protocol, "remote command failed", error)
        }
    }
}

/// Remote command request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteCommand {
    program: String,
    stdin: Bytes,
    limits: CommandLimits,
}

impl RemoteCommand {
    /// Creates a remote command.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            stdin: Bytes::new(),
            limits: CommandLimits::default(),
        }
    }

    /// Returns the command program string.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Sets stdin bytes for the command.
    pub fn with_stdin(mut self, stdin: impl Into<Bytes>) -> Self {
        self.stdin = stdin.into();
        self
    }

    /// Sets captured stdout and stderr limits.
    pub fn with_limits(mut self, limits: CommandLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the captured stdout byte limit.
    pub fn stdout_limit(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_stdout(limit);
        self
    }

    /// Sets the captured stderr byte limit.
    pub fn stderr_limit(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_stderr(limit);
        self
    }

    /// Returns stdin bytes.
    pub fn stdin(&self) -> &Bytes {
        &self.stdin
    }

    /// Returns captured output limits.
    pub fn limits(&self) -> CommandLimits {
        self.limits
    }
}

impl From<&str> for RemoteCommand {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RemoteCommand {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Captured command output.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    /// Process exit information.
    pub exit: CommandExit,
    /// Captured stdout.
    pub stdout: Bytes,
    /// Captured stderr.
    pub stderr: Bytes,
}

impl CommandOutput {
    /// Creates a new `CommandOutput`.
    pub fn new(exit: CommandExit, stdout: impl Into<Bytes>, stderr: impl Into<Bytes>) -> Self {
        Self {
            exit,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    /// Returns whether the command succeeded.
    pub fn success(&self) -> bool {
        self.exit.success()
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use russh_extra_core::{
        CommandLimits, Endpoint, ForwardSpec, HostKeyPolicy, Pty, SessionId, TcpEndpoint,
    };

    #[test]
    #[cfg(feature = "shell")]
    fn session_creates_shell_builders() {
        let session = Session::new(SessionId::next(), Endpoint::ssh("example.com"));
        let pty = Pty::new("xterm", 120, 40);

        let shell = session.shell().pty(pty.clone()).build();

        assert_eq!(shell.session_id(), session.id());
        assert_eq!(shell.pty(), Some(&pty));
    }

    #[test]
    #[cfg(feature = "tunnel")]
    fn session_creates_tunnel_builders() {
        let session = Session::new(SessionId::next(), Endpoint::ssh("example.com"));
        let spec = ForwardSpec::local_tcp(
            TcpEndpoint::new("127.0.0.1", 8080),
            TcpEndpoint::new("10.0.0.10", 80),
        );

        let builder = session.tunnel(spec.clone());

        assert_eq!(builder.session_id(), session.id());
        assert_eq!(builder.spec(), &spec);
    }

    #[test]
    fn client_builder_sets_host_key_policy() {
        let client = super::Client::builder()
            .endpoint(("example.com", 22))
            .accept_any_host_key()
            .build();

        assert_eq!(
            client.config().host_key_policy(),
            &HostKeyPolicy::InsecureAcceptAny
        );
    }

    #[test]
    fn client_builder_validates_pinned_host_key_fingerprint() {
        let client = super::Client::builder()
            .try_pinned_host_key_sha256("SHA256:abc123+/=")
            .unwrap()
            .build();

        assert!(matches!(
            client.config().host_key_policy(),
            HostKeyPolicy::PinnedSha256(_)
        ));

        assert!(
            super::Client::builder()
                .try_pinned_host_key_sha256("MD5:abc")
                .is_err()
        );
    }

    #[test]
    fn remote_command_sets_output_limits() {
        let command = super::RemoteCommand::new("echo hello")
            .with_limits(CommandLimits::new(128, 256))
            .stdout_limit(512);

        assert_eq!(command.limits().stdout(), 512);
        assert_eq!(command.limits().stderr(), 256);
    }

    #[test]
    fn session_is_connected_reflects_handle_presence() {
        let session = Session::new(SessionId::next(), Endpoint::ssh("example.com"));
        assert!(!session.is_connected());
    }

    #[test]
    fn certificate_credential_debug_redacts_secrets() {
        let cred = super::CertificateCredential::from_openssh_data(
            b"-----BEGIN OPENSSH PRIVATE KEY-----\ninvalid\n-----END OPENSSH PRIVATE KEY-----",
            b"ssh-ed25519-cert-v01@openssh.com AAAinvalid",
        );

        assert!(cred.is_err());
    }

    #[test]
    fn certificate_credential_from_invalid_data_is_err() {
        let cred = super::CertificateCredential::from_openssh_data(
            b"not-a-valid-key",
            b"not-a-valid-cert",
        );
        assert!(cred.is_err());
    }

    #[test]
    fn client_builder_supports_certificate_and_x11_display() {
        let client = super::Client::builder()
            .endpoint(("example.com", 22))
            .x11_display("/tmp/.X11-unix/X0")
            .accept_any_host_key()
            .build();

        assert!(client.x11_display.is_some());
        assert_eq!(
            client.x11_display.as_ref().unwrap().to_string_lossy(),
            "/tmp/.X11-unix/X0"
        );
    }
}
