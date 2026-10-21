//! The MCP tools Claude sees, and what each one does.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::pool::Pool;
use crate::progress::{human_bytes, Progress};
use crate::remote::{self, Entry, Kind, Remote, Totals};
use crate::store::{Server, Store};
use crate::text::{self, Encoding};

const DEFAULT_READ: u64 = 256 * 1024;
const MAX_READ: u64 = 4 * 1024 * 1024;
const MAX_EDIT: u64 = 8 * 1024 * 1024;
const MAX_LISTED: usize = 500;

pub struct Ctx {
    pub store: Arc<Store>,
    pub pool: Arc<Pool>,
    pub readonly: bool,
}

// ---------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------

fn tool(
    name: &str,
    title: &str,
    description: &str,
    props: Value,
    required: &[&str],
    ann: Value,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false
        },
        "annotations": ann
    })
}

fn server_prop() -> Value {
    json!({ "type": "string", "description": "Server name as saved in Talktoall Files (or its id). See list_servers." })
}

const WRITES: &[&str] = &[
    "edit_file",
    "write_file",
    "upload",
    "create_directory",
    "rename",
    "delete",
];

pub fn definitions(readonly: bool) -> Vec<Value> {
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": true });
    let all = vec![
        tool(
            "list_servers",
            "List saved servers",
            "List the servers saved in the Talktoall Files app on this computer (SFTP, FTP and FTPS). \
             Use a server's name or id with the other tools. Passwords are never exposed. The \
             environment field (for example production) tells you how careful to be.",
            json!({}),
            &[],
            json!({ "readOnlyHint": true, "openWorldHint": false }),
        ),
        tool(
            "list_directory",
            "List folder",
            "List a folder on a saved server, folders first. Relative paths start at the folder \
             the server logs into, which is shown in the output.",
            json!({
                "server": server_prop(),
                "path": { "type": "string", "description": "Folder to list. Empty for the login folder." }
            }),
            &["server"],
            read_only.clone(),
        ),
        tool(
            "get_info",
            "File info",
            "Check whether a path exists on the server and get its type, size, modification date \
             and permissions.",
            json!({ "server": server_prop(), "path": { "type": "string" } }),
            &["server", "path"],
            read_only.clone(),
        ),
        tool(
            "read_file",
            "Read text file",
            "Read a text file from the server (HTML, CSS, JS, PHP, .htaccess, configs). Detects \
             the encoding (utf-8, or windows-1252/latin-1 which is common on older sites). Binary \
             files are refused: use download for those. Long files are cut at max_bytes.",
            json!({
                "server": server_prop(),
                "path": { "type": "string" },
                "max_bytes": { "type": "integer", "description": "Default 262144 (256 KB), maximum 4194304." }
            }),
            &["server", "path"],
            read_only.clone(),
        ),
        tool(
            "edit_file",
            "Edit text file",
            "Change part of a text file on the server by replacing an exact snippet. Prefer this \
             over write_file for changes to existing files: only the snippet changes, and the file \
             keeps its encoding and line endings. old_text must appear exactly once unless \
             replace_all is true, so read the file first and copy old_text exactly, with enough \
             surrounding text to be unique.",
            json!({
                "server": server_prop(),
                "path": { "type": "string" },
                "old_text": { "type": "string", "description": "Exact text to find." },
                "new_text": { "type": "string", "description": "Replacement text." },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence. Default false." },
                "backup": { "type": "boolean", "description": "Keep a copy of the original as <name>.bak-<date> next to it. Default false." }
            }),
            &["server", "path", "old_text", "new_text"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true }),
        ),
        tool(
            "write_file",
            "Write text file",
            "Create a text file on the server, or replace one entirely (requires overwrite: true). \
             Missing parent folders are created. When replacing, the existing file's encoding is \
             kept unless encoding is given. For small changes use edit_file instead.",
            json!({
                "server": server_prop(),
                "path": { "type": "string" },
                "content": { "type": "string" },
                "overwrite": { "type": "boolean", "description": "Allow replacing an existing file. Default false." },
                "encoding": { "type": "string", "enum": ["utf-8", "utf-8-bom", "windows-1252"], "description": "Default: keep the existing file's encoding, utf-8 for new files." },
                "backup": { "type": "boolean", "description": "When replacing, keep a copy of the original as <name>.bak-<date>. Default false." }
            }),
            &["server", "path", "content"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": true }),
        ),
        tool(
            "upload",
            "Upload",
            "Send a file or folder from this computer to the server. local_path is an absolute path \
             on this computer. For a file, remote_path is the destination file, or an existing \
             folder to put it in. For a folder, its contents are copied into remote_path (created \
             if missing) keeping the structure, like a site deploy. Existing remote files are \
             skipped unless overwrite is true.",
            json!({
                "server": server_prop(),
                "local_path": { "type": "string" },
                "remote_path": { "type": "string" },
                "overwrite": { "type": "boolean", "description": "Replace files that already exist on the server. Default false." }
            }),
            &["server", "local_path", "remote_path"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": true }),
        ),
        tool(
            "download",
            "Download",
            "Bring a file or folder from the server to this computer. local_path is an absolute \
             path on this computer. For a file, local_path is the destination file, or an existing \
             folder to put it in. For a folder, its contents are copied into local_path (created \
             if missing). Existing local files are skipped unless overwrite is true.",
            json!({
                "server": server_prop(),
                "remote_path": { "type": "string" },
                "local_path": { "type": "string" },
                "overwrite": { "type": "boolean", "description": "Replace local files that already exist. Default false." }
            }),
            &["server", "remote_path", "local_path"],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true }),
        ),
        tool(
            "create_directory",
            "Create folder",
            "Create a folder on the server, including any missing parent folders.",
            json!({ "server": server_prop(), "path": { "type": "string" } }),
            &["server", "path"],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true }),
        ),
        tool(
            "rename",
            "Rename or move",
            "Rename or move a file or folder on the server. Fails if the destination exists.",
            json!({ "server": server_prop(), "from": { "type": "string" }, "to": { "type": "string" } }),
            &["server", "from", "to"],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }),
        ),
        tool(
            "delete",
            "Delete",
            "Delete a file or folder on the server. This cannot be undone. Folders that are not \
             empty need recursive: true. Confirm with the user before deleting anything they did \
             not explicitly ask to delete, especially on servers marked production.",
            json!({
                "server": server_prop(),
                "path": { "type": "string" },
                "recursive": { "type": "boolean", "description": "Required to delete a folder that has contents. Default false." }
            }),
            &["server", "path"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": true }),
        ),
    ];
    all.into_iter()
        .filter(|t| !(readonly && WRITES.contains(&t["name"].as_str().unwrap_or(""))))
        .collect()
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn s<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required argument \"{key}\""))
}

fn opt_s<'a>(a: &'a Value, key: &str) -> Option<&'a str> {
    a.get(key).and_then(Value::as_str)
}

fn flag(a: &Value, key: &str) -> bool {
    a.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn local_path(p: &str) -> Result<PathBuf> {
    let p = p.trim();
    let home = dirs::home_dir();
    let path = match (p, &home) {
        ("~", Some(h)) => h.clone(),
        (p, Some(h)) if p.starts_with("~/") || p.starts_with("~\\") => h.join(&p[2..]),
        (p, _) => PathBuf::from(p),
    };
    if !path.is_absolute() {
        bail!(
            "local_path must be an absolute path on this computer (for example C:\\Users\\you\\Downloads\\site), got \"{p}\""
        );
    }
    Ok(path)
}

fn fmt_time(t: Option<SystemTime>) -> String {
    match t {
        Some(t) => {
            let s = humantime::format_rfc3339_seconds(t).to_string();
            format!("{} {} UTC", &s[..10], &s[11..16])
        }
        None => "-".into(),
    }
}

fn backup_name(path: &str) -> String {
    let digits: String = humantime::format_rfc3339_seconds(SystemTime::now())
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    format!("{path}.bak-{}-{}", &digits[..8], &digits[8..14])
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Dir => "folder",
        Kind::File => "file",
        Kind::Symlink => "symlink",
        Kind::Other => "other",
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub async fn call(ctx: &Ctx, name: &str, a: &Value, progress: &Progress) -> Result<String> {
    if name == "list_servers" {
        return list_servers(ctx);
    }
    if !definitions(ctx.readonly).iter().any(|t| t["name"] == name) {
        if ctx.readonly && WRITES.contains(&name) {
            bail!("anyscp-mcp is running in read-only mode (ANYSCP_MCP_READONLY), so {name} is disabled");
        }
        bail!("unknown tool {name}");
    }
    let server = {
        let store = ctx.store.clone();
        let q = s(a, "server")?.to_string();
        tokio::task::spawn_blocking(move || store.find(&q)).await??
    };
    let mut slot = ctx.pool.acquire(&server).await?;
    let r = slot.remote();
    let result = match name {
        "list_directory" => list_directory(r, &server, a).await,
        "get_info" => get_info(r, a).await,
        "read_file" => read_file(r, a).await,
        "edit_file" => edit_file(r, a).await,
        "write_file" => write_file(r, a).await,
        "upload" => upload(r, a, progress).await,
        "download" => download(r, a, progress).await,
        "create_directory" => create_directory(r, a).await,
        "rename" => rename(r, a).await,
        "delete" => delete(r, a).await,
        other => Err(anyhow!("unknown tool {other}")),
    };
    if result.is_err() {
        slot.check_after_error().await;
    }
    result
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

fn list_servers(ctx: &Ctx) -> Result<String> {
    let servers = ctx.store.list()?;
    if servers.is_empty() {
        return Ok("No servers are saved in Talktoall Files yet. Add one in the app first.".into());
    }
    let mut out = format!("{} servers saved in Talktoall Files:\n", servers.len());
    for sv in &servers {
        out.push_str(&format!(
            "\n- {} | {} | {}@{}:{}",
            sv.label,
            sv.protocol_label(),
            sv.username,
            sv.host,
            sv.port
        ));
        if let Some(g) = &sv.group {
            out.push_str(&format!(" | group: {g}"));
        }
        if let Some(e) = sv.environment.as_deref().filter(|e| !e.is_empty()) {
            out.push_str(&format!(" | environment: {e}"));
        }
        if let Some(p) = &sv.initial_path {
            out.push_str(&format!(" | starts in: {p}"));
        }
        out.push_str(&format!(" | id: {}", sv.id));
        if let Some(n) = sv.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            let short: String = n.chars().take(200).collect();
            out.push_str(&format!("\n  notes: {}", short.replace('\n', " ")));
        }
    }
    Ok(out)
}

async fn list_directory(r: &mut dyn Remote, server: &Server, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), opt_s(a, "path").unwrap_or(""));
    let entries = r.list(&path).await?;
    let mut out = format!(
        "{} ({}) {}  [login folder: {}]\n{} items\n",
        server.label,
        server.protocol_label(),
        path,
        r.home(),
        entries.len()
    );
    for e in entries.iter().take(MAX_LISTED) {
        let size = if e.is_dir() {
            "-".to_string()
        } else {
            human_bytes(e.size)
        };
        let name = if e.is_dir() {
            format!("{}/", e.name)
        } else {
            e.name.clone()
        };
        let perms = e.permissions.clone().unwrap_or_default();
        out.push_str(&format!(
            "\n{:<7} {:>9}  {}  {}  {}",
            kind_word(e.kind),
            size,
            fmt_time(e.modified),
            perms,
            name
        ));
    }
    if entries.len() > MAX_LISTED {
        out.push_str(&format!(
            "\n\n… and {} more not shown.",
            entries.len() - MAX_LISTED
        ));
    }
    Ok(out)
}

async fn get_info(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    match r.stat(&path).await? {
        None => Ok(format!("{path} does not exist.")),
        Some(e) => Ok(format!(
            "{}\ntype: {}\nsize: {}\nmodified: {}\npermissions: {}",
            e.path,
            kind_word(e.kind),
            if e.is_dir() {
                "-".into()
            } else {
                format!("{} ({} bytes)", human_bytes(e.size), e.size)
            },
            fmt_time(e.modified),
            e.permissions.unwrap_or_else(|| "-".into())
        )),
    }
}

async fn existing_file(r: &mut dyn Remote, path: &str) -> Result<Entry> {
    match r.stat(path).await? {
        None => bail!("{path} does not exist"),
        Some(e) if e.is_dir() => bail!("{path} is a folder, not a file"),
        Some(e) => Ok(e),
    }
}

async fn read_file(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    let limit = a
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_READ)
        .clamp(1, MAX_READ);
    let entry = existing_file(r, &path).await?;
    let (bytes, truncated) = r.read(&path, limit).await?;
    if text::looks_binary(&bytes) {
        bail!(
            "{path} looks like a binary file ({}). Use download to save it to this computer instead.",
            human_bytes(entry.size)
        );
    }
    let (content, enc) = text::decode(&bytes);
    let mut header = format!("{path}  ({}, {})", human_bytes(entry.size), enc.name());
    if truncated {
        header.push_str(&format!(
            "\n[showing the first {} only; raise max_bytes (up to 4 MB) or download the file to see the rest]",
            human_bytes(limit)
        ));
    }
    Ok(format!("{header}\n\n{content}"))
}

async fn read_whole_text(r: &mut dyn Remote, path: &str) -> Result<(Vec<u8>, String, Encoding)> {
    let (bytes, truncated) = r.read(path, MAX_EDIT).await?;
    if truncated {
        bail!(
            "{path} is larger than {}; too big to edit as text",
            human_bytes(MAX_EDIT)
        );
    }
    if text::looks_binary(&bytes) {
        bail!("{path} is a binary file and cannot be edited as text");
    }
    let (content, enc) = text::decode(&bytes);
    Ok((bytes, content, enc))
}

async fn edit_file(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    let old = s(a, "old_text")?;
    let new = s(a, "new_text")?;
    if old.is_empty() {
        bail!("old_text cannot be empty");
    }
    existing_file(r, &path).await?;
    let (original, content, enc) = read_whole_text(r, &path).await?;

    // Files saved on Windows use CRLF; match the snippet the same way.
    let (old_m, new_m) = if !content.contains(old) && content.contains("\r\n") && old.contains('\n')
    {
        (
            old.replace("\r\n", "\n").replace('\n', "\r\n"),
            new.replace("\r\n", "\n").replace('\n', "\r\n"),
        )
    } else {
        (old.to_string(), new.to_string())
    };
    let count = content.matches(old_m.as_str()).count();
    if count == 0 {
        bail!(
            "old_text was not found in {path}. Read the file again and copy the snippet exactly, \
             including spaces and indentation."
        );
    }
    if count > 1 && !flag(a, "replace_all") {
        bail!(
            "old_text appears {count} times in {path}. Include more surrounding text so it is \
             unique, or set replace_all: true."
        );
    }
    let updated = content.replace(old_m.as_str(), new_m.as_str());
    let bytes = text::encode(&updated, enc)?;
    let mut note = String::new();
    if flag(a, "backup") {
        let b = backup_name(&path);
        r.write(&b, &original).await?;
        note = format!("\nOriginal saved as {b}");
    }
    r.write(&path, &bytes).await?;
    Ok(format!(
        "Replaced {count} occurrence{} in {path} (encoding kept: {}).{note}",
        if count == 1 { "" } else { "s" },
        enc.name()
    ))
}

async fn write_file(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    let content = s(a, "content")?;
    let requested = match opt_s(a, "encoding") {
        Some(e) => Some(Encoding::parse(e).ok_or_else(|| anyhow!("unknown encoding {e}"))?),
        None => None,
    };
    let mut note = String::new();
    let enc = match r.stat(&path).await? {
        Some(e) if e.is_dir() => bail!("{path} is a folder"),
        Some(_) => {
            if !flag(a, "overwrite") {
                bail!("{path} already exists. Set overwrite: true to replace it, or use edit_file for a partial change.");
            }
            let (original, _, existing_enc) = match read_whole_text(r, &path).await {
                Ok(v) => v,
                Err(_) => (
                    r.read(&path, MAX_EDIT).await?.0,
                    String::new(),
                    Encoding::Utf8,
                ),
            };
            if flag(a, "backup") {
                let b = backup_name(&path);
                r.write(&b, &original).await?;
                note = format!("\nOriginal saved as {b}");
            }
            requested.unwrap_or(existing_enc)
        }
        None => {
            let (parent, _) = remote::parent_and_name(&path);
            remote::mkdir_all(r, &parent).await?;
            requested.unwrap_or(Encoding::Utf8)
        }
    };
    let bytes = text::encode(content, enc)?;
    r.write(&path, &bytes).await?;
    Ok(format!(
        "Wrote {} to {path} ({}).{note}",
        human_bytes(bytes.len() as u64),
        enc.name()
    ))
}

fn summary(verb: &str, t: &Totals, skipped: &[String]) -> String {
    let mut out = format!(
        "{verb} {} file{} ({}){}.",
        t.files,
        if t.files == 1 { "" } else { "s" },
        human_bytes(t.bytes),
        if t.folders > 0 {
            format!(
                ", {} folder{}",
                t.folders,
                if t.folders == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        }
    );
    if !skipped.is_empty() {
        out.push_str(&format!(
            "\nSkipped {} that already existed (set overwrite: true to replace):",
            skipped.len()
        ));
        for s in skipped.iter().take(20) {
            out.push_str(&format!("\n  {s}"));
        }
        if skipped.len() > 20 {
            out.push_str(&format!("\n  … and {} more", skipped.len() - 20));
        }
    }
    out
}

async fn upload(r: &mut dyn Remote, a: &Value, progress: &Progress) -> Result<String> {
    let local = local_path(s(a, "local_path")?)?;
    let remote_path = remote::resolve(r.home(), s(a, "remote_path")?);
    let overwrite = flag(a, "overwrite");
    let meta = tokio::fs::metadata(&local)
        .await
        .map_err(|e| anyhow!("{}: {e}", local.display()))?;
    let mut totals = Totals::default();
    let mut skipped = Vec::new();

    if meta.is_file() {
        let dest = match r.stat(&remote_path).await? {
            Some(e) if e.is_dir() => {
                let name = local
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                remote::join(&remote_path, &name)
            }
            _ => remote_path.clone(),
        };
        if let Some(e) = r.stat(&dest).await? {
            if e.is_dir() {
                bail!("{dest} is a folder");
            }
            if !overwrite {
                bail!("{dest} already exists on the server. Set overwrite: true to replace it.");
            }
        } else {
            let (parent, _) = remote::parent_and_name(&dest);
            remote::mkdir_all(r, &parent).await?;
        }
        progress
            .note(&format!("uploading {}", local.display()))
            .await;
        totals.bytes = r.upload_file(&local, &dest, progress).await?;
        totals.files = 1;
        return Ok(format!(
            "{}\n{} -> {dest}",
            summary("Uploaded", &totals, &skipped),
            local.display()
        ));
    }

    if !overwrite {
        // Merge without replacing: walk the tree and skip what exists.
        upload_merge(r, &local, &remote_path, progress, &mut totals, &mut skipped).await?;
    } else {
        remote::upload_tree(r, &local, &remote_path, progress, &mut totals).await?;
    }
    Ok(format!(
        "{}\n{} -> {remote_path}",
        summary("Uploaded", &totals, &skipped),
        local.display()
    ))
}

async fn upload_merge(
    r: &mut dyn Remote,
    local: &Path,
    remote_dir: &str,
    progress: &Progress,
    totals: &mut Totals,
    skipped: &mut Vec<String>,
) -> Result<()> {
    remote::mkdir_all(r, remote_dir).await?;
    let existing: std::collections::HashMap<String, Entry> = r
        .list(remote_dir)
        .await?
        .into_iter()
        .map(|e| (e.name.clone(), e))
        .collect();
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
        let target = remote::join(remote_dir, &name);
        let is_dir = tokio::fs::metadata(&child).await?.is_dir();
        match (is_dir, existing.get(&name)) {
            (true, Some(e)) if !e.is_dir() => {
                skipped.push(format!("{target} (a file is in the way)"))
            }
            (true, _) => {
                totals.folders += 1;
                Box::pin(upload_merge(r, &child, &target, progress, totals, skipped)).await?;
            }
            (false, Some(_)) => skipped.push(target),
            (false, None) => {
                progress
                    .note(&format!("uploading {}", child.display()))
                    .await;
                totals.bytes += r.upload_file(&child, &target, progress).await?;
                totals.files += 1;
            }
        }
    }
    Ok(())
}

async fn download(r: &mut dyn Remote, a: &Value, progress: &Progress) -> Result<String> {
    let remote_path = remote::resolve(r.home(), s(a, "remote_path")?);
    let local = local_path(s(a, "local_path")?)?;
    let overwrite = flag(a, "overwrite");
    let entry = r
        .stat(&remote_path)
        .await?
        .ok_or_else(|| anyhow!("{remote_path} does not exist on the server"))?;
    let mut totals = Totals::default();
    let mut skipped = Vec::new();

    if !entry.is_dir() {
        let dest = if local.is_dir() {
            local.join(&entry.name)
        } else {
            local.clone()
        };
        if dest.exists() && !overwrite {
            bail!(
                "{} already exists on this computer. Set overwrite: true to replace it.",
                dest.display()
            );
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        progress.note(&format!("downloading {remote_path}")).await;
        totals.bytes = r.download_file(&remote_path, &dest, progress).await?;
        totals.files = 1;
        return Ok(format!(
            "{}\n{remote_path} -> {}",
            summary("Downloaded", &totals, &skipped),
            dest.display()
        ));
    }

    if overwrite {
        remote::download_tree(r, &entry, &local, progress, &mut totals).await?;
    } else {
        download_merge(r, &entry, &local, progress, &mut totals, &mut skipped).await?;
    }
    Ok(format!(
        "{}\n{remote_path} -> {}",
        summary("Downloaded", &totals, &skipped),
        local.display()
    ))
}

async fn download_merge(
    r: &mut dyn Remote,
    dir: &Entry,
    local: &Path,
    progress: &Progress,
    totals: &mut Totals,
    skipped: &mut Vec<String>,
) -> Result<()> {
    tokio::fs::create_dir_all(local).await?;
    for child in r.list(&dir.path).await? {
        let target = local.join(&child.name);
        match child.kind {
            Kind::Dir => {
                totals.folders += 1;
                Box::pin(download_merge(
                    r, &child, &target, progress, totals, skipped,
                ))
                .await?;
            }
            Kind::File => {
                if target.exists() {
                    skipped.push(target.display().to_string());
                } else {
                    progress.note(&format!("downloading {}", child.path)).await;
                    totals.bytes += r.download_file(&child.path, &target, progress).await?;
                    totals.files += 1;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn create_directory(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    if let Some(e) = r.stat(&path).await? {
        if e.is_dir() {
            return Ok(format!("{path} already exists."));
        }
        bail!("{path} exists and is a file");
    }
    remote::mkdir_all(r, &path).await?;
    Ok(format!("Created {path}"))
}

async fn rename(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let from = remote::resolve(r.home(), s(a, "from")?);
    let to = remote::resolve(r.home(), s(a, "to")?);
    if r.stat(&from).await?.is_none() {
        bail!("{from} does not exist");
    }
    if r.stat(&to).await?.is_some() {
        bail!("{to} already exists. Delete or rename it first.");
    }
    let (parent, _) = remote::parent_and_name(&to);
    remote::mkdir_all(r, &parent).await?;
    r.rename(&from, &to).await?;
    Ok(format!("Renamed {from} -> {to}"))
}

async fn delete(r: &mut dyn Remote, a: &Value) -> Result<String> {
    let path = remote::resolve(r.home(), s(a, "path")?);
    if path == "/" || path == r.home() {
        bail!(
            "Refusing to delete {path}: it is the server root or the login folder. Delete its \
             contents item by item, or do it in Talktoall Files if you really mean it."
        );
    }
    let entry = r
        .stat(&path)
        .await?
        .ok_or_else(|| anyhow!("{path} does not exist"))?;
    let mut totals = Totals::default();
    if entry.is_dir() {
        let children = r.list(&path).await?;
        if !children.is_empty() && !flag(a, "recursive") {
            bail!(
                "{path} is a folder with {} items. Set recursive: true to delete it with everything inside.",
                children.len()
            );
        }
    }
    remote::delete_tree(r, &entry, &mut totals).await?;
    Ok(if entry.is_dir() {
        format!(
            "Deleted {path} ({} files, {} folders).",
            totals.files, totals.folders
        )
    } else {
        format!("Deleted {path}.")
    })
}
