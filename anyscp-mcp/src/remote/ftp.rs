//! FTP/FTPS through the client shared with the Talktoall Files app.

use anyhow::{anyhow, bail, Context, Result};
use anyscp_ftp::{EntryKind, FtpClient, FtpConfig, FtpEntry, Security};
use async_trait::async_trait;
use std::path::Path;

use super::{Entry, Kind, Remote};
use crate::progress::Progress;
use crate::store::{self, FtpSecurity, Server, StoredCredential};

pub struct FtpRemote {
    c: FtpClient,
}

fn to_entry(e: FtpEntry) -> Entry {
    Entry {
        kind: match e.kind {
            EntryKind::Dir => Kind::Dir,
            EntryKind::File => Kind::File,
            EntryKind::Symlink => Kind::Symlink,
        },
        name: e.name,
        path: e.path,
        size: e.size,
        modified: e.modified,
        permissions: e.permissions.map(mode_string),
    }
}

fn mode_string(m: u32) -> String {
    let mut s = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (m >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

impl FtpRemote {
    pub async fn connect(server: &Server) -> Result<Self> {
        let cred = tokio::task::spawn_blocking({
            let s = server.clone();
            move || store::credential(&s)
        })
        .await??;
        let password = match cred {
            Some(StoredCredential::Password { password }) => password,
            _ if server.username.eq_ignore_ascii_case("anonymous") => "anonymous@".to_string(),
            _ => bail!(
                "No password is saved for {} in Talktoall Files. Edit the connection in the app and save the password.",
                server.label
            ),
        };
        let config = FtpConfig {
            host: server.host.clone(),
            port: server.port,
            username: server.username.clone(),
            password,
            security: match server.ftp_security {
                FtpSecurity::None => Security::None,
                FtpSecurity::Explicit => Security::Explicit,
                FtpSecurity::Implicit => Security::Implicit,
            },
            passive: server.ftp_passive,
            allow_invalid_cert: server.ftp_allow_invalid_cert,
            initial_path: server.initial_path.clone(),
        };
        let c = FtpClient::connect(&config)
            .await
            .map_err(|e| anyhow!("{}: {e}", server.label))?;
        Ok(Self { c })
    }
}

#[async_trait]
impl Remote for FtpRemote {
    fn home(&self) -> &str {
        self.c.home()
    }

    async fn ping(&mut self) -> bool {
        self.c.noop().await
    }

    async fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        Ok(self.c.list(path).await?.into_iter().map(to_entry).collect())
    }

    async fn stat(&mut self, path: &str) -> Result<Option<Entry>> {
        Ok(self.c.stat(path).await?.map(to_entry))
    }

    async fn read(&mut self, path: &str, limit: u64) -> Result<(Vec<u8>, bool)> {
        Ok(self.c.read(path, limit).await?)
    }

    async fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
        Ok(self.c.write(path, data).await?)
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
        Ok(self
            .c
            .upload(remote, &mut src, |n| {
                progress.add_bytes(n);
                true
            })
            .await?)
    }

    async fn download_file(
        &mut self,
        remote: &str,
        local: &Path,
        progress: &Progress,
    ) -> Result<u64> {
        let tmp = super::sftp::partial_path(local);
        let mut dst = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("could not create {}", tmp.display()))?;
        let n = self
            .c
            .download(remote, &mut dst, |n| {
                progress.add_bytes(n);
                true
            })
            .await;
        drop(dst);
        match n {
            Ok(n) => {
                tokio::fs::rename(&tmp, local).await?;
                Ok(n)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e.into())
            }
        }
    }

    async fn mkdir(&mut self, path: &str) -> Result<()> {
        Ok(self.c.mkdir(path).await?)
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        Ok(self.c.rename(from, to).await?)
    }

    async fn remove_file(&mut self, path: &str) -> Result<()> {
        Ok(self.c.remove_file(path).await?)
    }

    async fn remove_dir(&mut self, path: &str) -> Result<()> {
        Ok(self.c.remove_dir(path).await?)
    }

    async fn close(&mut self) {
        self.c.quit().await;
    }
}
