// useFileTree.ts — the lazy directory tree behind the web file tree: fetch/cache per path,
// expand/collapse state, the visible row list and the roving selection. No rendering.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { DEMO_FILE_TREE } from "../demoData";
import { visiblePaths } from "../components/fileTreeNav";
import { ROOT, collectStats, flattenDemo, toView } from "../components/fileTreeModel";
import type { FileListReply, FileTreeStats, ViewEntry } from "../components/fileTreeModel";
import { moveSelection } from "./useRovingFocus";

export type UseFileTreeOptions = {
  client: RpcClient | null;
  sessionId: string | null;
  demo: boolean;
};

export type UseFileTreeResult = {
  /** Parent-path map of loaded rows: demo fixtures in demo mode, the fetched cache otherwise. */
  entries: Map<string, ViewEntry[]>;
  /** Visible row paths, depth-first: a directory precedes its expanded children. */
  paths: string[];
  indexByPath: Map<string, number>;
  entryByPath: Map<string, ViewEntry>;
  stats: FileTreeStats;
  rootEntries: ViewEntry[] | undefined;
  expanded: Set<string>;
  loading: Set<string>;
  errors: Map<string, string>;
  selectedPath: string | null;
  /** Select without moving focus (focus follows the pointer / the row's own handler). */
  setSelectedPath: (path: string | null) => void;
  /** Select and focus the row (keyboard navigation). */
  selectPath: (path: string) => void;
  toggle: (path: string) => void;
  /** Re-request one directory (the row-level 重试 button). */
  reload: (path: string) => void;
  /** Reload the root plus every expanded directory. */
  refresh: () => void;
  moveBy: (delta: number) => void;
  registerRow: (path: string, el: HTMLElement | null) => void;
};

/**
 * Lazy tree state for one session. A reply is only applied while the session it was
 * requested for is still the current one, so a directory fetched in the previous session
 * never paints into the new tree.
 */
export function useFileTree({ client, sessionId, demo }: UseFileTreeOptions): UseFileTreeResult {
  const [fetched, setFetched] = useState<Map<string, ViewEntry[]>>(new Map());
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Map<string, string>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const rowRefs = useRef(new Map<string, HTMLElement>());
  /**
   * Session every in-flight tree reply must still belong to. Assigned before paint (layout
   * effect) instead of in the reset effect below: a reply can resolve in the microtask
   * window before passive effects flush and would otherwise paint the previous session's
   * rows for one frame.
   */
  const sessionRef = useRef(sessionId);
  useLayoutEffect(() => {
    sessionRef.current = sessionId;
  }, [sessionId]);

  const demoEntries = useMemo(() => flattenDemo(DEMO_FILE_TREE), []);
  const entries = demo ? demoEntries : fetched;

  const loadDir = useCallback(
    async (path: string): Promise<void> => {
      if (!client || !sessionId) return;
      const requestedFor = sessionId;
      setLoading((prev) => new Set(prev).add(path));
      setErrors((prev) => {
        const next = new Map(prev);
        next.delete(path);
        return next;
      });
      try {
        const reply = (await client.request(METHODS.fileList, {
          session_id: sessionId,
          ...(path ? { path } : {}),
          depth: 1,
        })) as FileListReply;
        if (sessionRef.current !== requestedFor) return;
        const list = (reply?.entries ?? []).map(toView);
        setFetched((prev) => new Map(prev).set(path, list));
      } catch (e) {
        if (sessionRef.current !== requestedFor) return;
        const message = e instanceof Error ? e.message : String(e);
        setErrors((prev) => new Map(prev).set(path, message));
      } finally {
        if (sessionRef.current === requestedFor) {
          setLoading((prev) => {
            const next = new Set(prev);
            next.delete(path);
            return next;
          });
        }
      }
    },
    [client, sessionId],
  );

  // Root load on mount / session change; demo mode resets to fixtures with no request.
  useEffect(() => {
    setFetched(new Map());
    setLoading(new Set());
    setErrors(new Map());
    setExpanded(new Set());
    setSelectedPath(null);
    if (demo || !client || !sessionId) return;
    void loadDir(ROOT);
  }, [demo, client, sessionId, loadDir]);

  const toggle = useCallback(
    (path: string): void => {
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });
      if (!demo && !fetched.has(path) && !loading.has(path)) void loadDir(path);
    },
    [demo, fetched, loading, loadDir],
  );

  const paths = useMemo(() => visiblePaths(entries, expanded), [entries, expanded]);
  const indexByPath = useMemo(() => new Map(paths.map((path, index) => [path, index])), [paths]);
  const entryByPath = useMemo(() => {
    const map = new Map<string, ViewEntry>();
    for (const list of entries.values()) for (const entry of list) map.set(entry.path, entry);
    return map;
  }, [entries]);

  // Drop a selection whose row disappeared (e.g. a refreshed tree no longer lists it).
  useEffect(() => {
    if (selectedPath && !indexByPath.has(selectedPath)) setSelectedPath(null);
  }, [selectedPath, indexByPath]);

  const registerRow = useCallback((path: string, el: HTMLElement | null): void => {
    if (el) rowRefs.current.set(path, el);
    else rowRefs.current.delete(path);
  }, []);

  const selectPath = useCallback((path: string): void => {
    setSelectedPath(path);
    rowRefs.current.get(path)?.focus();
  }, []);

  const moveBy = useCallback(
    (delta: number): void => {
      if (paths.length === 0) return;
      const current = selectedPath ? (indexByPath.get(selectedPath) ?? -1) : -1;
      selectPath(paths[moveSelection(current, delta, paths.length)]);
    },
    [paths, indexByPath, selectedPath, selectPath],
  );

  const reload = useCallback((path: string): void => void loadDir(path), [loadDir]);

  const refresh = useCallback((): void => {
    if (demo || !client || !sessionId) return;
    void loadDir(ROOT);
    for (const path of expanded) void loadDir(path);
  }, [demo, client, sessionId, expanded, loadDir]);

  return {
    entries,
    paths,
    indexByPath,
    entryByPath,
    stats: collectStats(entries),
    rootEntries: entries.get(ROOT),
    expanded,
    loading,
    errors,
    selectedPath,
    setSelectedPath,
    selectPath,
    toggle,
    reload,
    refresh,
    moveBy,
    registerRow,
  };
}
