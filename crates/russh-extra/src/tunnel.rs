//! Port forwarding and tunnel APIs.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use russh::ChannelMsg;
use russh_extra_core::{
    Error, ForwardDirection, ForwardSpec, ForwardingErrorKind, Result, SessionId, TcpEndpoint,
    Timeouts,
};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use super::client::ClientHandler;

/// Shared registry of remote forwarding targets.
///
/// Keyed by remote bind port. Written by [`Tunnel::start`] for remote
/// forwarding specs and read by [`ClientHandler::server_channel_open_forwarded_tcpip`].
pub(crate) type RemoteForwardMap = Arc<Mutex<HashMap<u16, TcpEndpoint>>>;

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
        spec: ForwardSpec,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            remote_forwards: Some(remote_forwards),
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
    /// For local forwarding, binds a local TCP listener and forwards
    /// accepted connections through `direct-tcpip` channels.
    ///
    /// For remote forwarding, sends a `tcpip-forward` global request to the
    /// server and handles incoming `forwarded-tcpip` channels.
    pub async fn start(self) -> Result<Tunnel> {
        let handle = self
            .handle
            .ok_or_else(|| Error::unsupported("port forwarding requires a connected session"))?;

        let remote_forwards = self
            .remote_forwards
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
            },
            ForwardSpec::StreamLocal { .. } => Err(Error::unsupported(
                "streamlocal forwarding is not implemented",
            )),
        }
    }
}

// ── Tunnel ────────────────────────────────────────────────────────────

/// Active forwarding tunnel.
///
/// Controls the lifecycle of a local or remote port forwarding session.
/// Created by [`TunnelBuilder::start`].
pub struct Tunnel {
    session_id: SessionId,
    spec: ForwardSpec,
    bound: SocketAddr,
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
    /// For local forwarding, this is the local listening address.
    /// For remote forwarding, this is the remote bind address as
    /// reported by the server.
    pub fn bound_addr(&self) -> SocketAddr {
        self.bound
    }

    /// Gracefully closes the tunnel.
    ///
    /// Sends a shutdown signal to the accept loop and waits for it to finish.
    /// For remote forwarding, also sends a `cancel-tcpip-forward` request.
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
        // The accept loop will clean up on shutdown signal.
        // We don't wait for it — that would block the drop.
    }
}

// ── Local forwarding ──────────────────────────────────────────────────

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
        bound,
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

// ── Remote forwarding ─────────────────────────────────────────────────

async fn start_remote_forward(
    handle: Arc<Mutex<russh::client::Handle<ClientHandler>>>,
    remote_forwards: RemoteForwardMap,
    bind: &TcpEndpoint,
    target: &TcpEndpoint,
    _timeouts: Timeouts,
) -> Result<Tunnel> {
    let remote_port = bind.port() as u32;
    let remote_host = bind.host().to_string();

    // Register the forwarding target before sending the global request.
    // If the registration fails (shouldn't), the request would go to the
    // server without a local handler.
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
                // Clean up the registration on failure.
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
        .unwrap_or_else(|_| format!("0.0.0.0:{}", allocated_port).parse().unwrap());

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
        bound,
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

        Ok(TunnelStream {
            channel,
            read_buf: Vec::new(),
            read_pos: 0,
            closed: false,
        })
    }
}

// ── TunnelStream ──────────────────────────────────────────────────────

/// Streaming I/O over a forwarded SSH channel.
///
/// Provides `read` and `write` methods for bidirectional data transfer
/// through a direct-tcpip or forwarded-tcpip channel.
pub struct TunnelStream {
    channel: russh::Channel<russh::client::Msg>,
    read_buf: Vec<u8>,
    read_pos: usize,
    closed: bool,
}

impl fmt::Debug for TunnelStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelStream")
            .field("closed", &self.closed)
            .finish()
    }
}

impl TunnelStream {
    /// Reads bytes from the channel.
    ///
    /// Returns `Ok(0)` when the channel is closed and all buffered
    /// data has been consumed.
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if self.read_pos < self.read_buf.len() {
            let remaining = &self.read_buf[self.read_pos..];
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.read_pos += n;
            return Ok(n);
        }

        if self.closed {
            return Ok(0);
        }

        loop {
            match self.channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    let n = data.len().min(buf.len());
                    buf[..n].copy_from_slice(&data[..n]);
                    if n < data.len() {
                        self.read_buf = data.to_vec();
                        self.read_pos = n;
                    }
                    return Ok(n);
                }
                Some(ChannelMsg::Close) | None => {
                    self.closed = true;
                    return Ok(0);
                }
                _ => {}
            }
        }
    }

    /// Writes bytes to the channel.
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        self.channel.data(data).await.map_err(map_tunnel_error)?;
        Ok(data.len())
    }

    /// Writes all bytes to the channel.
    pub async fn write_all(&self, data: &[u8]) -> Result<()> {
        self.write(data).await?;
        Ok(())
    }

    /// Sends EOF on the channel.
    pub async fn send_eof(&self) -> Result<()> {
        self.channel.eof().await.map_err(map_tunnel_error)?;
        Ok(())
    }

    /// Closes the channel.
    pub async fn close(self) -> Result<()> {
        self.channel.close().await.map_err(map_tunnel_error)?;
        Ok(())
    }

    /// Returns the underlying `russh` channel.
    ///
    /// **Expert**: use this to send custom requests through the channel.
    pub fn russh_channel(&self) -> &russh::Channel<russh::client::Msg> {
        &self.channel
    }

    /// Returns the underlying `russh` channel (mutable access).
    pub fn russh_channel_mut(&mut self) -> &mut russh::Channel<russh::client::Msg> {
        &mut self.channel
    }
}

// ── Bidirectional copy helper ─────────────────────────────────────────

/// Copies data bidirectionally between a `russh::Channel` and a `TcpStream`.
///
/// This is used internally for both local and remote forwarding.
/// The function completes when both directions have finished (typically
/// when one side closes or an error occurs).
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

/// Copies data bidirectionally between a `russh::Channel` and a TCP target.
///
/// Used by the client handler for forwarded-tcpip channels (remote
/// forwarding) where we need to connect to a local address first.
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

fn map_tunnel_error(e: russh::Error) -> Error {
    match e {
        russh::Error::RequestDenied => {
            Error::forwarding(ForwardingErrorKind::GlobalRequest, "SSH request denied")
        }
        e => Error::ssh_with_source("tunnel I/O error", e),
    }
}
