import { useState, useEffect, useCallback, useRef } from "react";
import { ModalShell, BTN_GHOST, BTN_PRIMARY, BTN_DANGER } from "../shared/ModalShell";
import { ModalBackdrop } from "../shared/ModalBackdrop";
import { useSettingsStore } from "../../stores/settings-store";
import { CustomSelect, type SelectOption } from "../shared/CustomSelect";
import { toast } from "../../stores/toast-store";
import { RefreshCw, AlertCircle, Palette, SquareTerminal, ArrowUpDown, Info, ExternalLink, Check, FileCode, Plus, Trash2, FolderOpen, Star, Search, Database, Download, Upload, ShieldCheck, Sparkles, Copy } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { TalktoallWordmark } from "../brand";
import type { CursorStyle, ThemeMode, EditorConfig, PasteButton, DoubleClickAction } from "../../stores/settings-store";

// ─── Shared styles ───────────────────────────────────────────────────────────

const LABEL_CLASS = "text-[length:var(--text-sm)] font-medium text-text-primary";
const DESC_CLASS = "text-[length:var(--text-xs)] text-text-muted mt-0.5";

const INPUT_CLASS = [
  "w-20 px-2.5 py-1.5 rounded-lg text-[length:var(--text-sm)] tabular-nums",
  "bg-bg-base border border-border text-text-primary",
  "outline-none focus:border-border-focus focus:ring-2 focus:ring-ring",
  "transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
].join(" ");

const BTN_SECONDARY = [
  "flex items-center gap-1.5 h-9 px-4 rounded-full shrink-0",
  "text-[length:var(--text-sm)] font-bold",
  "bg-white/[0.08] text-text-primary",
  "hover:bg-white/[0.13]",
  "disabled:opacity-50 disabled:pointer-events-none",
  "transition-all duration-[var(--duration-fast)]",
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
].join(" ");

// Mirrors the input/label styling used by the Host modal so dialogs feel uniform.
const TEXT_INPUT_CLASS = [
  "w-full px-3 py-2 rounded-lg text-[length:var(--text-sm)]",
  "bg-bg-base border border-border text-text-primary placeholder:text-text-muted",
  "outline-none focus:border-border-focus focus:ring-2 focus:ring-ring",
  "transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
].join(" ");

const FIELD_LABEL_CLASS = "block text-[length:var(--text-xs)] font-medium text-text-secondary mb-1";

const REPO_URL = "https://github.com/luan0308/talktoall-files";
const UPSTREAM_URL = "https://github.com/macnev2013/anySCP";

// ─── Sections ─────────────────────────────────────────────────────────────────
// Each settings category is a section here. To add a new category, add an entry
// to SECTIONS, a description, and render its content in <SectionContent />.

type SectionId = "appearance" | "terminal" | "explorer" | "transfers" | "editors" | "claude" | "data" | "about";

const SECTIONS: { id: SectionId; label: string; icon: LucideIcon }[] = [
  { id: "appearance", label: "Aparência", icon: Palette },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
  { id: "explorer", label: "Arquivos", icon: FolderOpen },
  { id: "transfers", label: "Transferências", icon: ArrowUpDown },
  { id: "editors", label: "Editores", icon: FileCode },
  { id: "claude", label: "Claude e Codex", icon: Sparkles },
  { id: "data", label: "Dados", icon: Database },
  { id: "about", label: "Sobre e atualizações", icon: Info },
];

const SECTION_DESCRIPTIONS: Record<SectionId, string> = {
  appearance: "Tema e visual da interface.",
  terminal: "Fonte, cursor e histórico de rolagem.",
  explorer: "Como a tela de Arquivos se comporta.",
  transfers: "Controle como os arquivos são transferidos.",
  editors: "Editores usados em “Editar” e “Abrir com” na tela de Arquivos.",
  claude: "O Claude e o Codex usam os servidores salvos aqui. As senhas ficam no cofre do sistema e nunca chegam até eles.",
  data: "Faça backup, restaure e apague seus dados.",
  about: "Informações do app, links e atualizações.",
};

// Remember the last-open section across tab switches. The settings page
// unmounts when another tab is active, so component state alone would reset.
let lastSettingsSection: SectionId = "appearance";

/** Open Settings on a given section next time it mounts (e.g. from the tab bar). */
export function openSettingsSection(id: SectionId) {
  lastSettingsSection = id;
}

// ─── Component ───────────────────────────────────────────────────────────────

export function SettingsPage() {
  const [active, setActive] = useState<SectionId>(() => lastSettingsSection);
  const selectSection = (id: SectionId) => { lastSettingsSection = id; setActive(id); };
  const activeSection = SECTIONS.find((s) => s.id === active);

  return (
    <div className="flex flex-col h-full p-2">
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Sidebar */}
        <nav
          aria-label="Seções das configurações"
          className="w-60 shrink-0 flex flex-col gap-1 px-3 py-4 overflow-y-auto no-select"
        >
          <h2 className="px-3 pt-1 pb-2 text-[length:var(--text-2xs)] font-semibold uppercase tracking-wider text-text-muted">
            Configurações
          </h2>
          {SECTIONS.map(({ id, label, icon: Icon }) => {
            const isActive = active === id;
            return (
              <button
                key={id}
                type="button"
                data-testid={`settings-nav-${id}`}
                aria-current={isActive ? "page" : undefined}
                onClick={() => selectSection(id)}
                className={[
                  "flex items-center gap-3 px-3.5 h-10 rounded-[12px] text-left",
                  "text-[length:var(--text-sm)] font-medium",
                  "transition-colors duration-[var(--duration-fast)]",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  isActive
                    ? "bg-white/[0.08] text-text-primary"
                    : "text-text-secondary hover:text-text-primary hover:bg-white/[0.05]",
                ].join(" ")}
              >
                <Icon
                  size={17}
                  strokeWidth={1.75}
                  className={`shrink-0 ${isActive ? "text-text-primary" : "text-text-muted"}`}
                />
                {label}
              </button>
            );
          })}
        </nav>

        {/* Content */}
        <div className="flex-1 overflow-y-scroll">
          <div className="max-w-4xl mx-auto px-8 py-6">
            {/* Section header */}
            <div className="mb-6">
              <h1 className="text-[28px] font-bold tracking-tight text-text-primary">
                {activeSection?.label}
              </h1>
              <p className="text-[length:var(--text-sm)] text-text-muted mt-1.5">
                {SECTION_DESCRIPTIONS[active]}
              </p>
            </div>

            <SectionContent section={active} />
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Section content ───────────────────────────────────────────────────────────

function SectionContent({ section }: { section: SectionId }) {
  switch (section) {
    case "appearance":
      return <AppearanceSettings />;
    case "terminal":
      return <TerminalSettings />;
    case "explorer":
      return <ExplorerSettings />;
    case "transfers":
      return <TransferSettings />;
    case "editors":
      return <EditorsSettings />;
    case "claude":
      return <ClaudeSettings />;
    case "data":
      return <DataSettings />;
    case "about":
      return <AboutSettings />;
  }
}

// Candidates for the interface font. Entries without a `family` are always
// offered (Geist is bundled; System UI is a generic). Entries with a `family`
// are only shown when that font is actually installed (see availableFonts),
// since an unavailable font silently falls back to the system default.
const INTERFACE_FONT_CANDIDATES: { value: string; label: string; family?: string }[] = [
  { value: "'Rethink Sans', system-ui, sans-serif", label: "Rethink Sans (padrão)" },
  { value: "'Geist', system-ui, sans-serif", label: "Geist" },
  { value: "system-ui, sans-serif", label: "Fonte do sistema" },
  { value: "'Arial', system-ui, sans-serif", label: "Arial", family: "Arial" },
  { value: "'Avenir', system-ui, sans-serif", label: "Avenir", family: "Avenir" },
  { value: "'Avenir Next', system-ui, sans-serif", label: "Avenir Next", family: "Avenir Next" },
  { value: "'Calibri', system-ui, sans-serif", label: "Calibri", family: "Calibri" },
  { value: "'Cantarell', system-ui, sans-serif", label: "Cantarell", family: "Cantarell" },
  { value: "'DejaVu Sans', system-ui, sans-serif", label: "DejaVu Sans", family: "DejaVu Sans" },
  { value: "'Fira Sans', system-ui, sans-serif", label: "Fira Sans", family: "Fira Sans" },
  { value: "'FreeSans', system-ui, sans-serif", label: "FreeSans", family: "FreeSans" },
  { value: "'Helvetica', system-ui, sans-serif", label: "Helvetica", family: "Helvetica" },
  { value: "'Helvetica Neue', system-ui, sans-serif", label: "Helvetica Neue", family: "Helvetica Neue" },
  { value: "'Inter', system-ui, sans-serif", label: "Inter", family: "Inter" },
  { value: "'Lato', system-ui, sans-serif", label: "Lato", family: "Lato" },
  { value: "'Liberation Sans', system-ui, sans-serif", label: "Liberation Sans", family: "Liberation Sans" },
  { value: "'Lucida Grande', system-ui, sans-serif", label: "Lucida Grande", family: "Lucida Grande" },
  { value: "'Nimbus Sans', system-ui, sans-serif", label: "Nimbus Sans", family: "Nimbus Sans" },
  { value: "'Noto Sans', system-ui, sans-serif", label: "Noto Sans", family: "Noto Sans" },
  { value: "'Open Sans', system-ui, sans-serif", label: "Open Sans", family: "Open Sans" },
  { value: "'Roboto', system-ui, sans-serif", label: "Roboto", family: "Roboto" },
  { value: "'Segoe UI', system-ui, sans-serif", label: "Segoe UI", family: "Segoe UI" },
  { value: "'Source Sans 3', system-ui, sans-serif", label: "Source Sans 3", family: "Source Sans 3" },
  { value: "'Source Sans Pro', system-ui, sans-serif", label: "Source Sans Pro", family: "Source Sans Pro" },
  { value: "'Tahoma', system-ui, sans-serif", label: "Tahoma", family: "Tahoma" },
  { value: "'Trebuchet MS', system-ui, sans-serif", label: "Trebuchet MS", family: "Trebuchet MS" },
  { value: "'Ubuntu', system-ui, sans-serif", label: "Ubuntu", family: "Ubuntu" },
  { value: "'Verdana', system-ui, sans-serif", label: "Verdana", family: "Verdana" },
  { value: "'Work Sans', system-ui, sans-serif", label: "Work Sans", family: "Work Sans" },
];

// Monospace candidates for the terminal. The default matches the store's
// terminalFontFamily so it selects correctly; JetBrains Mono is bundled.
const TERMINAL_FONT_CANDIDATES: FontCandidate[] = [
  { value: "'JetBrains Mono', 'Fira Code', 'SF Mono', Menlo, monospace", label: "JetBrains Mono (padrão)" },
  { value: "'JetBrainsMono Nerd Font', 'JetBrains Mono', monospace", label: "JetBrains Nerd Font (ícones)" },
  { value: "monospace", label: "Monoespaçada do sistema" },
  { value: "'Cascadia Code', monospace", label: "Cascadia Code", family: "Cascadia Code" },
  { value: "'Cascadia Mono', monospace", label: "Cascadia Mono", family: "Cascadia Mono" },
  { value: "'Consolas', monospace", label: "Consolas", family: "Consolas" },
  { value: "'Courier New', monospace", label: "Courier New", family: "Courier New" },
  { value: "'DejaVu Sans Mono', monospace", label: "DejaVu Sans Mono", family: "DejaVu Sans Mono" },
  { value: "'Fira Code', monospace", label: "Fira Code", family: "Fira Code" },
  { value: "'Fira Mono', monospace", label: "Fira Mono", family: "Fira Mono" },
  { value: "'Hack', monospace", label: "Hack", family: "Hack" },
  { value: "'IBM Plex Mono', monospace", label: "IBM Plex Mono", family: "IBM Plex Mono" },
  { value: "'Inconsolata', monospace", label: "Inconsolata", family: "Inconsolata" },
  { value: "'Liberation Mono', monospace", label: "Liberation Mono", family: "Liberation Mono" },
  { value: "'Menlo', monospace", label: "Menlo", family: "Menlo" },
  { value: "'Monaco', monospace", label: "Monaco", family: "Monaco" },
  { value: "'Noto Sans Mono', monospace", label: "Noto Sans Mono", family: "Noto Sans Mono" },
  { value: "'Roboto Mono', monospace", label: "Roboto Mono", family: "Roboto Mono" },
  { value: "'SF Mono', monospace", label: "SF Mono", family: "SF Mono" },
  { value: "'Source Code Pro', monospace", label: "Source Code Pro", family: "Source Code Pro" },
  { value: "'Ubuntu Mono', monospace", label: "Ubuntu Mono", family: "Ubuntu Mono" },
];

// Monospace candidates for the UI (`--font-mono`: permissions, paths, kbd,
// snippets, addresses). JetBrains Mono (the default) is bundled; "System UI"
// uses the OS UI monospace (`ui-monospace`). The rest are common system fonts,
// filtered to those actually installed.
const INTERFACE_MONO_FONT_CANDIDATES: FontCandidate[] = [
  { value: "'JetBrains Mono', 'Fira Code', ui-monospace, monospace", label: "JetBrains Mono (padrão)" },
  { value: "ui-monospace, monospace", label: "Monoespaçada do sistema" },
  { value: "'Cascadia Code', monospace", label: "Cascadia Code", family: "Cascadia Code" },
  { value: "'Cascadia Mono', monospace", label: "Cascadia Mono", family: "Cascadia Mono" },
  { value: "'Consolas', monospace", label: "Consolas", family: "Consolas" },
  { value: "'DejaVu Sans Mono', monospace", label: "DejaVu Sans Mono", family: "DejaVu Sans Mono" },
  { value: "'Fira Code', monospace", label: "Fira Code", family: "Fira Code" },
  { value: "'Hack', monospace", label: "Hack", family: "Hack" },
  { value: "'IBM Plex Mono', monospace", label: "IBM Plex Mono", family: "IBM Plex Mono" },
  { value: "'Menlo', monospace", label: "Menlo", family: "Menlo" },
  { value: "'Roboto Mono', monospace", label: "Roboto Mono", family: "Roboto Mono" },
  { value: "'SF Mono', monospace", label: "SF Mono", family: "SF Mono" },
  { value: "'Source Code Pro', monospace", label: "Source Code Pro", family: "Source Code Pro" },
  { value: "'Ubuntu Mono', monospace", label: "Ubuntu Mono", family: "Ubuntu Mono" },
];

/**
 * Whether a named font is actually installed. document.fonts.check() is
 * unreliable (it returns true for unknown names), so measure a test string:
 * if rendering with the font matches every generic fallback exactly, the font
 * isn't present and the browser fell back.
 */
function isFontAvailable(family: string): boolean {
  if (typeof document === "undefined") return false;
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return false;
  const sample = "mmmmmmmmmmlli MWQ 0123";
  const size = "72px";
  for (const base of ["monospace", "serif", "sans-serif"]) {
    ctx.font = `${size} ${base}`;
    const baseWidth = ctx.measureText(sample).width;
    ctx.font = `${size} "${family}", ${base}`;
    if (ctx.measureText(sample).width !== baseWidth) return true;
  }
  return false;
}

type FontCandidate = { value: string; label: string; family?: string };

/** Filter candidates down to those actually installed on this system. */
function filterInstalledFonts(candidates: FontCandidate[]): SelectOption[] {
  return candidates
    .filter((f) => !f.family || isFontAvailable(f.family))
    .map(({ value, label }) => ({ value, label }));
}

/** Font-picker options: installed candidates, re-checked once web fonts load,
 *  with the current value kept selectable even if it isn't detected. */
function useInstalledFontOptions(candidates: FontCandidate[], current: string): SelectOption[] {
  const [available, setAvailable] = useState<SelectOption[]>(() => filterInstalledFonts(candidates));
  useEffect(() => {
    let cancelled = false;
    document.fonts?.ready?.then(() => { if (!cancelled) setAvailable(filterInstalledFonts(candidates)); }).catch(() => {});
    return () => { cancelled = true; };
  }, [candidates]);
  if (available.some((o) => o.value === current)) return available;
  const cur = candidates.find((c) => c.value === current);
  return [{ value: current, label: cur?.label ?? "Atual" }, ...available];
}

const ACCENT_PRESETS: { name: string; hue: number }[] = [
  { name: "Verde talktoall", hue: 154 },
  { name: "Azul", hue: 250 },
  { name: "Índigo", hue: 277 },
  { name: "Violeta", hue: 300 },
  { name: "Rosa", hue: 350 },
  { name: "Vermelho", hue: 25 },
  { name: "Laranja", hue: 70 },
  { name: "Turquesa", hue: 195 },
];

function AppearanceSettings() {
  const themeMode = useSettingsStore((s) => s.themeMode);
  const setThemeMode = useSettingsStore((s) => s.setThemeMode);
  const accentHue = useSettingsStore((s) => s.accentHue);
  const setAccentHue = useSettingsStore((s) => s.setAccentHue);
  const accentCustom = useSettingsStore((s) => s.accentCustom);
  const setAccentCustom = useSettingsStore((s) => s.setAccentCustom);
  const interfaceFont = useSettingsStore((s) => s.interfaceFont);
  const setInterfaceFont = useSettingsStore((s) => s.setInterfaceFont);
  const interfaceMonoFont = useSettingsStore((s) => s.interfaceMonoFont);
  const setInterfaceMonoFont = useSettingsStore((s) => s.setInterfaceMonoFont);

  const fontOptions = useInstalledFontOptions(INTERFACE_FONT_CANDIDATES, interfaceFont);
  const monoFontOptions = useInstalledFontOptions(INTERFACE_MONO_FONT_CANDIDATES, interfaceMonoFont);

  const [wheelOpen, setWheelOpen] = useState(false);
  const customRef = useRef<HTMLDivElement>(null);
  const isCustom = accentCustom !== null;
  const working = accentCustom ?? { l: 0.888, c: 0.168, h: accentHue };
  const workingColor = `oklch(${working.l} ${working.c} ${working.h})`;
  const updateCustom = (patch: Partial<typeof working>) => setAccentCustom({ ...working, ...patch });

  // Close the wheel popover on outside click / Escape.
  useEffect(() => {
    if (!wheelOpen) return;
    const onDown = (e: PointerEvent) => {
      if (customRef.current && !customRef.current.contains(e.target as Node)) setWheelOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setWheelOpen(false); };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [wheelOpen]);

  return (
    <>
    <SettingsGroup label="Tema">
      <SettingRow>
        <div>
          <p className={LABEL_CLASS}>Tema de cores</p>
          <p className={DESC_CLASS}>Alterne entre a interface escura e a clara, em cinza suave</p>
        </div>
        <SegmentedControl<ThemeMode>
          id="s-light-theme"
          value={themeMode}
          onChange={setThemeMode}
          options={[
            { value: "dark", label: "Escuro" },
            { value: "light", label: "Claro" },
          ]}
        />
      </SettingRow>

      <SettingRow>
        <div>
          <p className={LABEL_CLASS}>Cor de destaque</p>
          <p className={DESC_CLASS}>Usada em botões, links e itens ativos</p>
        </div>
        <div className="flex items-center gap-2.5 shrink-0">
          {ACCENT_PRESETS.map((preset) => {
            const selected = !isCustom && accentHue === preset.hue;
            const color = `oklch(0.888 0.168 ${preset.hue})`;
            return (
              <button
                key={preset.hue}
                type="button"
                title={preset.name}
                aria-label={preset.name}
                aria-pressed={selected}
                data-testid={`s-accent-${preset.hue}`}
                onClick={() => setAccentHue(preset.hue)}
                className="relative flex items-center justify-center w-6 h-6 rounded-full shrink-0 transition-transform duration-[var(--duration-fast)] hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                style={{
                  backgroundColor: color,
                  boxShadow: selected ? `0 0 0 2px var(--color-bg-surface), 0 0 0 4px ${color}` : undefined,
                }}
              >
                {selected && <Check size={13} strokeWidth={3} className="text-text-inverse" />}
              </button>
            );
          })}

          {/* Custom — a rainbow swatch that opens the hue wheel */}
          <div className="relative" ref={customRef}>
            <button
              type="button"
              title="Personalizada"
              aria-label="Cor personalizada"
              aria-haspopup="dialog"
              aria-expanded={wheelOpen}
              aria-pressed={isCustom}
              data-testid="s-accent-custom"
              onClick={() => setWheelOpen((o) => !o)}
              className="relative flex items-center justify-center w-6 h-6 rounded-full shrink-0 transition-transform duration-[var(--duration-fast)] hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              style={{
                background: isCustom
                  ? workingColor
                  : "conic-gradient(oklch(0.70 0.15 0), oklch(0.70 0.15 60), oklch(0.70 0.15 120), oklch(0.70 0.15 180), oklch(0.70 0.15 240), oklch(0.70 0.15 300), oklch(0.70 0.15 360))",
                boxShadow: isCustom ? `0 0 0 2px var(--color-bg-surface), 0 0 0 4px ${workingColor}` : undefined,
              }}
            >
              {isCustom && <Check size={13} strokeWidth={3} className="text-white [filter:drop-shadow(0_1px_1px_rgb(0_0_0/0.5))]" />}
            </button>

            {wheelOpen && (
              <div
                role="dialog"
                aria-label="Cor de destaque personalizada"
                className="absolute right-0 top-full mt-2 z-50 flex flex-col items-center gap-2 p-3 rounded-xl bg-bg-overlay border border-border shadow-[var(--shadow-lg)]"
              >
                <HueWheel
                  hue={working.h}
                  l={working.l}
                  c={working.c}
                  onChange={(h) => updateCustom({ h })}
                  size={140}
                />
                <div className="w-full flex flex-col gap-2.5">
                  <label className="flex flex-col gap-1">
                    <span className="text-[length:var(--text-2xs)] uppercase tracking-wider text-text-muted">Luminosidade</span>
                    <input
                      type="range" min={0.45} max={0.85} step={0.01} value={working.l}
                      onChange={(e) => updateCustom({ l: Number(e.target.value) })}
                      className="w-full h-1.5 cursor-pointer"
                      style={{ accentColor: workingColor }}
                    />
                  </label>
                  <label className="flex flex-col gap-1">
                    <span className="text-[length:var(--text-2xs)] uppercase tracking-wider text-text-muted">Saturação</span>
                    <input
                      type="range" min={0} max={0.3} step={0.005} value={working.c}
                      onChange={(e) => updateCustom({ c: Number(e.target.value) })}
                      className="w-full h-1.5 cursor-pointer"
                      style={{ accentColor: workingColor }}
                    />
                  </label>
                </div>
              </div>
            )}
          </div>
        </div>
      </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Interface">
      <SettingRow>
        <div>
          <p className={LABEL_CLASS}>Fonte da interface</p>
          <p className={DESC_CLASS}>Fonte de menus, rótulos e painéis</p>
        </div>
        <CustomSelect
          id="s-interface-font"
          data-testid="s-interface-font"
          value={interfaceFont}
          onChange={setInterfaceFont}
          options={fontOptions}
          className="w-44"
          previewOptionFont
        />
      </SettingRow>
      <SettingRow>
        <div>
          <p className={LABEL_CLASS}>Fonte monoespaçada da interface</p>
          <p className={DESC_CLASS}>Fonte de caminhos, permissões e código (não vale para o terminal)</p>
        </div>
        <CustomSelect
          id="s-interface-mono-font"
          data-testid="s-interface-mono-font"
          value={interfaceMonoFont}
          onChange={setInterfaceMonoFont}
          options={monoFontOptions}
          className="w-44"
          previewOptionFont
        />
      </SettingRow>
    </SettingsGroup>
    </>
  );
}

/** Circular hue picker — click/drag around the ring to set the hue.
 *  Ring + thumb colours use the given lightness/chroma so the preview is honest
 *  (e.g. at zero chroma the ring turns gray). */
function HueWheel({ hue, onChange, size = 96, l = 0.70, c = 0.15 }: {
  hue: number; onChange: (h: number) => void; size?: number; l?: number; c?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);

  const setFromPointer = useCallback((clientX: number, clientY: number) => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const dx = clientX - (rect.left + rect.width / 2);
    const dy = clientY - (rect.top + rect.height / 2);
    let deg = Math.atan2(dx, -dy) * (180 / Math.PI);
    if (deg < 0) deg += 360;
    onChange(Math.round(deg) % 360);
  }, [onChange]);

  const r = size / 2;
  const ringWidth = 14;
  const tr = r - ringWidth / 2; // thumb track radius (centre of the ring band)
  const rad = (hue * Math.PI) / 180;
  const thumbX = r + tr * Math.sin(rad);
  const thumbY = r - tr * Math.cos(rad);

  const stops: string[] = [];
  for (let d = 0; d <= 360; d += 15) stops.push(`oklch(${l} ${c} ${d}) ${d}deg`);

  return (
    <div
      ref={ref}
      role="slider"
      aria-label="Matiz da cor de destaque"
      aria-valuemin={0}
      aria-valuemax={360}
      aria-valuenow={hue}
      tabIndex={0}
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        setFromPointer(e.clientX, e.clientY);
      }}
      onPointerMove={(e) => {
        if (e.buttons === 0) return;
        setFromPointer(e.clientX, e.clientY);
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight" || e.key === "ArrowUp") { e.preventDefault(); onChange((hue + 1) % 360); }
        if (e.key === "ArrowLeft" || e.key === "ArrowDown") { e.preventDefault(); onChange((hue + 359) % 360); }
      }}
      className="relative shrink-0 rounded-full cursor-pointer touch-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      style={{ width: size, height: size, background: `conic-gradient(${stops.join(", ")})` }}
    >
      {/* Donut hole — matches the card surface so the wheel reads as a ring */}
      <div className="absolute rounded-full bg-bg-overlay pointer-events-none" style={{ inset: ringWidth }} />
      {/* Thumb */}
      <span
        className="absolute w-4 h-4 rounded-full border-2 border-white shadow-[var(--shadow-md)] pointer-events-none -translate-x-1/2 -translate-y-1/2"
        style={{ left: thumbX, top: thumbY, backgroundColor: `oklch(${l} ${c} ${hue})` }}
      />
    </div>
  );
}

function TerminalSettings() {
  const fontSize = useSettingsStore((s) => s.terminalFontSize);
  const cursorStyle = useSettingsStore((s) => s.terminalCursorStyle);
  const cursorBlink = useSettingsStore((s) => s.terminalCursorBlink);
  const lineHeight = useSettingsStore((s) => s.terminalLineHeight);
  const scrollback = useSettingsStore((s) => s.terminalScrollback);

  const setFontSize = useSettingsStore((s) => s.setTerminalFontSize);
  const setCursorStyle = useSettingsStore((s) => s.setTerminalCursorStyle);
  const setCursorBlink = useSettingsStore((s) => s.setTerminalCursorBlink);
  const setLineHeight = useSettingsStore((s) => s.setTerminalLineHeight);
  const setScrollback = useSettingsStore((s) => s.setTerminalScrollback);
  const fontFamily = useSettingsStore((s) => s.terminalFontFamily);
  const setFontFamily = useSettingsStore((s) => s.setTerminalFontFamily);
  const copyOnSelect = useSettingsStore((s) => s.terminalCopyOnSelect);
  const setCopyOnSelect = useSettingsStore((s) => s.setTerminalCopyOnSelect);
  const pasteButton = useSettingsStore((s) => s.terminalPasteButton);
  const setPasteButton = useSettingsStore((s) => s.setTerminalPasteButton);

  const termFontOptions = useInstalledFontOptions(TERMINAL_FONT_CANDIDATES, fontFamily);

  return (
    <>
      <SettingsGroup label="Fonte">
        <SettingRow>
          <div>
            <label htmlFor="s-fontfamily" className={LABEL_CLASS}>Família da fonte</label>
            <p className={DESC_CLASS}>Fonte monoespaçada usada nos terminais</p>
          </div>
          <CustomSelect
            id="s-fontfamily"
            data-testid="s-fontfamily"
            value={fontFamily}
            onChange={setFontFamily}
            options={termFontOptions}
            className="w-44"
            previewOptionFont
          />
        </SettingRow>

        <SettingRow>
          <div>
            <label htmlFor="s-fontsize" className={LABEL_CLASS}>Tamanho da fonte</label>
            <p className={DESC_CLASS}>Tamanho em pixels (8 a 42)</p>
          </div>
          <RangeSetting id="s-fontsize" value={fontSize} min={8} max={42} step={1} unit="px" onChange={setFontSize} />
        </SettingRow>

        <SettingRow>
          <div>
            <label htmlFor="s-lineheight" className={LABEL_CLASS}>Altura da linha</label>
            <p className={DESC_CLASS}>Espaço entre as linhas (1,0 a 2,0)</p>
          </div>
          <RangeSetting id="s-lineheight" value={lineHeight} min={1.0} max={2.0} step={0.1} decimals={1} onChange={setLineHeight} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup label="Cursor">
        <SettingRow>
          <div>
            <p className={LABEL_CLASS}>Estilo do cursor</p>
            <p className={DESC_CLASS}>Formato do cursor do terminal</p>
          </div>
          <SegmentedControl<CursorStyle>
            id="s-cursor"
            value={cursorStyle}
            onChange={setCursorStyle}
            options={[
              { value: "bar", label: "Barra" },
              { value: "block", label: "Bloco" },
              { value: "underline", label: "Sublinhado" },
            ]}
          />
        </SettingRow>

        <SettingRow>
          <div>
            <label htmlFor="s-blink" className={LABEL_CLASS}>Cursor piscando</label>
            <p className={DESC_CLASS}>Faz o cursor piscar</p>
          </div>
          <Toggle id="s-blink" checked={cursorBlink} onChange={setCursorBlink} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup label="Área de transferência">
        <SettingRow>
          <div>
            <label htmlFor="s-copyonselect" className={LABEL_CLASS}>Copiar ao selecionar</label>
            <p className={DESC_CLASS}>Copia o texto selecionado para a área de transferência automaticamente</p>
          </div>
          <Toggle id="s-copyonselect" checked={copyOnSelect} onChange={setCopyOnSelect} />
        </SettingRow>

        <SettingRow>
          <div>
            <p className={LABEL_CLASS}>Botão de colar</p>
            <p className={DESC_CLASS}>Botão do mouse que cola a área de transferência no terminal</p>
          </div>
          <SegmentedControl<PasteButton>
            id="s-pastebutton"
            value={pasteButton}
            onChange={setPasteButton}
            options={[
              { value: "none", label: "Desligado" },
              { value: "right", label: "Botão direito" },
              { value: "middle", label: "Botão do meio" },
            ]}
          />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup label="Histórico">
        <SettingRow>
          <div>
            <label htmlFor="s-scrollback" className={LABEL_CLASS}>Histórico de rolagem</label>
            <p className={DESC_CLASS}>Quantidade de linhas guardadas no histórico (500 a 100.000)</p>
          </div>
          <NumberSetting id="s-scrollback" value={scrollback} min={500} max={100000} step={500} onChange={setScrollback} />
        </SettingRow>
        <p className="px-1 text-[length:var(--text-xs)] text-text-muted">
          As mudanças valem na hora para os terminais abertos.
        </p>
      </SettingsGroup>
    </>
  );
}

function ExplorerSettings() {
  const doubleClickAction = useSettingsStore((s) => s.explorerDoubleClickAction);
  const setDoubleClickAction = useSettingsStore((s) => s.setExplorerDoubleClickAction);

  return (
    <SettingsGroup>
      <SettingRow>
        <div>
          <p className={LABEL_CLASS}>Clique duplo em um arquivo</p>
          <p className={DESC_CLASS}>O que acontece ao dar clique duplo em um arquivo na tela de Arquivos</p>
        </div>
        <SegmentedControl<DoubleClickAction>
          id="s-doubleclick"
          value={doubleClickAction}
          onChange={setDoubleClickAction}
          options={[
            { value: "download", label: "Baixar" },
            { value: "open", label: "Abrir no editor" },
          ]}
        />
      </SettingRow>
      <p className="px-1 text-[length:var(--text-xs)] text-text-muted">
        Sem editor configurado, o arquivo é baixado (veja Editores).
      </p>
    </SettingsGroup>
  );
}

function TransferSettings() {
  const transferConcurrency = useSettingsStore((s) => s.transferConcurrency);
  const setConcurrency = useSettingsStore((s) => s.setTransferConcurrency);

  return (
    <SettingsGroup>
      <SettingRow>
        <div>
          <label htmlFor="s-concurrency" className={LABEL_CLASS}>Transferências simultâneas</label>
          <p className={DESC_CLASS}>Máximo de arquivos transferidos ao mesmo tempo (1 a 10)</p>
        </div>
        <NumberSetting id="s-concurrency" value={transferConcurrency} min={1} max={10} step={1} onChange={setConcurrency} />
      </SettingRow>
    </SettingsGroup>
  );
}

// ─── Claude (MCP) ───────────────────────────────────────────────────────────────

interface McpStatus {
  server_path: string | null;
  config_paths: string[];
  connected: boolean;
  claude_code_command: string | null;
  claude_running: boolean;
  codex_config_path: string | null;
  codex_connected: boolean;
  codex_installed: boolean;
}

const CLAUDE_CAN_DO: { label: string; note?: string }[] = [
  { label: "Listar pastas" },
  { label: "Ler arquivos" },
  { label: "Editar trechos" },
  { label: "Enviar pastas" },
  { label: "Baixar" },
  { label: "Renomear" },
  { label: "Excluir", note: "pede confirmação" },
];

type AgentCardProps = {
  title: string;
  connected: boolean;
  warning?: string | null;
  description: string;
  actionLabel: string;
  busy: boolean;
  disabled: boolean;
  onConnect: () => void;
  testId: string;
};

function AgentCard({ title, connected, warning, description, actionLabel, busy, disabled, onConnect, testId }: AgentCardProps) {
  return (
    <div
      data-testid={testId}
      className="flex flex-col items-center text-center gap-3 px-6 pt-8 pb-6 rounded-xl bg-bg-surface border border-border/50"
    >
      {connected ? (
        <span className="flex items-center justify-center w-14 h-14 rounded-full bg-accent text-text-inverse">
          <Check size={28} strokeWidth={2.5} />
        </span>
      ) : warning ? (
        <span className="flex items-center justify-center w-14 h-14 rounded-full bg-brand-amber/15 text-brand-amber">
          <AlertCircle size={26} />
        </span>
      ) : (
        <span className="flex items-center justify-center w-14 h-14 rounded-full bg-bg-subtle text-text-secondary">
          <Sparkles size={24} />
        </span>
      )}
      <div>
        <p className="text-[length:var(--text-lg)] font-bold text-text-primary">
          {connected ? `${title} conectado` : title}
        </p>
        <p className={`${DESC_CLASS} mt-1`}>{warning ?? description}</p>
      </div>
      <button
        type="button"
        disabled={disabled || busy}
        onClick={onConnect}
        className={[
          "mt-2 w-full max-w-[340px] h-11 rounded-full text-[length:var(--text-sm)] font-bold",
          "transition-colors duration-[var(--duration-fast)]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          "disabled:opacity-50 disabled:pointer-events-none",
          connected
            ? "border border-accent text-text-primary hover:bg-accent-muted"
            : "bg-accent text-text-inverse hover:bg-accent-hover",
        ].join(" ")}
      >
        {busy ? "Conectando…" : actionLabel}
      </button>
    </div>
  );
}

function ClaudeSettings() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [busy, setBusy] = useState<"claude" | "codex" | null>(null);

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      setStatus(await invoke<McpStatus>("mcp_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const connect = async (target: "claude" | "codex") => {
    setBusy(target);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      if (target === "claude") {
        setStatus(await invoke<McpStatus>("mcp_connect_claude_desktop"));
        toast.success("Conectado. Abra o Claude Desktop de novo para carregar o Talktoall Files.");
      } else {
        setStatus(await invoke<McpStatus>("mcp_connect_codex"));
        toast.success("Conectado. Reinicie o Codex para carregar o Talktoall Files.");
      }
    } catch (err) {
      toast.error(
        typeof err === "string"
          ? err
          : target === "claude"
            ? "Não foi possível atualizar a configuração do Claude Desktop"
            : "Não foi possível atualizar a configuração do Codex",
      );
      void refresh();
    } finally {
      setBusy(null);
    }
  };

  const copyCommand = async () => {
    if (!status?.claude_code_command) return;
    try {
      await navigator.clipboard.writeText(status.claude_code_command);
      toast.success("Comando copiado");
    } catch {
      toast.error("Não foi possível copiar");
    }
  };

  const noServer = !!status && !status.server_path;
  const claudeWarning =
    status && !status.connected && status.claude_running
      ? "Feche o Claude primeiro: clique com o botão direito no ícone perto do relógio e escolha Sair. Depois, conecte."
      : null;

  return (
    <>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6">
        <AgentCard
          testId="mcp-card-claude"
          title="Claude Desktop"
          connected={!!status?.connected}
          warning={claudeWarning}
          description={
            status?.connected
              ? "Seus servidores aparecem no Claude Desktop. As senhas ficam no cofre do sistema."
              : "Deixe o Claude listar, ler, editar, enviar e baixar arquivos nos seus servidores."
          }
          actionLabel={status?.connected ? "Conectar de novo" : "Conectar ao Claude"}
          busy={busy === "claude"}
          disabled={!status?.server_path || !!claudeWarning}
          onConnect={() => void connect("claude")}
        />
        <AgentCard
          testId="mcp-card-codex"
          title="Codex"
          connected={!!status?.codex_connected}
          description={
            status?.codex_connected
              ? "Seus servidores aparecem no Codex, da OpenAI. As senhas ficam no cofre do sistema."
              : status?.codex_installed
                ? "O mesmo acesso para o Codex, da OpenAI (app ou linha de comando)."
                : "O Codex, da OpenAI, não parece instalado. Dá para conectar mesmo assim e instalar depois."
          }
          actionLabel={status?.codex_connected ? "Conectar de novo" : "Conectar ao Codex"}
          busy={busy === "codex"}
          disabled={!status?.server_path}
          onConnect={() => void connect("codex")}
        />
      </div>

      {noServer && (
        <p className={`${DESC_CLASS} mb-6`}>
          O servidor MCP é instalado junto com o app. Ele não está disponível nas versões de desenvolvimento.
        </p>
      )}

      <SettingsGroup label="O que o Claude e o Codex podem fazer">
        <div className="flex flex-wrap gap-2">
          {CLAUDE_CAN_DO.map((c) => (
            <span
              key={c.label}
              className="px-3 py-1.5 rounded-full bg-bg-surface border border-border/50 text-[length:var(--text-xs)] font-semibold text-text-primary"
            >
              {c.label}
              {c.note && <span className="text-brand-amber"> · {c.note}</span>}
            </span>
          ))}
        </div>
      </SettingsGroup>

      {status?.claude_code_command && (
        <SettingsGroup label="Claude Code">
          <SettingRow>
            <code className="text-[length:var(--text-xs)] font-mono text-text-secondary break-all">
              {status.claude_code_command}
            </code>
            <button type="button" className={BTN_SECONDARY} onClick={() => void copyCommand()}>
              <Copy size={14} /> Copiar
            </button>
          </SettingRow>
        </SettingsGroup>
      )}
    </>
  );
}

// ─── Data ───────────────────────────────────────────────────────────────────────

function DataSettings() {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  // Import is two-step: pick a file, then prompt for its password.
  const [importPath, setImportPath] = useState<string | null>(null);

  const pickImportFile = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Selecionar backup do Talktoall Files",
        filters: [{ name: "Backup do Talktoall Files", extensions: ["ascpbak"] }],
      });
      if (typeof picked === "string") setImportPath(picked);
    } catch { /* dialog cancelled / unavailable */ }
  }, []);

  return (
    <>
      <SettingsGroup label="Backup">
        <SettingRow>
          <div>
            <p className={LABEL_CLASS}>Exportar backup criptografado</p>
            <p className={DESC_CLASS}>
              Salve todos os servidores, grupos, comandos, configurações e credenciais
              em um único arquivo protegido por senha.
            </p>
          </div>
          <button type="button" data-testid="s-export-backup" onClick={() => setExportOpen(true)} className={BTN_SECONDARY}>
            <Download size={13} strokeWidth={2} /> Exportar…
          </button>
        </SettingRow>
        <SettingRow>
          <div>
            <p className={LABEL_CLASS}>Importar backup</p>
            <p className={DESC_CLASS}>
              Restaure a partir de um arquivo de backup. Isso substitui todos os dados atuais e reinicia o Talktoall Files.
            </p>
          </div>
          <button type="button" data-testid="s-import-backup" onClick={() => void pickImportFile()} className={BTN_SECONDARY}>
            <Upload size={13} strokeWidth={2} /> Importar…
          </button>
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup label="Zona de perigo">
        <div className="flex items-center justify-between gap-4 px-4 py-3 rounded-xl bg-bg-surface border border-status-error/30">
          <div>
            <p className={LABEL_CLASS}>Apagar todos os dados</p>
            <p className={DESC_CLASS}>
              Exclui para sempre todos os servidores, grupos, registros do histórico de conexões,
              comandos, regras de túnel e conexões S3 salvos, junto com as credenciais
              guardadas e todas as preferências do app. O Talktoall Files volta ao estado
              da primeira abertura. Não dá para desfazer.
            </p>
          </div>
          <button
            type="button"
            data-testid="s-clear-data"
            onClick={() => setConfirmOpen(true)}
            className={[
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg shrink-0",
              "text-[length:var(--text-sm)] font-medium",
              "bg-status-error/10 border border-status-error/40 text-status-error",
              "hover:bg-status-error/15",
              "transition-colors duration-[var(--duration-fast)]",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            ].join(" ")}
          >
            <Trash2 size={13} strokeWidth={2} /> Apagar todos os dados…
          </button>
        </div>
      </SettingsGroup>

      <BackupPasswordModal mode="export" open={exportOpen} onClose={() => setExportOpen(false)} />
      <BackupPasswordModal
        mode="import"
        open={importPath !== null}
        path={importPath ?? undefined}
        onClose={() => setImportPath(null)}
      />
      <ConfirmResetModal open={confirmOpen} onClose={() => setConfirmOpen(false)} />
    </>
  );
}

/** Passphrase dialog for encrypted backup export/import.
 *  - export: set + confirm a password, then a save-file dialog picks the path.
 *  - import: enter the file's password; on success the app relaunches so the
 *    restored data loads cleanly. */
function BackupPasswordModal({ mode, open, path, onClose }: {
  mode: "export" | "import";
  open: boolean;
  path?: string;
  onClose: () => void;
}) {
  const isExport = mode === "export";
  const MIN_LEN = 8;
  const [pw, setPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);

  const valid = isExport ? pw.length >= MIN_LEN && pw === confirm : pw.length > 0;
  const canSubmit = valid && !busy;

  useEffect(() => {
    if (open) {
      setPw("");
      setConfirm("");
      setBusy(false);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const submit = useCallback(async () => {
    if (!canSubmit) return;
    setBusy(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      if (isExport) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const d = new Date();
        const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}`;
        const dest = await save({
          title: "Salvar backup do Talktoall Files",
          defaultPath: `talktoall-files-backup-${stamp}.ascpbak`,
          filters: [{ name: "Backup do Talktoall Files", extensions: ["ascpbak"] }],
        });
        if (!dest) { setBusy(false); return; } // dialog cancelled — keep the modal open
        await invoke("backup_export", { password: pw, path: dest });
        toast.success("Backup criptografado salvo.");
        onClose();
      } else {
        await invoke("backup_import", { password: pw, path });
        // Relaunch so all in-memory state reloads from the restored database.
        try {
          const { relaunch } = await import("@tauri-apps/plugin-process");
          await relaunch();
        } catch {
          window.location.reload();
        }
      }
    } catch (e: unknown) {
      setBusy(false);
      const msg = e && typeof e === "object" && "message" in e
        ? String((e as { message: unknown }).message)
        : null;
      toast.error(msg ?? (isExport ? "Não foi possível exportar o backup." : "A importação falhou."));
    }
  }, [canSubmit, isExport, pw, path, onClose]);

  return (
    <ModalShell
      open={open}
      onClose={onClose}
      title={isExport ? "Exportar backup criptografado" : "Importar backup"}
      icon={isExport ? ShieldCheck : AlertCircle}
      iconVariant={isExport ? "accent" : "danger"}
      maxWidth="md"
      busy={busy}
      testId={`backup-modal-${mode}`}
      footer={
        <>
          <button type="button" onClick={onClose} disabled={busy} className={BTN_GHOST}>Cancelar</button>
          <button
            form="backup-form"
            type="submit"
            data-testid="backup-submit"
            disabled={!canSubmit}
            className={isExport ? BTN_PRIMARY : BTN_DANGER}
          >
            {busy && <RefreshCw size={13} strokeWidth={2} className="motion-safe:animate-spin" />}
            {isExport ? (busy ? "Exportando…" : "Escolher arquivo e exportar") : (busy ? "Restaurando…" : "Importar e reiniciar")}
          </button>
        </>
      }
    >
        <form id="backup-form" onSubmit={(e) => { e.preventDefault(); void submit(); }} className="flex flex-col gap-4">
          <p className="text-[length:var(--text-sm)] text-text-secondary">
            {isExport
              ? "Escolha uma senha para criptografar o backup. Você vai precisar dela para restaurar: sem ela, não há como recuperar os dados."
              : "Digite a senha usada ao criar este backup. A importação substitui todos os dados atuais e reinicia o Talktoall Files."}
          </p>

          <div>
            <label htmlFor="backup-pw" className={FIELD_LABEL_CLASS}>Senha</label>
            <input
              ref={inputRef}
              id="backup-pw"
              data-testid="backup-password"
              type="password"
              autoComplete={isExport ? "new-password" : "current-password"}
              value={pw}
              disabled={busy}
              onChange={(e) => setPw(e.target.value)}
              placeholder={isExport ? `Pelo menos ${MIN_LEN} caracteres` : "Senha do backup"}
              className={TEXT_INPUT_CLASS}
            />
          </div>

          {isExport && (
            <div>
              <label htmlFor="backup-pw2" className={FIELD_LABEL_CLASS}>Confirmar senha</label>
              <input
                id="backup-pw2"
                data-testid="backup-password-confirm"
                type="password"
                autoComplete="new-password"
                value={confirm}
                disabled={busy}
                onChange={(e) => setConfirm(e.target.value)}
                placeholder="Digite a senha de novo"
                className={TEXT_INPUT_CLASS}
              />
              {confirm.length > 0 && pw !== confirm && (
                <p className="mt-1 text-[length:var(--text-xs)] text-status-error">As senhas não coincidem.</p>
              )}
            </div>
          )}

        </form>
    </ModalShell>
  );
}

/** Typed-confirmation dialog for the irreversible factory reset. The user must
 *  type the confirm word, then we wipe the backend and relaunch the app. */
function ConfirmResetModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const CONFIRM_WORD = "EXCLUIR";
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);

  const canReset = text.trim() === CONFIRM_WORD && !busy;

  useEffect(() => {
    if (open) {
      setText("");
      setBusy(false);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const doReset = useCallback(async () => {
    setBusy(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("factory_reset");
      // Relaunch so all in-memory state — frontend stores AND backend sessions —
      // restarts from the now-empty database, a true first-launch state.
      try {
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      } catch {
        // Relaunch unavailable (dev/web) — reload the webview as a fallback.
        window.location.reload();
      }
    } catch {
      setBusy(false);
      toast.error("Não foi possível apagar os dados. Tente de novo.");
    }
  }, []);

  return (
    <ModalShell
      open={open}
      onClose={onClose}
      title="Apagar todos os dados?"
      icon={AlertCircle}
      iconVariant="danger"
      maxWidth="md"
      busy={busy}
      testId="reset-modal"
      footer={
        <>
          <button type="button" onClick={onClose} disabled={busy} className={BTN_GHOST}>Cancelar</button>
          <button
            type="button"
            data-testid="reset-confirm-submit"
            onClick={() => void doReset()}
            disabled={!canReset}
            className={`flex items-center gap-1.5 ${BTN_DANGER}`}
          >
            {busy && <RefreshCw size={13} strokeWidth={2} className="motion-safe:animate-spin" />}
            {busy ? "Apagando…" : "Apagar todos os dados"}
          </button>
        </>
      }
    >
        <div className="flex flex-col gap-4">
          <p className="text-[length:var(--text-sm)] text-text-secondary">
            Isso exclui para sempre <strong className="text-text-primary">todos</strong> os
            servidores, grupos, histórico, comandos, regras de túnel, conexões S3, credenciais
            e preferências salvos. O Talktoall Files vai reiniciar do zero. Esta ação não pode ser desfeita.
          </p>
          <div>
            <label htmlFor="reset-confirm" className={FIELD_LABEL_CLASS}>
              Digite <code className="px-1 rounded bg-bg-base text-status-error">{CONFIRM_WORD}</code> para confirmar
            </label>
            <input
              ref={inputRef}
              id="reset-confirm"
              data-testid="reset-confirm-input"
              type="text"
              autoComplete="off"
              spellCheck={false}
              value={text}
              disabled={busy}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && canReset) void doReset(); }}
              placeholder={CONFIRM_WORD}
              className={TEXT_INPUT_CLASS}
            />
          </div>
        </div>
    </ModalShell>
  );
}

// ─── Editors ──────────────────────────────────────────────────────────────────

/** A detected editor as returned by the `detect_editors` backend command. */
type DetectedEditor = { name: string; execPath: string; args: string };

function EditorsSettings() {
  const editors = useSettingsStore((s) => s.editors);
  const defaultEditorId = useSettingsStore((s) => s.defaultEditorId);
  const addEditor = useSettingsStore((s) => s.addEditor);
  const removeEditor = useSettingsStore((s) => s.removeEditor);
  const setDefaultEditor = useSettingsStore((s) => s.setDefaultEditor);

  const [detected, setDetected] = useState<DetectedEditor[] | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [customOpen, setCustomOpen] = useState(false);

  const configuredPaths = new Set(editors.map((e) => e.execPath));
  const newlyDetected = (detected ?? []).filter((e) => !configuredPaths.has(e.execPath));

  const scan = useCallback(async () => {
    setDetecting(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const found = await invoke<DetectedEditor[]>("detect_editors");
      setDetected(found);
      // Feedback when the scan adds nothing new (the common case after the
      // first-run auto-seed) so the button doesn't feel inert.
      const configured = new Set(useSettingsStore.getState().editors.map((e) => e.execPath));
      if (found.filter((e) => !configured.has(e.execPath)).length === 0) {
        toast.info(found.length === 0
          ? "Nenhum editor encontrado neste computador."
          : "Todos os editores encontrados já foram adicionados.");
      }
    } catch {
      toast.error("Não foi possível procurar editores.");
    } finally {
      setDetecting(false);
    }
  }, []);

  return (
    <>
      <SettingsGroup label="Editores">
        {editors.length === 0 ? (
          <div className="px-4 py-6 rounded-xl bg-bg-surface border border-border/50 text-center">
            <p className="text-[length:var(--text-sm)] text-text-muted">
              Nenhum editor configurado. Procure os editores instalados ou adicione um manualmente.
            </p>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {editors.map((ed) => (
              <EditorRow
                key={ed.id}
                editor={ed}
                isDefault={ed.id === defaultEditorId}
                onMakeDefault={() => setDefaultEditor(ed.id)}
                onRemove={() => removeEditor(ed.id)}
              />
            ))}
          </div>
        )}

        {/* Actions: the two ways to add an editor. */}
        <div className="flex items-center gap-2 mt-3">
          <button onClick={() => void scan()} disabled={detecting} className={BTN_SECONDARY}>
            {detecting
              ? <RefreshCw size={13} strokeWidth={2} className="motion-safe:animate-spin" />
              : <Search size={13} strokeWidth={2} />}
            {detecting ? "Procurando…" : "Procurar editores"}
          </button>
          <button onClick={() => setCustomOpen(true)} className={BTN_SECONDARY}>
            <Plus size={13} strokeWidth={2} /> Adicionar editor personalizado
          </button>
        </div>

        {editors.length > 0 && (
          <p className="px-1 mt-2 text-[length:var(--text-xs)] text-text-muted">
            O editor com estrela é usado em “Editar”; os outros aparecem em “Abrir com”.
          </p>
        )}
      </SettingsGroup>

      {/* Detected-but-not-added editors appear only after a scan turns some up. */}
      {newlyDetected.length > 0 && (
        <SettingsGroup label="Encontrados neste computador">
          <div className="flex flex-col gap-2">
            {newlyDetected.map((ed) => (
              <div
                key={ed.execPath}
                className="flex items-center justify-between gap-3 px-4 py-3 rounded-xl bg-bg-surface border border-border/50"
              >
                <div className="min-w-0">
                  <p className={LABEL_CLASS}>{ed.name}</p>
                  <p className="text-[length:var(--text-xs)] text-text-muted truncate" title={ed.execPath}>
                    {ed.execPath}
                  </p>
                </div>
                <button
                  onClick={() => addEditor({ name: ed.name, execPath: ed.execPath, args: ed.args || "{path}" })}
                  className={BTN_SECONDARY}
                >
                  <Plus size={13} strokeWidth={2} /> Adicionar
                </button>
              </div>
            ))}
          </div>
        </SettingsGroup>
      )}

      <AddEditorModal open={customOpen} onClose={() => setCustomOpen(false)} onAdd={addEditor} />
    </>
  );
}

function EditorRow({ editor, isDefault, onMakeDefault, onRemove }: {
  editor: EditorConfig;
  isDefault: boolean;
  onMakeDefault: () => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 px-4 py-3 rounded-xl bg-bg-surface border border-border/50">
      <div className="min-w-0">
        <p className={`${LABEL_CLASS} flex items-center gap-1.5`}>
          {editor.name}
          {isDefault && (
            <span className="text-[length:var(--text-2xs)] font-medium text-accent uppercase tracking-wide">Padrão</span>
          )}
        </p>
        <p className="text-[length:var(--text-xs)] text-text-muted truncate" title={editor.execPath}>
          {editor.execPath} <span className="opacity-60">· {editor.args}</span>
        </p>
      </div>
      <div className="flex items-center gap-1.5 shrink-0">
        <button
          type="button"
          onClick={onMakeDefault}
          disabled={isDefault}
          title={isDefault ? "Editor padrão" : "Definir como padrão"}
          aria-label={isDefault ? "Editor padrão" : "Definir como padrão"}
          className={[
            "p-1.5 rounded-lg border transition-colors duration-[var(--duration-fast)]",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            isDefault
              ? "border-transparent text-accent pointer-events-none"
              : "border-border text-text-muted hover:text-text-primary hover:border-border-focus",
          ].join(" ")}
        >
          <Star size={15} strokeWidth={2} fill={isDefault ? "currentColor" : "none"} />
        </button>
        <button
          type="button"
          onClick={onRemove}
          title="Remover"
          aria-label={`Remover ${editor.name}`}
          className="p-1.5 rounded-lg border border-border text-text-muted hover:text-status-error hover:border-status-error/40 transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Trash2 size={15} strokeWidth={2} />
        </button>
      </div>
    </div>
  );
}

/** Modal form for adding a custom editor — opened from the Editors section so
 *  the multi-field form isn't always taking up space on the page. */
function AddEditorModal({ open, onClose, onAdd }: {
  open: boolean;
  onClose: () => void;
  onAdd: (editor: Omit<EditorConfig, "id">) => void;
}) {
  const [name, setName] = useState("");
  const [execPath, setExecPath] = useState("");
  const [args, setArgs] = useState("{path}");
  const [visible, setVisible] = useState(false);

  const nameRef = useRef<HTMLInputElement>(null);

  // Reset fields and play the open transition each time it's shown.
  useEffect(() => {
    if (open) {
      setName("");
      setExecPath("");
      setArgs("{path}");
      requestAnimationFrame(() => setVisible(true));
    } else {
      setVisible(false);
    }
  }, [open]);

  useEffect(() => {
    if (visible) requestAnimationFrame(() => nameRef.current?.focus());
  }, [visible]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const browse = useCallback(async () => {
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const picked = await openDialog({ multiple: false, directory: false, title: "Selecionar executável do editor" });
      if (typeof picked === "string") {
        setExecPath(picked);
        // Pre-fill the name from the file/app name when it's still blank.
        setName((cur) => (cur.trim() ? cur : (picked.split(/[\\/]/).pop() ?? "").replace(/\.(app|exe)$/i, "")));
      }
    } catch { /* dialog cancelled / unavailable */ }
  }, []);

  const canAdd = name.trim().length > 0 && execPath.trim().length > 0;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!canAdd) return;
    onAdd({ name: name.trim(), execPath: execPath.trim(), args: args.trim() || "{path}" });
    onClose();
  };

  if (!open) return null;

  return (
    <ModalBackdrop
      onClose={onClose}
      className={[
        "fixed inset-0 z-50 flex items-start justify-center pt-[8vh]",
        "transition-[background-color,backdrop-filter] duration-[var(--duration-base)]",
        visible ? "bg-black/50 backdrop-blur-sm" : "bg-black/0 backdrop-blur-none",
      ].join(" ")}
    >
      <form
        onSubmit={submit}
        data-testid="editor-modal"
        className={[
          "w-full max-w-md rounded-xl bg-bg-overlay border border-border shadow-[var(--shadow-lg)]",
          "flex flex-col max-h-[84vh]",
          "transition-[opacity,transform] duration-[var(--duration-slow)] ease-[var(--ease-expo-out)]",
          visible ? "opacity-100 translate-y-0" : "opacity-0 -translate-y-3",
        ].join(" ")}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 pt-5 pb-4 border-b border-border shrink-0">
          <h2 className="text-[length:var(--text-lg)] font-semibold text-text-primary">Adicionar editor</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Fechar"
            className="p-1.5 rounded-md text-text-muted hover:text-text-primary hover:bg-bg-subtle transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
              <path d="M1 1l12 12M13 1L1 13" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="px-6 py-4 overflow-y-auto flex-1 min-h-0 flex flex-col gap-4">
          <div>
            <label htmlFor="ed-name" className={FIELD_LABEL_CLASS}>Nome</label>
            <input
              ref={nameRef}
              id="ed-name"
              data-testid="ed-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="ex.: Sublime Text"
              className={TEXT_INPUT_CLASS}
            />
          </div>

          <div>
            <label htmlFor="ed-path" className={FIELD_LABEL_CLASS}>Caminho do executável</label>
            <div className="flex items-center gap-2">
              <input
                id="ed-path"
                data-testid="ed-path"
                type="text"
                value={execPath}
                onChange={(e) => setExecPath(e.target.value)}
                placeholder="/caminho/do/editor"
                className={TEXT_INPUT_CLASS}
              />
              <button
                type="button"
                onClick={() => void browse()}
                className="inline-flex items-center gap-1.5 px-3 py-2 shrink-0 rounded-lg text-[length:var(--text-sm)] font-medium bg-bg-base border border-border text-text-secondary hover:text-text-primary hover:border-border-focus hover:bg-bg-overlay transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <FolderOpen size={13} strokeWidth={2} /> Escolher
              </button>
            </div>
          </div>

          <div>
            <label htmlFor="ed-args" className={FIELD_LABEL_CLASS}>Argumentos</label>
            <input
              id="ed-args"
              data-testid="ed-args"
              type="text"
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              placeholder="{path}"
              className={TEXT_INPUT_CLASS}
            />
            <p className={DESC_CLASS}>
              Use <code className="px-1 rounded bg-bg-base">{"{path}"}</code> onde o arquivo deve entrar. Se ficar de fora, ele vai no final.
            </p>
          </div>
        </div>

        {/* Footer */}
        <div className="px-6 pb-5 pt-3 flex items-center justify-end gap-2 border-t border-border shrink-0">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-[length:var(--text-sm)] text-text-secondary hover:text-text-primary rounded-lg transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            Cancelar
          </button>
          <button
            type="submit"
            disabled={!canAdd}
            className="px-4 py-2 text-[length:var(--text-sm)] font-medium text-text-inverse bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed rounded-lg transition-colors duration-[var(--duration-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-bg-overlay"
          >
            Adicionar editor
          </button>
        </div>
      </form>
    </ModalBackdrop>
  );
}

function AboutSettings() {
  const openReleases = useCallback(async () => {
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(`${REPO_URL}/releases`);
    } catch { /* best-effort */ }
  }, []);

  return (
    <>
      <SettingsGroup label="Sobre">
        <AboutCard />
      </SettingsGroup>
      <SettingsGroup label="Atualizações">
        <SettingRow>
          <div>
            <p className={LABEL_CLASS}>Novas versões</p>
            <p className={DESC_CLASS}>
              Baixe o instalador mais recente e instale por cima. Seus servidores e senhas continuam salvos.
            </p>
          </div>
          <button type="button" className={BTN_SECONDARY} onClick={() => void openReleases()}>
            <ExternalLink size={14} /> Ver versões
          </button>
        </SettingRow>
      </SettingsGroup>
    </>
  );
}

function AboutCard() {
  const [appVersion, setAppVersion] = useState<string | null>(null);

  // Real app version (injected from git tags at build).
  useEffect(() => {
    void (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        setAppVersion(await getVersion());
      } catch { /* best-effort */ }
    })();
  }, []);

  const open = useCallback(async (url: string) => {
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } catch { /* best-effort */ }
  }, []);

  return (
    <div data-testid="about-card" className="px-8 py-8 rounded-xl bg-bg-surface border border-border/50">
      <div className="flex items-center gap-3 text-text-primary">
        <TalktoallWordmark height={34} />
        <span className="px-2.5 py-0.5 rounded-full bg-accent text-text-inverse text-[length:var(--text-2xs)] font-bold">
          Files
        </span>
      </div>
      <p className="mt-5 text-[length:var(--text-base)] text-text-secondary tabular-nums">
        {appVersion ? `Versão ${appVersion}` : "Versão de desenvolvimento"}
      </p>
      <p className="mt-4 max-w-md text-[length:var(--text-base)] text-text-primary">
        Seus servidores SSH, FTP e FTPS em um só lugar, com acesso pelo Claude e pelo Codex.
      </p>
      <div className="mt-6 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void open(REPO_URL)}
          className="flex items-center gap-2 h-11 px-5 rounded-full bg-bg-subtle text-text-primary text-[length:var(--text-sm)] font-bold hover:bg-bg-muted transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ExternalLink size={14} strokeWidth={2} />
          Código no GitHub
        </button>
      </div>
      <div className="mt-8 pt-5 border-t border-border/60 text-[length:var(--text-xs)] text-text-muted leading-relaxed">
        <p>Baseado no anySCP, de Nevil Macwan.</p>
        <p>
          Código aberto sob a licença MIT ·{" "}
          <button
            type="button"
            onClick={() => void open(`${UPSTREAM_URL}/blob/main/LICENSE`)}
            className="underline underline-offset-2 hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
          >
            Ver licença
          </button>{" "}
          ·{" "}
          <button
            type="button"
            onClick={() => void open(UPSTREAM_URL)}
            className="underline underline-offset-2 hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
          >
            Projeto original
          </button>
        </p>
      </div>
    </div>
  );
}

// ─── Sub-components ──────────────────────────────────────────────────────────

/** A labelled group of setting cards, mirroring the "THEME" / "INTERFACE" sections. */
function SettingsGroup({ label, children }: { label?: string; children: React.ReactNode }) {
  return (
    <section className="mb-6 last:mb-0">
      {label && (
        <h2 className="px-1 mb-3 text-[length:var(--text-2xs)] font-semibold uppercase tracking-wider text-text-muted">
          {label}
        </h2>
      )}
      <div className="flex flex-col gap-2">{children}</div>
    </section>
  );
}

function SettingRow({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3 rounded-xl bg-bg-surface border border-border/50">
      {children}
    </div>
  );
}

function Toggle({ id, checked, onChange }: { id: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      id={id}
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={[
        "relative w-9 h-5 rounded-full shrink-0",
        "transition-colors duration-[var(--duration-fast)]",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        checked ? "bg-accent" : "bg-bg-muted",
      ].join(" ")}
    >
      <span
        className={[
          "absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow-[var(--shadow-sm)]",
          "transition-transform duration-[var(--duration-fast)]",
          checked ? "translate-x-4" : "translate-x-0",
        ].join(" ")}
      />
    </button>
  );
}

/** Segmented toggle for small option sets (e.g. theme, cursor style). */
function SegmentedControl<T extends string>({ id, value, onChange, options }: {
  id?: string;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
}) {
  return (
    <div
      id={id}
      role="radiogroup"
      className="inline-grid shrink-0 gap-1 p-1 rounded-lg bg-bg-base border border-border"
      style={{ gridTemplateColumns: `repeat(${options.length}, 1fr)` }}
    >
      {options.map((opt) => {
        const selected = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={selected}
            data-testid={id ? `${id}-${opt.value}` : undefined}
            onClick={() => onChange(opt.value)}
            className={[
              "px-3 py-1.5 rounded-md text-center text-[length:var(--text-sm)] font-medium",
              "transition-colors duration-[var(--duration-fast)]",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              selected
                ? "bg-bg-overlay text-text-primary shadow-[var(--shadow-sm)]"
                : "text-text-muted hover:text-text-primary",
            ].join(" ")}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

// ─── Update checker ─────────────────────────────────────────────────────────


/** Slider with a live value readout, for bounded numeric settings. */
function RangeSetting({ id, value, min, max, step, decimals = 0, unit = "", onChange }: {
  id: string;
  value: number;
  min: number;
  max: number;
  step: number;
  decimals?: number;
  unit?: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-3 shrink-0">
      <input
        id={id}
        data-testid={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-36 h-1.5 cursor-pointer"
        style={{ accentColor: "var(--color-accent)" }}
      />
      <span className="w-10 shrink-0 text-right text-[length:var(--text-sm)] tabular-nums text-text-secondary">
        {value.toFixed(decimals).replace(".", ",")}{unit}
      </span>
    </div>
  );
}

/** Number input that uses local state while typing, commits on blur/Enter. */
function NumberSetting({ id, value, min, max, step, onChange }: {
  id: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
}) {
  const [local, setLocal] = useState(String(value));

  // Sync from store when value changes externally
  useEffect(() => {
    setLocal(String(value));
  }, [value]);

  const commit = () => {
    const n = Number(local);
    if (isNaN(n)) {
      setLocal(String(value)); // revert
      return;
    }
    const clamped = Math.max(min, Math.min(max, n));
    onChange(clamped);
    setLocal(String(clamped));
  };

  return (
    <input
      id={id}
      data-testid={id}
      type="text"
      inputMode="decimal"
      value={local}
      onChange={(e) => setLocal(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
          (e.target as HTMLInputElement).blur();
        }
        // Arrow keys for increment/decrement
        if (e.key === "ArrowUp") {
          e.preventDefault();
          const next = Math.min(max, Number(local) + step);
          setLocal(String(Number(next.toFixed(2))));
          onChange(next);
        }
        if (e.key === "ArrowDown") {
          e.preventDefault();
          const next = Math.max(min, Number(local) - step);
          setLocal(String(Number(next.toFixed(2))));
          onChange(next);
        }
      }}
      className={INPUT_CLASS}
    />
  );
}
