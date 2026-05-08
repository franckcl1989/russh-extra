//! Error and result types used across the workspace.

use std::{borrow::Cow, error::Error as StdError, fmt};

use crate::CommandExit;

/// Boxed error source preserved for diagnostics.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Result type used by `russh-extra`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Shared category details for typed error variants.
pub struct CategoryError<K> {
    kind: K,
    message: Cow<'static, str>,
    source: Option<BoxError>,
}

impl<K> CategoryError<K> {
    /// Creates a category error without a lower-level source.
    pub fn new(kind: K, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Creates a category error with a lower-level source.
    pub fn with_source<E>(kind: K, message: impl Into<Cow<'static, str>>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::with_boxed_source(kind, message, Box::new(source))
    }

    /// Creates a category error with an already boxed lower-level source.
    pub fn with_boxed_source(
        kind: K,
        message: impl Into<Cow<'static, str>>,
        source: BoxError,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            source: Some(source),
        }
    }

    /// Returns the stable subcategory.
    pub fn kind(&self) -> K
    where
        K: Copy,
    {
        self.kind
    }

    /// Returns the user-facing message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns whether a lower-level source is preserved.
    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }
}

impl<K> fmt::Debug for CategoryError<K>
where
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CategoryError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("has_source", &self.source.is_some())
            .finish()
    }
}

impl<K> fmt::Display for CategoryError<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl<K> StdError for CategoryError<K>
where
    K: fmt::Debug + 'static,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn StdError + 'static))
    }
}

/// Transport failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TransportErrorKind {
    /// DNS resolution failed.
    Dns,
    /// TCP connection failed.
    TcpConnect,
    /// SSH negotiation failed.
    Negotiation,
    /// Keepalive failed.
    Keepalive,
    /// Encryption or MAC handling failed.
    Encryption,
    /// Transport I/O failed.
    Io,
    /// Other transport failure.
    Other,
}

/// Host-key verification failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HostKeyErrorKind {
    /// Host key is unknown to the configured policy.
    Unknown,
    /// Host key changed from a previously trusted value.
    Changed,
    /// Host key was rejected by policy.
    Rejected,
    /// Host key algorithm or format is unsupported.
    Unsupported,
    /// Host key was unavailable when required.
    Unavailable,
}

/// Authentication failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuthenticationErrorKind {
    /// Credentials were rejected.
    Rejected,
    /// All configured credentials were exhausted.
    Exhausted,
    /// Authentication partially succeeded but did not complete.
    Partial,
    /// Requested authentication method is unsupported.
    UnsupportedMethod,
    /// Authentication could not be attempted.
    Unavailable,
}

/// Channel failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChannelErrorKind {
    /// Channel open failed.
    Open,
    /// Channel request failed.
    Request,
    /// Channel read failed.
    Read,
    /// Channel write failed.
    Write,
    /// Unexpected EOF.
    Eof,
    /// Channel close failed or arrived unexpectedly.
    Close,
    /// Protocol ordering or framing was invalid for the high-level API.
    Protocol,
}

/// SFTP failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SftpErrorKind {
    /// Remote SFTP status response indicated failure.
    Status,
    /// Packet was malformed.
    MalformedPacket,
    /// Protocol version is unsupported.
    UnsupportedVersion,
    /// Required extension is unsupported.
    UnsupportedExtension,
    /// Response request ID did not match an in-flight request.
    RequestIdMismatch,
    /// Local I/O failed while handling SFTP.
    LocalIo,
    /// Remote disconnected while SFTP work was in flight.
    RemoteDisconnect,
}

/// Forwarding failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ForwardingErrorKind {
    /// Local or remote bind failed.
    Bind,
    /// Listener setup failed.
    Listen,
    /// Accepting a forwarded connection failed.
    Accept,
    /// Connecting to a forwarding target failed.
    Connect,
    /// SSH global forwarding request failed.
    GlobalRequest,
    /// Opening a forwarding channel failed.
    ChannelOpen,
    /// Bidirectional stream copy failed.
    StreamCopy,
    /// Forwarding cancellation failed.
    Cancel,
    /// Forwarding shutdown failed.
    Shutdown,
}

/// High-level operation category used by timeout, cancellation, and disconnects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Operation {
    /// Establishing a client connection.
    Connect,
    /// Authenticating a client or server session.
    Authentication,
    /// Opening a channel.
    ChannelOpen,
    /// Running a remote command.
    Command,
    /// Running an interactive shell.
    Shell,
    /// Running SFTP.
    Sftp,
    /// Running forwarding or tunnel work.
    Forwarding,
    /// Running server work.
    Server,
    /// Shutting down an operation.
    Shutdown,
    /// Other operation.
    Other,
}

/// Lower-level SSH failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SshErrorKind {
    /// Error came from `russh`.
    Russh,
    /// Other lower-level SSH error.
    Other,
}

/// Transport failure details.
pub type TransportError = CategoryError<TransportErrorKind>;
/// Host-key verification failure details.
pub type HostKeyError = CategoryError<HostKeyErrorKind>;
/// Authentication failure details.
pub type AuthenticationError = CategoryError<AuthenticationErrorKind>;
/// Channel failure details.
pub type ChannelError = CategoryError<ChannelErrorKind>;
/// SFTP failure details.
pub type SftpError = CategoryError<SftpErrorKind>;
/// Forwarding failure details.
pub type ForwardingError = CategoryError<ForwardingErrorKind>;
/// Timeout failure details.
pub type TimeoutError = CategoryError<Operation>;
/// Cancellation failure details.
pub type CancelledError = CategoryError<Operation>;
/// Remote disconnect failure details.
pub type DisconnectedError = CategoryError<Operation>;
/// Lower-level SSH failure details.
pub type SshError = CategoryError<SshErrorKind>;

/// Error type used by `russh-extra`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A builder or parser received invalid configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(Cow<'static, str>),

    /// SSH transport failed.
    #[error("transport error: {0}")]
    Transport(#[source] TransportError),

    /// Host-key verification failed.
    #[error("host key verification failed: {0}")]
    HostKey(#[source] HostKeyError),

    /// Authentication was rejected or could not be attempted.
    #[error("authentication failed: {0}")]
    Authentication(#[source] AuthenticationError),

    /// A channel could not be opened or used.
    #[error("channel error: {0}")]
    Channel(#[source] ChannelError),

    /// A remote command exited unsuccessfully.
    #[error("remote command exited unsuccessfully: {exit:?}")]
    CommandExit {
        /// Reported remote command exit.
        exit: CommandExit,
    },

    /// SFTP operation failed.
    #[error("sftp error: {0}")]
    Sftp(#[source] SftpError),

    /// Forwarding operation failed.
    #[error("forwarding error: {0}")]
    Forwarding(#[source] ForwardingError),

    /// Operation timed out.
    #[error("operation timed out: {0}")]
    Timeout(#[source] TimeoutError),

    /// Operation was cancelled.
    #[error("operation cancelled: {0}")]
    Cancelled(#[source] CancelledError),

    /// Remote peer disconnected.
    #[error("remote disconnected: {0}")]
    Disconnected(#[source] DisconnectedError),

    /// A requested operation is not implemented or not supported.
    #[error("unsupported operation: {0}")]
    Unsupported(Cow<'static, str>),

    /// Local I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An unclassified lower-level SSH error occurred.
    #[error("ssh error: {0}")]
    Ssh(#[source] SshError),
}

impl Error {
    /// Creates an invalid configuration error.
    pub fn invalid_config(message: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidConfig(message.into())
    }

    /// Creates a transport error.
    pub fn transport(kind: TransportErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Transport(TransportError::new(kind, message))
    }

    /// Creates a transport error with a lower-level source.
    pub fn transport_with_source<E>(
        kind: TransportErrorKind,
        message: impl Into<Cow<'static, str>>,
        source: E,
    ) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Transport(TransportError::with_source(kind, message, source))
    }

    /// Creates a host-key verification error.
    pub fn host_key(kind: HostKeyErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self::HostKey(HostKeyError::new(kind, message))
    }

    /// Creates a host-key verification error with a lower-level source.
    pub fn host_key_with_source<E>(
        kind: HostKeyErrorKind,
        message: impl Into<Cow<'static, str>>,
        source: E,
    ) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::HostKey(HostKeyError::with_source(kind, message, source))
    }

    /// Creates an authentication error.
    pub fn authentication(message: impl Into<Cow<'static, str>>) -> Self {
        Self::Authentication(AuthenticationError::new(
            AuthenticationErrorKind::Rejected,
            message,
        ))
    }

    /// Creates an authentication error with a specific category.
    pub fn authentication_kind(
        kind: AuthenticationErrorKind,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Authentication(AuthenticationError::new(kind, message))
    }

    /// Creates an authentication error with a lower-level source.
    pub fn authentication_with_source<E>(
        kind: AuthenticationErrorKind,
        message: impl Into<Cow<'static, str>>,
        source: E,
    ) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Authentication(AuthenticationError::with_source(kind, message, source))
    }

    /// Creates a channel error.
    pub fn channel(message: impl Into<Cow<'static, str>>) -> Self {
        Self::Channel(ChannelError::new(ChannelErrorKind::Protocol, message))
    }

    /// Creates a channel error with a specific category.
    pub fn channel_kind(kind: ChannelErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Channel(ChannelError::new(kind, message))
    }

    /// Creates a channel error with a lower-level source.
    pub fn channel_with_source<E>(
        kind: ChannelErrorKind,
        message: impl Into<Cow<'static, str>>,
        source: E,
    ) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Channel(ChannelError::with_source(kind, message, source))
    }

    /// Creates a remote command exit error.
    pub fn command_exit(exit: CommandExit) -> Self {
        Self::CommandExit { exit }
    }

    /// Creates an SFTP error.
    pub fn sftp(kind: SftpErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Sftp(SftpError::new(kind, message))
    }

    /// Creates a forwarding error.
    pub fn forwarding(kind: ForwardingErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Forwarding(ForwardingError::new(kind, message))
    }

    /// Creates a timeout error.
    pub fn timeout(operation: Operation, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Timeout(TimeoutError::new(operation, message))
    }

    /// Creates a cancellation error.
    pub fn cancelled(operation: Operation, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Cancelled(CancelledError::new(operation, message))
    }

    /// Creates a remote disconnect error.
    pub fn disconnected(operation: Operation, message: impl Into<Cow<'static, str>>) -> Self {
        Self::Disconnected(DisconnectedError::new(operation, message))
    }

    /// Creates an unsupported operation error.
    pub fn unsupported(message: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported(message.into())
    }

    /// Creates an unclassified lower-level SSH error with a source.
    pub fn ssh_with_source<E>(message: impl Into<Cow<'static, str>>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Ssh(SshError::with_source(SshErrorKind::Other, message, source))
    }

    /// Returns whether this error is a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
            || matches!(self, Self::Io(error) if error.kind() == std::io::ErrorKind::TimedOut)
    }

    /// Returns whether this error is a cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    /// Returns whether this error is a remote disconnect.
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected(_))
    }
}

impl From<BoxError> for Error {
    fn from(source: BoxError) -> Self {
        Self::Ssh(SshError::with_boxed_source(
            SshErrorKind::Other,
            "lower-level SSH error",
            source,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error as StdError, fmt};

    use crate::{
        AuthenticationErrorKind, Error, HostKeyError, HostKeyErrorKind, Operation, SshErrorKind,
        TransportError, TransportErrorKind,
    };

    #[derive(Debug)]
    struct SourceError;

    impl fmt::Display for SourceError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("source display")
        }
    }

    impl StdError for SourceError {}

    #[derive(Debug)]
    struct SecretSource;

    impl fmt::Display for SecretSource {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("secret-source-display")
        }
    }

    impl StdError for SecretSource {}

    #[test]
    fn category_errors_preserve_source_without_debugging_it() {
        let error = TransportError::with_source(
            TransportErrorKind::Dns,
            "failed to resolve host",
            SecretSource,
        );

        assert_eq!(error.kind(), TransportErrorKind::Dns);
        assert_eq!(error.message(), "failed to resolve host");
        assert!(error.has_source());
        assert!(StdError::source(&error).is_some());

        let debug = format!("{error:?}");
        assert!(debug.contains("Dns"));
        assert!(debug.contains("has_source: true"));
        assert!(!debug.contains("secret-source-display"));
    }

    #[test]
    fn top_level_errors_preserve_category_sources() {
        let error = Error::transport_with_source(
            TransportErrorKind::TcpConnect,
            "tcp connect failed",
            SourceError,
        );

        let category = StdError::source(&error).expect("category source");
        assert_eq!(category.to_string(), "tcp connect failed");
        assert!(category.source().is_some());
    }

    #[test]
    fn helper_predicates_classify_common_control_flow() {
        assert!(Error::timeout(Operation::Connect, "connect timed out").is_timeout());
        assert!(Error::from(std::io::Error::new(std::io::ErrorKind::TimedOut, "io")).is_timeout());
        assert!(Error::cancelled(Operation::Command, "cancelled").is_cancelled());
        assert!(Error::disconnected(Operation::Sftp, "disconnect").is_disconnected());
        assert!(!Error::unsupported("not yet").is_timeout());
    }

    #[test]
    fn typed_variants_expose_stable_kinds() {
        let auth = Error::authentication_kind(AuthenticationErrorKind::Exhausted, "no credentials");
        let Error::Authentication(auth) = auth else {
            panic!("expected authentication error");
        };
        assert_eq!(auth.kind(), AuthenticationErrorKind::Exhausted);

        let host_key = Error::HostKey(HostKeyError::new(
            HostKeyErrorKind::Changed,
            "host key changed",
        ));
        let Error::HostKey(host_key) = host_key else {
            panic!("expected host key error");
        };
        assert_eq!(host_key.kind(), HostKeyErrorKind::Changed);
    }

    #[test]
    fn boxed_sources_convert_to_unclassified_ssh_errors() {
        let error = Error::from(Box::new(SourceError) as crate::BoxError);
        let Error::Ssh(ssh) = error else {
            panic!("expected ssh error");
        };

        assert_eq!(ssh.kind(), SshErrorKind::Other);
        assert!(ssh.has_source());
    }
}
