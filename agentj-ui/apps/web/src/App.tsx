import { useCallback, useEffect, useRef, useState } from "react";
import {
  METHODS,
  PROTOCOL_VERSION,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
  resolveWsUrl,
} from "@agentj/sdk";
import type { NotificationHandler } from "@agentj/sdk";
import { MoreHorizontal, Moon, Sun, Monitor, Menu } from "lucide-react";
import ApprovalDialog from "./ApprovalDialog";
import type { ApprovalRequest } from "./ApprovalDialog";
import RunPanel from "./RunPanel";
import UnifiedSidebar from "./components/UnifiedSidebar";
import { Trajectory, historyToConversation } from "./Trajectory";
import type { RawEvent } from "./Trajectory";
import heroImg from "./assets/hero.png";
import { loadCurrentProject, setCurrentProject } from "./projectStore";

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

let patched = false;
const handlerMap = new WeakMap<RpcClient, NotificationHandler[]>();

function ensurePatch(): void {
  if (patched) return;
  patched = true;
  const proto = RpcClient.prototype as unknown as {
    setNotificationHandler: (h: NotificationHandler) => void;
    removeNotificationHandler: (h: NotificationHandler) => void;
  };
  const origSet = proto.setNotificationHandler;
  const origRemove = proto.removeNotificationHandler;
  if (!origSet) return;
  proto.setNotificationHandler = function (this: RpcClient, handler: NotificationHandler) {
    let list = handlerMap.get(this);
    if (!list) {
      list = [];
      handlerMap.set(this, list);
      origSet.call(this, (method: string, params: unknown) => {
        const cur = handlerMap.get(this);
        cur?.forEach((fn) => {
          try {
            fn(method, params);
          } catch {}
        });
      });
    }
    if (!list.includes(handler)) list.push(handler);
  };
  proto.removeNotificationHandler = function (this: RpcClient, handler: NotificationHandler) {
    const list = handlerMap.get(this);
    if (!list) {
      origRemove?.call(this, handler);
      return;
    }
    const idx = list.indexOf(handler);
    if (idx >= 0) list.splice(idx, 1);
  };
}

function openOpClient(wsUrl: string) {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport, router);
  return { transport, client };
}



function useSharedKernel(wsUrl: string, seq: number) {
  const [modules, setModules] = useState<{ id: string; state: string }[]>([]);
  const [status, setStatus] = useState(`connecting to kernel on ${wsUrl}…`);
  const [banner, setBanner] = useState<string | null>(null);
  const [unreachable, setUnreachable] = useState(false);
  const [modelName, setModelName] = useState<string | null>(null);
  const [client, setClient] = useState<RpcClient | null>(null);
  const transportRef = useRef<WebSocketTransport | null>(null);

  useEffect(() => {
    ensurePatch();
    let cancelled = false;
    setUnreachable(false);
    setBanner(null);
    setStatus(`connecting to kernel on ${wsUrl}…`);
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(wsUrl, router);
    transportRef.current = transport;
    transport.onStatusChange = (s) => {
      if (!cancelled) setStatus(`kernel ${s} on ${wsUrl}`);
    };
    const cli = new RpcClient(transport as unknown as never, router as unknown as never);
    setClient(cli);
    cli.setNotificationHandler((method, params) => {
      if (method === METHODS.progress) console.debug("[agentj]", method, params);
    });
    cli
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
        cli
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
      setClient(null);
      transportRef.current = null;
    };
  }, [wsUrl, seq]);

  return { client, modules, status, banner, unreachable, modelName };
}

function useApproval(client: RpcClient | null, activeSessionId: string | null) {
  const [pending, setPending] = useState<ApprovalRequest | null>(null);
  const [approvalError, setApprovalError] = useState<string | null>(null);
  const activeRef = useRef(activeSessionId);
  useEffect(() => { activeRef.current = activeSessionId; }, [activeSessionId]);
  useEffect(() => {
    if (pending?.session_id && pending.session_id !== activeSessionId) {
      setPending(null);
      setApprovalError(null);
    }
  }, [activeSessionId, pending]);
  useEffect(() => {
    if (!client) return;
    setPending(null);
    setApprovalError(null);
    const handler: NotificationHandler = (method, params) => {
      if (method !== METHODS.permissionRequest) return;
      const p = params as { request_id: string; tool: string; args: unknown; session_id?: string | null; reason?: string };
      const active = activeRef.current;
      if (!active) return;
      const sid = p.session_id ?? null;
      if (sid && sid !== active) return;
      setPending({ request_id: p.request_id, tool: p.tool, args: p.args, session_id: sid ?? active, reason: p.reason });
      setApprovalError(null);
    };
    client.setNotificationHandler(handler);
    return () => {
      client.removeNotificationHandler?.(handler);
    };
  }, [client]);
  const respond = useCallback(async (decision: string) => {
    const req = pending;
    if (!req || !client) return;
    try {
      await client.request(METHODS.permissionRespond, { request_id: req.request_id, decision });
      setPending(null);
      setApprovalError(null);
    } catch (e) { setApprovalError(e instanceof Error ? e.message : String(e)); }
  }, [client, pending]);
  const close = useCallback(() => { setPending(null); setApprovalError(null); }, []);
  return { pending, approvalError, respond, close };
}

export default function App() {
  const [wsUrl, setWsUrl] = useState(() => resolveWsUrl({ envUrl: envWsUrl }));
  const [connectSeq, setConnectSeq] = useState(0);
  const { client: sharedClient, modules, status, banner, unreachable, modelName } = useSharedKernel(wsUrl, connectSeq);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const { pending, approvalError, respond, close } = useApproval(sharedClient, activeSessionId);
  const [theme, setTheme] = useState<ThemeMode>(storedThemeMode() ?? "system");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [statusOpen, setStatusOpen] = useState(false);
  const [tab, setTab] = useState<"conversation" | "trajectory">("conversation");
  const [historyEvents, setHistoryEvents] = useState<RawEvent[] | null>(null);
  const [historySession, setHistorySession] = useState<string | null>(null);
  const [currentProjectId, setCurrentProjectIdState] = useState<string>(() => {
    try { return loadCurrentProject(); } catch { return "default"; }
  });

  // stale currentProject fallback + cleanup
  useEffect(() => {
    try {
      const raw = localStorage.getItem("agentj.currentProject");
      const resolved = loadCurrentProject();
      if (raw && raw !== resolved) {
        try { localStorage.removeItem("agentj.currentProject"); } catch {}
        // also ensure set to default via helper for consistency
        try { setCurrentProject(resolved); localStorage.removeItem("agentj.currentProject"); } catch {}
        // if resolved is default, the removals ensure cleaned state
      }
      if (resolved !== currentProjectId) setCurrentProjectIdState(resolved);
    } catch {}
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleProjectChange = useCallback((id: string) => {
    try { setCurrentProject(id); } catch {}
    setCurrentProjectIdState(id);
  }, []);

  useEffect(() => {
    const el = document.documentElement;
    if (theme === "system") delete el.dataset.theme;
    else el.dataset.theme = theme;
    try { localStorage.setItem("agentj.theme-mode", theme); } catch {}
  }, [theme]);

  useEffect(() => {
    if (activeSessionId) {
      requestAnimationFrame(() => {
        document.getElementById("aj-prompt")?.focus();
      });
    }
  }, [activeSessionId]);

  useEffect(() => {
    if (!activeSessionId) { setHistorySession(null); setHistoryEvents(null); return; }
    let cancelled = false;
    setHistorySession(activeSessionId);
    setHistoryEvents(null);
    void (async () => {
      let tempTransport: { close?: () => void } | null = null;
      let clientToUse: RpcClient | null = sharedClient;
      if (!clientToUse) {
        const opened = openOpClient(wsUrl);
        tempTransport = opened.transport as unknown as { close?: () => void };
        clientToUse = opened.client;
      }
      try {
        const all: RawEvent[] = [];
        let cursor = 0;
        for (let page = 0; page < 10; page++) {
          const res = (await clientToUse!.request(METHODS.sessionHistory, {
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
        tempTransport?.close?.();
      }
    })();
    return () => { cancelled = true; };
  }, [wsUrl, activeSessionId, sharedClient]);

  const connState = connectionLight(status, unreachable);

  const handleConnect = useCallback((host: string, port: string, token: string) => {
    const p = port ? Number(port) : undefined;
    const next = resolveWsUrl({
      envUrl: envWsUrl,
      host: host || undefined,
      port: p,
      token: token || undefined,
    });
    setWsUrl(next);
    setConnectSeq((n) => n + 1);
    setDrawerOpen(false);
  }, []);

  // expose currentProjectId wiring via window for UnifiedSidebar new session assignment
  useEffect(() => {
    (window as unknown as Record<string, unknown>).__agentjCurrentProject = currentProjectId;
  }, [currentProjectId]);

  return (
    <div className="aj-app" data-drawer={drawerOpen ? "open" : "closed"}>
      <header className="aj-topbar" role="banner">
        <div className="aj-topbar__brand">
          <h1>agentj</h1>
          <p className="aj-statusline">
            <span className="aj-status-light" data-state={connState} aria-hidden="true" />
            <span className="aj-status-text">{status}</span>
          </p>
        </div>
        <div className="aj-topbar__center" aria-hidden="true" />
        <div className="aj-topbar__actions">
          {modelName !== null && (
            <span className="aj-chip aj-chip--model" aria-label="model">{modelName}</span>
          )}
          <div className="aj-popover-wrap">
            <button
              type="button"
              aria-label="status more"
              className="aj-popover-trigger"
              aria-expanded={statusOpen}
              aria-haspopup="true"
              onClick={() => setStatusOpen((o) => !o)}
            >
              <MoreHorizontal size={16} aria-hidden="true" />
            </button>
            {statusOpen && (
              <div className="aj-popover" role="dialog" aria-label="status">
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                  <span className="aj-status-light" data-state={connState} aria-hidden="true" />
                  <span style={{ fontSize: 12, color: "var(--aj-textMuted)" }}>{status}</span>
                </div>
                <ul className="aj-chips" aria-label="status">
                  {modules.length === 0 ? (
                    <li className="aj-chip">no modules</li>
                  ) : (
                    modules.map((m) => (
                      <li key={m.id} className="aj-chip" data-state={m.state}>
                        {m.id}: {m.state}
                      </li>
                    ))
                  )}
                </ul>
              </div>
            )}
          </div>
          <button type="button" aria-label="toggle sessions" className="aj-button aj-drawer-toggle" aria-expanded={drawerOpen} aria-controls="aj-sidebar" onClick={() => setDrawerOpen((o) => !o)}><Menu size={16} aria-hidden="true" /></button>
          <button type="button" aria-label="toggle theme" className="aj-button--ghost" onClick={() => setTheme((t) => THEME_CYCLE[t])}>
            {theme === "dark" ? <Moon size={16} aria-hidden="true" /> : theme === "light" ? <Sun size={16} aria-hidden="true" /> : <Monitor size={16} aria-hidden="true" />}
          </button>
        </div>
      </header>
      {drawerOpen && (
        <div
          data-testid="scrim"
          className="aj-scrim"
          role="presentation"
          aria-hidden="true"
          onClick={() => setDrawerOpen(false)}
        />
      )}
      {banner && <p role="alert" className="aj-banner" data-state="mismatch">{banner}</p>}
      {unreachable && (
        <p>{`agentj dashboard --port ${new URL(resolveWsUrl({ envUrl: envWsUrl })).port || new URL(resolveWsUrl({})).port}`}</p>
      )}
      <div className="aj-layout">
        <UnifiedSidebar
          wsUrl={wsUrl}
          client={sharedClient}
          activeSessionId={activeSessionId}
          onActiveChange={(id) => { setActiveSessionId(id); handleProjectChange(loadCurrentProject()); setDrawerOpen(false); }}
          onConnect={handleConnect}
          unreachable={unreachable}
        />
        <main className="aj-main">
          <div role="tablist" aria-label="views">
            <button role="tab" aria-selected={tab === "conversation"} className="aj-tab" onClick={() => setTab("conversation")}>Conversation</button>
            <button role="tab" aria-selected={tab === "trajectory"} className="aj-tab" onClick={() => setTab("trajectory")}>Trajectory</button>
          </div>
          {tab === "conversation" ? (
            activeSessionId === null ? (
              <section className="aj-hero" aria-label="hero empty">
                <img src={heroImg} alt="hero" width={120} height={120} />
                <h2>Where to next?</h2>
                <p>Select a session or start a new run</p>
                <button
                  type="button"
                  className="aj-button aj-hero-trigger"
                  onClick={() => {
                    const btn = document.querySelector<HTMLButtonElement>(".aj-session-actions button, .aj-project-new");
                    if (btn) btn.click();
                    else window.dispatchEvent(new CustomEvent("agentj:new-session"));
                  }}
                >
                  New session
                </button>
              </section>
            ) : (
              <>
                {(historyEvents === null || historySession !== activeSessionId) && <p className="aj-empty">Loading history…</p>}
                <RunPanel client={sharedClient} key={activeSessionId} activeSessionId={activeSessionId} initialEntries={historyEvents && historySession === activeSessionId ? historyToConversation(historyEvents) : []} pendingTool={pending?.tool} model={modelName ?? undefined} />
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
