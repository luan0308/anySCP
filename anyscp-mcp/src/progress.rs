//! MCP progress notifications.
//!
//! When the client sends a `progressToken`, long uploads and downloads report
//! progress. Besides keeping the user informed, clients that reset their
//! request timeout on progress then let big transfers finish.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub struct Progress {
    token: Option<Value>,
    tx: Option<UnboundedSender<Value>>,
    bytes: AtomicU64,
    ticks: AtomicU64,
    last_sent: Mutex<Instant>,
}

impl Progress {
    pub fn new(token: Option<Value>, tx: UnboundedSender<Value>) -> Self {
        Self {
            tx: token.as_ref().map(|_| tx),
            token,
            bytes: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            last_sent: Mutex::new(Instant::now() - Duration::from_secs(10)),
        }
    }

    fn send(&self, message: String) {
        let (Some(token), Some(tx)) = (&self.token, &self.tx) else {
            return;
        };
        // `progress` must only ever grow; ticks keep it moving even when a
        // step moves no bytes (creating a folder, starting a file).
        let value = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = tx.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": { "progressToken": token, "progress": value, "message": message }
        }));
        if let Ok(mut t) = self.last_sent.lock() {
            *t = Instant::now();
        }
    }

    /// A step started (a new file, a new folder).
    pub async fn note(&self, message: &str) {
        self.send(message.to_string());
    }

    /// Bytes moved. Throttled to about one notification per second.
    pub fn add_bytes(&self, n: u64) {
        let total = self.bytes.fetch_add(n, Ordering::Relaxed) + n;
        if self.token.is_none() {
            return;
        }
        let due = self
            .last_sent
            .lock()
            .map(|t| t.elapsed() >= Duration::from_secs(1))
            .unwrap_or(false);
        if due {
            self.send(format!("{} transferred", human_bytes(total)));
        }
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
