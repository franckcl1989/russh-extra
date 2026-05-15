//! Port forwarding and tunnel APIs.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use russh::ChannelMsg;
use russh_extra_core::{
    ChannelErrorKind, Error, ForwardDirection, ForwardSpec, ForwardingErrorKind, Result, SessionId,
    StreamLocalSpec, TcpEndpoint, Timeouts,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::client::ClientHandler;

/// Shared registry of remote TCP forwarding targets.
///
/// Keyed by remote bind port. Written by [`Tunnel::start`] for remote
/// forwarding specs and read by [`ClientHandler::server_channel_open_forwarded_tcpip`].
pub(crate) type RemoteForwardMap = Arc<Mutex<HashMap<u16, TcpEndpoint>>>;

/// Shared registry of remote streamlocal forwarding targets.
///
/// Keyed by remote bind socket path. Written by streamlocal forwarding
/// and read by [`ClientHandler::server_channel_open_forwarded_streamlocal`].
pub(crate) type RemoteStreamLocalForwardMap = Arc<Mutex<HashMap<String, PathBuf>>>;

// ── TunnelBuilder ─────────────────────────────────────────────────────

/// Builder for a port forwarding tunnel.
///
/// Created by [`Session::tunnel`](super::Session::tunnel).
/// Call [`start`](TunnelBuilder::start) to begin forwarding.
#[derive(Clone)]
pub struct TunnelBuilder {
    session_id: SessionId,
    handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
    remote_forwards: Option<RemoteForwardMap>,
    remote_streamlocal_forwards: Option<RemoteStreamLocalForwardMap>,
    spec: ForwardSpec,
    timeouts: Timeouts,
}

impl fmt::Debug for TunnelBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelBuilder")
            .field("session_id", &self.session_id)
            .field("spec", &self.spec)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl TunnelBuilder {
    /// Creates a tunnel builder from session state.
    pub(crate) fn from_session(
        session_id: SessionId,
        handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
        remote_forwards: RemoteForwardMap,
        remote_streamlocal_forwards: RemoteStreamLocalForwardMap,
        spec: ForwardSpec,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            remote_forwards: Some(remote_forwards),
            remote_streamlocal_forwards: Some(remote_streamlocal_forwards),
            spec,
            timeouts,
        }
    }

    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the forwarding specification.
    pub fn spec(&self) -> &ForwardSpec {
        &self.spec
    }

    /// Starts the tunnel.
    ///
    /// For local forwarding, binds a local listener and forwards accepted
    /// connections through `direct-tcpip` (TCP) or `direct-streamlocal`
    /// (Unix domain socket) channels.
    ///
    /// For remote forwarding, sends a global request to the server and
    /// handles incoming forwarded channels.
    pub async fn start(self) -> Result<Tunnel> {
        let handle = self
            .handle
            .ok_or_else(|| Error::unsupported("port forwarding requires a connected session"))?;

        let remote_forwards = self
            .remote_forwards
            .ok_or_else(|| Error::unsupported("port forwarding requires a connected session"))?;

        let remote_streamlocal_forwards = self
            .remote_streamlocal_forwards
            .ok_or_else(|| Error::unsupported("port forwarding requires a connected session"))?;

        match &self.spec {
            ForwardSpec::Tcp {
                direction,
                bind,
                target,
            } => match direction {
                ForwardDirection::Local => {
                    start_local_forward(handle, bind, target, self.timeouts).await
                }
                ForwardDirection::Remote => {
                    start_remote_forward(handle, remote_forwards, bind, target, self.timeouts).await
                }
                _ => Err(Error::unsupported("unsupported forwarding direction")),
            },
            ForwardSpec::StreamLocal {
                direction,
                bind,
                target,
            } => match direction {
                ForwardDirection::Local => {
                    #[cfg(unix)]
                    {
                        start_local_streamlocal_forward(handle, bind, target, self.timeouts).await
                    }
                    #[cfg(not(unix))]
                    {
                        Err(Error::unsupported(
                            "local streamlocal forwarding is not supported on this platform",
                        ))
                    }
                }
                ForwardDirection::Remote => {
                    start_remote_streamlocal_forward(
                        handle,
                        remote_streamlocal_forwards,
                        bind,
                        target,
                        self.timeouts,
                    )
                    .await
                }
                _ => Err(Error::unsupported("unsupported forwarding direction")),
            },
            _ => Err(Error::unsupported("unsupported forwarding specification")),
        }
    }
}

// ── Tunnel bind point ─────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum TunnelBindPoint {
    Tcp(SocketAddr),
    StreamLocal(PathBuf),
}

// ── Tunnel ────────────────────────────────────────────────────────────

/// Active forwarding tunnel.
///
/// Controls the lifecycle of a local or remote port/streamlocal forwarding
/// session.  Created by [`TunnelBuilder::start`].
pub struct Tunnel {
    session_id: SessionId,
    spec: ForwardSpec,
    bound: TunnelBindPoint,
    close_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tunnel")
            .field("session_id", &self.session_id)
            .field("spec", &self.spec)
            .field("bound", &self.bound)
            .finish()
    }
}

impl Tunnel {
    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the forwarding specification.
    pub fn spec(&self) -> &ForwardSpec {
        &self.spec
    }

    /// Returns the bound address.
    ///
    /// For local TCP forwarding, this is the local listening address.
    /// For remote TCP forwarding, this is the remote bind address as
    /// reported by the server.
    ///
    /// Returns `None` for streamlocal tunnels; use [`bound_path`](Tunnel::bound_path).
    pub fn bound_addr(&self) -> Option<SocketAddr> {
        match &self.bound {
            TunnelBindPoint::Tcp(addr) => Some(*addr),
            TunnelBindPoint::StreamLocal(_) => None,
        }
    }

    /// Returns the bound socket path for streamlocal tunnels.
    ///
    /// Returns `None` for TCP tunnels.
    pub fn bound_path(&self) -> Option<&Path> {
        match &self.bound {
            TunnelBindPoint::Tcp(_) => None,
            TunnelBindPoint::StreamLocal(path) => Some(path.as_path()),
        }
    }

    /// Gracefully closes the tunnel.
    ///
    /// Sends a shutdown signal to the accept loop and waits for it to finish.
    /// For remote forwarding, also sends a cancel request.
    pub async fn close(mut self) -> Result<()> {
        if let Some(tx) = self.close_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        Ok(())
    }

    /// Forcefully aborts the tunnel.
    ///
    /// Sends a shutdown signal but does not wait for the accept loop to finish.
    pub fn abort(mut self) {
        if let Some(tx) = self.close_tx.take() {
            let _ = tx.send(());
        }
        self.task = None;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(tx) = self.close_tx.take() {
            let _ = tx.send(());
        }
    }
}

// ── Local TCP forwarding ──────────────────────────────────────────────

async fn start_local_forward(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    bind: &TcpEndpoint,
    target: &TcpEndpoint,
    _timeouts: Timeouts,
) -> Result<Tunnel> {
    let bind_addr = format!("{}:{}", bind.host(), bind.port());
    let listener = TcpListener::bind(&bind_addr).await.map_err(|_e| {
        Error::forwarding(
            ForwardingErrorKind::Bind,
            format!("failed to bind local listener at {}", bind_addr),
        )
    })?;

    let bound = listener.local_addr().map_err(|_e| {
        Error::forwarding(
            ForwardingErrorKind::Listen,
            "failed to get local listener address",
        )
    })?;

    let (close_tx, close_rx) = oneshot::channel::<()>();
    let spawn_target = target.clone();
    let tunnel_target = target.clone();

    let task = tokio::spawn(async move {
        run_local_accept_loop(listener, handle, spawn_target, close_rx).await;
    });

    Ok(Tunnel {
        session_id: SessionId::next(),
        spec: ForwardSpec::Tcp {
            direction: ForwardDirection::Local,
            bind: bind.clone(),
            target: tunnel_target,
        },
        bound: TunnelBindPoint::Tcp(bound),
        close_tx: Some(close_tx),
        task: Some(task),
    })
}

async fn run_local_accept_loop(
    listener: TcpListener,
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    target: TcpEndpoint,
    mut close_rx: oneshot::Receiver<()>,
) {
    tracing::debug!(
        local_addr = %listener.local_addr().map(|a| a.to_string()).unwrap_or_default(),
        target = %format!("{}:{}", target.host(), target.port()),
        "local forwarding listener started",
    );

    loop {
        let accept_result = tokio::select! {
            _ = &mut close_rx => {
                tracing::debug!("local forwarding listener shutting down");
                break;
            }
            result = listener.accept() => result,
        };

        match accept_result {
            Ok((tcp_stream, peer_addr)) => {
                let handle = handle.clone();
                let target = target.clone();
                tracing::debug!(peer = %peer_addr, "accepted local forwarding connection");
                tokio::spawn(async move {
                    forward_direct_tcpip_connection(handle, &target, tcp_stream).await;
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "local forwarding accept error");
            }
        }
    }

    tracing::debug!("local forwarding listener stopped");
}

async fn forward_direct_tcpip_connection(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    target: &TcpEndpoint,
    tcp_stream: TcpStream,
) {
    let channel = {
        let guard = handle.lock().await;
        match guard
            .channel_open_direct_tcpip(target.host(), target.port() as u32, "127.0.0.1", 0u32)
            .await
        {
            Ok(ch) => ch,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    target = %format!("{}:{}", target.host(), target.port()),
                    "failed to open direct-tcpip channel",
                );
                return;
            }
        }
    };

    let target_addr = format!("{}:{}", target.host(), target.port());
    tracing::debug!(target = %target_addr, "direct-tcpip channel opened");
    copy_bidirectional(channel, tcp_stream).await;
}

// ── Remote TCP forwarding ─────────────────────────────────────────────

async fn start_remote_forward(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    remote_forwards: RemoteForwardMap,
    bind: &TcpEndpoint,
    target: &TcpEndpoint,
    _timeouts: Timeouts,
) -> Result<Tunnel> {
    let remote_port = bind.port() as u32;
    let remote_host = bind.host().to_string();

    {
        let mut fwds = remote_forwards.lock().await;
        if let Some(existing) = fwds.get(&bind.port()) {
            return Err(Error::forwarding(
                ForwardingErrorKind::Bind,
                format!(
                    "remote port {} is already registered for forwarding to {}:{}",
                    bind.port(),
                    existing.host(),
                    existing.port()
                ),
            ));
        }
        fwds.insert(bind.port(), target.clone());
    }

    let allocated_port = {
        let guard = handle.lock().await;
        guard
            .tcpip_forward(remote_host.as_str(), remote_port)
            .await
            .map_err(|e| {
                let fwds = remote_forwards.clone();
                let port = bind.port();
                tokio::spawn(async move {
                    fwds.lock().await.remove(&port);
                });

                match &e {
                    russh::Error::RequestDenied => Error::forwarding(
                        ForwardingErrorKind::GlobalRequest,
                        format!(
                            "remote tcpip-forward request denied for {}:{}",
                            remote_host, remote_port
                        ),
                    ),
                    _ => Error::forwarding(
                        ForwardingErrorKind::GlobalRequest,
                        format!(
                            "failed to request remote tcpip-forward for {}:{}",
                            remote_host, remote_port
                        ),
                    ),
                }
            })?
    };

    let bound = format!("{}:{}", remote_host, allocated_port)
        .parse()
        .unwrap_or_else(|_| {
            format!("0.0.0.0:{allocated_port}")
                .parse()
                .expect("0.0.0.0:{port} must parse as SocketAddr for a valid u16 port")
        });

    let (close_tx, close_rx) = oneshot::channel::<()>();
    let cancel_host = remote_host.clone();
    let cancel_handle = handle.clone();
    let cancel_fwds = remote_forwards.clone();
    let cancel_port = allocated_port;

    let task = tokio::spawn(async move {
        let _ = close_rx.await;

        tracing::debug!(
            remote_host = %cancel_host,
            allocated_port = cancel_port,
            "cancelling remote tcpip-forward",
        );

        {
            let mut fwds = cancel_fwds.lock().await;
            fwds.remove(&(cancel_port as u16));
        }

        let guard = cancel_handle.lock().await;
        if let Err(e) = guard
            .cancel_tcpip_forward(cancel_host.as_str(), cancel_port)
            .await
        {
            tracing::warn!(
                error = %e,
                remote_host = %cancel_host,
                allocated_port = cancel_port,
                "failed to cancel remote tcpip-forward",
            );
        }
    });

    Ok(Tunnel {
        session_id: SessionId::next(),
        spec: ForwardSpec::Tcp {
            direction: ForwardDirection::Remote,
            bind: TcpEndpoint::new(remote_host, allocated_port as u16),
            target: target.clone(),
        },
        bound: TunnelBindPoint::Tcp(bound),
        close_tx: Some(close_tx),
        task: Some(task),
    })
}

// ── Local streamlocal forwarding ──────────────────────────────────────

#[cfg(unix)]
async fn start_local_streamlocal_forward(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    bind: &StreamLocalSpec,
    target: &StreamLocalSpec,
    _timeouts: Timeouts,
) -> Result<Tunnel> {
    let bind_path = bind.path().to_path_buf();
    let listener = UnixListener::bind(&bind_path).map_err(|e| {
        Error::forwarding(
            ForwardingErrorKind::Bind,
            format!(
                "failed to bind local Unix listener at {}: {e}",
                bind_path.display()
            ),
        )
    })?;

    let bound_path = listener
        .local_addr()
        .map(|a| {
            a.as_pathname()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| bind_path.clone())
        })
        .unwrap_or_else(|_| bind_path.clone());

    let (close_tx, close_rx) = oneshot::channel::<()>();
    let spawn_target = target.clone();

    let task = tokio::spawn(async move {
        run_local_streamlocal_accept_loop(listener, bind_path, handle, spawn_target, close_rx)
            .await;
    });

    Ok(Tunnel {
        session_id: SessionId::next(),
        spec: ForwardSpec::StreamLocal {
            direction: ForwardDirection::Local,
            bind: bind.clone(),
            target: target.clone(),
        },
        bound: TunnelBindPoint::StreamLocal(bound_path),
        close_tx: Some(close_tx),
        task: Some(task),
    })
}

#[cfg(unix)]
async fn run_local_streamlocal_accept_loop(
    listener: UnixListener,
    bind_path: PathBuf,
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    target: StreamLocalSpec,
    mut close_rx: oneshot::Receiver<()>,
) {
    tracing::debug!(
        path = %target.path().display(),
        "local streamlocal forwarding listener started",
    );

    loop {
        let accept_result = tokio::select! {
            _ = &mut close_rx => {
                tracing::debug!("local streamlocal forwarding listener shutting down");
                break;
            }
            result = listener.accept() => result,
        };

        match accept_result {
            Ok((unix_stream, _peer_addr)) => {
                let handle = handle.clone();
                let target_path = target.path().to_path_buf();
                tracing::debug!(path = %target_path.display(), "accepted local streamlocal connection");
                tokio::spawn(async move {
                    forward_direct_streamlocal_connection(handle, &target_path, unix_stream).await;
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "local streamlocal forwarding accept error");
            }
        }
    }

    drop(listener);
    if let Err(e) = std::fs::remove_file(&bind_path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %bind_path.display(),
            error = %e,
            "failed to remove streamlocal socket after listener shutdown"
        );
    }

    tracing::debug!("local streamlocal forwarding listener stopped");
}

#[cfg(unix)]
async fn forward_direct_streamlocal_connection(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    target_path: &Path,
    unix_stream: UnixStream,
) {
    let channel = {
        let guard = handle.lock().await;
        match guard
            .channel_open_direct_streamlocal(target_path.to_string_lossy().as_ref())
            .await
        {
            Ok(ch) => ch,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    target = %target_path.display(),
                    "failed to open direct-streamlocal channel",
                );
                return;
            }
        }
    };

    tracing::debug!(
        target = %target_path.display(),
        "direct-streamlocal channel opened",
    );
    copy_bidirectional_unix(channel, unix_stream).await;
}

// ── Remote streamlocal forwarding ─────────────────────────────────────

async fn start_remote_streamlocal_forward(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    remote_forwards: RemoteStreamLocalForwardMap,
    bind: &StreamLocalSpec,
    target: &StreamLocalSpec,
    _timeouts: Timeouts,
) -> Result<Tunnel> {
    let socket_path = bind.path().to_string_lossy().to_string();

    {
        let mut fwds = remote_forwards.lock().await;
        if fwds.contains_key(&socket_path) {
            return Err(Error::forwarding(
                ForwardingErrorKind::Bind,
                format!(
                    "remote streamlocal path {} is already registered for forwarding",
                    socket_path,
                ),
            ));
        }
        fwds.insert(socket_path.clone(), target.path().to_path_buf());
    }

    {
        let guard = handle.lock().await;
        guard
            .streamlocal_forward(socket_path.as_str())
            .await
            .map_err(|e| {
                let fwds = remote_forwards.clone();
                let path = socket_path.clone();
                tokio::spawn(async move {
                    fwds.lock().await.remove(&path);
                });

                match &e {
                    russh::Error::RequestDenied => Error::forwarding(
                        ForwardingErrorKind::GlobalRequest,
                        format!("remote streamlocal-forward request denied for {socket_path}"),
                    ),
                    _ => Error::forwarding(
                        ForwardingErrorKind::GlobalRequest,
                        format!("failed to request remote streamlocal-forward for {socket_path}"),
                    ),
                }
            })?;
    }

    let bound_path = socket_path.clone();
    let (close_tx, close_rx) = oneshot::channel::<()>();
    let cancel_path = socket_path.clone();
    let cancel_handle = handle.clone();
    let cancel_fwds = remote_forwards.clone();

    let task = tokio::spawn(async move {
        let _ = close_rx.await;

        tracing::debug!(
            path = %cancel_path,
            "cancelling remote streamlocal-forward",
        );

        {
            let mut fwds = cancel_fwds.lock().await;
            fwds.remove(&cancel_path);
        }

        let guard = cancel_handle.lock().await;
        if let Err(e) = guard.cancel_streamlocal_forward(cancel_path.as_str()).await {
            tracing::warn!(
                error = %e,
                path = %cancel_path,
                "failed to cancel remote streamlocal-forward",
            );
        }
    });

    Ok(Tunnel {
        session_id: SessionId::next(),
        spec: ForwardSpec::StreamLocal {
            direction: ForwardDirection::Remote,
            bind: bind.clone(),
            target: target.clone(),
        },
        bound: TunnelBindPoint::StreamLocal(PathBuf::from(bound_path)),
        close_tx: Some(close_tx),
        task: Some(task),
    })
}

// ── Direct TCP ────────────────────────────────────────────────────────

/// Builder for a single direct TCP channel.
///
/// Created by [`Session::direct_tcp`](super::Session::direct_tcp).
/// Call [`open`](DirectTcpBuilder::open) to establish the channel.
#[derive(Clone)]
pub struct DirectTcpBuilder {
    session_id: SessionId,
    handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
    target: TcpEndpoint,
    timeouts: Timeouts,
}

impl fmt::Debug for DirectTcpBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectTcpBuilder")
            .field("session_id", &self.session_id)
            .field("target", &self.target)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl DirectTcpBuilder {
    pub(crate) fn from_session(
        session_id: SessionId,
        handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
        target: TcpEndpoint,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            target,
            timeouts,
        }
    }

    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the target endpoint.
    pub fn target(&self) -> &TcpEndpoint {
        &self.target
    }

    /// Opens the direct TCP channel.
    ///
    /// Returns a [`TunnelStream`] for bidirectional I/O with the remote
    /// endpoint through the SSH tunnel.
    pub async fn open(self) -> Result<TunnelStream> {
        let handle = self
            .handle
            .ok_or_else(|| Error::unsupported("direct TCP requires a connected session"))?;

        let guard = handle.lock().await;
        let channel = guard
            .channel_open_direct_tcpip(
                self.target.host(),
                self.target.port() as u32,
                "127.0.0.1",
                0u32,
            )
            .await
            .map_err(|_e| {
                Error::forwarding(
                    ForwardingErrorKind::ChannelOpen,
                    format!(
                        "failed to open direct-tcpip channel to {}:{}",
                        self.target.host(),
                        self.target.port()
                    ),
                )
            })?;

        let (read_tx, read_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let _task = tokio::spawn(async move {
            run_tunnel_stream_bridge(channel, read_tx, cmd_rx).await;
        });

        Ok(TunnelStream {
            read_rx,
            cmd_tx,
            read_buf: Vec::new(),
            read_pos: 0,
            closed: false,
            _task,
        })
    }
}

// ── Direct StreamLocal ────────────────────────────────────────────────

/// Builder for a single direct streamlocal (Unix domain socket) channel.
///
/// Created by [`Session::direct_streamlocal`](super::Session::direct_streamlocal).
/// Call [`open`](DirectStreamLocalBuilder::open) to establish the channel.
#[derive(Clone)]
pub struct DirectStreamLocalBuilder {
    session_id: SessionId,
    handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
    socket_path: PathBuf,
    #[allow(dead_code)]
    timeouts: Timeouts,
}

impl fmt::Debug for DirectStreamLocalBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectStreamLocalBuilder")
            .field("session_id", &self.session_id)
            .field("socket_path", &self.socket_path)
            .finish()
    }
}

impl DirectStreamLocalBuilder {
    pub(crate) fn from_session(
        session_id: SessionId,
        handle: Option<Arc<Mutex<russh::client::Handle<ClientHandler>>>>,
        socket_path: PathBuf,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            socket_path,
            timeouts,
        }
    }

    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the target socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Opens the direct streamlocal channel.
    ///
    /// Returns a [`TunnelStream`] for bidirectional I/O with the remote
    /// Unix domain socket through the SSH tunnel.
    pub async fn open(self) -> Result<TunnelStream> {
        let handle = self
            .handle
            .ok_or_else(|| Error::unsupported("direct streamlocal requires a connected session"))?;

        let guard = handle.lock().await;
        let channel = guard
            .channel_open_direct_streamlocal(self.socket_path.to_string_lossy().as_ref())
            .await
            .map_err(|_e| {
                Error::forwarding(
                    ForwardingErrorKind::ChannelOpen,
                    format!(
                        "failed to open direct-streamlocal channel to {}",
                        self.socket_path.display()
                    ),
                )
            })?;

        let (read_tx, read_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let _task = tokio::spawn(async move {
            run_tunnel_stream_bridge(channel, read_tx, cmd_rx).await;
        });

        Ok(TunnelStream {
            read_rx,
            cmd_tx,
            read_buf: Vec::new(),
            read_pos: 0,
            closed: false,
            _task,
        })
    }
}

// ── TunnelStream ──────────────────────────────────────────────────────

// ── Tunnel stream bridge task infrastructure ──────────────────────────

/// Internal command sent to the tunnel bridge task.
enum TunnelCmd {
    /// Write data to the channel.
    Write(Vec<u8>),
    /// Send EOF on the channel.
    Eof,
    /// Close the channel.
    Close,
}

/// Background task that bridges the SSH channel with mpsc channels
/// for [`TunnelStream`].
///
/// Reads [`ChannelMsg`] from the channel and forwards data to `read_tx`.
/// Receives commands from `cmd_rx` and issues them on the channel.
async fn run_tunnel_stream_bridge(
    mut channel: russh::Channel<russh::client::Msg>,
    read_tx: mpsc::UnboundedSender<Vec<u8>>,
    mut cmd_rx: mpsc::UnboundedReceiver<TunnelCmd>,
) {
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data })
                        if read_tx.send(data.to_vec()).is_err() =>
                    {
                        break;
                    }
                    Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    _ => {}
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(TunnelCmd::Write(data)) => {
                        let _ = channel.data(data.as_slice()).await;
                    }
                    Some(TunnelCmd::Eof) => {
                        let _ = channel.eof().await;
                    }
                    Some(TunnelCmd::Close) | None => {
                        let _ = channel.close().await;
                        break;
                    }
                }
            }
        }
    }
}

// ── TunnelStream ──────────────────────────────────────────────────────

/// Streaming I/O over a forwarded SSH channel.
///
/// Provides [`AsyncRead`] and [`AsyncWrite`] for bidirectional data
/// transfer through a direct-tcpip, forwarded-tcpip, direct-streamlocal,
/// or forwarded-streamlocal channel.
///
/// All `tokio::io::AsyncReadExt` methods (e.g. `read_exact`,
/// `read_to_end`, `read_buf`) are available on `TunnelStream`.
pub struct TunnelStream {
    read_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    cmd_tx: mpsc::UnboundedSender<TunnelCmd>,
    read_buf: Vec<u8>,
    read_pos: usize,
    closed: bool,
    _task: JoinHandle<()>,
}

impl fmt::Debug for TunnelStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelStream")
            .field("closed", &self.closed)
            .finish()
    }
}

impl TunnelStream {
    /// Writes bytes to the channel.
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        self.cmd_tx
            .send(TunnelCmd::Write(data.to_vec()))
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "tunnel channel closed"))?;
        Ok(data.len())
    }

    /// Writes all bytes to the channel.
    pub async fn write_all(&self, data: &[u8]) -> Result<()> {
        self.write(data).await?;
        Ok(())
    }

    /// Sends EOF on the channel.
    pub async fn send_eof(&self) -> Result<()> {
        self.cmd_tx
            .send(TunnelCmd::Eof)
            .map_err(|_| Error::channel_kind(ChannelErrorKind::Close, "tunnel channel closed"))?;
        Ok(())
    }

    /// Closes the channel.
    pub async fn close(self) -> Result<()> {
        let _ = self.cmd_tx.send(TunnelCmd::Close);
        Ok(())
    }
}

impl AsyncRead for TunnelStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Serve buffered data first.
        if self.read_pos < self.read_buf.len() {
            let remaining = &self.read_buf[self.read_pos..];
            let n = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..n]);
            self.read_pos += n;
            return Poll::Ready(Ok(()));
        }

        // If the bridge task has stopped sending (channel closed), signal EOF.
        if self.closed {
            return Poll::Ready(Ok(()));
        }

        // Poll the mpsc receiver for the next data chunk from the bridge task.
        match self.read_rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                let n = data.len().min(buf.remaining());
                buf.put_slice(&data[..n]);
                if n < data.len() {
                    self.read_buf = data;
                    self.read_pos = n;
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                // Bridge task dropped the sender — channel is closed.
                self.closed = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.cmd_tx
            .send(TunnelCmd::Write(buf.to_vec()))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tunnel channel closed"))?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _ = self.cmd_tx.send(TunnelCmd::Eof);
        Poll::Ready(Ok(()))
    }
}

// ── Bidirectional copy helpers ────────────────────────────────────────

/// Copies data bidirectionally between a `russh::Channel` and a `TcpStream`.
pub(crate) async fn copy_bidirectional(
    channel: russh::Channel<russh::client::Msg>,
    tcp: TcpStream,
) {
    let (mut channel_read, channel_write) = channel.split();
    let (mut tcp_read, mut tcp_write) = tcp.into_split();

    let c2t = tokio::spawn(async move {
        let mut reader = channel_read.make_reader();
        match tokio::io::copy(&mut reader, &mut tcp_write).await {
            Ok(n) => {
                tracing::debug!(bytes = n, "channel→tcp copy finished");
            }
            Err(e) => {
                tracing::debug!(error = %e, "channel→tcp copy ended");
            }
        }
        let _ = tcp_write.shutdown().await;
    });

    let t2c = async {
        match channel_write.data(&mut tcp_read).await {
            Ok(()) => {
                tracing::debug!("tcp→channel copy finished");
            }
            Err(e) => {
                tracing::debug!(error = %e, "tcp→channel copy ended");
            }
        }
        let _ = channel_write.eof().await;
    };

    let _ = tokio::join!(c2t, t2c);
}

/// Copies data bidirectionally between a `russh::Channel` and a `UnixStream`.
#[cfg(unix)]
pub(crate) async fn copy_bidirectional_unix(
    channel: russh::Channel<russh::client::Msg>,
    unix: UnixStream,
) {
    let (mut channel_read, channel_write) = channel.split();
    let (mut unix_read, mut unix_write) = unix.into_split();

    let c2u = tokio::spawn(async move {
        let mut reader = channel_read.make_reader();
        match tokio::io::copy(&mut reader, &mut unix_write).await {
            Ok(n) => {
                tracing::debug!(bytes = n, "channel→unix copy finished");
            }
            Err(e) => {
                tracing::debug!(error = %e, "channel→unix copy ended");
            }
        }
    });

    let u2c = async {
        match channel_write.data(&mut unix_read).await {
            Ok(()) => {
                tracing::debug!("unix→channel copy finished");
            }
            Err(e) => {
                tracing::debug!(error = %e, "unix→channel copy ended");
            }
        }
        let _ = channel_write.eof().await;
    };

    let _ = tokio::join!(c2u, u2c);
}

/// Copies data bidirectionally between a `russh::Channel` and a TCP target.
pub(crate) async fn copy_bidirectional_with_addr(
    channel: russh::Channel<russh::client::Msg>,
    addr: &str,
) {
    match TcpStream::connect(addr).await {
        Ok(tcp) => {
            copy_bidirectional(channel, tcp).await;
        }
        Err(e) => {
            tracing::warn!(
                target = %addr,
                error = %e,
                "failed to connect to forwarding target",
            );
            let _ = channel.close().await;
        }
    }
}

/// Copies data bidirectionally between a `russh::Channel` and a Unix domain socket target.
#[cfg(unix)]
pub(crate) async fn copy_bidirectional_with_unix_path(
    channel: russh::Channel<russh::client::Msg>,
    path: &Path,
) {
    match UnixStream::connect(path).await {
        Ok(unix) => {
            copy_bidirectional_unix(channel, unix).await;
        }
        Err(e) => {
            tracing::warn!(
                target = %path.display(),
                error = %e,
                "failed to connect to streamlocal forwarding target",
            );
            let _ = channel.close().await;
        }
    }
}
