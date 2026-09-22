// ConnectionStatus.tsx — topbar status light with a hover/focus detail panel.
// Holds the connection line, model and module states previously shown in TopbarMenu.
import { useState } from "react";

type ConnState = "connected" | "connecting" | "unreachable";

type Props = {
  state: ConnState;
  label: string;
  model?: string | null;
  modules?: string[];
  uiVersion?: string | null;
  serverVersion?: string | null;
};

export default function ConnectionStatus({
  state,
  label,
  model,
  modules,
  uiVersion,
  serverVersion,
}: Props) {
  const [open, setOpen] = useState(false);
  const connected = state === "connected";
  const mismatch = Boolean(uiVersion && serverVersion && uiVersion !== serverVersion);

  return (
    <div
      className="aj-status-hover"
      data-testid="connection-status"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <button
        type="button"
        className="aj-status-trigger"
        aria-label="连接状态"
        aria-haspopup="true"
        aria-expanded={open}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
      >
        <span className="aj-status-light" data-state={state} aria-hidden="true" />
      </button>
      {open && (
        <div className="aj-status-panel" role="tooltip" aria-label="连接状态">
          <span className="aj-status-row">
            <span className="aj-status-dot" data-state={connected ? "on" : "off"} aria-hidden="true" />
            <span className="aj-status-text">{label}</span>
          </span>
          {model && <span className="aj-status-row">{model}</span>}
          {modules?.map((m) => (
            <span key={m} className="aj-status-row">
              {m}
            </span>
          ))}
          {(uiVersion || serverVersion) && (
            <span
              className="aj-status-row"
              data-testid="status-versions"
              data-state={mismatch ? "warn" : "ok"}
            >
              {`版本 UI ${uiVersion ?? "未知"} · 内核 ${serverVersion ?? "未知"}${
                mismatch ? " ⚠" : ""
              }`}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
