//! Shared types for `russh-extra`.
//!
//! This crate contains feature-neutral SSH domain types that are used by the
//! user-facing crate, macros, tests, and future companion crates.

pub mod auth;
pub mod channel;
pub mod config;
pub mod error;
pub mod forward;
pub mod session;

pub use auth::{
    ClientKeyboardInteractiveInfo, ClientKeyboardInteractivePrompt, Credential, Identity,
    KeyboardInteractiveHandler, KeyboardInteractiveReply, Password, Username,
};
pub use channel::{
    ChannelKind, CommandExit, CommandLimits, DEFAULT_COMMAND_OUTPUT_LIMIT, Pty, TerminalMode,
};
pub use config::{
    ClientConfig, HostKeyFingerprint, HostKeyFingerprintAlgorithm, HostKeyPolicy, Keepalive,
    ServerConfig, Timeouts,
};
pub use error::{
    AuthenticationError, AuthenticationErrorKind, BoxError, CancelledError, CategoryError,
    ChannelError, ChannelErrorKind, DisconnectedError, Error, ForwardingError, ForwardingErrorKind,
    HostKeyError, HostKeyErrorKind, Operation, Result, SftpError, SftpErrorKind, SshError,
    SshErrorKind, TimeoutError, TransportError, TransportErrorKind,
};
pub use forward::{ForwardDirection, ForwardSpec, StreamLocalSpec, TcpEndpoint};
pub use session::{DEFAULT_SSH_PORT, Endpoint, SessionId};
