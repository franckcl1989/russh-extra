//! Native SFTP protocol implementation on top of `russh` subsystem channels.
//!
//! The `sftp` feature exposes a full SFTP v3 client with file, directory,
//! metadata, and symlink operations. Packet encoding, request pipelining,
//! and response decoding are handled in this module.
//!
//! The `SftpServerHandler` trait (requires both `sftp` and `server` features)
//! provides a high-level interface for implementing server-side SFTP handlers.

mod client;
mod packet;
#[cfg(feature = "server")]
pub(crate) mod server;

const SFTP_CHUNK_SIZE: u32 = 32768;
mod types;

pub(crate) use client::SftpClientRuntime;
use russh_extra_core::{Error, Result, SessionId};

#[cfg(feature = "server")]
pub use server::SftpServerHandler;
pub use types::{SftpDir, SftpDirEntry, SftpFile, SftpMetadata, SftpOpenMode};

/// High-level SFTP client handle.
///
/// Created by [`Session::sftp`](super::Session::sftp).  The client
/// negotiates SFTP version 3 over an SSH subsystem channel and
/// exposes file, directory, metadata, and symlink operations.
#[derive(Clone)]
pub struct SftpClient {
    runtime: Option<SftpClientRuntime>,
    session_id: SessionId,
}

impl std::fmt::Debug for SftpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpClient")
            .field("session_id", &self.session_id)
            .field("open", &self.runtime.is_some())
            .finish()
    }
}

impl SftpClient {
    /// Creates an SFTP client handle (pre-open placeholder).
    #[cfg(feature = "client")]
    pub(crate) fn from_session(session_id: SessionId) -> Self {
        Self {
            runtime: None,
            session_id,
        }
    }

    /// Returns the owning session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Opens the SFTP subsystem on the session.
    #[cfg(feature = "client")]
    pub(crate) async fn connect(
        mut self,
        handle: std::sync::Arc<
            tokio::sync::Mutex<russh::client::Handle<super::client::ClientHandler>>,
        >,
    ) -> Result<Self> {
        let runtime = SftpClientRuntime::connect(self.session_id, handle).await?;
        self.runtime = Some(runtime);
        Ok(self)
    }

    fn runtime(&self) -> Result<&SftpClientRuntime> {
        self.runtime
            .as_ref()
            .ok_or_else(|| Error::unsupported("SFTP client is not open"))
    }

    /// Opens a remote file.
    #[tracing::instrument(skip(self, filename), fields(filename = filename))]
    pub async fn open(&self, filename: &str, mode: SftpOpenMode) -> Result<SftpFile> {
        self.runtime()?.open(filename, mode.bits()).await
    }

    /// Reads bytes from an open file.
    pub async fn read(&self, file: &mut SftpFile, offset: u64, len: u32) -> Result<Vec<u8>> {
        self.runtime()?.read(file.handle(), offset, len).await
    }

    /// Writes bytes to an open file.
    pub async fn write(&self, file: &SftpFile, offset: u64, data: &[u8]) -> Result<()> {
        self.runtime()?.write(file.handle(), offset, data).await
    }

    /// Closes an open file.
    pub async fn close_file(&self, file: SftpFile) -> Result<()> {
        self.runtime()?.close(file.handle()).await
    }

    /// Retrieves file metadata (stat).
    pub async fn metadata(&self, path: &str) -> Result<SftpMetadata> {
        self.runtime()?.stat(path).await
    }

    /// Retrieves file metadata without following symlinks (lstat).
    pub async fn symlink_metadata(&self, path: &str) -> Result<SftpMetadata> {
        self.runtime()?.lstat(path).await
    }

    /// Opens a directory for listing.
    pub async fn opendir(&self, path: &str) -> Result<SftpDir> {
        self.runtime()?.opendir(path).await
    }

    /// Reads the next directory entry from a directory handle.
    pub async fn readdir(&self, dir: &mut SftpDir) -> Result<Option<SftpDirEntry>> {
        dir.read().await
    }

    /// Closes a directory handle.
    pub async fn closedir(&self, dir: SftpDir) -> Result<()> {
        self.runtime()?.close(dir.handle()).await
    }

    /// Removes a file.
    pub async fn remove(&self, filename: &str) -> Result<()> {
        self.runtime()?.remove(filename).await
    }

    /// Renames a file or directory.
    pub async fn rename(&self, oldpath: &str, newpath: &str) -> Result<()> {
        self.runtime()?.rename(oldpath, newpath).await
    }

    /// Creates a directory.
    pub async fn create_dir(&self, path: &str) -> Result<()> {
        self.runtime()?.mkdir(path).await
    }

    /// Removes a directory.
    pub async fn remove_dir(&self, path: &str) -> Result<()> {
        self.runtime()?.rmdir(path).await
    }

    /// Resolves a path to its canonical absolute path.
    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        self.runtime()?.realpath(path).await
    }

    /// Creates a symbolic link.
    pub async fn symlink(&self, linkpath: &str, targetpath: &str) -> Result<()> {
        self.runtime()?.symlink(linkpath, targetpath).await
    }

    /// Reads the target of a symbolic link.
    pub async fn readlink(&self, path: &str) -> Result<String> {
        self.runtime()?.readlink(path).await
    }

    /// Reads an entire remote file into a byte vector.
    ///
    /// Handles chunked reading internally. Not suitable for files
    /// larger than available memory.
    pub async fn read_to_vec(&self, path: &str) -> Result<Vec<u8>> {
        let file = self.open(path, SftpOpenMode::READ).await?;
        let meta = file.metadata().await?;
        let size = meta.size().unwrap_or(0) as usize;
        let mut buf = Vec::with_capacity(size);
        let mut offset: u64 = 0;
        loop {
            let chunk = file.read(offset, SFTP_CHUNK_SIZE).await?;
            if chunk.is_empty() {
                break;
            }
            offset += chunk.len() as u64;
            buf.extend_from_slice(&chunk);
        }
        if let Err(e) = file.close().await {
            tracing::warn!(error = %e, "failed to close SFTP file after read_to_vec");
        }
        Ok(buf)
    }

    /// Writes an entire byte slice to a remote file.
    ///
    /// Creates or truncates the remote file. Handles chunked writing
    /// internally.
    pub async fn write_all(&self, path: &str, data: &[u8]) -> Result<()> {
        let file = self
            .open(
                path,
                SftpOpenMode::WRITE | SftpOpenMode::CREATE | SftpOpenMode::TRUNCATE,
            )
            .await?;
        let mut offset: u64 = 0;
        for chunk in data.chunks(SFTP_CHUNK_SIZE as usize) {
            file.write(offset, chunk).await?;
            offset += chunk.len() as u64;
        }
        file.close().await
    }

    /// Sets file metadata by path.
    ///
    /// Only `Some` fields in the metadata value are applied; `None`
    /// fields are left unchanged.
    pub async fn set_stat(&self, path: &str, attrs: &SftpMetadata) -> Result<()> {
        self.runtime()?.setstat(path, &attrs.to_packet()).await
    }

    /// Sets metadata on an open file handle.
    ///
    /// Only `Some` fields in the metadata value are applied; `None`
    /// fields are left unchanged.
    pub async fn fset_stat(&self, file: &SftpFile, attrs: &SftpMetadata) -> Result<()> {
        self.runtime()?
            .fsetstat(file.handle(), &attrs.to_packet())
            .await
    }
}
