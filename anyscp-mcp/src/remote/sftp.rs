use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use russh::client;
use russh_keys::key::PublicKey;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{join, Entry, Kind, Remote};
use crate::known_hosts::{KnownHosts, Verdict};
use crate::progress::Progress;
use crate::store::{self, Server, Store, StoredCredential};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const CHUNK: usize = 256 * 1024;

struct Handler {
    target: String,
    known: Arc<KnownHosts>,
    problem: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let fp = key.fingerprint();
        match self.known.check(&self.target, &fp) {
            Ok(Verdict::Trusted) | Ok(Verdict::FirstSeen) => Ok(true),
            Ok(Verdict::Mismatch { expected }) => {
                *self.problem.lock().unwrap() = Some(format!(
                    "The SSH host key of {} changed (expected SHA256:{expected}, got SHA256:{fp}). \
                     This happens after a server reinstall, but can also mean the connection is being \
                     intercepted. Connection refused. If the change is expected, remove the line for \
                     {} from {}.",
                    self.target,
                    self.target,
                    self.known.path().display()
                ));
                Ok(false)
            }
            Err(e) => {
                *self.problem.lock().unwrap() = Some(format!("could not record the host key: {e}"));
                Ok(false)
            }
        }
    }
}

pub struct SftpRemote {
    // Order matters: the SFTP session must drop before the handles under it.
    sftp: SftpSession,
    handle: client::Handle<Handler>,
    _jumps: Vec<client::Handle<Handler>>,
    home: String,
}

type Handles = (client::Handle<Handler>, Vec<client::Handle<Handler>>);

fn establish<'a>(
    server: &'a Server,
    store: &'a Store,
    known: Arc<KnownHosts>,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Handles>> + Send + 'a>> {
    Box::pin(async move {
        if depth > 5 {
            bail!("ProxyJump chain is too long or circular");
        }
        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        });
        let target = format!("{}:{}", server.host, server.port);
        let problem = Arc::new(Mutex::new(None));
        let handler = Handler {
            target: target.clone(),
            known: known.clone(),
            problem: problem.clone(),
        };

        let (mut handle, chain) = match &server.proxy_jump_id {
            None => {
                let h = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    client::connect(config, target.as_str(), handler),
                )
                .await
                .map_err(|_| anyhow!("timed out connecting to {target}"))?;
                let h = h.map_err(|e| connect_error(e, &problem, &target))?;
                (h, Vec::new())
            }
            Some(jump_id) => {
                let jump = store.get_ssh_host(jump_id)?;
                let (jump_handle, mut chain) = establish(&jump, store, known.clone(), depth + 1)
                    .await
                    .with_context(|| format!("via tunnel host {}", jump.label))?;
                let channel = jump_handle
                    .channel_open_direct_tcpip(
                        server.host.clone(),
                        server.port as u32,
                        "127.0.0.1",
                        0,
                    )
                    .await
                    .map_err(|e| anyhow!("could not open the tunnel to {target}: {e}"))?;
                let h = client::connect_stream(config, channel.into_stream(), handler)
                    .await
                    .map_err(|e| connect_error(e, &problem, &target))?;
                chain.push(jump_handle);
                (h, chain)
            }
        };

        authenticate(&mut handle, server).await?;
        Ok((handle, chain))
    })
}

fn connect_error(
    e: russh::Error,
    problem: &Arc<Mutex<Option<String>>>,
    target: &str,
) -> anyhow::Error {
    if let Some(msg) = problem.lock().unwrap().take() {
        return anyhow!(msg);
    }
    anyhow!("could not connect to {target}: {e}")
}

async fn authenticate(handle: &mut client::Handle<Handler>, server: &Server) -> Result<()> {
    let cred = tokio::task::spawn_blocking({
        let s = server.clone();
        move || store::credential(&s)
    })
    .await??;

    let ok = if server.auth_type == "privateKey" {
        let path = server
            .key_path
            .clone()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "{} uses key authentication but has no key file set",
                    server.label
                )
            })?;
        let data = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("could not read the private key {path}"))?;
        if data.trim_start().starts_with("PuTTY-User-Key-File") {
            bail!(
                "{} uses a PuTTY .ppk key. Convert it to OpenSSH format (PuTTYgen > Conversions > \
                 Export OpenSSH key) and point the server to the new file in anySCP.",
                server.label
            );
        }
        let passphrase = match cred {
            Some(StoredCredential::KeyPassphrase { passphrase }) => Some(passphrase),
            _ => None,
        };
        let key = russh_keys::decode_secret_key(&data, passphrase.as_deref())
            .map_err(|e| anyhow!("could not load the private key for {}: {e}", server.label))?;
        handle
            .authenticate_publickey(&server.username, Arc::new(key))
            .await
            .map_err(|e| anyhow!("authentication error: {e}"))?
    } else {
        let password = match cred {
            Some(StoredCredential::Password { password }) => password,
            _ => bail!(
                "No password is saved for {} in anySCP. Edit the server in the app and save the password.",
                server.label
            ),
        };
        handle
            .authenticate_password(&server.username, password)
            .await
            .map_err(|e| anyhow!("authentication error: {e}"))?
    };
    if !ok {
        bail!(
            "{} rejected the login for user {}",
            server.label,
            server.username
        );
    }
    Ok(())
}

impl SftpRemote {
    pub async fn connect(server: &Server, store: &Store, known: Arc<KnownHosts>) -> Result<Self> {
        let (handle, jumps) = establish(server, store, known, 0).await?;
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("could not open an SSH channel: {e}"))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| anyhow!("could not start SFTP: {e}"))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| anyhow!("{} does not offer SFTP: {e}", server.label))?;
        let home = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/".to_string());
        Ok(Self {
            sftp,
            handle,
            _jumps: jumps,
            home,
        })
    }
}

fn not_found(e: &SftpError) -> bool {
    matches!(e, SftpError::Status(s) if s.status_code == StatusCode::NoSuchFile)
}

fn to_entry(name: String, path: String, a: &FileAttributes) -> Entry {
    let ft = a.file_type();
    let kind = if ft.is_dir() {
        Kind::Dir
    } else if ft.is_symlink() {
        Kind::Symlink
    } else if ft.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Entry {
        name,
        path,
        kind,
        size: a.size.unwrap_or(0),
        modified: a.modified().ok(),
        permissions: a.permissions.map(|_| a.permissions().to_string()),
    }
}

fn err(op: &str, path: &str, e: SftpError) -> anyhow::Error {
    anyhow!("{op} {path}: {e}")
}

#[async_trait]
impl Remote for SftpRemote {
    fn home(&self) -> &str {
        &self.home
    }

    async fn ping(&mut self) -> bool {
        !self.handle.is_closed()
            && tokio::time::timeout(Duration::from_secs(5), self.sftp.canonicalize("."))
                .await
                .map(|r| r.is_ok())
                .unwrap_or(false)
    }

    async fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let rd = self
            .sftp
            .read_dir(path)
            .await
            .map_err(|e| err("list", path, e))?;
        let mut out: Vec<Entry> = rd
            .filter(|e| e.file_name() != "." && e.file_name() != "..")
            .map(|e| {
                let name = e.file_name();
                let p = join(path, &name);
                to_entry(name, p, &e.metadata())
            })
            .collect();
        // Resolve symlinked folders so recursive operations and the listing
        // show them as what they point to.
        for e in out.iter_mut().filter(|e| e.kind == Kind::Symlink) {
            if let Ok(target) = self.sftp.metadata(e.path.clone()).await {
                if target.file_type().is_dir() {
                    e.kind = Kind::Dir;
                } else if target.file_type().is_file() {
                    e.kind = Kind::File;
                    e.size = target.size.unwrap_or(e.size);
                }
            }
        }
        out.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    async fn stat(&mut self, path: &str) -> Result<Option<Entry>> {
        match self.sftp.metadata(path.to_string()).await {
            Ok(a) => {
                let (_, name) = super::parent_and_name(path);
                Ok(Some(to_entry(name, path.to_string(), &a)))
            }
            Err(e) if not_found(&e) => Ok(None),
            Err(e) => Err(err("stat", path, e)),
        }
    }

    async fn read(&mut self, path: &str, limit: u64) -> Result<(Vec<u8>, bool)> {
        let file = self
            .sftp
            .open(path.to_string())
            .await
            .map_err(|e| err("open", path, e))?;
        let mut buf = Vec::new();
        let mut taken = file.take(limit + 1);
        taken
            .read_to_end(&mut buf)
            .await
            .with_context(|| format!("read {path}"))?;
        let truncated = buf.len() as u64 > limit;
        buf.truncate(limit as usize);
        Ok((buf, truncated))
    }

    async fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let mut file = self
            .sftp
            .create(path.to_string())
            .await
            .map_err(|e| err("create", path, e))?;
        file.write_all(data)
            .await
            .with_context(|| format!("write {path}"))?;
        file.shutdown()
            .await
            .with_context(|| format!("close {path}"))?;
        Ok(())
    }

    async fn upload_file(
        &mut self,
        local: &Path,
        remote: &str,
        progress: &Progress,
    ) -> Result<u64> {
        let mut src = tokio::fs::File::open(local)
            .await
            .with_context(|| format!("could not open {}", local.display()))?;
        let mut dst = self
            .sftp
            .create(remote.to_string())
            .await
            .map_err(|e| err("create", remote, e))?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n])
                .await
                .with_context(|| format!("write {remote}"))?;
            total += n as u64;
            progress.add_bytes(n as u64);
        }
        dst.shutdown()
            .await
            .with_context(|| format!("close {remote}"))?;
        Ok(total)
    }

    async fn download_file(
        &mut self,
        remote: &str,
        local: &Path,
        progress: &Progress,
    ) -> Result<u64> {
        let mut src = self
            .sftp
            .open(remote.to_string())
            .await
            .map_err(|e| err("open", remote, e))?;
        let tmp = partial_path(local);
        let mut dst = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("could not create {}", tmp.display()))?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = src
                .read(&mut buf)
                .await
                .with_context(|| format!("read {remote}"))?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            total += n as u64;
            progress.add_bytes(n as u64);
        }
        dst.flush().await?;
        drop(dst);
        tokio::fs::rename(&tmp, local).await?;
        Ok(total)
    }

    async fn mkdir(&mut self, path: &str) -> Result<()> {
        self.sftp
            .create_dir(path.to_string())
            .await
            .map_err(|e| err("mkdir", path, e))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.sftp
            .rename(from.to_string(), to.to_string())
            .await
            .map_err(|e| anyhow!("rename {from} -> {to}: {e}"))
    }

    async fn remove_file(&mut self, path: &str) -> Result<()> {
        self.sftp
            .remove_file(path.to_string())
            .await
            .map_err(|e| err("delete", path, e))
    }

    async fn remove_dir(&mut self, path: &str) -> Result<()> {
        self.sftp
            .remove_dir(path.to_string())
            .await
            .map_err(|e| err("delete folder", path, e))
    }

    async fn close(&mut self) {
        let _ = self.sftp.close().await;
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

/// Downloads land in `<name>.part` first and are renamed when complete, so an
/// interrupted transfer never leaves a truncated file under the real name.
pub fn partial_path(local: &Path) -> std::path::PathBuf {
    let mut name = local
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    local.with_file_name(name)
}
