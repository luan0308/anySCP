import { useState } from "react";
import { Cloud, Pencil, Copy, Trash2, FolderOpen } from "lucide-react";
import type { S3Connection } from "../../types";
import { ContextMenu } from "../shared/ContextMenu";
import { ConfirmDangerDialog } from "../shared/ConfirmDangerDialog";
import { getHostColor } from "./HostCard";
import { CardActionButton, CardActionStrip } from "./CardActionButton";
import { EnvBadge } from "../brand";

interface S3CardProps {
  conn: S3Connection;
  onConnect: (conn: S3Connection) => void;
  onEdit: (conn: S3Connection) => void;
  onDuplicate: (conn: S3Connection) => void;
  onDelete: (conn: S3Connection) => void;
}

// ─── Component ────────────────────────────────────────────────────────────────

export function S3Card({ conn, onConnect, onEdit, onDuplicate, onDelete }: S3CardProps) {
  const displayName = conn.label;
  const accentColor = conn.color || getHostColor(conn.label);

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const subtitleParts: string[] = [conn.provider, conn.region];
  if (conn.bucket) subtitleParts.push(conn.bucket);
  const subtitle = subtitleParts.join(" · ");


  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onConnect(conn);
    }
  };

  const contextItems = [
    { label: "Abrir arquivos", icon: FolderOpen, onClick: () => onConnect(conn) },
    { label: "Editar", icon: Pencil, onClick: () => onEdit(conn) },
    { label: "Duplicar", icon: Copy, onClick: () => onDuplicate(conn) },
    { label: "Excluir", icon: Trash2, danger: true, separator: true, onClick: () => setConfirmDelete(true) },
  ];

  return (
    <>
      <div
        data-testid={`s3-card-${conn.id}`}
        data-s3-id={conn.id}
        data-s3-label={displayName}
        role="button"
        tabIndex={0}
        onClick={() => onConnect(conn)}
        onKeyDown={handleKeyDown}
        onContextMenu={handleContextMenu}
        title={`Conectar a ${displayName}`}
        className={[
          // grab/grabbing communicates the card is draggable; a plain click still
          // connects (drag needs a 5px move to activate first).
          "group relative isolate flex flex-col gap-2.5 p-3.5 rounded-xl text-left w-full h-full cursor-grab active:cursor-grabbing overflow-hidden",
          "bg-bg-surface border border-border",
          "hover:border-border-focus hover:bg-bg-overlay",
          "transition-[background-color,border-color] duration-[var(--duration-fast)]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        ].join(" ")}
      >

        {/* Color accent gradient */}
        <div
          className="absolute inset-0 pointer-events-none -z-10 opacity-70 group-hover:opacity-100 transition-opacity duration-[var(--duration-fast)]"
          style={{
            background: `radial-gradient(circle at top left, ${accentColor}33, transparent 60%)`,
          }}
          aria-hidden="true"
        />

        {/* Action buttons (top-right) */}
        <CardActionStrip>
          <CardActionButton
            icon={FolderOpen}
            label="Arquivos"
            onClick={() => onConnect(conn)}
            ariaLabel={`Abrir arquivos de ${displayName}`}
            testId={`s3-card-${conn.id}-explorer`}
          />
        </CardActionStrip>

        {/* Avatar circle */}
        <div
          className="flex items-center justify-center w-9 h-9 rounded-full shrink-0 select-none"
          style={{
            backgroundColor: `${accentColor}25`,
            color: accentColor,
          }}
          aria-hidden="true"
        >
          <Cloud size={16} strokeWidth={2} />
        </div>

        {/* Connection info */}
        <div className="min-w-0">
          <p className="text-[length:var(--text-sm)] font-medium text-text-primary truncate leading-tight pr-5">
            {displayName}
          </p>
          <div className="flex items-center gap-1.5 mt-0.5 flex-wrap">
            <p className="text-[length:var(--text-xs)] text-text-muted font-mono truncate">
              {subtitle}
            </p>
            <EnvBadge env={conn.environment} />
          </div>
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
        title="Excluir esta conexão S3?"
        message="Esta conexão será removida permanentemente."
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => {
          setConfirmDelete(false);
          onDelete(conn);
        }}
      />
    </>
  );
}
