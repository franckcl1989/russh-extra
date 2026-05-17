//! Server-side high-level SSH APIs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use russh::keys::{Certificate, PrivateKey, ssh_key::PublicKey};
use russh::server::{self, Auth, Msg, Server as _};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use russh_extra_core::{
    AuthenticationErrorKind, ChannelErrorKind, CommandExit, Endpoint, Error, Operation, Password,
    Result, ServerConfig, SessionId, TransportErrorKind, Username,
};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

type BoxFutureResult<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;
type PasswordAuthCallback =
    Arc<dyn Fn(AuthContext, Password) -> BoxFutureResult<AuthDecision> + Send + Sync>;
type PublicKeyAuthCallback =
    Arc<dyn Fn(AuthContext, PublicKey) -> BoxFutureResult<AuthDecision> + Send + Sync>;
type ExecCallback = Arc<dyn Fn(ExecContext) -> BoxFutureResult<ExecResponse> + Send + Sync>;
type ShellCallback = Arc<dyn Fn(ShellContext) -> BoxFutureResult<()> + Send + Sync>;
type PtyCallback = Arc<dyn Fn(PtyContext, PtyParams) -> BoxFutureResult<()> + Send + Sync>;
type SubsystemCallback = Arc<dyn Fn(SubsystemContext) -> BoxFutureResult<()> + Send + Sync>;
type EnvCallback = Arc<dyn Fn(ShellContext, EnvRequest) -> BoxFutureResult<()> + Send + Sync>;
type WindowChangeCallback =
    Arc<dyn Fn(ShellContext, WindowChange) -> BoxFutureResult<()> + Send + Sync>;
type TcpipForwardCallback = Arc<dyn Fn(TcpipForwardContext) -> BoxFutureResult<bool> + Send + Sync>;
type DirectTcpipCallback = Arc<dyn Fn(DirectTcpipContext) -> BoxFutureResult<bool> + Send + Sync>;
type ForwardedTcpipCallback =
    Arc<dyn Fn(ForwardedTcpipContext) -> BoxFutureResult<bool> + Send + Sync>;
type CancelTcpipForwardCallback =
    Arc<dyn Fn(TcpipForwardContext) -> BoxFutureResult<bool> + Send + Sync>;
type StreamLocalForwardCallback =
    Arc<dyn Fn(StreamLocalForwardContext) -> BoxFutureResult<bool> + Send + Sync>;
type CancelStreamLocalForwardCallback =
    Arc<dyn Fn(StreamLocalForwardContext) -> BoxFutureResult<bool> + Send + Sync>;
type DirectStreamLocalCallback =
    Arc<dyn Fn(DirectStreamLocalContext) -> BoxFutureResult<bool> + Send + Sync>;
type ConnectCallback = Arc<dyn Fn(SessionContext) -> BoxFutureResult<()> + Send + Sync>;
type DisconnectCallback = Arc<dyn Fn(SessionId) -> BoxFutureResult<()> + Send + Sync>;
type AuthSuccessCallback = Arc<dyn Fn(SessionContext) -> BoxFutureResult<()> + Send + Sync>;
type CertAuthCallback =
    Arc<dyn Fn(AuthContext, Certificate) -> BoxFutureResult<AuthDecision> + Send + Sync>;
type X11RequestCallback = Arc<dyn Fn(X11RequestContext) -> BoxFutureResult<bool> + Send + Sync>;
type X11ChannelCallback = Arc<dyn Fn(X11ChannelContext) -> BoxFutureResult<bool> + Send + Sync>;
type AgentRequestCallback = Arc<dyn Fn(AgentRequestContext) -> BoxFutureResult<bool> + Send + Sync>;
type AuthBannerCallback = Arc<dyn Fn() -> BoxFutureResult<Option<String>> + Send + Sync>;
type StreamingExecCallback = Arc<dyn Fn(StreamingExecContext) -> BoxFutureResult<()> + Send + Sync>;
type KeyboardInteractiveCallback = Arc<
    dyn Fn(KeyboardInteractiveContext) -> BoxFutureResult<KeyboardInteractiveResponse>
        + Send
        + Sync,
>;

/// High-level SSH server.
#[derive(Clone)]
pub struct Server {
    config: ServerConfig,
    runtime: Arc<ServerRuntime>,
}

impl Server {
    /// Creates a server builder.
    pub fn builder() -> ServerBuilder {
        ServerBuilder::default()
    }

    /// Returns server configuration.
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Returns a handle that can request shutdown.
    pub fn handle(&self) -> ServerHandle {
        self.runtime.handle.clone()
    }

    /// Runs the server until shutdown.
    pub async fn run(self) -> Result<()> {
        self.run_inner(std::future::pending::<()>()).await
    }

    /// Runs the server until the given shutdown future resolves.
    #[tracing::instrument(skip(self, shutdown), fields(listen = %self.config.listen()))]
    pub async fn run_until<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()>,
    {
        self.run_inner(shutdown).await
    }

    #[tracing::instrument(skip(self, shutdown), fields(listen = %self.config.listen()))]
    async fn run_inner<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()>,
    {
        let listener = TcpListener::bind((
            self.config.listen().host().to_owned(),
            self.config.listen().port(),
        ))
        .await?;
        let runtime = Arc::clone(&self.runtime);
        let russh_config = Arc::new(runtime.russh_config());
        let mut russh_server = HighLevelRusshServer {
            runtime: Arc::clone(&runtime),
        };
        let mut running = russh_server.run_on_socket(russh_config, &listener);
        let running_handle = running.handle();
        let mut shutdown_rx = runtime.handle.shutdown_tx.subscribe();
        let handle_shutdown = async {
            loop {
                if let Some(reason) = shutdown_rx.borrow().clone() {
                    break reason;
                }

                if shutdown_rx.changed().await.is_err() {
                    break "russh-extra server handle closed".to_owned();
                }
            }
        };

        tokio::pin!(shutdown);
        tokio::pin!(handle_shutdown);

        let result = tokio::select! {
            result = &mut running => result.map_err(Error::from),
            reason = &mut handle_shutdown => {
                running_handle.shutdown(reason);
                running.await.map_err(Error::from)
            }
            () = &mut shutdown => {
                running_handle.shutdown("russh-extra server shutdown requested".to_owned());
                running.await.map_err(Error::from)
            }
        };

        result?;

        if let Some(error) = runtime.take_error() {
            return Err(error);
        }

        Ok(())
    }
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("config", &self.config)
            .field("host_key_count", &self.runtime.host_keys.len())
            .field("exec_route_count", &self.runtime.exec_routes.len())
            .finish()
    }
}

/// Builder for [`Server`].
pub struct ServerBuilder {
    config: ServerConfig,
    host_keys: Vec<ServerHostKeySource>,
    password_auth: Option<PasswordAuthCallback>,
    public_key_auth: Option<PublicKeyAuthCallback>,
    keyboard_interactive_auth: Option<KeyboardInteractiveCallback>,
    exec_routes: HashMap<String, ExecCallback>,
    fallback_exec: Option<ExecCallback>,
    exec_routes_streaming: HashMap<String, StreamingExecCallback>,
    fallback_streaming_exec: Option<StreamingExecCallback>,
    shell_handler: Option<ShellCallback>,
    pty_handler: Option<PtyCallback>,
    subsystem_handler: Option<SubsystemCallback>,
    env_handler: Option<EnvCallback>,
    window_change_handler: Option<WindowChangeCallback>,
    tcpip_forward_handler: Option<TcpipForwardCallback>,
    cancel_tcpip_forward_handler: Option<CancelTcpipForwardCallback>,
    direct_tcpip_handler: Option<DirectTcpipCallback>,
    forwarded_tcpip_handler: Option<ForwardedTcpipCallback>,
    streamlocal_forward_handler: Option<StreamLocalForwardCallback>,
    cancel_streamlocal_forward_handler: Option<CancelStreamLocalForwardCallback>,
    direct_streamlocal_handler: Option<DirectStreamLocalCallback>,
    shutdown_grace: Duration,
    on_connect: Option<ConnectCallback>,
    on_disconnect: Option<DisconnectCallback>,
    on_auth_success: Option<AuthSuccessCallback>,
    cert_auth: Option<CertAuthCallback>,
    x11_request_handler: Option<X11RequestCallback>,
    x11_channel_handler: Option<X11ChannelCallback>,
    agent_request_handler: Option<AgentRequestCallback>,
    auth_banner: Option<AuthBannerCallback>,
    #[cfg(feature = "sftp")]
    sftp_handler: Option<std::sync::Arc<dyn crate::sftp::SftpServerHandler + Send + Sync>>,
}

impl ServerBuilder {
    /// Sets the listen endpoint.
    pub fn listen(mut self, listen: impl Into<Endpoint>) -> Self {
        self.config.set_listen(listen);
        self
    }

    /// Sets the SSH server identification string.
    pub fn server_id(mut self, server_id: impl Into<String>) -> Self {
        self.config.set_server_id(server_id);
        self
    }

    /// Sets the maximum session count per connection.
    pub fn max_sessions(mut self, max_sessions: usize) -> Self {
        self.config.set_max_sessions(max_sessions);
        self
    }

    /// Sets the graceful shutdown wait period.
    pub fn shutdown_grace(mut self, shutdown_grace: Duration) -> Self {
        self.shutdown_grace = shutdown_grace;
        self
    }

    /// Adds an already loaded host key.
    pub fn host_key(mut self, host_key: ServerHostKey) -> Self {
        self.host_keys
            .push(ServerHostKeySource::Loaded(Box::new(host_key)));
        self
    }

    /// Adds a host key loaded from an OpenSSH private-key file during build.
    pub fn host_key_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.host_keys
            .push(ServerHostKeySource::OpenSshFile(path.into()));
        self
    }

    /// Configures password authentication.
    pub fn password_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AuthContext, Password) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<AuthDecision>> + Send + 'static,
    {
        self.password_auth = Some(Arc::new(move |ctx, password| {
            Box::pin(handler(ctx, password))
        }));
        self
    }

    /// Configures public key authentication.
    pub fn public_key_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AuthContext, PublicKey) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<AuthDecision>> + Send + 'static,
    {
        self.public_key_auth = Some(Arc::new(move |ctx, public_key| {
            Box::pin(handler(ctx, public_key))
        }));
        self
    }

    /// Configures keyboard-interactive authentication.
    ///
    /// The handler is called once per challenge round. For the initial
    /// request, `responses` in the context is empty. The handler may
    /// return [`KeyboardInteractiveResponse::FurtherAction`] to send
    /// prompts, [`KeyboardInteractiveResponse::Accept`] to accept, or
    /// [`KeyboardInteractiveResponse::Reject`] to reject.
    pub fn keyboard_interactive_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(KeyboardInteractiveContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<KeyboardInteractiveResponse>> + Send + 'static,
    {
        self.keyboard_interactive_auth = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers an exact command route.
    pub fn exec<F, Fut>(mut self, command: impl Into<String>, handler: F) -> Self
    where
        F: Fn(ExecContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ExecResponse>> + Send + 'static,
    {
        self.exec_routes
            .insert(command.into(), Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a streaming exec handler for an exact command.
    ///
    /// Streaming handlers own the channel for their duration, reading stdin
    /// and writing stdout/stderr progressively through [`StreamingExecContext`].
    pub fn streaming_exec<F, Fut>(mut self, command: impl Into<String>, handler: F) -> Self
    where
        F: Fn(StreamingExecContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.exec_routes_streaming
            .insert(command.into(), Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Uses a stateful handler for authentication and command execution.
    pub fn handler<H>(mut self, handler: H) -> Self
    where
        H: ServerHandler,
    {
        let password_handler = Arc::new(handler.clone());
        self.password_auth = Some(Arc::new(move |ctx, password| {
            let password_handler = Arc::clone(&password_handler);
            Box::pin(async move { password_handler.auth_password(ctx, password).await })
        }));

        let pubkey_handler = Arc::new(handler.clone());
        self.public_key_auth = Some(Arc::new(move |ctx, public_key| {
            let pubkey_handler = Arc::clone(&pubkey_handler);
            Box::pin(async move { pubkey_handler.auth_publickey(ctx, public_key).await })
        }));

        let kbdint_handler = Arc::new(handler.clone());
        self.keyboard_interactive_auth = Some(Arc::new(move |ctx| {
            let kbdint_handler = Arc::clone(&kbdint_handler);
            Box::pin(async move { kbdint_handler.auth_keyboard_interactive(ctx).await })
        }));

        let exec_handler = Arc::new(handler.clone());
        self.fallback_exec = Some(Arc::new(move |ctx| {
            let exec_handler = Arc::clone(&exec_handler);
            Box::pin(async move { exec_handler.exec(ctx).await })
        }));

        let streaming_exec_handler = Arc::new(handler.clone());
        self.fallback_streaming_exec = Some(Arc::new(move |ctx| {
            let streaming_exec_handler = Arc::clone(&streaming_exec_handler);
            Box::pin(async move { streaming_exec_handler.streaming_exec(ctx).await })
        }));

        let shell_handler = Arc::new(handler.clone());
        self.shell_handler = Some(Arc::new(move |ctx| {
            let shell_handler = Arc::clone(&shell_handler);
            Box::pin(async move { shell_handler.shell(ctx).await })
        }));

        let pty_handler = Arc::new(handler.clone());
        self.pty_handler = Some(Arc::new(move |ctx, params| {
            let pty_handler = Arc::clone(&pty_handler);
            Box::pin(async move { pty_handler.pty(ctx, params).await })
        }));

        let subsystem_handler = Arc::new(handler.clone());
        self.subsystem_handler = Some(Arc::new(move |ctx| {
            let subsystem_handler = Arc::clone(&subsystem_handler);
            Box::pin(async move { subsystem_handler.subsystem(ctx).await })
        }));

        let env_handler = Arc::new(handler.clone());
        self.env_handler = Some(Arc::new(move |ctx, request| {
            let env_handler = Arc::clone(&env_handler);
            Box::pin(async move { env_handler.env(ctx, request).await })
        }));

        let winch_handler = Arc::new(handler.clone());
        self.window_change_handler = Some(Arc::new(move |ctx, change| {
            let winch_handler = Arc::clone(&winch_handler);
            Box::pin(async move { winch_handler.window_change(ctx, change).await })
        }));

        let tcpip_handler = Arc::new(handler.clone());
        self.tcpip_forward_handler = Some(Arc::new(move |ctx| {
            let tcpip_handler = Arc::clone(&tcpip_handler);
            Box::pin(async move { tcpip_handler.tcpip_forward(ctx).await })
        }));

        let cancel_tcpip_handler = Arc::new(handler.clone());
        self.cancel_tcpip_forward_handler = Some(Arc::new(move |ctx| {
            let cancel_tcpip_handler = Arc::clone(&cancel_tcpip_handler);
            Box::pin(async move { cancel_tcpip_handler.cancel_tcpip_forward(ctx).await })
        }));

        let direct_tcpip_handler = Arc::new(handler.clone());
        self.direct_tcpip_handler = Some(Arc::new(move |ctx| {
            let direct_tcpip_handler = Arc::clone(&direct_tcpip_handler);
            Box::pin(async move { direct_tcpip_handler.channel_open_direct_tcpip(ctx).await })
        }));

        let fwd_tcpip_handler = Arc::new(handler.clone());
        self.forwarded_tcpip_handler = Some(Arc::new(move |ctx| {
            let fwd_tcpip_handler = Arc::clone(&fwd_tcpip_handler);
            Box::pin(async move { fwd_tcpip_handler.channel_open_forwarded_tcpip(ctx).await })
        }));

        let streamlocal_handler = Arc::new(handler.clone());
        self.streamlocal_forward_handler = Some(Arc::new(move |ctx| {
            let streamlocal_handler = Arc::clone(&streamlocal_handler);
            Box::pin(async move { streamlocal_handler.streamlocal_forward(ctx).await })
        }));

        let cancel_streamlocal_handler = Arc::new(handler.clone());
        self.cancel_streamlocal_forward_handler = Some(Arc::new(move |ctx| {
            let cancel_streamlocal_handler = Arc::clone(&cancel_streamlocal_handler);
            Box::pin(async move {
                cancel_streamlocal_handler
                    .cancel_streamlocal_forward(ctx)
                    .await
            })
        }));

        let direct_streamlocal_handler = Arc::new(handler.clone());
        self.direct_streamlocal_handler = Some(Arc::new(move |ctx| {
            let direct_streamlocal_handler = Arc::clone(&direct_streamlocal_handler);
            Box::pin(async move {
                direct_streamlocal_handler
                    .channel_open_direct_streamlocal(ctx)
                    .await
            })
        }));

        let connect_handler = Arc::new(handler.clone());
        self.on_connect = Some(Arc::new(move |ctx| {
            let connect_handler = Arc::clone(&connect_handler);
            Box::pin(async move { connect_handler.on_connect(ctx).await })
        }));

        let disconnect_handler = Arc::new(handler.clone());
        self.on_disconnect = Some(Arc::new(move |id| {
            let disconnect_handler = Arc::clone(&disconnect_handler);
            Box::pin(async move { disconnect_handler.on_disconnect(id).await })
        }));

        let auth_success_handler = Arc::new(handler.clone());
        self.on_auth_success = Some(Arc::new(move |ctx| {
            let auth_success_handler = Arc::clone(&auth_success_handler);
            Box::pin(async move { auth_success_handler.on_auth_success(ctx).await })
        }));

        let cert_handler = Arc::new(handler.clone());
        self.cert_auth = Some(Arc::new(move |ctx, cert| {
            let cert_handler = Arc::clone(&cert_handler);
            Box::pin(async move { cert_handler.auth_openssh_certificate(ctx, cert).await })
        }));

        let x11_req_handler = Arc::new(handler.clone());
        self.x11_request_handler = Some(Arc::new(move |ctx| {
            let x11_req_handler = Arc::clone(&x11_req_handler);
            Box::pin(async move { x11_req_handler.x11_request(ctx).await })
        }));

        let x11_ch_handler = Arc::new(handler.clone());
        self.x11_channel_handler = Some(Arc::new(move |ctx| {
            let x11_ch_handler = Arc::clone(&x11_ch_handler);
            Box::pin(async move { x11_ch_handler.channel_open_x11(ctx).await })
        }));

        let agent_req_handler = Arc::new(handler.clone());
        self.agent_request_handler = Some(Arc::new(move |ctx| {
            let agent_req_handler = Arc::clone(&agent_req_handler);
            Box::pin(async move { agent_req_handler.agent_request(ctx).await })
        }));

        let banner_handler = Arc::new(handler);
        self.auth_banner = Some(Arc::new(move || {
            let handler = Arc::clone(&banner_handler);
            Box::pin(async move { handler.authentication_banner().await })
        }));

        self
    }

    /// Registers a shell handler.
    pub fn shell_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(ShellContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.shell_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a PTY handler.
    pub fn pty_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(PtyContext, PtyParams) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.pty_handler = Some(Arc::new(move |ctx, params| Box::pin(handler(ctx, params))));
        self
    }

    /// Registers a subsystem handler.
    pub fn subsystem_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(SubsystemContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.subsystem_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers an SFTP server handler.
    ///
    /// When the `sftp` subsystem is requested, the handler receives
    /// decoded SFTP requests and returns responses.  Requires both
    /// the `sftp` and `server` features.
    #[cfg(feature = "sftp")]
    pub fn sftp_handler<H>(mut self, handler: H) -> Self
    where
        H: crate::sftp::SftpServerHandler + Send + Sync + 'static,
    {
        self.sftp_handler = Some(std::sync::Arc::new(handler));
        self
    }

    /// Registers an environment-variable handler.
    pub fn env_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(ShellContext, EnvRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.env_handler = Some(Arc::new(move |ctx, request| {
            Box::pin(handler(ctx, request))
        }));
        self
    }

    /// Registers a window-change handler.
    pub fn window_change_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(ShellContext, WindowChange) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.window_change_handler =
            Some(Arc::new(move |ctx, change| Box::pin(handler(ctx, change))));
        self
    }

    /// Registers a TCP/IP forwarding handler.
    pub fn tcpip_forward_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(TcpipForwardContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.tcpip_forward_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a cancel TCP/IP forwarding handler.
    pub fn cancel_tcpip_forward_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(TcpipForwardContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.cancel_tcpip_forward_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a direct TCP/IP channel handler.
    pub fn direct_tcpip_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(DirectTcpipContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.direct_tcpip_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a forwarded TCP/IP channel handler.
    pub fn forwarded_tcpip_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(ForwardedTcpipContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.forwarded_tcpip_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a streamlocal forwarding handler.
    pub fn streamlocal_forward_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(StreamLocalForwardContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.streamlocal_forward_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a cancel streamlocal forwarding handler.
    pub fn cancel_streamlocal_forward_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(StreamLocalForwardContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.cancel_streamlocal_forward_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a direct streamlocal channel handler.
    pub fn direct_streamlocal_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(DirectStreamLocalContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.direct_streamlocal_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a connection lifecycle callback.
    ///
    /// Called (via `tokio::spawn`) when a new client TCP connection is
    /// accepted, before any SSH authentication. The callback runs in a
    /// spawned task and its result is logged but not propagated.
    pub fn on_connect<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(SessionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_connect = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers a disconnection callback.
    ///
    /// Called when a client session ends (normal close, disconnect, or error).
    /// The callback runs in a spawned task.
    pub fn on_disconnect<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(SessionId) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_disconnect = Some(Arc::new(move |id| Box::pin(handler(id))));
        self
    }

    /// Registers an authentication-success callback.
    ///
    /// Called when a client successfully authenticates (password or public key).
    /// The callback receives the session context with the authenticated username.
    pub fn on_auth_success<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(SessionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_auth_success = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers an OpenSSH certificate authentication handler.
    pub fn certificate_auth<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AuthContext, Certificate) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<AuthDecision>> + Send + 'static,
    {
        self.cert_auth = Some(Arc::new(move |ctx, cert| Box::pin(handler(ctx, cert))));
        self
    }

    /// Registers an X11 request handler.
    ///
    /// Called when a client requests X11 forwarding on an existing channel.
    /// Return `true` to accept the forwarding, `false` to reject.
    pub fn x11_request_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(X11RequestContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.x11_request_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers an X11 channel open handler.
    ///
    /// Called when a client opens an X11 forwarding channel. Return `true`
    /// to accept the channel, `false` to reject.
    pub fn x11_channel_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(X11ChannelContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.x11_channel_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Registers an agent forwarding request handler.
    ///
    /// Called when a client requests agent forwarding. Return `true` to
    /// accept, `false` to reject.
    pub fn agent_request_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AgentRequestContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.agent_request_handler = Some(Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Configures an authentication banner.
    ///
    /// The banner is a message sent to the client during the authentication
    /// phase, typically a warning or legal notice. Return `None` to send
    /// no banner.
    pub fn banner<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        self.auth_banner = Some(Arc::new(move || Box::pin(handler())));
        self
    }

    /// Builds the server.
    pub fn build(self) -> Result<Server> {
        if self.config.max_sessions() == 0 {
            return Err(Error::invalid_config(
                "server max_sessions must be greater than zero",
            ));
        }

        let host_keys = self
            .host_keys
            .into_iter()
            .map(ServerHostKeySource::load)
            .collect::<Result<Vec<_>>>()?;

        if host_keys.is_empty() {
            return Err(Error::invalid_config(
                "server requires at least one host key",
            ));
        }

        let (shutdown_tx, _shutdown_rx) = watch::channel(None::<String>);
        let handle = ServerHandle { shutdown_tx };
        let runtime = Arc::new(ServerRuntime {
            config: self.config.clone(),
            host_keys,
            password_auth: self.password_auth,
            public_key_auth: self.public_key_auth,
            keyboard_interactive_auth: self.keyboard_interactive_auth,
            exec_routes: self.exec_routes,
            fallback_exec: self.fallback_exec,
            exec_routes_streaming: self.exec_routes_streaming,
            fallback_streaming_exec: self.fallback_streaming_exec,
            shell_handler: self.shell_handler,
            pty_handler: self.pty_handler,
            subsystem_handler: self.subsystem_handler,
            env_handler: self.env_handler,
            window_change_handler: self.window_change_handler,
            tcpip_forward_handler: self.tcpip_forward_handler,
            cancel_tcpip_forward_handler: self.cancel_tcpip_forward_handler,
            direct_tcpip_handler: self.direct_tcpip_handler,
            forwarded_tcpip_handler: self.forwarded_tcpip_handler,
            streamlocal_forward_handler: self.streamlocal_forward_handler,
            cancel_streamlocal_forward_handler: self.cancel_streamlocal_forward_handler,
            direct_streamlocal_handler: self.direct_streamlocal_handler,
            shutdown_grace: self.shutdown_grace,
            handle,
            last_error: Mutex::new(None),
            on_connect: self.on_connect,
            on_disconnect: self.on_disconnect,
            on_auth_success: self.on_auth_success,
            cert_auth: self.cert_auth,
            x11_request_handler: self.x11_request_handler,
            x11_channel_handler: self.x11_channel_handler,
            agent_request_handler: self.agent_request_handler,
            auth_banner: self.auth_banner,
            #[cfg(feature = "sftp")]
            sftp_handler: self.sftp_handler,
        });

        Ok(Server {
            config: self.config,
            runtime,
        })
    }
}

impl fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("config", &self.config)
            .field("host_key_count", &self.host_keys.len())
            .field("has_password_auth", &self.password_auth.is_some())
            .field("has_public_key_auth", &self.public_key_auth.is_some())
            .field(
                "has_keyboard_interactive_auth",
                &self.keyboard_interactive_auth.is_some(),
            )
            .field("exec_route_count", &self.exec_routes.len())
            .field("has_fallback_exec", &self.fallback_exec.is_some())
            .field("has_shell_handler", &self.shell_handler.is_some())
            .field("has_pty_handler", &self.pty_handler.is_some())
            .field("has_subsystem_handler", &self.subsystem_handler.is_some())
            .field("has_env_handler", &self.env_handler.is_some())
            .field(
                "has_window_change_handler",
                &self.window_change_handler.is_some(),
            )
            .field("shutdown_grace", &self.shutdown_grace)
            .field("has_on_connect", &self.on_connect.is_some())
            .field("has_on_disconnect", &self.on_disconnect.is_some())
            .finish()
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            config: ServerConfig::default(),
            host_keys: Vec::new(),
            password_auth: None,
            public_key_auth: None,
            keyboard_interactive_auth: None,
            exec_routes: HashMap::new(),
            fallback_exec: None,
            exec_routes_streaming: HashMap::new(),
            fallback_streaming_exec: None,
            shell_handler: None,
            pty_handler: None,
            subsystem_handler: None,
            env_handler: None,
            window_change_handler: None,
            tcpip_forward_handler: None,
            cancel_tcpip_forward_handler: None,
            direct_tcpip_handler: None,
            forwarded_tcpip_handler: None,
            streamlocal_forward_handler: None,
            cancel_streamlocal_forward_handler: None,
            direct_streamlocal_handler: None,
            shutdown_grace: Duration::from_secs(30),
            on_connect: None,
            on_disconnect: None,
            on_auth_success: None,
            cert_auth: None,
            x11_request_handler: None,
            x11_channel_handler: None,
            agent_request_handler: None,
            auth_banner: None,
            #[cfg(feature = "sftp")]
            sftp_handler: None,
        }
    }
}

/// Cloneable handle for requesting server shutdown.
#[derive(Clone)]
pub struct ServerHandle {
    shutdown_tx: watch::Sender<Option<String>>,
}

impl ServerHandle {
    /// Requests graceful shutdown.
    pub fn shutdown(&self, reason: impl Into<String>) {
        let mut reason = Some(reason.into());
        self.shutdown_tx.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }

            *current = reason.take();
            true
        });
    }

    /// Returns whether shutdown has been requested.
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_tx.borrow().is_some()
    }
}

impl fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerHandle")
            .field("shutdown_requested", &self.is_shutdown_requested())
            .finish()
    }
}

/// SSH server host key.
#[derive(Clone)]
pub struct ServerHostKey {
    private_key: PrivateKey,
}

impl ServerHostKey {
    /// Creates a server host key from a lower-level `russh` private key.
    pub fn from_private_key(private_key: PrivateKey) -> Self {
        Self { private_key }
    }

    /// Loads an OpenSSH private key from a file.
    pub fn from_openssh_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        validate_host_key_permissions(path)?;
        let private_key = PrivateKey::read_openssh_file(path)
            .map_err(|source| Error::ssh_with_source("failed to read server host key", source))?;
        Self::from_loaded_private_key(private_key)
    }

    /// Loads an OpenSSH private key from PEM bytes.
    pub fn from_openssh_pem(pem: impl AsRef<[u8]>) -> Result<Self> {
        let private_key = PrivateKey::from_openssh(pem)
            .map_err(|source| Error::ssh_with_source("failed to parse server host key", source))?;
        Self::from_loaded_private_key(private_key)
    }

    /// Loads and decrypts an OpenSSH private key from PEM bytes.
    pub fn from_openssh_pem_with_passphrase(
        pem: impl AsRef<[u8]>,
        passphrase: impl Into<Password>,
    ) -> Result<Self> {
        let private_key = PrivateKey::from_openssh(pem)
            .map_err(|source| Error::ssh_with_source("failed to parse server host key", source))?;
        let passphrase = passphrase.into();
        let private_key = if private_key.is_encrypted() {
            private_key
                .decrypt(passphrase.expose_secret())
                .map_err(|source| {
                    Error::ssh_with_source("failed to decrypt server host key", source)
                })?
        } else {
            private_key
        };

        Self::from_loaded_private_key(private_key)
    }

    /// Loads and decrypts an OpenSSH private key from a file.
    pub fn from_openssh_file_with_passphrase(
        path: impl AsRef<Path>,
        passphrase: impl Into<Password>,
    ) -> Result<Self> {
        let path = path.as_ref();
        validate_host_key_permissions(path)?;
        let pem = std::fs::read(path)?;
        Self::from_openssh_pem_with_passphrase(pem, passphrase)
    }

    fn from_loaded_private_key(private_key: PrivateKey) -> Result<Self> {
        if private_key.is_encrypted() {
            return Err(Error::invalid_config(
                "server host key is encrypted and requires a passphrase",
            ));
        }

        Ok(Self { private_key })
    }

    fn into_private_key(self) -> PrivateKey {
        self.private_key
    }
}

impl fmt::Debug for ServerHostKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerHostKey")
            .field("private_key", &"***")
            .finish()
    }
}

enum ServerHostKeySource {
    Loaded(Box<ServerHostKey>),
    OpenSshFile(PathBuf),
}

impl ServerHostKeySource {
    fn load(self) -> Result<PrivateKey> {
        match self {
            Self::Loaded(host_key) => Ok(host_key.into_private_key()),
            Self::OpenSshFile(path) => {
                Ok(ServerHostKey::from_openssh_file(path)?.into_private_key())
            }
        }
    }
}

impl fmt::Debug for ServerHostKeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loaded(_) => f.write_str("Loaded(ServerHostKey(***)"),
            Self::OpenSshFile(path) => f.debug_tuple("OpenSshFile").field(path).finish(),
        }
    }
}

/// Authentication context passed to server auth handlers.
#[derive(Clone, Debug)]
pub struct AuthContext {
    session_id: SessionId,
    username: Username,
    peer_addr: Option<SocketAddr>,
    server: ServerHandle,
}

impl AuthContext {
    /// Returns the server-side session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the username being authenticated.
    pub fn username(&self) -> &Username {
        &self.username
    }

    /// Returns the peer socket address when available.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }

    /// Returns a server handle.
    pub fn server(&self) -> &ServerHandle {
        &self.server
    }
}

/// Authentication decision returned by server auth handlers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AuthDecision {
    accepted: bool,
}

impl AuthDecision {
    /// Accepts authentication.
    pub fn accept() -> Self {
        Self { accepted: true }
    }

    /// Rejects authentication.
    pub fn reject() -> Self {
        Self { accepted: false }
    }

    /// Returns whether authentication was accepted.
    pub fn is_accepted(self) -> bool {
        self.accepted
    }
}

/// A single prompt item for keyboard-interactive authentication.
#[derive(Clone, Debug)]
pub struct KeyboardInteractivePromptItem {
    /// The prompt text shown to the user.
    pub prompt: String,
    /// Whether the user's input should be echoed back.
    pub echo: bool,
}

impl KeyboardInteractivePromptItem {
    /// Creates a prompt item.
    pub fn new(prompt: impl Into<String>, echo: bool) -> Self {
        Self {
            prompt: prompt.into(),
            echo,
        }
    }
}

/// A set of prompts for keyboard-interactive authentication.
///
/// Each round of keyboard-interactive auth sends one or more prompts
/// to the client.
#[derive(Clone, Debug)]
pub struct KeyboardInteractivePrompt {
    /// A human-readable name for this prompt block (may be empty).
    pub name: String,
    /// Instructions shown to the user (may be empty).
    pub instruction: String,
    /// The individual prompts.
    pub prompts: Vec<KeyboardInteractivePromptItem>,
}

impl KeyboardInteractivePrompt {
    /// Creates a prompt block.
    pub fn new(
        name: impl Into<String>,
        instruction: impl Into<String>,
        prompts: Vec<KeyboardInteractivePromptItem>,
    ) -> Self {
        Self {
            name: name.into(),
            instruction: instruction.into(),
            prompts,
        }
    }
}

/// The outcome of a keyboard-interactive challenge round.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum KeyboardInteractiveResponse {
    /// Accept authentication.
    Accept,
    /// Reject authentication.
    Reject,
    /// Continue with additional prompts.
    FurtherAction(KeyboardInteractivePrompt),
}

/// Context passed to a keyboard-interactive authentication handler.
///
/// The handler receives the submethods requested by the client,
/// the auth session context, and (when present) the user's responses
/// to the immediately preceding prompts.
#[derive(Clone)]
pub struct KeyboardInteractiveContext {
    /// The authentication session context.
    pub session: AuthContext,
    /// Submethods requested by the client (often empty).
    pub submethods: String,
    /// User responses to the previous prompts, if any.
    /// Empty for the initial request.
    pub responses: Vec<Bytes>,
}

impl fmt::Debug for KeyboardInteractiveContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyboardInteractiveContext")
            .field("session", &self.session)
            .field("submethods", &self.submethods)
            .field(
                "responses",
                &format_args!("<redacted {} responses>", self.responses.len()),
            )
            .finish()
    }
}

/// Server exec context passed to command handlers.
#[derive(Clone, Debug)]
pub struct ExecContext {
    session_id: SessionId,
    username: Username,
    peer_addr: Option<SocketAddr>,
    channel_id: u32,
    command: ExecCommand,
    server: ServerHandle,
    env: HashMap<String, String>,
}

impl ExecContext {
    /// Returns the server-side session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the authenticated username.
    pub fn username(&self) -> &Username {
        &self.username
    }

    /// Returns the peer socket address when available.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }

    /// Returns the SSH channel identifier.
    pub fn channel_id(&self) -> u32 {
        self.channel_id
    }

    /// Returns the requested command.
    pub fn command(&self) -> &ExecCommand {
        &self.command
    }

    /// Returns a server handle.
    pub fn server(&self) -> &ServerHandle {
        &self.server
    }

    /// Returns environment variables set by the client before exec.
    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
    }
}

/// Server-side exec command bytes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExecCommand {
    bytes: Bytes,
}

impl ExecCommand {
    /// Creates command bytes.
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    /// Returns command bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the command as UTF-8, when valid.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }
}

/// Buffered response for a server-side exec request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecResponse {
    accepted: bool,
    stdout: Bytes,
    stderr: Bytes,
    exit: CommandExit,
}

impl ExecResponse {
    /// Creates a successful empty response with exit status `0`.
    pub fn success() -> Self {
        Self {
            accepted: true,
            stdout: Bytes::new(),
            stderr: Bytes::new(),
            exit: CommandExit::status(0),
        }
    }

    /// Rejects the exec request.
    pub fn reject() -> Self {
        Self {
            accepted: false,
            stdout: Bytes::new(),
            stderr: Bytes::new(),
            exit: CommandExit::Missing,
        }
    }

    /// Sets stdout bytes.
    pub fn stdout(mut self, stdout: impl AsRef<[u8]>) -> Self {
        self.stdout = Bytes::copy_from_slice(stdout.as_ref());
        self
    }

    /// Sets stderr bytes.
    pub fn stderr(mut self, stderr: impl AsRef<[u8]>) -> Self {
        self.stderr = Bytes::copy_from_slice(stderr.as_ref());
        self
    }

    /// Sets process exit information.
    pub fn exit(mut self, exit: CommandExit) -> Self {
        self.exit = exit;
        self
    }

    /// Sets an exit status.
    pub fn exit_status(self, status: u32) -> Self {
        self.exit(CommandExit::status(status))
    }

    /// Returns whether the exec request is accepted.
    pub fn is_accepted(&self) -> bool {
        self.accepted
    }

    /// Returns stdout bytes.
    pub fn stdout_bytes(&self) -> &Bytes {
        &self.stdout
    }

    /// Returns stderr bytes.
    pub fn stderr_bytes(&self) -> &Bytes {
        &self.stderr
    }

    /// Returns process exit information.
    pub fn exit_info(&self) -> &CommandExit {
        &self.exit
    }
}

/// Commands sent from a streaming exec handler to the session loop.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum StreamingExecCmd {
    /// Data for stdout.
    Stdout(Bytes),
    /// Data for stderr (SSH extended data type 1).
    Stderr(Bytes),
    /// Exit status was set.
    Exited(CommandExit),
}

/// Context passed to a streaming exec handler.
///
/// The handler owns this value, reading stdin and writing stdout/stderr
/// progressively through async methods. When the handler returns, the
/// channel is closed.
pub struct StreamingExecContext {
    session_id: SessionId,
    username: Username,
    peer_addr: Option<SocketAddr>,
    channel_id: u32,
    command: ExecCommand,
    server: ServerHandle,
    cmd_tx: mpsc::UnboundedSender<StreamingExecCmd>,
    stdin_rx: mpsc::UnboundedReceiver<Bytes>,
    env: HashMap<String, String>,
}

impl StreamingExecContext {
    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the authenticated username.
    pub fn username(&self) -> &Username {
        &self.username
    }

    /// Returns the client peer address, if known.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }

    /// Returns the SSH channel identifier.
    pub fn channel_id(&self) -> u32 {
        self.channel_id
    }

    /// Returns the exec command.
    pub fn command(&self) -> &ExecCommand {
        &self.command
    }

    /// Returns a handle that can check server shutdown status.
    pub fn server(&self) -> &ServerHandle {
        &self.server
    }

    /// Returns environment variables set by the client before exec.
    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Reads the next stdin chunk from the client.
    ///
    /// Returns `None` when the stdin stream is exhausted (client close).
    pub async fn read_stdin(&mut self) -> Option<Bytes> {
        self.stdin_rx.recv().await
    }

    /// Sends data to stdout.
    pub async fn stdout(&mut self, data: impl Into<Bytes>) -> Result<()> {
        self.cmd_tx
            .send(StreamingExecCmd::Stdout(data.into()))
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "exec channel closed"))
    }

    /// Sends data to stderr.
    pub async fn stderr(&mut self, data: impl Into<Bytes>) -> Result<()> {
        self.cmd_tx
            .send(StreamingExecCmd::Stderr(data.into()))
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "exec channel closed"))
    }

    /// Sends an exit status and signals the channel to close.
    ///
    /// After calling this method, further stdout/stderr writes will fail.
    pub async fn exit_status(&mut self, status: u32) -> Result<()> {
        self.cmd_tx
            .send(StreamingExecCmd::Exited(CommandExit::Status(status)))
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "exec channel closed"))
    }

    /// Sends an exit signal and signals the channel to close.
    ///
    /// After calling this method, further stdout/stderr writes will fail.
    ///
    /// The `core_dumped` flag is always `false` for handler-generated signals.
    /// If you need to report a core-dump signal, use
    /// `StreamingExecCmd::Exited(CommandExit::Signal(name, true))` through
    /// the internal command channel directly.
    pub async fn exit_signal(&mut self, signal: String) -> Result<()> {
        self.cmd_tx
            .send(StreamingExecCmd::Exited(CommandExit::Signal(signal, false)))
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "exec channel closed"))
    }
}

impl fmt::Debug for StreamingExecContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingExecContext")
            .field("session_id", &self.session_id)
            .field("username", &self.username.as_str())
            .field("peer_addr", &self.peer_addr)
            .field("channel_id", &self.channel_id)
            .field("command", &self.command)
            .finish()
    }
}

/// Context for shell requests.
#[derive(Clone, Debug)]
pub struct ShellContext {
    /// Session metadata.
    pub session: SessionContext,
    /// Channel identifier.
    pub channel: ChannelId,
}

/// Pseudo-terminal parameters from a PTY request.
#[derive(Clone, Debug)]
pub struct PtyParams {
    /// Terminal type.
    pub term: String,
    /// Column width.
    pub col_width: u32,
    /// Row height.
    pub row_height: u32,
    /// Pixel width.
    pub pix_width: u32,
    /// Pixel height.
    pub pix_height: u32,
    /// Terminal modes as (opcode, value) pairs.
    pub modes: Vec<(u8, u32)>,
}

/// Context for PTY requests.
#[derive(Clone, Debug)]
pub struct PtyContext {
    /// Session metadata.
    pub session: SessionContext,
    /// Channel identifier.
    pub channel: ChannelId,
}

/// Context for subsystem requests.
#[derive(Clone, Debug)]
pub struct SubsystemContext {
    /// Session metadata.
    pub session: SessionContext,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Subsystem name.
    pub name: String,
}

/// An environment variable set request.
#[derive(Clone, Debug)]
pub struct EnvRequest {
    /// Variable name.
    pub name: String,
    /// Variable value.
    pub value: String,
}

/// Terminal window resize notification.
#[derive(Clone, Debug)]
pub struct WindowChange {
    /// New column count.
    pub col_width: u32,
    /// New row count.
    pub row_height: u32,
    /// New pixel width.
    pub pix_width: u32,
    /// New pixel height.
    pub pix_height: u32,
}

/// Context for a TCP/IP forwarding global request.
#[derive(Clone, Debug)]
pub struct TcpipForwardContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Requested bind address.
    pub address: String,
    /// Requested bind port.
    pub port: u32,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for a streamlocal forwarding global request.
#[derive(Clone, Debug)]
pub struct StreamLocalForwardContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Requested bind socket path.
    pub socket_path: String,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for a direct TCP/IP channel open request.
#[derive(Clone, Debug)]
pub struct DirectTcpipContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Remote host to connect to.
    pub host_to_connect: String,
    /// Remote port to connect to.
    pub port_to_connect: u32,
    /// Originator address.
    pub originator_address: String,
    /// Originator port.
    pub originator_port: u32,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for a forwarded TCP/IP channel open request.
#[derive(Clone, Debug)]
pub struct ForwardedTcpipContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Address the connection was forwarded to.
    pub connected_address: String,
    /// Port the connection was forwarded to.
    pub connected_port: u32,
    /// Originator address.
    pub originator_address: String,
    /// Originator port.
    pub originator_port: u32,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for a direct streamlocal (Unix domain socket) channel open request.
#[derive(Clone, Debug)]
pub struct DirectStreamLocalContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Remote socket path to connect to.
    pub socket_path: String,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for an X11 forwarding channel request.
#[derive(Clone)]
pub struct X11RequestContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Whether the server should accept a single X11 connection.
    pub single_connection: bool,
    /// The X11 authentication protocol.
    pub auth_protocol: String,
    /// The X11 authentication cookie.
    pub auth_cookie: String,
    /// The X11 screen number.
    pub screen_number: u32,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

impl fmt::Debug for X11RequestContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11RequestContext")
            .field("session_id", &self.session_id)
            .field("username", &self.username)
            .field("channel", &self.channel)
            .field("single_connection", &self.single_connection)
            .field("auth_protocol", &self.auth_protocol)
            .field("auth_cookie", &"<redacted>")
            .field("screen_number", &self.screen_number)
            .field("server", &self.server)
            .finish()
    }
}

/// Context for an X11 channel open request.
#[derive(Clone, Debug)]
pub struct X11ChannelContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Authenticated username.
    pub username: Username,
    /// Originator address.
    pub originator_address: String,
    /// Originator port.
    pub originator_port: u32,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Context for an agent forwarding request.
#[derive(Clone, Debug)]
pub struct AgentRequestContext {
    /// Session identifier.
    pub session_id: SessionId,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Server handle for lifecycle operations.
    pub server: ServerHandle,
}

/// Stateful high-level server handler.
pub trait ServerHandler: Clone + Send + Sync + 'static {
    /// Called when a client TCP connection is accepted.
    fn on_connect(&self, _ctx: SessionContext) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Called when a client successfully authenticates.
    fn on_auth_success(&self, _ctx: SessionContext) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Called when a client session ends.
    fn on_disconnect(&self, _id: SessionId) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Handles password authentication.
    fn auth_password(
        &self,
        _ctx: AuthContext,
        _password: Password,
    ) -> impl Future<Output = Result<AuthDecision>> + Send {
        async { Ok(AuthDecision::reject()) }
    }

    /// Handles public key authentication.
    fn auth_publickey(
        &self,
        _ctx: AuthContext,
        _public_key: PublicKey,
    ) -> impl Future<Output = Result<AuthDecision>> + Send {
        async { Ok(AuthDecision::reject()) }
    }

    /// Handles keyboard-interactive authentication.
    ///
    /// Called once per challenge round. The initial request has
    /// `responses` empty. Return [`KeyboardInteractiveResponse::FurtherAction`]
    /// with prompts for the next round, or [`KeyboardInteractiveResponse::Accept`]
    /// / [`KeyboardInteractiveResponse::Reject`] to finalize.
    fn auth_keyboard_interactive(
        &self,
        _ctx: KeyboardInteractiveContext,
    ) -> impl Future<Output = Result<KeyboardInteractiveResponse>> + Send {
        async { Ok(KeyboardInteractiveResponse::Reject) }
    }

    /// Handles an exec request.
    fn exec(&self, _ctx: ExecContext) -> impl Future<Output = Result<ExecResponse>> + Send {
        async { Ok(ExecResponse::reject()) }
    }

    /// Handles a streaming exec request.
    ///
    /// Streaming handlers own the channel and can read stdin, write stdout/stderr,
    /// and signal exit status progressively through [`StreamingExecContext`].
    fn streaming_exec(
        &self,
        _ctx: StreamingExecContext,
    ) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Handles a shell request.
    fn shell(&self, _ctx: ShellContext) -> impl Future<Output = Result<()>> + Send {
        async {
            Err(Error::channel_kind(
                ChannelErrorKind::Request,
                "shell not supported",
            ))
        }
    }

    /// Handles a PTY request.
    fn pty(&self, _ctx: PtyContext, _params: PtyParams) -> impl Future<Output = Result<()>> + Send {
        async {
            Err(Error::channel_kind(
                ChannelErrorKind::Request,
                "PTY not supported",
            ))
        }
    }

    /// Handles a subsystem request.
    fn subsystem(&self, _ctx: SubsystemContext) -> impl Future<Output = Result<()>> + Send {
        async {
            Err(Error::channel_kind(
                ChannelErrorKind::Request,
                "subsystem not supported",
            ))
        }
    }

    /// Handles an environment-variable request.
    fn env(
        &self,
        _ctx: ShellContext,
        _request: EnvRequest,
    ) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Handles a terminal-window-change request.
    fn window_change(
        &self,
        _ctx: ShellContext,
        _change: WindowChange,
    ) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Handles a TCP/IP forwarding global request.
    fn tcpip_forward(
        &self,
        _ctx: TcpipForwardContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a cancel TCP/IP forwarding global request.
    fn cancel_tcpip_forward(
        &self,
        _ctx: TcpipForwardContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a direct TCP/IP channel open request.
    fn channel_open_direct_tcpip(
        &self,
        _ctx: DirectTcpipContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a forwarded TCP/IP channel open request.
    fn channel_open_forwarded_tcpip(
        &self,
        _ctx: ForwardedTcpipContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a streamlocal forwarding global request.
    fn streamlocal_forward(
        &self,
        _ctx: StreamLocalForwardContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a cancel streamlocal forwarding global request.
    fn cancel_streamlocal_forward(
        &self,
        _ctx: StreamLocalForwardContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles a direct streamlocal channel open request.
    fn channel_open_direct_streamlocal(
        &self,
        _ctx: DirectStreamLocalContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles OpenSSH certificate authentication.
    fn auth_openssh_certificate(
        &self,
        _ctx: AuthContext,
        _certificate: Certificate,
    ) -> impl Future<Output = Result<AuthDecision>> + Send {
        async { Ok(AuthDecision::reject()) }
    }

    /// Handles an X11 forwarding request on an existing channel.
    fn x11_request(&self, _ctx: X11RequestContext) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles an X11 channel open request.
    fn channel_open_x11(
        &self,
        _ctx: X11ChannelContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Handles an agent forwarding request.
    fn agent_request(
        &self,
        _ctx: AgentRequestContext,
    ) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(false) }
    }

    /// Returns an authentication banner to display to connecting clients.
    ///
    /// Return `None` to send no banner. The banner is sent before
    /// authentication begins and is typically a warning or legal notice.
    fn authentication_banner(&self) -> impl Future<Output = Result<Option<String>>> + Send {
        async { Ok(None) }
    }
}

/// Session metadata passed to server handlers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionContext {
    id: SessionId,
    username: Option<Username>,
}

impl SessionContext {
    /// Creates session context.
    pub fn new(id: SessionId) -> Self {
        Self { id, username: None }
    }

    /// Returns the session identifier.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Returns the authenticated username, when present.
    pub fn username(&self) -> Option<&Username> {
        self.username.as_ref()
    }

    /// Sets the authenticated username.
    pub fn with_username(mut self, username: impl Into<Username>) -> Self {
        self.username = Some(username.into());
        self
    }
}

/// Server event exposed by high-level handlers.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerEvent {
    /// A client connected.
    Connected(SessionContext),
    /// A session authenticated.
    Authenticated(SessionContext),
    /// A session disconnected.
    Disconnected(SessionId),
}

struct ServerRuntime {
    config: ServerConfig,
    host_keys: Vec<PrivateKey>,
    password_auth: Option<PasswordAuthCallback>,
    public_key_auth: Option<PublicKeyAuthCallback>,
    keyboard_interactive_auth: Option<KeyboardInteractiveCallback>,
    exec_routes: HashMap<String, ExecCallback>,
    fallback_exec: Option<ExecCallback>,
    exec_routes_streaming: HashMap<String, StreamingExecCallback>,
    fallback_streaming_exec: Option<StreamingExecCallback>,
    shell_handler: Option<ShellCallback>,
    pty_handler: Option<PtyCallback>,
    subsystem_handler: Option<SubsystemCallback>,
    env_handler: Option<EnvCallback>,
    window_change_handler: Option<WindowChangeCallback>,
    tcpip_forward_handler: Option<TcpipForwardCallback>,
    cancel_tcpip_forward_handler: Option<CancelTcpipForwardCallback>,
    direct_tcpip_handler: Option<DirectTcpipCallback>,
    forwarded_tcpip_handler: Option<ForwardedTcpipCallback>,
    streamlocal_forward_handler: Option<StreamLocalForwardCallback>,
    cancel_streamlocal_forward_handler: Option<CancelStreamLocalForwardCallback>,
    direct_streamlocal_handler: Option<DirectStreamLocalCallback>,
    shutdown_grace: Duration,
    handle: ServerHandle,
    last_error: Mutex<Option<Error>>,
    on_connect: Option<ConnectCallback>,
    on_disconnect: Option<DisconnectCallback>,
    on_auth_success: Option<AuthSuccessCallback>,
    cert_auth: Option<CertAuthCallback>,
    x11_request_handler: Option<X11RequestCallback>,
    x11_channel_handler: Option<X11ChannelCallback>,
    agent_request_handler: Option<AgentRequestCallback>,
    auth_banner: Option<AuthBannerCallback>,
    #[cfg(feature = "sftp")]
    sftp_handler: Option<std::sync::Arc<dyn crate::sftp::SftpServerHandler + Send + Sync>>,
}

impl ServerRuntime {
    fn russh_config(&self) -> server::Config {
        let mut methods = MethodSet::empty();
        if self.password_auth.is_some() {
            methods.push(MethodKind::Password);
        }
        if self.public_key_auth.is_some() {
            methods.push(MethodKind::PublicKey);
        }
        if self.keyboard_interactive_auth.is_some() {
            methods.push(MethodKind::KeyboardInteractive);
        }

        server::Config {
            server_id: russh::SshId::Standard(Cow::Owned(self.config.server_id().to_owned())),
            methods,
            keys: self.host_keys.clone(),
            max_auth_attempts: 10,
            inactivity_timeout: Some(self.shutdown_grace),
            ..Default::default()
        }
    }

    fn record_error(&self, error: Error) {
        if let Ok(mut last_error) = self.last_error.lock()
            && last_error.is_none()
        {
            *last_error = Some(error);
        }
    }

    fn take_error(&self) -> Option<Error> {
        self.last_error
            .lock()
            .ok()
            .and_then(|mut last_error| last_error.take())
    }
}

#[derive(Clone)]
struct HighLevelRusshServer {
    runtime: Arc<ServerRuntime>,
}

impl server::Server for HighLevelRusshServer {
    type Handler = HighLevelRusshHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        let session_id = SessionId::next();
        if let Some(ref cb) = self.runtime.on_connect {
            let cb = Arc::clone(cb);
            let ctx = SessionContext::new(session_id);
            tokio::spawn(async move {
                let _ = cb(ctx).await;
            });
        }
        HighLevelRusshHandler {
            runtime: Arc::clone(&self.runtime),
            session_id,
            peer_addr,
            username: None,
            open_sessions: 0,
            disconnect_notified: Arc::new(Mutex::new(false)),
            stdin_txs: Arc::new(Mutex::new(HashMap::new())),
            env_vars: HashMap::new(),
            #[cfg(feature = "sftp")]
            sftp: None,
        }
    }

    fn handle_session_error(&mut self, error: <Self::Handler as server::Handler>::Error) {
        match error {
            ServerRuntimeError::Russh(russh::Error::Disconnect | russh::Error::HUP) => {}
            ServerRuntimeError::Russh(error) => self.runtime.record_error(map_server_error(error)),
            ServerRuntimeError::HighLevel(error) => self.runtime.record_error(error),
        }
    }
}

struct HighLevelRusshHandler {
    runtime: Arc<ServerRuntime>,
    session_id: SessionId,
    peer_addr: Option<SocketAddr>,
    username: Option<Username>,
    open_sessions: usize,
    disconnect_notified: Arc<Mutex<bool>>,
    stdin_txs: Arc<Mutex<HashMap<ChannelId, mpsc::UnboundedSender<Bytes>>>>,
    env_vars: HashMap<ChannelId, HashMap<String, String>>,
    #[cfg(feature = "sftp")]
    sftp: Option<crate::sftp::server::SftpServerRuntime>,
}

impl Clone for HighLevelRusshHandler {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            session_id: self.session_id,
            peer_addr: self.peer_addr,
            username: self.username.clone(),
            open_sessions: self.open_sessions,
            disconnect_notified: Arc::clone(&self.disconnect_notified),
            stdin_txs: Arc::clone(&self.stdin_txs),
            env_vars: self.env_vars.clone(),
            #[cfg(feature = "sftp")]
            sftp: None,
        }
    }
}

impl HighLevelRusshHandler {
    fn auth_context(&self, username: &str) -> AuthContext {
        AuthContext {
            session_id: self.session_id,
            username: Username::from(username),
            peer_addr: self.peer_addr,
            server: self.runtime.handle.clone(),
        }
    }

    fn session_context(&self) -> SessionContext {
        SessionContext {
            id: self.session_id,
            username: self.username.clone(),
        }
    }

    fn exec_context(&self, channel: ChannelId, command: &[u8]) -> Result<ExecContext> {
        let username = self.username.clone().ok_or_else(|| {
            Error::authentication_kind(
                AuthenticationErrorKind::Unavailable,
                "exec request received before authentication",
            )
        })?;

        let env = self.env_vars.get(&channel).cloned().unwrap_or_default();

        Ok(ExecContext {
            session_id: self.session_id,
            username,
            peer_addr: self.peer_addr,
            channel_id: u32::from(channel),
            command: ExecCommand::new(Bytes::copy_from_slice(command)),
            server: self.runtime.handle.clone(),
            env,
        })
    }

    async fn run_streaming_exec(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut server::Session,
        username: Username,
        handler: StreamingExecCallback,
    ) -> std::result::Result<(), ServerRuntimeError> {
        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<Bytes>();
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<StreamingExecCmd>();

        self.stdin_txs
            .lock()
            .expect("stdin_txs lock not poisoned")
            .insert(channel, stdin_tx);

        let env = self.env_vars.get(&channel).cloned().unwrap_or_default();

        let cmd_tx_for_ctx = cmd_tx.clone();
        let ctx = StreamingExecContext {
            session_id: self.session_id,
            username,
            peer_addr: self.peer_addr,
            channel_id: u32::from(channel),
            command: ExecCommand::new(Bytes::copy_from_slice(command)),
            server: self.runtime.handle.clone(),
            cmd_tx: cmd_tx_for_ctx,
            stdin_rx,
            env,
        };

        let handle = session.handle();
        session.channel_success(channel)?;

        let handler = handler.clone();
        let stdin_txs = Arc::clone(&self.stdin_txs);

        tokio::spawn(async move {
            let handler_task = tokio::spawn(async move {
                let result = handler(ctx).await;
                drop(cmd_tx);
                result
            });

            let mut exit_sent = false;

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    StreamingExecCmd::Stdout(data) => {
                        if !exit_sent {
                            let _ = handle.data(channel, data).await;
                        }
                    }
                    StreamingExecCmd::Stderr(data) => {
                        if !exit_sent {
                            let _ = handle.extended_data(channel, 1, data).await;
                        }
                    }
                    StreamingExecCmd::Exited(exit_info) => {
                        if exit_sent {
                            continue;
                        }
                        exit_sent = true;
                        match exit_info {
                            CommandExit::Status(status) => {
                                let _ = handle.exit_status_request(channel, status).await;
                            }
                            CommandExit::Signal(signal, core_dumped) => {
                                let _ = handle
                                    .exit_signal_request(
                                        channel,
                                        russh::Sig::Custom(signal),
                                        core_dumped,
                                        "".to_owned(),
                                        "".to_owned(),
                                    )
                                    .await;
                            }
                            CommandExit::Missing => {}
                            _ => {}
                        }
                        break;
                    }
                }
            }

            if exit_sent {
                cmd_rx.close();
            }

            let handler_err = !matches!(handler_task.await, Ok(Ok(())));

            if !exit_sent {
                let status = if handler_err { 1 } else { 0 };
                let _ = handle.exit_status_request(channel, status).await;
            }

            if let Ok(mut map) = stdin_txs.lock() {
                map.remove(&channel);
            }
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });

        Ok(())
    }

    fn fire_auth_success(&self) {
        if let Some(ref cb) = self.runtime.on_auth_success {
            let cb = Arc::clone(cb);
            let ctx = self.session_context();
            tokio::spawn(async move {
                let _ = cb(ctx).await;
            });
        }
    }
}

impl Drop for HighLevelRusshHandler {
    fn drop(&mut self) {
        if let Ok(mut notified) = self.disconnect_notified.lock() {
            if *notified {
                return;
            }
            *notified = true;
        } else {
            return;
        }
        if let Some(ref cb) = self.runtime.on_disconnect {
            let cb = Arc::clone(cb);
            let id = self.session_id;
            tokio::spawn(async move {
                let _ = cb(id).await;
            });
        }
    }
}

impl server::Handler for HighLevelRusshHandler {
    type Error = ServerRuntimeError;

    async fn auth_none(&mut self, _user: &str) -> std::result::Result<Auth, Self::Error> {
        Ok(Auth::reject())
    }

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<Auth, Self::Error> {
        let Some(handler) = self.runtime.password_auth.as_ref() else {
            return Ok(Auth::reject());
        };

        let decision = handler(self.auth_context(user), Password::new(password.to_owned()))
            .await
            .map_err(ServerRuntimeError::HighLevel)?;

        if decision.is_accepted() {
            self.username = Some(Username::from(user));
            self.fire_auth_success();
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _public_key: &PublicKey,
    ) -> std::result::Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> std::result::Result<Auth, Self::Error> {
        let Some(handler) = self.runtime.public_key_auth.as_ref() else {
            return Ok(Auth::reject());
        };

        let decision = handler(self.auth_context(user), public_key.clone())
            .await
            .map_err(ServerRuntimeError::HighLevel)?;

        if decision.is_accepted() {
            self.username = Some(Username::from(user));
            self.fire_auth_success();
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_openssh_certificate(
        &mut self,
        user: &str,
        certificate: &Certificate,
    ) -> std::result::Result<Auth, Self::Error> {
        let Some(handler) = self.runtime.cert_auth.as_ref() else {
            return Ok(Auth::reject());
        };

        let decision = handler(self.auth_context(user), certificate.clone())
            .await
            .map_err(ServerRuntimeError::HighLevel)?;

        if decision.is_accepted() {
            self.username = Some(Username::from(user));
            self.fire_auth_success();
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        submethods: &str,
        response: Option<russh::server::Response<'a>>,
    ) -> std::result::Result<Auth, Self::Error> {
        let Some(handler) = self.runtime.keyboard_interactive_auth.as_ref() else {
            return Ok(Auth::reject());
        };

        let responses: Vec<Bytes> = response.into_iter().flatten().collect();

        let ctx = KeyboardInteractiveContext {
            session: self.auth_context(user),
            submethods: submethods.to_owned(),
            responses,
        };

        let decision = handler(ctx).await.map_err(ServerRuntimeError::HighLevel)?;

        match decision {
            KeyboardInteractiveResponse::Accept => {
                self.username = Some(Username::from(user));
                self.fire_auth_success();
                Ok(Auth::Accept)
            }
            KeyboardInteractiveResponse::Reject => Ok(Auth::reject()),
            KeyboardInteractiveResponse::FurtherAction(prompt) => Ok(Auth::Partial {
                name: Cow::Owned(prompt.name),
                instructions: Cow::Owned(prompt.instruction),
                prompts: Cow::Owned(
                    prompt
                        .prompts
                        .into_iter()
                        .map(|p| (Cow::Owned(p.prompt), p.echo))
                        .collect(),
                ),
            }),
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        if self.username.is_none() || self.open_sessions >= self.runtime.config.max_sessions() {
            return Ok(false);
        }

        self.open_sessions += 1;
        Ok(true)
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        self.open_sessions = self.open_sessions.saturating_sub(1);
        self.stdin_txs
            .lock()
            .expect("stdin_txs lock not poisoned")
            .remove(&channel);
        self.env_vars.remove(&channel);
        #[cfg(feature = "sftp")]
        if let Some(sftp) = &mut self.sftp {
            sftp.handle_channel_close(channel);
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        self.stdin_txs
            .lock()
            .expect("stdin_txs lock not poisoned")
            .remove(&channel);
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        #[cfg(feature = "sftp")]
        if let Some(sftp) = &mut self.sftp
            && sftp.is_sftp_channel(channel)
        {
            sftp.handle_data(channel, data, session)
                .await
                .map_err(ServerRuntimeError::HighLevel)?;
            return Ok(());
        }

        if let Some(tx) = self
            .stdin_txs
            .lock()
            .expect("stdin_txs lock not poisoned")
            .get(&channel)
        {
            let _ = tx.send(Bytes::copy_from_slice(data));
        }
        #[cfg(not(feature = "sftp"))]
        let _ = session;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        let username = self.username.clone().ok_or_else(|| {
            ServerRuntimeError::HighLevel(Error::authentication_kind(
                AuthenticationErrorKind::Unavailable,
                "exec request received before authentication",
            ))
        })?;

        let command_str = std::str::from_utf8(data).ok().map(|s| s.to_owned());

        // Try streaming route (exact match first)
        let streaming_handler = {
            let runtime = &self.runtime;
            command_str
                .as_ref()
                .and_then(|cmd| runtime.exec_routes_streaming.get(cmd))
                .or(runtime.fallback_streaming_exec.as_ref())
                .cloned()
        };

        if let Some(handler) = streaming_handler {
            self.run_streaming_exec(channel, data, session, username, handler)
                .await?;
            return Ok(());
        }

        // Try buffered route
        let ctx = self
            .exec_context(channel, data)
            .map_err(ServerRuntimeError::HighLevel)?;

        let buffered_handler = ctx
            .command()
            .as_str()
            .and_then(|command| self.runtime.exec_routes.get(command))
            .or(self.runtime.fallback_exec.as_ref());

        let Some(buffered_handler) = buffered_handler else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        let response = match buffered_handler(ctx).await {
            Ok(response) => response,
            Err(error) => {
                session.channel_failure(channel)?;
                return Err(ServerRuntimeError::HighLevel(error));
            }
        };

        if !response.is_accepted() {
            session.channel_failure(channel)?;
            return Ok(());
        }

        send_exec_response(session, channel, response)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        let ctx = ShellContext {
            session: self.session_context(),
            channel,
        };

        let Some(handler) = self.runtime.shell_handler.as_ref() else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        match handler(ctx).await {
            Ok(()) => session.channel_success(channel)?,
            Err(error) => {
                tracing::debug!(?error, "shell handler rejected shell request");
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        let ctx = PtyContext {
            session: self.session_context(),
            channel,
        };
        let params = PtyParams {
            term: term.to_owned(),
            col_width,
            row_height,
            pix_width,
            pix_height,
            modes: modes.iter().map(|(pty, val)| (*pty as u8, *val)).collect(),
        };

        let Some(handler) = self.runtime.pty_handler.as_ref() else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        match handler(ctx, params).await {
            Ok(()) => session.channel_success(channel)?,
            Err(error) => {
                tracing::debug!(?error, "pty handler rejected PTY request");
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        let ctx = ShellContext {
            session: self.session_context(),
            channel,
        };
        let request = EnvRequest {
            name: variable_name.to_owned(),
            value: variable_value.to_owned(),
        };

        let Some(handler) = self.runtime.env_handler.as_ref() else {
            self.env_vars
                .entry(channel)
                .or_default()
                .insert(variable_name.to_owned(), variable_value.to_owned());
            session.channel_success(channel)?;
            return Ok(());
        };

        match handler(ctx, request).await {
            Ok(()) => {
                self.env_vars
                    .entry(channel)
                    .or_default()
                    .insert(variable_name.to_owned(), variable_value.to_owned());
                session.channel_success(channel)?;
            }
            Err(error) => {
                tracing::debug!(?error, "env handler rejected env request");
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        #[cfg(feature = "sftp")]
        if name == "sftp"
            && let Some(handler) = self.runtime.sftp_handler.clone()
        {
            let mut sftp = crate::sftp::server::SftpServerRuntime::new(handler);
            sftp.register_channel(channel);
            self.sftp = Some(sftp);
            session.channel_success(channel)?;
            return Ok(());
        }

        let ctx = SubsystemContext {
            session: self.session_context(),
            channel,
            name: name.to_owned(),
        };

        let Some(handler) = self.runtime.subsystem_handler.as_ref() else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        match handler(ctx).await {
            Ok(()) => session.channel_success(channel)?,
            Err(error) => {
                tracing::debug!(?error, "subsystem handler rejected subsystem request");
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), ServerRuntimeError> {
        let ctx = ShellContext {
            session: self.session_context(),
            channel,
        };
        let change = WindowChange {
            col_width,
            row_height,
            pix_width,
            pix_height,
        };

        if let Some(handler) = self.runtime.window_change_handler.as_ref() {
            let _ = handler(ctx, change).await;
        }
        // Window changes are always accepted.
        session.channel_success(channel)?;
        Ok(())
    }

    async fn agent_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.agent_request_handler.as_ref() else {
            session.channel_failure(channel)?;
            return Ok(false);
        };

        let ctx = AgentRequestContext {
            session_id: self.session_id,
            channel,
            server: self.runtime.handle.clone(),
        };

        match handler(ctx).await {
            Ok(true) => {
                session.channel_success(channel)?;
                Ok(true)
            }
            Ok(false) => {
                session.channel_failure(channel)?;
                Ok(false)
            }
            Err(error) => {
                session.channel_failure(channel)?;
                Err(ServerRuntimeError::HighLevel(error))
            }
        }
    }

    async fn x11_request(
        &mut self,
        channel: ChannelId,
        single_connection: bool,
        x11_auth_protocol: &str,
        x11_auth_cookie: &str,
        x11_screen_number: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        let Some(handler) = self.runtime.x11_request_handler.as_ref() else {
            session.channel_failure(channel)?;
            return Ok(());
        };

        let username = match &self.username {
            Some(u) => u.clone(),
            None => {
                session.channel_failure(channel)?;
                return Ok(());
            }
        };

        let ctx = X11RequestContext {
            session_id: self.session_id,
            username,
            channel,
            single_connection,
            auth_protocol: x11_auth_protocol.to_owned(),
            auth_cookie: x11_auth_cookie.to_owned(),
            screen_number: x11_screen_number,
            server: self.runtime.handle.clone(),
        };

        match handler(ctx).await {
            Ok(true) => session.channel_success(channel)?,
            Ok(false) => session.channel_failure(channel)?,
            Err(error) => {
                session.channel_failure(channel)?;
                return Err(ServerRuntimeError::HighLevel(error));
            }
        }

        Ok(())
    }

    async fn channel_open_x11(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        originator_address: &str,
        originator_port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.x11_channel_handler.as_ref() else {
            let _ = channel.close().await;
            return Ok(false);
        };

        let username = match &self.username {
            Some(u) => u.clone(),
            None => {
                let _ = channel.close().await;
                return Ok(false);
            }
        };

        let ctx = X11ChannelContext {
            session_id: self.session_id,
            username,
            originator_address: originator_address.to_owned(),
            originator_port,
            server: self.runtime.handle.clone(),
        };

        match handler(ctx).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = channel.close().await;
                Ok(false)
            }
            Err(e) => {
                let _ = channel.close().await;
                Err(ServerRuntimeError::HighLevel(e))
            }
        }
    }

    async fn authentication_banner(&mut self) -> std::result::Result<Option<String>, Self::Error> {
        let Some(handler) = self.runtime.auth_banner.as_ref() else {
            return Ok(None);
        };

        handler().await.map_err(ServerRuntimeError::HighLevel)
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.tcpip_forward_handler.as_ref() else {
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => return Ok(false),
        };
        let ctx = TcpipForwardContext {
            session_id: self.session_id,
            username,
            address: address.to_owned(),
            port: *port,
            server: self.runtime.handle.clone(),
        };
        handler(ctx).await.map_err(ServerRuntimeError::HighLevel)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.cancel_tcpip_forward_handler.as_ref() else {
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => return Ok(false),
        };
        let ctx = TcpipForwardContext {
            session_id: self.session_id,
            username,
            address: address.to_owned(),
            port,
            server: self.runtime.handle.clone(),
        };
        handler(ctx).await.map_err(ServerRuntimeError::HighLevel)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        originator_address: &str,
        originator_port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.direct_tcpip_handler.as_ref() else {
            let _ = channel.close().await;
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => {
                let _ = channel.close().await;
                return Ok(false);
            }
        };
        let ctx = DirectTcpipContext {
            session_id: self.session_id,
            username,
            host_to_connect: host_to_connect.to_owned(),
            port_to_connect,
            originator_address: originator_address.to_owned(),
            originator_port,
            server: self.runtime.handle.clone(),
        };
        match handler(ctx).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = channel.close().await;
                Ok(false)
            }
            Err(e) => {
                let _ = channel.close().await;
                Err(ServerRuntimeError::HighLevel(e))
            }
        }
    }

    async fn channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        connected_address: &str,
        connected_port: u32,
        originator_address: &str,
        originator_port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.forwarded_tcpip_handler.as_ref() else {
            let _ = channel.close().await;
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => {
                let _ = channel.close().await;
                return Ok(false);
            }
        };
        let ctx = ForwardedTcpipContext {
            session_id: self.session_id,
            username,
            connected_address: connected_address.to_owned(),
            connected_port,
            originator_address: originator_address.to_owned(),
            originator_port,
            server: self.runtime.handle.clone(),
        };
        match handler(ctx).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = channel.close().await;
                Ok(false)
            }
            Err(e) => {
                let _ = channel.close().await;
                Err(ServerRuntimeError::HighLevel(e))
            }
        }
    }

    async fn streamlocal_forward(
        &mut self,
        socket_path: &str,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.streamlocal_forward_handler.as_ref() else {
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => return Ok(false),
        };
        let ctx = StreamLocalForwardContext {
            session_id: self.session_id,
            username,
            socket_path: socket_path.to_owned(),
            server: self.runtime.handle.clone(),
        };
        handler(ctx).await.map_err(ServerRuntimeError::HighLevel)
    }

    async fn cancel_streamlocal_forward(
        &mut self,
        socket_path: &str,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.cancel_streamlocal_forward_handler.as_ref() else {
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => return Ok(false),
        };
        let ctx = StreamLocalForwardContext {
            session_id: self.session_id,
            username,
            socket_path: socket_path.to_owned(),
            server: self.runtime.handle.clone(),
        };
        handler(ctx).await.map_err(ServerRuntimeError::HighLevel)
    }

    async fn channel_open_direct_streamlocal(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        socket_path: &str,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        let Some(handler) = self.runtime.direct_streamlocal_handler.as_ref() else {
            let _ = channel.close().await;
            return Ok(false);
        };
        let username = match &self.username {
            Some(u) => u.clone(),
            None => {
                let _ = channel.close().await;
                return Ok(false);
            }
        };
        let ctx = DirectStreamLocalContext {
            session_id: self.session_id,
            username,
            socket_path: socket_path.to_owned(),
            server: self.runtime.handle.clone(),
        };
        match handler(ctx).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = channel.close().await;
                Ok(false)
            }
            Err(e) => {
                let _ = channel.close().await;
                Err(ServerRuntimeError::HighLevel(e))
            }
        }
    }
}

#[derive(Debug)]
enum ServerRuntimeError {
    Russh(russh::Error),
    HighLevel(Error),
}

impl From<russh::Error> for ServerRuntimeError {
    fn from(error: russh::Error) -> Self {
        Self::Russh(error)
    }
}

fn send_exec_response(
    session: &mut server::Session,
    channel: ChannelId,
    response: ExecResponse,
) -> std::result::Result<(), russh::Error> {
    session.channel_success(channel)?;

    if !response.stdout.is_empty() {
        session.data(channel, response.stdout)?;
    }

    if !response.stderr.is_empty() {
        session.extended_data(channel, 1, response.stderr)?;
    }

    match response.exit {
        CommandExit::Status(status) => session.exit_status_request(channel, status)?,
        CommandExit::Signal(signal, core_dumped) => {
            session.exit_signal_request(
                channel,
                russh::Sig::Custom(signal),
                core_dumped,
                "",
                "",
            )?;
        }
        CommandExit::Missing => {}
        _ => {}
    }

    session.eof(channel)?;
    session.close(channel)?;
    Ok(())
}

fn map_server_error(error: russh::Error) -> Error {
    match error {
        russh::Error::NotAuthenticated => Error::authentication_kind(
            AuthenticationErrorKind::Unavailable,
            "server request was received before authentication",
        ),
        russh::Error::UnsupportedAuthMethod => Error::authentication_kind(
            AuthenticationErrorKind::UnsupportedMethod,
            "server received an unsupported authentication method",
        ),
        russh::Error::Disconnect | russh::Error::HUP => {
            Error::disconnected(Operation::Server, "client disconnected from server")
        }
        russh::Error::IO(source) => Error::transport_with_source(
            TransportErrorKind::Io,
            "server transport I/O failed",
            source,
        ),
        russh::Error::KexInit
        | russh::Error::Kex
        | russh::Error::NoCommonAlgo { .. }
        | russh::Error::PacketAuth
        | russh::Error::Version
        | russh::Error::StrictKeyExchangeViolation { .. } => Error::transport_with_source(
            TransportErrorKind::Negotiation,
            "server SSH negotiation failed",
            error,
        ),
        russh::Error::WrongChannel | russh::Error::Inconsistent => Error::channel_with_source(
            ChannelErrorKind::Protocol,
            "server channel entered an invalid state",
            error,
        ),
        russh::Error::SendError => Error::channel_with_source(
            ChannelErrorKind::Write,
            "server failed to send channel data",
            error,
        ),
        error => Error::ssh_with_source("russh server session failed", error),
    }
}

fn validate_host_key_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path)?;
        let mode = metadata.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(Error::invalid_config(format!(
                "host key file `{}` must not be accessible by group or others",
                path.display()
            )));
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AuthDecision, ExecCommand, ExecResponse, Server, ServerHostKey};
    use bytes::Bytes;
    use russh::keys::{Algorithm, PrivateKey};
    use russh_extra_core::{CommandExit, Error};
    use tokio::sync::watch;

    fn test_host_key() -> ServerHostKey {
        let private_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("test host key should be generated");
        ServerHostKey::from_private_key(private_key)
    }

    #[test]
    fn server_builder_requires_host_key() {
        let error = Server::builder().build().unwrap_err();

        assert!(matches!(error, Error::InvalidConfig(_)));
    }

    #[test]
    fn server_builder_accepts_host_key() {
        let server = Server::builder()
            .host_key(test_host_key())
            .max_sessions(4)
            .build()
            .unwrap();

        assert_eq!(server.config().max_sessions(), 4);
    }

    #[test]
    fn host_key_debug_redacts_private_key() {
        let host_key = test_host_key();

        let debug = format!("{host_key:?}");

        assert!(debug.contains("***"));
    }

    #[test]
    fn auth_decision_defaults_to_reject() {
        assert!(!AuthDecision::default().is_accepted());
        assert!(AuthDecision::accept().is_accepted());
        assert!(!AuthDecision::reject().is_accepted());
    }

    #[test]
    fn exec_command_exposes_utf8_when_valid() {
        let command = ExecCommand::new(Bytes::from_static(b"whoami"));

        assert_eq!(command.as_str(), Some("whoami"));
        assert_eq!(command.as_bytes(), b"whoami");

        let command = ExecCommand::new(Bytes::from_static(b"\xff"));
        assert_eq!(command.as_str(), None);
    }

    #[test]
    fn exec_response_builders_set_output_and_exit() {
        let response = ExecResponse::success()
            .stdout("out\n")
            .stderr("err\n")
            .exit_status(42);

        assert!(response.is_accepted());
        assert_eq!(response.stdout_bytes().as_ref(), b"out\n");
        assert_eq!(response.stderr_bytes().as_ref(), b"err\n");
        assert_eq!(response.exit_info(), &CommandExit::status(42));
        assert!(!ExecResponse::reject().is_accepted());
    }

    #[test]
    fn keyboard_interactive_context_debug_redacts_responses() {
        let (tx, _rx) = watch::channel(None);
        let server_handle = super::ServerHandle { shutdown_tx: tx };
        let auth = super::AuthContext {
            session_id: russh_extra_core::SessionId::next(),
            username: russh_extra_core::Username::from("testuser"),
            peer_addr: None,
            server: server_handle,
        };
        let ctx = super::KeyboardInteractiveContext {
            session: auth,
            submethods: String::new(),
            responses: vec![Bytes::from_static(b"secret123")],
        };
        let debug = format!("{:?}", ctx);
        assert!(!debug.contains("secret"), "responses leaked: {debug}");
        assert!(debug.contains("<redacted"));
    }

    #[test]
    fn x11_request_context_debug_redacts_cookie() {
        let (tx, _rx) = watch::channel(None);
        let server_handle = super::ServerHandle { shutdown_tx: tx };
        let channel: russh::ChannelId = {
            #[allow(unsafe_code)]
            // SAFETY: ChannelId is a repr(transparent) newtype over u32.
            // Transmuting u32 -> ChannelId is sound in test code.
            unsafe {
                std::mem::transmute::<u32, russh::ChannelId>(5)
            }
        };
        let ctx = super::X11RequestContext {
            session_id: russh_extra_core::SessionId::next(),
            username: russh_extra_core::Username::from("testuser"),
            channel,
            single_connection: false,
            auth_protocol: "MIT-MAGIC-COOKIE-1".into(),
            auth_cookie: "deadbeef1234".into(),
            screen_number: 0,
            server: server_handle,
        };
        let debug = format!("{:?}", ctx);
        assert!(
            !debug.contains(&ctx.auth_cookie),
            "auth cookie leaked: {debug}"
        );
        assert!(debug.contains("<redacted>"));
    }
}
