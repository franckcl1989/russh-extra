//! Port forwarding domain types.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Direction of an SSH forwarding request.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ForwardDirection {
    /// Local forwarding: local listener to remote target.
    Local,
    /// Remote forwarding: remote listener to local target.
    Remote,
}

/// TCP endpoint used by forwarding.
#[non_exhaustive]
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

impl fmt::Display for TcpEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

impl FromStr for TcpEndpoint {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix('[') {
            let (host, port_str) = rest.split_once("]:").ok_or_else(|| {
                crate::Error::invalid_config(format!("invalid TCP endpoint: {s}"))
            })?;
            let port: u16 = port_str.parse().map_err(|_| {
                crate::Error::invalid_config(format!("invalid TCP endpoint port: {s}"))
            })?;
            Ok(Self::new(host, port))
        } else {
            let (host, port_str) = s.rsplit_once(':').ok_or_else(|| {
                crate::Error::invalid_config(format!("invalid TCP endpoint: {s}"))
            })?;
            let port: u16 = port_str.parse().map_err(|_| {
                crate::Error::invalid_config(format!("invalid TCP endpoint port: {s}"))
            })?;
            Ok(Self::new(host, port))
        }
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
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StreamLocalSpec {
    path: PathBuf,
}

impl StreamLocalSpec {
    /// Creates a streamlocal endpoint.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let path = expand_tilde_path(path);
        Self { path }
    }

    /// Returns the streamlocal path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl fmt::Display for StreamLocalSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

impl FromStr for StreamLocalSpec {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
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
        let path = expand_tilde_path(path);
        Self { path }
    }
}

/// High-level forwarding specification.
#[non_exhaustive]
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

fn expand_tilde_path(path: PathBuf) -> PathBuf {
    if let Some(path_str) = path.to_str()
        && (path_str == "~" || path_str.starts_with("~/"))
    {
        #[cfg(target_os = "windows")]
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
        #[cfg(not(target_os = "windows"))]
        let home = std::env::var("HOME");

        if let Ok(home) = home {
            if path_str == "~" {
                return PathBuf::from(home);
            }
            return PathBuf::from(home).join(&path_str[2..]);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_endpoint_display_ipv4() {
        let ep = TcpEndpoint::new("192.168.1.1", 22);
        assert_eq!(ep.to_string(), "192.168.1.1:22");
    }

    #[test]
    fn tcp_endpoint_display_ipv6() {
        let ep = TcpEndpoint::new("2001:db8::1", 22);
        assert_eq!(ep.to_string(), "[2001:db8::1]:22");
    }

    #[test]
    fn tcp_endpoint_display_hostname() {
        let ep = TcpEndpoint::new("example.com", 8080);
        assert_eq!(ep.to_string(), "example.com:8080");
    }

    #[test]
    fn tcp_endpoint_from_str_ipv4() {
        let ep: TcpEndpoint = "10.0.0.1:2222".parse().unwrap();
        assert_eq!(ep.host(), "10.0.0.1");
        assert_eq!(ep.port(), 2222);
    }

    #[test]
    fn tcp_endpoint_from_str_ipv6() {
        let ep: TcpEndpoint = "[::1]:2200".parse().unwrap();
        assert_eq!(ep.host(), "::1");
        assert_eq!(ep.port(), 2200);
    }

    #[test]
    fn tcp_endpoint_from_str_hostname() {
        let ep: TcpEndpoint = "db.internal:5432".parse().unwrap();
        assert_eq!(ep.host(), "db.internal");
        assert_eq!(ep.port(), 5432);
    }

    #[test]
    fn tcp_endpoint_display_round_trip_ipv4() {
        let original = "127.0.0.1:8022";
        let ep: TcpEndpoint = original.parse().unwrap();
        assert_eq!(ep.to_string(), original);
    }

    #[test]
    fn tcp_endpoint_display_round_trip_ipv6() {
        let original = "[2001:db8::1]:22";
        let ep: TcpEndpoint = original.parse().unwrap();
        assert_eq!(ep.to_string(), original);
    }

    #[test]
    fn tcp_endpoint_from_str_invalid_missing_port() {
        let result: Result<TcpEndpoint, _> = "host".parse();
        assert!(result.is_err());
    }

    #[test]
    fn tcp_endpoint_from_str_invalid_bad_port() {
        let result: Result<TcpEndpoint, _> = "host:abc".parse();
        assert!(result.is_err());
    }

    #[test]
    fn streamlocal_spec_display() {
        let spec = StreamLocalSpec::new("/tmp/app.sock");
        assert_eq!(spec.to_string(), "/tmp/app.sock");
    }

    #[test]
    fn streamlocal_spec_from_str() {
        let spec: StreamLocalSpec = "/var/run/service.sock".parse().unwrap();
        assert_eq!(spec.path(), std::path::Path::new("/var/run/service.sock"));
    }

    #[test]
    fn streamlocal_spec_display_round_trip() {
        let path = "/tmp/my-app.sock";
        let spec: StreamLocalSpec = path.parse().unwrap();
        assert_eq!(spec.to_string(), path);
    }

    #[test]
    fn streamlocal_spec_tilde_expansion() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return; // skip if HOME not set
        }
        let spec = StreamLocalSpec::new("~/myapp/agent.sock");
        let expected = format!("{}/myapp/agent.sock", home);
        assert_eq!(spec.to_string(), expected);
    }
}
