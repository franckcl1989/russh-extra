//! Port forwarding domain types.

use std::path::PathBuf;

/// Direction of an SSH forwarding request.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ForwardDirection {
    /// Local forwarding: local listener to remote target.
    Local,
    /// Remote forwarding: remote listener to local target.
    Remote,
}

/// TCP endpoint used by forwarding.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TcpEndpoint {
    host: String,
    port: u16,
}

impl TcpEndpoint {
    /// Creates a TCP endpoint.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// Returns the host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the port.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl From<(&str, u16)> for TcpEndpoint {
    fn from((host, port): (&str, u16)) -> Self {
        Self::new(host, port)
    }
}

impl From<(String, u16)> for TcpEndpoint {
    fn from((host, port): (String, u16)) -> Self {
        Self::new(host, port)
    }
}

/// Unix-domain streamlocal forwarding endpoint.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StreamLocalSpec {
    path: PathBuf,
}

impl StreamLocalSpec {
    /// Creates a streamlocal endpoint.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the streamlocal path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl From<&str> for StreamLocalSpec {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl From<String> for StreamLocalSpec {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

impl From<PathBuf> for StreamLocalSpec {
    fn from(path: PathBuf) -> Self {
        Self { path }
    }
}

/// High-level forwarding specification.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForwardSpec {
    /// TCP forwarding between two endpoints.
    Tcp {
        /// Forward direction.
        direction: ForwardDirection,
        /// Bind endpoint.
        bind: TcpEndpoint,
        /// Target endpoint.
        target: TcpEndpoint,
    },
    /// Streamlocal forwarding between two paths.
    StreamLocal {
        /// Forward direction.
        direction: ForwardDirection,
        /// Bind endpoint.
        bind: StreamLocalSpec,
        /// Target endpoint.
        target: StreamLocalSpec,
    },
}

impl ForwardSpec {
    /// Creates a local TCP forwarding specification.
    pub fn local_tcp(bind: impl Into<TcpEndpoint>, target: impl Into<TcpEndpoint>) -> Self {
        Self::Tcp {
            direction: ForwardDirection::Local,
            bind: bind.into(),
            target: target.into(),
        }
    }

    /// Creates a remote TCP forwarding specification.
    pub fn remote_tcp(bind: impl Into<TcpEndpoint>, target: impl Into<TcpEndpoint>) -> Self {
        Self::Tcp {
            direction: ForwardDirection::Remote,
            bind: bind.into(),
            target: target.into(),
        }
    }

    /// Creates a local streamlocal forwarding specification.
    pub fn local_streamlocal(
        bind: impl Into<StreamLocalSpec>,
        target: impl Into<StreamLocalSpec>,
    ) -> Self {
        Self::StreamLocal {
            direction: ForwardDirection::Local,
            bind: bind.into(),
            target: target.into(),
        }
    }

    /// Creates a remote streamlocal forwarding specification.
    pub fn remote_streamlocal(
        bind: impl Into<StreamLocalSpec>,
        target: impl Into<StreamLocalSpec>,
    ) -> Self {
        Self::StreamLocal {
            direction: ForwardDirection::Remote,
            bind: bind.into(),
            target: target.into(),
        }
    }
}
