import { createCliRenderer } from "@opentui/core";
import { createRoot, useKeyboard, useRenderer } from "@opentui/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { METHODS, ResponseRouter, RpcClient, WebSocketTransport, parseProgress } from "@agentj/sdk";
import type { NotificationHandler, ProgressFrame, TransportStatus } from "@agentj/sdk";
import { resolveWsUrl } from "@agentj/sdk";
import { Composer } from "./composer.tsx";
import { applyTodoFrame, EMPTY_TODOS, nextKey, rowsFromFrame, RunTranscript } from "./runview.tsx";
import type { TodoState, TranscriptRow } from "./runview.tsx";
import { restoreTerminal, setupGracefulShutdown } from "./connection.ts";
import {
  SessionList,
  addToHistory,
  loadSessionHistory,
  saveSessionHistory,
  useSession,
} from "./session.tsx";
import { PermissionDialog, usePermissionState } from "./permission.tsx";
import { Palette } from "./palette.tsx";
import { ModelSelector, useModelConfig } from "./models.tsx";
import { ThemeProvider, useTheme } from "./theme.tsx";
import Sidebar, { SIDEBAR_WIDTH, loadCollapsed, loadProjects, saveCollapsed } from "./sidebar.tsx";
import type { Project, SidebarView } from "./sidebar.tsx";
import { useFileTree } from "./files.tsx";
import { clampSelection, flattenVisible, moveSelection, parentPath } from "./files.ts";
export interface TransportHandle {
  onStatusChange?: (s: TransportStatus) => void;
  close(): void;
  lastError?: string;
}

export type AppProps = {
  url?: string;
  createTransport?: (url: string, router: ResponseRouter) => TransportHandle;
  backoffBaseMs?: number;
};

export function getBackoffDelay(attempt: number, baseMs = 1000): number {
  return Math.min(baseMs * Math.pow(2, attempt), 30000);
}

function isAuthError(msg: string): boolean {
  const low = msg.toLowerCase();
  return (
    msg.includes("403") ||
    low.includes("notauthorized") ||
    low.includes("auth") ||
    low.includes("forbidden")
  );
}

function useCtrlCExit(): void {
  const renderer = useRenderer();
  useEffect(() => setupGracefulShutdown(restoreTerminal), []);
  useKeyboard((key) => {
    const isCtrlC = key.ctrl && (key.name === "c" || key.name === "C");
    if (isCtrlC) {
      try {
        restoreTerminal();
      } catch {}
      try {
        renderer?.destroy?.();
      } catch {}
      process.exit(0);
    }
  });
}

function buildTransport(
  url: string | undefined,
  createTransport: AppProps["createTransport"],
  router: ResponseRouter,
): TransportHandle {
  const wsUrl = url ?? resolveWsUrl({});
  if (createTransport) return createTransport(wsUrl, router);
  return new WebSocketTransport(wsUrl, router) as unknown as TransportHandle;
}

function bindClient(
  transport: TransportHandle,
  router: ResponseRouter,
  ref: MutableRefObject<RpcClient | null>,
): void {
  try {
    const t = transport as unknown as { send: (l: string) => Promise<string> };
    if (typeof t.send === "function") ref.current = new RpcClient(t, router);
  } catch {}
}

function attachStatus(
  transport: TransportHandle,
  setAuthError: (e: string | null) => void,
  setStatus: (s: TransportStatus) => void,
): void {
  const prev = transport.onStatusChange;
  transport.onStatusChange = (s: TransportStatus) => {
    if (s === "closed" && transport.lastError && isAuthError(transport.lastError)) {
      setAuthError(transport.lastError);
    }
    if (s === "open") setAuthError(null);
    setStatus(s);
    prev?.(s);
  };
  if (transport.lastError && isAuthError(transport.lastError)) {
    setAuthError(transport.lastError);
    setStatus("closed");
  }
}

function useTransportLifecycle(
  url: string | undefined,
  createTransport: AppProps["createTransport"],
  attempt: number,
  authError: string | null,
  setStatus: (s: TransportStatus) => void,
  setAuthError: (e: string | null) => void,
  transportRef: MutableRefObject<TransportHandle | null>,
  timerRef: MutableRefObject<number | null>,
  backoffBaseMs: number,
  routerRef: MutableRefObject<ResponseRouter | null>,
  clientRef: MutableRefObject<RpcClient | null>,
): void {
  useEffect(() => {
    if (authError) return;
    const router = new ResponseRouter();
    routerRef.current = router;
    let transport: TransportHandle;
    try {
      transport = buildTransport(url, createTransport, router);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (isAuthError(msg)) setAuthError(msg);
      setStatus("closed");
      return;
    }
    transportRef.current = transport;
    bindClient(transport, router, clientRef);
    attachStatus(transport, setAuthError, setStatus);
    return () => {
      try {
        transport.close();
      } catch {}
      clearTimeout(timerRef.current as unknown as number);
    };
  }, [
    attempt,
    authError,
    backoffBaseMs,
    createTransport,
    url,
    setAuthError,
    setStatus,
    timerRef,
    transportRef,
    routerRef,
    clientRef,
  ]);
}

function useBackoffTimer(
  status: TransportStatus,
  authError: string | null,
  attempt: number,
  setAttempt: Dispatch<SetStateAction<number>>,
  timerRef: MutableRefObject<number | null>,
  backoffBaseMs: number,
): void {
  useEffect(() => {
    if (status === "closed" && !authError) {
      const delay = getBackoffDelay(attempt, backoffBaseMs);
      timerRef.current = setTimeout(
        () => setAttempt((a) => a + 1),
        delay,
      ) as unknown as number;
      return () => {
        clearTimeout(timerRef.current as unknown as number);
      };
    }
    if (status === "open" && attempt !== 0) setAttempt(0);
  }, [status, authError, attempt, backoffBaseMs, setAttempt, timerRef]);
}

function useConnection(
  url: string | undefined,
  createTransport: AppProps["createTransport"],
  backoffBaseMs: number,
): {
  status: TransportStatus;
  authError: string | null;
  client: RpcClient | null;
} {
  const [status, setStatus] = useState<TransportStatus>("connecting");
  const [authError, setAuthError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const transportRef = useRef<TransportHandle | null>(null);
  const timerRef = useRef<number | null>(null);
  const routerRef = useRef<ResponseRouter | null>(null);
  const clientRef = useRef<RpcClient | null>(null);
  const [client, setClient] = useState<RpcClient | null>(null);
  useTransportLifecycle(
    url,
    createTransport,
    attempt,
    authError,
    setStatus,
    setAuthError,
    transportRef,
    timerRef,
    backoffBaseMs,
    routerRef,
    clientRef,
  );
  useBackoffTimer(status, authError, attempt, setAttempt, timerRef, backoffBaseMs);
  useEffect(() => {
    if (status === "open" && clientRef.current) setClient(clientRef.current);
    if (status !== "open") setClient(null);
  }, [status]);
  useEffect(() => {
    return () => {
      clearTimeout(timerRef.current as unknown as number);
      try {
        transportRef.current?.close();
      } catch {}
    };
  }, []);
  return { status, authError, client };
}

export type SessionState = {
  history: string[];
  setHistory: Dispatch<SetStateAction<string[]>>;
  activeId: string | null;
  setActiveId: Dispatch<SetStateAction<string | null>>;
  info: string | null;
  setInfo: Dispatch<SetStateAction<string | null>>;
  err: string | null;
  setErr: Dispatch<SetStateAction<string | null>>;
};

function useSessionState(): SessionState {
  const [history, setHistory] = useState<string[]>(() => loadSessionHistory());
  const [activeId, setActiveId] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  return { history, setHistory, activeId, setActiveId, info, setInfo, err, setErr };
}

function useAppHandlers(client: RpcClient | null, st: SessionState) {
  const { createNew, continueSession } = useSession(client);
  const handleSelect = useCallback(
    async (id: string) => {
      st.setErr(null);
      try {
        await continueSession(id);
        st.setActiveId(id);
        st.setInfo(`status: ${id}`);
        st.setHistory((prev) => {
          const next = addToHistory(prev, id);
          saveSessionHistory(next);
          return next;
        });
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        st.setErr(msg);
      }
    },
    [continueSession, st],
  );
  const handleNew = useCallback(async () => {
    st.setErr(null);
    try {
      const id = await createNew();
      st.setActiveId(id);
      st.setInfo(`status: ${id}`);
      st.setHistory((prev) => {
        const next = addToHistory(prev, id);
        saveSessionHistory(next);
        return next;
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      st.setErr(msg);
    }
  }, [createNew, st]);
  return { handleSelect, handleNew };
}

function getStatusLine(authError: string | null, status: TransportStatus): string {
  if (authError) return `auth error: ${authError}`;
  if (status === "open") return "connected";
  if (status === "closed") return "kernel unreachable";
  return "connecting...";
}
function usePaletteControl(onThemeToggle?: () => void) {
  const [paletteQuery, setPaletteQuery] = useState<string | null>(null);
  const [modelOpen, setModelOpen] = useState(false);
  const handleSelect = useCallback(
    (cmd: string) => {
      if (cmd === "models") setModelOpen(true);
      if (cmd === "themes") onThemeToggle?.();
      setPaletteQuery(null);
    },
    [onThemeToggle],
  );
  const handleClose = useCallback(() => setPaletteQuery(null), []);
  const handleModelClose = useCallback(() => setModelOpen(false), []);
  useKeyboard((key) => {
    if (modelOpen) {
      const name = (key.name ?? "").toLowerCase();
      if (name === "escape") setModelOpen(false);
      return;
    }
    if (paletteQuery !== null) {
      const name = (key.name ?? "").toLowerCase();
      if (name === "escape") {
        setPaletteQuery(null);
        return;
      }
      if (name === "return" || name === "enter" || name === "up" || name === "down") return;
      if (name === "backspace") {
        setPaletteQuery((prev) => {
          if (!prev) return null;
          const next = prev.slice(0, -1);
          return next.length === 0 ? null : next;
        });
        return;
      }
      const seq = key.sequence ?? "";
      if (seq.length === 1 && !key.ctrl && !key.meta) {
        setPaletteQuery((prev) => (prev ?? "") + seq);
      }
      return;
    }
    const seq = key.sequence ?? "";
    if (seq === "/") setPaletteQuery("/");
  });
  return { paletteQuery, modelOpen, handleSelect, handleClose, handleModelClose };
}

function AppInner({ url, createTransport, backoffBaseMs = 1000 }: AppProps) {
  useCtrlCExit();
  const { status, authError, client } = useConnection(url, createTransport, backoffBaseMs);
  const st = useSessionState();
  const { handleSelect, handleNew } = useAppHandlers(client, st);
  const { pending: permReq, respond: handlePermRespond, clear: clearPerm } = usePermissionState(client);
  const { toggle: toggleTheme, palette } = useTheme();
  const paletteCtrl = usePaletteControl(toggleTheme);
  const statusLine = getStatusLine(authError, status);
  const modelConfig = useModelConfig(client);
  const [transcriptRows, setTranscriptRows] = useState<TranscriptRow[]>([]);
  const [todos, setTodos] = useState<TodoState>(EMPTY_TODOS);
  const [sidebarView, setSidebarView] = useState<SidebarView>("threads");
  const [filesIdx, setFilesIdx] = useState(0);
  const fileTree = useFileTree(client, sidebarView === "files" ? st.activeId : null);
  const filesRows = useMemo(() => flattenVisible(fileTree.nodes ?? []), [fileTree.nodes]);
  useEffect(() => {
    setFilesIdx((v) => clampSelection(v, filesRows.length));
  }, [filesRows.length]);
  useEffect(() => {
    setFilesIdx(0);
  }, [st.activeId]);
  useEffect(() => {
    setTranscriptRows([]);
    setTodos(EMPTY_TODOS);
  }, [st.activeId]);
  useEffect(() => {
    if (!client) return;
    const handler: NotificationHandler = (method, params) => {
      if (method !== METHODS.progress) return;
      const frame = parseProgress(params);
      if (frame.kind === "todo") {
        setTodos((prev) => applyTodoFrame(prev, frame));
        return;
      }
      setTranscriptRows((prev) => rowsFromFrame(prev, frame));
    };
    client.setNotificationHandler(handler);
  }, [client]);
  const handleSubmit = useCallback(
    async (text: string) => {
      const sid = st.activeId;
      if (!client || !sid) return;
      st.setErr(null);
      setTranscriptRows((prev) => [...prev, { key: nextKey(prev), kind: "user", text }]);
      try {
        await client.request(METHODS.sessionRun, { session_id: sid, content: text });
      } catch (e) {
        st.setErr(e instanceof Error ? e.message : String(e));
      }
    },
    [client, st],
  );
  const statusColor =
    authError !== null ? palette.error : status === "open" ? palette.success : palette.warning;
  const [projects] = useState<Project[]>(() => loadProjects());
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(() => loadCollapsed());
  const sidebarSessions = useMemo(() => st.history.map((id) => ({ id, preview: id })), [st.history]);
  const sidebarEntries = useMemo(() => {
    const entries: Array<{ kind: "project"; id: string } | { kind: "session"; id: string; projectId: string }> = [];
    if (projects.length === 0) {
      for (const s of sidebarSessions) entries.push({ kind: "session", id: s.id, projectId: "default" });
      return entries;
    }
    for (const p of projects) {
      entries.push({ kind: "project", id: p.id });
      if (!collapsed[p.id]) {
        const rows = sidebarSessions.filter((s) => {
          const pid = (s as { projectId?: string }).projectId;
          if (pid) return pid === p.id;
          return p.id === projects[0].id;
        });
        for (const r of rows) entries.push({ kind: "session", id: r.id, projectId: p.id });
      }
    }
    return entries;
  }, [projects, collapsed, sidebarSessions]);
  const [sidebarIdx, setSidebarIdx] = useState(0);
  useEffect(() => {
    if (sidebarIdx >= sidebarEntries.length && sidebarEntries.length > 0) setSidebarIdx(sidebarEntries.length - 1);
    if (sidebarEntries.length === 0) setSidebarIdx(0);
  }, [sidebarEntries, sidebarIdx]);
  const handleToggle = useCallback(
    (id: string) => {
      setCollapsed((prev) => {
        const next = { ...prev, [id]: !prev[id] };
        saveCollapsed(next);
        return next;
      });
    },
    [],
  );
  useKeyboard((key) => {
    if (paletteCtrl.paletteQuery !== null || paletteCtrl.modelOpen || permReq) return;
    const name = (key.name ?? "").toLowerCase();
    const seq = key.sequence ?? "";
    if (name === "f") {
      setSidebarView((v) => (v === "files" ? "threads" : "files"));
      return;
    }
    if (sidebarView === "files") {
      const rows = filesRows;
      const dirRow = rows[filesIdx];
      const toggleDir = (): void => {
        if (!dirRow || dirRow.node.kind !== "dir") return;
        if (dirRow.node.expanded) fileTree.collapse(dirRow.node.path);
        else fileTree.toggle(dirRow.node);
      };
      if (name === "j" || name === "down" || seq === "\u001B[B") {
        setFilesIdx((v) => moveSelection(v, 1, rows.length));
        return;
      }
      if (name === "k" || name === "up" || seq === "\u001B[A") {
        setFilesIdx((v) => moveSelection(v, -1, rows.length));
        return;
      }
      if (name === "return" || name === "enter" || seq === "\r" || seq === "\n") {
        toggleDir();
        return;
      }
      if (name === "l" || name === "right" || seq === "\u001B[C") {
        toggleDir();
        return;
      }
      if (name === "h" || name === "left" || seq === "\u001B[D") {
        if (!dirRow) return;
        if (dirRow.node.kind === "dir" && dirRow.node.expanded) {
          fileTree.collapse(dirRow.node.path);
          return;
        }
        const parent = parentPath(dirRow.node.path);
        if (parent) setFilesIdx(rows.findIndex((r) => r.node.path === parent));
        return;
      }
      if (name === "r") {
        void fileTree.refresh();
        return;
      }
    }
    if (name === "j" || name === "down" || seq === "\u001B[B") {
      setSidebarIdx((v) => Math.min(sidebarEntries.length - 1, v + 1));
      return;
    }
    if (name === "k" || name === "up" || seq === "\u001B[A") {
      setSidebarIdx((v) => Math.max(0, v - 1));
      return;
    }
    if (name === "h" || name === "l") {
      const cur = sidebarEntries[sidebarIdx];
      if (cur && cur.kind === "project") handleToggle(cur.id);
      return;
    }
    if (name === "return" || name === "enter" || seq === "\r" || seq === "\n") {
      const cur = sidebarEntries[sidebarIdx];
      if (!cur) return;
      if (cur.kind === "session") void handleSelect(cur.id);
      else if (cur.kind === "project") handleToggle(cur.id);
      return;
    }
    if (name === "n") {
      void handleNew();
    }
  });
  return (
    <box flexDirection="column" padding={1}>
      <text style={{ fg: statusColor }}>{statusLine}</text>
      {status === "connecting" && !authError ? <text style={{ fg: palette.warning }}>Connecting to kernel...</text> : null}
      {authError ? <text style={{ fg: palette.error }}>Authentication failed (403)</text> : null}
      {permReq ? (
        <PermissionDialog
          tool={permReq.tool}
          args={permReq.args}
          sessionId={permReq.session_id ?? permReq.sessionId ?? null}
          requestId={permReq.request_id}
          onRespond={handlePermRespond}
          onClose={clearPerm}
        />
      ) : null}
      {paletteCtrl.modelOpen ? (
        <ModelSelector default_model={modelConfig.default_model} providers={modelConfig.providers} />
      ) : null}
      {paletteCtrl.paletteQuery !== null && !paletteCtrl.modelOpen ? (
        <Palette
          query={paletteCtrl.paletteQuery}
          onSelect={paletteCtrl.handleSelect}
          onClose={paletteCtrl.handleClose}
        />
      ) : null}
      <box flexDirection="row" flexGrow={1}>
        <Sidebar
          projects={projects}
          sessions={sidebarSessions}
          activeId={st.activeId}
          collapsed={collapsed}
          onToggle={handleToggle}
          onSelect={handleSelect}
          width={SIDEBAR_WIDTH}
          view={sidebarView}
          filesProps={{
            nodes: fileTree.nodes,
            selected: filesIdx,
            error: fileTree.error,
            truncated: fileTree.truncated,
            noSession: st.activeId === null,
            width: 0,
          }}
        />
        <box flexDirection="column" flexGrow={1} paddingLeft={1}>
          {status === "open" && st.activeId === null && !permReq && !paletteCtrl.modelOpen && paletteCtrl.paletteQuery === null ? (
            <SessionList history={st.history} onSelect={handleSelect} onNew={handleNew} activeId={st.activeId} />
          ) : null}
          {status === "open" && st.activeId !== null ? (
            <box flexDirection="column" flexGrow={1}>
              <text style={{ fg: palette.textMuted }}>session: {st.activeId}</text>
              {st.info ? <text style={{ fg: palette.textMuted }}>{st.info}</text> : null}
              <RunTranscript rows={transcriptRows} todos={todos.items} />
              {!permReq && !paletteCtrl.modelOpen && paletteCtrl.paletteQuery === null ? (
                <Composer onSubmit={handleSubmit} placeholder="send a message" />
              ) : null}
            </box>
          ) : null}
        </box>
      </box>
      {st.err ? <text style={{ fg: palette.error }}>{st.err}</text> : null}
    </box>
  );
}

export function App(props: AppProps) {
  return (
    <ThemeProvider>
      <AppInner {...props} />
    </ThemeProvider>
  );
}

if (import.meta.main) {
  const renderer = await createCliRenderer({ exitOnCtrlC: false });
  createRoot(renderer).render(<App />);
}
