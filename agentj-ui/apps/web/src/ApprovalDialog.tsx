import { useState } from "react";

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
    <div role="dialog" aria-label="permission request" aria-modal="true">
      <div className="aj-dialog-card">
        <h2>Permission request</h2>
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
        <div>
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
          <button type="button" className="aj-button" onClick={() => void onRespond("allow_always")}>
            Allow All
          </button>
          {error && (
            <button type="button" onClick={onClose}>
              Close
            </button>
          )}
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
