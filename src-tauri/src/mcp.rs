//! Hooking the bundled `talktoall-files-mcp` server up to Claude and Codex.
//!
//! The installer puts `talktoall-files-mcp` next to the app executable.
//! Claude Desktop finds local MCP servers through its
//! `claude_desktop_config.json` and Codex through `~/.codex/config.toml`, so
//! this module reports whether Talktoall Files is registered in each and adds
//! the entry on request, keeping every other server the user already has.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const SERVER_KEY: &str = "talktoall-files";
/// Key the app used while it was still anySCP. Removed when connecting, so
/// Claude does not load the same tools twice.
const LEGACY_SERVER_KEY: &str = "anyscp";
const BINARY: &str = "talktoall-files-mcp";

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
    /// Codex config file (`~/.codex/config.toml`, or under `CODEX_HOME`).
    pub codex_config_path: Option<String>,
    /// Codex's config points at `server_path`.
    pub codex_connected: bool,
    /// The Codex folder exists, so Codex is (or was) installed.
    pub codex_installed: bool,
}

fn server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        format!("{BINARY}.exe")
    } else {
        BINARY.to_string()
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
                "{} não é um JSON válido ({e}). Corrija ou apague o arquivo antes.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("não foi possível ler {}: {e}", path.display())),
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
            paths
                .iter()
                .filter(|p| p.exists())
                .all(|p| read_config(p).map(|c| points_at(&c, s)).unwrap_or(false))
                && paths.iter().any(|p| p.exists())
        }
        None => false,
    };
    let codex = codex_config_path();
    let codex_connected = match (&server, &codex) {
        (Some(s), Some(c)) => std::fs::read_to_string(c)
            .map(|text| codex_points_at(&text, s))
            .unwrap_or(false),
        _ => false,
    };
    McpStatus {
        claude_code_command: server.as_ref().map(|s| {
            format!(
                "claude mcp add --scope user {SERVER_KEY} -- \"{}\"",
                s.display()
            )
        }),
        server_path: server.map(|p| p.display().to_string()),
        claude_running: claude_desktop_running(),
        config_paths: paths.iter().map(|p| p.display().to_string()).collect(),
        connected,
        codex_installed: codex
            .as_ref()
            .and_then(|c| c.parent())
            .map(Path::is_dir)
            .unwrap_or(false),
        codex_config_path: codex.map(|p| p.display().to_string()),
        codex_connected,
    }
}

fn require_server() -> Result<PathBuf, String> {
    server_path().ok_or_else(|| {
        "O servidor MCP (talktoall-files-mcp) não foi encontrado ao lado do app. \
         Reinstale o Talktoall Files com o instalador oficial."
            .to_string()
    })
}

/// True when `entry` is an old anySCP registration (its command is the
/// anyscp-mcp binary), so it can be replaced by the new one.
fn is_legacy_entry(entry: &Value) -> bool {
    // Split on both separators: the config may hold a Windows path even when
    // this code runs elsewhere (tests), and Path only knows the native one.
    entry
        .get("command")
        .and_then(Value::as_str)
        .and_then(|c| c.rsplit(['/', '\\']).next())
        .map(|name| {
            let name = name.to_ascii_lowercase();
            name == "anyscp-mcp" || name == "anyscp-mcp.exe"
        })
        .unwrap_or(false)
}

/// Add (or update) the Talktoall Files entry in a Claude Desktop config
/// object, dropping the old anySCP entry.
fn register_in_claude_config(config: &mut Value, server: &Path) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "a configuração do Claude não contém um objeto JSON".to_string())?;
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    if !servers.is_object() {
        *servers = Value::Object(Map::new());
    }
    let servers = servers.as_object_mut().expect("object");
    if servers.get(LEGACY_SERVER_KEY).map(is_legacy_entry).unwrap_or(false) {
        servers.remove(LEGACY_SERVER_KEY);
    }
    servers.insert(
        SERVER_KEY.into(),
        json!({ "command": server.display().to_string(), "args": [] }),
    );
    Ok(())
}

/// Add (or update) the Talktoall Files entry in Claude Desktop's config.
/// Other servers and settings are preserved, and the previous file is kept as
/// `claude_desktop_config.json.bak`. Claude Desktop must be restarted to pick
/// the change up.
#[tauri::command]
pub fn mcp_connect_claude_desktop() -> Result<McpStatus, String> {
    let server = require_server()?;
    if claude_desktop_running() {
        return Err("Feche o Claude Desktop primeiro (clique com o botão direito no ícone dele perto \
                    do relógio e escolha Sair) e clique em Conectar de novo. O Claude regrava as \
                    configurações ao fechar e desfaria a mudança."
            .into());
    }
    let paths = config_paths();
    if paths.is_empty() {
        return Err("Não foi possível encontrar a pasta de configuração do usuário".into());
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
            return Err(format!("{} não contém um objeto JSON", path.display()));
        }
        if path.exists() {
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
        }
        register_in_claude_config(&mut config, &server)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("não foi possível criar {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(path, text)
            .map_err(|e| format!("não foi possível gravar {}: {e}", path.display()))?;
    }
    Ok(mcp_status())
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// `$CODEX_HOME/config.toml`, else `~/.codex/config.toml` (all platforms).
fn codex_config_path() -> Option<PathBuf> {
    let home = match std::env::var_os("CODEX_HOME") {
        Some(h) if !h.is_empty() => PathBuf::from(h),
        _ => dirs::home_dir()?.join(".codex"),
    };
    Some(home.join("config.toml"))
}

fn codex_points_at(text: &str, server: &Path) -> bool {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("mcp_servers")
        .and_then(|s| s.get(SERVER_KEY))
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .map(|c| Path::new(c) == server)
        .unwrap_or(false)
}

/// Return `text` with `[mcp_servers.talktoall-files]` pointing at `server`,
/// keeping everything else (comments included).
fn register_in_codex_config(text: &str, server: &Path) -> Result<String, String> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("a configuração do Codex não é um TOML válido ({e})"))?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            toml_edit::Item::Table(t)
        })
        .as_table_mut()
        .ok_or_else(|| "mcp_servers na configuração do Codex não é uma tabela".to_string())?;
    let mut entry = toml_edit::Table::new();
    entry.insert("command", toml_edit::value(server.display().to_string()));
    entry.insert("args", toml_edit::value(toml_edit::Array::new()));
    servers.insert(SERVER_KEY, toml_edit::Item::Table(entry));
    Ok(doc.to_string())
}

/// Add (or update) the Talktoall Files entry in Codex's config.toml. The
/// previous file is kept as `config.toml.bak`. Codex reads it on start.
#[tauri::command]
pub fn mcp_connect_codex() -> Result<McpStatus, String> {
    let server = require_server()?;
    let path = codex_config_path()
        .ok_or_else(|| "Não foi possível encontrar a pasta do usuário".to_string())?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("não foi possível ler {}: {e}", path.display())),
    };
    let updated = register_in_codex_config(&text, &server)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("não foi possível criar {}: {e}", dir.display()))?;
    }
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("toml.bak"));
    }
    std::fs::write(&path, updated)
        .map_err(|e| format!("não foi possível gravar {}: {e}", path.display()))?;
    Ok(mcp_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_matching_entry() {
        let cfg = json!({ "mcpServers": { "talktoall-files": { "command": "/opt/a/talktoall-files-mcp" } } });
        assert!(points_at(&cfg, Path::new("/opt/a/talktoall-files-mcp")));
        assert!(!points_at(&cfg, Path::new("/opt/b/talktoall-files-mcp")));
        assert!(!points_at(&json!({}), Path::new("/opt/a/talktoall-files-mcp")));
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

    #[test]
    fn claude_config_replaces_legacy_anyscp_entry_only() {
        let mut cfg = json!({
            "mcpServers": {
                "anyscp": { "command": "C:\\Program Files\\anySCP\\anyscp-mcp.exe" },
                "other": { "command": "node", "args": ["x.js"] }
            },
            "theme": "dark"
        });
        let server = Path::new("/opt/ttf/talktoall-files-mcp");
        register_in_claude_config(&mut cfg, server).unwrap();
        assert!(cfg["mcpServers"].get("anyscp").is_none());
        assert_eq!(cfg["mcpServers"]["other"]["command"], "node");
        assert_eq!(cfg["theme"], "dark");
        assert!(points_at(&cfg, server));

        // An unrelated server that happens to be called "anyscp" stays.
        let mut cfg = json!({ "mcpServers": { "anyscp": { "command": "python" } } });
        register_in_claude_config(&mut cfg, server).unwrap();
        assert!(cfg["mcpServers"].get("anyscp").is_some());
    }

    #[test]
    fn codex_config_keeps_existing_content() {
        let server = Path::new("/opt/ttf/talktoall-files-mcp");
        let before = "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\nargs = [\"docs\"]\n";
        let after = register_in_codex_config(before, server).unwrap();
        assert!(after.contains("# my settings"));
        assert!(after.contains("model = \"gpt-5\""));
        assert!(after.contains("[mcp_servers.docs]"));
        assert!(after.contains("[mcp_servers.talktoall-files]"));
        assert!(codex_points_at(&after, server));
        assert!(!codex_points_at(before, server));

        // Re-registering is idempotent and empty files work.
        let again = register_in_codex_config(&after, server).unwrap();
        assert_eq!(again.matches("[mcp_servers.talktoall-files]").count(), 1);
        let fresh = register_in_codex_config("", server).unwrap();
        assert!(codex_points_at(&fresh, server));
        assert!(!fresh.contains("[mcp_servers]\n"), "no empty parent table: {fresh}");
    }

    #[test]
    fn codex_config_rejects_invalid_toml() {
        assert!(register_in_codex_config("[[[", Path::new("/x")).is_err());
    }
}
