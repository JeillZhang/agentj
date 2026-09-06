import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";

type PermissionArgs = unknown;

export type ApprovalRequest = {
  request_id: string;
  tool: string;
  args: PermissionArgs;
  session_id: string | null;
  reason?: string;
};

type Props = {
  request: ApprovalRequest | null;
  error?: string | null;
  onRespond: (decision: string) => Promise<void> | void;
  onClose: () => void;
};

const FIELD_MAP: Record<string, Record<string, string>> = {
  bash: { cmd: "命令", timeout: "超时", cwd: "目录" },
  read: { path: "路径", offset: "偏移", limit: "行数" },
  write: { path: "文件", content: "内容" },
  edit: { path: "文件", content: "内容" },
};

export function fieldRows(tool: string, args: unknown): Array<{ label: string; value: unknown }> {
  if (!args || typeof args !== "object") return [{ label: "args", value: args }];
  const map = FIELD_MAP[tool] ?? {};
  const obj = args as Record<string, unknown>;
  const keys = Object.keys(obj);
  if (keys.length === 0) return [];
  const known = keys.filter((k) => map[k]).map((k) => ({ label: map[k], value: obj[k] }));
  const rest = keys.filter((k) => !map[k]);
  return [...known, ...(rest.length ? [{ label: "args", value: rest.reduce((a, k) => ({ ...a, [k]: obj[k] }), {} as Record<string, unknown>) }] : [])];
}

export default function ApprovalDialog({ request, error, onRespond, onClose }: Props) {
  const [allowAlways, setAllowAlways] = useState(false);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const scrimRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!request) return;
    const el = scrimRef.current;
    if (!el) return;
    const focusables = el.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    if (focusables.length > 0) {
      // focus first focusable element
      focusables[0].focus();
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key === "Tab") {
        const nodes = Array.from(
          el!.querySelectorAll<HTMLElement>(
            'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
          ),
        ).filter((n) => !(n as HTMLButtonElement).disabled && n.tabIndex !== -1);
        if (nodes.length === 0) return;
        const first = nodes[0];
        const last = nodes[nodes.length - 1];
        const active = document.activeElement as HTMLElement | null;
        if (e.shiftKey) {
          if (active === first) {
            e.preventDefault();
            last.focus();
          }
        } else {
          if (active === last) {
            e.preventDefault();
            first.focus();
          }
        }
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [request, onClose]);

  if (!request) return null;

  const rows = fieldRows(request.tool, request.args);

  function toggle(idx: number) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }

  return (
    <div ref={scrimRef} role="dialog" aria-label="permission request" aria-modal="true">
      <div className="aj-dialog-card">
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>Permission request</h2>
          <button
            type="button"
            aria-label="close"
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              cursor: "pointer",
              padding: 4,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--aj-textMuted)",
            }}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </div>
        <p>tool: {request.tool}</p>
        {request.session_id && <p>session: {request.session_id}</p>}
        {request.reason && <p>reason: {request.reason}</p>}
        {rows.map((row, idx) => {
          const value = row.value;
          let displayStr: string;
          if (typeof value === "string") displayStr = value;
          else if (value !== null && typeof value === "object") displayStr = JSON.stringify(value);
          else displayStr = String(value ?? "");
          const isLong = typeof value === "string" ? value.length > 200 : displayStr.length > 200;
          const isExpanded = expanded.has(idx);
          const shown = isLong && !isExpanded ? `${displayStr.slice(0, 200)}…` : displayStr;
          return (
            <div key={idx} className="aj-row">
              <b>{row.label}</b>
              <span>{shown}</span>
              {isLong && (
                <button type="button" onClick={() => toggle(idx)}>
                  {isExpanded ? "collapse" : "expand"}
                </button>
              )}
            </div>
          );
        })}
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button type="button" className="aj-button--ghost" onClick={() => void onRespond("deny")}>
            Deny
          </button>
          <button
            type="button"
            className="aj-button"
            onClick={() => void onRespond(allowAlways ? "allow_always" : "allow")}
          >
            Allow
          </button>
          <button type="button" className="aj-button--danger" onClick={() => void onRespond("allow_always")}>
            Allow All
          </button>
        </div>
        <label className="aj-always">
          <input type="checkbox" checked={allowAlways} onChange={(e) => setAllowAlways(e.target.checked)} />
          allow_always 会话内生效
        </label>
        {error && (
          <p role="alert" className="aj-error" style={{ color: "var(--aj-error)" }}>
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
