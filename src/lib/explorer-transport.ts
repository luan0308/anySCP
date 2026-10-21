// Explorer transport dispatch.
//
// SFTP, SCP and FTP expose an identical command surface — same operation
// names, same argument shapes, same return types — differing only in the
// command prefix (`sftp_` / `scp_` / `ftp_`) and the session-id argument key
// (`sftpSessionId` / `scpSessionId` / `ftpSessionId`). This module centralises
// that difference so the Explorer UI can stay transport-agnostic.
//
// SCP is used transparently as a fallback when a host has the SFTP subsystem
// disabled; the user never picks it explicitly (see exploreHost). FTP is used
// for hosts saved with the FTP protocol.

export type Transport = "sftp" | "scp" | "ftp";

/** The Tauri event channel that carries transfer progress for a transport. */
export function transferEventName(transport: Transport): string {
  return `${transport}:transfer`;
}

/** The session-id argument key a transport's commands expect. */
function sessionKey(transport: Transport): "sftpSessionId" | "scpSessionId" | "ftpSessionId" {
  if (transport === "scp") return "scpSessionId";
  if (transport === "ftp") return "ftpSessionId";
  return "sftpSessionId";
}

/** Close a transport session with the command that matches it. */
export async function closeExplorerSession(transport: Transport, sessionId: string): Promise<void> {
  await explorerInvoke(transport, "close", sessionId);
}

/**
 * Invoke a transport command. `op` is the bare operation (e.g. "list_dir");
 * the prefix and session-id key are derived from `transport`. `extra` holds
 * the operation-specific arguments (path, oldPath, sourcePaths, …).
 */
export async function explorerInvoke<T>(
  transport: Transport,
  op: string,
  sessionId: string,
  extra: Record<string, unknown> = {},
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(`${transport}_${op}`, {
    [sessionKey(transport)]: sessionId,
    ...extra,
  });
}
