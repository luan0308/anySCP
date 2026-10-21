import { useRef, useCallback, useState } from "react";
import { Server, Braces, Settings, ArrowUpDown, Plug, History, ChevronsLeft, ChevronsRight } from "lucide-react";
import { useUiStore } from "../../stores/ui-store";
import { useTabStore, type PageId } from "../../stores/tab-store";
import { useTransferStore } from "../../stores/transfer-store";
import { TransferPopover } from "../transfers/TransferPopover";
import { getStatusString } from "../../utils/format";
import { FilesMark } from "../brand";

interface NavItem {
  id: string;
  icon: React.ElementType;
  label: string;
  /** For page tabs */
  page?: PageId;
  /** For session-type tabs */
  sessionType?: "terminal" | "sftp";
}

const NAV_ITEMS: NavItem[] = [
  { id: "hosts",            icon: Server,         label: "Servidores", page: "hosts" },
  { id: "snippets",         icon: Braces,         label: "Comandos",  page: "snippets" },
  { id: "port-forwarding",  icon: Plug,           label: "Túneis",    page: "port-forwarding" },
  { id: "history",          icon: History,        label: "Histórico", page: "history" },
];

// ─── Pill button ─────────────────────────────────────────────────────────────

function PillButton({
  icon: Icon,
  label,
  isActive,
  badge,
  expanded,
  onClick,
  buttonRef,
  ariaExpanded,
}: {
  icon: React.ElementType;
  label: string;
  isActive: boolean;
  badge?: number;
  expanded: boolean;
  onClick: () => void;
  buttonRef?: React.Ref<HTMLButtonElement>;
  ariaExpanded?: boolean;
}) {
  const [hovered, setHovered] = useState(false);

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        onClick={onClick}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        title={label}
        aria-label={label}
        aria-current={isActive ? "page" : undefined}
        aria-expanded={ariaExpanded}
        className={[
          "relative flex items-center rounded-[12px]",
          "transition-all duration-[var(--duration-fast)]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          expanded ? "w-full gap-2.5 px-3 h-10" : "justify-center w-10 h-10",
          isActive
            ? "bg-accent text-text-inverse"
            : "text-text-secondary hover:text-text-primary hover:bg-white/[0.07]",
        ].join(" ")}
      >
        <Icon
          size={18}
          strokeWidth={1.75}
          className={`shrink-0 ${isActive ? "text-text-inverse" : "opacity-70"}`}
        />

        {/* Label — expanded only */}
        {expanded && (
          <span className="text-[length:var(--text-sm)] font-medium truncate">
            {label}
          </span>
        )}

        {/* Badge */}
        {badge !== undefined && badge > 0 && (
          expanded ? (
            <span className={[
              "ml-auto flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded-full",
              "text-[length:var(--text-2xs)] font-bold tabular-nums leading-none",
              "bg-accent/15 text-accent",
            ].join(" ")}>
              {badge}
            </span>
          ) : (
            <span className="absolute -top-1 -right-1 flex items-center justify-center min-w-[15px] h-[15px] px-0.5 rounded-full bg-accent text-[9px] font-bold text-text-inverse leading-none">
              {badge}
            </span>
          )
        )}
      </button>

      {/* Tooltip — collapsed only */}
      {!expanded && hovered && (
        <div
          className={[
            "absolute left-full ml-3 top-1/2 -translate-y-1/2 z-50",
            "px-2.5 py-1 rounded-lg",
            "bg-bg-overlay border border-border shadow-[var(--shadow-md)]",
            "text-[length:var(--text-xs)] font-medium text-text-primary whitespace-nowrap",
            "animate-[fadeIn_80ms_var(--ease-expo-out)_both]",
            "pointer-events-none",
          ].join(" ")}
        >
          {label}
        </div>
      )}
    </div>
  );
}

// ─── Sidebar ─────────────────────────────────────────────────────────────────

export function Sidebar() {
  const expanded = useUiStore((s) => s.sidebarExpanded);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);

  const activeTab = useTabStore((s) => s.activeTabId ? s.tabs.get(s.activeTabId) : null);
  const openPageTab = useTabStore((s) => s.openPageTab);
  const activateRecent = useTabStore((s) => s.activateRecentTabOfType);

  const activeTransferCount = useTransferStore((s) => {
    let count = 0;
    for (const t of s.transfers.values()) {
      const st = getStatusString(t.status);
      if (st === "InProgress" || st === "Queued") count++;
    }
    return count;
  });
  const popoverOpen = useTransferStore((s) => s.popoverOpen);
  const togglePopover = useTransferStore((s) => s.togglePopover);
  const setPopoverOpenStore = useTransferStore((s) => s.setPopoverOpen);
  const transferBtnRef = useRef<HTMLButtonElement>(null);

  const handleTransferClick = useCallback(() => {
    togglePopover();
  }, [togglePopover]);

  const handlePopoverClose = useCallback(() => {
    setPopoverOpenStore(false);
  }, [setPopoverOpenStore]);

  // Determine which nav item is "active" based on the active tab
  const getActiveId = (): string | null => {
    if (!activeTab) return null;
    if (activeTab.type === "terminal") return "terminal";
    if (activeTab.type === "sftp") return "sftp";
    if (activeTab.type === "s3") return "sftp"; // S3 grouped under Explorer
    if (activeTab.type === "page") return activeTab.page;
    return null;
  };
  const activeNavId = getActiveId();

  const visibleNavItems = NAV_ITEMS;

  const handleNavClick = (item: NavItem) => {
    if (item.page) {
      openPageTab(item.page, item.label);
    } else if (item.sessionType) {
      activateRecent(item.sessionType);
    }
  };

  return (
    <>
      <nav
        data-testid="sidebar"
        data-sidebar-expanded={expanded}
        aria-label="Navegação principal"
        className={[
          "no-select flex flex-col shrink-0 h-[calc(100%-16px)] py-4 ml-2 mt-2",
          "bg-white/[0.05] backdrop-blur-sm rounded-[18px]",
          "transition-[width] duration-[var(--duration-base)] ease-[var(--ease-expo-out)]",
          expanded ? "w-[208px] items-stretch px-3" : "w-[64px] items-center",
        ].join(" ")}
      >
        {/* Brand */}
        <div className={`flex items-center gap-2.5 mb-5 ${expanded ? "px-1" : "justify-center"}`}>
          <FilesMark size={32} />
          {expanded && (
            <span className="text-[length:var(--text-base)] font-bold text-text-primary truncate">
              Files
            </span>
          )}
        </div>

        {/* Nav items */}
        <div className={`flex flex-col gap-2 ${expanded ? "" : "items-center"}`}>
          {visibleNavItems.map((item) => (
            <PillButton
              key={item.id}
              icon={item.icon}
              label={item.label}
              isActive={activeNavId === item.id}
              badge={undefined}
              expanded={expanded}
              onClick={() => handleNavClick(item)}
            />
          ))}
        </div>

        {/* Spacer */}
        <div className="flex-1" />

        {/* Bottom actions */}
        <div className={`flex flex-col gap-2 ${expanded ? "" : "items-center"}`}>
          <PillButton
            icon={ArrowUpDown}
            label="Transferências"
            isActive={popoverOpen}
            badge={activeTransferCount || undefined}
            expanded={expanded}
            onClick={handleTransferClick}
            buttonRef={transferBtnRef}
            ariaExpanded={popoverOpen}
          />

          <PillButton
            icon={Settings}
            label="Configurações"
            isActive={activeNavId === "settings"}
            expanded={expanded}
            onClick={() => openPageTab("settings", "Configurações")}
          />

          {/* Expand/collapse */}
          <PillButton
            icon={expanded ? ChevronsLeft : ChevronsRight}
            label={expanded ? "Recolher" : "Expandir"}
            isActive={false}
            expanded={expanded}
            onClick={toggleSidebar}
          />
        </div>
      </nav>

      {/* Transfer popover */}
      {popoverOpen && (
        <TransferPopover
          anchorRect={transferBtnRef.current?.getBoundingClientRect() ?? null}
          triggerRef={transferBtnRef}
          onClose={handlePopoverClose}
        />
      )}
    </>
  );
}
