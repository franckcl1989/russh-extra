//! SFTP protocol packet encoding and decoding.
//!
//! Implements the SFTP v3 wire format. Every call to an `encode_*`
//! function returns a `Vec<u8>` ready to write to the SSH channel.
//! Every `decode_*` function takes a raw `&[u8]` payload (without the
//! 4-byte length prefix) and returns the parsed fields.

use std::collections::HashMap;

use russh_extra_core::{Error, Result, SftpErrorKind};

/// Maximum SFTP packet payload size (256 KiB).
pub(crate) const MAX_SFTP_PACKET_SIZE: u32 = 256 * 1024;

pub(crate) const FXP_INIT: u8 = 1;
pub(crate) const FXP_VERSION: u8 = 2;
pub(crate) const FXP_OPEN: u8 = 3;
pub(crate) const FXP_CLOSE: u8 = 4;
pub(crate) const FXP_READ: u8 = 5;
pub(crate) const FXP_WRITE: u8 = 6;
pub(crate) const FXP_LSTAT: u8 = 7;
pub(crate) const FXP_FSTAT: u8 = 8;
pub(crate) const FXP_SETSTAT: u8 = 9;
pub(crate) const FXP_FSETSTAT: u8 = 10;
pub(crate) const FXP_OPENDIR: u8 = 11;
pub(crate) const FXP_READDIR: u8 = 12;
pub(crate) const FXP_REMOVE: u8 = 13;
pub(crate) const FXP_MKDIR: u8 = 14;
pub(crate) const FXP_RMDIR: u8 = 15;
pub(crate) const FXP_REALPATH: u8 = 16;
pub(crate) const FXP_STAT: u8 = 17;
pub(crate) const FXP_RENAME: u8 = 18;
pub(crate) const FXP_READLINK: u8 = 19;
pub(crate) const FXP_SYMLINK: u8 = 20;
pub(crate) const FXP_STATUS: u8 = 101;
pub(crate) const FXP_HANDLE: u8 = 102;
pub(crate) const FXP_DATA: u8 = 103;
pub(crate) const FXP_NAME: u8 = 104;
pub(crate) const FXP_ATTRS: u8 = 105;

pub(crate) const SSH_FILEXFER_ATTR_SIZE: u32 = 0x00000001;
pub(crate) const SSH_FILEXFER_ATTR_UIDGID: u32 = 0x00000002;
pub(crate) const SSH_FILEXFER_ATTR_PERMISSIONS: u32 = 0x00000004;
pub(crate) const SSH_FILEXFER_ATTR_ACMODTIME: u32 = 0x00000008;

pub(crate) const SSH_FX_OK: u32 = 0;
pub(crate) const SSH_FX_EOF: u32 = 1;
pub(crate) const SSH_FX_NO_SUCH_FILE: u32 = 2;
pub(crate) const SSH_FX_PERMISSION_DENIED: u32 = 3;
pub(crate) const SSH_FX_FAILURE: u32 = 4;
pub(crate) const SSH_FX_BAD_MESSAGE: u32 = 5;
pub(crate) const SSH_FX_NO_CONNECTION: u32 = 6;
pub(crate) const SSH_FX_CONNECTION_LOST: u32 = 7;
pub(crate) const SSH_FX_OP_UNSUPPORTED: u32 = 8;

pub(crate) fn encode_init(version: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, version);
    push_u32(&mut buf, 0); // extension count
    frame(FXP_INIT, &buf)
}

pub(crate) fn encode_open(id: u32, filename: &str, pflags: u32, attrs: &SftpFileAttrs) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, filename);
    push_u32(&mut buf, pflags);
    push_attrs(&mut buf, attrs);
    frame(FXP_OPEN, &buf)
}

pub(crate) fn encode_close(id: u32, handle: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    frame(FXP_CLOSE, &buf)
}

pub(crate) fn encode_read(id: u32, handle: &str, offset: u64, len: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    push_u64(&mut buf, offset);
    push_u32(&mut buf, len);
    frame(FXP_READ, &buf)
}

pub(crate) fn encode_write(id: u32, handle: &str, offset: u64, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    push_u64(&mut buf, offset);
    push_bytes(&mut buf, data);
    frame(FXP_WRITE, &buf)
}

pub(crate) fn encode_stat(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_STAT, &buf)
}

pub(crate) fn encode_lstat(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_LSTAT, &buf)
}

pub(crate) fn encode_fstat(id: u32, handle: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    frame(FXP_FSTAT, &buf)
}

pub(crate) fn encode_setstat(id: u32, path: &str, attrs: &SftpFileAttrs) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    push_attrs(&mut buf, attrs);
    frame(FXP_SETSTAT, &buf)
}

pub(crate) fn encode_opendir(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_OPENDIR, &buf)
}

pub(crate) fn encode_readdir(id: u32, handle: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    frame(FXP_READDIR, &buf)
}

pub(crate) fn encode_remove(id: u32, filename: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, filename);
    frame(FXP_REMOVE, &buf)
}

pub(crate) fn encode_mkdir(id: u32, path: &str, attrs: &SftpFileAttrs) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    push_attrs(&mut buf, attrs);
    frame(FXP_MKDIR, &buf)
}

pub(crate) fn encode_rmdir(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_RMDIR, &buf)
}

pub(crate) fn encode_realpath(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_REALPATH, &buf)
}

pub(crate) fn encode_rename(id: u32, oldpath: &str, newpath: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, oldpath);
    push_string(&mut buf, newpath);
    frame(FXP_RENAME, &buf)
}

pub(crate) fn encode_readlink(id: u32, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, path);
    frame(FXP_READLINK, &buf)
}

pub(crate) fn encode_symlink(id: u32, linkpath: &str, targetpath: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, linkpath);
    push_string(&mut buf, targetpath);
    frame(FXP_SYMLINK, &buf)
}

pub(crate) fn encode_fsetstat(id: u32, handle: &str, attrs: &SftpFileAttrs) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    push_attrs(&mut buf, attrs);
    frame(FXP_FSETSTAT, &buf)
}

pub(crate) fn decode_version(payload: &[u8]) -> Result<(u32, HashMap<String, String>)> {
    let mut pos = 0;
    let version = pop_u32(payload, &mut pos)?;
    let count = pop_u32(payload, &mut pos)?;
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    let max_count = payload.len().saturating_sub(pos) / 8;
    let count = count.min(max_count).min(128);
    let mut extensions = HashMap::new();
    for _ in 0..count {
        let key = pop_string(payload, &mut pos)?;
        let value = pop_string(payload, &mut pos)?;
        extensions.insert(key, value);
    }
    Ok((version, extensions))
}

pub(crate) fn decode_status(payload: &[u8]) -> Result<(u32, u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let code = pop_u32(payload, &mut pos)?;
    let msg = pop_string(payload, &mut pos)?;
    let _lang = pop_string(payload, &mut pos)?;
    Ok((id, code, msg))
}

pub(crate) fn decode_handle(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    Ok((id, handle))
}

pub(crate) fn decode_data(payload: &[u8]) -> Result<(u32, Vec<u8>)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let data = pop_bytes(payload, &mut pos)?;
    Ok((id, data))
}

pub(crate) fn decode_name(payload: &[u8]) -> Result<(u32, Vec<SftpNameEntry>)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let count = pop_u32(payload, &mut pos)?;
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    let max_count = payload.len().saturating_sub(pos) / 16;
    let count = count.min(max_count).min(65536);
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let filename = pop_string(payload, &mut pos)?;
        let longname = pop_string(payload, &mut pos)?;
        let attrs = pop_attrs(payload, &mut pos)?;
        entries.push(SftpNameEntry {
            filename,
            longname,
            attrs,
        });
    }
    Ok((id, entries))
}

pub(crate) fn decode_attrs(payload: &[u8]) -> Result<(u32, SftpFileAttrs)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let attrs = pop_attrs(payload, &mut pos)?;
    Ok((id, attrs))
}

pub(crate) fn decode_packet_type(payload: &[u8]) -> Result<u8> {
    if payload.is_empty() {
        return Err(Error::sftp(SftpErrorKind::Protocol, "empty SFTP packet"));
    }
    Ok(payload[0])
}

pub(crate) fn check_status(status_code: u32, message: &str) -> Result<()> {
    if status_code == SSH_FX_OK {
        Ok(())
    } else {
        Err(Error::sftp(
            sftp_error_kind_for_code(status_code),
            format!("SFTP error code {status_code}: {message}"),
        ))
    }
}

pub(crate) fn sftp_error_kind_for_code(code: u32) -> SftpErrorKind {
    match code {
        SSH_FX_NO_SUCH_FILE => SftpErrorKind::NoSuchFile,
        SSH_FX_PERMISSION_DENIED => SftpErrorKind::PermissionDenied,
        SSH_FX_BAD_MESSAGE => SftpErrorKind::Protocol,
        SSH_FX_OP_UNSUPPORTED => SftpErrorKind::Unsupported,
        _ => SftpErrorKind::RemoteStatus,
    }
}

pub(crate) fn status_code_name(code: u32) -> &'static str {
    match code {
        SSH_FX_OK => "OK",
        SSH_FX_EOF => "EOF",
        SSH_FX_NO_SUCH_FILE => "NO_SUCH_FILE",
        SSH_FX_PERMISSION_DENIED => "PERMISSION_DENIED",
        SSH_FX_FAILURE => "FAILURE",
        SSH_FX_BAD_MESSAGE => "BAD_MESSAGE",
        SSH_FX_NO_CONNECTION => "NO_CONNECTION",
        SSH_FX_CONNECTION_LOST => "CONNECTION_LOST",
        SSH_FX_OP_UNSUPPORTED => "OP_UNSUPPORTED",
        _ => "UNKNOWN",
    }
}

// ── Server-side: decode SFTP requests ─────────────────────────────

#[cfg(feature = "server")]
pub(crate) fn decode_init_request(payload: &[u8]) -> Result<(u32, HashMap<String, String>)> {
    let (version, extensions) = decode_version(payload)?;
    Ok((version, extensions))
}

#[cfg(feature = "server")]
pub(crate) fn decode_open_request(payload: &[u8]) -> Result<(u32, String, u32, SftpFileAttrs)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let filename = pop_string(payload, &mut pos)?;
    let pflags = pop_u32(payload, &mut pos)?;
    let attrs = pop_attrs(payload, &mut pos)?;
    Ok((id, filename, pflags, attrs))
}

#[cfg(feature = "server")]
pub(crate) fn decode_close_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    Ok((id, handle))
}

#[cfg(feature = "server")]
pub(crate) fn decode_read_request(payload: &[u8]) -> Result<(u32, String, u64, u32)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    let offset = pop_u64(payload, &mut pos)?;
    let len = pop_u32(payload, &mut pos)?;
    Ok((id, handle, offset, len))
}

#[cfg(feature = "server")]
pub(crate) fn decode_write_request(payload: &[u8]) -> Result<(u32, String, u64, Vec<u8>)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    let offset = pop_u64(payload, &mut pos)?;
    let data = pop_bytes(payload, &mut pos)?;
    Ok((id, handle, offset, data))
}

#[cfg(feature = "server")]
pub(crate) fn decode_remove_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let filename = pop_string(payload, &mut pos)?;
    Ok((id, filename))
}

#[cfg(feature = "server")]
pub(crate) fn decode_mkdir_request(payload: &[u8]) -> Result<(u32, String, SftpFileAttrs)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    let attrs = pop_attrs(payload, &mut pos)?;
    Ok((id, path, attrs))
}

#[cfg(feature = "server")]
pub(crate) fn decode_rmdir_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_opendir_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_readdir_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    Ok((id, handle))
}

#[cfg(feature = "server")]
pub(crate) fn decode_stat_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_lstat_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_fstat_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    Ok((id, handle))
}

#[cfg(feature = "server")]
pub(crate) fn decode_setstat_request(payload: &[u8]) -> Result<(u32, String, SftpFileAttrs)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    let attrs = pop_attrs(payload, &mut pos)?;
    Ok((id, path, attrs))
}

#[cfg(feature = "server")]
pub(crate) fn decode_fsetstat_request(payload: &[u8]) -> Result<(u32, String, SftpFileAttrs)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let handle = pop_string(payload, &mut pos)?;
    let attrs = pop_attrs(payload, &mut pos)?;
    Ok((id, handle, attrs))
}

#[cfg(feature = "server")]
pub(crate) fn decode_realpath_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_rename_request(payload: &[u8]) -> Result<(u32, String, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let oldpath = pop_string(payload, &mut pos)?;
    let newpath = pop_string(payload, &mut pos)?;
    Ok((id, oldpath, newpath))
}

#[cfg(feature = "server")]
pub(crate) fn decode_readlink_request(payload: &[u8]) -> Result<(u32, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let path = pop_string(payload, &mut pos)?;
    Ok((id, path))
}

#[cfg(feature = "server")]
pub(crate) fn decode_symlink_request(payload: &[u8]) -> Result<(u32, String, String)> {
    let mut pos = 0;
    let id = pop_u32(payload, &mut pos)?;
    let linkpath = pop_string(payload, &mut pos)?;
    let targetpath = pop_string(payload, &mut pos)?;
    Ok((id, linkpath, targetpath))
}

// ── Server-side: encode SFTP responses ────────────────────────────

#[cfg(feature = "server")]
pub(crate) fn encode_version_response(
    version: u32,
    extensions: &HashMap<String, String>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, version);
    push_u32(&mut buf, extensions.len() as u32);
    for (key, value) in extensions {
        push_string(&mut buf, key);
        push_string(&mut buf, value);
    }
    frame(FXP_VERSION, &buf)
}

#[cfg(feature = "server")]
pub(crate) fn encode_status_response(id: u32, code: u32, msg: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_u32(&mut buf, code);
    push_string(&mut buf, msg);
    push_string(&mut buf, "en");
    frame(FXP_STATUS, &buf)
}

#[cfg(feature = "server")]
pub(crate) fn encode_handle_response(id: u32, handle: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_string(&mut buf, handle);
    frame(FXP_HANDLE, &buf)
}

#[cfg(feature = "server")]
pub(crate) fn encode_data_response(id: u32, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_bytes(&mut buf, data);
    frame(FXP_DATA, &buf)
}

#[cfg(feature = "server")]
pub(crate) fn encode_name_response(id: u32, entries: &[SftpNameEntry]) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_u32(&mut buf, entries.len() as u32);
    for entry in entries {
        push_string(&mut buf, &entry.filename);
        push_string(&mut buf, &entry.longname);
        push_attrs(&mut buf, &entry.attrs);
    }
    frame(FXP_NAME, &buf)
}

#[cfg(feature = "server")]
pub(crate) fn encode_attrs_response(id: u32, attrs: &SftpFileAttrs) -> Vec<u8> {
    let mut buf = Vec::new();
    push_u32(&mut buf, id);
    push_attrs(&mut buf, attrs);
    frame(FXP_ATTRS, &buf)
}

// ── SFTP attribute types (shared between packet and types modules) ──

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SftpFileAttrs {
    pub flags: u32,
    pub size: Option<u64>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub permissions: Option<u32>,
    pub atime: Option<u32>,
    pub mtime: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SftpNameEntry {
    pub filename: String,
    pub longname: String,
    pub attrs: SftpFileAttrs,
}

// ── Internal helpers ──

fn frame(packet_type: u8, payload: &[u8]) -> Vec<u8> {
    let len = 1 + payload.len() as u32;
    let mut buf = Vec::with_capacity(4 + len as usize);
    push_u32(&mut buf, len);
    buf.push(packet_type);
    buf.extend_from_slice(payload);
    buf
}

fn push_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn push_string(buf: &mut Vec<u8>, s: &str) {
    push_bytes(buf, s.as_bytes());
}

fn push_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    push_u32(buf, data.len() as u32);
    buf.extend_from_slice(data);
}

fn push_attrs(buf: &mut Vec<u8>, attrs: &SftpFileAttrs) {
    push_u32(buf, attrs.flags);
    if let Some(size) = attrs.size {
        push_u64(buf, size);
    }
    if let Some(uid) = attrs.uid {
        push_u32(buf, uid);
        push_u32(buf, attrs.gid.unwrap_or(0));
    }
    if let Some(permissions) = attrs.permissions {
        push_u32(buf, permissions);
    }
    if let Some(atime) = attrs.atime {
        push_u32(buf, atime);
        push_u32(buf, attrs.mtime.unwrap_or(0));
    }
}

fn pop_u32(data: &[u8], pos: &mut usize) -> Result<u32> {
    check_bounds(data, *pos, 4)?;
    let bytes: [u8; 4] = data[*pos..*pos + 4].try_into().unwrap();
    *pos += 4;
    Ok(u32::from_be_bytes(bytes))
}

fn pop_u64(data: &[u8], pos: &mut usize) -> Result<u64> {
    check_bounds(data, *pos, 8)?;
    let bytes: [u8; 8] = data[*pos..*pos + 8].try_into().unwrap();
    *pos += 8;
    Ok(u64::from_be_bytes(bytes))
}

fn pop_string(data: &[u8], pos: &mut usize) -> Result<String> {
    let bytes = pop_bytes(data, pos)?;
    String::from_utf8(bytes)
        .map_err(|_| Error::sftp(SftpErrorKind::Protocol, "SFTP string is not valid UTF-8"))
}

fn pop_bytes(data: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let len = pop_u32(data, pos)?;
    if len > MAX_SFTP_PACKET_SIZE {
        return Err(Error::sftp(
            SftpErrorKind::Protocol,
            format!("SFTP byte length {len} exceeds maximum {MAX_SFTP_PACKET_SIZE}"),
        ));
    }
    let len = len as usize;
    check_bounds(data, *pos, len)?;
    let bytes = data[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(bytes)
}

fn pop_attrs(data: &[u8], pos: &mut usize) -> Result<SftpFileAttrs> {
    let flags = pop_u32(data, pos)?;
    let mut attrs = SftpFileAttrs {
        flags,
        ..Default::default()
    };
    if flags & SSH_FILEXFER_ATTR_SIZE != 0 {
        attrs.size = Some(pop_u64(data, pos)?);
    }
    if flags & SSH_FILEXFER_ATTR_UIDGID != 0 {
        attrs.uid = Some(pop_u32(data, pos)?);
        attrs.gid = Some(pop_u32(data, pos)?);
    }
    if flags & SSH_FILEXFER_ATTR_PERMISSIONS != 0 {
        attrs.permissions = Some(pop_u32(data, pos)?);
    }
    if flags & SSH_FILEXFER_ATTR_ACMODTIME != 0 {
        attrs.atime = Some(pop_u32(data, pos)?);
        attrs.mtime = Some(pop_u32(data, pos)?);
    }
    Ok(attrs)
}

fn check_bounds(data: &[u8], pos: usize, needed: usize) -> Result<()> {
    if pos + needed > data.len() {
        Err(Error::sftp(
            SftpErrorKind::Protocol,
            format!(
                "SFTP packet truncated: need {needed} bytes at position {pos}, have {}",
                data.len().saturating_sub(pos)
            ),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_init() {
        let packet = encode_init(3);
        let len = u32::from_be_bytes(packet[0..4].try_into().unwrap()) as usize;
        assert_eq!(packet[4], FXP_INIT);
        let payload = &packet[5..4 + len];
        assert_eq!(pop_u32(payload, &mut 0).unwrap(), 3);
        assert_eq!(pop_u32(payload, &mut 4).unwrap(), 0);
    }

    #[test]
    fn encode_decode_version() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 3);
        push_u32(&mut payload, 0);
        let (version, extensions) = decode_version(&payload).unwrap();
        assert_eq!(version, 3);
        assert!(extensions.is_empty());
    }

    #[test]
    fn encode_decode_open_close() {
        let attrs = SftpFileAttrs::default();
        let open_packet = encode_open(1, "/test.txt", 0x01, &attrs);
        let payload = &open_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 1);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/test.txt");
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 0x01);

        let close_packet = encode_close(2, "handle-1");
        let payload = &close_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 2);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "handle-1");
    }

    #[test]
    fn encode_decode_read_write() {
        let read_packet = encode_read(3, "h1", 100, 4096);
        let payload = &read_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 3);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "h1");
        assert_eq!(pop_u64(payload, &mut pos).unwrap(), 100);
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 4096);

        let write_packet = encode_write(4, "h2", 200, b"hello");
        let payload = &write_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 4);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "h2");
        assert_eq!(pop_u64(payload, &mut pos).unwrap(), 200);
        assert_eq!(pop_bytes(payload, &mut pos).unwrap(), b"hello");
    }

    #[test]
    fn decode_status_and_check() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 1);
        push_u32(&mut payload, SSH_FX_OK);
        push_string(&mut payload, "OK");
        push_string(&mut payload, "en");
        let (id, code, msg) = decode_status(&payload).unwrap();
        assert_eq!(id, 1);
        assert_eq!(code, SSH_FX_OK);
        check_status(code, &msg).unwrap();

        let mut payload = Vec::new();
        push_u32(&mut payload, 2);
        push_u32(&mut payload, SSH_FX_PERMISSION_DENIED);
        push_string(&mut payload, "denied");
        push_string(&mut payload, "en");
        let (id, code, msg) = decode_status(&payload).unwrap();
        assert_eq!(id, 2);
        assert_eq!(code, SSH_FX_PERMISSION_DENIED);
        assert!(check_status(code, &msg).is_err());
    }

    #[test]
    fn test_decode_handle_unit() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 5);
        push_string(&mut payload, "my-handle");
        let (id, handle) = decode_handle(&payload).unwrap();
        assert_eq!(id, 5);
        assert_eq!(handle, "my-handle");
    }

    #[test]
    fn test_decode_data_unit() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 6);
        push_bytes(&mut payload, b"hello world");
        let (id, data) = decode_data(&payload).unwrap();
        assert_eq!(id, 6);
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn decode_name_empty() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 7);
        push_u32(&mut payload, 0);
        let (id, entries) = decode_name(&payload).unwrap();
        assert_eq!(id, 7);
        assert!(entries.is_empty());
    }

    #[test]
    fn decode_name_with_entries() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 8);
        push_u32(&mut payload, 1);
        push_string(&mut payload, "file.txt");
        push_string(
            &mut payload,
            "-rw-r--r-- 1 user group 100 Jan 1 2024 file.txt",
        );
        push_attrs(&mut payload, &SftpFileAttrs::default());
        let (id, entries) = decode_name(&payload).unwrap();
        assert_eq!(id, 8);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "file.txt");
        assert_eq!(
            entries[0].longname,
            "-rw-r--r-- 1 user group 100 Jan 1 2024 file.txt"
        );
    }

    #[test]
    fn test_decode_attrs_unit() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 9);
        push_u32(
            &mut payload,
            SSH_FILEXFER_ATTR_SIZE | SSH_FILEXFER_ATTR_PERMISSIONS,
        );
        push_u64(&mut payload, 1024);
        push_u32(&mut payload, 0o644);
        let (id, attrs) = decode_attrs(&payload).unwrap();
        assert_eq!(id, 9);
        assert_eq!(attrs.size, Some(1024));
        assert_eq!(attrs.permissions, Some(0o644));
        assert_eq!(attrs.uid, None);
    }

    #[test]
    fn truncated_packet_errors() {
        assert!(pop_u32(&[0, 0], &mut 0).is_err());
        assert!(pop_u64(&[], &mut 0).is_err());

        let mut payload = Vec::new();
        push_u32(&mut payload, 100); // claim 100 bytes of string data
        assert!(pop_string(&payload, &mut 0).is_err());
    }

    #[test]
    fn test_encode_rename_unit() {
        let packet = encode_rename(1, "/old.txt", "/new.txt");
        let payload = &packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 1);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/old.txt");
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/new.txt");
    }

    #[test]
    fn encode_symlink_readlink() {
        let sym_packet = encode_symlink(1, "/link", "/target");
        let payload = &sym_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 1);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/link");
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/target");

        let rl_packet = encode_readlink(2, "/link");
        let payload = &rl_packet[5..];
        let mut pos = 0;
        assert_eq!(pop_u32(payload, &mut pos).unwrap(), 2);
        assert_eq!(pop_string(payload, &mut pos).unwrap(), "/link");
    }

    #[test]
    fn attrs_round_trip() {
        let attrs = SftpFileAttrs {
            flags: SSH_FILEXFER_ATTR_SIZE
                | SSH_FILEXFER_ATTR_UIDGID
                | SSH_FILEXFER_ATTR_PERMISSIONS
                | SSH_FILEXFER_ATTR_ACMODTIME,
            size: Some(4096),
            uid: Some(1000),
            gid: Some(1000),
            permissions: Some(0o755),
            atime: Some(1000000),
            mtime: Some(2000000),
        };
        let mut buf = Vec::new();
        push_attrs(&mut buf, &attrs);
        let decoded = pop_attrs(&buf, &mut 0).unwrap();
        assert_eq!(decoded, attrs);
    }

    #[test]
    fn decode_packet_type_identifies_types() {
        let packet = encode_init(3);
        assert_eq!(decode_packet_type(&packet[4..]).unwrap(), FXP_INIT);

        let packet = encode_close(1, "h");
        assert_eq!(decode_packet_type(&packet[4..]).unwrap(), FXP_CLOSE);
    }

    #[test]
    fn status_code_names() {
        assert_eq!(status_code_name(SSH_FX_OK), "OK");
        assert_eq!(status_code_name(SSH_FX_NO_SUCH_FILE), "NO_SUCH_FILE");
        assert_eq!(
            status_code_name(SSH_FX_PERMISSION_DENIED),
            "PERMISSION_DENIED"
        );
        assert_eq!(status_code_name(999), "UNKNOWN");
    }
}
