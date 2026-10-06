import { useCallback, useEffect, useRef, useState } from "react";
import {
  METHODS,
  PROTOCOL_VERSION,
  ResponseRouter,
  RpcClient,
  VERSION,
  WebSocketTransport,
  resolveWsUrl,
  parseProgress,
} from "@agentj/sdk";
import type { NotificationHandler } from "@agentj/sdk";
import { Moon, Sun, Monitor, Menu, PanelRight } from "lucide-react";
import ApprovalDialog from "./ApprovalDialog";
import type { ApprovalRequest } from "./ApprovalDialog";
import RunPanel from "./RunPanel";
import TopbarMenu from "./components/TopbarMenu";
import ConnectionStatus from "./components/ConnectionStatus";
import UnifiedSidebar from "./components/UnifiedSidebar";
import WorkspaceShell from "./components/WorkspaceShell";
import RightPanel, { groupByRunId } from "./components/RightPanel";
import TodoPanel from "./components/TodoPanel";
import ChangeList from "./components/ChangeList";
import { Trajectory, historyToConversation } from "./Trajectory";
import type { RawEvent } from "./Trajectory";
import heroImg from "./assets/hero.png";
import { loadCurrentProject, setCurrentProject } from "./projectStore";
import { loadConnection, saveConnection } from "./connectionStore";
import { loadComposerPrefs, loadLayout, saveLayout, subscribeLayout } from "./layoutStore";
import { loadDisplayPrefs, subscribeDisplayPrefs } from "./displayPrefsStore";
import type { DisplayPrefs } from "./displayPrefsStore";
import { isDemoEnabled, subscribeDemo, DEMO_TODOS, DEMO_CHANGES } from "./demoData";
import { applyFileChange, changesFromHistory, EMPTY_CHANGES } from "./changes";
import type { ChangesState } from "./changes";
import { EMPTY_TODO_STATE, applyTodoFrame, focusRun, todosFromHistory } from "./todoState";
import type { TodoView } from "./todoState";

const rawWsEnv = import.meta.env.VITE_AGENTJ_WS_URL;
const envWsUrl = typeof rawWsEnv === "string" ? rawWsEnv : undefined;
const pageLocation =
  typeof window === "undefined"
    ? undefined
    : { hostname: window.location.hostname, protocol: window.location.protocol };

/**
 * Manual settings (dialog / stored) win over the build-time env URL; when no
 * host or port is given, fall back to the env URL, then the page origin, then
 * loopback (see `resolveWsUrl`).
 */
function parsePort(port: string): number | undefined {
  if (!port.trim()) return undefined;
  const parsed = Number(port);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function resolveConnection(host: string, port: string, token: string): string {
  const trimmedHost = host.trim();
  const portNumber = parsePort(port);
  if (trimmedHost || portNumber !== undefined) {
    return resolveWsUrl({
      host: trimmedHost || undefined,
      port: portNumber,
      token: token || undefined,
      location: pageLocation,
    });
  }
  return resolveWsUrl({ envUrl: envWsUrl, token: token || undefined, location: pageLocation });
}

/** Malformed endpoints (e.g. a host with an embedded port) would make `new WebSocket` throw at mount. */
function safeWsUrl(candidate: string): string {
  try {
    new URL(candidate);
    return candidate;
  } catch {
    return resolveConnection("", "", "");
  }
}

function initialWsUrl(): string {
  const stored = loadConnection();
  return safeWsUrl(resolveConnection(stored?.host ?? "", stored?.port ?? "", stored?.token ?? ""));
}

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

const SEMVER_RE = /^(\d+)\.(\d+)(?:\.(\d+))?(?:-[0-9A-Za-z.-]+)?$/;

/**
 * Protocol-version compatibility per docs/specs/ipc-protocol.md §4.3:
 * `protocol_version` is semver where only a major change is breaking, so minor/patch
 * drift in either direction is compatible after a successful `initialize`.
 * Unparsable input is treated as incompatible (defensive banner).
 */
export function isProtocolCompatible(clientVersion: string, serverVersion: string): boolean {
  const client = SEMVER_RE.exec(clientVersion);
  const server = SEMVER_RE.exec(serverVersion);
  if (!client || !server) return false;
  return Number(client[1]) === Number(server[1]);
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
  const [models, setModels] = useState<{ name: string }[]>([]);
  const [serverVersion, setServerVersion] = useState<string | null>(null);
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
        setServerVersion(res.server_info?.version ?? null);
        if (!isProtocolCompatible(PROTOCOL_VERSION, res.protocol_version)) {
          setBanner(`expected ${PROTOCOL_VERSION}, got ${res.protocol_version}`);
        } else if (res.protocol_version !== PROTOCOL_VERSION) {
          console.debug(`[agentj] protocol drift: expected ${PROTOCOL_VERSION}, got ${res.protocol_version}`);
        }
      })
      .catch(() => {})
      .finally(() => {
        if (cancelled) return;
        cli
          .request(METHODS.kernelStatus)
          .then((r) => {
            const data = r as {
              modules?: { id: string; state: string }[];
              default_model?: string;
              providers?: { id: string; models?: string[] }[];
            };
            if (cancelled) return;
            setModules(data.modules ?? []);
            setModelName(data.default_model ?? null);
            const served = (data.providers ?? [])
              .flatMap((p) => p.models ?? [])
              .map((name) => ({ name }));
            if (served.length > 0) setModels(served);
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

  return { client, modules, status, banner, unreachable, modelName, models, serverVersion };
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
      if (typeof p.request_id === "string" && loadComposerPrefs().permissionMode === "bypass") {
        void client
          .request(METHODS.permissionRespond, { request_id: p.request_id, decision: "allow" })
          .catch((err) => {
            console.debug("permission auto-allow failed", err);
          });
        return;
      }
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
  const [wsUrl, setWsUrl] = useState(initialWsUrl);
  const [connectSeq, setConnectSeq] = useState(0);
  const { client: sharedClient, modules, status, banner, unreachable, modelName, models, serverVersion } =
    useSharedKernel(wsUrl, connectSeq);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const { pending, approvalError, respond, close } = useApproval(sharedClient, activeSessionId);
  const [theme, setTheme] = useState<ThemeMode>(storedThemeMode() ?? "system");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [rightDrawerOpen, setRightDrawerOpen] = useState(false);
  const [tab, setTab] = useState<"conversation" | "trajectory">("conversation");
  const [historyEvents, setHistoryEvents] = useState<RawEvent[] | null>(null);
  const [historySession, setHistorySession] = useState<string | null>(null);
  const [currentProjectId, setCurrentProjectIdState] = useState<string>(() => {
    try { return loadCurrentProject(); } catch { return "default"; }
  });
  const [demo, setDemo] = useState(() => isDemoEnabled());
  const [rightCollapsed, setRightCollapsed] = useState(() => loadLayout().rightCollapsed);
  const [fileChanges, setFileChanges] = useState<ChangesState>(EMPTY_CHANGES);
  const [todos, setTodos] = useState(EMPTY_TODO_STATE);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [highlightedPath, setHighlightedPath] = useState<string | null>(null);
  const activeSessionRef = useRef<string | null>(activeSessionId);
  const activeRunIdRef = useRef<string | null>(activeRunId);
  useEffect(() => { activeSessionRef.current = activeSessionId; }, [activeSessionId]);
  useEffect(() => { activeRunIdRef.current = activeRunId; }, [activeRunId]);

  useEffect(() => subscribeDemo(setDemo), []);

  useEffect(() => subscribeLayout((state) => setRightCollapsed(state.rightCollapsed)), []);

  useEffect(() => {
    setFileChanges(EMPTY_CHANGES);
    setTodos(EMPTY_TODO_STATE);
    setActiveRunId(null);
    setHighlightedPath(null);
  }, [activeSessionId]);

  // second notification handler (useSharedKernel owns the first): apply file_change frames
  useEffect(() => {
    if (!sharedClient) return;
    const handler: NotificationHandler = (method, params) => {
      if (method !== METHODS.progress) return;
      const frame = parseProgress(params);
      if (frame.kind === "file_change" && frame.session_id !== activeSessionRef.current) return;
      if (frame.kind === "file_change" && frame.runId) setActiveRunId(frame.runId);
      if (frame.kind === "todo" && frame.runId) setActiveRunId(frame.runId);
      if (frame.kind === "file_change") setFileChanges((prev) => applyFileChange(prev, frame));
    };
    sharedClient.setNotificationHandler(handler);
    return () => { sharedClient.removeNotificationHandler?.(handler); };
  }, [sharedClient]);

  // third notification handler: apply `$/progress` todo snapshots for the active session
  useEffect(() => {
    if (!sharedClient) return;
    const handler: NotificationHandler = (method, params) => {
      if (method !== METHODS.progress) return;
      const frame = parseProgress(params);
      if (frame.kind === "todo" && frame.session_id !== activeSessionRef.current) return;
      if (frame.kind === "todo" && frame.runId) setActiveRunId(frame.runId);
      if (frame.kind === "file_change" && frame.runId) setActiveRunId(frame.runId);
      setTodos((prev) => applyTodoFrame(prev, frame));
    };
    sharedClient.setNotificationHandler(handler);
    return () => { sharedClient.removeNotificationHandler?.(handler); };
  }, [sharedClient]);

  const handleRightCollapsedChange = useCallback((collapsed: boolean) => {
    saveLayout({ rightCollapsed: collapsed });
    setRightCollapsed(collapsed);
  }, []);

  const handleSelectRun = useCallback((runId: string | null) => {
    if (runId === activeRunId) return;
    setActiveRunId(runId);
    if (runId) {
      setTodos((prev) => focusRun(prev, runId));
    }
  }, [activeRunId]);

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
    const apply = (prefs: DisplayPrefs) => {
      const el = document.documentElement;
      if (prefs.reducedMotion) el.dataset.reducedMotion = "true";
      else delete el.dataset.reducedMotion;
    };
    apply(loadDisplayPrefs());
    return subscribeDisplayPrefs(apply);
  }, []);

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
          const req: Record<string, unknown> = {
            session_id: activeSessionId,
            cursor,
            limit: 500,
          };
          if (activeRunId) req.run_id = activeRunId;
          const res = (await clientToUse!.request(METHODS.sessionHistory, req)) as {
            events?: RawEvent[];
            next_cursor?: number | null;
          } | null;
          if (!cancelled && res?.events) all.push(...res.events);
          const next = res?.next_cursor;
          if (typeof next !== "number" || next <= cursor) break;
          cursor = next;
        }
        if (!cancelled) {
          setHistoryEvents(all);
          setHistorySession(activeSessionId);
          if (activeRunId) {
            setTodos(todosFromHistory(all, activeRunId));
            setFileChanges(changesFromHistory(all, activeRunId));
          } else {
            setTodos(todosFromHistory(all));
            setFileChanges(changesFromHistory(all));
          }
        }
      } catch (err) {
        console.debug("history", err);
        if (!cancelled) setHistoryEvents([]);
      } finally {
        tempTransport?.close?.();
      }
    })();
    return () => { cancelled = true; };
  }, [wsUrl, activeSessionId, activeRunId, sharedClient]);

  const connState = connectionLight(status, unreachable);

  const todosByRunMap = (() => {
    const byRun = (todos as unknown as {
      byRun?: Map<string | null, { revision: number; items: TodoView[] }>;
    })?.byRun;
    if (!(byRun instanceof Map)) return undefined;
    const map = new Map<string | null, TodoView[]>();
    for (const [runId, entry] of byRun) map.set(runId, entry.items);
    return map;
  })();

  const runGroups = demo
    ? groupByRunId(DEMO_CHANGES, new Map<string | null, TodoView[]>([[null, DEMO_TODOS]]))
    : groupByRunId(fileChanges.summary, todosByRunMap);

  const handleConnect = useCallback((host: string, port: string, token: string) => {
    saveConnection({ host, port, token });
    setWsUrl(safeWsUrl(resolveConnection(host, port, token)));
    setConnectSeq((n) => n + 1);
    setDrawerOpen(false);
  }, []);

  const setActiveSessionIdAndHydrate = useCallback((id: string) => {
    const trimmed = id.trim();
    if (!trimmed) return;
    setActiveSessionId(trimmed);
    setDrawerOpen(false);
    const rpc = sharedClient;
    if (!rpc) return;
    void rpc
      .request(METHODS.sessionStatus, { session_id: trimmed })
      .then(() => rpc.request(METHODS.sessionSubscribe, { session_id: trimmed }))
      .catch(() => {});
  }, [sharedClient]);

  useEffect(() => {
    if (!rightDrawerOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setRightDrawerOpen(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [rightDrawerOpen]);

  return (
    <div className="aj-app" data-drawer={drawerOpen ? "open" : "closed"}>
      <header className="aj-topbar" role="banner">
        <div className="aj-topbar__brand">
          <h1>agentj</h1>
          <ConnectionStatus
            state={connState}
            label={connState === "connected" ? `已连接 ${wsUrl}` : "未连接"}
            model={modelName}
            modules={modules.map((m) => `${m.id}: ${m.state}`)}
            uiVersion={VERSION}
            serverVersion={serverVersion}
          />
        </div>
        <div className="aj-topbar__center" aria-hidden="true" />
        <div className="aj-topbar__actions">
          <TopbarMenu onConnect={handleConnect} onContinue={setActiveSessionIdAndHydrate} />
          <button type="button" aria-label="toggle sessions" className="aj-button aj-drawer-toggle" aria-expanded={drawerOpen} aria-controls="aj-sidebar" onClick={() => setDrawerOpen((o) => !o)}><Menu size={16} aria-hidden="true" /></button>
          <button type="button" aria-label="toggle theme" className="aj-icon-btn" onClick={() => setTheme((t) => THEME_CYCLE[t])}>
            {theme === "dark" ? <Moon size={16} aria-hidden="true" /> : theme === "light" ? <Sun size={16} aria-hidden="true" /> : <Monitor size={16} aria-hidden="true" />}
          </button>
          <button
            type="button"
            className="aj-right-toggle"
            aria-label="运行详情"
            aria-expanded={rightDrawerOpen}
            onClick={() => setRightDrawerOpen((o) => !o)}
          >
            <PanelRight size={16} aria-hidden="true" />
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
      {rightDrawerOpen && (
        <div
          data-testid="right-scrim"
          className="aj-scrim"
          role="presentation"
          aria-hidden="true"
          onClick={() => setRightDrawerOpen(false)}
        />
      )}
      {banner && <p role="alert" className="aj-banner" data-state="mismatch">{banner}</p>}
      <WorkspaceShell
        right={
          <RightPanel
            collapsed={rightCollapsed}
            onToggle={() => handleRightCollapsedChange(!rightCollapsed)}
            currentRunId={activeRunId}
            groups={runGroups}
            history={demo ? [] : fileChanges.history}
            highlightedPath={demo ? null : highlightedPath}
            onSelectRun={handleSelectRun}
          >
            <TodoPanel
              items={demo ? DEMO_TODOS : todos.items}
              state={demo ? null : todos}
              activeRunId={demo ? null : activeRunId}
              onSelectRun={handleSelectRun}
            />
            <ChangeList
              changes={demo ? DEMO_CHANGES : fileChanges.summary}
              history={demo ? [] : fileChanges.history}
              highlightedPath={demo ? null : highlightedPath}
              activeRunId={demo ? null : activeRunId}
              onSelectRun={handleSelectRun}
            />
          </RightPanel>
        }
        rightCollapsed={rightCollapsed}
        onRightCollapsedChange={handleRightCollapsedChange}
        rightDrawerOpen={rightDrawerOpen}
      >
        <UnifiedSidebar
          wsUrl={wsUrl}
          client={sharedClient}
          activeSessionId={activeSessionId}
          onActiveChange={(id) => { setActiveSessionId(id); handleProjectChange(loadCurrentProject()); setDrawerOpen(false); }}
          onSessionRemoved={(id) => { if (activeSessionRef.current === id) setActiveSessionId(null); }}
          onFileSelect={(path) => setHighlightedPath(path)}
          unreachable={unreachable}
        />
        <div className="aj-main">
          <div role="tablist" aria-label="views">
            <button
              role="tab"
              aria-selected={tab === "conversation"}
              className="aj-tab"
              onClick={() => setTab("conversation")}
            >
              对话
            </button>
            <button
              role="tab"
              aria-selected={tab === "trajectory"}
              className="aj-tab"
              onClick={() => setTab("trajectory")}
            >
              轨迹
            </button>
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
                    const btn = document.querySelector<HTMLButtonElement>(".aj-session-actions button");
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
                <RunPanel client={sharedClient} key={activeSessionId} activeSessionId={activeSessionId} initialEntries={historyEvents && historySession === activeSessionId ? historyToConversation(historyEvents) : []} pendingTool={pending?.tool} model={modelName ?? undefined} models={models} modules={modules} status={status} unreachable={unreachable} onRunId={(id) => setActiveRunId(id)} />
              </>
            )
          ) : activeSessionId && historySession === activeSessionId && historyEvents ? (
            <Trajectory events={historyEvents} />
          ) : (
            <p className="aj-empty">Select a session to inspect its trajectory.</p>
          )}
        </div>
      </WorkspaceShell>
      {pending && <ApprovalDialog request={pending} error={approvalError} onRespond={respond} onClose={close} />}
    </div>
  );
}
