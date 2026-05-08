//! SFTP client runtime.
//!
//! Implements the SSH subsystem lifecycle, SFTP version negotiation,
//! request pipelining, and public API methods.

use std::collections::HashMap;
use std::sync::Arc;

use russh::ChannelMsg;
use russh_extra_core::{Error, Result, SessionId, SftpErrorKind};
use tokio::sync::{Mutex, mpsc, oneshot};

use super::packet;
use super::types::{SftpDir, SftpDirEntry, SftpFile, SftpMetadata};

const MAX_PACKET_SIZE: usize = 256 * 1024;

#[derive(Clone)]
pub(crate) struct SftpClientRuntime {
    write_tx: mpsc::Sender<(u32, Vec<u8>)>,
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Result<SftpResponse>>>>>,
    next_id: Arc<std::sync::atomic::AtomicU32>,
}

#[derive(Debug)]
enum SftpResponse {
    Status(u32, String),
    Handle(String),
    Data(Vec<u8>),
    Name(Vec<(String, String, packet::SftpFileAttrs)>),
    Attrs(packet::SftpFileAttrs),
    #[allow(dead_code)]
    Version(u32, HashMap<String, String>),
}

impl SftpClientRuntime {
    pub async fn connect(
        _session_id: SessionId,
        handle: Arc<Mutex<russh::client::Handle<super::super::client::ClientHandler>>>,
    ) -> Result<Self> {
        let guard = handle.lock().await;
        let channel = guard.channel_open_session().await.map_err(|e| {
            Error::sftp_with_source(
                SftpErrorKind::ChannelIo,
                "failed to open SFTP session channel",
                e,
            )
        })?;

        channel.request_subsystem(true, "sftp").await.map_err(|e| {
            Error::sftp_with_source(
                SftpErrorKind::ChannelIo,
                "failed to request sftp subsystem",
                e,
            )
        })?;

        let (mut read_half, write_half) = channel.split();
        drop(guard);

        let init_packet = packet::encode_init(3);
        write_half.data(&init_packet[..]).await.map_err(|e| {
            Error::sftp_with_source(SftpErrorKind::ChannelIo, "failed to send SFTP init", e)
        })?;

        let version_response = read_sftp_response(&mut read_half).await?;
        let payload = version_response.ok_or_else(|| {
            Error::sftp(
                SftpErrorKind::Protocol,
                "channel closed before SFTP version",
            )
        })?;
        let ptype = packet::decode_packet_type(&payload)?;
        if ptype != packet::FXP_VERSION {
            return Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected FXP_VERSION, got packet type {ptype}"),
            ));
        }
        let (version, _extensions) = packet::decode_version(&payload[1..])?;
        if version < 3 {
            return Err(Error::sftp(
                SftpErrorKind::UnsupportedVersion,
                format!("server SFTP version {version} is not supported"),
            ));
        }

        let (write_tx, write_rx) = mpsc::channel(256);
        let pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Result<SftpResponse>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(std::sync::atomic::AtomicU32::new(10));

        let read_pending = pending.clone();
        tokio::spawn(async move {
            read_task(read_half, read_pending).await;
        });

        let write_pending = pending.clone();
        tokio::spawn(async move {
            write_task(write_half, write_rx, write_pending).await;
        });

        Ok(Self {
            write_tx,
            pending,
            next_id,
        })
    }

    fn next_request_id(&self) -> u32 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    async fn send_and_await(&self, packet: Vec<u8>, id: u32) -> Result<SftpResponse> {
        let (tx, rx) = oneshot::channel();
        {
            self.pending.lock().await.insert(id, tx);
        }

        self.write_tx
            .send((id, packet))
            .await
            .map_err(|_| Error::sftp(SftpErrorKind::ChannelIo, "SFTP write task closed"))?;

        rx.await
            .map_err(|_| Error::sftp(SftpErrorKind::ChannelIo, "SFTP request cancelled"))?
    }

    async fn expect_handle(&self, packet: Vec<u8>, id: u32) -> Result<String> {
        match self.send_and_await(packet, id).await? {
            SftpResponse::Handle(handle) => Ok(handle),
            SftpResponse::Status(code, msg) => Err(Error::sftp(
                SftpErrorKind::RemoteStatus,
                format!(
                    "SFTP {} (code {code}): {msg}",
                    packet::status_code_name(code)
                ),
            )),
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected handle, got: {other:?}"),
            )),
        }
    }

    async fn expect_status(&self, packet: Vec<u8>, id: u32) -> Result<()> {
        match self.send_and_await(packet, id).await? {
            SftpResponse::Status(code, msg) => packet::check_status(code, &msg),
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected status, got: {other:?}"),
            )),
        }
    }

    async fn expect_attrs(&self, packet: Vec<u8>, id: u32) -> Result<SftpMetadata> {
        match self.send_and_await(packet, id).await? {
            SftpResponse::Attrs(attrs) => Ok(SftpMetadata::from_packet(attrs)),
            SftpResponse::Status(code, msg) => {
                packet::check_status(code, &msg).map(|()| SftpMetadata::default())
            }
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected attrs, got: {other:?}"),
            )),
        }
    }

    async fn expect_name_single(&self, packet: Vec<u8>, id: u32) -> Result<String> {
        match self.send_and_await(packet, id).await? {
            SftpResponse::Name(entries) => entries
                .into_iter()
                .next()
                .map(|(filename, _, _)| filename)
                .ok_or_else(|| Error::sftp(SftpErrorKind::Protocol, "empty name response")),
            SftpResponse::Status(code, msg) => {
                packet::check_status(code, &msg).map(|()| String::new())
            }
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected name, got: {other:?}"),
            )),
        }
    }

    // ── Public API methods ─────────────────────────────────────────

    pub async fn open(&self, filename: &str, pflags: u32) -> Result<SftpFile> {
        let id = self.next_request_id();
        let attrs = packet::SftpFileAttrs::default();
        let packet = packet::encode_open(id, filename, pflags, &attrs);
        let handle = self.expect_handle(packet, id).await?;
        Ok(SftpFile::new(handle, self.clone()))
    }

    pub async fn read(&self, handle: &str, offset: u64, len: u32) -> Result<Vec<u8>> {
        let id = self.next_request_id();
        let packet = packet::encode_read(id, handle, offset, len);
        match self.send_and_await(packet, id).await? {
            SftpResponse::Data(data) => Ok(data),
            SftpResponse::Status(code, msg) => {
                if code == packet::SSH_FX_EOF {
                    Ok(Vec::new())
                } else {
                    packet::check_status(code, &msg).map(|()| Vec::new())
                }
            }
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected data, got: {other:?}"),
            )),
        }
    }

    pub async fn write(&self, handle: &str, offset: u64, data: &[u8]) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_write(id, handle, offset, data);
        self.expect_status(packet, id).await
    }

    pub async fn close(&self, handle: &str) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_close(id, handle);
        self.expect_status(packet, id).await
    }

    pub async fn stat(&self, path: &str) -> Result<SftpMetadata> {
        let id = self.next_request_id();
        let packet = packet::encode_stat(id, path);
        self.expect_attrs(packet, id).await
    }

    pub async fn lstat(&self, path: &str) -> Result<SftpMetadata> {
        let id = self.next_request_id();
        let packet = packet::encode_lstat(id, path);
        self.expect_attrs(packet, id).await
    }

    pub async fn fstat(&self, handle: &str) -> Result<SftpMetadata> {
        let id = self.next_request_id();
        let packet = packet::encode_fstat(id, handle);
        self.expect_attrs(packet, id).await
    }

    pub async fn opendir(&self, path: &str) -> Result<SftpDir> {
        let id = self.next_request_id();
        let packet = packet::encode_opendir(id, path);
        let handle = self.expect_handle(packet, id).await?;
        Ok(SftpDir::new(handle, self.clone()))
    }

    pub async fn readdir_entry(&self, handle: &str) -> Result<Option<SftpDirEntry>> {
        let id = self.next_request_id();
        let packet = packet::encode_readdir(id, handle);
        match self.send_and_await(packet, id).await? {
            SftpResponse::Name(entries) => {
                let (filename, longname, attrs) = entries
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::sftp(SftpErrorKind::Protocol, "empty NAME response"))?;
                Ok(Some(SftpDirEntry::from_packet(filename, longname, attrs)))
            }
            SftpResponse::Status(code, msg) => {
                if code == packet::SSH_FX_EOF {
                    Ok(None)
                } else {
                    packet::check_status(code, &msg).map(|()| None)
                }
            }
            other => Err(Error::sftp(
                SftpErrorKind::Protocol,
                format!("expected name or status, got: {other:?}"),
            )),
        }
    }

    pub async fn remove(&self, filename: &str) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_remove(id, filename);
        self.expect_status(packet, id).await
    }

    pub async fn rename(&self, oldpath: &str, newpath: &str) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_rename(id, oldpath, newpath);
        self.expect_status(packet, id).await
    }

    pub async fn mkdir(&self, path: &str) -> Result<()> {
        let id = self.next_request_id();
        let attrs = packet::SftpFileAttrs::default();
        let packet = packet::encode_mkdir(id, path, &attrs);
        self.expect_status(packet, id).await
    }

    pub async fn rmdir(&self, path: &str) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_rmdir(id, path);
        self.expect_status(packet, id).await
    }

    pub async fn realpath(&self, path: &str) -> Result<String> {
        let id = self.next_request_id();
        let packet = packet::encode_realpath(id, path);
        self.expect_name_single(packet, id).await
    }

    pub async fn readlink(&self, path: &str) -> Result<String> {
        let id = self.next_request_id();
        let packet = packet::encode_readlink(id, path);
        self.expect_name_single(packet, id).await
    }

    pub async fn symlink(&self, linkpath: &str, targetpath: &str) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_symlink(id, linkpath, targetpath);
        self.expect_status(packet, id).await
    }

    pub async fn setstat(&self, path: &str, attrs: &packet::SftpFileAttrs) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_setstat(id, path, attrs);
        self.expect_status(packet, id).await
    }

    pub async fn fsetstat(&self, handle: &str, attrs: &packet::SftpFileAttrs) -> Result<()> {
        let id = self.next_request_id();
        let packet = packet::encode_fsetstat(id, handle, attrs);
        self.expect_status(packet, id).await
    }
}

// ── Background tasks ──────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
async fn read_task(
    mut read_half: russh::ChannelReadHalf,
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Result<SftpResponse>>>>>,
) {
    let mut buf = Vec::new();
    loop {
        match read_half.wait().await {
            Some(ChannelMsg::Data { data }) => {
                buf.extend_from_slice(&data);
                loop {
                    if buf.len() < 4 {
                        break;
                    }
                    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                    if len > MAX_PACKET_SIZE || len == 0 {
                        tracing::warn!(len, "invalid SFTP packet length, aborting read task");
                        return;
                    }
                    if buf.len() < 4 + len {
                        break;
                    }
                    let packet = buf[4..4 + len].to_vec();
                    buf.drain(..4 + len);
                    dispatch_response(&packet, &pending).await;
                }
            }
            Some(ChannelMsg::Close) | None => {
                break;
            }
            _ => {}
        }
    }
}

#[allow(clippy::type_complexity)]
async fn dispatch_response(
    packet: &[u8],
    pending: &Arc<Mutex<HashMap<u32, oneshot::Sender<Result<SftpResponse>>>>>,
) {
    if packet.is_empty() {
        return;
    }
    let result = match packet[0] {
        packet::FXP_VERSION => decode_version_response(packet),
        packet::FXP_STATUS => decode_status_response(packet),
        packet::FXP_HANDLE => decode_handle_response(packet),
        packet::FXP_DATA => decode_data_response(packet),
        packet::FXP_NAME => decode_name_response(packet),
        packet::FXP_ATTRS => decode_attrs_response(packet),
        _ => Err((
            0,
            Error::sftp(
                SftpErrorKind::Protocol,
                format!("unknown SFTP packet type: {}", packet[0]),
            ),
        )),
    };

    match result {
        Ok((id, response)) => {
            if let Some(tx) = pending.lock().await.remove(&id) {
                let _ = tx.send(Ok(response));
            }
        }
        Err((id, error)) => {
            if let Some(tx) = pending.lock().await.remove(&id) {
                let _ = tx.send(Err(error));
            }
        }
    }
}

fn decode_version_response(
    packet: &[u8],
) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (version, extensions) = packet::decode_version(&packet[1..]).map_err(|e| (0, e))?;
    Ok((0, SftpResponse::Version(version, extensions)))
}

fn decode_status_response(packet: &[u8]) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (id, code, msg) = packet::decode_status(&packet[1..]).map_err(|e| (0, e))?;
    Ok((id, SftpResponse::Status(code, msg)))
}

fn decode_handle_response(packet: &[u8]) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (id, handle) = packet::decode_handle(&packet[1..]).map_err(|e| (0, e))?;
    Ok((id, SftpResponse::Handle(handle)))
}

fn decode_data_response(packet: &[u8]) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (id, data) = packet::decode_data(&packet[1..]).map_err(|e| (0, e))?;
    Ok((id, SftpResponse::Data(data)))
}

fn decode_name_response(packet: &[u8]) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (id, entries) = packet::decode_name(&packet[1..]).map_err(|e| (0, e))?;
    let entries = entries
        .into_iter()
        .map(|e| (e.filename, e.longname, e.attrs))
        .collect();
    Ok((id, SftpResponse::Name(entries)))
}

fn decode_attrs_response(packet: &[u8]) -> std::result::Result<(u32, SftpResponse), (u32, Error)> {
    let (id, attrs) = packet::decode_attrs(&packet[1..]).map_err(|e| (0, e))?;
    Ok((id, SftpResponse::Attrs(attrs)))
}

#[allow(clippy::type_complexity)]
async fn write_task(
    write_half: russh::ChannelWriteHalf<russh::client::Msg>,
    mut write_rx: mpsc::Receiver<(u32, Vec<u8>)>,
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Result<SftpResponse>>>>>,
) {
    while let Some((_id, packet)) = write_rx.recv().await {
        if let Err(e) = write_half.data(&packet[..]).await {
            tracing::warn!(error = %e, "SFTP write task failed");
            break;
        }
    }

    let _ = write_half.close().await;

    let mut map = pending.lock().await;
    for (_, tx) in map.drain() {
        let _ = tx.send(Err(Error::sftp(
            SftpErrorKind::ChannelIo,
            "SFTP channel closed",
        )));
    }
}

async fn read_sftp_response(read_half: &mut russh::ChannelReadHalf) -> Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    loop {
        match read_half.wait().await {
            Some(ChannelMsg::Data { data }) => {
                buf.extend_from_slice(&data);
                if buf.len() < 4 {
                    continue;
                }
                let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                if len > MAX_PACKET_SIZE || len == 0 {
                    return Err(Error::sftp(
                        SftpErrorKind::Protocol,
                        "invalid SFTP packet length during negotiation",
                    ));
                }
                if buf.len() < 4 + len {
                    continue;
                }
                let packet = buf[4..4 + len].to_vec();
                return Ok(Some(packet));
            }
            Some(ChannelMsg::Close) | None => return Ok(None),
            _ => {}
        }
    }
}
