import { useEffect, useState, useRef, useCallback } from "react";
import { Search, Clock, ChevronDown, Loader2, TerminalSquare, FolderOpen, Trash2 } from "lucide-react";
import { toast } from "../../stores/toast-store";
import { CustomSelect } from "../shared/CustomSelect";
import { useSftpStore } from "../../stores/sftp-store";
import { useSessionStore } from "../../stores/session-store";
import { useHostsStore } from "../../stores/hosts-store";
import { useTabStore } from "../../stores/tab-store";
import { ContextMenu } from "../shared/ContextMenu";
import { ConfirmDangerDialog } from "../shared/ConfirmDangerDialog";
import type { ContextMenuItem } from "../shared/ContextMenu";
import type { ConnectionHistoryEntry } from "../../types";
import { parseSqliteUtc } from "../../utils/time";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function formatDateTime(iso: string): string {
  const d = parseSqliteUtc(iso);
  return d.toLocaleString("pt-BR", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatTime(iso: string): string {
  const d = parseSqliteUtc(iso);
  return d.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
}

function relativeTime(iso: string): string {
  const diff = Date.now() - parseSqliteUtc(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "agora";
  if (mins < 60) return `há ${mins} min`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `há ${hours} h`;
  const days = Math.floor(hours / 24);
  if (days === 1) return "ontem";
  if (days < 7) return `há ${days} dias`;
  return formatTime(iso);
}

// ─── Component ───────────────────────────────────────────────────────────────

const PAGE_SIZE = 50;


export function HistoryPage() {
  const hosts = useHostsStore((s) => s.hosts);
  const loadHosts = useHostsStore((s) => s.loadHosts);
  const [entries, setEntries] = useState<ConnectionHistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [hostFilter, setHostFilter] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [mutating, setMutating] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const offsetRef = useRef(0);
  const [contextMenu, setContextMenu] = useState<{ entry: ConnectionHistoryEntry; x: number; y: number } | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);

  const loadEntries = async (reset: boolean) => {
    setLoading(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const currentOffset = reset ? 0 : offsetRef.current;
      const result = await invoke<ConnectionHistoryEntry[]>("list_connection_history", {
        hostId: hostFilter,
        limit: PAGE_SIZE,
        offset: currentOffset,
      });
      if (reset) {
        setEntries(result);
        offsetRef.current = PAGE_SIZE;
      } else {
        setEntries((prev) => [...prev, ...result]);
        offsetRef.current = currentOffset + PAGE_SIZE;
      }
      setHasMore(result.length === PAGE_SIZE);
    } catch {
      // best-effort
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadEntries(true);
    void loadHosts();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostFilter]);

  // Filter locally by search query
  const filtered = query.trim()
    ? entries.filter((e) => {
        const q = query.trim().toLowerCase();
        return (
          e.host_label.toLowerCase().includes(q) ||
          e.host.toLowerCase().includes(q) ||
          e.username.toLowerCase().includes(q)
        );
      })
    : entries;

  const grouped = groupByDate(filtered);

  // Total count for display
  const totalCount = entries.length + (hasMore ? "+" : "");

  // FTP hosts have no terminal: both actions open the explorer for them.
  const openIfFtp = useCallback(async (entry: ConnectionHistoryEntry): Promise<boolean> => {
    const { useHostsStore } = await import("../../stores/hosts-store");
    const host = useHostsStore.getState().hosts.find((h) => h.id === entry.host_id);
    if (!host || host.protocol !== "ftp") return false;
    const { openFtpExplorer } = await import("../../lib/ftp-explorer");
    try {
      await openFtpExplorer(host).done;
    } catch {
      // Errors surface via the explorer
    }
    return true;
  }, []);

  const handleTerminal = useCallback(async (entry: ConnectionHistoryEntry) => {
    if (await openIfFtp(entry)) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const sessionId = await invoke<string>("connect_saved_host", { hostId: entry.host_id });
      const label = entry.host_label || `${entry.username}@${entry.host}`;
      useSessionStore.getState().addSession(sessionId, {
        host: entry.host,
        port: entry.port,
        username: entry.username,
        label: entry.host_label || undefined,
        auth_method: { type: "password", password: "" },
      });
      useTabStore.getState().addTab({ type: "terminal", id: sessionId, label });
    } catch {
      // Connection errors show via disconnect overlay
    }
  }, [openIfFtp]);

  const handleContextMenu = (e: React.MouseEvent, entry: ConnectionHistoryEntry) => {
    e.preventDefault();
    setContextMenu({ entry, x: e.clientX, y: e.clientY });
  };

  const handleExplorer = useCallback(async (entry: ConnectionHistoryEntry) => {
    if (await openIfFtp(entry)) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const sessionId = await invoke<string>("connect_saved_host_no_pty", { hostId: entry.host_id });
      const label = entry.host_label || `${entry.username}@${entry.host}`;

      // Prefer SFTP; fall back to SCP on the same connection when the SFTP
      // subsystem is disabled.
      let explorerSessionId: string;
      let transport: "sftp" | "scp" = "sftp";
      try {
        explorerSessionId = await invoke<string>("sftp_open", { sessionId });
      } catch (sftpErr) {
        try {
          explorerSessionId = await invoke<string>("scp_open", { sessionId });
          transport = "scp";
        } catch {
          throw sftpErr;
        }
      }

      useSftpStore.getState().openSession(explorerSessionId, sessionId, label, entry.username);
      useTabStore.getState().addTab({ type: "sftp", id: explorerSessionId, label, transport });
    } catch {
      // Errors surface via SFTP page
    }
  }, [openIfFtp]);

  const handleDeleteHistoryEntry = useCallback(async (entryId: number) => {
    setMutating(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("delete_connection_history_entry", { id: entryId });
      setEntries((prev) => prev.filter((entry) => entry.id !== entryId));
      offsetRef.current -= 1;
      setConfirmDeleteId(null);
    } catch {
      toast.error("Não foi possível excluir o registro do histórico.");
    } finally {
      setMutating(false);
    }
  }, []);

  const buildContextItems = (entry: ConnectionHistoryEntry): ContextMenuItem[] => [
    {
      label: "Terminal",
      icon: TerminalSquare,
      onClick: () => void handleTerminal(entry),
    },
    {
      label: "Arquivos",
      icon: FolderOpen,
      onClick: () => void handleExplorer(entry),
    },
    {
      label: "Excluir",
      icon: Trash2,
      danger: true,
      disabled: mutating,
      onClick: () => setConfirmDeleteId(entry.id),
    },
    {
      label: `Filtrar por ${entry.host_label || entry.host}`,
      separator: true,
      disabled: mutating,
      onClick: () => {
        setHostFilter(entry.host_id);
        setQuery("");
      },
    },
  ];

  return (
    <>
      <div className="flex flex-col h-full overflow-y-scroll">
        <div className="max-w-4xl w-full mx-auto px-8 py-8 flex flex-col gap-8">

          {/* Page title */}
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-[28px] font-bold tracking-tight text-text-primary">Histórico</h1>
              <p className="text-[length:var(--text-xs)] text-text-muted mt-1">
                Registro de todas as conexões SSH e FTP: quando você conectou, em qual servidor e com qual usuário
              </p>
            </div>
            {entries.length > 0 && (
              <span className="text-[length:var(--text-2xs)] text-text-muted tabular-nums shrink-0">
                {totalCount} {entries.length === 1 && !hasMore ? "conexão" : "conexões"}
              </span>
            )}
          </div>

          {/* Search */}
          <div className="relative">
            <Search
              size={16}
              strokeWidth={2}
              className="absolute left-3.5 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none"
              aria-hidden="true"
            />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Escape") setQuery(""); }}
              placeholder="Buscar conexões…"
              aria-label="Buscar no histórico de conexões"
              className={[
                "w-full pl-10 pr-4 py-2.5 rounded-xl text-[length:var(--text-sm)]",
                "bg-bg-surface border border-border text-text-primary placeholder:text-text-muted",
                "outline-none transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
                "focus:border-border-focus focus:ring-2 focus:ring-ring",
              ].join(" ")}
            />
          </div>

          {/* Filters */}
          {hosts.length > 0 && (
            <div className="flex gap-2">
              <CustomSelect
                value={hostFilter ?? ""}
                onChange={(v) => setHostFilter(v || null)}
                aria-label="Filtrar por servidor"
                options={[
                  { value: "", label: "Todos os servidores" },
                  ...hosts.map((h) => ({
                    value: h.id,
                    label: h.label || `${h.username}@${h.host}`,
                  })),
                ]}
                className="w-56"
              />
            </div>
          )}

          {/* Entries grouped by date */}
          {grouped.length > 0 ? (
            <div className="flex flex-col gap-6">
              {grouped.map((group) => (
                <section key={group.label} aria-label={group.label}>
                  <h2 className="text-[length:var(--text-xs)] font-semibold uppercase tracking-widest text-text-muted mb-3">
                    {group.label}
                  </h2>
                  <div className="rounded-lg bg-bg-surface border border-border/50 divide-y divide-border/30 overflow-hidden">
                    {group.entries.map((entry) => {
                      const hasCustomLabel = entry.host_label && entry.host_label !== entry.host && entry.host_label !== `${entry.username}@${entry.host}`;

                      return (
                        <div
                          key={entry.id}
                          data-testid={`history-entry-${entry.id}`}
                          data-history-host-id={entry.host_id}
                          data-history-host-label={entry.host_label || entry.host}
                          onContextMenu={(e) => handleContextMenu(e, entry)}
                          onDoubleClick={() => void handleTerminal(entry)}
                          className={[
                            "flex items-center gap-4 px-4 py-2 cursor-default",
                            "hover:bg-bg-overlay/40 transition-colors duration-[var(--duration-fast)]",
                          ].join(" ")}
                          title="Clique duplo para reconectar · Botão direito para mais opções"
                        >
                          {/* Time */}
                          <span
                            className="text-[length:var(--text-2xs)] text-text-muted tabular-nums w-14 shrink-0"
                            title={formatDateTime(entry.connected_at)}
                          >
                            {relativeTime(entry.connected_at)}
                          </span>

                          {/* Host label — clickable to filter */}
                          <button
                            onClick={() => {
                              setHostFilter(entry.host_id);
                              setQuery("");
                            }}
                            className="text-[length:var(--text-sm)] font-medium text-text-primary truncate flex-1 text-left hover:text-accent transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
                            title={`Filtrar por ${entry.host_label || entry.host}`}
                          >
                            {entry.host_label || entry.host}
                          </button>

                          {/* User@host — only show if host has a custom label */}
                          {hasCustomLabel && (
                            <span className="text-[length:var(--text-2xs)] font-mono text-text-muted truncate max-w-[200px]">
                              {entry.username}@{entry.host}
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </section>
              ))}

              {/* Load more */}
              {hasMore && !query.trim() && (
                <button
                  onClick={() => void loadEntries(false)}
                  disabled={loading}
                  className={[
                    "flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg mx-auto",
                    "text-[length:var(--text-xs)] font-medium text-text-muted",
                    "bg-bg-surface border border-border",
                    "hover:border-border-focus hover:text-text-secondary hover:bg-bg-overlay",
                    "transition-all duration-[var(--duration-fast)]",
                    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    "disabled:opacity-50",
                  ].join(" ")}
                >
                  {loading ? (
                    <Loader2 size={14} strokeWidth={2} className="motion-safe:animate-spin" />
                  ) : (
                    <ChevronDown size={14} strokeWidth={2} />
                  )}
                  {loading ? "Carregando…" : "Carregar mais"}
                </button>
              )}
            </div>
          ) : loading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 size={22} strokeWidth={2} className="text-text-muted motion-safe:animate-spin" />
            </div>
          ) : entries.length > 0 && query.trim() ? (
            <p className="text-[length:var(--text-sm)] text-text-muted py-8 text-center">
              Nenhuma conexão encontrada para &ldquo;{query}&rdquo;
            </p>
          ) : (
            <div className="flex flex-col items-center justify-center py-16 gap-3">
              <Clock size={30} strokeWidth={1.2} className="text-text-muted/30" />
              <p className="text-[length:var(--text-sm)] text-text-muted">
                Nenhuma conexão no histórico
              </p>
              <p className="text-[length:var(--text-xs)] text-text-muted/60 text-center max-w-xs">
                O registro das suas conexões aparece aqui conforme você se conecta aos servidores
              </p>
            </div>
          )}
        </div>
      </div>

      {/* Context menu */}
      {contextMenu && (
        <ContextMenu
          items={buildContextItems(contextMenu.entry)}
          position={{ x: contextMenu.x, y: contextMenu.y }}
          onClose={() => setContextMenu(null)}
        />
      )}

      <ConfirmDangerDialog
        open={confirmDeleteId !== null}
        title="Excluir este registro do histórico?"
        message="Este registro será removido para sempre."
        busy={mutating}
        onCancel={() => setConfirmDeleteId(null)}
        onConfirm={() => {
          if (confirmDeleteId !== null) void handleDeleteHistoryEntry(confirmDeleteId);
        }}
      />
    </>
  );
}

// ─── Grouping ────────────────────────────────────────────────────────────────

interface DateGroup {
  label: string;
  entries: ConnectionHistoryEntry[];
}

function groupByDate(entries: ConnectionHistoryEntry[]): DateGroup[] {
  const groups = new Map<string, ConnectionHistoryEntry[]>();
  const today = new Date();
  const todayStr = today.toDateString();
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const yesterdayStr = yesterday.toDateString();

  for (const entry of entries) {
    const d = parseSqliteUtc(entry.connected_at);
    const dateStr = d.toDateString();

    const label = dateStr === todayStr
      ? "Hoje"
      : dateStr === yesterdayStr
        ? "Ontem"
        : d.toLocaleDateString("pt-BR", { weekday: "long", month: "long", day: "numeric", year: "numeric" });

    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(entry);
  }

  return Array.from(groups.entries()).map(([label, entries]) => ({ label, entries }));
}
