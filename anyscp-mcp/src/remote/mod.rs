//! Protocol-neutral file operations. SFTP and FTP each implement [`Remote`];
//! everything recursive (folder uploads, downloads, deletes) is written once
//! on top of it.

pub mod ftp;
pub mod sftp;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::progress::Progress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub permissions: Option<String>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

#[async_trait]
pub trait Remote: Send {
    /// Folder the session starts in (absolute).
    fn home(&self) -> &str;
    /// Cheap liveness check used before reusing a cached connection.
    async fn ping(&mut self) -> bool;
    async fn list(&mut self, path: &str) -> Result<Vec<Entry>>;
    /// `None` when the path does not exist.
    async fn stat(&mut self, path: &str) -> Result<Option<Entry>>;
    /// Read up to `limit` bytes. The flag is true when the file was longer.
    async fn read(&mut self, path: &str, limit: u64) -> Result<(Vec<u8>, bool)>;
    async fn write(&mut self, path: &str, data: &[u8]) -> Result<()>;
    async fn upload_file(&mut self, local: &Path, remote: &str, progress: &Progress)
        -> Result<u64>;
    async fn download_file(
        &mut self,
        remote: &str,
        local: &Path,
        progress: &Progress,
    ) -> Result<u64>;
    async fn mkdir(&mut self, path: &str) -> Result<()>;
    async fn rename(&mut self, from: &str, to: &str) -> Result<()>;
    async fn remove_file(&mut self, path: &str) -> Result<()>;
    async fn remove_dir(&mut self, path: &str) -> Result<()>;
    async fn close(&mut self);
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Turn whatever the model sent into a clean absolute remote path. Relative
/// paths are taken from the session's starting folder; `.` and `..` are
/// collapsed so nothing can escape above `/`.
pub fn resolve(home: &str, path: &str) -> String {
    let p = path.trim().replace('\\', "/");
    let joined = if p.starts_with('/') {
        p
    } else if p.is_empty() || p == "." || p == "~" {
        home.to_string()
    } else if let Some(rest) = p.strip_prefix("~/") {
        format!("{}/{}", home.trim_end_matches('/'), rest)
    } else {
        format!("{}/{}", home.trim_end_matches('/'), p)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

pub fn join(parent: &str, name: &str) -> String {
    if parent == "/" {
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
// Recursive helpers
// ---------------------------------------------------------------------------

/// `mkdir -p`: create every missing folder on the way to `path`.
pub async fn mkdir_all(r: &mut dyn Remote, path: &str) -> Result<()> {
    if path == "/" {
        return Ok(());
    }
    if let Some(e) = r.stat(path).await? {
        if e.is_dir() {
            return Ok(());
        }
        bail!("{path} exists and is a file, not a folder");
    }
    let (parent, _) = parent_and_name(path);
    Box::pin(mkdir_all(r, &parent)).await?;
    r.mkdir(path).await
}

#[derive(Default, Debug)]
pub struct Totals {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
}

pub async fn upload_tree(
    r: &mut dyn Remote,
    local: &Path,
    remote: &str,
    progress: &Progress,
    totals: &mut Totals,
) -> Result<()> {
    let meta = tokio::fs::metadata(local).await?;
    if meta.is_dir() {
        mkdir_all(r, remote).await?;
        totals.folders += 1;
        let mut rd = tokio::fs::read_dir(local).await?;
        let mut children = Vec::new();
        while let Some(e) = rd.next_entry().await? {
            children.push(e.path());
        }
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Box::pin(upload_tree(
                r,
                &child,
                &join(remote, &name),
                progress,
                totals,
            ))
            .await?;
        }
    } else {
        progress
            .note(&format!("uploading {}", local.display()))
            .await;
        let n = r.upload_file(local, remote, progress).await?;
        totals.files += 1;
        totals.bytes += n;
    }
    Ok(())
}

pub async fn download_tree(
    r: &mut dyn Remote,
    remote: &Entry,
    local: &Path,
    progress: &Progress,
    totals: &mut Totals,
) -> Result<()> {
    if remote.is_dir() {
        tokio::fs::create_dir_all(local).await?;
        totals.folders += 1;
        let children = r.list(&remote.path).await?;
        for child in children {
            if child.kind == Kind::Symlink || child.kind == Kind::Other {
                continue;
            }
            let target: PathBuf = local.join(&child.name);
            Box::pin(download_tree(r, &child, &target, progress, totals)).await?;
        }
    } else {
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        progress.note(&format!("downloading {}", remote.path)).await;
        let n = r.download_file(&remote.path, local, progress).await?;
        totals.files += 1;
        totals.bytes += n;
    }
    Ok(())
}

pub async fn delete_tree(r: &mut dyn Remote, entry: &Entry, totals: &mut Totals) -> Result<()> {
    if entry.is_dir() {
        for child in r.list(&entry.path).await? {
            Box::pin(delete_tree(r, &child, totals)).await?;
        }
        r.remove_dir(&entry.path).await?;
        totals.folders += 1;
    } else {
        r.remove_file(&entry.path).await?;
        totals.files += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_paths() {
        assert_eq!(resolve("/home/luan", ""), "/home/luan");
        assert_eq!(
            resolve("/home/luan", "public_html"),
            "/home/luan/public_html"
        );
        assert_eq!(resolve("/home/luan", "~/a/b"), "/home/luan/a/b");
        assert_eq!(resolve("/home/luan", "/var/www/"), "/var/www");
        assert_eq!(resolve("/", "../../etc"), "/etc");
        assert_eq!(resolve("/home/luan", "a/./b/../c"), "/home/luan/a/c");
        assert_eq!(resolve("/x", "C:\\Users"), "/x/C:/Users");
    }

    #[test]
    fn parent_name() {
        assert_eq!(
            parent_and_name("/a/b/c.txt"),
            ("/a/b".into(), "c.txt".into())
        );
        assert_eq!(parent_and_name("/a"), ("/".into(), "a".into()));
    }
}
