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
import { SubscriptionOverlay } from "./subscription.tsx";
import { ModelSelector, useModelConfig } from "./models.tsx";
import type { ModelEntry } from "./models.tsx";
import { ThemeProvider, useTheme } from "./theme.tsx";
import Sidebar, { SIDEBAR_WIDTH, loadCollapsed, loadProjects, saveCollapsed } from "./sidebar.tsx";
import type { Project, SidebarSession, SidebarView } from "./sidebar.tsx";
import { useFileTree } from "./files.tsx";
import { DEFAULT_PROJECT_KEY } from "./filetreePrefs.ts";
import { buildSearchIndex, clampSelection, flattenVisible, moveSelection, parentPath } from "./files.ts";
import type { RawFileEntry, SearchEntry } from "./files.ts";
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

export type SearchState = {
  query: string;
  rows: SearchEntry[];
  total: number;
  truncated: boolean;
  error: string | null;
  notice: string | null;
  idx: number;
  mode: "unknown" | "server" | "unsupported";
};

export type FileListSearchReply = { entries?: RawFileEntry[]; total?: number; truncated?: boolean };

export const SEARCH_UNSUPPORTED_HINT =
  "kernel does not support server-side search · upgrade the kernel";
const SEARCH_DEBOUNCE_MS = 150;

// A reply carrying `total` is a server-ranked search. Without it the kernel is too
// old to understand the query, so the TUI reports that instead of re-ranking the
// listing locally — the kernel is the single ranking authority.
export function applySearchReply(
  state: SearchState,
  query: string,
  reply: FileListSearchReply,
): SearchState {
  if (typeof reply.total === "number") {
    return {
      ...state,
      query,
      mode: "server",
      rows: buildSearchIndex(reply.entries ?? []),
      total: reply.total,
      truncated: Boolean(reply.truncated),
      idx: 0,
      error: null,
      notice: null,
    };
  }
  return {
    ...state,
    query,
    mode: "unsupported",
    rows: [],
    total: 0,
    truncated: false,
    idx: 0,
    error: null,
    notice: SEARCH_UNSUPPORTED_HINT,
  };
}

export function initialSearchState(): SearchState {
  return {
    query: "",
    rows: [],
    total: 0,
    truncated: false,
    error: null,
    notice: null,
    idx: 0,
    mode: "unknown",
  };
}

type SearchKey =
  | { kind: "clear" }
  | { kind: "exit" }
  | { kind: "backspace" }
  | { kind: "move"; delta: number }
  | { kind: "jump"; path: string }
  | { kind: "rebuild" }
  | { kind: "append"; ch: string }
  | { kind: "swallow" };

// Search mode swallows every printable (D10), so mapped keys are matched first
// and everything else becomes query text. null = Tab keeps its focus switching.
function searchKey(
  key: { name?: string; sequence?: string; ctrl?: boolean; meta?: boolean },
  search: Pick<SearchState, "query" | "rows" | "idx">,
): SearchKey | null {
  const name = (key.name ?? "").toLowerCase();
  const seq = key.sequence ?? "";
  if (name === "tab" || seq === "\t" || seq === "\x09") return null;
  if (name === "escape" || seq === "\u001B" || seq === "\x1b") {
    return search.query.length > 0 ? { kind: "clear" } : { kind: "exit" };
  }
  if (key.ctrl || key.meta) return { kind: "swallow" };
  if (name === "backspace") return { kind: "backspace" };
  if (name === "j" || name === "down" || seq === "\u001B[B") return { kind: "move", delta: 1 };
  if (name === "k" || name === "up" || seq === "\u001B[A") return { kind: "move", delta: -1 };
  if (name === "return" || name === "enter" || seq === "\r" || seq === "\n") {
    const row = search.rows[search.idx];
    return row ? { kind: "jump", path: row.path } : { kind: "swallow" };
  }
  if (name === "r") return { kind: "rebuild" };
  if (seq.length === 1) return { kind: "append", ch: seq };
  return { kind: "swallow" };
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
  const session = useSession(client);
  const { createNew, continueSession } = session;
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
  return { handleSelect, handleNew, projectRoot: session.projectRoot };
}

function getStatusLine(authError: string | null, status: TransportStatus): string {
  if (authError) return `auth error: ${authError}`;
  if (status === "open") return "connected";
  if (status === "closed") return "kernel unreachable";
  return "connecting...";
}
function usePaletteControl(onThemeToggle?: () => void, disabled = false) {
  const [paletteQuery, setPaletteQuery] = useState<string | null>(null);
  const [modelOpen, setModelOpen] = useState(false);
  const [subOpen, setSubOpen] = useState(false);
  const handleSelect = useCallback(
    (cmd: string) => {
      if (cmd === "subscription") {
        setSubOpen(true);
        setPaletteQuery(null);
        return;
      }
      if (cmd === "models") setModelOpen(true);
      if (cmd === "themes") onThemeToggle?.();
      setPaletteQuery(null);
    },
    [onThemeToggle],
  );
  const handleClose = useCallback(() => setPaletteQuery(null), []);
  const handleModelClose = useCallback(() => setModelOpen(false), []);
  const handleSubClose = useCallback(() => setSubOpen(false), []);
  useKeyboard((key) => {
    if (disabled) return;
    if (subOpen) return;
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
  return { paletteQuery, modelOpen, subOpen, handleSelect, handleClose, handleModelClose, handleSubClose };
}

function AppInner({ url, createTransport, backoffBaseMs = 1000 }: AppProps) {
  useCtrlCExit();
  const { status, authError, client } = useConnection(url, createTransport, backoffBaseMs);
  const st = useSessionState();
  const session = useAppHandlers(client, st);
  const { handleSelect, handleNew } = session;
  const { pending: permReq, respond: handlePermRespond, clear: clearPerm } = usePermissionState(client);
  const { toggle: toggleTheme, palette } = useTheme();
  const statusLine = getStatusLine(authError, status);
  const modelConfig = useModelConfig(client);
  const [selectedModel, setSelectedModel] = useState<ModelEntry | null>(null);
  const [transcriptRows, setTranscriptRows] = useState<TranscriptRow[]>([]);
  const [todos, setTodos] = useState<TodoState>(EMPTY_TODOS);
  const [sidebarView, setSidebarView] = useState<SidebarView>("threads");
  const [filesIdx, setFilesIdx] = useState(0);
  const [focus, setFocus] = useState<"composer" | "sidebar">("composer");
  const [projects] = useState<Project[]>(() => loadProjects());
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(() => loadCollapsed());
  const sidebarSessions = useMemo<SidebarSession[]>(
    () => st.history.map((id) => ({ id, preview: id })),
    [st.history],
  );
  // G12/D11: browse state keys off the session's effective filesystem root
  // (the file/list sandbox), shared by same-root sessions; unknown → __default__.
  const activeProjectKey = session.projectRoot ?? DEFAULT_PROJECT_KEY;
  const fileTree = useFileTree(client, sidebarView === "files" ? st.activeId : null, activeProjectKey);
  const filesRows = useMemo(() => flattenVisible(fileTree.nodes ?? []), [fileTree.nodes]);
  const [search, setSearch] = useState<SearchState | null>(null);
  const searchRef = useRef<SearchState | null>(null);
  const searchQueryRef = useRef("");
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const revealRef = useRef<string | null>(null);
  const paletteCtrl = usePaletteControl(toggleTheme, search !== null);
  useEffect(() => {
    searchRef.current = search;
    if (search === null) searchQueryRef.current = "";
  }, [search]);
  useEffect(() => {
    setFilesIdx((v) => clampSelection(v, filesRows.length));
  }, [filesRows.length]);
  useEffect(() => {
    setFilesIdx(0);
  }, [st.activeId]);
  const failReveal = useCallback((): void => {
    revealRef.current = null;
    st.setErr("path no longer exists");
    void fileTree.loadRoot();
  }, [fileTree, st.setErr]);
  const advanceReveal = useCallback((): void => {
    const target = revealRef.current;
    if (!target || fileTree.nodes === null) return;
    const idx = filesRows.findIndex((row) => row.node.path === target);
    if (idx >= 0) {
      setFilesIdx(idx);
      revealRef.current = null;
      const node = filesRows[idx].node;
      if (node.kind === "dir" && !node.expanded) fileTree.toggle(node);
      return;
    }
    const parts = target.split("/");
    for (let i = 1; i < parts.length; i += 1) {
      const prefix = parts.slice(0, i).join("/");
      const row = filesRows.find((r) => r.node.path === prefix);
      if (!row || row.node.kind !== "dir" || row.node.error) {
        failReveal();
        return;
      }
      if (row.node.loading) return;
      if (!row.node.expanded) {
        fileTree.toggle(row.node);
        return;
      }
      if (row.node.children === null) return;
    }
    failReveal();
  }, [failReveal, fileTree, filesRows]);
  useEffect(() => {
    advanceReveal();
  }, [advanceReveal]);
  const cancelSearchDebounce = useCallback((): void => {
    if (searchTimerRef.current !== null) {
      clearTimeout(searchTimerRef.current);
      searchTimerRef.current = null;
    }
  }, []);
  useEffect(() => cancelSearchDebounce, [cancelSearchDebounce]);
  const runServerSearch = useCallback(
    async (query: string, force = false): Promise<void> => {
      const sid = st.activeId;
      const current = searchRef.current;
      if (!client || !sid || !current || searchQueryRef.current !== query) return;
      // A failed request stays put until `r` retries it; keystrokes must not resend it.
      if (!force && current.error !== null) return;
      try {
        const reply = (await client.request(METHODS.fileList, {
          session_id: sid,
          path: null,
          depth: 8,
          query,
        })) as FileListSearchReply;
        if (searchRef.current === null || searchQueryRef.current !== query) return;
        setSearch((prev) => (prev && prev.query === query ? applySearchReply(prev, query, reply) : prev));
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setSearch((prev) =>
          prev && prev.query === query
            ? { ...prev, rows: [], total: 0, idx: 0, error: msg, notice: null, truncated: false }
            : prev,
        );
      }
    },
    [client, st.activeId],
  );
  const applyQuery = useCallback(
    (next: string): void => {
      cancelSearchDebounce();
      searchQueryRef.current = next;
      // Whitespace-only is empty: it must not be dispatched (the kernel trims it to
      // an unranked listing, which would be misread as an old-kernel reply).
      const blank = next.trim().length === 0;
      setSearch((prev) => {
        if (!prev) return prev;
        if (blank) {
          return { ...prev, query: "", rows: [], total: 0, idx: 0, notice: null, truncated: false };
        }
        if (next === prev.query) return prev;
        return { ...prev, query: next, idx: 0, notice: null, truncated: false };
      });
      if (blank) return;
      searchTimerRef.current = setTimeout(() => {
        searchTimerRef.current = null;
        void runServerSearch(next);
      }, SEARCH_DEBOUNCE_MS);
    },
    [cancelSearchDebounce, runServerSearch],
  );
  const rebuildSearch = useCallback((): void => {
    cancelSearchDebounce();
    if (!searchRef.current) return;
    void runServerSearch(searchQueryRef.current, true);
  }, [cancelSearchDebounce, runServerSearch]);
  const startSearch = useCallback((): void => {
    if (st.activeId === null) return;
    st.setErr(null);
    cancelSearchDebounce();
    searchQueryRef.current = "";
    setSearch(initialSearchState());
  }, [st.activeId, st.setErr, cancelSearchDebounce]);
  const beginReveal = useCallback((path: string): void => {
    cancelSearchDebounce();
    setSearch(null);
    revealRef.current = path;
    advanceReveal();
  }, [advanceReveal, cancelSearchDebounce]);
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
      // D5: no picked entry sends exactly {session_id, content}; the provider
      // always travels with the picked entry (D7), never from list position.
      const params: Record<string, unknown> = { session_id: sid, content: text };
      if (selectedModel) {
        params.model = selectedModel.name;
        if (selectedModel.provider) params.provider = selectedModel.provider;
      }
      try {
        await client.request(METHODS.sessionRun, params);
      } catch (e) {
        st.setErr(e instanceof Error ? e.message : String(e));
      }
    },
    [client, st, selectedModel],
  );
  const statusColor =
    authError !== null ? palette.error : status === "open" ? palette.success : palette.warning;
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
    if (paletteCtrl.paletteQuery !== null || paletteCtrl.modelOpen || paletteCtrl.subOpen || permReq) return;
    const name = (key.name ?? "").toLowerCase();
    const seq = key.sequence ?? "";
    const isTab = name === "tab" || seq === "\t" || seq === "\x09";
    const isEsc = name === "escape" || seq === "\u001B" || seq === "\x1b";
    const composerVisible =
      status === "open" &&
      st.activeId !== null &&
      !permReq &&
      !paletteCtrl.modelOpen &&
      paletteCtrl.paletteQuery === null;
    if (focus !== "sidebar") {
      // Composer focus: only Tab leaves the composer. The mounted composer owns
      // Tab when its completion overlay is visible, so switch only when absent.
      if (isTab && !composerVisible) setFocus("sidebar");
      return;
    }
    if (search && sidebarView === "files") {
      const decision = searchKey(key, {
        query: searchQueryRef.current,
        rows: search.rows,
        idx: search.idx,
      });
      if (decision === null) {
        setFocus("composer");
        return;
      }
      switch (decision.kind) {
        case "clear":
          applyQuery("");
          return;
        case "exit":
          cancelSearchDebounce();
          setSearch(null);
          return;
        case "backspace": {
          const current = searchQueryRef.current;
          if (current.length > 0) applyQuery(current.slice(0, -1));
          return;
        }
        case "move":
          setSearch((prev) =>
            prev ? { ...prev, idx: moveSelection(prev.idx, decision.delta, prev.rows.length) } : prev);
          return;
        case "jump":
          beginReveal(decision.path);
          return;
        case "rebuild":
          rebuildSearch();
          return;
        case "append":
          applyQuery(searchQueryRef.current + decision.ch);
          return;
        default:
          return;
      }
    }
    if (isTab || isEsc) {
      setFocus("composer");
      return;
    }
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
      if (name === "s") {
        startSearch();
        return;
      }
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
        if (parent) {
          const idx = rows.findIndex((r) => r.node.path === parent);
          if (idx >= 0) setFilesIdx(idx);
        }
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
        <ModelSelector
          default_model={modelConfig.default_model}
          providers={modelConfig.providers}
          selected={selectedModel}
          onSelect={(entry) => {
            // Closing the picker is required: the global key handler swallows
            // composer typing while modelOpen stays true.
            setSelectedModel(entry);
            paletteCtrl.handleModelClose();
          }}
        />
      ) : null}
      {paletteCtrl.subOpen && client ? (
        <SubscriptionOverlay client={client} onClose={paletteCtrl.handleSubClose} />
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
            selected: search ? search.idx : filesIdx,
            error: fileTree.error,
            truncated: fileTree.truncated,
            noSession: st.activeId === null,
            width: 0,
            search: search
              ? {
                  active: true,
                  query: search.query,
                  rows: search.rows,
                  total: search.total,
                  truncated: search.truncated,
                  error: search.error,
                  notice: search.notice,
                }
              : undefined,
          }}
          focusMode={focus}
          searchMode={search ? { query: search.query } : null}
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
                <Composer
                  onSubmit={handleSubmit}
                  placeholder="send a message"
                  disabled={focus !== "composer"}
                  onFocusSidebar={() => setFocus("sidebar")}
                />
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
