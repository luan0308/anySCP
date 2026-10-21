import { Cloud } from "lucide-react";
import { ExplorerView } from "./ExplorerView";
import { S3Browser } from "../s3/S3Browser";
import { useSftpStore } from "../../stores/sftp-store";
import { useS3Store } from "../../stores/s3-store";
import type { Transport } from "../../lib/explorer-transport";
import { useHostsStore } from "../../stores/hosts-store";
import { protocolLabel } from "../../types/ssh";
import { ProtocolBadge, EnvBadge } from "../brand";

interface ExplorerPageProps {
  /** SFTP/SCP transport session id (both live in the sftp store). */
  sftpSessionId?: string;
  /** Defaults to "sftp"; "scp" when the host fell back to SCP. */
  transport?: Transport;
  s3SessionId?: string;
  /** Whether this tab is the active/visible one. Explorer tabs stay mounted
   *  (issue #17), so document-level listeners must only fire for the active one. */
  isActive?: boolean;
}

export function ExplorerPage({ sftpSessionId, transport = "sftp", s3SessionId, isActive = true }: ExplorerPageProps) {
  const sftpSession = useSftpStore((s) => sftpSessionId ? s.sessions.get(sftpSessionId) : null);
  const s3Session = useS3Store((s) => s3SessionId ? s.sessions.get(s3SessionId) : null);

  const baseLabel = sftpSession?.label ?? s3Session?.label ?? "Arquivos";
  // The explorer session only knows the label; find the saved host by it for
  // the protocol / environment badges (best effort, labels are rarely shared).
  const host = useHostsStore((s) => (sftpSession ? s.hosts.find((h) => h.label === sftpSession.label) : undefined));
  const badge =
    !sftpSessionId ? "S3" : transport === "ftp" ? (host ? protocolLabel(host) : "FTP") : transport === "scp" ? "SCP" : "SFTP";
  const isSftp = !!sftpSessionId;

  return (
    <div className="flex flex-col h-full">
      {/* Title + badges, as in the talktoall layouts */}
      <div className="flex items-center gap-2.5 px-6 pt-5 pb-4 shrink-0 no-select min-w-0">
        {!isSftp && <Cloud size={20} strokeWidth={1.75} className="shrink-0 text-text-secondary" aria-hidden="true" />}
        <h1 className="text-[22px] font-bold text-text-primary truncate" title={baseLabel}>
          {baseLabel}
        </h1>
        <ProtocolBadge label={badge} />
        <EnvBadge env={host?.environment} />
      </div>

      {/* Browser content */}
      <div
        className="flex-1 min-h-0"
        data-explorer-transport={sftpSessionId ? transport : s3SessionId ? "s3" : undefined}
      >
        {sftpSessionId && <ExplorerView sessionId={sftpSessionId} transport={transport} isActive={isActive} />}
        {s3SessionId && <S3Browser sessionId={s3SessionId} isActive={isActive} />}
      </div>
    </div>
  );
}
