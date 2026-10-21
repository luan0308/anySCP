# anyscp-mcp

MCP server that lets Claude work with the servers saved in anySCP: SSH hosts
(over SFTP) and FTP/FTPS hosts. It reads the server list from anySCP's
database and the passwords from the system keychain, so Claude refers to
servers by name and never sees a credential.

## Connect

In anySCP, open **Settings › Claude** and click **Connect**. That adds this
entry to Claude Desktop's `claude_desktop_config.json` (restart Claude
Desktop afterwards):

```json
{ "mcpServers": { "anyscp": { "command": "C:\\Program Files\\anySCP\\anyscp-mcp.exe", "args": [] } } }
```

For Claude Code:

```
claude mcp add --scope user anyscp -- "C:\Program Files\anySCP\anyscp-mcp.exe"
```

`anyscp-mcp --check` lists the servers Claude will see and whether each has a
saved credential. `anyscp-mcp --config` prints the JSON snippet above with the
real path.

## Tools

| Tool | What it does |
|---|---|
| `list_servers` | Saved servers with protocol, user, group, environment and notes |
| `list_directory` | Folder listing (relative paths start at the login folder) |
| `get_info` | Type, size, date and permissions of a path |
| `read_file` | Text files; detects utf-8 or windows-1252, refuses binaries |
| `edit_file` | Replace an exact snippet, keeping encoding and line endings |
| `write_file` | Create or fully replace a text file (`overwrite: true` required) |
| `upload` / `download` | Files or whole folders; existing files are skipped unless `overwrite: true` |
| `create_directory` | `mkdir -p` |
| `rename` | Rename or move; refuses to overwrite |
| `delete` | Needs `recursive: true` for non-empty folders; never deletes `/` or the login folder |

## Safety

- Passwords come from the OS keychain; tool output never includes them.
- SSH host keys are trusted on first use and pinned in `mcp_known_hosts.json`
  next to anySCP's database; a changed key refuses the connection.
- `ANYSCP_MCP_READONLY=1` exposes only the read tools.
- `ANYSCP_DB=path` points at a different `anyscp.db`.

## Development

FTP goes through `crates/anyscp-ftp`, shared with the app. Unit tests:
`cargo test`. The end-to-end suite used during development ran this binary
against local SFTP, FTP and FTPS servers through a minimal MCP client.
