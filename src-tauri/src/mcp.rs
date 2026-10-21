//! Hooking the bundled `anyscp-mcp` server up to Claude.
//!
//! The installer puts `anyscp-mcp` next to the app executable. Claude Desktop
//! finds local MCP servers through its `claude_desktop_config.json`, so this
//! module reports whether anySCP is registered there and adds the entry on
//! request, keeping every other server the user already has.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const SERVER_KEY: &str = "anyscp";

#[derive(Debug, Serialize)]
pub struct McpStatus {
    /// Path of the bundled MCP server, when it exists next to the app.
    pub server_path: Option<String>,
    /// Claude Desktop config files found (or the default location).
    pub config_paths: Vec<String>,
    /// True when every found config already points at `server_path`.
    pub connected: bool,
    /// Ready-to-paste command for Claude Code.
    pub claude_code_command: Option<String>,
    /// Claude Desktop is open (it must be closed to connect).
    pub claude_running: bool,
}

fn server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "anyscp-mcp.exe"
    } else {
        "anyscp-mcp"
    };
    let p = exe.parent()?.join(name);
    p.exists().then_some(p)
}

/// Where Claude Desktop keeps its config on this platform.
///
/// Windows has had three locations over time, and current builds use the
/// first one that exists:
/// - `%LOCALAPPDATA%\Claude-Data\<package>\claude_desktop_config.json`
///   (packaged Claude, current versions)
/// - `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\...`
///   (packaged Claude, older versions; left with only a marker after the move)
/// - `%APPDATA%\Claude\claude_desktop_config.json` (classic installer)
///
/// Writing to a location Claude no longer reads silently does nothing, so
/// when a newer location holds a config the older ones are ignored.
fn config_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        let mut data = Vec::new();
        if let Ok(rd) = std::fs::read_dir(local.join("Claude-Data")) {
            for e in rd.flatten() {
                let f = e.path().join("claude_desktop_config.json");
                if f.exists() {
                    data.push(f);
                }
            }
        }
        if !data.is_empty() {
            return data;
        }
        let mut packaged = Vec::new();
        if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") {
                    let f = e
                        .path()
                        .join("LocalCache")
                        .join("Roaming")
                        .join("Claude")
                        .join("claude_desktop_config.json");
                    if f.exists() {
                        packaged.push(f);
                    }
                }
            }
        }
        if !packaged.is_empty() {
            return packaged;
        }
    }
    dirs::config_dir()
        .map(|cfg| vec![cfg.join("Claude").join("claude_desktop_config.json")])
        .unwrap_or_default()
}

/// Claude Desktop rewrites its config when it quits, which would drop an
/// entry added while it runs, so connecting requires it to be closed.
fn claude_desktop_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(out) = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq claude.exe", "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            return String::from_utf8_lossy(&out.stdout)
                .to_ascii_lowercase()
                .contains("\"claude.exe\"");
        }
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn read_config(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}); fix or remove it first",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

fn points_at(config: &Value, server: &Path) -> bool {
    config
        .get("mcpServers")
        .and_then(|s| s.get(SERVER_KEY))
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .map(|c| Path::new(c) == server)
        .unwrap_or(false)
}

#[tauri::command]
pub fn mcp_status() -> McpStatus {
    let server = server_path();
    let paths = config_paths();
    let connected = match &server {
        Some(s) => {
            !paths.is_empty()
                && paths
                    .iter()
                    .filter(|p| p.exists())
                    .all(|p| read_config(p).map(|c| points_at(&c, s)).unwrap_or(false))
                && paths.iter().any(|p| p.exists())
        }
        None => false,
    };
    McpStatus {
        claude_code_command: server
            .as_ref()
            .map(|s| format!("claude mcp add --scope user anyscp -- \"{}\"", s.display())),
        server_path: server.map(|p| p.display().to_string()),
        claude_running: claude_desktop_running(),
        config_paths: paths.iter().map(|p| p.display().to_string()).collect(),
        connected,
    }
}

/// Add (or update) the anySCP entry in Claude Desktop's config. Other
/// servers and settings are preserved, and the previous file is kept as
/// `claude_desktop_config.json.bak`. Claude Desktop must be restarted to
/// pick the change up.
#[tauri::command]
pub fn mcp_connect_claude_desktop() -> Result<McpStatus, String> {
    let server = server_path().ok_or_else(|| {
        "The anyscp-mcp server was not found next to the app. Reinstall anySCP with the installer \
         that includes it."
            .to_string()
    })?;
    if claude_desktop_running() {
        return Err("Quit Claude Desktop first (right-click its icon next to the clock, then Quit), \
                    then click Connect again. Claude rewrites its settings when it closes and would \
                    undo the change."
            .into());
    }
    let paths = config_paths();
    if paths.is_empty() {
        return Err("Could not locate the user config folder".into());
    }
    // Write every existing config; if none exists yet, create the default one.
    let targets: Vec<&PathBuf> = {
        let existing: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
        if existing.is_empty() {
            vec![&paths[0]]
        } else {
            existing
        }
    };
    for path in targets {
        let mut config = read_config(path)?;
        if !config.is_object() {
            return Err(format!("{} does not contain a JSON object", path.display()));
        }
        if path.exists() {
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
        }
        let root = config.as_object_mut().expect("checked above");
        let servers = root
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        if !servers.is_object() {
            *servers = Value::Object(Map::new());
        }
        servers.as_object_mut().expect("object").insert(
            SERVER_KEY.into(),
            json!({ "command": server.display().to_string(), "args": [] }),
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(path, text)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }
    Ok(mcp_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_matching_entry() {
        let cfg = json!({ "mcpServers": { "anyscp": { "command": "/opt/a/anyscp-mcp" } } });
        assert!(points_at(&cfg, Path::new("/opt/a/anyscp-mcp")));
        assert!(!points_at(&cfg, Path::new("/opt/b/anyscp-mcp")));
        assert!(!points_at(&json!({}), Path::new("/opt/a/anyscp-mcp")));
    }

    #[test]
    fn empty_or_missing_config_reads_as_object() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        assert_eq!(read_config(&missing).unwrap(), json!({}));
        let empty = dir.path().join("empty.json");
        std::fs::write(&empty, "  ").unwrap();
        assert_eq!(read_config(&empty).unwrap(), json!({}));
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{ nope").unwrap();
        assert!(read_config(&bad).is_err());
    }
}
