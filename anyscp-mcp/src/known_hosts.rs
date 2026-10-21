//! Trust-on-first-use for SSH host keys.
//!
//! The first time Claude connects to a server its host key fingerprint is
//! remembered. If the key ever changes (server reinstalled, or someone in the
//! middle) the connection is refused until the user removes the old entry.

use anyhow::Result;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub enum Verdict {
    Trusted,
    FirstSeen,
    Mismatch { expected: String },
}

pub struct KnownHosts {
    path: PathBuf,
    lock: Mutex<()>,
}

impl KnownHosts {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            path: dir.join("mcp_known_hosts.json"),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn check(&self, host_port: &str, fingerprint: &str) -> Result<Verdict> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.load();
        match map.get(host_port) {
            Some(fp) if fp == fingerprint => Ok(Verdict::Trusted),
            Some(fp) => Ok(Verdict::Mismatch {
                expected: fp.clone(),
            }),
            None => {
                map.insert(host_port.to_string(), fingerprint.to_string());
                if let Some(dir) = self.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                std::fs::write(&self.path, serde_json::to_string_pretty(&map)?)?;
                Ok(Verdict::FirstSeen)
            }
        }
    }
}
