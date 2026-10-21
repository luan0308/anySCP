//! Read-only access to the servers saved in Talktoall Files.
//!
//! anySCP keeps its server list in a SQLite database inside the app data
//! folder and the secrets in the OS credential store (Windows Credential
//! Manager, macOS Keychain, Linux keyutils). This module reads both, so the
//! user registers each server once, in the app, and Claude reaches it by name
//! without ever seeing a password.

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Bundle identifier of the Talktoall Files app; Tauri stores app data under it.
const APP_IDENTIFIER: &str = "br.com.talktoall.files";
/// Identifier used before the app was renamed (anySCP). Read when the new
/// folder does not exist yet, i.e. the app was not opened since the update.
const LEGACY_APP_IDENTIFIER: &str = "com.macnev2013.anyscp";
/// Keychain service namespace used by the app's vault module.
const KEYRING_SERVICE: &str = "br.com.talktoall.files";
/// Namespace the app used as anySCP; still read as a fallback.
const LEGACY_KEYRING_SERVICE: &str = "com.anyscp.credentials";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Sftp,
    Ftp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtpSecurity {
    /// Plain FTP, everything in clear text.
    None,
    /// FTPS via AUTH TLS on the normal port (the common case).
    Explicit,
    /// FTPS with TLS from the first byte, usually port 990.
    Implicit,
}

#[derive(Debug, Clone)]
pub struct Server {
    pub id: String,
    pub label: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group: Option<String>,
    pub environment: Option<String>,
    pub notes: Option<String>,
    // SFTP
    pub auth_type: String,
    pub key_path: Option<String>,
    pub proxy_jump_id: Option<String>,
    // FTP
    pub ftp_security: FtpSecurity,
    pub ftp_passive: bool,
    pub ftp_allow_invalid_cert: bool,
    pub initial_path: Option<String>,
}

impl Server {
    pub fn protocol_label(&self) -> &'static str {
        match (self.protocol, self.ftp_security) {
            (Protocol::Sftp, _) => "sftp",
            (Protocol::Ftp, FtpSecurity::None) => "ftp",
            (Protocol::Ftp, FtpSecurity::Explicit) => "ftps",
            (Protocol::Ftp, FtpSecurity::Implicit) => "ftps-implicit",
        }
    }
}

/// Mirrors `vault::StoredCredential` in the app.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum StoredCredential {
    Password { password: String },
    KeyPassphrase { passphrase: String },
}

pub struct Store {
    db_path: PathBuf,
}

impl Store {
    /// Find anySCP's database: `ANYSCP_DB` wins, otherwise the standard
    /// Tauri app-data folder for this platform.
    pub fn locate() -> Result<Self> {
        for var in ["TALKTOALL_FILES_DB", "ANYSCP_DB"] {
            if let Ok(p) = std::env::var(var) {
                let path = PathBuf::from(p);
                if !path.exists() {
                    bail!(
                        "{var} points to {} but that file does not exist",
                        path.display()
                    );
                }
                return Ok(Self { db_path: path });
            }
        }
        let base =
            dirs::data_dir().ok_or_else(|| anyhow!("could not resolve the user data folder"))?;
        let path = base.join(APP_IDENTIFIER).join("anyscp.db");
        let legacy = base.join(LEGACY_APP_IDENTIFIER).join("anyscp.db");
        if !path.exists() && legacy.exists() {
            return Ok(Self { db_path: legacy });
        }
        Ok(Self { db_path: path })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Folder for files the MCP server owns (known host keys).
    pub fn data_dir(&self) -> PathBuf {
        self.db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn open(&self) -> Result<Connection> {
        if !self.db_path.exists() {
            bail!(
                "Talktoall Files database not found at {}. Open Talktoall Files at least once and save a server first.",
                self.db_path.display()
            );
        }
        let conn = Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("could not open {}", self.db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        Ok(conn)
    }

    /// Every server saved in Talktoall Files that Claude can reach (SSH hosts over
    /// SFTP, and FTP/FTPS hosts). S3 connections are not exposed.
    pub fn list(&self) -> Result<Vec<Server>> {
        let conn = self.open()?;
        let mut out = Vec::new();
        out.extend(read_ssh_hosts(&conn)?);
        out.sort_by_key(|s| s.label.to_lowercase());
        Ok(out)
    }

    /// Resolve what the user or model typed (label, id, or an unambiguous
    /// piece of a label) to exactly one server.
    pub fn find(&self, query: &str) -> Result<Server> {
        let all = self.list()?;
        if all.is_empty() {
            bail!("No servers are saved in Talktoall Files yet. Add one in the app first.");
        }
        let q = query.trim().to_lowercase();
        if let Some(s) = all.iter().find(|s| s.id.to_lowercase() == q) {
            return Ok(s.clone());
        }
        let exact: Vec<&Server> = all.iter().filter(|s| s.label.to_lowercase() == q).collect();
        if exact.len() == 1 {
            return Ok(exact[0].clone());
        }
        let partial: Vec<&Server> = if exact.len() > 1 {
            exact
        } else {
            all.iter()
                .filter(|s| s.label.to_lowercase().contains(&q) || s.host.to_lowercase() == q)
                .collect()
        };
        match partial.len() {
            1 => Ok(partial[0].clone()),
            0 => bail!(
                "No saved server matches \"{query}\". Available: {}",
                all.iter()
                    .map(|s| s.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => bail!(
                "\"{query}\" matches more than one server: {}. Use the exact name or the id.",
                partial
                    .iter()
                    .map(|s| format!("{} (id {})", s.label, s.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// Read a saved server by id (used for ProxyJump chains).
    pub fn get_ssh_host(&self, id: &str) -> Result<Server> {
        let conn = self.open()?;
        read_ssh_hosts(&conn)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| anyhow!("tunnel host {id} is no longer saved in Talktoall Files"))
    }
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(cols)
}

/// Builds `col` when it exists, otherwise `NULL AS col`, so the queries keep
/// working against older and newer anySCP schemas.
fn col_or_null(cols: &[String], col: &str) -> String {
    if cols.iter().any(|c| c == col) {
        format!("t.{col}")
    } else {
        format!("NULL AS {col}")
    }
}

fn read_ssh_hosts(conn: &Connection) -> Result<Vec<Server>> {
    if !table_exists(conn, "saved_hosts")? {
        return Ok(Vec::new());
    }
    let cols = columns(conn, "saved_hosts")?;
    let has_groups = table_exists(conn, "host_groups")? && cols.iter().any(|c| c == "group_id");
    let sql = format!(
        "SELECT t.id, t.label, t.host, t.port, t.username, t.auth_type, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}
         FROM saved_hosts t {}",
        col_or_null(&cols, "key_path"),
        col_or_null(&cols, "proxy_jump_host_id"),
        col_or_null(&cols, "environment"),
        col_or_null(&cols, "notes"),
        if has_groups { "g.name" } else { "NULL" },
        col_or_null(&cols, "protocol"),
        col_or_null(&cols, "ftp_security"),
        col_or_null(&cols, "ftp_passive"),
        col_or_null(&cols, "ftp_allow_invalid_cert"),
        col_or_null(&cols, "start_directory"),
        if has_groups { "LEFT JOIN host_groups g ON g.id = t.group_id" } else { "" },
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        let protocol: Option<String> = r.get(11)?;
        let is_ftp = protocol.as_deref() == Some("ftp");
        let security: Option<String> = r.get(12)?;
        let start_dir: Option<String> = r
            .get::<_, Option<String>>(15)?
            .filter(|s| !s.trim().is_empty());
        Ok(Server {
            id: r.get(0)?,
            label: r.get(1)?,
            protocol: if is_ftp {
                Protocol::Ftp
            } else {
                Protocol::Sftp
            },
            host: r.get(2)?,
            port: r.get::<_, i64>(3)? as u16,
            username: r.get(4)?,
            auth_type: r.get(5)?,
            key_path: r.get(6)?,
            proxy_jump_id: r.get::<_, Option<String>>(7)?.filter(|s| !s.is_empty()),
            environment: r.get(8)?,
            notes: r.get(9)?,
            group: r.get(10)?,
            ftp_security: match security.as_deref() {
                Some("explicit") => FtpSecurity::Explicit,
                Some("implicit") => FtpSecurity::Implicit,
                _ => FtpSecurity::None,
            },
            ftp_passive: r.get::<_, Option<i64>>(13)?.unwrap_or(1) != 0,
            ftp_allow_invalid_cert: r.get::<_, Option<i64>>(14)?.unwrap_or(0) != 0,
            // The app's "start directory" doubles as the FTP starting folder.
            initial_path: if is_ftp {
                start_dir.filter(|p| p.starts_with('/'))
            } else {
                None
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The keychain entry name anySCP uses for this server's secret: the host id,
/// for SSH and FTP hosts alike.
fn vault_key(server: &Server) -> String {
    server.id.clone()
}

/// Fetch the secret from the OS credential store. Returns `None` when the
/// app saved the server without one (e.g. an unencrypted private key).
pub fn credential(server: &Server) -> Result<Option<StoredCredential>> {
    let key = vault_key(server);
    for service in [KEYRING_SERVICE, LEGACY_KEYRING_SERVICE] {
        let entry = keyring::Entry::new(service, &key)
            .map_err(|e| anyhow!("credential store unavailable: {e}"))?;
        match entry.get_password() {
            Ok(json) => {
                let cred: StoredCredential = serde_json::from_str(&json).map_err(|e| {
                    anyhow!(
                        "credential for {} is in an unexpected format: {e}",
                        server.label
                    )
                })?;
                return Ok(Some(cred));
            }
            Err(keyring::Error::NoEntry) => continue,
            Err(e) => {
                return Err(anyhow!(
                    "could not read the credential for {}: {e}",
                    server.label
                ))
            }
        }
    }
    Ok(None)
}
