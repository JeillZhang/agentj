import { useCallback, useEffect, useRef, useState } from "react";
import {
  METHODS,
  PROTOCOL_VERSION,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
  resolveWsUrl,
} from "@agentj/sdk";
import ApprovalDialog from "./ApprovalDialog";
import type { ApprovalRequest } from "./ApprovalDialog";
import ConnectionBar from "./ConnectionBar";
import RunPanel from "./RunPanel";
import SessionPanel from "./SessionPanel";
import { Trajectory, historyToConversation } from "./Trajectory";
import type { RawEvent } from "./Trajectory";

const rawWsEnv = import.meta.env.VITE_AGENTJ_WS_URL;
const envWsUrl = typeof rawWsEnv === "string" ? rawWsEnv : undefined;

type ThemeMode = "dark" | "light" | "system";
const THEME_CYCLE: Record<ThemeMode, ThemeMode> = { dark: "light", light: "system", system: "dark" };
function storedThemeMode(): ThemeMode | null {
  try {
    const t = localStorage.getItem("agentj.theme-mode");
    return t === "dark" || t === "light" || t === "system" ? t : null;
  } catch {
    return null;
  }
}

export function connectionLight(status: string, unreachable: boolean): "connected" | "connecting" | "unreachable" {
  if (unreachable) return "unreachable";
  return status.startsWith("connected") ? "connected" : "connecting";
}

function openOpClient(wsUrl: string) {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport, router);
  return { transport, client };
}

function useKernel(wsUrl: string) {
  const [modules, setModules] = useState<{ id: string; state: string }[]>([]);
  const [status, setStatus] = useState(`connecting to kernel on ${wsUrl}…`);
  const [banner, setBanner] = useState<string | null>(null);
  const [unreachable, setUnreachable] = useState(false);
  const [modelName, setModelName] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    setUnreachable(false);
    setBanner(null);
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    transport.onStatusChange = (s) => {
      if (!cancelled) setStatus(`kernel ${s} on ${wsUrl}`);
    };
    const client = new RpcClient(transport, router);
    client.setNotificationHandler((method, params) => {
      if (method === METHODS.progress) console.debug("[agentj]", method, params);
    });
    client
      .initialize(PROTOCOL_VERSION)
      .then((res) => {
        if (cancelled) return;
        if (res.protocol_version !== PROTOCOL_VERSION) {
          setBanner(`expected ${PROTOCOL_VERSION}, got ${res.protocol_version}`);
        }
      })
      .catch(() => {})
      .finally(() => {
        if (cancelled) return;
        client
          .request(METHODS.kernelStatus)
          .then((r) => {
            const data = r as { modules?: { id: string; state: string }[]; default_model?: string };
            if (cancelled) return;
            setModules(data.modules ?? []);
            setModelName((data as { default_model?: string }).default_model ?? null);
            setStatus("connected");
          })
          .catch(() => {
            if (cancelled) return;
            setUnreachable(true);
            setStatus(`kernel unreachable on ${wsUrl}`);
          });
      });
    return () => {
      cancelled = true;
      transport.close();
    };
  }, [wsUrl]);
  return { modules, status, banner, unreachable, modelName };
}

function useApproval(wsUrl: string, activeSessionId: string | null) {
  const [pending, setPending] = useState<ApprovalRequest | null>(null);
  const [approvalError, setApprovalError] = useState<string | null>(null);
  const clientRef = useRef<RpcClient | null>(null);
  const activeRef = useRef(activeSessionId);
  useEffect(() => { activeRef.current = activeSessionId; }, [activeSessionId]);
  useEffect(() => {
    if (pending?.session_id && pending.session_id !== activeSessionId) {
      setPending(null);
      setApprovalError(null);
    }
  }, [activeSessionId, pending]);
  useEffect(() => {
    setPending(null);
    setApprovalError(null);
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    const client = new RpcClient(transport, router);
    clientRef.current = client;
    client.setNotificationHandler((method, params) => {
      if (method !== METHODS.permissionRequest) return;
      const p = params as { request_id: string; tool: string; args: unknown; session_id?: string | null; reason?: string };
      const active = activeRef.current;
      if (!active) return;
      const sid = p.session_id ?? null;
      if (sid && sid !== active) return;
      setPending({ request_id: p.request_id, tool: p.tool, args: p.args, session_id: sid ?? active, reason: p.reason });
      setApprovalError(null);
    });
    return () => { clientRef.current = null; transport.close(); };
  }, [wsUrl]);
  const respond = useCallback(async (decision: string, allowAlways: boolean) => {
    const req = pending;
    const client = clientRef.current;
    if (!req || !client) return;
    try {
      await client.request(METHODS.permissionRespond, { request_id: req.request_id, decision, allow_always: allowAlways });
      setPending(null);
      setApprovalError(null);
    } catch (e) { setApprovalError(e instanceof Error ? e.message : String(e)); }
  }, [pending]);
  const close = useCallback(() => { setPending(null); setApprovalError(null); }, []);
  return { pending, approvalError, respond, close };
}

export default function App() {
  const [wsUrl, setWsUrl] = useState(() => resolveWsUrl({ envUrl: envWsUrl }));
  const { modules, status, banner, unreachable, modelName } = useKernel(wsUrl);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const { pending, approvalError, respond, close } = useApproval(wsUrl, activeSessionId);
  const [theme, setTheme] = useState<ThemeMode>(storedThemeMode() ?? "system");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [tab, setTab] = useState<"conversation" | "trajectory">("conversation");
  const [historyEvents, setHistoryEvents] = useState<RawEvent[] | null>(null);
  const [historySession, setHistorySession] = useState<string | null>(null);

  useEffect(() => {
    const el = document.documentElement;
    if (theme === "system") delete el.dataset.theme;
    else el.dataset.theme = theme;
    try { localStorage.setItem("agentj.theme-mode", theme); } catch {}
  }, [theme]);

  useEffect(() => {
    if (!activeSessionId) { setHistorySession(null); setHistoryEvents(null); return; }
    let cancelled = false;
    setHistorySession(activeSessionId);
    setHistoryEvents(null);
    void (async () => {
      const { transport, client } = openOpClient(wsUrl);
      try {
        const all: RawEvent[] = [];
        let cursor = 0;
        for (let page = 0; page < 10; page++) {
          const res = (await client.request(METHODS.sessionHistory, {
            session_id: activeSessionId, cursor, limit: 500,
          })) as { events?: RawEvent[]; next_cursor?: number | null } | null;
          if (!cancelled && res?.events) all.push(...res.events);
          const next = res?.next_cursor;
          if (typeof next !== "number" || next <= cursor) break;
          cursor = next;
        }
        if (!cancelled) { setHistoryEvents(all); setHistorySession(activeSessionId); }
      } catch (err) {
        console.debug("history", err);
        if (!cancelled) setHistoryEvents([]);
      } finally {
        (transport as unknown as { close?: () => void }).close?.();
      }
    })();
    return () => { cancelled = true; };
  }, [wsUrl, activeSessionId]);

  const connState = connectionLight(status, unreachable);

  return (
    <div className="aj-app" data-drawer={drawerOpen ? "open" : "closed"}>
      <header className="aj-topbar" role="banner">
        <h1>agentj</h1>
        <ConnectionBar
          onConnect={(host, port, token) => {
            const p = port ? Number(port) : undefined;
            const next = resolveWsUrl({
              envUrl: envWsUrl,
              host: host || undefined,
              port: p,
              token: token || undefined,
            });
            setWsUrl(next);
            setDrawerOpen(false);
          }}
        />
        <p className="aj-statusline">
          <span className="aj-status-light" data-state={connState} aria-hidden="true" />
          {status}
        </p>
        {modelName !== null && (
          <span className="aj-chip aj-chip--model" aria-label="model">{modelName}</span>
        )}
        <ul className="aj-chips" aria-label="modules">
          {modules.map((m) => (
            <li key={m.id} className="aj-chip" data-state={m.state}>{m.id}: {m.state}</li>
          ))}
        </ul>
        <button type="button" aria-label="toggle sessions" className="aj-button aj-drawer-toggle" aria-expanded={drawerOpen} aria-controls="aj-session-rail" onClick={() => setDrawerOpen((o) => !o)}>☰</button>
        <button type="button" aria-label="toggle theme" className="aj-button--ghost" onClick={() => setTheme((t) => THEME_CYCLE[t])}>{theme === "dark" ? "☾" : theme === "light" ? "☀" : "◐"}</button>
      </header>
      {banner && <p role="alert" className="aj-banner" data-state="mismatch">{banner}</p>}
      {unreachable && (
        <p>{`agentj dashboard --port ${new URL(resolveWsUrl({ envUrl: envWsUrl })).port || new URL(resolveWsUrl({})).port}`}</p>
      )}
      <div className="aj-layout">
        <aside className="aj-session-rail" id="aj-session-rail">
          <SessionPanel wsUrl={wsUrl} activeSessionId={activeSessionId} onActiveChange={(id) => { setActiveSessionId(id); setDrawerOpen(false); }} />
        </aside>
        <main className="aj-main">
          <div role="tablist" aria-label="views">
            <button role="tab" aria-selected={tab === "conversation"} className="aj-tab" onClick={() => setTab("conversation")}>Conversation</button>
            <button role="tab" aria-selected={tab === "trajectory"} className="aj-tab" onClick={() => setTab("trajectory")}>Trajectory</button>
          </div>
          {tab === "conversation" ? (
            activeSessionId === null ? (
              <p className="aj-empty">Select a session to view its conversation, or start a new run.</p>
            ) : (
              <>
                {(historyEvents === null || historySession !== activeSessionId) && <p className="aj-empty">Loading history…</p>}
                <RunPanel wsUrl={wsUrl} key={activeSessionId} activeSessionId={activeSessionId} initialEntries={historyEvents && historySession === activeSessionId ? historyToConversation(historyEvents) : []} pendingTool={pending?.tool} model={modelName ?? undefined} />
              </>
            )
          ) : activeSessionId && historySession === activeSessionId && historyEvents ? (
            <Trajectory events={historyEvents} />
          ) : (
            <p className="aj-empty">Select a session to inspect its trajectory.</p>
          )}
        </main>
      </div>
      {pending && <ApprovalDialog request={pending} error={approvalError} onRespond={respond} onClose={close} />}
    </div>
  );
}
