//! Client and server configuration.

use std::time::Duration;

use crate::{Credential, Endpoint, Error, HostKeyErrorKind, Identity, Result, Username};

/// Host-key verification policy.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum HostKeyPolicy {
    /// Reject host keys unless a future persistent store or verifier accepts
    /// them. This is the default.
    #[default]
    Strict,
    /// Accept every host key.
    ///
    /// **Insecure**: this disables host-key verification entirely. Only use
    /// this policy in tests or controlled environments.
    InsecureAcceptAny,
    /// Accept only pinned SHA256 host-key fingerprints.
    PinnedSha256(Vec<HostKeyFingerprint>),
}

impl HostKeyPolicy {
    /// Creates a pinned SHA256 host-key policy.
    pub fn pinned_sha256(fingerprint: impl Into<String>) -> Result<Self> {
        Ok(Self::PinnedSha256(vec![HostKeyFingerprint::sha256(
            fingerprint,
        )?]))
    }

    /// Returns whether this policy accepts any host key.
    pub fn accepts_any(&self) -> bool {
        matches!(self, Self::InsecureAcceptAny)
    }
}

/// Pinned host-key fingerprint.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HostKeyFingerprint {
    algorithm: HostKeyFingerprintAlgorithm,
    value: String,
}

impl HostKeyFingerprint {
    /// Creates a SHA256 host-key fingerprint.
    pub fn sha256(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_sha256_fingerprint(&value)?;
        Ok(Self {
            algorithm: HostKeyFingerprintAlgorithm::Sha256,
            value,
        })
    }

    /// Returns the fingerprint algorithm.
    pub fn algorithm(&self) -> HostKeyFingerprintAlgorithm {
        self.algorithm
    }

    /// Returns the fingerprint value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Host-key fingerprint algorithm.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HostKeyFingerprintAlgorithm {
    /// OpenSSH-style SHA256 host-key fingerprint.
    Sha256,
}

/// Client connection configuration.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientConfig {
    endpoint: Endpoint,
    username: Option<Username>,
    #[cfg_attr(feature = "serde", serde(skip))]
    credentials: Vec<Credential>,
    timeouts: Timeouts,
    keepalive: Keepalive,
    host_key_policy: HostKeyPolicy,
}

impl ClientConfig {
    /// Creates a config for the given endpoint.
    pub fn new(endpoint: impl Into<Endpoint>) -> Self {
        Self {
            endpoint: endpoint.into(),
            username: None,
            credentials: Vec::new(),
            timeouts: Timeouts::default(),
            keepalive: Keepalive::default(),
            host_key_policy: HostKeyPolicy::default(),
        }
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Sets the endpoint.
    pub fn set_endpoint(&mut self, endpoint: impl Into<Endpoint>) {
        self.endpoint = endpoint.into();
    }

    /// Returns the optional username.
    pub fn username(&self) -> Option<&Username> {
        self.username.as_ref()
    }

    /// Sets the username.
    pub fn set_username(&mut self, username: impl Into<Username>) {
        self.username = Some(username.into());
    }

    /// Returns configured credentials in preference order.
    pub fn credentials(&self) -> &[Credential] {
        &self.credentials
    }

    /// Adds a credential.
    pub fn add_credential(&mut self, credential: Credential) {
        self.credentials.push(credential);
    }

    /// Adds an SSH agent credential.
    pub fn use_agent(&mut self) {
        self.add_credential(Credential::identity(Identity::agent()));
    }

    /// Returns timeout settings.
    pub fn timeouts(&self) -> &Timeouts {
        &self.timeouts
    }

    /// Sets timeout settings.
    pub fn set_timeouts(&mut self, timeouts: Timeouts) {
        self.timeouts = timeouts;
    }

    /// Returns keepalive settings.
    pub fn keepalive(&self) -> &Keepalive {
        &self.keepalive
    }

    /// Sets keepalive settings.
    pub fn set_keepalive(&mut self, keepalive: Keepalive) {
        self.keepalive = keepalive;
    }

    /// Returns whether strict host key checking is enabled.
    pub fn strict_host_key_checking(&self) -> bool {
        !self.host_key_policy.accepts_any()
    }

    /// Sets strict host key checking.
    #[deprecated = "use set_host_key_policy instead"]
    pub fn set_strict_host_key_checking(&mut self, enabled: bool) {
        self.host_key_policy = if enabled {
            HostKeyPolicy::Strict
        } else {
            HostKeyPolicy::InsecureAcceptAny
        };
    }

    /// Returns the configured host-key policy.
    pub fn host_key_policy(&self) -> &HostKeyPolicy {
        &self.host_key_policy
    }

    /// Sets the host-key policy.
    pub fn set_host_key_policy(&mut self, policy: HostKeyPolicy) {
        self.host_key_policy = policy;
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::new(Endpoint::default())
    }
}

/// Server configuration.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    listen: Endpoint,
    server_id: String,
    max_sessions: usize,
}

impl ServerConfig {
    /// Creates server configuration for a listen endpoint.
    pub fn new(listen: impl Into<Endpoint>) -> Self {
        Self {
            listen: listen.into(),
            server_id: "SSH-2.0-russh-extra".to_owned(),
            max_sessions: 1024,
        }
    }

    /// Returns the listen endpoint.
    pub fn listen(&self) -> &Endpoint {
        &self.listen
    }

    /// Sets the listen endpoint.
    pub fn set_listen(&mut self, listen: impl Into<Endpoint>) {
        self.listen = listen.into();
    }

    /// Returns the SSH identification string.
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Sets the SSH identification string.
    pub fn set_server_id(&mut self, server_id: impl Into<String>) {
        self.server_id = server_id.into();
    }

    /// Returns the configured maximum session count.
    pub fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// Sets the maximum session count.
    pub fn set_max_sessions(&mut self, max_sessions: usize) {
        self.max_sessions = max_sessions;
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new(("127.0.0.1", 0))
    }
}

/// Timeout configuration.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeouts {
    /// Timeout for establishing a TCP connection.
    pub connect: Duration,
    /// Timeout for completing authentication.
    pub auth: Duration,
    /// Timeout for opening a channel.
    pub channel_open: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(30),
            auth: Duration::from_secs(30),
            channel_open: Duration::from_secs(10),
        }
    }
}

impl Timeouts {
    /// Creates new timeout configuration.
    pub fn new(connect: Duration, auth: Duration, channel_open: Duration) -> Self {
        Self {
            connect,
            auth,
            channel_open,
        }
    }
}

/// Keepalive configuration.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Keepalive {
    /// Whether keepalives are enabled.
    pub enabled: bool,
    /// Interval between keepalive messages.
    pub interval: Duration,
    /// Number of unanswered keepalives before disconnecting.
    pub max_missed: u32,
}

impl Default for Keepalive {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Duration::from_secs(30),
            max_missed: 3,
        }
    }
}

fn validate_sha256_fingerprint(value: &str) -> Result<()> {
    let Some(rest) = value.strip_prefix("SHA256:") else {
        return Err(Error::host_key(
            HostKeyErrorKind::Unsupported,
            "host-key fingerprint must start with SHA256:",
        ));
    };

    if rest.is_empty() {
        return Err(Error::host_key(
            HostKeyErrorKind::Unavailable,
            "host-key fingerprint must not be empty",
        ));
    }

    if rest.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(Error::host_key(
            HostKeyErrorKind::Rejected,
            "host-key fingerprint must not contain whitespace",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        ClientConfig, Endpoint, Error, HostKeyFingerprint, HostKeyFingerprintAlgorithm,
        HostKeyPolicy,
    };

    #[test]
    fn server_config_defaults_to_loopback_ephemeral_port() {
        let config = crate::ServerConfig::default();

        assert_eq!(config.listen(), &Endpoint::new("127.0.0.1", 0));
    }

    #[test]
    fn client_config_defaults_to_strict_host_key_policy() {
        let config = ClientConfig::default();

        assert_eq!(config.host_key_policy(), &HostKeyPolicy::Strict);
        assert!(config.strict_host_key_checking());
    }

    #[test]
    #[allow(deprecated)]
    fn disabling_strict_host_key_checking_sets_accept_any_policy() {
        let mut config = ClientConfig::default();

        config.set_strict_host_key_checking(false);

        assert_eq!(config.host_key_policy(), &HostKeyPolicy::InsecureAcceptAny);
        assert!(!config.strict_host_key_checking());
    }

    #[test]
    fn validates_sha256_host_key_fingerprints() {
        let fingerprint = HostKeyFingerprint::sha256("SHA256:abc123+/=").unwrap();

        assert_eq!(fingerprint.algorithm(), HostKeyFingerprintAlgorithm::Sha256);
        assert_eq!(fingerprint.value(), "SHA256:abc123+/=");
    }

    #[test]
    fn rejects_invalid_sha256_host_key_fingerprints() {
        let error = HostKeyFingerprint::sha256("MD5:abc").unwrap_err();
        assert!(matches!(error, Error::HostKey(_)));

        let error = HostKeyFingerprint::sha256("SHA256:").unwrap_err();
        assert!(matches!(error, Error::HostKey(_)));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn client_config_serialization_skips_credentials() {
        let mut config = ClientConfig::new(Endpoint::new("example.com", 2222));
        config.add_credential(crate::Credential::password("secret"));

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: ClientConfig = serde_json::from_str(&serialized).unwrap();

        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("credentials"));
        assert!(deserialized.credentials().is_empty());
    }

    #[test]
    fn client_config_debug_does_not_expose_credential_content() {
        let mut config = ClientConfig::new(Endpoint::new("example.com", 2222));
        config.add_credential(crate::Credential::password("my-secret-password"));
        let debug = format!("{:?}", config);
        assert!(!debug.contains("my-secret-password"));
        assert!(debug.contains("Password(***)"));
    }
}
