// ── Minimal SFTP mock for loopback integration tests ─────────────────

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::{ChannelId, server::Session};

/// Minimal in-memory SFTP handler for loopback tests.
///
/// Maintains a per-channel read buffer and an in-memory filesystem
/// so integration tests can exercise the full SFTP pipeline without
/// an external SSH server.
#[derive(Clone, Default)]
pub(crate) struct MockSftpServer {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    file_attrs: Arc<Mutex<HashMap<String, (u64, u32)>>>,
    dir_entries: Arc<Mutex<HashMap<String, Vec<String>>>>,
    symlinks: Arc<Mutex<HashMap<String, String>>>,
    buffers: Arc<Mutex<HashMap<ChannelId, Vec<u8>>>>,
    next_handle: Arc<std::sync::atomic::AtomicU32>,
    handles: Arc<Mutex<HashMap<String, (String, u32)>>>,
    readdir_positions: Arc<Mutex<HashMap<String, usize>>>,
}

impl MockSftpServer {
    pub(crate) fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            file_attrs: Arc::new(Mutex::new(HashMap::new())),
            dir_entries: Arc::new(Mutex::new(HashMap::new())),
            symlinks: Arc::new(Mutex::new(HashMap::new())),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            next_handle: Arc::new(std::sync::atomic::AtomicU32::new(1)),
            handles: Arc::new(Mutex::new(HashMap::new())),
            readdir_positions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn add_file(&self, path: &str, data: &[u8], size: u64, perms: u32) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), data.to_vec());
        self.file_attrs
            .lock()
            .unwrap()
            .insert(path.to_owned(), (size, perms));
    }

    pub(crate) fn add_dir(&self, path: &str, entries: &[&str]) {
        self.dir_entries.lock().unwrap().insert(
            path.to_owned(),
            entries.iter().map(|s| s.to_string()).collect(),
        );
    }

    pub(crate) fn add_symlink(&self, linkpath: &str, targetpath: &str) {
        self.symlinks
            .lock()
            .unwrap()
            .insert(linkpath.to_owned(), targetpath.to_owned());
    }

    fn alloc_handle(&self, kind: u32, data: String) -> String {
        let id = self
            .next_handle
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let handle = format!("h-{id}");
        self.handles
            .lock()
            .unwrap()
            .insert(handle.clone(), (data, kind));
        handle
    }

    fn resolve_handle(&self, handle: &str) -> Option<(String, u32)> {
        self.handles.lock().unwrap().get(handle).cloned()
    }

    pub(crate) fn feed(
        &self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        {
            let mut bufs = self.buffers.lock().unwrap();
            let buf = bufs.entry(channel).or_default();
            buf.extend_from_slice(data);
        }

        loop {
            let packet = {
                let mut bufs = self.buffers.lock().unwrap();
                let buf = bufs.entry(channel).or_default();
                if buf.len() < 4 {
                    break;
                }
                let packet_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                if packet_len == 0 || packet_len > 256 * 1024 {
                    buf.clear();
                    break;
                }
                if buf.len() < 4 + packet_len {
                    break;
                }
                let packet = buf[4..4 + packet_len].to_vec();
                buf.drain(..4 + packet_len);
                packet
            };

            self.handle_packet(channel, &packet, session)?;
        }
        Ok(())
    }

    fn handle_packet(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        if packet.is_empty() {
            return Ok(());
        }
        match packet[0] {
            1 => self.handle_init(channel, packet, session),
            3 => self.handle_open(channel, packet, session),
            4 => self.handle_close(channel, packet, session),
            5 => self.handle_read(channel, packet, session),
            6 => self.handle_write(channel, packet, session),
            7 => self.handle_lstat(channel, packet, session),
            11 => self.handle_opendir(channel, packet, session),
            12 => self.handle_readdir(channel, packet, session),
            13 => self.handle_remove(channel, packet, session),
            14 => self.handle_mkdir(channel, packet, session),
            15 => self.handle_rmdir(channel, packet, session),
            16 => self.handle_realpath(channel, packet, session),
            17 => self.handle_stat(channel, packet, session),
            18 => self.handle_rename(channel, packet, session),
            19 => self.handle_readlink(channel, packet, session),
            20 => self.handle_symlink(channel, packet, session),
            _ => Ok(()),
        }
    }

    fn handle_init(
        &self,
        channel: ChannelId,
        _packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let mut payload = Vec::new();
        push_u32_sftp(&mut payload, 3); // version
        push_u32_sftp(&mut payload, 0); // no extensions
        let response = frame_sftp(2, &payload);
        session.data(channel, response)?;
        Ok(())
    }

    fn handle_open(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let filename = pop_string_sftp(payload, &mut pos);
        let _flags = pop_u32_sftp(payload, &mut pos);

        let handle = self.alloc_handle(1, filename.clone());
        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_string_sftp(&mut response, &handle);
        session.data(channel, frame_sftp(102, &response))?;
        Ok(())
    }

    fn handle_close(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let _handle = pop_string_sftp(payload, &mut pos);
        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, 0); // SSH_FX_OK
        push_string_sftp(&mut response, "OK");
        push_string_sftp(&mut response, "en");
        session.data(channel, frame_sftp(101, &response))?;
        Ok(())
    }

    fn handle_read(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let handle = pop_string_sftp(payload, &mut pos);
        let offset = pop_u64_sftp(payload, &mut pos) as usize;
        let len = pop_u32_sftp(payload, &mut pos) as usize;

        let mut response = Vec::new();
        if let Some((ref_data, _kind)) = self.resolve_handle(&handle) {
            let file_data = self
                .files
                .lock()
                .unwrap()
                .get(&ref_data)
                .cloned()
                .unwrap_or_default();
            let slice =
                &file_data[offset.min(file_data.len())..(offset + len).min(file_data.len())];
            push_u32_sftp(&mut response, id);
            push_bytes_sftp(&mut response, slice);
            session.data(channel, frame_sftp(103, &response))?;
        } else {
            push_u32_sftp(&mut response, id);
            push_u32_sftp(&mut response, 1); // SSH_FX_EOF
            push_string_sftp(&mut response, "EOF");
            push_string_sftp(&mut response, "en");
            session.data(channel, frame_sftp(101, &response))?;
        }
        Ok(())
    }

    fn handle_write(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let handle = pop_string_sftp(payload, &mut pos);
        let offset = pop_u64_sftp(payload, &mut pos) as usize;
        let data = pop_bytes_sftp(payload, &mut pos);

        if let Some((ref_data, _kind)) = self.resolve_handle(&handle)
            && let Some(file) = self.files.lock().unwrap().get_mut(&ref_data)
        {
            if file.len() < offset + data.len() {
                file.resize(offset + data.len(), 0);
            }
            file[offset..offset + data.len()].copy_from_slice(&data);
            if let Some((size, _)) = self.file_attrs.lock().unwrap().get_mut(&ref_data) {
                *size = file.len() as u64;
            }
        }

        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, 0); // SSH_FX_OK
        push_string_sftp(&mut response, "OK");
        push_string_sftp(&mut response, "en");
        session.data(channel, frame_sftp(101, &response))?;
        Ok(())
    }

    fn handle_stat(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.handle_attrs_response(channel, packet, session)
    }

    fn handle_lstat(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.handle_attrs_response(channel, packet, session)
    }

    fn handle_attrs_response(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let path = pop_string_sftp(payload, &mut pos);

        let (size, perms) = self
            .file_attrs
            .lock()
            .unwrap()
            .get(&path)
            .copied()
            .unwrap_or((4096, 0o644));

        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, 0x00000001 | 0x00000004); // flags: size + permissions
        push_u64_sftp(&mut response, size);
        push_u32_sftp(&mut response, perms);
        session.data(channel, frame_sftp(105, &response))?;
        Ok(())
    }

    fn handle_opendir(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let path = pop_string_sftp(payload, &mut pos);

        let handle = self.alloc_handle(2, path);
        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_string_sftp(&mut response, &handle);
        session.data(channel, frame_sftp(102, &response))?;
        Ok(())
    }

    fn handle_readdir(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let handle = pop_string_sftp(payload, &mut pos);

        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);

        if let Some((dir_path, _kind)) = self.resolve_handle(&handle) {
            let entries = self
                .dir_entries
                .lock()
                .unwrap()
                .get(&dir_path)
                .cloned()
                .unwrap_or_default();

            let mut positions = self.readdir_positions.lock().unwrap();
            let pos = positions.entry(handle.clone()).or_insert(0);

            if *pos < entries.len() {
                let entry = &entries[*pos];
                *pos += 1;

                push_u32_sftp(&mut response, 1);
                push_string_sftp(&mut response, entry);
                push_string_sftp(
                    &mut response,
                    &format!("-rw-r--r-- 1 user group 0 Jan 1 2024 {entry}"),
                );
                push_u32_sftp(&mut response, 0x00000001 | 0x00000004);
                push_u64_sftp(&mut response, 0);
                push_u32_sftp(&mut response, 0o644);
                session.data(channel, frame_sftp(104, &response))?;
            } else {
                // EOF
                push_u32_sftp(&mut response, 1); // SSH_FX_EOF
                push_string_sftp(&mut response, "EOF");
                push_string_sftp(&mut response, "en");
                session.data(channel, frame_sftp(101, &response))?;
            }
        } else {
            push_u32_sftp(&mut response, 1);
            push_string_sftp(&mut response, "EOF");
            push_string_sftp(&mut response, "en");
            session.data(channel, frame_sftp(101, &response))?;
        }
        Ok(())
    }

    fn handle_remove(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.simple_status(channel, packet, session, 0) // SSH_FX_OK
    }

    fn handle_mkdir(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.simple_status(channel, packet, session, 0)
    }

    fn handle_rmdir(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.simple_status(channel, packet, session, 0)
    }

    fn handle_realpath(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let path = pop_string_sftp(payload, &mut pos);

        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, 1); // one entry
        push_string_sftp(&mut response, &path);
        push_string_sftp(&mut response, &path);
        push_u32_sftp(&mut response, 0x00000004); // attrs flags: permissions
        push_u32_sftp(&mut response, 0o755); // directory perms
        session.data(channel, frame_sftp(104, &response))?;
        Ok(())
    }

    fn handle_rename(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.simple_status(channel, packet, session, 0)
    }

    fn handle_readlink(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);
        let path = pop_string_sftp(payload, &mut pos);

        let target = self
            .symlinks
            .lock()
            .unwrap()
            .get(&path)
            .cloned()
            .unwrap_or_default();
        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, 1);
        push_string_sftp(&mut response, &target);
        push_string_sftp(&mut response, &target);
        push_u32_sftp(&mut response, 0x00000004);
        push_u32_sftp(&mut response, 0o777);
        session.data(channel, frame_sftp(104, &response))?;
        Ok(())
    }

    fn handle_symlink(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.simple_status(channel, packet, session, 0)
    }

    fn simple_status(
        &self,
        channel: ChannelId,
        packet: &[u8],
        session: &mut Session,
        code: u32,
    ) -> Result<(), russh::Error> {
        let payload = &packet[1..];
        let mut pos = 0;
        let id = pop_u32_sftp(payload, &mut pos);

        let mut response = Vec::new();
        push_u32_sftp(&mut response, id);
        push_u32_sftp(&mut response, code);
        push_string_sftp(&mut response, "OK");
        push_string_sftp(&mut response, "en");
        session.data(channel, frame_sftp(101, &response))?;
        Ok(())
    }
}

// ── SFTP wire helpers (minimal, for mock use only) ────────────────────

fn frame_sftp(ptype: u8, payload: &[u8]) -> Vec<u8> {
    let len = 1 + payload.len() as u32;
    let mut buf = Vec::with_capacity(4 + len as usize);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.push(ptype);
    buf.extend_from_slice(payload);
    buf
}

fn push_u32_sftp(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn push_u64_sftp(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn push_string_sftp(buf: &mut Vec<u8>, s: &str) {
    push_bytes_sftp(buf, s.as_bytes());
}

fn push_bytes_sftp(buf: &mut Vec<u8>, data: &[u8]) {
    push_u32_sftp(buf, data.len() as u32);
    buf.extend_from_slice(data);
}

fn pop_u32_sftp(data: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    v
}

fn pop_u64_sftp(data: &[u8], pos: &mut usize) -> u64 {
    let v = u64::from_be_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ]);
    *pos += 8;
    v
}

fn pop_string_sftp(data: &[u8], pos: &mut usize) -> String {
    let bytes = pop_bytes_sftp(data, pos);
    String::from_utf8(bytes).unwrap_or_default()
}

fn pop_bytes_sftp(data: &[u8], pos: &mut usize) -> Vec<u8> {
    let len = pop_u32_sftp(data, pos) as usize;
    let bytes = data[*pos..*pos + len].to_vec();
    *pos += len;
    bytes
}
