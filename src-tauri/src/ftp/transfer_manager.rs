//! Queue-based, concurrency-limited transfer manager for FTP, the FTP
//! analogue of `scp::transfer_manager`. Every running job opens its own FTP
//! connection (FTP allows one transfer per control connection), so browsing
//! and other transfers are never blocked behind it.
//!
//! Progress goes out on the `ftp:transfer` channel with the same event shape
//! as SCP/SFTP.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyscp_ftp::{FtpClient, FtpEntry};
use dashmap::DashMap;
use tauri::{AppHandle, Emitter};

/// Where progress events go: the frontend in the app, nowhere in tests.
pub type Emit = Arc<dyn Fn(TransferEvent) + Send + Sync>;
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::scp::{TransferDirection, TransferStatus};
use crate::transfer_common::{
    apply_concurrency, eta_secs, record_finished, record_progress, FinishedStatus, ProgressFields,
};

use super::{FtpError, FtpManager, TransferEvent, TransferInfo};

const EVENT: &str = "ftp:transfer";

// ─── Job state ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum TransferJobKind {
    UploadFile {
        local_path: PathBuf,
        remote_path: String,
    },
    UploadDir {
        local_path: PathBuf,
        remote_dir: String,
    },
    DownloadFile {
        remote_path: String,
        local_path: PathBuf,
    },
    DownloadDir {
        remote_path: String,
        local_dir: PathBuf,
    },
}

pub struct TransferJobState {
    pub transfer_id: String,
    pub ftp_session_id: String,
    pub name: String,
    pub direction: TransferDirection,
    pub kind: TransferJobKind,
    pub status: TransferStatus,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub speed_bps: u64,
    pub cancel_token: CancellationToken,
    pub error: Option<String>,
    pub created_at: u64,
    pub last_emit: Instant,
    pub speed_window_bytes: u64,
    pub speed_window_start: Instant,
}

impl TransferJobState {
    fn to_event(&self) -> TransferEvent {
        TransferEvent {
            transfer_id: self.transfer_id.clone(),
            ftp_session_id: self.ftp_session_id.clone(),
            name: self.name.clone(),
            direction: self.direction.clone(),
            status: self.status.clone(),
            error: self.error.clone(),
            bytes_transferred: self.bytes_transferred,
            total_bytes: self.total_bytes,
            files_done: self.files_done,
            files_total: self.files_total,
            speed_bps: self.speed_bps,
            eta_secs: eta_secs(self.speed_bps, self.total_bytes, self.bytes_transferred),
            created_at: self.created_at,
        }
    }
}

impl ProgressFields for TransferJobState {
    fn bytes_transferred(&mut self) -> &mut u64 {
        &mut self.bytes_transferred
    }
    fn speed_bps(&mut self) -> &mut u64 {
        &mut self.speed_bps
    }
    fn speed_window_bytes(&mut self) -> &mut u64 {
        &mut self.speed_window_bytes
    }
    fn speed_window_start(&mut self) -> &mut Instant {
        &mut self.speed_window_start
    }
    fn last_emit(&mut self) -> &mut Instant {
        &mut self.last_emit
    }
}

impl FinishedStatus for TransferJobState {
    fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TransferStatus::Completed | TransferStatus::Failed(_) | TransferStatus::Cancelled
        )
    }
}

type Jobs = Arc<DashMap<String, TransferJobState>>;
type Finished = Arc<std::sync::Mutex<VecDeque<String>>>;

// ─── Manager ─────────────────────────────────────────────────────────────────

pub struct FtpTransferManager {
    jobs: Jobs,
    finished_order: Finished,
    queue_tx: mpsc::UnboundedSender<String>,
    semaphore: Arc<Semaphore>,
    ftp_manager: Arc<FtpManager>,
    emit: Emit,
    max_concurrent: Arc<AtomicU32>,
    worker_rx: Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<String>>>>,
}

impl FtpTransferManager {
    pub fn new(ftp_manager: Arc<FtpManager>, app_handle: AppHandle) -> Self {
        let emit: Emit = Arc::new(move |event| {
            let _ = app_handle.emit(EVENT, event);
        });
        Self::with_emitter(ftp_manager, emit)
    }

    pub fn with_emitter(ftp_manager: Arc<FtpManager>, emit: Emit) -> Self {
        let (queue_tx, queue_rx) = mpsc::unbounded_channel::<String>();
        Self {
            jobs: Arc::new(DashMap::new()),
            finished_order: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            queue_tx,
            // Shared hosts often cap simultaneous logins per account; keep the
            // default parallelism modest.
            semaphore: Arc::new(Semaphore::new(2)),
            ftp_manager,
            emit,
            max_concurrent: Arc::new(AtomicU32::new(2)),
            worker_rx: Arc::new(std::sync::Mutex::new(Some(queue_rx))),
        }
    }

    fn ensure_worker_spawned(&self) {
        let mut guard = self.worker_rx.lock().expect("worker_rx mutex poisoned");
        if let Some(mut queue_rx) = guard.take() {
            let jobs = self.jobs.clone();
            let finished_order = self.finished_order.clone();
            let semaphore = self.semaphore.clone();
            let ftp_manager = self.ftp_manager.clone();
            let emit = self.emit.clone();

            tokio::spawn(async move {
                while let Some(job_id) = queue_rx.recv().await {
                    let permit = semaphore
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("semaphore closed");
                    let jobs = jobs.clone();
                    let finished_order = finished_order.clone();
                    let ftp_manager = ftp_manager.clone();
                    let emit = emit.clone();
                    tokio::spawn(async move {
                        execute_transfer(&jobs, &finished_order, &job_id, &ftp_manager, &emit)
                            .await;
                        drop(permit);
                    });
                }
            });
        }
    }

    fn unix_now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn insert_and_queue(&self, job: TransferJobState) -> Result<String, FtpError> {
        let id = job.transfer_id.clone();
        let event = job.to_event();
        self.jobs.insert(id.clone(), job);
        (self.emit)(event);
        self.queue_tx
            .send(id.clone())
            .map_err(|e| FtpError::ChannelError(e.to_string()))?;
        Ok(id)
    }

    fn new_job(
        ftp_session_id: &str,
        name: String,
        direction: TransferDirection,
        kind: TransferJobKind,
        total_bytes: u64,
        files_total: u32,
    ) -> TransferJobState {
        let now = Instant::now();
        TransferJobState {
            transfer_id: uuid::Uuid::new_v4().to_string(),
            ftp_session_id: ftp_session_id.to_string(),
            name,
            direction,
            kind,
            status: TransferStatus::Queued,
            bytes_transferred: 0,
            total_bytes,
            files_done: 0,
            files_total,
            speed_bps: 0,
            cancel_token: CancellationToken::new(),
            error: None,
            created_at: Self::unix_now_millis(),
            last_emit: now,
            speed_window_bytes: 0,
            speed_window_start: now,
        }
    }

    #[instrument(skip(self), fields(ftp_session_id = %ftp_session_id))]
    pub async fn enqueue_upload(
        &self,
        ftp_session_id: String,
        local_paths: Vec<PathBuf>,
        remote_dir: String,
    ) -> Result<Vec<String>, FtpError> {
        self.ftp_manager.get(&ftp_session_id)?;
        self.ensure_worker_spawned();
        let mut ids = Vec::with_capacity(local_paths.len());
        for local_path in local_paths {
            let meta = tokio::fs::metadata(&local_path)
                .await
                .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
            let name = local_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let remote_path = anyscp_ftp::join(&remote_dir, &name);
            let job = if meta.is_dir() {
                let (bytes, count) = walk_local_dir_stats(&local_path).await;
                Self::new_job(
                    &ftp_session_id,
                    name,
                    TransferDirection::Upload,
                    TransferJobKind::UploadDir {
                        local_path,
                        remote_dir: remote_path,
                    },
                    bytes,
                    count,
                )
            } else {
                Self::new_job(
                    &ftp_session_id,
                    name,
                    TransferDirection::Upload,
                    TransferJobKind::UploadFile {
                        local_path,
                        remote_path,
                    },
                    meta.len(),
                    1,
                )
            };
            ids.push(self.insert_and_queue(job)?);
        }
        Ok(ids)
    }

    #[instrument(skip(self), fields(ftp_session_id = %ftp_session_id))]
    pub async fn enqueue_download(
        &self,
        ftp_session_id: String,
        remote_paths: Vec<String>,
        local_dir: PathBuf,
    ) -> Result<Vec<String>, FtpError> {
        let session = self.ftp_manager.get(&ftp_session_id)?;
        self.ensure_worker_spawned();
        let mut ids = Vec::with_capacity(remote_paths.len());
        for remote_path in remote_paths {
            let entry = {
                let mut c = session.client().await?;
                c.0.stat(&remote_path).await?
            }
            .ok_or_else(|| FtpError::NotFound(remote_path.clone()))?;
            let local_dest = local_dir.join(&entry.name);
            // Folder sizes are counted when the job starts, on its own
            // connection, rather than here where it would block the UI.
            let job = if entry.is_dir() {
                Self::new_job(
                    &ftp_session_id,
                    entry.name.clone(),
                    TransferDirection::Download,
                    TransferJobKind::DownloadDir {
                        remote_path,
                        local_dir: local_dest,
                    },
                    0,
                    0,
                )
            } else {
                Self::new_job(
                    &ftp_session_id,
                    entry.name.clone(),
                    TransferDirection::Download,
                    TransferJobKind::DownloadFile {
                        remote_path,
                        local_path: local_dest,
                    },
                    entry.size,
                    1,
                )
            };
            ids.push(self.insert_and_queue(job)?);
        }
        Ok(ids)
    }

    pub fn cancel(&self, transfer_id: &str) -> Result<(), FtpError> {
        let mut job = self.jobs.get_mut(transfer_id).ok_or_else(|| {
            FtpError::SessionNotFound(format!("transfer not found: {transfer_id}"))
        })?;
        job.cancel_token.cancel();
        if job.status == TransferStatus::Queued {
            job.status = TransferStatus::Cancelled;
            let event = job.to_event();
            drop(job);
            (self.emit)(event);
            record_finished(&self.jobs, &self.finished_order, transfer_id);
        }
        Ok(())
    }

    pub fn retry(&self, transfer_id: &str) -> Result<(), FtpError> {
        self.ensure_worker_spawned();
        {
            let mut job = self.jobs.get_mut(transfer_id).ok_or_else(|| {
                FtpError::SessionNotFound(format!("transfer not found: {transfer_id}"))
            })?;
            if !matches!(
                &job.status,
                TransferStatus::Failed(_) | TransferStatus::Cancelled
            ) {
                return Err(FtpError::ProtocolError(format!(
                    "transfer {transfer_id} is not in a failed/cancelled state"
                )));
            }
            job.status = TransferStatus::Queued;
            job.bytes_transferred = 0;
            job.files_done = 0;
            job.speed_bps = 0;
            job.error = None;
            job.cancel_token = CancellationToken::new();
            job.last_emit = Instant::now();
            job.speed_window_bytes = 0;
            job.speed_window_start = Instant::now();
            let event = job.to_event();
            drop(job);
            (self.emit)(event);
        }
        self.queue_tx
            .send(transfer_id.to_string())
            .map_err(|e| FtpError::ChannelError(e.to_string()))?;
        Ok(())
    }

    pub fn list_all(&self) -> Vec<TransferInfo> {
        self.jobs.iter().map(|r| r.value().to_event()).collect()
    }

    pub fn clear_finished(&self) {
        self.jobs.retain(|_, job| !job.is_terminal());
    }

    pub fn set_max_concurrent(&self, n: u32) {
        apply_concurrency(&self.semaphore, &self.max_concurrent, n);
    }
}

// ─── Local directory statistics ──────────────────────────────────────────────

async fn walk_local_dir_stats(path: &Path) -> (u64, u32) {
    let mut visited = HashSet::new();
    Box::pin(walk_local_dir_inner(path, &mut visited)).await
}

async fn walk_local_dir_inner(path: &Path, visited: &mut HashSet<PathBuf>) -> (u64, u32) {
    let Ok(canonical) = tokio::fs::canonicalize(path).await else {
        return (0, 0);
    };
    if !visited.insert(canonical) {
        return (0, 0);
    }
    let (mut bytes, mut count) = (0u64, 0u32);
    let Ok(mut rd) = tokio::fs::read_dir(path).await else {
        return (0, 0);
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if meta.is_dir() {
            let (b, c) = Box::pin(walk_local_dir_inner(&entry.path(), visited)).await;
            bytes += b;
            count += c;
        } else {
            bytes += meta.len();
            count += 1;
        }
    }
    (bytes, count)
}

// ─── Execute ─────────────────────────────────────────────────────────────────

async fn execute_transfer(
    jobs: &Jobs,
    finished_order: &Finished,
    job_id: &str,
    ftp_manager: &Arc<FtpManager>,
    emit: &Emit,
) {
    let Some((sid, kind, cancel)) = jobs.get(job_id).map(|j| {
        (
            j.ftp_session_id.clone(),
            j.kind.clone(),
            j.cancel_token.clone(),
        )
    }) else {
        return;
    };
    if cancel.is_cancelled() {
        set_job_status(
            jobs,
            finished_order,
            job_id,
            TransferStatus::Cancelled,
            None,
            emit,
        );
        return;
    }
    set_job_status(
        jobs,
        finished_order,
        job_id,
        TransferStatus::InProgress,
        None,
        emit,
    );

    let result = async {
        let session = ftp_manager.get(&sid)?;
        let mut client = session.dedicated().await?;
        let ctx = Ctx {
            jobs,
            job_id,
            cancel: &cancel,
            emit,
        };
        let r = match kind {
            TransferJobKind::UploadFile {
                local_path,
                remote_path,
            } => {
                let (parent, _) = anyscp_ftp::parent_and_name(&remote_path);
                client.mkdir_all(&parent).await?;
                upload_one(&mut client, &ctx, &local_path, &remote_path, 0)
                    .await
                    .map(|_| ())
            }
            TransferJobKind::UploadDir {
                local_path,
                remote_dir,
            } => upload_dir(&mut client, &ctx, &local_path, &remote_dir).await,
            TransferJobKind::DownloadFile {
                remote_path,
                local_path,
            } => download_one(&mut client, &ctx, &remote_path, &local_path, 0)
                .await
                .map(|_| ()),
            TransferJobKind::DownloadDir {
                remote_path,
                local_dir,
            } => download_dir(&mut client, &ctx, &remote_path, &local_dir).await,
        };
        client.quit().await;
        r
    }
    .await;

    match result {
        Ok(()) => {
            if let Some(mut job) = jobs.get_mut(job_id) {
                job.bytes_transferred = job.total_bytes;
            }
            set_job_status(
                jobs,
                finished_order,
                job_id,
                TransferStatus::Completed,
                None,
                emit,
            );
        }
        Err(FtpError::TransferCancelled) => set_job_status(
            jobs,
            finished_order,
            job_id,
            TransferStatus::Cancelled,
            None,
            emit,
        ),
        Err(e) => set_job_status(
            jobs,
            finished_order,
            job_id,
            TransferStatus::Failed(e.to_string()),
            Some(e.to_string()),
            emit,
        ),
    }
}

fn set_job_status(
    jobs: &Jobs,
    finished_order: &Finished,
    job_id: &str,
    status: TransferStatus,
    error: Option<String>,
    emit: &Emit,
) {
    let Some(mut job) = jobs.get_mut(job_id) else {
        return;
    };
    // A terminal status is final (see the SCP manager for the race this avoids).
    if job.is_terminal() {
        return;
    }
    job.status = status;
    job.error = error;
    let event = job.to_event();
    let terminal = job.is_terminal();
    drop(job);
    emit(event);
    if terminal {
        record_finished(jobs, finished_order, job_id);
    }
}

struct Ctx<'a> {
    jobs: &'a Jobs,
    job_id: &'a str,
    cancel: &'a CancellationToken,
    emit: &'a Emit,
}

impl Ctx<'_> {
    fn report(&self, cumulative: u64) {
        if let Some(mut job) = self.jobs.get_mut(self.job_id) {
            let delta = cumulative.saturating_sub(job.bytes_transferred);
            if record_progress(&mut *job, delta) {
                let event = job.to_event();
                drop(job);
                (self.emit)(event);
            }
        }
    }

    fn file_done(&self) {
        if let Some(mut job) = self.jobs.get_mut(self.job_id) {
            job.files_done += 1;
        }
    }

    fn set_totals(&self, bytes: u64, files: u32) {
        if let Some(mut job) = self.jobs.get_mut(self.job_id) {
            job.total_bytes = bytes;
            job.files_total = files;
        }
    }

    fn check(&self) -> Result<(), FtpError> {
        if self.cancel.is_cancelled() {
            Err(FtpError::TransferCancelled)
        } else {
            Ok(())
        }
    }
}

async fn upload_one(
    client: &mut FtpClient,
    ctx: &Ctx<'_>,
    local: &Path,
    remote: &str,
    base: u64,
) -> Result<u64, FtpError> {
    ctx.check()?;
    let mut src = tokio::fs::File::open(local)
        .await
        .map_err(|e| FtpError::LocalIoError(format!("{}: {e}", local.display())))?;
    let mut done = 0u64;
    let n = client
        .upload(remote, &mut src, |n| {
            done += n;
            ctx.report(base + done);
            !ctx.cancel.is_cancelled()
        })
        .await?;
    ctx.file_done();
    Ok(n)
}

async fn download_one(
    client: &mut FtpClient,
    ctx: &Ctx<'_>,
    remote: &str,
    local: &Path,
    base: u64,
) -> Result<u64, FtpError> {
    ctx.check()?;
    if let Some(parent) = local.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
    }
    // Write to a .part file and rename on success, so an interrupted download
    // never leaves a truncated file under the real name.
    let mut tmp_name = local
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    tmp_name.push(".part");
    let tmp = local.with_file_name(tmp_name);
    let mut dst = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| FtpError::LocalIoError(format!("{}: {e}", tmp.display())))?;
    let mut done = 0u64;
    let result = client
        .download(remote, &mut dst, |n| {
            done += n;
            ctx.report(base + done);
            !ctx.cancel.is_cancelled()
        })
        .await;
    drop(dst);
    match result {
        Ok(n) => {
            tokio::fs::rename(&tmp, local)
                .await
                .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
            ctx.file_done();
            Ok(n)
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e.into())
        }
    }
}

async fn upload_dir(
    client: &mut FtpClient,
    ctx: &Ctx<'_>,
    local_root: &Path,
    remote_root: &str,
) -> Result<(), FtpError> {
    client.mkdir_all(remote_root).await?;
    let mut base = 0u64;
    let mut stack = vec![(local_root.to_path_buf(), remote_root.to_string())];
    while let Some((dir, remote_dir)) = stack.pop() {
        ctx.check()?;
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
        while let Ok(Some(entry)) = rd.next_entry().await {
            ctx.check()?;
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let remote = anyscp_ftp::join(&remote_dir, &name);
            if meta.is_dir() {
                client.mkdir_all(&remote).await?;
                stack.push((entry.path(), remote));
            } else {
                base += upload_one(client, ctx, &entry.path(), &remote, base).await?;
            }
        }
    }
    Ok(())
}

async fn download_dir(
    client: &mut FtpClient,
    ctx: &Ctx<'_>,
    remote_root: &str,
    local_root: &Path,
) -> Result<(), FtpError> {
    // Enumerate first so the progress bar has real totals.
    let mut files: Vec<FtpEntry> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let mut stack = vec![remote_root.to_string()];
    while let Some(dir) = stack.pop() {
        ctx.check()?;
        for e in client.list(&dir).await? {
            if e.is_dir() {
                stack.push(e.path.clone());
                dirs.push(e.path.clone());
            } else if e.kind == anyscp_ftp::EntryKind::File {
                files.push(e);
            }
        }
    }
    ctx.set_totals(files.iter().map(|f| f.size).sum(), files.len() as u32);

    let rel = |p: &str| {
        p.strip_prefix(remote_root)
            .unwrap_or(p)
            .trim_start_matches('/')
            .to_string()
    };
    tokio::fs::create_dir_all(local_root)
        .await
        .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
    for d in dirs {
        tokio::fs::create_dir_all(local_root.join(rel(&d)))
            .await
            .map_err(|e| FtpError::LocalIoError(e.to_string()))?;
    }
    let mut base = 0u64;
    for f in files {
        let local = local_root.join(rel(&f.path));
        base += download_one(client, ctx, &f.path, &local, base).await?;
    }
    Ok(())
}
