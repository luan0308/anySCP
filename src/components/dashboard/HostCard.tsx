import { useState } from "react";
import { Activity, Pencil, TerminalSquare, Copy, Trash2, FolderOpen, Waypoints } from "lucide-react";
import type { SavedHost } from "../../types";
import { isFtpHost, protocolLabel } from "../../types";
import { CardActionButton, CardActionStrip } from "./CardActionButton";
import { relativeTime } from "../../utils/time";
import { ContextMenu } from "../shared/ContextMenu";
import { ConfirmDangerDialog } from "../shared/ConfirmDangerDialog";
import { useHealthStore, IDLE_HEALTH, type HealthStatus } from "../../stores/health-store";
import { useHostsStore } from "../../stores/hosts-store";
import { ProtocolBadge, EnvBadge } from "../brand";

// Single source of truth for status → colour, shared by the button and the label.
function statusColor(status: HealthStatus): string {
  if (status === "reachable") return "text-status-connected";
  if (status === "checking") return "text-status-connecting";
  if (status === "idle") return "text-text-muted";
  return "text-status-error";
}

interface HostCardProps {
  host: SavedHost;
  onConnect: (host: SavedHost) => void;
  onExplore: (host: SavedHost) => void;
  onEdit: (hostId: string) => void;
  onDelete: (hostId: string) => void;
  onDuplicate: (host: SavedHost) => void;
}

// talktoall palette: green, violet, amber, coral, plus two softer tints.
export const HOST_COLORS = [
  "#72FAA6",
  "#A98BFF",
  "#FFB648",
  "#FF7A7A",
  "#7CC8FF",
  "#F28BD8",
];

export function getHostColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) | 0;
  }
  return HOST_COLORS[Math.abs(hash) % HOST_COLORS.length];
}

// ─── Component ────────────────────────────────────────────────────────────────

export function HostCard({ host, onConnect, onExplore, onEdit, onDelete, onDuplicate }: HostCardProps) {
  const displayName = host.label || host.host;
  const avatarColor = host.color || getHostColor(host.host);
  const initial = displayName.charAt(0).toUpperCase();

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  // Health lives in a store (keyed by host id), not local state, so a status
  // survives the dashboard unmounting when a terminal/other tab becomes active.
  const health = useHealthStore((s) => s.byHostId[host.id] ?? IDLE_HEALTH);
  const checkHealth = useHealthStore((s) => s.checkHealth);

  // Resolve the ProxyJump / tunnel host (if any) for the "via …" badge.
  const jumpHost = useHostsStore((s) =>
    host.proxy_jump_host_id
      ? s.hosts.find((h) => h.id === host.proxy_jump_host_id) ?? null
      : null,
  );
  const jumpLabel = jumpHost ? jumpHost.label || jumpHost.host : null;

  // Subtitle: who and where, e.g. "deploy · jardimdacolina.com.br · :22".
  const ftp = isFtpHost(host);
  const proto = protocolLabel(host);
  const defaultPort = ftp ? (host.ftp_security === "implicit" ? 990 : 21) : 22;
  const subtitleParts: string[] = [host.username, host.host];
  if (host.port && host.port !== defaultPort) subtitleParts.push(`:${host.port}`);
  const lastSeen = host.last_connected_at ? relativeTime(host.last_connected_at) : null;
  if (lastSeen) subtitleParts.push(lastSeen);
  const subtitle = subtitleParts.filter(Boolean).join(" · ");

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const contextItems = [
    {
      label: "Testar conexão",
      icon: Activity,
      onClick: () => void checkHealth(host.id),
    },
    ...(ftp
      ? []
      : [
          {
            label: "Terminal",
            icon: TerminalSquare,
            onClick: () => onConnect(host),
          },
        ]),
    {
      label: "Arquivos",
      icon: FolderOpen,
      onClick: () => onExplore(host),
    },
    {
      label: "Editar",
      icon: Pencil,
      onClick: () => onEdit(host.id),
    },
    {
      label: "Duplicar",
      icon: Copy,
      onClick: () => onDuplicate(host),
    },
    {
      label: "Excluir",
      icon: Trash2,
      danger: true,
      onClick: () => setConfirmDelete(true),
    },
  ];

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onConnect(host);
    }
  };

  const healthLabel = (() => {
    if (health.status === "idle") return null;
    if (health.status === "checking") return "Testando…";
    const latency = health.latencyMs !== null ? ` · ${health.latencyMs} ms` : "";
    if (health.status === "reachable") return `${proto} respondendo${latency}`;
    if (health.status === "dnsFailed") return "Endereço não encontrado (DNS)";
    if (health.status === "portClosed") return "Porta inacessível";
    if (health.status === "sshFailed") return `${proto} falhou`;
    return "Sem resposta";
  })();

  return (
    <>
      <div
        data-testid={`host-card-${host.id}`}
        data-host-id={host.id}
        data-host-label={displayName}
        role="button"
        tabIndex={0}
        onClick={() => onConnect(host)}
        onKeyDown={handleKeyDown}
        onContextMenu={handleContextMenu}
        title={ftp ? `Abrir os arquivos de ${displayName}` : `Conectar em ${displayName}`}
        className={[
          // grab/grabbing communicates the card is draggable; the click-to-connect
          // action still fires for a plain click (drag needs a 5px move first).
          "group relative isolate flex flex-col gap-3 p-5 rounded-[17px] text-left w-full h-full cursor-grab active:cursor-grabbing overflow-hidden",
          "bg-white/[0.07] border border-transparent",
          "hover:border-accent/60 hover:bg-white/[0.09]",
          "transition-[background-color,border-color] duration-[var(--duration-fast)]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        ].join(" ")}
      >

        {/* Color accent gradient */}
        <div
          className="absolute inset-0 pointer-events-none -z-10 opacity-70 group-hover:opacity-100 transition-opacity duration-[var(--duration-fast)]"
          style={{
            background: `radial-gradient(circle at top left, ${avatarColor}1f, transparent 55%)`,
          }}
          aria-hidden="true"
        />

        {/* Action buttons (top-right) */}
        <CardActionStrip>
          <CardActionButton
            icon={Activity}
            label="Testar"
            onClick={() => void checkHealth(host.id)}
            ariaLabel={`Testar conexão com ${displayName}`}
            detail={health.message ?? undefined}
            testId={`host-card-${host.id}-health`}
            disabled={health.status === "checking"}
            busy={health.status === "checking"}
            colorClass={statusColor(health.status)}
          />
          {!ftp && (
            <CardActionButton
              icon={TerminalSquare}
              label="Terminal"
              onClick={() => onConnect(host)}
              ariaLabel={`Abrir terminal de ${displayName}`}
              testId={`host-card-${host.id}-terminal`}
            />
          )}
          <CardActionButton
            icon={FolderOpen}
            label="Arquivos"
            onClick={() => onExplore(host)}
            ariaLabel={`Abrir arquivos de ${displayName}`}
            testId={`host-card-${host.id}-explorer`}
          />
        </CardActionStrip>

        {/* Avatar circle */}
        <div
          className="flex items-center justify-center w-10 h-10 rounded-full shrink-0 font-bold text-[length:var(--text-base)] select-none"
          style={{
            backgroundColor: `${avatarColor}25`,
            color: avatarColor,
            fontFamily: "var(--font-sans)",
          }}
          aria-hidden="true"
        >
          {initial}
        </div>

        {/* Host info */}
        <div className="min-w-0">
          <p className="text-[length:var(--text-base)] font-bold text-text-primary truncate leading-tight">
            {displayName}
          </p>
          <p className="mt-1.5 text-[13px] text-text-secondary truncate" title={subtitle}>
            {subtitle}
          </p>
          <div className="flex items-center gap-1.5 mt-2.5 flex-wrap">
            <ProtocolBadge label={proto} />
            <EnvBadge env={host.environment} />
          </div>
          {jumpLabel && (
            <div
              data-testid={`host-card-${host.id}-tunnel`}
              className="flex items-center gap-1 mt-0.5 text-[length:var(--text-xs)] text-text-muted truncate"
              title={`Passa por ${jumpLabel}`}
            >
              <Waypoints size={11} strokeWidth={2} aria-hidden="true" className="shrink-0" />
              <span className="truncate">via {jumpLabel}</span>
            </div>
          )}
          {/* Always-mounted live region: announces the result to screen
              readers and reserves a line so checking a host doesn't shift the
              card height (and its row neighbours in the stretch grid). */}
          <p
            data-testid={`host-card-${host.id}-health-status`}
            role="status"
            aria-live="polite"
            className={[
              "mt-1 min-h-[1rem] text-[length:var(--text-xs)] font-medium truncate",
              healthLabel ? statusColor(health.status) : "",
            ].join(" ")}
            title={health.message ?? undefined}
          >
            {healthLabel}
          </p>
        </div>

      </div>

      {contextMenu && (
        <ContextMenu
          items={contextItems}
          position={contextMenu}
          onClose={() => setContextMenu(null)}
        />
      )}

      <ConfirmDangerDialog
        open={confirmDelete}
        title="Excluir este servidor?"
        message="O servidor e a senha salva serão removidos de vez."
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => {
          setConfirmDelete(false);
          onDelete(host.id);
        }}
      />
    </>
  );
}
