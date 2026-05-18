//! Authentication domain types.

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

/// Username used for SSH authentication.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Username(String);

impl Username {
    /// Creates a username.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the username as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Username {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Username {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Password value with redacted debug output.
///
/// When the `serde` feature is enabled, this type does **not** implement
/// `Serialize` or `Deserialize` to prevent accidental credential leakage.
/// Use environment variables or secret managers instead of serializing
/// passwords directly.
#[non_exhaustive]
#[derive(Clone, Eq, PartialEq)]
pub struct Password(String);

impl Password {
    /// Creates a password.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Exposes the password to code that performs authentication.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(***)")
    }
}

impl From<&str> for Password {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Password {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// SSH identity material.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Eq, PartialEq)]
pub enum Identity {
    /// Use an SSH agent.
    #[cfg_attr(feature = "serde", serde(skip))]
    Agent,
    /// Use a private key from disk.
    #[cfg_attr(feature = "serde", serde(skip))]
    KeyFile {
        /// Path to the private key.
        path: PathBuf,
        /// Optional passphrase for the private key.
        passphrase: Option<Password>,
    },
    /// Use an in-memory private key.
    #[cfg_attr(feature = "serde", serde(skip))]
    PrivateKey {
        /// PEM or OpenSSH private key bytes.
        data: Vec<u8>,
        /// Optional passphrase for the private key.
        passphrase: Option<Password>,
    },
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent => f.write_str("Agent"),
            Self::KeyFile { path, passphrase } => f
                .debug_struct("KeyFile")
                .field("path", path)
                .field("passphrase", passphrase)
                .finish(),
            Self::PrivateKey { data, passphrase } => f
                .debug_struct("PrivateKey")
                .field("data", &format_args!("<{} bytes redacted>", data.len()))
                .field("passphrase", passphrase)
                .finish(),
        }
    }
}

impl Identity {
    /// Creates an agent identity.
    pub fn agent() -> Self {
        Self::Agent
    }

    /// Creates a key-file identity.
    pub fn key_file(path: impl Into<PathBuf>) -> Self {
        let path = expand_tilde(&path.into());
        Self::KeyFile {
            path,
            passphrase: None,
        }
    }

    /// Loads an OpenSSH private key from a file.
    ///
    /// On Unix, the file must not be accessible by group or others.
    /// The key bytes are stored in memory; actual key parsing happens
    /// during authentication.
    pub fn load_openssh_file(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let path = path.as_ref();
        let path = expand_tilde(path);
        validate_private_key_permissions(&path)?;
        let data = std::fs::read(&path)?;
        Ok(Self::PrivateKey {
            data,
            passphrase: None,
        })
    }

    /// Creates an identity from in-memory PEM or OpenSSH key bytes.
    pub fn load_openssh_pem(data: impl Into<Vec<u8>>) -> Self {
        Self::PrivateKey {
            data: data.into(),
            passphrase: None,
        }
    }

    /// Adds a passphrase to a key identity.
    pub fn with_passphrase(mut self, passphrase: impl Into<Password>) -> Self {
        match &mut self {
            Self::Agent => {}
            Self::KeyFile {
                passphrase: current,
                ..
            }
            | Self::PrivateKey {
                passphrase: current,
                ..
            } => *current = Some(passphrase.into()),
        }

        self
    }
}

/// Information about a keyboard-interactive authentication prompt.
#[non_exhaustive]
#[derive(Clone)]
pub struct ClientKeyboardInteractiveInfo {
    /// Human-readable name describing the authentication step.
    pub name: String,
    /// Instructions for the user (may be empty).
    pub instructions: String,
    prompts: Vec<ClientKeyboardInteractivePrompt>,
}

impl ClientKeyboardInteractiveInfo {
    /// Creates a new info request from the given fields.
    ///
    /// This constructor is intended for wrapping raw russh keyboard-interactive
    /// info requests.
    pub fn new(
        name: String,
        instructions: String,
        prompts: Vec<ClientKeyboardInteractivePrompt>,
    ) -> Self {
        Self {
            name,
            instructions,
            prompts,
        }
    }

    /// Returns the prompts in this keyboard-interactive challenge.
    pub fn prompts(&self) -> &[ClientKeyboardInteractivePrompt] {
        &self.prompts
    }
}

impl fmt::Debug for ClientKeyboardInteractiveInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientKeyboardInteractiveInfo")
            .field("name", &self.name)
            .field("instructions", &self.instructions)
            .field("prompts", &self.prompts)
            .finish()
    }
}

/// A single keyboard-interactive prompt.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct ClientKeyboardInteractivePrompt {
    /// Prompt text to display to the user.
    pub prompt: String,
    /// Whether the response should be echoed back.
    pub echo: bool,
}

impl ClientKeyboardInteractivePrompt {
    /// Creates a new keyboard-interactive prompt.
    pub fn new(prompt: String, echo: bool) -> Self {
        Self { prompt, echo }
    }
}

/// Reply to a keyboard-interactive authentication challenge.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum KeyboardInteractiveReply {
    /// Send these responses to the server.
    Responses(Vec<String>),
    /// Abort keyboard-interactive authentication (try next credential).
    Abort,
}

/// Handler for keyboard-interactive authentication challenges.
pub type KeyboardInteractiveHandler = Arc<
    dyn Fn(
            ClientKeyboardInteractiveInfo,
        ) -> Pin<Box<dyn Future<Output = KeyboardInteractiveReply> + Send>>
        + Send
        + Sync,
>;

/// Authentication credential.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone)]
pub enum Credential {
    /// Password authentication.
    #[cfg_attr(feature = "serde", serde(skip))]
    Password(Password),
    /// Public key authentication.
    #[cfg_attr(feature = "serde", serde(skip))]
    Identity(Identity),
    /// Attempt `none` authentication.
    None,
    /// Keyboard-interactive authentication with a challenge handler.
    #[cfg_attr(feature = "serde", serde(skip))]
    KeyboardInteractive(KeyboardInteractiveHandler),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Password(***)"),
            Self::Identity(identity) => identity.fmt(f),
            Self::None => f.write_str("None"),
            Self::KeyboardInteractive(_) => f.write_str("KeyboardInteractive"),
        }
    }
}

impl PartialEq for Credential {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Password(a), Self::Password(b)) => a == b,
            (Self::Identity(a), Self::Identity(b)) => a == b,
            (Self::None, Self::None) => true,
            (Self::KeyboardInteractive(_), Self::KeyboardInteractive(_)) => true,
            _ => false,
        }
    }
}

impl Eq for Credential {}

impl Credential {
    /// Creates a password credential.
    pub fn password(password: impl Into<Password>) -> Self {
        Self::Password(password.into())
    }

    /// Creates an identity credential.
    pub fn identity(identity: Identity) -> Self {
        Self::Identity(identity)
    }

    /// Creates a keyboard-interactive credential with the given handler.
    pub fn keyboard_interactive(
        handler: impl Fn(
            ClientKeyboardInteractiveInfo,
        ) -> Pin<Box<dyn Future<Output = KeyboardInteractiveReply> + Send>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::KeyboardInteractive(Arc::new(handler))
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
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

    path.to_path_buf()
}

fn validate_private_key_permissions(path: &Path) -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path).map_err(|source| {
            crate::Error::invalid_config(format!(
                "cannot access private key file `{}`: {source}",
                path.display()
            ))
        })?;
        let mode = metadata.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(crate::Error::invalid_config(format!(
                "private key file `{}` must not be accessible by group or others",
                path.display()
            )));
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Credential, Identity, KeyboardInteractiveReply, Password};

    #[test]
    fn password_debug_redacts_secret() {
        let password = Password::new("do-not-print");

        let debug = format!("{password:?}");

        assert!(debug.contains("***"));
        assert!(!debug.contains("do-not-print"));
    }

    #[test]
    fn private_key_debug_redacts_key_bytes_and_passphrase() {
        let identity = Identity::PrivateKey {
            data: b"private-key-material".to_vec(),
            passphrase: Some(Password::new("do-not-print")),
        };

        let debug = format!("{identity:?}");

        assert!(debug.contains("redacted"));
        assert!(!debug.contains("private-key-material"));
        assert!(!debug.contains("do-not-print"));
    }

    #[test]
    fn credential_debug_redacts_nested_secret() {
        let credential = Credential::password("do-not-print");

        let debug = format!("{credential:?}");

        assert!(!debug.contains("do-not-print"));
    }

    #[test]
    fn identity_load_openssh_file_permits_pem_round_trip() {
        let identity = Identity::load_openssh_pem(b"private key data");

        let Identity::PrivateKey { data, passphrase } = identity else {
            panic!("expected PrivateKey variant");
        };
        assert_eq!(data, b"private key data");
        assert!(passphrase.is_none());
    }

    #[test]
    fn identity_with_passphrase_sets_passphrase_on_key_variants() {
        let keyfile = Identity::key_file("id_rsa").with_passphrase("secret");
        let Identity::KeyFile { passphrase, .. } = keyfile else {
            panic!("expected KeyFile");
        };
        assert_eq!(passphrase.unwrap().expose_secret(), "secret");

        let privkey = Identity::load_openssh_pem(b"key").with_passphrase("secret");
        let Identity::PrivateKey { passphrase, .. } = privkey else {
            panic!("expected PrivateKey");
        };
        assert_eq!(passphrase.unwrap().expose_secret(), "secret");
    }

    #[test]
    fn keyboard_interactive_credentials_compare_equal() {
        let a = Credential::keyboard_interactive(|_info| {
            Box::pin(async { KeyboardInteractiveReply::Abort })
        });
        let b = Credential::keyboard_interactive(|_info| {
            Box::pin(async { KeyboardInteractiveReply::Abort })
        });
        assert_eq!(a, b);
        assert_eq!(b, a);
    }

    #[test]
    fn different_credential_kinds_not_equal() {
        let pw1 = Credential::password("hello");
        let pw2 = Credential::password("hello");
        assert_eq!(pw1, pw2);

        let ki = Credential::keyboard_interactive(|_info| {
            Box::pin(async { KeyboardInteractiveReply::Abort })
        });
        assert_ne!(pw1, ki);
        assert_ne!(Credential::None, ki);
    }

    #[test]
    fn identity_key_file_expands_tilde() {
        let id = Identity::key_file("~/nonexistent_key");
        let Identity::KeyFile { path, .. } = id else {
            panic!("expected KeyFile");
        };
        let path_str = path.to_string_lossy();
        assert!(!path_str.starts_with("~"), "tilde not expanded: {path_str}");
    }

    #[test]
    fn identity_key_file_no_tilde_passes_through() {
        let id = Identity::key_file("/absolute/path/to/key");
        let Identity::KeyFile { path, .. } = id else {
            panic!("expected KeyFile");
        };
        assert_eq!(path.to_string_lossy(), "/absolute/path/to/key");
    }
}
