//! SFTP server runtime.
//!
//! Implements the server side of the SFTP protocol: receives SFTP
//! packets over a subsystem channel, dispatches decoded requests to
//! a [`SftpServerHandler`], and encodes responses back to the wire.

use std::collections::HashMap;

use russh::{ChannelId, server};
use russh_extra_core::{Error, Result, SftpErrorKind};

use super::packet;
use super::types::SftpDirEntry;
use crate::Result as ExtraResult;

/// Handler trait for server-side SFTP operations.
///
/// Implement this trait to serve files, directories, and metadata
/// over the SFTP subsystem.  Every method receives the client's
/// request id so hand-written handlers can use it for logging.
///
/// All methods have default implementations that return a typed
/// "unsupported" error.
#[async_trait::async_trait]
pub trait SftpServerHandler: Send + Sync + 'static {
    /// Called with the negotiated version after `FXP_INIT`.
    ///
    /// Return the version the server supports (default 3).  The
    /// runtime already enforces a minimum of version 3.
    async fn init(&self, _version: u32, _extensions: HashMap<String, String>) -> ExtraResult<u32> {
        Ok(3)
    }

    /// Open a file.
    ///
    /// `pflags` contains the access mode and creation flags
    /// (`SSH_FXF_READ`, `SSH_FXF_WRITE`, etc.).
    async fn open(
        &self,
        _id: u32,
        _filename: String,
        _pflags: u32,
        _attrs: crate::SftpMetadata,
    ) -> ExtraResult<String> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "open not implemented",
        ))
    }

    /// Close a file or directory handle.
    async fn close(&self, _id: u32, _handle: String) -> ExtraResult<()> {
        Ok(())
    }

    /// Read bytes from an open file.
    async fn read(
        &self,
        _id: u32,
        _handle: String,
        _offset: u64,
        _len: u32,
    ) -> ExtraResult<Vec<u8>> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "read not implemented",
        ))
    }

    /// Write bytes to an open file.
    async fn write(
        &self,
        _id: u32,
        _handle: String,
        _offset: u64,
        _data: Vec<u8>,
    ) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "write not implemented",
        ))
    }

    /// Remove a file.
    async fn remove(&self, _id: u32, _filename: String) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "remove not implemented",
        ))
    }

    /// Rename a file or directory.
    async fn rename(&self, _id: u32, _oldpath: String, _newpath: String) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "rename not implemented",
        ))
    }

    /// Create a directory.
    async fn mkdir(&self, _id: u32, _path: String, _attrs: crate::SftpMetadata) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "mkdir not implemented",
        ))
    }

    /// Remove a directory.
    async fn rmdir(&self, _id: u32, _path: String) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "rmdir not implemented",
        ))
    }

    /// Open a directory for listing.
    async fn opendir(&self, _id: u32, _path: String) -> ExtraResult<String> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "opendir not implemented",
        ))
    }

    /// Read the next directory entries.
    ///
    /// Returns one or more entries.  An empty `Vec` signals end of
    /// directory (the runtime translates this to `SSH_FX_EOF`).
    async fn readdir(&self, _id: u32, _handle: String) -> ExtraResult<Vec<SftpDirEntry>> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "readdir not implemented",
        ))
    }

    /// Stat a file by path.
    async fn stat(&self, _id: u32, _path: String) -> ExtraResult<crate::SftpMetadata> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "stat not implemented",
        ))
    }

    /// Lstat a file by path (no symlink following).
    async fn lstat(&self, _id: u32, _path: String) -> ExtraResult<crate::SftpMetadata> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "lstat not implemented",
        ))
    }

    /// Fstat an open file handle.
    async fn fstat(&self, _id: u32, _handle: String) -> ExtraResult<crate::SftpMetadata> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "fstat not implemented",
        ))
    }

    /// Set file attributes by path.
    async fn setstat(
        &self,
        _id: u32,
        _path: String,
        _attrs: crate::SftpMetadata,
    ) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "setstat not implemented",
        ))
    }

    /// Set attributes on an open file handle.
    async fn fsetstat(
        &self,
        _id: u32,
        _handle: String,
        _attrs: crate::SftpMetadata,
    ) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "fsetstat not implemented",
        ))
    }

    /// Resolve a path to its canonical absolute form.
    async fn realpath(&self, _id: u32, _path: String) -> ExtraResult<Vec<SftpDirEntry>> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "realpath not implemented",
        ))
    }

    /// Read the target of a symbolic link.
    async fn readlink(&self, _id: u32, _path: String) -> ExtraResult<Vec<SftpDirEntry>> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "readlink not implemented",
        ))
    }

    /// Create a symbolic link.
    async fn symlink(&self, _id: u32, _linkpath: String, _targetpath: String) -> ExtraResult<()> {
        Err(Error::sftp(
            SftpErrorKind::UnsupportedVersion,
            "symlink not implemented",
        ))
    }
}

// ── Server runtime ───────────────────────────────────────────────────

/// Per-connection SFTP server state.
///
/// Buffers incoming data, reassembles complete SFTP packets, and
/// dispatches requests to the configured handler.
pub(crate) struct SftpServerRuntime {
    handler: std::sync::Arc<dyn SftpServerHandler + Send + Sync>,
    /// Per-channel reassembly buffer: `channel_id → Vec<u8>`.
    buffers: HashMap<ChannelId, Vec<u8>>,
    /// Tracked SFTP channels.
    channels: HashMap<ChannelId, ()>,
    /// Server version (negotiated during init).
    negotiated: bool,
    version: u32,
}

impl SftpServerRuntime {
    pub fn new(handler: std::sync::Arc<dyn SftpServerHandler + Send + Sync>) -> Self {
        Self {
            handler,
            buffers: HashMap::new(),
            channels: HashMap::new(),
            negotiated: false,
            version: 3,
        }
    }

    /// Register a newly-opened SFTP subsystem channel.
    pub fn register_channel(&mut self, channel: ChannelId) {
        self.channels.insert(channel, ());
        self.buffers.insert(channel, Vec::new());
    }

    /// Returns `true` if the channel is an SFTP channel.
    pub fn is_sftp_channel(&self, channel: ChannelId) -> bool {
        self.channels.contains_key(&channel)
    }

    /// Process an incoming data chunk on the SFTP subsystem channel.
    ///
    /// Reassembles complete SFTP packets, dispatches them to the
    /// configured handler, and writes responses to `session`.
    pub async fn handle_data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        let mut packets = Vec::new();
        {
            let buf = self.buffers.get_mut(&channel).ok_or_else(|| {
                Error::sftp(SftpErrorKind::ChannelIo, "sftp channel not registered")
            })?;

            buf.extend_from_slice(data);

            loop {
                if buf.len() < 4 {
                    break;
                }
                let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                if len > 256 * 1024 || len == 0 {
                    tracing::warn!(len, "invalid SFTP packet length, closing sftp channel");
                    self.channels.remove(&channel);
                    self.buffers.remove(&channel);
                    return Ok(());
                }
                if buf.len() < 4 + len {
                    break;
                }
                let packet = buf[4..4 + len].to_vec();
                buf.drain(..4 + len);
                packets.push(packet);
            }
        }

        for packet in packets {
            let response = self.dispatch_packet(&packet).await;
            if let Err(e) = &response {
                tracing::debug!(?e, "sftp request failed");
            }

            if let Ok(resp) = response
                && let Err(e) = session.data(channel, resp.to_vec())
            {
                tracing::warn!(?e, "failed to write sftp response");
                return Err(Error::sftp(
                    SftpErrorKind::ChannelIo,
                    "failed to write sftp response",
                ));
            }
        }

        Ok(())
    }

    /// Handle channel close: clean up per-channel buffers.
    pub fn handle_channel_close(&mut self, channel: ChannelId) {
        self.channels.remove(&channel);
        self.buffers.remove(&channel);
    }

    /// Decode a single SFTP packet and dispatch to the handler.
    async fn dispatch_packet(&mut self, packet: &[u8]) -> Result<Vec<u8>> {
        if packet.is_empty() {
            return Err(Error::sftp(SftpErrorKind::Protocol, "empty sftp packet"));
        }

        match packet[0] {
            packet::FXP_INIT => self.handle_init(&packet[1..]).await,
            packet::FXP_OPEN => self.handle_open(&packet[1..]).await,
            packet::FXP_CLOSE => self.handle_close(&packet[1..]).await,
            packet::FXP_READ => self.handle_read(&packet[1..]).await,
            packet::FXP_WRITE => self.handle_write(&packet[1..]).await,
            packet::FXP_REMOVE => self.handle_remove(&packet[1..]).await,
            packet::FXP_RENAME => self.handle_rename(&packet[1..]).await,
            packet::FXP_MKDIR => self.handle_mkdir(&packet[1..]).await,
            packet::FXP_RMDIR => self.handle_rmdir(&packet[1..]).await,
            packet::FXP_OPENDIR => self.handle_opendir(&packet[1..]).await,
            packet::FXP_READDIR => self.handle_readdir(&packet[1..]).await,
            packet::FXP_STAT => self.handle_stat(&packet[1..]).await,
            packet::FXP_LSTAT => self.handle_lstat(&packet[1..]).await,
            packet::FXP_FSTAT => self.handle_fstat(&packet[1..]).await,
            packet::FXP_SETSTAT => self.handle_setstat(&packet[1..]).await,
            packet::FXP_FSETSTAT => self.handle_fsetstat(&packet[1..]).await,
            packet::FXP_REALPATH => self.handle_realpath(&packet[1..]).await,
            packet::FXP_READLINK => self.handle_readlink(&packet[1..]).await,
            packet::FXP_SYMLINK => self.handle_symlink(&packet[1..]).await,
            _ => Ok(packet::encode_status_response(
                0,
                packet::SSH_FX_OP_UNSUPPORTED,
                "unsupported SFTP operation",
            )),
        }
    }

    // ── Per-packet-type handlers ────────────────────────────────────

    async fn handle_init(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (version, extensions) = packet::decode_init_request(payload)?;
        if version < 3 {
            return Ok(packet::encode_version_response(3, &HashMap::new()));
        }
        let negotiated = self.handler.init(version, extensions).await?;
        self.version = negotiated.max(3);
        self.negotiated = true;
        Ok(packet::encode_version_response(
            self.version,
            &HashMap::new(),
        ))
    }

    async fn handle_open(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, filename, pflags, attrs) = packet::decode_open_request(payload)?;
        let metadata = crate::SftpMetadata::from_packet(attrs);
        match self.handler.open(id, filename, pflags, metadata).await {
            Ok(handle) => Ok(packet::encode_handle_response(id, &handle)),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_close(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle) = packet::decode_close_request(payload)?;
        match self.handler.close(id, handle).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_read(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle, offset, len) = packet::decode_read_request(payload)?;
        match self.handler.read(id, handle, offset, len).await {
            Ok(data) => {
                if data.is_empty() {
                    Ok(packet::encode_status_response(
                        id,
                        packet::SSH_FX_EOF,
                        "End of file",
                    ))
                } else {
                    Ok(packet::encode_data_response(id, &data))
                }
            }
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_write(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle, offset, data) = packet::decode_write_request(payload)?;
        match self.handler.write(id, handle, offset, data).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_remove(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, filename) = packet::decode_remove_request(payload)?;
        match self.handler.remove(id, filename).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_rename(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, oldpath, newpath) = packet::decode_rename_request(payload)?;
        match self.handler.rename(id, oldpath, newpath).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_mkdir(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path, attrs) = packet::decode_mkdir_request(payload)?;
        let metadata = crate::SftpMetadata::from_packet(attrs);
        match self.handler.mkdir(id, path, metadata).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_rmdir(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_rmdir_request(payload)?;
        match self.handler.rmdir(id, path).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_opendir(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_opendir_request(payload)?;
        match self.handler.opendir(id, path).await {
            Ok(handle) => Ok(packet::encode_handle_response(id, &handle)),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_readdir(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle) = packet::decode_readdir_request(payload)?;
        match self.handler.readdir(id, handle).await {
            Ok(entries) => {
                if entries.is_empty() {
                    Ok(packet::encode_status_response(
                        id,
                        packet::SSH_FX_EOF,
                        "End of directory",
                    ))
                } else {
                    let name_entries: Vec<packet::SftpNameEntry> =
                        entries.iter().map(|e| e.to_name_entry()).collect();
                    Ok(packet::encode_name_response(id, &name_entries))
                }
            }
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_stat(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_stat_request(payload)?;
        match self.handler.stat(id, path).await {
            Ok(attrs) => Ok(packet::encode_attrs_response(id, &attrs.to_packet())),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_lstat(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_lstat_request(payload)?;
        match self.handler.lstat(id, path).await {
            Ok(attrs) => Ok(packet::encode_attrs_response(id, &attrs.to_packet())),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_fstat(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle) = packet::decode_fstat_request(payload)?;
        match self.handler.fstat(id, handle).await {
            Ok(attrs) => Ok(packet::encode_attrs_response(id, &attrs.to_packet())),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_setstat(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path, attrs) = packet::decode_setstat_request(payload)?;
        let metadata = crate::SftpMetadata::from_packet(attrs);
        match self.handler.setstat(id, path, metadata).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_fsetstat(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, handle, attrs) = packet::decode_fsetstat_request(payload)?;
        let metadata = crate::SftpMetadata::from_packet(attrs);
        match self.handler.fsetstat(id, handle, metadata).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_realpath(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_realpath_request(payload)?;
        match self.handler.realpath(id, path).await {
            Ok(entries) => {
                let name_entries: Vec<packet::SftpNameEntry> =
                    entries.iter().map(|e| e.to_name_entry()).collect();
                Ok(packet::encode_name_response(id, &name_entries))
            }
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_readlink(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, path) = packet::decode_readlink_request(payload)?;
        match self.handler.readlink(id, path).await {
            Ok(entries) => {
                let name_entries: Vec<packet::SftpNameEntry> =
                    entries.iter().map(|e| e.to_name_entry()).collect();
                Ok(packet::encode_name_response(id, &name_entries))
            }
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }

    async fn handle_symlink(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let (id, linkpath, targetpath) = packet::decode_symlink_request(payload)?;
        match self.handler.symlink(id, linkpath, targetpath).await {
            Ok(()) => Ok(packet::encode_status_response(id, packet::SSH_FX_OK, "OK")),
            Err(e) => Ok(sftp_error_status(id, &e)),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Convert a handler error into an SFTP status response.
fn sftp_error_status(id: u32, error: &Error) -> Vec<u8> {
    let (code, message) = sftp_status_from_error(error);
    packet::encode_status_response(id, code, &message)
}

fn sftp_status_from_error(error: &Error) -> (u32, String) {
    if let Error::Sftp(sftp_err) = error {
        let (code, default_msg) = match sftp_err.kind() {
            SftpErrorKind::NoSuchFile => (packet::SSH_FX_NO_SUCH_FILE, "no such file"),
            SftpErrorKind::PermissionDenied => {
                (packet::SSH_FX_PERMISSION_DENIED, "permission denied")
            }
            SftpErrorKind::Unsupported => (packet::SSH_FX_OP_UNSUPPORTED, "unsupported operation"),
            SftpErrorKind::Protocol => (packet::SSH_FX_BAD_MESSAGE, "protocol error"),
            SftpErrorKind::UnsupportedVersion => (packet::SSH_FX_FAILURE, "unsupported version"),
            SftpErrorKind::ChannelIo => (packet::SSH_FX_FAILURE, "channel I/O error"),
            SftpErrorKind::RemoteStatus => (packet::SSH_FX_FAILURE, "internal error"),
            SftpErrorKind::UnexpectedResponse => (packet::SSH_FX_FAILURE, "internal error"),
            _ => (packet::SSH_FX_FAILURE, "failure"),
        };
        let msg = sftp_err.message();
        let message = if msg.is_empty() {
            default_msg.to_string()
        } else {
            msg.to_string()
        };
        (code, message)
    } else {
        (packet::SSH_FX_FAILURE, format!("{error}"))
    }
}
