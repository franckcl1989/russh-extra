//! Interactive shell, PTY, and subsystem abstractions.
//!
//! Shell channels provide streaming async I/O with separate stdin, stdout,
//! and stderr, plus resize, signal, and exit-status observation.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use russh::ChannelMsg;
use russh::client;
use russh_extra_core::{
    ChannelErrorKind, CommandExit, Error, Operation, Pty, Result, SessionId, Timeouts,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{Mutex, mpsc};
use tokio::time;

/// Converts `russh-extra-core::TerminalMode` to a `russh::Pty` opcode.
fn terminal_mode_to_pty(mode: russh_extra_core::TerminalMode) -> Option<(russh::Pty, u32)> {
    match mode {
        russh_extra_core::TerminalMode::Interrupt => Some((russh::Pty::VINTR, 0)),
        russh_extra_core::TerminalMode::Quit => Some((russh::Pty::VQUIT, 0)),
        russh_extra_core::TerminalMode::Erase => Some((russh::Pty::VERASE, 0)),
        russh_extra_core::TerminalMode::Kill => Some((russh::Pty::VKILL, 0)),
        russh_extra_core::TerminalMode::EndOfFile => Some((russh::Pty::VEOF, 0)),
        russh_extra_core::TerminalMode::InputSpeed => Some((russh::Pty::TTY_OP_ISPEED, 0)),
        russh_extra_core::TerminalMode::OutputSpeed => Some((russh::Pty::TTY_OP_OSPEED, 0)),
        russh_extra_core::TerminalMode::Echo => Some((russh::Pty::ECHO, 0)),
        russh_extra_core::TerminalMode::EchoErase => Some((russh::Pty::ECHOE, 0)),
        russh_extra_core::TerminalMode::EchoKill => Some((russh::Pty::ECHOK, 0)),
        russh_extra_core::TerminalMode::EchoNl => Some((russh::Pty::ECHONL, 0)),
        russh_extra_core::TerminalMode::CanonicalInput => Some((russh::Pty::ICANON, 0)),
        russh_extra_core::TerminalMode::SigCheck => Some((russh::Pty::ISIG, 0)),
        russh_extra_core::TerminalMode::CrToNlInput => Some((russh::Pty::ICRNL, 0)),
        russh_extra_core::TerminalMode::NlToCrInput => Some((russh::Pty::INLCR, 0)),
        russh_extra_core::TerminalMode::IgnoreCrInput => Some((russh::Pty::IGNCR, 0)),
        russh_extra_core::TerminalMode::PostProcessOutput => Some((russh::Pty::OPOST, 0)),
        russh_extra_core::TerminalMode::NlToCrNlOutput => Some((russh::Pty::ONLCR, 0)),
        russh_extra_core::TerminalMode::CrToNlOutput => Some((russh::Pty::OCRNL, 0)),
        russh_extra_core::TerminalMode::NoCrOnNl => Some((russh::Pty::ONLRET, 0)),
        russh_extra_core::TerminalMode::Custom(opcode) => {
            russh::Pty::from_u8(opcode).map(|pty| (pty, 0))
        }
        _ => None,
    }
}

/// Builds terminal mode tuples from a PTY configuration for `russh`.
fn build_terminal_modes(pty: &Pty) -> Vec<(russh::Pty, u32)> {
    pty.modes()
        .iter()
        .filter_map(|(mode, value)| {
            terminal_mode_to_pty(*mode).map(|(pty_opcode, _)| (pty_opcode, *value))
        })
        .collect()
}

/// X11 forwarding parameters.
#[derive(Clone)]
pub struct X11Params {
    /// Whether the server should accept a single X11 connection.
    pub single_connection: bool,
    /// The X11 authentication protocol (e.g. "MIT-MAGIC-COOKIE-1").
    pub protocol: String,
    /// The X11 authentication cookie (128-bit hex string).
    pub cookie: String,
    /// The X11 screen number.
    pub screen: u32,
}

impl X11Params {
    /// Creates X11 parameters with an auto-generated MIT-MAGIC-COOKIE-1 cookie.
    pub fn new(screen: u32) -> Self {
        use rand::Rng;
        let mut cookie_bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut cookie_bytes);
        let cookie = cookie_bytes
            .iter()
            .fold(String::with_capacity(32), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            });

        Self {
            single_connection: false,
            protocol: "MIT-MAGIC-COOKIE-1".into(),
            cookie,
            screen,
        }
    }

    /// Creates X11 parameters with a user-supplied protocol and cookie.
    pub fn with_cookie(
        protocol: impl Into<String>,
        cookie: impl Into<String>,
        screen: u32,
    ) -> Self {
        Self {
            single_connection: false,
            protocol: protocol.into(),
            cookie: cookie.into(),
            screen,
        }
    }
}

impl fmt::Debug for X11Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11Params")
            .field("single_connection", &self.single_connection)
            .field("protocol", &self.protocol)
            .field("cookie", &"<redacted>")
            .field("screen", &self.screen)
            .finish()
    }
}

/// Interactive shell handle.
///
/// Created by [`Shell::open`]. Provides streaming I/O, resize, signal,
/// and exit-status observation.
pub struct ShellHandle {
    channel: russh::Channel<russh::client::Msg>,
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
    exit: CommandExit,
    closed: bool,
}

impl fmt::Debug for ShellHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellHandle")
            .field("channel_id", &self.channel.id())
            .field("exit", &self.exit)
            .field("closed", &self.closed)
            .finish()
    }
}

impl ShellHandle {
    /// Reads bytes from the shell channel.
    ///
    /// Returns bytes from buffered stdout/stderr or blocks until new data
    /// arrives. Returns `Ok(0)` when the channel is closed and all buffered
    /// data has been consumed.
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if !self.stdout_buf.is_empty() {
            let n = self.stdout_buf.len().min(buf.len());
            buf[..n].copy_from_slice(&self.stdout_buf[..n]);
            self.stdout_buf.drain(..n);
            return Ok(n);
        }

        if !self.stderr_buf.is_empty() {
            let n = self.stderr_buf.len().min(buf.len());
            buf[..n].copy_from_slice(&self.stderr_buf[..n]);
            self.stderr_buf.drain(..n);
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
                        self.stdout_buf.extend_from_slice(&data[n..]);
                    }
                    return Ok(n);
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    let n = data.len().min(buf.len());
                    buf[..n].copy_from_slice(&data[..n]);
                    if n < data.len() {
                        self.stderr_buf.extend_from_slice(&data[n..]);
                    }
                    return Ok(n);
                }
                Some(ChannelMsg::ExitStatus {
                    exit_status: status,
                }) => {
                    self.exit = CommandExit::Status(status);
                }
                Some(ChannelMsg::ExitSignal {
                    signal_name,
                    core_dumped,
                    ..
                }) => {
                    self.exit = CommandExit::Signal(
                        crate::client::signal_to_name(signal_name),
                        core_dumped,
                    );
                }
                Some(ChannelMsg::Close) | None => {
                    self.closed = true;
                    return Ok(0);
                }
                _ => {}
            }
        }
    }

    /// Writes bytes to the shell stdin.
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        self.channel.data(data).await.map_err(map_shell_error)?;
        Ok(data.len())
    }

    /// Writes all bytes to the shell stdin.
    pub async fn write_all(&self, data: &[u8]) -> Result<()> {
        self.write(data).await?;
        Ok(())
    }

    /// Sends EOF on stdin.
    pub async fn send_eof(&self) -> Result<()> {
        self.channel.eof().await.map_err(map_shell_error)
    }

    /// Resizes the terminal window.
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<()> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(map_shell_error)
    }

    /// Sends a signal to the remote process.
    pub async fn signal(&self, sig: russh::Sig) -> Result<()> {
        self.channel.signal(sig).await.map_err(map_shell_error)
    }

    /// Returns the exit status if the remote process has exited.
    pub fn exit(&self) -> Option<&CommandExit> {
        if self.exit != CommandExit::Missing {
            Some(&self.exit)
        } else {
            None
        }
    }

    /// Returns whether the channel is closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Closes the channel.
    pub async fn close(self) -> Result<()> {
        self.channel.close().await.map_err(map_shell_error)
    }

    /// Returns a reference to the underlying russh channel.
    pub fn russh_channel(&self) -> &russh::Channel<russh::client::Msg> {
        &self.channel
    }

    /// Converts this shell handle into an `AsyncRead` + `AsyncWrite` wrapper.
    ///
    /// Spawns a background task that bridges the underlying SSH channel
    /// to pollable read/write streams. The returned [`ShellAsyncIo`]
    /// implements [`tokio::io::AsyncRead`] and [`tokio::io::AsyncWrite`],
    /// enabling use with `tokio::io::copy`, `tokio::io::split`, etc.
    ///
    /// The original `ShellHandle` is consumed. After calling this method,
    /// use the returned `ShellAsyncIo` for all I/O.
    pub fn into_async_io(self) -> ShellAsyncIo {
        let (read_tx, read_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let exit = Arc::new(Mutex::new(CommandExit::Missing));

        let exit_clone = exit.clone();
        tokio::spawn(async move {
            run_channel_bridge(self.channel, read_tx, cmd_rx, exit_clone).await;
        });

        ShellAsyncIo {
            read_rx,
            cmd_tx,
            read_buf: Vec::new(),
            read_pos: 0,
            eof_sent: false,
        }
    }
}

/// SSH shell configuration.
///
/// Created by [`ShellBuilder::build`]. Call [`open`](Shell::open) to
/// start the interactive shell.
#[derive(Clone)]
pub struct Shell {
    session_id: SessionId,
    handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
    pty: Option<Pty>,
    env: Vec<(String, String)>,
    timeouts: Timeouts,
    x11_params: Option<X11Params>,
    want_agent_forward: bool,
}

impl fmt::Debug for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shell")
            .field("session_id", &self.session_id)
            .field("pty", &self.pty)
            .field("env_count", &self.env.len())
            .field("timeouts", &self.timeouts)
            .field("x11_params", &self.x11_params)
            .field("agent_forward", &self.want_agent_forward)
            .finish()
    }
}

impl Shell {
    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the PTY configuration, if any.
    pub fn pty(&self) -> Option<&Pty> {
        self.pty.as_ref()
    }

    /// Opens the interactive shell.
    ///
    /// Opens a session channel, requests PTY and environment variables
    /// (if configured), optionally requests X11 forwarding and agent
    /// forwarding, and starts the remote shell. Returns a streaming
    /// [`ShellHandle`] on success.
    pub async fn open(self) -> Result<ShellHandle> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| Error::unsupported("shell requires a connected session"))?;
        let handle_guard = handle.lock().await;
        let mut channel = time::timeout(
            self.timeouts.channel_open,
            handle_guard.channel_open_session(),
        )
        .await
        .map_err(|_| Error::timeout(Operation::ChannelOpen, "session channel open timed out"))?
        .map_err(map_channel_open_error)?;

        if let Some(pty) = &self.pty {
            let modes = build_terminal_modes(pty);

            channel
                .request_pty(
                    true,
                    pty.term(),
                    pty.width_columns(),
                    pty.height_rows(),
                    pty.width_pixels(),
                    pty.height_pixels(),
                    &modes,
                )
                .await
                .map_err(map_shell_error)?;

            match channel.wait().await {
                Some(ChannelMsg::Success) => {}
                Some(ChannelMsg::Failure) | None => {
                    let _ = channel.close().await;
                    return Err(Error::channel_kind(
                        ChannelErrorKind::Request,
                        "PTY request was rejected by the server",
                    ));
                }
                _ => {}
            }

            for (name, value) in &self.env {
                let _ = channel.set_env(true, name.as_str(), value.as_str()).await;
                let _ = channel.wait().await;
            }
        }

        if let Some(ref x11) = self.x11_params {
            channel
                .request_x11(
                    false,
                    x11.single_connection,
                    x11.protocol.as_str(),
                    x11.cookie.as_str(),
                    x11.screen,
                )
                .await
                .map_err(map_shell_error)?;
        }

        if self.want_agent_forward {
            channel
                .agent_forward(false)
                .await
                .map_err(map_shell_error)?;
        }

        channel.request_shell(true).await.map_err(map_shell_error)?;

        match channel.wait().await {
            Some(ChannelMsg::Success) => {}
            Some(ChannelMsg::Failure) | None => {
                let _ = channel.close().await;
                return Err(Error::channel_kind(
                    ChannelErrorKind::Request,
                    "shell request was rejected by the server",
                ));
            }
            _ => {}
        }

        Ok(ShellHandle {
            channel,
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            exit: CommandExit::Missing,
            closed: false,
        })
    }
}

/// Builder for [`Shell`].
#[derive(Clone)]
pub struct ShellBuilder {
    session_id: SessionId,
    handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
    pty: Option<Pty>,
    env: Vec<(String, String)>,
    timeouts: Timeouts,
    x11_params: Option<X11Params>,
    want_agent_forward: bool,
}

impl fmt::Debug for ShellBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellBuilder")
            .field("session_id", &self.session_id)
            .field("pty", &self.pty)
            .field("env_count", &self.env.len())
            .field("timeouts", &self.timeouts)
            .field("x11_params", &self.x11_params)
            .field("agent_forward", &self.want_agent_forward)
            .finish()
    }
}

impl ShellBuilder {
    /// Creates a shell builder for a session.
    pub(crate) fn from_session(
        session_id: SessionId,
        handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            pty: None,
            env: Vec::new(),
            timeouts,
            x11_params: None,
            want_agent_forward: false,
        }
    }

    /// Sets the PTY configuration.
    pub fn pty(mut self, pty: Pty) -> Self {
        self.pty = Some(pty);
        self
    }

    /// Sets an environment variable on the remote shell.
    ///
    /// The server may ignore or reject this request. Multiple calls
    /// add multiple variables.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Enables X11 forwarding.
    ///
    /// Generates a random MIT-MAGIC-COOKIE-1 cookie for the given screen
    /// number.
    pub fn x11(mut self, screen: u32) -> Self {
        self.x11_params = Some(X11Params::new(screen));
        self
    }

    /// Enables X11 forwarding with a custom authentication protocol and cookie.
    pub fn x11_with_cookie(
        mut self,
        protocol: impl Into<String>,
        cookie: impl Into<String>,
        screen: u32,
    ) -> Self {
        self.x11_params = Some(X11Params::with_cookie(protocol, cookie, screen));
        self
    }

    /// Provides explicit X11 forwarding parameters.
    pub fn x11_params(mut self, params: X11Params) -> Self {
        self.x11_params = Some(params);
        self
    }

    /// Requests SSH agent forwarding (`auth-agent-req@openssh.com`).
    pub fn agent_forward(mut self) -> Self {
        self.want_agent_forward = true;
        self
    }

    /// Builds the [`Shell`] configuration.
    pub fn build(self) -> Shell {
        Shell {
            session_id: self.session_id,
            handle: self.handle,
            pty: self.pty,
            env: self.env,
            timeouts: self.timeouts,
            x11_params: self.x11_params,
            want_agent_forward: self.want_agent_forward,
        }
    }
}

/// SSH subsystem configuration.
///
/// Created by [`SubsystemBuilder::build`]. Call [`open`](Subsystem::open)
/// to start the subsystem.
#[derive(Clone)]
pub struct Subsystem {
    session_id: SessionId,
    handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
    name: String,
    timeouts: Timeouts,
    x11_params: Option<X11Params>,
    want_agent_forward: bool,
}

impl fmt::Debug for Subsystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subsystem")
            .field("session_id", &self.session_id)
            .field("name", &self.name)
            .field("timeouts", &self.timeouts)
            .field("x11_params", &self.x11_params)
            .field("agent_forward", &self.want_agent_forward)
            .finish()
    }
}

impl Subsystem {
    /// Returns the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the subsystem name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Opens the subsystem channel.
    ///
    /// Opens a session channel and requests the named subsystem.
    /// Returns a streaming [`ShellHandle`] for I/O.
    pub async fn open(self) -> Result<ShellHandle> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| Error::unsupported("subsystem requires a connected session"))?;
        let handle_guard = handle.lock().await;
        let mut channel = time::timeout(
            self.timeouts.channel_open,
            handle_guard.channel_open_session(),
        )
        .await
        .map_err(|_| Error::timeout(Operation::ChannelOpen, "session channel open timed out"))?
        .map_err(map_channel_open_error)?;

        if let Some(ref x11) = self.x11_params {
            channel
                .request_x11(
                    false,
                    x11.single_connection,
                    x11.protocol.as_str(),
                    x11.cookie.as_str(),
                    x11.screen,
                )
                .await
                .map_err(map_shell_error)?;
        }

        if self.want_agent_forward {
            channel
                .agent_forward(false)
                .await
                .map_err(map_shell_error)?;
        }

        channel
            .request_subsystem(true, self.name.clone())
            .await
            .map_err(map_shell_error)?;

        match channel.wait().await {
            Some(ChannelMsg::Success) => {}
            Some(ChannelMsg::Failure) | None => {
                let _ = channel.close().await;
                return Err(Error::channel_kind(
                    ChannelErrorKind::Request,
                    format!("subsystem '{}' was rejected by the server", self.name),
                ));
            }
            _ => {}
        }

        Ok(ShellHandle {
            channel,
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            exit: CommandExit::Missing,
            closed: false,
        })
    }
}

/// Builder for [`Subsystem`].
#[derive(Clone)]
pub struct SubsystemBuilder {
    session_id: SessionId,
    handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
    name: String,
    timeouts: Timeouts,
    x11_params: Option<X11Params>,
    want_agent_forward: bool,
}

impl fmt::Debug for SubsystemBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsystemBuilder")
            .field("session_id", &self.session_id)
            .field("name", &self.name)
            .field("timeouts", &self.timeouts)
            .field("x11_params", &self.x11_params)
            .field("agent_forward", &self.want_agent_forward)
            .finish()
    }
}

impl SubsystemBuilder {
    /// Creates a subsystem builder for a session.
    pub(crate) fn from_session(
        session_id: SessionId,
        handle: Option<Arc<Mutex<client::Handle<super::ClientHandler>>>>,
        name: String,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            session_id,
            handle,
            name,
            timeouts,
            x11_params: None,
            want_agent_forward: false,
        }
    }

    /// Enables X11 forwarding.
    ///
    /// Generates a random MIT-MAGIC-COOKIE-1 cookie for the given screen number.
    pub fn x11(mut self, screen: u32) -> Self {
        self.x11_params = Some(X11Params::new(screen));
        self
    }

    /// Enables X11 forwarding with a custom authentication protocol and cookie.
    pub fn x11_with_cookie(
        mut self,
        protocol: impl Into<String>,
        cookie: impl Into<String>,
        screen: u32,
    ) -> Self {
        self.x11_params = Some(X11Params::with_cookie(protocol, cookie, screen));
        self
    }

    /// Provides explicit X11 forwarding parameters.
    pub fn x11_params(mut self, params: X11Params) -> Self {
        self.x11_params = Some(params);
        self
    }

    /// Requests SSH agent forwarding.
    pub fn agent_forward(mut self) -> Self {
        self.want_agent_forward = true;
        self
    }

    /// Builds the [`Subsystem`] configuration.
    pub fn build(self) -> Subsystem {
        Subsystem {
            session_id: self.session_id,
            handle: self.handle,
            name: self.name,
            timeouts: self.timeouts,
            x11_params: self.x11_params,
            want_agent_forward: self.want_agent_forward,
        }
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
            russh_extra_core::AuthenticationErrorKind::Unavailable,
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
            russh_extra_core::TransportErrorKind::Io,
            "transport I/O failed while opening a session channel",
            source,
        ),
        error => {
            Error::channel_with_source(ChannelErrorKind::Open, "session channel open failed", error)
        }
    }
}

fn map_shell_error(error: russh::Error) -> Error {
    match error {
        russh::Error::RequestDenied => Error::channel_with_source(
            ChannelErrorKind::Request,
            "channel request was denied",
            error,
        ),
        russh::Error::WrongChannel | russh::Error::Inconsistent => Error::channel_with_source(
            ChannelErrorKind::Protocol,
            "channel entered an invalid state",
            error,
        ),
        russh::Error::SendError => Error::channel_with_source(
            ChannelErrorKind::Write,
            "failed to write channel data",
            error,
        ),
        russh::Error::ConnectionTimeout | russh::Error::Elapsed(_) => {
            Error::timeout(Operation::Shell, "shell operation timed out")
        }
        russh::Error::Disconnect | russh::Error::HUP => Error::disconnected(
            Operation::Shell,
            "server disconnected during shell operation",
        ),
        russh::Error::IO(source) => {
            Error::channel_with_source(ChannelErrorKind::Read, "shell channel I/O failed", source)
        }
        error => {
            Error::channel_with_source(ChannelErrorKind::Protocol, "shell operation failed", error)
        }
    }
}

// ── ShellAsyncIo (AsyncRead + AsyncWrite wrapper) ──────────────────

/// Command sent from `ShellAsyncIo` to the channel bridge task.
#[derive(Debug)]
enum ShellCmd {
    Write(Vec<u8>),
    Eof,
    Resize(u32, u32),
    Signal(russh::Sig),
}

/// An `AsyncRead` + `AsyncWrite` wrapper around a shell or subsystem channel.
///
/// Obtained via [`ShellHandle::into_async_io`]. Spawns a background task
/// that bridges the underlying `russh` channel to pollable streams.
///
/// Implements [`tokio::io::AsyncRead`] and [`tokio::io::AsyncWrite`],
/// enabling use with `tokio::io::copy`, `tokio::io::split`, etc.
pub struct ShellAsyncIo {
    read_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    cmd_tx: mpsc::UnboundedSender<ShellCmd>,
    read_buf: Vec<u8>,
    read_pos: usize,
    eof_sent: bool,
}

impl fmt::Debug for ShellAsyncIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellAsyncIo")
            .field("eof_sent", &self.eof_sent)
            .finish()
    }
}

impl ShellAsyncIo {
    /// Resizes the terminal window.
    pub fn resize(&self, cols: u32, rows: u32) {
        let _ = self.cmd_tx.send(ShellCmd::Resize(cols, rows));
    }

    /// Sends a signal to the remote process.
    pub fn signal(&self, sig: russh::Sig) {
        let _ = self.cmd_tx.send(ShellCmd::Signal(sig));
    }
}

impl AsyncRead for ShellAsyncIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_pos < self.read_buf.len() {
            let remaining = &self.read_buf[self.read_pos..];
            let n = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..n]);
            self.read_pos += n;
            return Poll::Ready(Ok(()));
        }

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
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for ShellAsyncIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.cmd_tx
            .send(ShellCmd::Write(buf.to_vec()))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "shell channel closed"))?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.eof_sent {
            let _ = self.cmd_tx.send(ShellCmd::Eof);
            self.eof_sent = true;
        }
        Poll::Ready(Ok(()))
    }
}

/// Background task that bridges the SSH channel with mpsc channels.
///
/// Reads `ChannelMsg` from the channel and forwards data to `read_tx`.
/// Receives commands from `cmd_rx` and issues them on the channel.
async fn run_channel_bridge(
    mut channel: russh::Channel<russh::client::Msg>,
    read_tx: mpsc::UnboundedSender<Vec<u8>>,
    mut cmd_rx: mpsc::UnboundedReceiver<ShellCmd>,
    exit: Arc<Mutex<CommandExit>>,
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
                    Some(ChannelMsg::ExtendedData { data, ext: _ }) => {
                        let _ = read_tx.send(data.to_vec());
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        *exit.lock().await = CommandExit::Status(exit_status);
                    }
                    Some(ChannelMsg::ExitSignal { signal_name, core_dumped, .. }) => {
                        *exit.lock().await = CommandExit::Signal(
                            crate::client::signal_to_name(signal_name),
                            core_dumped,
                        );
                    }
                    Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    _ => {}
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ShellCmd::Write(data)) => {
                        let _ = channel.data(data.as_slice()).await;
                    }
                    Some(ShellCmd::Eof) => {
                        let _ = channel.eof().await;
                    }
                    Some(ShellCmd::Resize(cols, rows)) => {
                        let _ = channel.window_change(cols, rows, 0, 0).await;
                    }
                    Some(ShellCmd::Signal(sig)) => {
                        let _ = channel.signal(sig).await;
                    }
                    None => {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh_extra_core::{Pty, TerminalMode};

    #[test]
    fn terminal_mode_converts_to_russh_opcodes() {
        assert!(
            terminal_mode_to_pty(TerminalMode::Interrupt)
                .map(|(p, _)| p == russh::Pty::VINTR)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::Quit)
                .map(|(p, _)| p == russh::Pty::VQUIT)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::Erase)
                .map(|(p, _)| p == russh::Pty::VERASE)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::Kill)
                .map(|(p, _)| p == russh::Pty::VKILL)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::EndOfFile)
                .map(|(p, _)| p == russh::Pty::VEOF)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::InputSpeed)
                .map(|(p, _)| p == russh::Pty::TTY_OP_ISPEED)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::OutputSpeed)
                .map(|(p, _)| p == russh::Pty::TTY_OP_OSPEED)
                .unwrap_or(false)
        );
    }

    #[test]
    fn terminal_mode_custom_maps_known_opcodes() {
        let result = terminal_mode_to_pty(TerminalMode::Custom(53));
        assert!(result.map(|(p, _)| p == russh::Pty::ECHO).unwrap_or(false));
    }

    #[test]
    fn terminal_mode_new_variants_map_correctly() {
        use russh_extra_core::TerminalMode;

        assert!(
            terminal_mode_to_pty(TerminalMode::Echo)
                .map(|(p, _)| p == russh::Pty::ECHO)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::EchoErase)
                .map(|(p, _)| p == russh::Pty::ECHOE)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::EchoKill)
                .map(|(p, _)| p == russh::Pty::ECHOK)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::EchoNl)
                .map(|(p, _)| p == russh::Pty::ECHONL)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::CanonicalInput)
                .map(|(p, _)| p == russh::Pty::ICANON)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::SigCheck)
                .map(|(p, _)| p == russh::Pty::ISIG)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::CrToNlInput)
                .map(|(p, _)| p == russh::Pty::ICRNL)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::NlToCrInput)
                .map(|(p, _)| p == russh::Pty::INLCR)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::IgnoreCrInput)
                .map(|(p, _)| p == russh::Pty::IGNCR)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::PostProcessOutput)
                .map(|(p, _)| p == russh::Pty::OPOST)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::NlToCrNlOutput)
                .map(|(p, _)| p == russh::Pty::ONLCR)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::CrToNlOutput)
                .map(|(p, _)| p == russh::Pty::OCRNL)
                .unwrap_or(false)
        );
        assert!(
            terminal_mode_to_pty(TerminalMode::NoCrOnNl)
                .map(|(p, _)| p == russh::Pty::ONLRET)
                .unwrap_or(false)
        );
    }

    #[test]
    fn terminal_mode_custom_returns_none_for_invalid() {
        assert!(terminal_mode_to_pty(TerminalMode::Custom(255)).is_none());
    }

    #[test]
    fn build_terminal_modes_from_pty() {
        let pty = Pty::new("xterm", 80, 24)
            .with_mode(TerminalMode::Erase, 1)
            .with_mode(TerminalMode::Interrupt, 3);
        let modes = build_terminal_modes(&pty);
        assert_eq!(modes.len(), 2);
        assert!(modes.contains(&(russh::Pty::VERASE, 1)));
        assert!(modes.contains(&(russh::Pty::VINTR, 3)));
    }

    #[test]
    fn build_terminal_modes_skips_unmappable() {
        let pty = Pty::new("xterm", 80, 24).with_mode(TerminalMode::Custom(255), 1);
        let modes = build_terminal_modes(&pty);
        assert!(modes.is_empty());
    }

    #[test]
    fn shell_builder_stores_pty_and_env() {
        let pty = Pty::new("xterm", 120, 40);
        let builder = ShellBuilder::from_session(SessionId::next(), None, Timeouts::default())
            .pty(pty.clone())
            .env("LANG", "C.UTF-8")
            .env("TERM", "xterm-256color");

        let shell = builder.build();
        assert_eq!(shell.pty().unwrap(), &pty);
    }

    #[test]
    fn subsystem_builder_stores_name() {
        let builder = SubsystemBuilder::from_session(
            SessionId::next(),
            None,
            "sftp".into(),
            Timeouts::default(),
        );
        let subsystem = builder.build();
        assert_eq!(subsystem.name(), "sftp");
    }

    #[test]
    fn shell_debug_redacts_env_variables() {
        let shell = ShellBuilder::from_session(SessionId::next(), None, Timeouts::default())
            .env("SECRET", "value")
            .build();
        let debug = format!("{shell:?}");
        assert!(!debug.contains("SECRET"));
        assert!(!debug.contains("value"));
        assert!(debug.contains("env_count"));
    }

    #[test]
    fn x11_params_new_generates_32_char_hex_cookie() {
        let params = X11Params::new(0);
        assert_eq!(params.protocol, "MIT-MAGIC-COOKIE-1");
        assert_eq!(params.cookie.len(), 32);
        assert!(params.cookie.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(params.screen, 0);
    }

    #[test]
    fn x11_params_with_cookie_uses_provided_values() {
        let params = X11Params::with_cookie("XDM-AUTHORIZATION-1", "deadbeef", 1);
        assert_eq!(params.protocol, "XDM-AUTHORIZATION-1");
        assert_eq!(params.cookie, "deadbeef");
        assert_eq!(params.screen, 1);
    }

    #[test]
    fn x11_params_debug_redacts_cookie() {
        let params = X11Params::with_cookie("MIT-MAGIC-COOKIE-1", "deadbeef1234", 0);
        let debug = format!("{:?}", params);
        assert!(debug.contains("X11Params"));
        assert!(!debug.contains(&params.cookie), "cookie leaked: {debug}");
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn shell_builder_supports_x11_and_agent_forwarding() {
        let builder = ShellBuilder::from_session(SessionId::next(), None, Timeouts::default())
            .x11(0)
            .agent_forward();

        let shell = builder.build();
        assert!(shell.x11_params.is_some());
        assert!(shell.want_agent_forward);
    }

    #[test]
    fn subsystem_builder_supports_x11_and_agent_forwarding() {
        let builder = SubsystemBuilder::from_session(
            SessionId::next(),
            None,
            "sftp".into(),
            Timeouts::default(),
        )
        .x11_with_cookie("MIT-MAGIC-COOKIE-1", "abc123", 0)
        .agent_forward();

        let sub = builder.build();
        assert!(sub.x11_params.is_some());
        assert!(sub.want_agent_forward);
    }
}
