// Opening an FTP/FTPS host. Unlike SSH hosts there is no connection to set up
// first: `ftp_open` logs in with the saved host's settings and password and
// returns an explorer session id, which then behaves like an SFTP/SCP one
// (same store, same Explorer view, "ftp" transport).

import type { SavedHost } from "../types";
import { useSftpStore } from "../stores/sftp-store";
import { useTabStore } from "../stores/tab-store";
import { useHostsStore } from "../stores/hosts-store";

export interface FtpOpenHandle {
  /** Resolves with the explorer session id, or null when cancelled. */
  done: Promise<string | null>;
  cancel: () => void;
}

export function openFtpExplorer(host: SavedHost): FtpOpenHandle {
  let cancelled = false;
  const label = host.label || `${host.username}@${host.host}`;
  const done = (async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const ftpSessionId = await invoke<string>("ftp_open", { hostId: host.id });
    if (cancelled) {
      void invoke("ftp_close", { ftpSessionId });
      return null;
    }
    useSftpStore
      .getState()
      .openSession(ftpSessionId, "", label, host.username, false, host.start_directory ?? undefined);
    useTabStore.getState().addTab({ type: "sftp", id: ftpSessionId, label, transport: "ftp" });
    void useHostsStore.getState().recordConnection(host.id);
    return ftpSessionId;
  })();
  return {
    done,
    cancel: () => {
      cancelled = true;
    },
  };
}
