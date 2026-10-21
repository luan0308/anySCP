import { useState, useCallback, useRef } from "react";
import { AlertTriangle, Braces } from "lucide-react";
import type { Snippet } from "../../types";
import { useSessionStore } from "../../stores/session-store";
import { CustomSelect } from "../shared/CustomSelect";
import {
  extractVariables,
  parseVariables,
  resolveCommand,
  resolveBuiltin,
  BUILTIN_NAMES,
} from "../../utils/snippet-resolve";
import { ModalShell, BTN_GHOST, BTN_PRIMARY } from "../shared/ModalShell";

interface VariableDialogProps {
  snippet: Snippet;
  onExecute: (resolvedCommand: string) => void;
  onCancel: () => void;
}

export function VariableDialog({ snippet, onExecute, onCancel }: VariableDialogProps) {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const sessions = useSessionStore((s) => s.sessions);
  const session = activeSessionId ? (sessions.get(activeSessionId) ?? null) : null;

  const variableMeta = parseVariables(snippet.variables);
  const allVarNames = extractVariables(snippet.command);
  const builtinVars = allVarNames.filter((n) => BUILTIN_NAMES.includes(n));
  const userVarNames = allVarNames.filter((n) => !BUILTIN_NAMES.includes(n));

  const initialValues = Object.fromEntries(
    userVarNames.map((name) => {
      const meta = variableMeta.find((v) => v.name === name);
      return [name, meta?.default_value ?? ""];
    }),
  );

  const [values, setValues] = useState<Record<string, string>>(initialValues);
  const firstInputRef = useRef<HTMLInputElement | null>(null);

  const handleExecute = useCallback(() => {
    const resolved = resolveCommand(snippet.command, values, session);
    onExecute(resolved);
  }, [snippet.command, values, session, onExecute]);

  const setValue = (name: string, value: string) => {
    setValues((prev) => ({ ...prev, [name]: value }));
  };

  const hasUnmetRequired = userVarNames.some((name) => {
    const meta = variableMeta.find((v) => v.name === name);
    return meta?.required && !values[name]?.trim();
  });

  const inputClass = [
    "w-full rounded-lg bg-bg-base border border-border px-3 py-2",
    "text-[length:var(--text-sm)] text-text-primary placeholder:text-text-muted font-mono",
    "outline-none focus:border-border-focus focus:ring-2 focus:ring-ring",
    "transition-[border-color,box-shadow] duration-[var(--duration-fast)]",
  ].join(" ");

  const labelClass = "block text-[length:var(--text-xs)] font-medium text-text-secondary mb-1";
  let inputIndex = 0;

  return (
    <ModalShell
      open
      onClose={onCancel}
      title={snippet.name}
      subtitle={snippet.command}
      icon={Braces}
      maxWidth="md"
      scrollable
      footer={
        <>
          <button type="button" onClick={onCancel} className={BTN_GHOST}>
            Cancelar
          </button>
          <button
            form="variable-dialog-form"
            type="submit"
            disabled={hasUnmetRequired}
            className={BTN_PRIMARY}
          >
            Executar
          </button>
        </>
      }
    >
      <form id="variable-dialog-form" onSubmit={(e) => { e.preventDefault(); handleExecute(); }} className="flex flex-col gap-4">
        {snippet.is_dangerous && (
          <div className="flex items-start gap-2.5 p-3 rounded-lg bg-status-error/10 border border-status-error/30">
            <AlertTriangle size={16} strokeWidth={2} className="text-status-error shrink-0 mt-0.5" aria-hidden="true" />
            <p className="text-[length:var(--text-xs)] text-status-error leading-relaxed">
              Este comando está marcado como perigoso. Revise o comando final com atenção antes de executar.
            </p>
          </div>
        )}

        {builtinVars.length > 0 && (
          <div className="flex flex-col gap-3">
            <p className="text-[length:var(--text-xs)] font-semibold uppercase tracking-widest text-text-muted">Preenchidas automaticamente</p>
            {builtinVars.map((name) => {
              const resolved = resolveBuiltin(name, session) ?? "(nenhuma sessão ativa)";
              return (
                <div key={name}>
                  <label className={labelClass}>{`{{${name}}}`}</label>
                  <div className="w-full rounded-lg bg-bg-base border border-border px-3 py-2 text-[length:var(--text-sm)] text-text-muted font-mono cursor-default select-text">
                    {resolved}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {builtinVars.length > 0 && userVarNames.length > 0 && <hr className="border-border" />}

        {userVarNames.length > 0 && (
          <div className="flex flex-col gap-3">
            {builtinVars.length > 0 && (
              <p className="text-[length:var(--text-xs)] font-semibold uppercase tracking-widest text-text-muted">Variáveis</p>
            )}
            {userVarNames.map((name) => {
              const meta = variableMeta.find((v) => v.name === name);
              const isFirst = inputIndex === 0;
              inputIndex++;
              const label = meta?.label ?? name;
              const placeholder = meta?.placeholder ?? `Informe ${name}`;
              const options = meta?.options ?? null;
              const required = meta?.required ?? false;

              return (
                <div key={name}>
                  <label className={labelClass}>
                    {label}
                    {required && <span className="text-status-error ml-1" aria-label="obrigatório">*</span>}
                  </label>
                  {options && options.length > 0 ? (
                    <CustomSelect
                      value={values[name] ?? ""}
                      onChange={(v) => setValue(name, v)}
                      placeholder="Selecionar…"
                      options={[{ value: "", label: "Selecionar…" }, ...options.map((opt) => ({ value: opt, label: opt }))]}
                    />
                  ) : (
                    <input
                      ref={isFirst ? (el) => { firstInputRef.current = el; } : undefined}
                      type="text"
                      value={values[name] ?? ""}
                      onChange={(e) => setValue(name, e.target.value)}
                      placeholder={placeholder}
                      required={required}
                      className={inputClass}
                    />
                  )}
                </div>
              );
            })}
          </div>
        )}

        {allVarNames.length === 0 && (
          <p className="text-[length:var(--text-sm)] text-text-muted text-center py-2">
            Nenhuma variável para preencher.
          </p>
        )}
      </form>
    </ModalShell>
  );
}
