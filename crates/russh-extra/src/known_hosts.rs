//! Known hosts file parsing, storage, and host-key verification.
//!
//! This module implements OpenSSH `known_hosts` file parsing and in-memory
//! lookup. The first runtime slice supports plain hostname entries,
//! `[host]:port` format, and host-key comparison via `PublicKey` matching.
//! Hashed hostname entries and wildcard patterns are deferred.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use russh::keys::ssh_key::PublicKey;
use russh_extra_core::{Error, HostKeyErrorKind, Result};

/// A parsed known-hosts entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownHostsEntry {
    hostname: String,
    port: u16,
    algorithm: String,
    key_blob: Vec<u8>,
    comment: Option<String>,
    revoked: bool,
}

impl KnownHostsEntry {
    /// Returns the hostname pattern.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Returns the port marker, or 0 when the line matches the default port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the key algorithm string (e.g. `ssh-ed25519`).
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    /// Returns the raw public key blob.
    pub fn key_blob(&self) -> &[u8] {
        &self.key_blob
    }

    /// Returns the optional trailing comment.
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    /// Returns whether the entry is marked `@revoked`.
    pub fn is_revoked(&self) -> bool {
        self.revoked
    }

    /// Parses a single `known_hosts` line.
    fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        let (revoked, rest) = parse_revoked_marker(trimmed);
        let (_cert_authority, rest) = parse_cert_authority_marker(rest);

        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }

        let (hostname, port) = parse_host_pattern(parts[0])?;
        let algorithm = parts[1].to_owned();
        let key_blob = match base64_decode_key(parts[2]) {
            Ok(blob) => blob,
            Err(_) => return None,
        };
        let comment = if parts.len() > 3 {
            Some(parts[3..].join(" "))
        } else {
            None
        };

        Some(KnownHostsEntry {
            hostname,
            port,
            algorithm,
            key_blob,
            comment,
            revoked,
        })
    }
}

fn parse_revoked_marker(line: &str) -> (bool, &str) {
    if let Some(rest) = line.strip_prefix("@revoked ") {
        (true, rest)
    } else {
        (false, line)
    }
}

fn parse_cert_authority_marker(line: &str) -> (bool, &str) {
    if let Some(rest) = line.strip_prefix("@cert-authority ") {
        (true, rest)
    } else {
        (false, line)
    }
}

fn parse_host_pattern(pattern: &str) -> Option<(String, u16)> {
    if let Some(inner) = pattern.strip_prefix('[') {
        if let Some(before_close) = inner.strip_suffix(']') {
            if let Some((host, port_str)) = before_close.rsplit_once(':') {
                let port: u16 = port_str.parse().ok()?;
                return Some((host.to_owned(), port));
            }
            return Some((before_close.to_owned(), 0));
        }
        if let Some((host, port_str)) = inner.rsplit_once("]:") {
            let port: u16 = port_str.parse().ok()?;
            let host = host.strip_suffix(']').unwrap_or(host);
            return Some((host.to_owned(), port));
        }
        return Some((inner.to_owned(), 0));
    }

    let parts: Vec<&str> = pattern.split(',').collect();
    let first = parts.first()?;

    Some(((*first).to_owned(), 0))
}

fn base64_decode_key(encoded: &str) -> Result<Vec<u8>, ()> {
    use std::io::Read;

    let mut decoder = base64::read::DecoderReader::new(
        encoded.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).map_err(|_| ())?;
    Ok(buf)
}

/// Status returned by a known-hosts lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownHostStatus {
    /// The host key matched a known entry.
    Match,
    /// No entry was found for this host.
    NotFound,
    /// A known entry exists but the key does not match.
    Changed,
    /// A known entry is marked `@revoked` for this host.
    Revoked,
}

/// Warning collected during known-hosts file parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownHostsParseWarning {
    /// Line number (1-based).
    pub line: usize,
    /// The raw line content.
    pub content: String,
    /// Description of the parse issue.
    pub reason: &'static str,
}

/// In-memory known-hosts store.
#[derive(Clone, Debug)]
pub struct KnownHosts {
    inner: Arc<RwLock<KnownHostsInner>>,
}

#[derive(Debug)]
struct KnownHostsInner {
    entries: Vec<KnownHostsEntry>,
    warnings: Vec<KnownHostsParseWarning>,
    source_paths: Vec<PathBuf>,
    hash_hostnames: bool,
}

impl KnownHosts {
    /// Creates an empty known-hosts store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(KnownHostsInner {
                entries: Vec::new(),
                warnings: Vec::new(),
                source_paths: Vec::new(),
                hash_hostnames: false,
            })),
        }
    }

    /// Loads known-hosts entries from a file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = expand_tilde(path.as_ref());
        validate_known_hosts_permissions(&path)?;
        let content = std::fs::read_to_string(&path)?;
        Ok(Self::parse(&path, &content))
    }

    /// Loads entries from multiple files.
    pub fn load_files(paths: &[impl AsRef<Path>]) -> Result<Self> {
        let store = Self::new();
        for path in paths {
            let contents = KnownHosts::load(path)?;
            store.merge(contents);
        }
        Ok(store)
    }

    /// Parses known-hosts content and records the source path.
    fn parse(path: &Path, content: &str) -> Self {
        let (entries, warnings) = parse_known_hosts_content(content);

        Self {
            inner: Arc::new(RwLock::new(KnownHostsInner {
                entries,
                warnings,
                source_paths: vec![path.to_owned()],
                hash_hostnames: false,
            })),
        }
    }

    /// Merges entries from another store into this one.
    pub fn merge(&self, other: KnownHosts) {
        let mut inner = self.inner.write().expect("known-hosts lock poisoned");
        let other_inner = other.inner.read().expect("known-hosts lock poisoned");
        inner.entries.extend(other_inner.entries.clone());
        inner.warnings.extend(other_inner.warnings.clone());
        inner.source_paths.extend(other_inner.source_paths.clone());
    }

    /// Checks a host key against the store.
    pub fn check(&self, host: &str, port: u16, public_key: &PublicKey) -> KnownHostStatus {
        let inner = self.inner.read().expect("known-hosts lock poisoned");
        let key_bytes = public_key_to_bytes(public_key);
        let Some(key_bytes) = key_bytes else {
            return KnownHostStatus::NotFound;
        };

        for entry in &inner.entries {
            if !host_matches(&entry.hostname, host) {
                continue;
            }
            if entry.port != 0 && entry.port != port {
                continue;
            }

            if entry.revoked {
                return KnownHostStatus::Revoked;
            }

            if entry.key_blob == key_bytes {
                return KnownHostStatus::Match;
            }

            return KnownHostStatus::Changed;
        }

        KnownHostStatus::NotFound
    }

    /// Adds or updates an entry for a host.
    pub fn add_entry(
        &self,
        host: &str,
        port: u16,
        public_key: &PublicKey,
        algorithm: &str,
    ) -> Result<()> {
        let key_blob = public_key_to_bytes(public_key).ok_or_else(|| {
            Error::host_key(
                HostKeyErrorKind::Unsupported,
                "failed to serialize public key for known-hosts entry",
            )
        })?;

        let mut inner = self.inner.write().expect("known-hosts lock poisoned");

        inner.entries.retain(|entry| {
            !(host_matches(&entry.hostname, host)
                && (entry.port == port || entry.port == 0 || port == 0 || port == 22))
        });

        inner.entries.push(KnownHostsEntry {
            hostname: host.to_owned(),
            port,
            algorithm: algorithm.to_owned(),
            key_blob,
            comment: None,
            revoked: false,
        });

        Ok(())
    }

    /// Returns parse warnings collected during loading.
    pub fn warnings(&self) -> Vec<KnownHostsParseWarning> {
        let inner = self.inner.read().expect("known-hosts lock poisoned");
        inner.warnings.clone()
    }

    /// Returns the number of entries in the store.
    pub fn entry_count(&self) -> usize {
        let inner = self.inner.read().expect("known-hosts lock poisoned");
        inner.entries.len()
    }

    /// Sets whether new entries should use hashed hostnames.
    pub fn set_hash_hostnames(&mut self, hash: bool) {
        let mut inner = self.inner.write().expect("known-hosts lock poisoned");
        inner.hash_hostnames = hash;
    }

    /// Saves the store to a file in OpenSSH known-hosts format.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = expand_tilde(path.as_ref());
        let inner = self.inner.read().expect("known-hosts lock poisoned");
        let mut output = String::new();

        for entry in &inner.entries {
            let host_part = if entry.port != 0 && entry.port != 22 {
                format!("[{}]:{}", entry.hostname, entry.port)
            } else {
                entry.hostname.clone()
            };

            let marker = if entry.revoked { "@revoked " } else { "" };
            let comment = entry
                .comment
                .as_deref()
                .map(|c| format!(" {c}"))
                .unwrap_or_default();
            let key_b64 = base64_encode(&entry.key_blob);

            output.push_str(&format!(
                "{marker}{host_part} {} {key_b64}{comment}\n",
                entry.algorithm
            ));
        }

        let mut file = std::fs::File::create(&path)?;
        use std::io::Write;
        file.write_all(output.as_bytes())?;
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |_| {
                    Error::invalid_config(format!(
                        "failed to set permissions on known-hosts file `{}`",
                        path.display()
                    ))
                },
            )?;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }

        Ok(())
    }

    /// Returns the source file paths that were loaded.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        let inner = self.inner.read().expect("known-hosts lock poisoned");
        inner.source_paths.clone()
    }
}

impl Default for KnownHosts {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_known_hosts_content(content: &str) -> (Vec<KnownHostsEntry>, Vec<KnownHostsParseWarning>) {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('|') {
            warnings.push(KnownHostsParseWarning {
                line: index + 1,
                content: trimmed.to_owned(),
                reason: "hashed hostname entries are not yet supported",
            });
            continue;
        }

        if trimmed.starts_with("@cert-authority ") {
            warnings.push(KnownHostsParseWarning {
                line: index + 1,
                content: trimmed.to_owned(),
                reason: "cert-authority entries are not yet supported",
            });
            continue;
        }

        if let Some(entry) = KnownHostsEntry::parse(trimmed) {
            entries.push(entry);
        } else {
            warnings.push(KnownHostsParseWarning {
                line: index + 1,
                content: trimmed.to_owned(),
                reason: "failed to parse known-hosts line",
            });
        }
    }

    (entries, warnings)
}

fn host_matches(entry_host: &str, target_host: &str) -> bool {
    if entry_host.eq_ignore_ascii_case(target_host) {
        return true;
    }

    false
}

fn public_key_to_bytes(public_key: &PublicKey) -> Option<Vec<u8>> {
    public_key.to_bytes().ok()
}

fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;

    let mut buf = Vec::new();
    {
        let mut encoder =
            base64::write::EncoderWriter::new(&mut buf, &base64::engine::general_purpose::STANDARD);
        encoder.write_all(data).ok();
    }

    String::from_utf8(buf).unwrap_or_default()
}

fn validate_known_hosts_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path)?;
        let mode = metadata.permissions().mode();
        if mode & 0o022 != 0 {
            return Err(Error::invalid_config(format!(
                "known-hosts file `{}` must not be writable by group or others",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_file_owner_only(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[cfg(not(unix))]
    fn make_file_owner_only(_path: &std::path::Path) {}

    #[test]
    fn parses_plain_hostname_entry() {
        let key_b64 = "AAAAC3NzaC1lZDI1NTE5AAAAIGk8abcdefghi";
        let encoded = base64_encode(b"real-key-data");
        let line = format!("example.com ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "example.com");
        assert_eq!(entry.port(), 0);
        assert_eq!(entry.algorithm(), "ssh-ed25519");
        assert!(!entry.key_blob().is_empty());
        assert!(!entry.is_revoked());
        let _ = key_b64;
    }

    #[test]
    fn parses_bracketed_port_entry() {
        let encoded = base64_encode(b"real-key-data");
        let line = format!("[example.com]:2222 ssh-rsa {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "example.com");
        assert_eq!(entry.port(), 2222);
        assert_eq!(entry.algorithm(), "ssh-rsa");
    }

    #[test]
    fn parses_revoked_entry() {
        let encoded = base64_encode(b"revoked-key-data");
        let line = format!("@revoked example.com ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert!(entry.is_revoked());
    }

    #[test]
    fn parses_entry_with_comment() {
        let encoded = base64_encode(b"key-with-comment");
        let line = format!("example.com ssh-ed25519 {encoded} extra info here");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.comment(), Some("extra info here"));
    }

    #[test]
    fn skips_empty_and_comment_lines() {
        assert!(KnownHostsEntry::parse("").is_none());
        assert!(KnownHostsEntry::parse("  ").is_none());
        assert!(KnownHostsEntry::parse("# example.com ssh-ed25519 key").is_none());
    }

    #[test]
    fn skips_hashed_hostname_entries() {
        let content = "|1|abc|def ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGk8abcdef\n";
        let (entries, warnings) = parse_known_hosts_content(content);

        assert!(entries.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("hashed"));
    }

    #[test]
    fn skips_cert_authority_entries() {
        let encoded = base64_encode(b"ca-key");
        let content = format!("@cert-authority example.com ssh-ed25519 {encoded}\n");
        let (entries, warnings) = parse_known_hosts_content(&content);

        assert!(entries.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("cert-authority"));
    }

    #[test]
    fn parses_multiple_valid_entries_and_collects_warnings() {
        let encoded = base64_encode(b"multi-key");
        let content = format!(
            "\
            example.com ssh-ed25519 {encoded}\n\
            [host]:2222 ssh-rsa {encoded}\n\
            |1|xxx|yyy ssh-ed25519 {encoded}\n\
            bad line without enough parts\n\
            # comment\n\
            "
        );

        let (entries, warnings) = parse_known_hosts_content(&content);

        assert_eq!(entries.len(), 2);
        assert_eq!(warnings.len(), 2);
        assert_eq!(entries[0].hostname(), "example.com");
        assert_eq!(entries[1].hostname(), "host");
        assert_eq!(entries[1].port(), 2222);
    }

    #[test]
    fn new_store_is_empty() {
        let store = KnownHosts::new();

        assert_eq!(store.entry_count(), 0);
        assert!(store.warnings().is_empty());
    }

    #[test]
    fn store_starts_empty_and_accepts_added_entry() {
        let store = KnownHosts::new();

        assert_eq!(store.entry_count(), 0);
    }

    #[test]
    fn added_port_specific_entry_matches_plain_hostname() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();

        store
            .add_entry("example.com", 2222, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("example.com", 2222, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn debug_of_entry_excludes_sensitive_data() {
        let entry = KnownHostsEntry {
            hostname: "example.com".into(),
            port: 0,
            algorithm: "ssh-ed25519".into(),
            key_blob: b"fake-key-data".to_vec(),
            comment: None,
            revoked: false,
        };

        let debug = format!("{entry:?}");

        assert!(debug.contains("example.com"));
        assert!(debug.contains("ssh-ed25519"));
    }

    #[test]
    fn known_hosts_clone_shares_store() {
        let store = KnownHosts::new();
        let clone = store.clone();

        assert_eq!(store.entry_count(), clone.entry_count());
        assert_eq!(store.entry_count(), 0);
    }

    #[test]
    fn parses_comma_separated_hostname_line() {
        let encoded = base64_encode(b"comma-sep-key");
        let line = format!("host-a,host-b,host-c ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "host-a");
    }

    #[test]
    fn parses_ip_address_entry() {
        let encoded = base64_encode(b"ip-addr-key");
        let line = format!("192.168.1.1 ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "192.168.1.1");
        assert_eq!(entry.port(), 0);
    }

    #[test]
    fn parses_ecdsa_algorithm_entry() {
        let encoded = base64_encode(b"ecdsa-key-data");
        let line = format!("example.com ecdsa-sha2-nistp256 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.algorithm(), "ecdsa-sha2-nistp256");
    }

    #[test]
    fn rejects_line_with_missing_key_type() {
        let line = "example.com";
        assert!(KnownHostsEntry::parse(line).is_none());
    }

    #[test]
    fn rejects_line_with_invalid_base64() {
        let line = "example.com ssh-ed25519 not-valid-base64!!!";
        assert!(KnownHostsEntry::parse(line).is_none());
    }

    #[test]
    fn rejects_line_with_only_tag_and_key() {
        let encoded = base64_encode(b"only-two-parts");
        let line = format!("ssh-ed25519 {encoded}");
        assert!(KnownHostsEntry::parse(&line).is_none());
    }

    #[test]
    fn host_match_exact() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("example.com", 22, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn host_match_case_insensitive() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("EXAMPLE.COM", 22, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn host_match_non_matching() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("other.com", 22, &public_key),
            KnownHostStatus::NotFound
        );
    }

    #[test]
    fn host_match_port_specific_only_matches_that_port() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 2222, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("example.com", 2222, &public_key),
            KnownHostStatus::Match
        );
        assert_eq!(
            store.check("example.com", 22, &public_key),
            KnownHostStatus::NotFound
        );
    }

    #[test]
    fn host_match_port_zero_matches_any_port() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("example.com", 22, &public_key),
            KnownHostStatus::Match
        );
        assert_eq!(
            store.check("example.com", 2222, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn key_match_different_key_same_algorithm_is_changed() {
        let private_key1 =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key1 = private_key1.public_key().clone();

        let private_key2 =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key2 = private_key2.public_key().clone();

        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key1, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("example.com", 22, &public_key2),
            KnownHostStatus::Changed
        );
    }

    #[test]
    fn key_match_different_algorithm() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();

        let store = KnownHosts::new();
        store
            .add_entry("example.com", 0, &public_key, "ssh-rsa")
            .unwrap();

        assert_eq!(
            store.check("example.com", 22, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn revoked_entry_returns_revoked_status() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        let key_blob = public_key.to_bytes().unwrap();

        store.inner.write().unwrap().entries.push(KnownHostsEntry {
            hostname: "evil.com".into(),
            port: 0,
            algorithm: "ssh-ed25519".into(),
            key_blob,
            comment: None,
            revoked: true,
        });

        assert_eq!(
            store.check("evil.com", 22, &public_key),
            KnownHostStatus::Revoked
        );
    }

    #[test]
    fn save_format_plain_hostname_for_port_22() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 22, &public_key, "ssh-ed25519")
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        store.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("example.com "));
        assert!(!content.contains("[example.com]:22"));
    }

    #[test]
    fn save_format_bracketed_hostname_for_non_default_port() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 2222, &public_key, "ssh-ed25519")
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        store.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[example.com]:2222"));
    }

    #[test]
    fn save_format_includes_revoked_marker() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let key_blob = public_key.to_bytes().unwrap();
        let store = KnownHosts::new();
        store.inner.write().unwrap().entries.push(KnownHostsEntry {
            hostname: "revoked-host.com".into(),
            port: 0,
            algorithm: "ssh-ed25519".into(),
            key_blob,
            comment: None,
            revoked: true,
        });

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        store.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("@revoked "));
    }

    #[test]
    fn save_and_load_round_trip() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");

        {
            let store = KnownHosts::new();
            store
                .add_entry("example.com", 2222, &public_key, "ssh-ed25519")
                .unwrap();
            store
                .add_entry("other.com", 0, &public_key, "ssh-rsa")
                .unwrap();
            store.save(&path).unwrap();
        }

        let loaded = KnownHosts::load(&path).unwrap();
        assert_eq!(loaded.entry_count(), 2);

        assert_eq!(
            loaded.check("example.com", 2222, &public_key),
            KnownHostStatus::Match
        );
        assert_eq!(
            loaded.check("other.com", 22, &public_key),
            KnownHostStatus::Match
        );
    }

    #[test]
    fn save_and_load_preserves_revoked() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let key_blob = public_key.to_bytes().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");

        {
            let store = KnownHosts::new();
            store.inner.write().unwrap().entries.push(KnownHostsEntry {
                hostname: "evil.com".into(),
                port: 0,
                algorithm: "ssh-ed25519".into(),
                key_blob,
                comment: None,
                revoked: true,
            });
            store.save(&path).unwrap();
        }

        let loaded = KnownHosts::load(&path).unwrap();
        assert_eq!(loaded.entry_count(), 1);
        assert_eq!(
            loaded.check("evil.com", 22, &public_key),
            KnownHostStatus::Revoked
        );
    }

    #[test]
    fn source_paths_recorded_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let encoded = base64_encode(b"source-path-key");
        std::fs::write(&path, format!("example.com ssh-ed25519 {encoded}\n")).unwrap();
        make_file_owner_only(&path);

        let store = KnownHosts::load(&path).unwrap();
        let paths = store.source_paths();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], path);
    }

    #[test]
    fn warnings_from_file_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let encoded = base64_encode(b"warning-key");
        std::fs::write(
            &path,
            format!(
                "\
                    example.com ssh-ed25519 {encoded}\n\
                    |1|xxx|yyy ssh-ed25519 {encoded}\n\
                    @cert-authority ca.com ssh-ed25519 {encoded}\n\
                    bad line\n\
                    "
            ),
        )
        .unwrap();
        make_file_owner_only(&path);

        let store = KnownHosts::load(&path).unwrap();

        assert_eq!(store.entry_count(), 1);

        let warnings = store.warnings();
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].reason.contains("hashed"));
        assert!(warnings[1].reason.contains("cert-authority"));
        assert!(warnings[2].reason.contains("failed to parse"));
    }

    #[test]
    fn load_merge_and_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let path1 = dir.path().join("kh1");
        let path2 = dir.path().join("kh2");
        let encoded1 = base64_encode(b"merge-key-1");
        let encoded2 = base64_encode(b"merge-key-2");

        std::fs::write(&path1, format!("host-a.com ssh-ed25519 {encoded1}\n")).unwrap();
        std::fs::write(&path2, format!("host-b.com ssh-ed25519 {encoded2}\n")).unwrap();
        make_file_owner_only(&path1);
        make_file_owner_only(&path2);

        let store1 = KnownHosts::load(&path1).unwrap();
        let store2 = KnownHosts::load(&path2).unwrap();
        store1.merge(store2);

        assert_eq!(store1.entry_count(), 2);

        let merged_paths = store1.source_paths();
        assert_eq!(merged_paths.len(), 2);
    }

    #[test]
    fn large_number_of_entries() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();
        let count = 200;

        for i in 0..count {
            let host = format!("host-{i}.example.com");
            store
                .add_entry(&host, 0, &public_key, "ssh-ed25519")
                .unwrap();
        }

        assert_eq!(store.entry_count(), count);

        for i in 0..count {
            let host = format!("host-{i}.example.com");
            assert_eq!(store.check(&host, 22, &public_key), KnownHostStatus::Match);
        }
    }

    #[test]
    fn load_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, "").unwrap();
        make_file_owner_only(&path);

        let store = KnownHosts::load(&path).unwrap();
        assert_eq!(store.entry_count(), 0);
        assert!(store.warnings().is_empty());
    }

    #[test]
    fn load_file_with_only_comments_and_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(
            &path,
            "\
                # comment one\n\
                \n\
                # comment two\n\
                \n\
                ",
        )
        .unwrap();
        make_file_owner_only(&path);

        let store = KnownHosts::load(&path).unwrap();
        assert_eq!(store.entry_count(), 0);
        assert!(store.warnings().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_group_writable_file_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let encoded = base64_encode(b"perm-key");
        std::fs::write(&path, format!("example.com ssh-ed25519 {encoded}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();

        let result = KnownHosts::load(&path);
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn save_creates_file_with_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHosts::new();
        store
            .add_entry("example.com", 22, &public_key, "ssh-ed25519")
            .unwrap();
        store.save(&path).unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    fn home_dir() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME").ok()
        }
    }

    #[test]
    fn tilde_expansion_to_home() {
        let home = home_dir().expect("HOME or USERPROFILE not set");
        let result = expand_tilde(std::path::Path::new("~/.ssh/known_hosts"));

        assert_eq!(
            result,
            std::path::PathBuf::from(&home).join(".ssh/known_hosts")
        );
    }

    #[test]
    fn tilde_alone_expands_to_home() {
        let home = home_dir().expect("HOME or USERPROFILE not set");
        let result = expand_tilde(std::path::Path::new("~"));

        assert_eq!(result, std::path::PathBuf::from(&home));
    }

    #[test]
    fn tilde_not_expanded_without_slash() {
        let result = expand_tilde(std::path::Path::new("~user"));

        assert_eq!(result, std::path::PathBuf::from("~user"));
    }

    #[test]
    fn tilde_not_expanded_in_middle_of_path() {
        let result = expand_tilde(std::path::Path::new("/home/~user/ssh"));

        assert_eq!(result, std::path::PathBuf::from("/home/~user/ssh"));
    }

    #[test]
    fn known_hosts_entry_accessors() {
        let entry = KnownHostsEntry {
            hostname: "accessor-test.com".into(),
            port: 2222,
            algorithm: "ssh-rsa".into(),
            key_blob: b"accessor-key-data".to_vec(),
            comment: Some("a comment".into()),
            revoked: true,
        };

        assert_eq!(entry.hostname(), "accessor-test.com");
        assert_eq!(entry.port(), 2222);
        assert_eq!(entry.algorithm(), "ssh-rsa");
        assert_eq!(entry.key_blob(), b"accessor-key-data");
        assert_eq!(entry.comment(), Some("a comment"));
        assert!(entry.is_revoked());
    }

    #[test]
    fn known_hosts_entry_accessor_no_comment() {
        let entry = KnownHostsEntry {
            hostname: "no-comment.com".into(),
            port: 0,
            algorithm: "ssh-ed25519".into(),
            key_blob: b"no-comment-key".to_vec(),
            comment: None,
            revoked: false,
        };

        assert_eq!(entry.comment(), None);
        assert!(!entry.is_revoked());
    }

    #[test]
    fn known_hosts_entry_round_trip_through_parse() {
        let encoded = base64_encode(b"round-trip-key-data");
        let line = format!("@revoked [example.com]:2222 ssh-ed25519 {encoded} my comment");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "example.com");
        assert_eq!(entry.port(), 2222);
        assert_eq!(entry.algorithm(), "ssh-ed25519");
        assert_eq!(entry.key_blob(), b"round-trip-key-data");
        assert_eq!(entry.comment(), Some("my comment"));
        assert!(entry.is_revoked());
    }

    #[test]
    fn known_hosts_set_hash_hostnames() {
        let mut store = KnownHosts::new();
        assert!(!store.inner.read().unwrap().hash_hostnames);

        store.set_hash_hostnames(true);
        assert!(store.inner.read().unwrap().hash_hostnames);
    }

    #[test]
    fn ip_address_bracketed_port() {
        let encoded = base64_encode(b"ip-bracket-key");
        let line = format!("[192.168.1.1]:2222 ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "192.168.1.1");
        assert_eq!(entry.port(), 2222);
    }

    #[test]
    fn wildcard_looking_entry_does_not_match_unrelated_hosts() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let store = KnownHosts::new();

        let encoded = base64_encode(&public_key.to_bytes().unwrap());
        let line = format!("*.example.com ssh-ed25519 {encoded}");
        let entry = KnownHostsEntry::parse(&line).unwrap();

        assert_eq!(entry.hostname(), "*.example.com");

        store
            .add_entry("*.example.com", 0, &public_key, "ssh-ed25519")
            .unwrap();

        assert_eq!(
            store.check("*.example.com", 22, &public_key),
            KnownHostStatus::Match,
            "exact wildcard string should match itself"
        );
        assert_eq!(
            store.check("foo.example.com", 22, &public_key),
            KnownHostStatus::NotFound,
            "subdomain should not match wildcard-looking entry"
        );
        assert_eq!(
            store.check("example.com", 22, &public_key),
            KnownHostStatus::NotFound,
            "bare domain should not match wildcard-looking entry"
        );
    }

    #[test]
    fn hashed_entry_skipped_with_warning_and_not_trusted() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let key_encoded = base64_encode(&public_key.to_bytes().unwrap());

        let content = format!(
            "example.com ssh-ed25519 {key_encoded}\n|1|abc123|def456 ssh-ed25519 {key_encoded}\n"
        );

        let (entries, warnings) = parse_known_hosts_content(&content);

        assert_eq!(entries.len(), 1, "hashed entry should not be parsed");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("hashed"));
        assert_eq!(entries[0].hostname(), "example.com");
    }

    #[test]
    fn malformed_lines_collected_as_warnings_not_trusted() {
        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();
        let key_encoded = base64_encode(&public_key.to_bytes().unwrap());

        let content = format!(
            "incomplete\n\
             example.com ssh-ed25519 {key_encoded}\n\
             another:bad!line!!\n\
             "
        );

        let (entries, warnings) = parse_known_hosts_content(&content);

        assert_eq!(entries.len(), 1);
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .all(|w| w.reason.contains("failed to parse"))
        );
    }

    #[test]
    fn set_hash_hostnames_is_documented_as_not_active() {
        let mut store = KnownHosts::new();
        assert!(!store.inner.read().unwrap().hash_hostnames);

        store.set_hash_hostnames(true);
        assert!(store.inner.read().unwrap().hash_hostnames);

        let private_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let public_key = private_key.public_key().clone();

        store
            .add_entry("example.com", 22, &public_key, "ssh-ed25519")
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        store.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("example.com ssh-ed25519"),
            "hostname should be plain text even when hash_hostnames is set (hashing not yet implemented)"
        );
    }
}
