//! Endpoint and session identifiers.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, Result};

/// The default TCP port for SSH.
pub const DEFAULT_SSH_PORT: u16 = 22;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Remote SSH endpoint.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Endpoint {
    host: String,
    port: u16,
}

impl Endpoint {
    /// Creates an endpoint from a host and port.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// Creates an endpoint using the default SSH port.
    pub fn ssh(host: impl Into<String>) -> Self {
        Self::new(host, DEFAULT_SSH_PORT)
    }

    /// Parses an endpoint from `host`, `host:port`, `[ipv6]`, or `[ipv6]:port`.
    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }

    /// Returns the host name or IP address.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the TCP port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns `host:port`.
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl Default for Endpoint {
    fn default() -> Self {
        Self::ssh("localhost")
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl From<(&str, u16)> for Endpoint {
    fn from((host, port): (&str, u16)) -> Self {
        Self::new(host, port)
    }
}

impl From<(String, u16)> for Endpoint {
    fn from((host, port): (String, u16)) -> Self {
        Self::new(host, port)
    }
}

impl FromStr for Endpoint {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(Error::invalid_config("endpoint host cannot be empty"));
        }

        if let Some(rest) = value.strip_prefix('[') {
            let Some((host, suffix)) = rest.split_once(']') else {
                return Err(Error::invalid_config(
                    "bracketed IPv6 endpoint must close with ']'",
                ));
            };

            if host.is_empty() {
                return Err(Error::invalid_config("endpoint host cannot be empty"));
            }

            return match suffix.strip_prefix(':') {
                Some(port) => Ok(Self::new(host, parse_port(port)?)),
                None if suffix.is_empty() => Ok(Self::ssh(host)),
                None => Err(Error::invalid_config(
                    "bracketed IPv6 endpoint must be '[host]' or '[host]:port'",
                )),
            };
        }

        let colon_count = value.bytes().filter(|byte| *byte == b':').count();
        if colon_count == 0 || colon_count > 1 {
            return Ok(Self::ssh(value));
        }

        let Some((host, port)) = value.rsplit_once(':') else {
            unreachable!("colon_count guarantees a separator")
        };

        if host.is_empty() {
            return Err(Error::invalid_config("endpoint host cannot be empty"));
        }

        Ok(Self::new(host, parse_port(port)?))
    }
}

fn parse_port(value: &str) -> Result<u16> {
    if value.is_empty() {
        return Err(Error::invalid_config("endpoint port cannot be empty"));
    }

    value
        .parse()
        .map_err(|_| Error::invalid_config("endpoint port must be a valid u16"))
}

/// Opaque identifier assigned to high-level sessions.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(u64);

impl SessionId {
    /// Allocates a new session identifier.
    pub fn next() -> Self {
        Self(NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the numeric session identifier.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SSH_PORT, Endpoint};

    #[test]
    fn parses_host_without_port_using_default_ssh_port() {
        let endpoint = Endpoint::parse("example.com").unwrap();

        assert_eq!(endpoint.host(), "example.com");
        assert_eq!(endpoint.port(), DEFAULT_SSH_PORT);
    }

    #[test]
    fn parses_host_with_port() {
        let endpoint = Endpoint::parse("example.com:2222").unwrap();

        assert_eq!(endpoint.host(), "example.com");
        assert_eq!(endpoint.port(), 2222);
    }

    #[test]
    fn parses_bracketed_ipv6_with_port() {
        let endpoint = Endpoint::parse("[2001:db8::1]:2222").unwrap();

        assert_eq!(endpoint.host(), "2001:db8::1");
        assert_eq!(endpoint.port(), 2222);
    }

    #[test]
    fn parses_unbracketed_ipv6_without_port() {
        let endpoint = Endpoint::parse("2001:db8::1").unwrap();

        assert_eq!(endpoint.host(), "2001:db8::1");
        assert_eq!(endpoint.port(), DEFAULT_SSH_PORT);
    }

    #[test]
    fn rejects_invalid_endpoint_ports() {
        let error = Endpoint::parse("example.com:not-a-port").unwrap_err();

        assert!(error.to_string().contains("valid u16"));
    }

    #[test]
    fn rejects_empty_endpoint_hosts() {
        let error = Endpoint::parse(":22").unwrap_err();

        assert!(error.to_string().contains("host cannot be empty"));
    }
}
