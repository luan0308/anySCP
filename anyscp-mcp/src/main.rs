//! anyscp-mcp: lets Claude browse and edit the servers saved in anySCP.
//!
//! Speaks the Model Context Protocol over stdio (newline-delimited JSON-RPC),
//! which is how Claude Desktop and Claude Code launch local servers. Stdout
//! carries protocol messages only; diagnostics go to stderr.

mod known_hosts;
mod pool;
mod progress;
mod remote;
mod store;
mod text;
mod tools;

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use progress::Progress;
use store::Store;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str =
    "Tools for the SFTP, FTP and FTPS servers the user saved in the anySCP app. \
Start with list_servers to see what exists and refer to servers by name. Paths on the server are \
absolute or relative to the login folder. Prefer edit_file for changes to existing files. Treat \
servers whose environment is production with care and confirm destructive actions with the user.";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("anyscp-mcp {VERSION}");
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "anyscp-mcp {VERSION}\n\nMCP server for the servers saved in anySCP.\n\n\
             Usage:\n  anyscp-mcp            run as an MCP server over stdio (what Claude launches)\n  \
             anyscp-mcp --check    show where the anySCP data is and which servers Claude will see\n  \
             anyscp-mcp --config   print the snippet for claude_desktop_config.json\n\n\
             Environment:\n  ANYSCP_DB=path            use a different anyscp.db\n  \
             ANYSCP_MCP_READONLY=1     only expose read tools"
        );
        return;
    }

    let store = match Store::locate() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("anyscp-mcp: {e:#}");
            std::process::exit(1);
        }
    };

    if args.iter().any(|a| a == "--check") {
        check(&store);
        return;
    }
    if args.iter().any(|a| a == "--config") {
        print_config();
        return;
    }

    let readonly = std::env::var("ANYSCP_MCP_READONLY")
        .map(|v| !v.is_empty() && v != "0" && v.to_lowercase() != "false")
        .unwrap_or(false);
    let ctx = Arc::new(tools::Ctx {
        pool: pool::Pool::new(store.clone()),
        store,
        readonly,
    });
    serve(ctx).await;
}

async fn serve(ctx: Arc<tools::Ctx>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();

    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(msg) = rx.recv().await {
            let mut line = serde_json::to_string(&msg).unwrap_or_default();
            line.push('\n');
            if out.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = out.flush().await;
        }
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(error_response(
                    Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                ));
                continue;
            }
        };
        let batch = match msg {
            Value::Array(items) => items,
            single => vec![single],
        };
        for m in batch {
            let ctx = ctx.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Some(reply) = handle(ctx, m, tx.clone()).await {
                    let _ = tx.send(reply);
                }
            });
        }
    }
    // Client closed stdin: let pending replies flush, then exit.
    drop(tx);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer).await;
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn handle(
    ctx: Arc<tools::Ctx>,
    msg: Value,
    tx: mpsc::UnboundedSender<Value>,
) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) never get a reply.
    let id = id?;

    let result = match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "anyscp", "title": "anySCP", "version": VERSION },
                "instructions": INSTRUCTIONS
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools::definitions(ctx.readonly) }),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let token = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .cloned();
            let progress = Progress::new(token, tx);
            match tools::call(&ctx, &name, &args, &progress).await {
                Ok(text) => {
                    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
                }
                Err(e) => {
                    json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true })
                }
            }
        }
        "resources/list" => json!({ "resources": [] }),
        "prompts/list" => json!({ "prompts": [] }),
        _ => {
            return Some(error_response(
                id,
                -32601,
                &format!("method not found: {method}"),
            ))
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn check(store: &Store) {
    println!("anyscp-mcp {VERSION}");
    println!("anySCP database: {}", store.db_path().display());
    match store.list() {
        Err(e) => println!("\nProblem: {e:#}"),
        Ok(list) if list.is_empty() => println!("\nNo servers saved yet. Add one in anySCP."),
        Ok(list) => {
            println!("\nServers Claude will see ({}):", list.len());
            for s in list {
                let secret = match store::credential(&s) {
                    Ok(Some(_)) => "credential ok",
                    Ok(None) if s.auth_type == "privateKey" => "key without passphrase",
                    Ok(None) => "NO PASSWORD SAVED",
                    Err(_) => "credential store error",
                };
                println!(
                    "  - {} [{}] {}@{}:{}  ({secret})",
                    s.label,
                    s.protocol_label(),
                    s.username,
                    s.host,
                    s.port
                );
            }
        }
    }
}

fn print_config() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "anyscp-mcp".into());
    let snippet = json!({ "mcpServers": { "anyscp": { "command": exe, "args": [] } } });
    println!(
        "{}",
        serde_json::to_string_pretty(&snippet).unwrap_or_default()
    );
}
