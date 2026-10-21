//! FTP / FTPS support.
//!
//! The command surface mirrors `scp` and `sftp` exactly (same operation names,
//! arguments and return shapes, with the `ftp_` prefix and an `ftp_session_id`
//! argument), so the Explorer UI drives it through the same transport switch.
//!
//! The protocol itself lives in the shared `anyscp-ftp` crate, which the
//! `anyscp-mcp` server also uses. FTP allows one transfer per control
//! connection, so each session keeps one connection for browsing and every
//! queued transfer opens its own.

pub mod commands;
pub mod transfer_manager;

use anyscp_ftp::{EntryKind, FtpClient, FtpClientError, FtpConfig, FtpEntry, Security};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;

use crate::db::SavedHost;
use crate::scp::{format_permissions, ScpEntry, ScpEntryType, TransferDirection, TransferStatus};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum FtpError {
    #[error("FTP session not found: {0}")]
    SessionNotFound(String),
    #[error("{0}")]
    ConnectionFailed(String),
    #[error("{0}")]
    AuthenticationFailed(String),
    #[error("{0}")]
    TlsError(String),
    #[error("{0}")]
    RemoteError(String),
    #[error("Remote I/O error: {0}")]
    RemoteIoError(String),
    #[error("Local I/O error: {0}")]
    LocalIoError(String),
    #[error("Transfer cancelled")]
    TransferCancelled,
    #[error("Path not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    ProtocolError(String),
    #[error("Channel error: {0}")]
    ChannelError(String),
}

/// Serialize as `{ kind, message }`, same convention as ScpError / SftpError.
impl Serialize for FtpError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("FtpError", 2)?;
        let kind = match self {
            FtpError::SessionNotFound(_) => "session_not_found",
            FtpError::ConnectionFailed(_) => "connection_failed",
            FtpError::AuthenticationFailed(_) => "authentication_failed",
            FtpError::TlsError(_) => "tls_error",
            FtpError::RemoteError(_) => "remote_error",
            FtpError::RemoteIoError(_) => "remote_io_error",
            FtpError::LocalIoError(_) => "local_io_error",
            FtpError::TransferCancelled => "transfer_cancelled",
            FtpError::NotFound(_) => "not_found",
            FtpError::ProtocolError(_) => "protocol_error",
            FtpError::ChannelError(_) => "channel_error",
        };
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<FtpClientError> for FtpError {
    fn from(e: FtpClientError) -> Self {
        match e {
            FtpClientError::Connect(m) => FtpError::ConnectionFailed(m),
            FtpClientError::Auth(m) => FtpError::AuthenticationFailed(m),
            FtpClientError::Tls(m) => FtpError::TlsError(m),
            FtpClientError::NotFound(m) => FtpError::NotFound(m),
            FtpClientError::Cancelled => FtpError::TransferCancelled,
            FtpClientError::Io(m) => FtpError::RemoteIoError(m),
            FtpClientError::Server(m) => FtpError::RemoteError(m),
        }
    }
}

// ─── Transfer events ─────────────────────────────────────────────────────────

/// Payload on the `ftp:transfer` channel. Same shape as the SCP/SFTP events
/// with the session-id key renamed, so the frontend normalizes all of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferEvent {
    pub transfer_id: String,
    pub ftp_session_id: String,
    pub name: String,
    pub direction: TransferDirection,
    pub status: TransferStatus,
    pub error: Option<String>,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub created_at: u64,
}

pub type TransferInfo = TransferEvent;

/// Legacy single-file progress payload (`ftp:progress`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub ftp_session_id: String,
    pub file_name: String,
    pub direction: TransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub status: TransferStatus,
}

// ─── Entries ─────────────────────────────────────────────────────────────────

/// Convert to the entry shape the Explorer already renders for SFTP/SCP.
pub fn to_scp_entry(e: &FtpEntry) -> ScpEntry {
    let (entry_type, is_symlink) = match e.kind {
        EntryKind::Dir => (ScpEntryType::Directory, false),
        EntryKind::File => (ScpEntryType::File, false),
        EntryKind::Symlink => (ScpEntryType::Symlink, true),
    };
    let permissions = e.permissions.unwrap_or(0);
    ScpEntry {
        name: e.name.clone(),
        path: e.path.clone(),
        entry_type,
        size: e.size,
        permissions,
        permissions_display: if e.permissions.is_some() {
            format_permissions(permissions)
        } else {
            String::new()
        },
        modified: e
            .modified
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        is_symlink,
    }
}

// ─── Sessions ────────────────────────────────────────────────────────────────

/// Recheck a browsing connection that sat idle longer than this.
const RECHECK_AFTER: Duration = Duration::from_secs(30);

pub struct FtpSession {
    /// Saved host this session belongs to (diagnostics).
    #[allow(dead_code)]
    pub host_id: String,
    pub config: FtpConfig,
    browse: Mutex<(FtpClient, Instant)>,
}

impl FtpSession {
    /// The browsing connection, reconnected first if it went stale (servers
    /// commonly drop idle control connections after a few minutes).
    pub async fn client(&self) -> Result<MutexGuard<'_, (FtpClient, Instant)>, FtpError> {
        let mut guard = self.browse.lock().await;
        if guard.1.elapsed() > RECHECK_AFTER && !guard.0.noop().await {
            guard.0.reconnect().await?;
        }
        guard.1 = Instant::now();
        Ok(guard)
    }

    pub async fn home(&self) -> String {
        self.browse.lock().await.0.home().to_string()
    }

    /// A separate connection for a transfer, so browsing stays responsive.
    pub async fn dedicated(&self) -> Result<FtpClient, FtpError> {
        Ok(FtpClient::connect(&self.config).await?)
    }
}

pub struct FtpManager {
    sessions: DashMap<String, Arc<FtpSession>>,
    active_transfers: DashMap<String, CancellationToken>,
}

impl Default for FtpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FtpManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            active_transfers: DashMap::new(),
        }
    }

    pub async fn open(&self, host_id: String, config: FtpConfig) -> Result<String, FtpError> {
        let client = FtpClient::connect(&config).await?;
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            id.clone(),
            Arc::new(FtpSession {
                host_id,
                config,
                browse: Mutex::new((client, Instant::now())),
            }),
        );
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Arc<FtpSession>, FtpError> {
        self.sessions
            .get(id)
            .map(|s| s.value().clone())
            .ok_or_else(|| FtpError::SessionNotFound(id.to_string()))
    }

    pub async fn close(&self, id: &str) {
        if let Some((_, session)) = self.sessions.remove(id) {
            let mut guard = session.browse.lock().await;
            guard.0.quit().await;
        }
    }

    pub fn insert_transfer(&self, id: String, token: CancellationToken) {
        self.active_transfers.insert(id, token);
    }

    pub fn cancel_transfer(&self, id: &str) -> Result<(), FtpError> {
        let entry = self
            .active_transfers
            .get(id)
            .ok_or_else(|| FtpError::SessionNotFound(format!("transfer not found: {id}")))?;
        entry.value().cancel();
        Ok(())
    }

    pub fn remove_transfer(&self, id: &str) {
        self.active_transfers.remove(id);
    }
}

/// Build the connection settings for a saved FTP host. The password comes
/// from the OS credential store, saved there by the host editor.
pub fn config_for_host(host: &SavedHost) -> Result<FtpConfig, FtpError> {
    if !host.is_ftp() {
        return Err(FtpError::ProtocolError(format!(
            "{} is not an FTP host",
            host.label
        )));
    }
    let password = match crate::vault::get_credential(&host.id) {
        Ok(crate::vault::StoredCredential::Password { password }) => password,
        _ if host.username.eq_ignore_ascii_case("anonymous") => "anonymous@".to_string(),
        _ => {
            return Err(FtpError::AuthenticationFailed(format!(
                "No password saved for {}. Edit the host and enter its password.",
                host.label
            )))
        }
    };
    let initial_path = host
        .start_directory
        .as_deref()
        .map(str::trim)
        .filter(|p| p.starts_with('/'))
        .map(str::to_string);
    Ok(FtpConfig {
        host: host.host.clone(),
        port: host.port,
        username: host.username.clone(),
        password,
        security: Security::parse(host.ftp_security.as_deref().unwrap_or("none")),
        passive: host.ftp_passive.unwrap_or(true),
        allow_invalid_cert: host.ftp_allow_invalid_cert.unwrap_or(false),
        initial_path,
    })
}

/// End-to-end check of the FTP commands and transfer queue against a live
/// server. Skipped unless `ANYSCP_FTP_TEST` is set to `host:port:user:password`
/// (add `ANYSCP_FTP_TEST_TLS=1` for explicit FTPS).
#[cfg(test)]
mod live_tests {
    use super::commands::*;
    use super::transfer_manager::FtpTransferManager;
    use super::*;
    use tauri::Manager;

    fn live_config() -> Option<FtpConfig> {
        let spec = std::env::var("ANYSCP_FTP_TEST").ok()?;
        let parts: Vec<&str> = spec.splitn(4, ':').collect();
        Some(FtpConfig {
            host: parts[0].into(),
            port: parts[1].parse().ok()?,
            username: parts[2].into(),
            password: parts[3].into(),
            // ANYSCP_FTP_TEST_TLS=1: explicit FTPS, accepting a self-signed cert.
            security: if std::env::var_os("ANYSCP_FTP_TEST_TLS").is_some() {
                Security::Explicit
            } else {
                Security::None
            },
            passive: true,
            allow_invalid_cert: std::env::var_os("ANYSCP_FTP_TEST_TLS").is_some(),
            initial_path: None,
        })
    }

    async fn wait_done(tm: &FtpTransferManager, ids: &[String]) -> Vec<TransferInfo> {
        for _ in 0..200 {
            let all = tm.list_all();
            let mine: Vec<_> = all
                .into_iter()
                .filter(|t| ids.contains(&t.transfer_id))
                .collect();
            if mine.iter().all(|t| {
                matches!(
                    t.status,
                    TransferStatus::Completed
                        | TransferStatus::Failed(_)
                        | TransferStatus::Cancelled
                )
            }) {
                return mine;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("transfers did not finish");
    }

    #[tokio::test]
    async fn commands_and_queue_against_live_server() {
        let Some(config) = live_config() else {
            eprintln!("ANYSCP_FTP_TEST not set; skipping");
            return;
        };
        let app = tauri::test::mock_app();
        let ftp = Arc::new(FtpManager::new());
        app.manage(ftp.clone());
        app.manage(Arc::new(FtpTransferManager::with_emitter(
            ftp.clone(),
            Arc::new(|_| {}),
        )));

        let sid = ftp.open("test-host".into(), config).await.unwrap();
        let base = format!("/anyscp-test-{}", uuid::Uuid::new_v4());

        // mkdir -p, create file, list
        ftp_mkdir(sid.clone(), format!("{base}/a/b"), app.state())
            .await
            .unwrap();
        ftp_create_file(sid.clone(), format!("{base}/a/empty.txt"), app.state())
            .await
            .unwrap();
        let listed = ftp_list_dir(sid.clone(), format!("{base}/a"), app.state())
            .await
            .unwrap();
        let names: Vec<_> = listed.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["b", "empty.txt"]);
        assert!(ftp_home_dir(sid.clone(), app.state())
            .await
            .unwrap()
            .starts_with('/'));

        // Queue a folder upload, then download it back.
        let local = tempfile::tempdir().unwrap();
        let src = local.path().join("site");
        std::fs::create_dir_all(src.join("css")).unwrap();
        std::fs::write(src.join("index.html"), b"<h1>oi</h1>").unwrap();
        let big: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(src.join("css").join("big.bin"), &big).unwrap();
        let tm: tauri::State<'_, Arc<FtpTransferManager>> = app.state();
        let ids = ftp_enqueue_upload(
            sid.clone(),
            vec![src.display().to_string()],
            base.clone(),
            app.state(),
        )
        .await
        .unwrap();
        let done = wait_done(&tm, &ids).await;
        assert_eq!(
            done[0].status,
            TransferStatus::Completed,
            "{:?}",
            done[0].error
        );
        assert_eq!(done[0].files_done, 2);

        let back = local.path().join("back");
        let ids = ftp_enqueue_download(
            sid.clone(),
            vec![format!("{base}/site")],
            back.display().to_string(),
            app.state(),
        )
        .await
        .unwrap();
        let done = wait_done(&tm, &ids).await;
        assert_eq!(
            done[0].status,
            TransferStatus::Completed,
            "{:?}",
            done[0].error
        );
        assert_eq!(done[0].total_bytes, big.len() as u64 + 11);
        assert_eq!(std::fs::read(back.join("site/css/big.bin")).unwrap(), big);

        // Copy (through this machine) and move, both deduplicating names.
        let copies = ftp_copy_entries(
            sid.clone(),
            vec![format!("{base}/site")],
            format!("{base}/a"),
            app.state(),
        )
        .await
        .unwrap();
        assert_eq!(copies, vec![format!("{base}/a/site")]);
        let copies = ftp_copy_entries(
            sid.clone(),
            vec![format!("{base}/site/index.html")],
            format!("{base}/site"),
            app.state(),
        )
        .await
        .unwrap();
        assert_eq!(copies, vec![format!("{base}/site/index (1).html")]);
        let moved = ftp_move_entries(
            sid.clone(),
            vec![format!("{base}/a/empty.txt")],
            format!("{base}/a/b"),
            app.state(),
        )
        .await
        .unwrap();
        assert_eq!(moved, vec![format!("{base}/a/b/empty.txt")]);
        assert!(ftp_move_entries(
            sid.clone(),
            vec![format!("{base}/a")],
            format!("{base}/a/b"),
            app.state()
        )
        .await
        .is_err());

        // Rename, then delete everything recursively.
        ftp_rename(
            sid.clone(),
            format!("{base}/site"),
            format!("{base}/site2"),
            app.state(),
        )
        .await
        .unwrap();
        ftp_delete(sid.clone(), base.clone(), true, app.state())
            .await
            .unwrap();
        let root = ftp_list_dir(sid.clone(), "/".into(), app.state())
            .await
            .unwrap();
        assert!(!root.iter().any(|e| base.ends_with(&e.name)));

        ftp_close(sid, app.state()).await.unwrap();
    }
}
