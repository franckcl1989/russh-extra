//! SFTP domain types.
//!
//! Public types for file handles, directory entries, metadata,
//! and open modes used by the `SftpClient` and `SftpServerHandler` APIs.

use std::collections::VecDeque;
use std::fmt;

use crate::sftp::packet::SftpFileAttrs as PacketAttrs;

/// A remote file handle obtained from `SftpClient::open()`.
///
/// Closing the file is best-effort on drop: if the file is dropped
/// without calling `close()`, the close request is sent but the
/// response is not awaited.  Calling `close()` explicitly prevents
/// the drop-based close from firing.
pub struct SftpFile {
    handle: String,
    client: crate::sftp::client::SftpClientRuntime,
    closed: bool,
}

impl SftpFile {
    pub(crate) fn new(handle: String, client: crate::sftp::client::SftpClientRuntime) -> Self {
        Self {
            handle,
            client,
            closed: false,
        }
    }

    /// Returns the raw SFTP handle string.
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Reads up to `len` bytes starting at `offset`.
    ///
    /// Returns an empty `Vec<u8>` when the end of the file is reached.
    pub async fn read(&mut self, offset: u64, len: u32) -> crate::Result<Vec<u8>> {
        self.client.read(&self.handle, offset, len).await
    }

    /// Writes data starting at `offset`.
    pub async fn write(&self, offset: u64, data: &[u8]) -> crate::Result<()> {
        self.client.write(&self.handle, offset, data).await
    }

    /// Closes the remote file handle.
    ///
    /// After `close()` returns, the file handle is invalid and no
    /// further operations should be performed on it.  Calling `close()`
    /// explicitly prevents the best-effort drop-based close from firing.
    pub async fn close(mut self) -> crate::Result<()> {
        self.closed = true;
        self.client.close(&self.handle).await
    }

    /// Returns file metadata.
    pub async fn metadata(&self) -> crate::Result<SftpMetadata> {
        self.client.fstat(&self.handle).await
    }

    /// Sets file metadata.
    pub async fn set_metadata(&self, attrs: &SftpMetadata) -> crate::Result<()> {
        self.client.fsetstat(&self.handle, &attrs.to_packet()).await
    }
}

impl fmt::Debug for SftpFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpFile")
            .field("handle", &self.handle)
            .field("closed", &self.closed)
            .finish()
    }
}

impl Drop for SftpFile {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let client = self.client.clone();
        let handle = self.handle.clone();
        tokio::spawn(async move {
            let _ = client.close(&handle).await;
        });
    }
}

/// A remote directory handle from `SftpClient::opendir()`.
pub struct SftpDir {
    handle: String,
    client: crate::sftp::client::SftpClientRuntime,
    closed: bool,
    pending_entries: VecDeque<SftpDirEntry>,
}

impl SftpDir {
    pub(crate) fn new(handle: String, client: crate::sftp::client::SftpClientRuntime) -> Self {
        Self {
            handle,
            client,
            closed: false,
            pending_entries: VecDeque::new(),
        }
    }

    /// Returns the raw SFTP handle string.
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Reads the next directory entry.
    ///
    /// Returns `Ok(None)` at end of directory.
    pub async fn read(&mut self) -> crate::Result<Option<SftpDirEntry>> {
        if let Some(entry) = self.pending_entries.pop_front() {
            return Ok(Some(entry));
        }
        let batch = self.client.readdir_batch(&self.handle).await?;
        if batch.is_empty() {
            return Ok(None);
        }
        let mut entries = VecDeque::from(batch);
        let first = entries.pop_front();
        self.pending_entries = entries;
        Ok(first)
    }

    /// Closes the remote directory handle.
    ///
    /// After `close()` returns, the directory handle is invalid and no
    /// further operations should be performed on it.  Calling `close()`
    /// explicitly prevents the best-effort drop-based close from firing.
    pub async fn close(mut self) -> crate::Result<()> {
        self.closed = true;
        self.client.close(&self.handle).await
    }
}

impl fmt::Debug for SftpDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpDir")
            .field("handle", &self.handle)
            .field("closed", &self.closed)
            .finish()
    }
}

impl Drop for SftpDir {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let client = self.client.clone();
        let handle = self.handle.clone();
        tokio::spawn(async move {
            let _ = client.close(&handle).await;
        });
    }
}

/// A single directory entry.
#[derive(Clone, Debug)]
pub struct SftpDirEntry {
    filename: String,
    longname: String,
    metadata: SftpMetadata,
}

impl SftpDirEntry {
    pub(crate) fn from_packet(filename: String, longname: String, attrs: PacketAttrs) -> Self {
        Self {
            filename,
            longname,
            metadata: SftpMetadata::from_packet(attrs),
        }
    }

    /// Creates a new directory entry.
    pub fn new(
        filename: impl Into<String>,
        longname: impl Into<String>,
        metadata: SftpMetadata,
    ) -> Self {
        Self {
            filename: filename.into(),
            longname: longname.into(),
            metadata,
        }
    }

    /// Returns the filename.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Returns the long format name string.
    pub fn longname(&self) -> &str {
        &self.longname
    }

    /// Returns the file metadata.
    pub fn metadata(&self) -> &SftpMetadata {
        &self.metadata
    }

    #[cfg(feature = "server")]
    pub(crate) fn to_name_entry(&self) -> super::packet::SftpNameEntry {
        super::packet::SftpNameEntry {
            filename: self.filename.clone(),
            longname: self.longname.clone(),
            attrs: self.metadata.to_packet(),
        }
    }
}

/// File metadata returned by stat/lstat/fstat.
#[non_exhaustive]
#[derive(Clone, Debug, Default)]
pub struct SftpMetadata {
    size: Option<u64>,
    uid: Option<u32>,
    gid: Option<u32>,
    permissions: Option<u32>,
    accessed: Option<u64>,
    modified: Option<u64>,
}

impl SftpMetadata {
    /// Creates a new `SftpMetadata` with the given field values.
    ///
    /// All fields are optional; `None` means "not set" and will be
    /// excluded from wire-level attributes.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        size: Option<u64>,
        uid: Option<u32>,
        gid: Option<u32>,
        permissions: Option<u32>,
        accessed: Option<u64>,
        modified: Option<u64>,
    ) -> Self {
        Self {
            size,
            uid,
            gid,
            permissions,
            accessed,
            modified,
        }
    }

    pub(crate) fn from_packet(attrs: PacketAttrs) -> Self {
        Self {
            size: attrs.size,
            uid: attrs.uid,
            gid: attrs.gid,
            permissions: attrs.permissions,
            accessed: attrs.atime.map(|v| v as u64),
            modified: attrs.mtime.map(|v| v as u64),
        }
    }

    /// Creates `SftpFileAttrs` suitable for use with setstat/fsetstat.
    ///
    /// Only `Some` fields will be included in the attributes mask.
    pub(crate) fn to_packet(&self) -> PacketAttrs {
        let mut attrs = PacketAttrs::default();
        if let Some(size) = self.size {
            attrs.flags |= crate::sftp::packet::SSH_FILEXFER_ATTR_SIZE;
            attrs.size = Some(size);
        }
        if self.uid.is_some() || self.gid.is_some() {
            attrs.flags |= crate::sftp::packet::SSH_FILEXFER_ATTR_UIDGID;
            attrs.uid = self.uid;
            attrs.gid = self.gid;
        }
        if let Some(perm) = self.permissions {
            attrs.flags |= crate::sftp::packet::SSH_FILEXFER_ATTR_PERMISSIONS;
            attrs.permissions = Some(perm);
        }
        if let Some(atime) = self.accessed {
            attrs.flags |= crate::sftp::packet::SSH_FILEXFER_ATTR_ACMODTIME;
            attrs.atime = Some(atime as u32);
        }
        if let Some(mtime) = self.modified {
            attrs.flags |= crate::sftp::packet::SSH_FILEXFER_ATTR_ACMODTIME;
            attrs.mtime = Some(mtime as u32);
        }
        attrs
    }

    /// Returns the file size in bytes.
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /// Returns the owner UID.
    pub fn uid(&self) -> Option<u32> {
        self.uid
    }

    /// Returns the owner GID.
    pub fn gid(&self) -> Option<u32> {
        self.gid
    }

    /// Returns the Unix file permissions.
    pub fn permissions(&self) -> Option<u32> {
        self.permissions
    }

    /// Returns the last access time (Unix timestamp).
    pub fn accessed(&self) -> Option<u64> {
        self.accessed
    }

    /// Returns the last modification time (Unix timestamp).
    pub fn modified(&self) -> Option<u64> {
        self.modified
    }

    /// Sets the file size.
    #[must_use]
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    /// Sets the Unix file permissions.
    #[must_use]
    pub fn with_permissions(mut self, permissions: u32) -> Self {
        self.permissions = Some(permissions);
        self
    }

    /// Sets the UID and GID.
    #[must_use]
    pub fn with_uid_gid(mut self, uid: u32, gid: u32) -> Self {
        self.uid = Some(uid);
        self.gid = Some(gid);
        self
    }

    /// Sets the last access time (Unix timestamp).
    #[must_use]
    pub fn with_accessed(mut self, accessed: u64) -> Self {
        self.accessed = Some(accessed);
        self
    }

    /// Sets the last modification time (Unix timestamp).
    #[must_use]
    pub fn with_modified(mut self, modified: u64) -> Self {
        self.modified = Some(modified);
        self
    }
}

impl fmt::Display for SftpMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SftpMetadata {{ ")?;
        if let Some(size) = self.size {
            write!(f, "size={size}, ")?;
        }
        if let Some(perm) = self.permissions {
            write!(f, "perm={perm:o}, ")?;
        }
        if let Some(uid) = self.uid {
            write!(f, "uid={uid}, ")?;
        }
        if let Some(gid) = self.gid {
            write!(f, "gid={gid}, ")?;
        }
        write!(f, "}}")?;
        Ok(())
    }
}

/// Open mode flags for `SftpClient::open()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SftpOpenMode(u32);

impl SftpOpenMode {
    /// Open for reading.
    pub const READ: Self = Self(0x00000001);
    /// Open for writing.
    pub const WRITE: Self = Self(0x00000002);
    /// Force all writes to append data to the end of the file.
    pub const APPEND: Self = Self(0x00000004);
    /// Create the file if it does not exist.
    pub const CREATE: Self = Self(0x00000008);
    /// Truncate the file to zero length if it exists.
    pub const TRUNCATE: Self = Self(0x00000010);
    /// Fail if the file already exists (used with `CREATE`).
    pub const EXCLUSIVE: Self = Self(0x00000020);
}

impl SftpOpenMode {
    /// Returns the raw pflags value.
    pub fn bits(&self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for SftpOpenMode {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
