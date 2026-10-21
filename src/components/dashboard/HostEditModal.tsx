import { useState, useCallback, useEffect, useRef } from "react";
import { Monitor, TerminalSquare, FolderOpen, Check } from "lucide-react";
import { ModalShell, BTN_GHOST, BTN_SECONDARY, BTN_PRIMARY } from "../shared/ModalShell";
import { useUiStore } from "../../stores/ui-store";
import { useHostsStore } from "../../stores/hosts-store";
import { useGroupsStore } from "../../stores/groups-store";
import { useSessionStore } from "../../stores/session-store";
import { useTabStore } from "../../stores/tab-store";
import type { SavedHost, HostConfig, StoredCredential, HostProtocol, FtpSecurity } from "../../types";
import { openFtpExplorer } from "../../lib/ftp-explorer";
import { HOST_COLORS } from "./HostCard";
import { CustomSelect } from "../shared/CustomSelect";

// ─── Field types ─────────────────────────────────────────────────────────────

type AuthType = "password" | "privateKey";

/** Sentinel value: when editingHostId === NEW_HOST_ID, we create a new host
 *  instead of loading an existing one. */
export const NEW_HOST_ID = "__new__";

interface FormState {
  // Connection
  protocol: HostProtocol;
  ftpSecurity: FtpSecurity;
  ftpPassive: boolean;
  ftpAllowInvalidCert: boolean;
  label: string;
  host: string;
  port: string;
  username: string;
  authType: AuthType;
  groupId: string;
  keyPath: string;
  proxyJump: string;
  proxyJumpHostId: string;
  keepAliveInterval: string;
  defaultShell: string;
  startupCommand: string;
  startDirectory: string;
  // Auth credentials (only used at connect-time, never persisted)
  password: string;
  passphrase: string;
  // Appearance
  color: string;
  environment: string;
  osType: string;
  // Notes
  notes: string;
}

const EMPTY_FORM: FormState = {
  protocol: "ssh",
  ftpSecurity: "explicit",
  ftpPassive: true,
  ftpAllowInvalidCert: false,
  label: "",
  host: "",
  port: "22",
  username: "",
  authType: "password",
  groupId: "",
  keyPath: "",
  proxyJump: "",
  proxyJumpHostId: "",
  keepAliveInterval: "",
  defaultShell: "",
  startupCommand: "",
  startDirectory: "",
  password: "",
  passphrase: "",
  color: "",
  environment: "",
  osType: "",
  notes: "",
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

function extractError(err: unknown, fallback: string): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: string }).message);
  }
  return fallback;
}

function savedHostToForm(host: SavedHost): FormState {
  const authType: AuthType =
    host.auth_type === "privateKey" ? "privateKey" : "password";
  return {
    protocol: host.protocol === "ftp" ? "ftp" : "ssh",
    ftpSecurity: host.ftp_security ?? "none",
    ftpPassive: host.ftp_passive ?? true,
    ftpAllowInvalidCert: host.ftp_allow_invalid_cert ?? false,
    label: host.label ?? "",
    host: host.host,
    port: String(host.port),
    username: host.username,
    authType,
    groupId: host.group_id ?? "",
    keyPath: host.key_path ?? "",
    proxyJump: host.proxy_jump ?? "",
    proxyJumpHostId: host.proxy_jump_host_id ?? "",
    keepAliveInterval: host.keep_alive_interval != null ? String(host.keep_alive_interval) : "",
    defaultShell: host.default_shell ?? "",
    startupCommand: host.startup_command ?? "",
    startDirectory: host.start_directory ?? "",
    password: "",
    passphrase: "",
    color: host.color ?? "",
    environment: host.environment ?? "",
    osType: host.os_type ?? "",
    notes: host.notes ?? "",
  };
}

/** Default port for a protocol / FTPS mode. */
function defaultPort(protocol: HostProtocol, security: FtpSecurity): string {
  if (protocol === "ssh") return "22";
  return security === "implicit" ? "990" : "21";
}

const DEFAULT_PORTS = new Set(["", "21", "22", "990"]);

// ─── Section header ───────────────────────────────────────────────────────────

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 my-1">
      <span className="text-[length:var(--text-xs)] font-semibold uppercase tracking-widest text-text-muted whitespace-nowrap">
        {children}
      </span>
      <div className="flex-1 h-px bg-border" aria-hidden="true" />
    </div>
  );
}

// ─── Component ────────────────────────────────────────────────────────────────

export function HostEditModal() {
  const editingHostId = useUiStore((s) => s.editingHostId);
  const setEditingHostId = useUiStore((s) => s.setEditingHostId);

  const saveHost = useHostsStore((s) => s.saveHost);
  const deleteHost = useHostsStore((s) => s.deleteHost);
  const hosts = useHostsStore((s) => s.hosts);
  const loadHosts = useHostsStore((s) => s.loadHosts);
  const groups = useGroupsStore((s) => s.groups);
  const loadGroups = useGroupsStore((s) => s.loadGroups);
  const addSession = useSessionStore((s) => s.addSession);

  // Whether "Connect through SSH tunnel" is enabled for this host.
  const [tunnelEnabled, setTunnelEnabled] = useState(false);

  // Original host snapshot (preserved for id + created_at on save)
  const [originalHost, setOriginalHost] = useState<SavedHost | null>(null);

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [loadingHost, setLoadingHost] = useState(false);
  const [saving, setSaving] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState(false);

  // Vault credential state
  /** True when the keychain already holds a credential for this host. */
  const [hasSavedCred, setHasSavedCred] = useState(false);
  /** True when the user has explicitly clicked "Clear" on the saved credential. */
  const [credCleared, setCredCleared] = useState(false);

  const firstInputRef = useRef<HTMLInputElement>(null);

  const isOpen = editingHostId !== null;

  // ── Close helper ────────────────────────────────────────────────────────────
  const close = useCallback(() => {
    setEditingHostId(null);
  }, [setEditingHostId]);

  const isNewHost = editingHostId === NEW_HOST_ID;

  // ── Load SSH keys once ──────────────────────────────────────────────────────
  const [sshKeys, setSshKeys] = useState<import("../../types").SshKeyInfo[]>([]);
  const sshKeysLoaded = useRef(false);

  useEffect(() => {
    if (sshKeysLoaded.current) return;
    let cancelled = false;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const keys = await invoke<import("../../types").SshKeyInfo[]>("list_ssh_keys");
        if (!cancelled) setSshKeys(keys);
      } catch { /* non-fatal */ }
      finally { sshKeysLoaded.current = true; }
    })();
    return () => { cancelled = true; };
  }, []);

  // ── Load host data when modal opens ────────────────────────────────────────
  useEffect(() => {
    if (!isOpen || !editingHostId) {
      return;
    }

    // Reset transient state
    setError(null);
    setDeleteConfirm(false);
    setSaving(false);
    setConnecting(false);
    setOriginalHost(null);
    setForm(EMPTY_FORM);
    setHasSavedCred(false);
    setCredCleared(false);
    setTunnelEnabled(false);

    // Load groups + hosts (for the tunnel dropdown) in parallel
    loadGroups().catch(() => {/* non-fatal */});
    loadHosts().catch(() => {/* non-fatal */});

    if (isNewHost) {
      // New host — no fetch needed
      setLoadingHost(false);
      return;
    }

    setLoadingHost(true);

    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const [host, hasCred] = await Promise.all([
          invoke<SavedHost>("get_host", { id: editingHostId }),
          invoke<boolean>("vault_has_credential", { hostId: editingHostId }).catch(() => false),
        ]);
        setOriginalHost(host);
        setForm(savedHostToForm(host));
        setHasSavedCred(hasCred);
        setTunnelEnabled(!!host.proxy_jump_host_id);
      } catch (err) {
        setError(extractError(err, "Não foi possível carregar o servidor"));
      } finally {
        setLoadingHost(false);
        requestAnimationFrame(() => firstInputRef.current?.focus());
      }
    })();
  }, [isOpen, editingHostId, isNewHost, loadGroups, loadHosts]);

  // Focus the first field (Label) when the modal opens.
  useEffect(() => {
    if (isOpen && !loadingHost) {
      requestAnimationFrame(() => firstInputRef.current?.focus());
    }
  }, [isOpen, loadingHost]);

  // When deleteConfirm is active, Escape clears it rather than closing the modal.
  const handleClose = useCallback(() => {
    if (deleteConfirm) setDeleteConfirm(false);
    else close();
  }, [deleteConfirm, close]);

  // ── Form field updater ──────────────────────────────────────────────────────
  const setField = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setError(null);
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  /** Switch protocol / FTPS mode, following the port along when it is still
   *  a default one (22, 21 or 990) so a custom port is never overwritten. */
  const setProtocol = (protocol: HostProtocol, security: FtpSecurity = form.ftpSecurity) => {
    setError(null);
    if (protocol === "ftp") setTunnelEnabled(false);
    setForm((prev) => ({
      ...prev,
      protocol,
      ftpSecurity: security,
      authType: protocol === "ftp" ? "password" : prev.authType,
      port: DEFAULT_PORTS.has(prev.port.trim()) ? defaultPort(protocol, security) : prev.port,
    }));
  };

  const isFtp = form.protocol === "ftp";
  // Only SSH hosts can serve as a tunnel.
  const tunnelCandidates = hosts.filter((h) => h.protocol !== "ftp");

  // ── Validation ──────────────────────────────────────────────────────────────
  const validate = (): string | null => {
    if (!form.host.trim()) return "Informe o endereço";
    if (!form.username.trim()) return "Informe o usuário";
    const portNum = parseInt(form.port, 10);
    if (isNaN(portNum) || portNum < 1 || portNum > 65535) {
      return "A porta precisa estar entre 1 e 65535";
    }
    if (form.keepAliveInterval !== "") {
      const kai = parseInt(form.keepAliveInterval, 10);
      if (isNaN(kai) || kai < 0) return "O keep-alive precisa ser um número positivo";
    }
    if (isFtp && form.startDirectory.trim() && !form.startDirectory.trim().startsWith("/")) {
      return "A pasta inicial precisa ser um caminho absoluto, como /public_html";
    }
    if (tunnelEnabled && !isFtp) {
      const candidates = tunnelCandidates.filter((h) => h.id !== originalHost?.id);
      if (candidates.length === 0) {
        return "Não há outro servidor salvo para usar como túnel";
      }
      // Rejects both an empty selection and a stale one whose host no longer
      // exists in the dropdown (e.g. it was deleted while the modal was open).
      if (!candidates.some((h) => h.id === form.proxyJumpHostId)) {
        return "Escolha o servidor do túnel ou desative o túnel SSH";
      }
    }
    return null;
  };

  // ── Build SavedHost from form (works for both new and edit) ─────────────────
  const buildHost = (): SavedHost => {
    const now = new Date().toISOString();
    const base: SavedHost = originalHost ?? {
      id: crypto.randomUUID(),
      label: "",
      host: "",
      port: 22,
      username: "",
      auth_type: "password",
      group_id: null,
      created_at: now,
      updated_at: now,
      key_path: null,
      color: null,
      notes: null,
      environment: null,
      os_type: null,
      startup_command: null,
      proxy_jump: null,
      proxy_jump_host_id: null,
      start_directory: null,
      keep_alive_interval: null,
      default_shell: null,
      font_size: null,
      last_connected_at: null,
      connection_count: null,
      protocol: "ssh",
    };
    if (isFtp) {
      return {
        ...base,
        protocol: "ftp",
        ftp_security: form.ftpSecurity,
        ftp_passive: form.ftpPassive,
        ftp_allow_invalid_cert: form.ftpSecurity !== "none" && form.ftpAllowInvalidCert,
        label: form.label.trim(),
        host: form.host.trim(),
        port: parseInt(form.port, 10),
        username: form.username.trim(),
        auth_type: "password",
        group_id: form.groupId === "" ? null : form.groupId,
        updated_at: new Date().toISOString(),
        key_path: null,
        proxy_jump: null,
        proxy_jump_host_id: null,
        keep_alive_interval: null,
        default_shell: null,
        startup_command: null,
        start_directory: form.startDirectory.trim() || null,
        color: form.color || null,
        environment: form.environment || null,
        os_type: null,
        notes: form.notes.trim() || null,
      };
    }
    return {
      ...base,
      protocol: "ssh",
      ftp_security: null,
      ftp_passive: null,
      ftp_allow_invalid_cert: null,
      label: form.label.trim(),
      host: form.host.trim(),
      port: parseInt(form.port, 10),
      username: form.username.trim(),
      auth_type: form.authType,
      group_id: form.groupId === "" ? null : form.groupId,
      updated_at: new Date().toISOString(),
      key_path: form.authType === "privateKey" && form.keyPath.trim()
        ? form.keyPath.trim()
        : null,
      proxy_jump: form.proxyJump.trim() || null,
      proxy_jump_host_id:
        tunnelEnabled && form.proxyJumpHostId ? form.proxyJumpHostId : null,
      keep_alive_interval: form.keepAliveInterval.trim()
        ? parseInt(form.keepAliveInterval, 10)
        : null,
      default_shell: form.defaultShell.trim() || null,
      startup_command: form.startupCommand.trim() || null,
      start_directory: form.startDirectory.trim() || null,
      color: form.color || null,
      environment: form.environment || null,
      os_type: form.osType || null,
      notes: form.notes.trim() || null,
    };
  };

  // ── Vault helpers ────────────────────────────────────────────────────────────

  /** Saves a credential to the OS keychain, or removes it if credCleared. */
  const syncVaultCredential = async (
    hostId: string,
    invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>,
  ): Promise<void> => {
    if (credCleared) {
      // User explicitly cleared — remove from keychain (non-fatal if it didn't exist)
      try {
        await invoke("vault_delete_credential", { hostId });
      } catch { /* non-fatal */ }
      return;
    }

    if (form.authType === "password" && form.password) {
      const credential: StoredCredential = { type: "Password", password: form.password };
      await invoke("vault_save_credential", { hostId, credential });
    } else if (form.authType === "privateKey" && form.passphrase) {
      const credential: StoredCredential = { type: "KeyPassphrase", passphrase: form.passphrase };
      await invoke("vault_save_credential", { hostId, credential });
    }
    // If the field is empty and not cleared, leave the existing keychain entry untouched.
  };

  // ── Save ────────────────────────────────────────────────────────────────────
  const handleSave = async () => {
    const validationError = validate();
    if (validationError) { setError(validationError); return; }

    setSaving(true);
    setError(null);
    try {
      const host = buildHost();
      await saveHost(host);

      const { invoke } = await import("@tauri-apps/api/core");
      await syncVaultCredential(host.id, invoke as (cmd: string, args?: Record<string, unknown>) => Promise<unknown>);

      close();
    } catch (err) {
      setError(extractError(err, "Não foi possível salvar o servidor"));
    } finally {
      setSaving(false);
    }
  };

  // ── Connect (save → vault → connect_saved_host) ─────────────────────────────
  const handleConnect = async () => {
    const validationError = validate();
    if (validationError) { setError(validationError); return; }

    setConnecting(true);
    setError(null);
    try {
      const host = buildHost();
      await saveHost(host);

      const { invoke } = await import("@tauri-apps/api/core");
      const typedInvoke = invoke as (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

      // Persist credential to keychain before connecting — the Rust backend
      // reads credentials exclusively from the keychain, never from the frontend.
      await syncVaultCredential(host.id, typedInvoke);

      if (isFtp) {
        // FTP has no terminal: "Connect" opens the explorer.
        await openFtpExplorer(host).done;
        close();
        return;
      }

      // The backend resolves host config + credentials from its own DB and keychain.
      const sessionId = await invoke<string>("connect_saved_host", { hostId: host.id });

      // Build a minimal HostConfig for the session store label — no credentials.
      const hostConfig: HostConfig = {
        host: host.host,
        port: host.port,
        username: host.username,
        label: host.label || undefined,
        auth_method:
          form.authType === "privateKey"
            ? { type: "privateKey", key_path: form.keyPath }
            : { type: "password", password: "" },
      };
      addSession(sessionId, hostConfig);
      const label = hostConfig.label || `${hostConfig.username}@${hostConfig.host}`;
      useTabStore.getState().addTab({ type: "terminal", id: sessionId, label });
      // Mirror the recordConnection() call made by HostsDashboard's
      // card-click and recent-click flows — otherwise hosts connected via
      // this modal are missing from the recent list and history.
      void useHostsStore.getState().recordConnection(host.id);
      close();
    } catch (err) {
      setError(extractError(err, "A conexão falhou"));
    } finally {
      setConnecting(false);
    }
  };

  // ── Delete ───────────────────────────────────────────────────────────────────
  const handleDeleteConfirmed = async () => {
    if (!editingHostId) return;
    setSaving(true);
    setError(null);
    try {
      await deleteHost(editingHostId);
      close();
    } catch (err) {
      setError(extractError(err, "Não foi possível excluir o servidor"));
      setSaving(false);
      setDeleteConfirm(false);
    }
  };

  if (!isOpen) return null;

  const isBusy = saving || connecting;

  // ── Shared input class ───────────────────────────────────────────────────────
  const inputClass =
    "w-full rounded-lg bg-bg-base border border-border px-3 py-2 text-[length:var(--text-sm)] text-text-primary placeholder:text-text-muted outline-none focus:border-border-focus focus:ring-2 focus:ring-ring transition-[border-color,box-shadow] duration-[var(--duration-fast)]";

  const labelClass =
    "block text-[length:var(--text-xs)] font-medium text-text-secondary mb-1";



  return (
    <ModalShell
      open={isOpen}
      onClose={handleClose}
      title={isNewHost ? "Novo servidor" : "Editar servidor"}
      icon={Monitor}
      maxWidth="lg"
      scrollable
      busy={isBusy}
      testId="host-modal"
      dataAttributes={{ "data-host-modal-mode": isNewHost ? "new" : "edit" }}
      footerStart={
        !isNewHost ? (
          deleteConfirm ? (
            <DeleteConfirmRow
              onCancel={() => setDeleteConfirm(false)}
              onConfirm={handleDeleteConfirmed}
              busy={isBusy}
            />
          ) : (
            <button
              type="button"
              data-testid="host-modal-delete"
              onClick={() => setDeleteConfirm(true)}
              disabled={isBusy || loadingHost}
              className="px-3 py-1.5 text-[length:var(--text-sm)] font-medium text-status-error hover:bg-status-error/10 rounded-lg transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            >
              Excluir
            </button>
          )
        ) : undefined
      }
      footer={
        !deleteConfirm ? (
          <>
            <button type="button" data-testid="host-modal-cancel" onClick={close} disabled={isBusy} className={BTN_GHOST}>
              Cancelar
            </button>
            <button type="button" data-testid="host-modal-save" onClick={handleSave} disabled={isBusy || loadingHost} className={BTN_SECONDARY}>
              {saving ? "Salvando…" : "Salvar"}
            </button>
            <button type="button" data-testid="host-modal-connect" onClick={handleConnect} disabled={isBusy || loadingHost} className={BTN_PRIMARY}>
              {connecting ? "Conectando…" : (isFtp ? "Abrir" : "Conectar")}
            </button>
          </>
        ) : undefined
      }
    >
      <div>
          {loadingHost ? (
            <LoadingSkeleton />
          ) : (
            <div className="flex flex-col gap-3.5">

              {/* ════════════════ CONNECTION ════════════════ */}
              <SectionHeader>Conexão</SectionHeader>

              {/* Protocol: two choice cards (as in the Carrossel Splitter picker) */}
              <div className="grid grid-cols-2 gap-3" role="radiogroup" aria-label="Protocolo" data-testid="host-modal-protocol">
                {([
                  { value: "ssh", title: "SSH", desc: "Terminal + arquivos (SFTP)", Icon: TerminalSquare },
                  { value: "ftp", title: "FTP / FTPS", desc: "Só arquivos", Icon: FolderOpen },
                ] as const).map(({ value, title, desc, Icon }) => {
                  const selected = form.protocol === value;
                  return (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      data-testid={`host-modal-protocol-${value}`}
                      disabled={isBusy}
                      onClick={() => setProtocol(value as HostProtocol)}
                      className={[
                        "flex flex-col items-start gap-3 p-4 rounded-[17px] text-left",
                        "transition-colors duration-[var(--duration-fast)]",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60",
                        selected ? "bg-white text-[#1C1E21]" : "bg-white/[0.08] text-text-primary hover:bg-white/[0.12]",
                      ].join(" ")}
                    >
                      <span
                        className={[
                          "flex items-center justify-center w-[30px] h-[30px] rounded-full",
                          selected ? "bg-accent text-[#1C1E21]" : "bg-white text-[#1C1E21]",
                        ].join(" ")}
                        aria-hidden="true"
                      >
                        {selected ? <Check size={16} strokeWidth={2.5} /> : <Icon size={16} strokeWidth={1.75} />}
                      </span>
                      <span>
                        <span className="block text-[length:var(--text-base)] font-bold">{title}</span>
                        <span className={`block text-[length:var(--text-xs)] ${selected ? "text-[#1C1E21]/60" : "text-text-secondary"}`}>{desc}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
              {isFtp && (
                <div>
                  <span id="hem-ftp-security-label" className={labelClass}>Criptografia</span>
                  <div
                    role="radiogroup"
                    aria-labelledby="hem-ftp-security-label"
                    data-testid="host-modal-ftp-security"
                    className="flex p-1 rounded-full bg-white/[0.07]"
                  >
                    {([
                      { value: "explicit", label: "FTPS explícito" },
                      { value: "implicit", label: "FTPS implícito" },
                      { value: "none", label: "Sem (FTP)" },
                    ] as const).map((o) => {
                      const selected = form.ftpSecurity === o.value;
                      return (
                        <button
                          key={o.value}
                          type="button"
                          role="radio"
                          aria-checked={selected}
                          data-testid={`host-modal-ftp-security-${o.value}`}
                          disabled={isBusy}
                          onClick={() => setProtocol("ftp", o.value as FtpSecurity)}
                          className={[
                            "flex-1 h-9 rounded-full text-[13px] font-semibold",
                            "transition-colors duration-[var(--duration-fast)]",
                            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                            selected ? "bg-white text-[#1C1E21]" : "text-text-secondary hover:text-text-primary",
                          ].join(" ")}
                        >
                          {o.label}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
              {isFtp && form.ftpSecurity === "none" && (
                <p className="-mt-1.5 text-[length:var(--text-xs)] text-status-connecting">
                  FTP sem criptografia envia a senha e os arquivos abertos. Prefira FTPS quando a hospedagem oferecer.
                </p>
              )}

              {/* Label */}
              <div>
                <label htmlFor="hem-label" className={labelClass}>
                  Nome
                  <span className="ml-1 text-text-muted font-normal">(opcional)</span>
                </label>
                <input
                  ref={firstInputRef}
                  id="hem-label"
                  data-testid="host-modal-label"
                  type="text"
                  value={form.label}
                  onChange={(e) => setField("label", e.target.value)}
                  placeholder="Ex.: Site do cliente"
                  disabled={isBusy}
                  className={inputClass}
                />
              </div>

              {/* Host + Port row */}
              <div className="flex gap-3">
                <div className="flex-1">
                  <label htmlFor="hem-host" className={labelClass}>
                    Endereço <RequiredMark />
                  </label>
                  <input
                    id="hem-host"
                    data-testid="host-modal-host"
                    type="text"
                    value={form.host}
                    onChange={(e) => setField("host", e.target.value)}
                    placeholder="ftp.seudominio.com.br ou 192.168.1.1"
                    disabled={isBusy}
                    className={`${inputClass} font-mono`}
                  />
                </div>
                <div className="w-20">
                  <label htmlFor="hem-port" className={labelClass}>
                    Porta <RequiredMark />
                  </label>
                  <input
                    id="hem-port"
                    data-testid="host-modal-port"
                    type="number"
                    min={1}
                    max={65535}
                    value={form.port}
                    onChange={(e) => setField("port", e.target.value)}
                    disabled={isBusy}
                    className={`${inputClass} font-mono`}
                  />
                </div>
              </div>

              {/* Username */}
              <div>
                <label htmlFor="hem-username" className={labelClass}>
                  Usuário <RequiredMark />
                </label>
                <input
                  id="hem-username"
                  data-testid="host-modal-username"
                  type="text"
                  value={form.username}
                  onChange={(e) => setField("username", e.target.value)}
                  placeholder="root"
                  disabled={isBusy}
                  className={`${inputClass} font-mono`}
                />
              </div>

              {/* Auth Type + Group row */}
              <div className="flex gap-3">
                {!isFtp && (
                <div className="flex-1">
                  <label htmlFor="hem-auth" className={labelClass}>
                    Autenticação
                  </label>
                  <CustomSelect
                    id="hem-auth"
                    data-testid="host-modal-auth"
                    value={form.authType}
                    onChange={(v) => setField("authType", v as AuthType)}
                    disabled={isBusy}
                    options={[
                      { value: "password", label: "Senha" },
                      { value: "privateKey", label: "Chave privada" },
                    ]}
                  />
                </div>
                )}

                <div className="flex-1">
                  <label htmlFor="hem-group" className={labelClass}>
                    Grupo
                  </label>
                  <GroupSelect
                    id="hem-group"
                    value={form.groupId}
                    onChange={(val) => setField("groupId", val)}
                    groups={groups}
                    disabled={isBusy}
                    inputClass={inputClass}
                  />
                </div>
              </div>

              {/* Auth credentials — conditional on auth type */}
              {form.authType === "password" ? (
                <div>
                  <label htmlFor="hem-password" className={labelClass}>
                    Senha
                  </label>
                  <input
                    id="hem-password"
                    data-testid="host-modal-password"
                    type="password"
                    value={form.password}
                    onChange={(e) => setField("password", e.target.value)}
                    placeholder={
                      hasSavedCred && !credCleared && !form.password
                        ? "\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022"
                        : "Digite a senha"
                    }
                    disabled={isBusy}
                    className={inputClass}
                  />
                  <CredentialStatus
                    visible={hasSavedCred && !credCleared && !form.password}
                    busy={isBusy}
                    onClear={() => setCredCleared(true)}
                  />
                </div>
              ) : (
                <>
                  <div>
                    <label htmlFor="hem-keypath" className={labelClass}>
                      Chave SSH
                    </label>
                    <div className="flex gap-2">
                      <div className="flex-1 min-w-0">
                        {sshKeys.length > 0 ? (
                          <CustomSelect
                            id="hem-keypath"
                            data-testid="host-modal-keypath-select"
                            value={form.keyPath}
                            onChange={(v) => setField("keyPath", v)}
                            disabled={isBusy}
                            placeholder="Escolha uma chave…"
                            options={sshKeys.map((key) => ({
                              value: key.path,
                              label: `${key.name} (${key.algorithm})`,
                            }))}
                          />
                        ) : (
                          <input
                            id="hem-keypath"
                            data-testid="host-modal-keypath"
                            type="text"
                            value={form.keyPath}
                            onChange={(e) => setField("keyPath", e.target.value)}
                            placeholder="~/.ssh/id_ed25519"
                            disabled={isBusy}
                            className={`${inputClass} font-mono`}
                          />
                        )}
                      </div>
                      <button
                        type="button"
                        disabled={isBusy}
                        onClick={() => {
                          void (async () => {
                            try {
                              const { open } = await import("@tauri-apps/plugin-dialog");
                              const { invoke } = await import("@tauri-apps/api/core");
                              const path = await open({
                                title: "Escolha a chave privada SSH",
                                multiple: false,
                              });
                              if (path && typeof path === "string") {
                                // Validate and inspect the key
                                try {
                                  const keyInfo = await invoke<import("../../types").SshKeyInfo>("inspect_ssh_key", { path });
                                  setField("keyPath", keyInfo.path);
                                  if (!sshKeys.some((k) => k.path === keyInfo.path)) {
                                    setSshKeys((prev) => [...prev, keyInfo]);
                                  }
                                } catch (err) {
                                  const msg = err && typeof err === "object" && "message" in err
                                    ? String((err as { message: string }).message)
                                    : "Arquivo de chave inválido";
                                  setError(msg);
                                }
                              }
                            } catch {
                              // Dialog cancelled or unavailable
                            }
                          })();
                        }}
                        className={[
                          "px-3 py-2 rounded-lg text-[length:var(--text-sm)] font-medium shrink-0",
                          "bg-bg-base border border-border text-text-secondary",
                          "hover:border-border-focus hover:text-text-primary hover:bg-bg-overlay",
                          "transition-all duration-[var(--duration-fast)]",
                          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                          "disabled:opacity-50",
                        ].join(" ")}
                      >
                        Escolher
                      </button>
                    </div>
                    {form.keyPath && !sshKeys.some((k) => k.path === form.keyPath) && (
                      <p className="text-[length:var(--text-2xs)] font-mono text-text-muted mt-1 truncate" title={form.keyPath}>
                        {form.keyPath}
                      </p>
                    )}
                  </div>
                  <div>
                    <label htmlFor="hem-passphrase" className={labelClass}>
                      Frase secreta
                      <span className="ml-1 text-text-muted font-normal">(optional)</span>
                    </label>
                    <input
                      id="hem-passphrase"
                      type="password"
                      value={form.passphrase}
                      onChange={(e) => setField("passphrase", e.target.value)}
                      placeholder={
                        hasSavedCred && !credCleared && !form.passphrase
                          ? "\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022"
                          : "Deixe vazio se não tiver"
                      }
                      disabled={isBusy}
                      className={inputClass}
                    />
                    <CredentialStatus
                      visible={hasSavedCred && !credCleared && !form.passphrase}
                      busy={isBusy}
                      onClear={() => setCredCleared(true)}
                    />
                  </div>
                </>
              )}

              {isFtp && (
                <div className="flex flex-col gap-2">
                  <label className="flex items-center gap-2.5 select-none cursor-pointer">
                    <input
                      type="checkbox"
                      data-testid="host-modal-ftp-passive"
                      checked={form.ftpPassive}
                      disabled={isBusy}
                      onChange={(e) => setField("ftpPassive", e.target.checked)}
                      className="h-4 w-4 rounded border-border bg-bg-base text-accent accent-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 cursor-pointer"
                    />
                    <span className="text-[length:var(--text-sm)] text-text-primary">
                      Modo passivo
                      <span className="ml-1 text-text-muted">(recomendado)</span>
                    </span>
                  </label>
                  {form.ftpSecurity !== "none" && (
                    <label className="flex items-center gap-2.5 select-none cursor-pointer">
                      <input
                        type="checkbox"
                        data-testid="host-modal-ftp-invalid-cert"
                        checked={form.ftpAllowInvalidCert}
                        disabled={isBusy}
                        onChange={(e) => setField("ftpAllowInvalidCert", e.target.checked)}
                        className="h-4 w-4 rounded border-border bg-bg-base text-accent accent-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 cursor-pointer"
                      />
                      <span className="text-[length:var(--text-sm)] text-text-primary">
                        Aceitar certificado da hospedagem
                        <span className="ml-1 text-text-muted">
                          (hospedagens compartilhadas costumam usar o certificado do provedor)
                        </span>
                      </span>
                    </label>
                  )}
                </div>
              )}

              {!isFtp && (<>
              {/* ════════════════ TUNNEL ════════════════ */}
              <SectionHeader>Túnel</SectionHeader>

              <TunnelSection
                enabled={tunnelEnabled}
                onToggle={(on) => {
                  setError(null);
                  setTunnelEnabled(on);
                  if (!on) setField("proxyJumpHostId", "");
                }}
                value={form.proxyJumpHostId}
                onChange={(v) => setField("proxyJumpHostId", v)}
                hosts={tunnelCandidates}
                currentHostId={originalHost?.id ?? null}
                disabled={isBusy}
                labelClass={labelClass}
              />

              {/* Keep Alive + Default Shell row */}
              <div className="flex gap-3">
                <div className="flex-1">
                  <label htmlFor="hem-keepalive" className={labelClass}>
                    Keep-alive (s)
                    <span className="ml-1 text-text-muted font-normal">(seconds)</span>
                  </label>
                  <input
                    id="hem-keepalive"
                    type="number"
                    min={0}
                    value={form.keepAliveInterval}
                    onChange={(e) => setField("keepAliveInterval", e.target.value)}
                    placeholder="60"
                    disabled={isBusy}
                    className={`${inputClass} font-mono`}
                  />
                </div>
                <div className="flex-1">
                  <label htmlFor="hem-shell" className={labelClass}>
                    Shell padrão
                  </label>
                  <input
                    id="hem-shell"
                    type="text"
                    value={form.defaultShell}
                    onChange={(e) => setField("defaultShell", e.target.value)}
                    placeholder="/bin/zsh"
                    disabled={isBusy}
                    className={`${inputClass} font-mono`}
                  />
                </div>
              </div>

              {/* Startup Command */}
              <div>
                <label htmlFor="hem-startup" className={labelClass}>
                  Comando ao conectar
                  <span className="ml-1 text-text-muted font-normal">(optional)</span>
                </label>
                <input
                  id="hem-startup"
                  data-testid="host-modal-startup-command"
                  type="text"
                  value={form.startupCommand}
                  onChange={(e) => setField("startupCommand", e.target.value)}
                  placeholder="cd /app && tail -f logs"
                  disabled={isBusy}
                  className={`${inputClass} font-mono`}
                />
                {/* TODO: startup_command execution should be handled in the Rust backend
                    after the shell prompt is detected — not sent as raw input from the frontend. */}
              </div>
              </>)}

              {/* Start Directory */}
              <div>
                <label htmlFor="hem-start-dir" className={labelClass}>
                  Pasta inicial
                  <span className="ml-1 text-text-muted font-normal">(optional)</span>
                </label>
                <input
                  id="hem-start-dir"
                  data-testid="host-modal-start-directory"
                  type="text"
                  value={form.startDirectory}
                  onChange={(e) => setField("startDirectory", e.target.value)}
                  placeholder={isFtp ? "/public_html" : "~/projects or /var/www"}
                  disabled={isBusy}
                  className={`${inputClass} font-mono`}
                />
                <p className="mt-1 text-[length:var(--text-xs)] text-text-muted">
                  {isFtp
                    ? "Pasta em que os arquivos abrem (caminho absoluto). Se vazio, abre na pasta do login."
                    : "Pasta em que os arquivos abrem. Se vazio, abre na pasta do usuário."}
                </p>
              </div>

              {/* ════════════════ APPEARANCE ════════════════ */}
              <SectionHeader>Aparência</SectionHeader>

              {/* Color swatches */}
              <div>
                <span className={labelClass}>Cor</span>
                <div className="flex items-center gap-2 flex-wrap">
                  {/* Auto option — clears custom color */}
                  <button
                    type="button"
                    onClick={() => setField("color", "")}
                    disabled={isBusy}
                    title="Automática"
                    aria-label="Cor automática"
                    className={[
                      "w-6 h-6 rounded-full border-2 text-[11px] font-bold",
                      "flex items-center justify-center",
                      "transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
                      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      form.color === ""
                        ? "border-border-focus ring-2 ring-ring"
                        : "border-border hover:border-border-focus",
                    ].join(" ")}
                    style={{ background: "conic-gradient(#ef4444, #f97316, #eab308, #22c55e, #06b6d4, #8b5cf6, #ef4444)" }}
                  >
                    <span className="sr-only">Auto</span>
                  </button>

                  {HOST_COLORS.map((c) => (
                    <button
                      key={c}
                      type="button"
                      onClick={() => setField("color", c)}
                      disabled={isBusy}
                      title={c}
                      aria-label={`Cor ${c}`}
                      aria-pressed={form.color === c}
                      className={[
                        "w-6 h-6 rounded-full border-2",
                        "transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-bg-overlay",
                        form.color === c
                          ? "border-white ring-2 ring-ring scale-110"
                          : "border-transparent hover:border-white/60 hover:scale-105",
                      ].join(" ")}
                      style={{ backgroundColor: c }}
                    />
                  ))}
                </div>
              </div>

              {/* Environment + OS Type row */}
              <div className="flex gap-3">
                <div className="flex-1">
                  <label htmlFor="hem-env" className={labelClass}>
                    Ambiente
                  </label>
                  <CustomSelect
                    id="hem-env"
                    value={form.environment}
                    onChange={(v) => setField("environment", v)}
                    disabled={isBusy}
                    placeholder="Nenhum"
                    options={[
                      { value: "", label: "Nenhum" },
                      { value: "production", label: "Produção" },
                      { value: "staging", label: "Homologação" },
                      { value: "dev", label: "Desenvolvimento" },
                      { value: "testing", label: "Testes" },
                    ]}
                  />
                </div>

                {!isFtp && (
                <div className="flex-1">
                  <label htmlFor="hem-os" className={labelClass}>
                    Sistema
                  </label>
                  <CustomSelect
                    id="hem-os"
                    value={form.osType}
                    onChange={(v) => setField("osType", v)}
                    disabled={isBusy}
                    placeholder="Automático"
                    options={[
                      { value: "", label: "Automático" },
                      { value: "linux", label: "Linux" },
                      { value: "macos", label: "macOS" },
                      { value: "windows", label: "Windows" },
                      { value: "freebsd", label: "FreeBSD" },
                    ]}
                  />
                </div>
                )}
              </div>

              {/* ════════════════ NOTES ════════════════ */}
              <SectionHeader>Observações</SectionHeader>

              <div>
                <label htmlFor="hem-notes" className={labelClass}>
                  Observações
                  <span className="ml-1 text-text-muted font-normal">(optional)</span>
                </label>
                <textarea
                  id="hem-notes"
                  rows={3}
                  value={form.notes}
                  onChange={(e) => setField("notes", e.target.value)}
                  placeholder="Anotações sobre este servidor…"
                  disabled={isBusy}
                  className={`${inputClass} resize-none`}
                />
              </div>

              {/* Error banner */}
              {error && (
                <p
                  role="alert"
                  data-testid="host-modal-error"
                  className="text-[length:var(--text-sm)] text-status-error bg-status-error/10 rounded-lg px-3 py-2"
                >
                  {error}
                </p>
              )}
            </div>
          )}
        </div>
    </ModalShell>
  );
}


// ─── Sub-components ────────────────────────────────────────────────────────────

function RequiredMark() {
  return (
    <span className="ml-0.5 text-status-error" aria-hidden="true">
      *
    </span>
  );
}

interface GroupSelectProps {
  id: string;
  value: string;
  onChange: (val: string) => void;
  groups: { id: string; name: string; color: string }[];
  disabled: boolean;
  inputClass: string;
}

function GroupSelect({ id, value, onChange, groups, disabled }: GroupSelectProps) {
  return (
    <CustomSelect
      id={id}
      value={value}
      onChange={onChange}
      disabled={disabled}
      placeholder="Sem grupo"
      data-testid="host-modal-group"
      options={[
        { value: "", label: "Sem grupo" },
        ...groups.map((g) => ({
          value: g.id,
          label: g.name,
        })),
      ]}
    />
  );
}

// ─── TunnelSection ────────────────────────────────────────────────────────────

interface TunnelSectionProps {
  enabled: boolean;
  onToggle: (on: boolean) => void;
  value: string;
  onChange: (val: string) => void;
  hosts: SavedHost[];
  /** Id of the host being edited — excluded from the dropdown. Null for new. */
  currentHostId: string | null;
  disabled: boolean;
  labelClass: string;
}

/**
 * "Connect through SSH tunnel" toggle plus a dropdown of saved hosts to use as
 * the ProxyJump / bastion host. The currently-edited host is excluded so a host
 * can't tunnel through itself.
 */
function TunnelSection({
  enabled,
  onToggle,
  value,
  onChange,
  hosts,
  currentHostId,
  disabled,
  labelClass,
}: TunnelSectionProps) {
  const candidates = hosts.filter((h) => h.id !== currentHostId);
  const hasCandidates = candidates.length > 0;
  // A selected value that isn't among the candidates is stale (its host was
  // deleted, or it's a corrupt self-reference that got excluded).
  const selectedIsStale = !!value && !candidates.some((h) => h.id === value);

  const options = candidates.map((h) => ({
    value: h.id,
    label: h.label
      ? `${h.label} (${h.host}:${h.port})`
      : `${h.host}:${h.port}`,
  }));

  return (
    <div className="flex flex-col gap-2.5">
      {/* Checkbox row. Disabled when there's nothing to tunnel through (and not
          already enabled) so the user can't enter a dead-end required-field state. */}
      <label
        className={`flex items-center gap-2.5 select-none ${
          disabled || (!enabled && !hasCandidates) ? "cursor-not-allowed" : "cursor-pointer"
        }`}
      >
        <input
          type="checkbox"
          data-testid="host-modal-tunnel-enabled"
          checked={enabled}
          disabled={disabled || (!enabled && !hasCandidates)}
          onChange={(e) => onToggle(e.target.checked)}
          className="h-4 w-4 rounded border-border bg-bg-base text-accent accent-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 cursor-pointer"
        />
        <span className="text-[length:var(--text-sm)] text-text-primary">
          Conectar por um túnel SSH
        </span>
      </label>

      {/* Hint when the toggle is unavailable because no other hosts exist yet. */}
      {!enabled && !hasCandidates && (
        <p className="text-[length:var(--text-xs)] text-text-muted">
          Salve outro servidor antes para usar como túnel.
        </p>
      )}

      {/* Tunnel host dropdown — only when enabled */}
      {enabled && (
        <div>
          <label htmlFor="hem-tunnel-host" className={labelClass}>
            Tunnel Host <span className="ml-0.5 text-status-error" aria-hidden="true">*</span>
          </label>
          {hasCandidates ? (
            <>
              <CustomSelect
                id="hem-tunnel-host"
                data-testid="host-modal-tunnel-host"
                value={selectedIsStale ? "" : value}
                onChange={onChange}
                disabled={disabled}
                placeholder="Escolha um servidor…"
                options={options}
              />
              {selectedIsStale && (
                <p className="mt-1 text-[length:var(--text-xs)] text-status-error">
                  O servidor de túnel escolhido antes não existe mais.
                  Escolha outro ou desative o túnel.
                </p>
              )}
            </>
          ) : (
            <p className="text-[length:var(--text-xs)] text-text-muted px-3 py-2 rounded-lg bg-bg-base border border-border">
              Não há outro servidor salvo para usar como túnel. Salve outro
              servidor antes.
            </p>
          )}
        </div>
      )}

      {/* Divider separating tunnel config from the fields below */}
      <div className="flex items-center gap-3 my-1">
        <span className="text-[length:var(--text-xs)] font-semibold uppercase tracking-widest text-text-muted whitespace-nowrap">
          Avançado
        </span>
        <div className="flex-1 h-px bg-border" aria-hidden="true" />
      </div>
    </div>
  );
}

// ─── CredentialStatus ─────────────────────────────────────────────────────────

interface CredentialStatusProps {
  /** Whether to show the badge at all. */
  visible: boolean;
  busy: boolean;
  onClear: () => void;
}

/**
 * Shown below a password/passphrase field when a credential is already
 * saved in the OS keychain.  The actual secret is never sent to the frontend —
 * only the boolean "exists" flag comes from Rust.
 */
function CredentialStatus({ visible, busy, onClear }: CredentialStatusProps) {
  if (!visible) return null;
  return (
    <div className="flex items-center justify-between mt-1.5 px-2.5 py-1.5 rounded-md bg-bg-subtle border border-border">
      <div className="flex items-center gap-1.5 text-[length:var(--text-xs)] text-text-secondary">
        {/* Lock icon — inline SVG to avoid adding another icon import */}
        <svg
          width="11"
          height="11"
          viewBox="0 0 12 14"
          fill="none"
          aria-hidden="true"
          className="text-text-muted shrink-0"
        >
          <rect
            x="1"
            y="6"
            width="10"
            height="7"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.3"
          />
          <path
            d="M4 6V4a2 2 0 1 1 4 0v2"
            stroke="currentColor"
            strokeWidth="1.3"
            strokeLinecap="round"
          />
        </svg>
        Senha salva no cofre do sistema
      </div>
      <button
        type="button"
        onClick={onClear}
        disabled={busy}
        aria-label="Apagar senha salva"
        className={[
          "text-[length:var(--text-xs)] text-text-muted hover:text-status-error",
          "transition-colors duration-[var(--duration-fast)]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded px-1",
        ].join(" ")}
      >
        Apagar
      </button>
    </div>
  );
}

// ─── DeleteConfirmRow ─────────────────────────────────────────────────────────

interface DeleteConfirmRowProps {
  onCancel: () => void;
  onConfirm: () => void;
  busy: boolean;
}

function DeleteConfirmRow({ onCancel, onConfirm, busy }: DeleteConfirmRowProps) {
  return (
    <div className="flex items-center gap-2 animate-in fade-in slide-in-from-left-1 duration-[var(--duration-fast)]">
      <span className="text-[length:var(--text-xs)] text-text-secondary whitespace-nowrap">
        Excluir este servidor?
      </span>
      <button
        type="button"
        onClick={onCancel}
        disabled={busy}
        className="px-3 py-1.5 text-[length:var(--text-xs)] text-text-secondary hover:text-text-primary rounded-md transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        Cancelar
      </button>
      <button
        type="button"
        data-testid="host-modal-delete-confirm"
        onClick={onConfirm}
        disabled={busy}
        autoFocus
        className="px-3 py-1.5 text-[length:var(--text-xs)] font-medium text-text-inverse bg-status-error hover:opacity-90 disabled:opacity-50 rounded-md transition-[opacity] duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {busy ? "Excluindo\u2026" : "Excluir"}
      </button>
    </div>
  );
}

function LoadingSkeleton() {
  const skeletonClass = "rounded-md bg-bg-subtle animate-pulse";

  return (
    <div className="flex flex-col gap-3.5" aria-label="Loading host data">
      {/* Section header skeleton */}
      <div className={`h-3 w-24 ${skeletonClass}`} />
      {/* Host + port row */}
      <div className="flex gap-3">
        <div className="flex-1">
          <div className={`h-3 w-8 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
        <div className="w-20">
          <div className={`h-3 w-6 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
      </div>
      {/* Username row */}
      <div>
        <div className={`h-3 w-14 mb-2 ${skeletonClass}`} />
        <div className={`h-9 w-full ${skeletonClass}`} />
      </div>
      {/* Auth + group row */}
      <div className="flex gap-3">
        <div className="flex-1">
          <div className={`h-3 w-14 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
        <div className="flex-1">
          <div className={`h-3 w-10 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
      </div>
      {/* Two more field rows */}
      <div>
        <div className={`h-3 w-20 mb-2 ${skeletonClass}`} />
        <div className={`h-9 w-full ${skeletonClass}`} />
      </div>
      <div className="flex gap-3">
        <div className="flex-1">
          <div className={`h-3 w-16 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
        <div className="flex-1">
          <div className={`h-3 w-16 mb-2 ${skeletonClass}`} />
          <div className={`h-9 w-full ${skeletonClass}`} />
        </div>
      </div>
    </div>
  );
}
