//! FTP and FTPS client used by both the anySCP app (file explorer and
//! transfer queue) and anyscp-mcp (Claude's tools).
//!
//! One [`FtpClient`] is one control connection. FTP allows a single transfer
//! per control connection, so callers that want parallel transfers open one
//! client per transfer.

mod tls;

use std::str::FromStr;
use std::time::{Duration, SystemTime};
use suppaftp::list::{File as ListedFile, ListParser, PosixPexQuery};
use suppaftp::tokio::AsyncRustlsFtpStream;
use suppaftp::types::FileType;
use suppaftp::FtpError;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const CHUNK: usize = 128 * 1024;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// Plain FTP: login and files travel unencrypted.
    None,
    /// FTPS via `AUTH TLS` on the normal port (the common case).
    Explicit,
    /// FTPS with TLS from the first byte, usually on port 990.
    Implicit,
}

impl Security {
    pub fn parse(s: &str) -> Self {
        match s {
            "explicit" => Security::Explicit,
            "implicit" => Security::Implicit,
            _ => Security::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Security::None => "none",
            Security::Explicit => "explicit",
            Security::Implicit => "implicit",
        }
    }
}

#[derive(Clone)]
pub struct FtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub security: Security,
    /// Passive mode (the default almost everywhere). Active mode needs the
    /// server to reach back to this computer, which rarely works behind NAT.
    pub passive: bool,
    /// Skip certificate validation. Shared hosts often present a certificate
    /// issued for the hosting company rather than the address used.
    pub allow_invalid_cert: bool,
    /// Folder to start in instead of the server's login folder.
    pub initial_path: Option<String>,
}

impl std::fmt::Debug for FtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("security", &self.security)
            .field("passive", &self.passive)
            .field("allow_invalid_cert", &self.allow_invalid_cert)
            .field("initial_path", &self.initial_path)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone)]
pub struct FtpEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Unix permission bits when the server reports them.
    pub permissions: Option<u32>,
    pub symlink_target: Option<String>,
}

impl FtpEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FtpClientError {
    #[error("{0}")]
    Connect(String),
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    Tls(String),
    #[error("{0}")]
    NotFound(String),
    #[error("transfer cancelled")]
    Cancelled,
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Server(String),
}

pub type Result<T> = std::result::Result<T, FtpClientError>;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn status_of(e: &FtpError) -> Option<u32> {
    match e {
        FtpError::UnexpectedResponse(r) => Some(r.status.code()),
        _ => None,
    }
}

fn is_missing(e: &FtpError) -> bool {
    matches!(status_of(e), Some(550) | Some(450) | Some(553))
}

pub fn describe(e: &FtpError) -> String {
    match e {
        FtpError::UnexpectedResponse(r) => {
            let body = String::from_utf8_lossy(&r.body).trim().to_string();
            format!("server replied {} {}", r.status.code(), body)
        }
        other => other.to_string(),
    }
}

fn op_err(op: &str, path: &str, e: FtpError) -> FtpClientError {
    let msg = format!("{op} {path}: {}", describe(&e));
    match &e {
        _ if is_missing(&e)
            && matches!(
                op,
                "list" | "stat" | "read" | "download" | "delete" | "delete folder"
            ) =>
        {
            FtpClientError::NotFound(msg)
        }
        FtpError::ConnectionError(_) => FtpClientError::Io(msg),
        _ => FtpClientError::Server(msg),
    }
}

fn io_err(op: &str, path: &str, e: std::io::Error) -> FtpClientError {
    FtpClientError::Io(format!("{op} {path}: {e}"))
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub fn join(parent: &str, name: &str) -> String {
    if parent == "/" || parent.is_empty() {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}

pub fn parent_and_name(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => ("/".to_string(), trimmed[1..].to_string()),
        Some(i) => (trimmed[..i].to_string(), trimmed[i + 1..].to_string()),
        None => ("/".to_string(), trimmed.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct FtpClient {
    s: AsyncRustlsFtpStream,
    config: FtpConfig,
    home: String,
    mlsd: bool,
    mlst: bool,
}

impl FtpClient {
    pub async fn connect(config: &FtpConfig) -> Result<Self> {
        tokio::time::timeout(CONNECT_TIMEOUT, Self::connect_inner(config))
            .await
            .map_err(|_| {
                FtpClientError::Connect(format!(
                    "timed out connecting to {}:{}",
                    config.host, config.port
                ))
            })?
    }

    async fn connect_inner(config: &FtpConfig) -> Result<Self> {
        let addr = format!("{}:{}", config.host, config.port);
        let host = config.host.as_str();
        let connect_err = |e: FtpError| {
            FtpClientError::Connect(format!("could not connect to {addr}: {}", describe(&e)))
        };

        let mut s = match config.security {
            Security::Implicit => {
                let tls = tls::connector(config.allow_invalid_cert).map_err(FtpClientError::Tls)?;
                AsyncRustlsFtpStream::connect_secure_implicit(addr.as_str(), tls, host)
                    .await
                    .map_err(|e| tls::explain(e, &addr))?
            }
            Security::Explicit => {
                let s = AsyncRustlsFtpStream::connect(addr.as_str())
                    .await
                    .map_err(connect_err)?;
                let tls = tls::connector(config.allow_invalid_cert).map_err(FtpClientError::Tls)?;
                s.into_secure(tls, host)
                    .await
                    .map_err(|e| tls::explain(e, &addr))?
            }
            Security::None => AsyncRustlsFtpStream::connect(addr.as_str())
                .await
                .map_err(connect_err)?,
        };

        if config.passive {
            // Many shared hosts answer PASV with their private LAN address;
            // reuse the control connection's address instead.
            s.set_passive_nat_workaround(true);
        } else {
            s = s.active_mode(Duration::from_secs(30));
        }

        s.login(config.username.as_str(), config.password.as_str())
            .await
            .map_err(|e| {
                if status_of(&e) == Some(530) {
                    FtpClientError::Auth(format!(
                        "{addr} rejected the login for user {}",
                        config.username
                    ))
                } else {
                    FtpClientError::Auth(format!("login failed: {}", describe(&e)))
                }
            })?;

        let feats = s.feat().await.unwrap_or_default();
        let has = |k: &str| feats.keys().any(|f| f.eq_ignore_ascii_case(k));
        if has("UTF8") {
            let _ = s.opts("UTF8", Some("ON")).await;
        }
        s.transfer_type(FileType::Binary).await.map_err(|e| {
            FtpClientError::Server(format!("could not switch to binary mode: {}", describe(&e)))
        })?;

        let home = match &config.initial_path {
            Some(p) if p.starts_with('/') => {
                let p = p.trim_end_matches('/');
                if p.is_empty() {
                    "/".to_string()
                } else {
                    p.to_string()
                }
            }
            _ => s.pwd().await.unwrap_or_else(|_| "/".to_string()),
        };
        let home = if home.is_empty() {
            "/".to_string()
        } else {
            home
        };

        Ok(Self {
            mlsd: has("MLSD") || has("MLST"),
            mlst: has("MLST"),
            s,
            config: config.clone(),
            home,
        })
    }

    pub fn config(&self) -> &FtpConfig {
        &self.config
    }

    /// Folder the session starts in (absolute).
    pub fn home(&self) -> &str {
        &self.home
    }

    /// Replace a control connection that got out of step with a fresh one.
    pub async fn reconnect(&mut self) -> Result<()> {
        let fresh = FtpClient::connect(&self.config).await?;
        let mut old = std::mem::replace(self, fresh);
        let _ = tokio::time::timeout(Duration::from_secs(2), old.s.quit()).await;
        Ok(())
    }

    /// Cheap liveness check.
    pub async fn noop(&mut self) -> bool {
        tokio::time::timeout(Duration::from_secs(5), self.s.noop())
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false)
    }

    pub async fn quit(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(3), self.s.quit()).await;
    }

    async fn list_raw(
        &mut self,
        path: &str,
    ) -> std::result::Result<Vec<(ListedFile, bool)>, FtpError> {
        if self.mlsd {
            if let Ok(lines) = self.s.mlsd(Some(path)).await {
                return Ok(lines
                    .iter()
                    .filter_map(|l| {
                        let has_mode = l.to_ascii_lowercase().contains("unix.mode=");
                        ListParser::parse_mlsd(l).ok().map(|f| (f, has_mode))
                    })
                    .collect());
            }
        }
        let lines = self.s.list(Some(path)).await?;
        Ok(lines
            .iter()
            .filter_map(|l| {
                // POSIX style lines carry permissions; DOS style ones do not.
                let posix = l.len() > 10 && matches!(l.as_bytes()[0], b'-' | b'd' | b'l');
                ListedFile::from_str(l).ok().map(|f| (f, posix))
            })
            .collect())
    }

    pub async fn list(&mut self, path: &str) -> Result<Vec<FtpEntry>> {
        let files = self
            .list_raw(path)
            .await
            .map_err(|e| op_err("list", path, e))?;
        let mut out: Vec<FtpEntry> = files
            .iter()
            .filter(|(f, _)| f.name() != "." && f.name() != "..")
            .map(|(f, has_mode)| to_entry(f, path, *has_mode))
            .collect();
        out.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    /// `None` when the path does not exist.
    pub async fn stat(&mut self, path: &str) -> Result<Option<FtpEntry>> {
        if path == "/" || path.is_empty() {
            return Ok(Some(FtpEntry {
                name: "/".into(),
                path: "/".into(),
                kind: EntryKind::Dir,
                size: 0,
                modified: None,
                permissions: None,
                symlink_target: None,
            }));
        }
        let (parent, name) = parent_and_name(path);
        if self.mlst {
            match self.s.mlst(Some(path)).await {
                Ok(line) => {
                    let has_mode = line.to_ascii_lowercase().contains("unix.mode=");
                    if let Ok(f) = ListParser::parse_mlst(line.trim()) {
                        let mut e = to_entry(&f, &parent, has_mode);
                        e.name = name;
                        e.path = path.to_string();
                        return Ok(Some(e));
                    }
                }
                Err(e) if is_missing(&e) => return Ok(None),
                Err(_) => {}
            }
        }
        // Fallback: look the name up in its parent folder.
        match self.list_raw(&parent).await {
            Ok(files) => Ok(files
                .iter()
                .find(|(f, _)| f.name().rsplit('/').next() == Some(name.as_str()))
                .map(|(f, has_mode)| {
                    let mut e = to_entry(f, &parent, *has_mode);
                    e.name = name.clone();
                    e.path = path.to_string();
                    e
                })),
            Err(e) if is_missing(&e) => Ok(None),
            Err(e) => Err(op_err("stat", path, e)),
        }
    }

    /// Read up to `limit` bytes. The flag is true when the file was longer.
    pub async fn read(&mut self, path: &str, limit: u64) -> Result<(Vec<u8>, bool)> {
        let stream = self
            .s
            .retr_as_stream(path)
            .await
            .map_err(|e| op_err("read", path, e))?;
        let mut buf = Vec::new();
        let mut taken = stream.take(limit + 1);
        taken
            .read_to_end(&mut buf)
            .await
            .map_err(|e| io_err("read", path, e))?;
        let stream = taken.into_inner();
        let truncated = buf.len() as u64 > limit;
        if truncated {
            buf.truncate(limit as usize);
            // Closing the data connection early makes the server answer 426
            // (or 226 if it had already sent everything): exactly one reply,
            // which finish() consumes. ABOR is avoided on purpose, servers
            // disagree on how many replies it produces.
            let _ = stream.finish().await;
            if !self.noop().await {
                self.reconnect().await?;
            }
        } else {
            stream.finish().await.map_err(|e| op_err("read", path, e))?;
        }
        Ok((buf, truncated))
    }

    pub async fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let mut cursor = std::io::Cursor::new(data);
        self.s
            .put_file(path, &mut cursor)
            .await
            .map_err(|e| op_err("write", path, e))?;
        Ok(())
    }

    /// Stream `reader` to `path`. `on_bytes` is called after every chunk;
    /// returning false cancels the transfer (the partial remote file is
    /// removed).
    pub async fn upload<R, F>(&mut self, path: &str, reader: &mut R, mut on_bytes: F) -> Result<u64>
    where
        R: AsyncRead + Unpin,
        F: FnMut(u64) -> bool,
    {
        let mut dst = self
            .s
            .put_with_stream(path)
            .await
            .map_err(|e| op_err("upload", path, e))?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .await
                .map_err(|e| io_err("read local file for", path, e))?;
            if n == 0 {
                break;
            }
            if let Err(e) = dst.write_all(&buf[..n]).await {
                let _ = dst.finish().await;
                return Err(io_err("upload", path, e));
            }
            total += n as u64;
            if !on_bytes(n as u64) {
                let _ = dst.finish().await;
                if !self.noop().await {
                    self.reconnect().await?;
                }
                let _ = self.s.rm(path).await;
                return Err(FtpClientError::Cancelled);
            }
        }
        dst.finish().await.map_err(|e| op_err("upload", path, e))?;
        Ok(total)
    }

    /// Stream `path` into `writer`. `on_bytes` works as in [`Self::upload`].
    pub async fn download<W, F>(
        &mut self,
        path: &str,
        writer: &mut W,
        mut on_bytes: F,
    ) -> Result<u64>
    where
        W: AsyncWrite + Unpin,
        F: FnMut(u64) -> bool,
    {
        let mut src = self
            .s
            .retr_as_stream(path)
            .await
            .map_err(|e| op_err("download", path, e))?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = match src.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = src.finish().await;
                    return Err(io_err("download", path, e));
                }
            };
            if n == 0 {
                break;
            }
            if let Err(e) = writer.write_all(&buf[..n]).await {
                let _ = src.finish().await;
                if !self.noop().await {
                    let _ = self.reconnect().await;
                }
                return Err(io_err("write local copy of", path, e));
            }
            total += n as u64;
            if !on_bytes(n as u64) {
                let _ = src.finish().await;
                if !self.noop().await {
                    self.reconnect().await?;
                }
                return Err(FtpClientError::Cancelled);
            }
        }
        src.finish()
            .await
            .map_err(|e| op_err("download", path, e))?;
        writer
            .flush()
            .await
            .map_err(|e| io_err("flush local copy of", path, e))?;
        Ok(total)
    }

    pub async fn mkdir(&mut self, path: &str) -> Result<()> {
        self.s
            .mkdir(path)
            .await
            .map_err(|e| op_err("mkdir", path, e))
    }

    /// `mkdir -p`.
    pub async fn mkdir_all(&mut self, path: &str) -> Result<()> {
        if path == "/" || path.is_empty() {
            return Ok(());
        }
        if let Some(e) = self.stat(path).await? {
            if e.is_dir() {
                return Ok(());
            }
            return Err(FtpClientError::Server(format!(
                "{path} exists and is a file, not a folder"
            )));
        }
        let (parent, _) = parent_and_name(path);
        Box::pin(self.mkdir_all(&parent)).await?;
        self.mkdir(path).await
    }

    pub async fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.s
            .rename(from, to)
            .await
            .map_err(|e| FtpClientError::Server(format!("rename {from} -> {to}: {}", describe(&e))))
    }

    pub async fn remove_file(&mut self, path: &str) -> Result<()> {
        self.s.rm(path).await.map_err(|e| op_err("delete", path, e))
    }

    pub async fn remove_dir(&mut self, path: &str) -> Result<()> {
        self.s
            .rmdir(path)
            .await
            .map_err(|e| op_err("delete folder", path, e))
    }

    /// Delete a file, or a folder with everything inside it.
    pub async fn remove_all(&mut self, entry: &FtpEntry) -> Result<()> {
        if entry.is_dir() {
            for child in self.list(&entry.path).await? {
                Box::pin(self.remove_all(&child)).await?;
            }
            self.remove_dir(&entry.path).await
        } else {
            self.remove_file(&entry.path).await
        }
    }

    /// `SITE CHMOD`. Not every server supports it (Windows/IIS hosts don't).
    pub async fn chmod(&mut self, path: &str, mode: u32) -> Result<()> {
        self.s
            .site(format!("CHMOD {:o} {}", mode & 0o7777, path))
            .await
            .map(|_| ())
            .map_err(|e| FtpClientError::Server(format!("chmod {path}: {}", describe(&e))))
    }
}

fn to_entry(f: &ListedFile, parent: &str, has_mode: bool) -> FtpEntry {
    // Some servers return full paths in LIST output; keep the last segment.
    let name = f.name().rsplit('/').next().unwrap_or(f.name()).to_string();
    let kind = if f.is_directory() {
        EntryKind::Dir
    } else if f.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    let permissions = has_mode.then(|| mode_bits(f));
    FtpEntry {
        path: join(parent, &name),
        name,
        kind,
        size: f.size() as u64,
        modified: Some(f.modified()),
        permissions,
        symlink_target: f.symlink().map(|p| p.to_string_lossy().to_string()),
    }
}

fn mode_bits(f: &ListedFile) -> u32 {
    let mut mode = 0u32;
    for (shift, who) in [
        (6, PosixPexQuery::Owner),
        (3, PosixPexQuery::Group),
        (0, PosixPexQuery::Others),
    ] {
        if f.can_read(who) {
            mode |= 4 << shift;
        }
        if f.can_write(who) {
            mode |= 2 << shift;
        }
        if f.can_execute(who) {
            mode |= 1 << shift;
        }
    }
    mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join("/a/", "b"), "/a/b");
        assert_eq!(
            parent_and_name("/a/b/c.txt"),
            ("/a/b".into(), "c.txt".into())
        );
        assert_eq!(parent_and_name("/a"), ("/".into(), "a".into()));
    }

    #[test]
    fn posix_mode() {
        let f = ListedFile::from_str(
            "-rw-r--r--    1 1000     1000         66 Oct 21 17:27 legado.php",
        )
        .unwrap();
        assert_eq!(mode_bits(&f), 0o644);
        let d = ListedFile::from_str("drwxr-x---    2 1000     1000       4096 Oct 21 17:27 img")
            .unwrap();
        assert_eq!(mode_bits(&d), 0o750);
    }
}
