//! SFTP abstractions.
//!
//! SFTP is reserved for a native implementation over `russh` session channels
//! and the `sftp` subsystem. The current public type is a placeholder that
//! returns [`Error::Unsupported`] until the packet layer and runtime are
//! implemented.

use russh_extra_core::{Error, Result, SessionId};

/// High-level SFTP client handle.
#[derive(Clone, Debug)]
pub struct SftpClient {
    session_id: SessionId,
}

impl SftpClient {
    /// Creates an SFTP client handle.
    #[cfg(feature = "client")]
    pub(crate) fn from_session(session_id: SessionId) -> Self {
        Self { session_id }
    }

    /// Returns the owning session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Opens the SFTP subsystem.
    pub async fn open(self) -> Result<Self> {
        Err(Error::unsupported("SFTP client is not implemented yet"))
    }
}

/// High-level SFTP server handle.
#[derive(Clone, Debug)]
pub struct SftpServer;

impl SftpServer {
    /// Creates an SFTP server handle.
    pub fn new() -> Self {
        Self
    }
}

impl Default for SftpServer {
    fn default() -> Self {
        Self::new()
    }
}
