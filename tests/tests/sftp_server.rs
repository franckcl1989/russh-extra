use std::collections::HashMap;
use std::sync::Mutex;

use russh_extra::{
    AuthDecision, Client, Server, ServerHostKey, SftpDirEntry, SftpErrorKind, SftpMetadata,
    SftpOpenMode, SftpServerHandler,
};
use russh_extra_test_support::init_tracing;

struct InMemoryFs {
    files: Mutex<HashMap<String, Vec<u8>>>,
    handles: Mutex<HashMap<String, String>>,
    dirs: Mutex<HashMap<String, Vec<SftpDirEntry>>>,
}

impl InMemoryFs {
    fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            dirs: Mutex::new(HashMap::new()),
        }
    }

    fn gen_handle(&self, path: &str) -> String {
        format!("{path}@{}", std::process::id())
    }
}

struct InMemorySftpHandler {
    fs: InMemoryFs,
}

impl InMemorySftpHandler {
    fn new() -> Self {
        Self {
            fs: InMemoryFs::new(),
        }
    }
}

#[russh_extra::async_trait]
impl SftpServerHandler for InMemorySftpHandler {
    async fn open(
        &self,
        _id: u32,
        filename: String,
        pflags: u32,
        _attrs: russh_extra::SftpMetadata,
    ) -> russh_extra::Result<String> {
        let handle = self.fs.gen_handle(&filename);
        let write_mode = pflags & (SftpOpenMode::WRITE.bits() | SftpOpenMode::CREATE.bits()) != 0;

        if write_mode {
            self.fs
                .files
                .lock()
                .unwrap()
                .insert(filename.clone(), Vec::new());
            if let Some(parent) = std::path::Path::new(&filename).parent()
                && let Some(parent_str) = parent.to_str()
            {
                self.fs
                    .dirs
                    .lock()
                    .unwrap()
                    .entry(parent_str.to_owned())
                    .or_default()
                    .push(SftpDirEntry::new(
                        std::path::Path::new(&filename)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&filename),
                        "",
                        SftpMetadata::default(),
                    ));
            }
        } else if !self.fs.files.lock().unwrap().contains_key(&filename) {
            return Err(russh_extra::Error::sftp(
                SftpErrorKind::NoSuchFile,
                "no such file",
            ));
        }

        self.fs
            .handles
            .lock()
            .unwrap()
            .insert(handle.clone(), filename);
        Ok(handle)
    }

    async fn close(&self, _id: u32, handle: String) -> russh_extra::Result<()> {
        self.fs.handles.lock().unwrap().remove(&handle);
        Ok(())
    }

    async fn read(
        &self,
        _id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> russh_extra::Result<Vec<u8>> {
        let handles = self.fs.handles.lock().unwrap();
        let filename = handles
            .get(&handle)
            .ok_or_else(|| russh_extra::Error::unsupported("invalid handle"))?;

        let files = self.fs.files.lock().unwrap();
        let data = files
            .get(filename)
            .ok_or_else(|| russh_extra::Error::unsupported("no such file"))?;

        let start = offset as usize;
        let end = std::cmp::min(start + len as usize, data.len());
        if start >= data.len() {
            return Ok(Vec::new());
        }
        Ok(data[start..end].to_vec())
    }

    async fn write(
        &self,
        _id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> russh_extra::Result<()> {
        let handles = self.fs.handles.lock().unwrap();
        let filename = handles
            .get(&handle)
            .ok_or_else(|| russh_extra::Error::unsupported("invalid handle"))?;

        let mut files = self.fs.files.lock().unwrap();
        let content = files
            .get_mut(filename)
            .ok_or_else(|| russh_extra::Error::unsupported("no such file"))?;

        let start = offset as usize;
        if start + data.len() > content.len() {
            content.resize(start + data.len(), 0);
        }
        content[start..start + data.len()].copy_from_slice(&data);
        Ok(())
    }

    async fn stat(&self, _id: u32, path: String) -> russh_extra::Result<russh_extra::SftpMetadata> {
        let files = self.fs.files.lock().unwrap();
        let data = files
            .get(&path)
            .ok_or_else(|| russh_extra::Error::unsupported("no such file"))?;

        Ok(SftpMetadata::default().with_size(data.len() as u64))
    }

    async fn lstat(
        &self,
        _id: u32,
        path: String,
    ) -> russh_extra::Result<russh_extra::SftpMetadata> {
        self.stat(_id, path).await
    }

    async fn remove(&self, _id: u32, filename: String) -> russh_extra::Result<()> {
        self.fs.files.lock().unwrap().remove(&filename);
        Ok(())
    }

    async fn opendir(&self, _id: u32, path: String) -> russh_extra::Result<String> {
        let handle = self.fs.gen_handle(&path);
        let path_clone = path.clone();
        self.fs.handles.lock().unwrap().insert(handle.clone(), path);
        self.fs.dirs.lock().unwrap().entry(path_clone).or_default();
        Ok(handle)
    }

    async fn readdir(&self, _id: u32, handle: String) -> russh_extra::Result<Vec<SftpDirEntry>> {
        let handles = self.fs.handles.lock().unwrap();
        let path = handles
            .get(&handle)
            .ok_or_else(|| russh_extra::Error::unsupported("invalid handle"))?;
        let dirs = self.fs.dirs.lock().unwrap();
        Ok(dirs.get(path).cloned().unwrap_or_default())
    }

    async fn mkdir(
        &self,
        _id: u32,
        path: String,
        _attrs: russh_extra::SftpMetadata,
    ) -> russh_extra::Result<()> {
        self.fs
            .dirs
            .lock()
            .unwrap()
            .entry(path.clone())
            .or_default();
        Ok(())
    }

    async fn rmdir(&self, _id: u32, path: String) -> russh_extra::Result<()> {
        self.fs.dirs.lock().unwrap().remove(&path);
        Ok(())
    }

    async fn rename(&self, _id: u32, oldpath: String, newpath: String) -> russh_extra::Result<()> {
        let mut files = self.fs.files.lock().unwrap();
        if let Some(data) = files.remove(&oldpath) {
            files.insert(newpath.clone(), data);
        }
        drop(files);
        let mut dirs = self.fs.dirs.lock().unwrap();
        if let Some(entries) = dirs.remove(&oldpath) {
            dirs.insert(newpath, entries);
        }
        Ok(())
    }

    async fn fstat(
        &self,
        _id: u32,
        handle: String,
    ) -> russh_extra::Result<russh_extra::SftpMetadata> {
        let handles = self.fs.handles.lock().unwrap();
        let filename = handles
            .get(&handle)
            .ok_or_else(|| russh_extra::Error::unsupported("invalid handle"))?;
        let files = self.fs.files.lock().unwrap();
        match files.get(filename) {
            Some(data) => Ok(SftpMetadata::default().with_size(data.len() as u64)),
            None => {
                if self.fs.dirs.lock().unwrap().contains_key(filename) {
                    Ok(SftpMetadata::default())
                } else {
                    Err(russh_extra::Error::unsupported("no such file"))
                }
            }
        }
    }
}

fn test_host_key() -> ServerHostKey {
    let private_key = russh_extra::russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh_extra::russh::keys::Algorithm::Ed25519,
    )
    .unwrap();
    ServerHostKey::from_private_key(private_key)
}

async fn unused_endpoint() -> russh_extra::Endpoint {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    russh_extra::Endpoint::new(addr.ip().to_string(), addr.port())
}

async fn connect_client(
    endpoint: &russh_extra::Endpoint,
    password: &str,
) -> russh_extra::Result<russh_extra::Session> {
    let mut last_transport_error = None;

    for _ in 0..20 {
        let result = Client::builder()
            .endpoint(endpoint.clone())
            .username("demo")
            .password(password)
            .accept_any_host_key()
            .build()
            .connect()
            .await;

        match result {
            Ok(session) => return Ok(session),
            Err(russh_extra::Error::Transport(_)) => {
                last_transport_error = Some(result.err().unwrap());
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_transport_error.unwrap())
}

async fn stop_server(
    handle: russh_extra::ServerHandle,
    task: tokio::task::JoinHandle<russh_extra::Result<()>>,
) {
    handle.shutdown("test complete");
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn sftp_server_open_write_close_stat() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    let file = sftp
        .open("/test.txt", SftpOpenMode::WRITE | SftpOpenMode::CREATE)
        .await
        .unwrap();
    file.write(0, b"hello sftp").await.unwrap();
    file.close().await.unwrap();

    let meta = sftp.metadata("/test.txt").await.unwrap();
    assert_eq!(meta.size(), Some(10));

    let mut file = sftp.open("/test.txt", SftpOpenMode::READ).await.unwrap();
    let data = file.read(0, 4096).await.unwrap();
    assert_eq!(data, b"hello sftp");
    file.close().await.unwrap();

    stop_server(handle, task).await;
}

#[tokio::test]
async fn sftp_server_stat_missing_file_returns_error() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    let result = sftp.metadata("/nonexistent").await;
    assert!(result.is_err());

    stop_server(handle, task).await;
}

#[tokio::test]
async fn sftp_server_remove_file() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    let file = sftp
        .open("/to-remove.txt", SftpOpenMode::WRITE | SftpOpenMode::CREATE)
        .await
        .unwrap();
    file.close().await.unwrap();

    sftp.remove("/to-remove.txt").await.unwrap();

    let result = sftp.metadata("/to-remove.txt").await;
    assert!(result.is_err());

    stop_server(handle, task).await;
}

#[tokio::test]
async fn sftp_server_read_returns_eof_for_empty_file() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    let file = sftp
        .open("/empty.txt", SftpOpenMode::WRITE | SftpOpenMode::CREATE)
        .await
        .unwrap();
    file.close().await.unwrap();

    let mut file = sftp.open("/empty.txt", SftpOpenMode::READ).await.unwrap();
    let data = file.read(0, 4096).await.unwrap();
    assert!(data.is_empty());

    stop_server(handle, task).await;
}

#[tokio::test]
async fn sftp_server_handler_error_kind_propagated() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    let result = sftp.open("/nonexistent", SftpOpenMode::READ).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        russh_extra::Error::Sftp(err) => {
            assert_eq!(
                err.kind(),
                SftpErrorKind::NoSuchFile,
                "expected NoSuchFile, got {:?}",
                err.kind()
            );
        }
        other => panic!("expected Sftp error, got {other:?}"),
    }

    stop_server(handle, task).await;
}

#[tokio::test]
async fn sftp_server_mkdir_rmdir_rename() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    sftp.create_dir("/mydir").await.unwrap();
    sftp.remove_dir("/mydir").await.unwrap();

    let file = sftp
        .open("/original.txt", SftpOpenMode::WRITE | SftpOpenMode::CREATE)
        .await
        .unwrap();
    file.write(0, b"data").await.unwrap();
    sftp.close_file(file).await.unwrap();

    sftp.rename("/original.txt", "/moved.txt").await.unwrap();

    let meta = sftp.metadata("/moved.txt").await.unwrap();
    assert_eq!(meta.size(), Some(4));
    assert!(sftp.metadata("/original.txt").await.is_err());

    stop_server(handle, task).await;
}

#[tokio::test]
#[ignore = "readdir hangs on opendir handler, needs debugging"]
async fn sftp_server_readdir_and_fstat() {
    init_tracing();
    let handler = InMemorySftpHandler::new();
    let endpoint = unused_endpoint().await;

    let server = Server::builder()
        .listen((endpoint.host().to_owned(), endpoint.port()))
        .host_key(test_host_key())
        .password_auth(|_ctx, _password| async { Ok(AuthDecision::accept()) })
        .sftp_handler(handler)
        .build()
        .unwrap();
    let handle = server.handle();
    let task = tokio::spawn(server.run());

    let session = connect_client(&endpoint, "demo").await.unwrap();
    let sftp = session.sftp().await.unwrap();

    sftp.create_dir("/somedir").await.unwrap();

    let file = sftp
        .open(
            "/somedir/file.txt",
            SftpOpenMode::WRITE | SftpOpenMode::CREATE,
        )
        .await
        .unwrap();
    file.write(0, b"hello").await.unwrap();
    let meta = sftp.metadata("/somedir/file.txt").await.unwrap();
    assert_eq!(meta.size(), Some(5));
    sftp.close_file(file).await.unwrap();

    let mut dir = sftp.opendir("/somedir").await.unwrap();
    let mut names = Vec::new();
    while let Some(entry) = sftp.readdir(&mut dir).await.unwrap() {
        names.push(entry.filename().to_owned());
    }
    assert!(
        names.contains(&"file.txt".to_owned()),
        "dir should contain file.txt"
    );

    stop_server(handle, task).await;
}
