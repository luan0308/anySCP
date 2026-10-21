//! Keeps one live connection per server so a sequence of tool calls does not
//! log in again every time. Idle connections are closed after a while.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::known_hosts::KnownHosts;
use crate::remote::ftp::FtpRemote;
use crate::remote::sftp::SftpRemote;
use crate::remote::Remote;
use crate::store::{Protocol, Server, Store};

const RECHECK_AFTER: Duration = Duration::from_secs(30);
const IDLE_CLOSE: Duration = Duration::from_secs(10 * 60);

pub struct Slot {
    pub conn: Option<Box<dyn Remote>>,
    last_used: Instant,
}

impl Slot {
    pub fn remote(&mut self) -> &mut dyn Remote {
        self.conn.as_deref_mut().expect("acquire() always connects")
    }

    /// Drop the connection if it no longer answers, so the next call
    /// reconnects instead of failing again.
    pub async fn check_after_error(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            if !c.ping().await {
                c.close().await;
                self.conn = None;
            }
        }
    }
}

pub struct Pool {
    slots: Mutex<HashMap<String, Arc<AsyncMutex<Slot>>>>,
    store: Arc<Store>,
    known: Arc<KnownHosts>,
}

impl Pool {
    pub fn new(store: Arc<Store>) -> Arc<Self> {
        let known = Arc::new(KnownHosts::new(store.data_dir()));
        let pool = Arc::new(Self {
            slots: Mutex::new(HashMap::new()),
            store,
            known,
        });
        let weak = Arc::downgrade(&pool);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                let Some(pool) = weak.upgrade() else { break };
                pool.reap().await;
            }
        });
        pool
    }

    pub async fn acquire(&self, server: &Server) -> Result<OwnedMutexGuard<Slot>> {
        let slot = {
            let mut map = self.slots.lock().unwrap();
            map.entry(server.id.clone())
                .or_insert_with(|| {
                    Arc::new(AsyncMutex::new(Slot {
                        conn: None,
                        last_used: Instant::now(),
                    }))
                })
                .clone()
        };
        let mut guard = slot.lock_owned().await;
        if guard.last_used.elapsed() > RECHECK_AFTER {
            if let Some(c) = guard.conn.as_mut() {
                if !c.ping().await {
                    c.close().await;
                    guard.conn = None;
                }
            }
        }
        if guard.conn.is_none() {
            guard.conn = Some(self.open(server).await?);
        }
        guard.last_used = Instant::now();
        Ok(guard)
    }

    async fn open(&self, server: &Server) -> Result<Box<dyn Remote>> {
        Ok(match server.protocol {
            Protocol::Sftp => {
                Box::new(SftpRemote::connect(server, &self.store, self.known.clone()).await?)
            }
            Protocol::Ftp => Box::new(FtpRemote::connect(server).await?),
        })
    }

    async fn reap(&self) {
        let slots: Vec<_> = self.slots.lock().unwrap().values().cloned().collect();
        for slot in slots {
            if let Ok(mut g) = slot.try_lock() {
                if g.conn.is_some() && g.last_used.elapsed() > IDLE_CLOSE {
                    if let Some(mut c) = g.conn.take() {
                        c.close().await;
                    }
                }
            }
        }
    }
}
