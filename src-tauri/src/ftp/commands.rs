//! Tauri commands for FTP sessions. Same surface as `scp::commands` (and
//! `sftp::commands`): only the prefix and the session-id argument differ, so
//! the Explorer UI dispatches on the tab's transport.
//!
//! The one structural difference: there is no SSH connection underneath, so
//! `ftp_open` takes the saved host id and logs in itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::db::HostDb;
use crate::scp::{ScpEntry, TransferDirection, TransferStatus};

use super::transfer_manager::FtpTransferManager;
use super::{config_for_host, to_scp_entry, FtpError, FtpManager, TransferInfo, TransferProgress};

// ─── Open / Close ────────────────────────────────────────────────────────────

/// Log in to a saved FTP host and return a fresh `ftp_session_id`.
#[tauri::command]
#[instrument(skip(db, ftp_manager), fields(host_id = %host_id))]
pub async fn ftp_open(
    host_id: String,
    db: State<'_, Arc<HostDb>>,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<String, FtpError> {
    let db = Arc::clone(&db);
    let hid = host_id.clone();
    let config = tokio::task::spawn_blocking(move || {
        let host = db
            .get_host(&hid)
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?
            .ok_or_else(|| FtpError::NotFound(format!("host not found: {hid}")))?;
        config_for_host(&host)
    })
    .await
    .map_err(|e| FtpError::LocalIoError(format!("task panicked: {e}")))??;

    let id = ftp_manager.open(host_id, config).await?;
    tracing::info!(ftp_session_id = %id, "FTP session opened");
    Ok(id)
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_close(
    ftp_session_id: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    ftp_manager.close(&ftp_session_id).await;
    Ok(())
}

// ─── Directory operations ────────────────────────────────────────────────────

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path))]
pub async fn ftp_list_dir(
    ftp_session_id: String,
    path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<Vec<ScpEntry>, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    Ok(c.0.list(&path).await?.iter().map(to_scp_entry).collect())
}

#[tauri::command]
pub async fn ftp_home_dir(
    ftp_session_id: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<String, FtpError> {
    Ok(ftp_manager.get(&ftp_session_id)?.home().await)
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path))]
pub async fn ftp_mkdir(
    ftp_session_id: String,
    path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    Ok(c.0.mkdir_all(&path).await?)
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path))]
pub async fn ftp_create_file(
    ftp_session_id: String,
    path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    if c.0.stat(&path).await?.is_some() {
        return Err(FtpError::RemoteError(format!("{path} already exists")));
    }
    Ok(c.0.write(&path, &[]).await?)
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path))]
pub async fn ftp_delete(
    ftp_session_id: String,
    path: String,
    is_dir: bool,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    if is_dir {
        // Same semantics as `rm -rf` in the SCP backend.
        let entry =
            c.0.stat(&path)
                .await?
                .ok_or_else(|| FtpError::NotFound(path.clone()))?;
        Ok(c.0.remove_all(&entry).await?)
    } else {
        Ok(c.0.remove_file(&path).await?)
    }
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_rename(
    ftp_session_id: String,
    old_path: String,
    new_path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    Ok(c.0.rename(&old_path, &new_path).await?)
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path, mode = mode))]
pub async fn ftp_chmod(
    ftp_session_id: String,
    path: String,
    mode: u32,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    Ok(c.0.chmod(&path, mode).await?)
}

/// Recursive chmod, one `SITE CHMOD` per entry; per-entry failures are
/// collected instead of aborting.
#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id, path = %path, mode = mode))]
pub async fn ftp_chmod_recursive(
    ftp_session_id: String,
    path: String,
    mode: u32,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<crate::sftp::ChmodSummary, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    let mut applied = 0u32;
    let mut errors = Vec::new();
    let mut stack = vec![path.clone()];
    while let Some(p) = stack.pop() {
        match c.0.chmod(&p, mode).await {
            Ok(()) => applied += 1,
            Err(e) => errors.push(e.to_string()),
        }
        if let Some(entry) = c.0.stat(&p).await? {
            if entry.is_dir() {
                for child in c.0.list(&p).await? {
                    if child.is_dir() {
                        stack.push(child.path);
                    } else {
                        match c.0.chmod(&child.path, mode).await {
                            Ok(()) => applied += 1,
                            Err(e) => errors.push(e.to_string()),
                        }
                    }
                }
            }
        }
    }
    Ok(crate::sftp::ChmodSummary { applied, errors })
}

// ─── Copy / Move ─────────────────────────────────────────────────────────────

/// Pick `name`, or `name (1)`, `name (2)`… so nothing in `dir` is overwritten.
async fn deduplicate_name(
    c: &mut anyscp_ftp::FtpClient,
    dir: &str,
    name: &str,
) -> Result<String, FtpError> {
    let existing: std::collections::HashSet<String> =
        c.list(dir).await?.into_iter().map(|e| e.name).collect();
    if !existing.contains(name) {
        return Ok(name.to_string());
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1.. {
        let candidate = format!("{stem} ({n}){ext}");
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    unreachable!()
}

#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_move_entries(
    ftp_session_id: String,
    source_paths: Vec<String>,
    target_dir: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<Vec<String>, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.client().await?;
    let mut new_paths = Vec::with_capacity(source_paths.len());
    for source in &source_paths {
        let (_, name) = anyscp_ftp::parent_and_name(source);
        if target_dir == *source
            || target_dir.starts_with(&format!("{}/", source.trim_end_matches('/')))
        {
            return Err(FtpError::RemoteIoError(format!(
                "Cannot move {source} into itself"
            )));
        }
        let deduped = deduplicate_name(&mut c.0, &target_dir, &name).await?;
        let dest = anyscp_ftp::join(&target_dir, &deduped);
        c.0.rename(source, &dest).await?;
        new_paths.push(dest);
    }
    Ok(new_paths)
}

/// FTP has no server-side copy: each file is read and written back through
/// this computer, on a dedicated connection.
#[tauri::command]
#[instrument(skip(ftp_manager), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_copy_entries(
    ftp_session_id: String,
    source_paths: Vec<String>,
    target_dir: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
) -> Result<Vec<String>, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let mut c = session.dedicated().await?;
    let mut new_paths = Vec::with_capacity(source_paths.len());
    let result = async {
        for source in &source_paths {
            let entry = c
                .stat(source)
                .await?
                .ok_or_else(|| FtpError::NotFound(source.clone()))?;
            let deduped = deduplicate_name(&mut c, &target_dir, &entry.name).await?;
            let dest = anyscp_ftp::join(&target_dir, &deduped);
            copy_tree(&mut c, &entry, &dest).await?;
            new_paths.push(dest);
        }
        Ok::<_, FtpError>(())
    }
    .await;
    c.quit().await;
    result.map(|_| new_paths)
}

async fn copy_tree(
    c: &mut anyscp_ftp::FtpClient,
    entry: &anyscp_ftp::FtpEntry,
    dest: &str,
) -> Result<(), FtpError> {
    if entry.is_dir() {
        c.mkdir_all(dest).await?;
        for child in c.list(&entry.path).await? {
            let child_dest = anyscp_ftp::join(dest, &child.name);
            Box::pin(copy_tree(c, &child, &child_dest)).await?;
        }
        return Ok(());
    }
    // Spool through a temp file so large files don't sit in memory.
    let tmp = std::env::temp_dir().join(format!("anyscp-ftp-copy-{}", uuid::Uuid::new_v4()));
    let r = async {
        let mut f = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        c.download(&entry.path, &mut f, |_| true).await?;
        drop(f);
        let mut f = tokio::fs::File::open(&tmp)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        c.upload(dest, &mut f, |_| true).await?;
        Ok::<_, FtpError>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    r
}

// ─── Direct transfers (legacy single-file) ───────────────────────────────────

async fn download_to(
    session: &super::FtpSession,
    remote: &str,
    local: &Path,
    token: &CancellationToken,
) -> Result<(), FtpError> {
    let mut c = session.dedicated().await?;
    let r = async {
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        }
        let mut f = tokio::fs::File::create(local)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        c.download(remote, &mut f, |_| !token.is_cancelled())
            .await?;
        Ok(())
    }
    .await;
    c.quit().await;
    r
}

async fn upload_from(
    session: &super::FtpSession,
    local: &Path,
    remote: &str,
    token: &CancellationToken,
) -> Result<(), FtpError> {
    let mut c = session.dedicated().await?;
    let r = async {
        let (parent, _) = anyscp_ftp::parent_and_name(remote);
        c.mkdir_all(&parent).await?;
        let mut f = tokio::fs::File::open(local)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        c.upload(remote, &mut f, |_| !token.is_cancelled()).await?;
        Ok(())
    }
    .await;
    c.quit().await;
    r
}

fn status_of(r: &Result<(), FtpError>) -> TransferStatus {
    match r {
        Ok(()) => TransferStatus::Completed,
        Err(FtpError::TransferCancelled) => TransferStatus::Cancelled,
        Err(e) => TransferStatus::Failed(e.to_string()),
    }
}

#[tauri::command]
#[instrument(skip(ftp_manager, app_handle), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_download(
    ftp_session_id: String,
    remote_path: String,
    local_path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
    app_handle: AppHandle,
) -> Result<String, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let transfer_id = uuid::Uuid::new_v4().to_string();
    let token = CancellationToken::new();
    let manager = Arc::clone(&ftp_manager);
    manager.insert_transfer(transfer_id.clone(), token.clone());
    let tid = transfer_id.clone();
    tokio::spawn(async move {
        let result = download_to(&session, &remote_path, Path::new(&local_path), &token).await;
        manager.remove_transfer(&tid);
        let (_, file_name) = anyscp_ftp::parent_and_name(&remote_path);
        let _ = app_handle.emit(
            "ftp:progress",
            &TransferProgress {
                transfer_id: tid,
                ftp_session_id,
                file_name,
                direction: TransferDirection::Download,
                bytes_transferred: 0,
                total_bytes: 0,
                status: status_of(&result),
            },
        );
    });
    Ok(transfer_id)
}

#[tauri::command]
#[instrument(skip(ftp_manager, app_handle), fields(ftp_session_id = %ftp_session_id))]
pub async fn ftp_upload(
    ftp_session_id: String,
    local_path: String,
    remote_path: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
    app_handle: AppHandle,
) -> Result<String, FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let transfer_id = uuid::Uuid::new_v4().to_string();
    let token = CancellationToken::new();
    let manager = Arc::clone(&ftp_manager);
    manager.insert_transfer(transfer_id.clone(), token.clone());
    let tid = transfer_id.clone();
    tokio::spawn(async move {
        let result = upload_from(&session, Path::new(&local_path), &remote_path, &token).await;
        manager.remove_transfer(&tid);
        let file_name = Path::new(&local_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let _ = app_handle.emit(
            "ftp:progress",
            &TransferProgress {
                transfer_id: tid,
                ftp_session_id,
                file_name,
                direction: TransferDirection::Upload,
                bytes_transferred: 0,
                total_bytes: 0,
                status: status_of(&result),
            },
        );
    });
    Ok(transfer_id)
}

#[tauri::command]
pub async fn ftp_cancel_transfer(
    transfer_id: String,
    ftp_manager: State<'_, Arc<FtpManager>>,
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<(), FtpError> {
    if transfer_manager.cancel(&transfer_id).is_ok() {
        return Ok(());
    }
    ftp_manager.cancel_transfer(&transfer_id)
}

/// Download to a temp file, open it in an external editor, and upload it back
/// on every save for 30 minutes. Same flow as `scp_edit_external`.
#[tauri::command]
#[instrument(skip(ftp_manager, app_handle, editor), fields(ftp_session_id = %ftp_session_id, remote_path = %remote_path))]
pub async fn ftp_edit_external(
    ftp_session_id: String,
    remote_path: String,
    editor: Option<crate::editors::EditorConfig>,
    ftp_manager: State<'_, Arc<FtpManager>>,
    app_handle: AppHandle,
) -> Result<(), FtpError> {
    let session = ftp_manager.get(&ftp_session_id)?;
    let (_, file_name) = anyscp_ftp::parent_and_name(&remote_path);
    let key = format!("{ftp_session_id}\0{remote_path}");
    let local_path = crate::editors::edit_temp_path(&key, &file_name);

    let token = CancellationToken::new();
    download_to(&session, &remote_path, &local_path, &token).await?;

    let editor = editor
        .or_else(crate::editors::resolve_default)
        .ok_or_else(|| {
            FtpError::LocalIoError("No editor found. Add one in Settings → Editors.".to_string())
        })?;
    crate::editors::launch(&editor, &local_path).map_err(FtpError::LocalIoError)?;

    let rt = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        use notify::{Config, Event, EventKind, RecursiveMode, Watcher};
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<Event>();
        let watcher = notify::RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default(),
        );
        let Ok(mut watcher) = watcher else {
            tracing::error!("could not create file watcher for FTP edit");
            return;
        };
        if watcher
            .watch(&local_path, RecursiveMode::NonRecursive)
            .is_err()
        {
            tracing::error!("could not watch {}", local_path.display());
            return;
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(event) => {
                    let is_write = matches!(
                        event.kind,
                        EventKind::Modify(notify::event::ModifyKind::Data(_))
                            | EventKind::Modify(notify::event::ModifyKind::Any)
                    );
                    if !is_write {
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let session = session.clone();
                    let remote_path = remote_path.clone();
                    let local_path = local_path.clone();
                    let app_handle = app_handle.clone();
                    let sid = ftp_session_id.clone();
                    rt.spawn(async move {
                        let token = CancellationToken::new();
                        match upload_from(&session, &local_path, &remote_path, &token).await {
                            Ok(()) => {
                                tracing::info!(remote_path = %remote_path, "File re-uploaded on save (ftp)");
                                let _ = app_handle.emit("ftp:file-edited", &sid);
                            }
                            Err(e) => tracing::error!(error = %e, "Failed to re-upload on save (ftp)"),
                        }
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if std::time::Instant::now() > deadline {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        crate::editors::edit_temp_cleanup(&local_path);
    });

    Ok(())
}

// ─── Transfer manager commands ───────────────────────────────────────────────

#[tauri::command]
pub async fn ftp_enqueue_upload(
    ftp_session_id: String,
    local_paths: Vec<String>,
    remote_dir: String,
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<Vec<String>, FtpError> {
    let paths: Vec<PathBuf> = local_paths.into_iter().map(PathBuf::from).collect();
    transfer_manager
        .enqueue_upload(ftp_session_id, paths, remote_dir)
        .await
}

#[tauri::command]
pub async fn ftp_enqueue_download(
    ftp_session_id: String,
    remote_paths: Vec<String>,
    local_dir: String,
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<Vec<String>, FtpError> {
    transfer_manager
        .enqueue_download(ftp_session_id, remote_paths, PathBuf::from(local_dir))
        .await
}

#[tauri::command]
pub async fn ftp_retry_transfer(
    transfer_id: String,
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<String, FtpError> {
    transfer_manager.retry(&transfer_id)?;
    Ok(transfer_id)
}

#[tauri::command]
pub async fn ftp_list_transfers(
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<Vec<TransferInfo>, FtpError> {
    Ok(transfer_manager.list_all())
}

#[tauri::command]
pub async fn ftp_clear_finished_transfers(
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<(), FtpError> {
    transfer_manager.clear_finished();
    Ok(())
}

#[tauri::command]
pub async fn ftp_set_concurrency(
    max_concurrent: u32,
    transfer_manager: State<'_, Arc<FtpTransferManager>>,
) -> Result<(), FtpError> {
    if max_concurrent == 0 {
        return Err(FtpError::ProtocolError(
            "max_concurrent must be at least 1".to_string(),
        ));
    }
    transfer_manager.set_max_concurrent(max_concurrent);
    Ok(())
}
