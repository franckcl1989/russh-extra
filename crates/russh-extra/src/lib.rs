//! High-level async SSH APIs built on top of `russh`.
//!
//! This crate is the user-facing entry point. It re-exports shared domain
//! types from `russh-extra-core` and exposes ergonomic builders for clients,
//! servers, shells, tunnels, and known-hosts verification. The `sftp` feature
//! currently exposes reserved experimental marker types while the native SFTP
//! runtime is designed.

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "known-hosts")]
pub mod known_hosts;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "sftp")]
pub mod sftp;
#[cfg(feature = "shell")]
pub mod shell;
#[cfg(feature = "tunnel")]
pub mod tunnel;

#[cfg(feature = "_russh")]
pub use russh;
pub use russh_extra_core::{
    AuthenticationError, AuthenticationErrorKind, BoxError, CancelledError, CategoryError,
    ChannelError, ChannelErrorKind, ChannelKind, ClientConfig, ClientKeyboardInteractiveInfo,
    ClientKeyboardInteractivePrompt, CommandExit, CommandLimits, Credential,
    DEFAULT_COMMAND_OUTPUT_LIMIT, DEFAULT_SSH_PORT, DisconnectedError, Endpoint, Error,
    ForwardDirection, ForwardSpec, ForwardingError, ForwardingErrorKind, HostKeyError,
    HostKeyErrorKind, HostKeyFingerprint, HostKeyFingerprintAlgorithm, HostKeyPolicy, Identity,
    Keepalive, KeyboardInteractiveHandler, KeyboardInteractiveReply, Operation, Password, Pty,
    Result, ServerConfig, SessionId, SftpError, SftpErrorKind, SshError, SshErrorKind,
    StreamLocalSpec, TcpEndpoint, TerminalMode, TimeoutError, Timeouts, TransportError,
    TransportErrorKind, Username,
};

#[cfg(feature = "client")]
pub use client::{
    Client, ClientBuilder, ClientHandler, CommandOutput, RemoteCommand, RusshHandleGuard, Session,
};
#[cfg(feature = "known-hosts")]
pub use known_hosts::{KnownHostStatus, KnownHosts, KnownHostsEntry, KnownHostsParseWarning};
#[cfg(feature = "server")]
pub use server::{
    AuthContext, AuthDecision, DirectTcpipContext, EnvRequest, ExecCommand, ExecContext,
    ExecResponse, ForwardedTcpipContext, KeyboardInteractiveContext, KeyboardInteractivePrompt,
    KeyboardInteractivePromptItem, KeyboardInteractiveResponse, PtyContext, PtyParams, Server,
    ServerBuilder, ServerEvent, ServerHandle, ServerHandler, ServerHostKey, SessionContext,
    ShellContext, StreamingExecCmd, StreamingExecContext, SubsystemContext, TcpipForwardContext,
    WindowChange,
};
#[cfg(feature = "sftp")]
pub use sftp::{SftpClient, SftpServer};
#[cfg(feature = "shell")]
pub use shell::{Shell, ShellBuilder, ShellHandle, Subsystem, SubsystemBuilder};
#[cfg(feature = "tunnel")]
pub use tunnel::{DirectTcpBuilder, Tunnel, TunnelBuilder, TunnelStream};
