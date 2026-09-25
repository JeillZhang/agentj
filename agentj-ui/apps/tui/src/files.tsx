// agentj-ui/apps/tui/src/files.tsx (new)
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useTheme } from "./theme.tsx";
import {
  buildFileNode, flattenVisible, formatFileRowParts, formatSearchRowParts, MAX_INDENT_DEPTH,
  parentPath, statusLetterColor, updateNode,
} from "./files.ts";
import type { FileNode, RawFileEntry, SearchEntry } from "./files.ts";
import { DEFAULT_PROJECT_KEY, loadFileTreePrefs, saveFileTreePrefs } from "./filetreePrefs.ts";

const MAX_RESTORE_PATHS = 50;

function pathDepth(path: string): number {
  return path.split("/").length;
}

function findNode(nodes: FileNode[] | null, path: string): FileNode | null {
  for (const node of nodes ?? []) {
    if (node.path === path) return node;
    if (node.kind === "dir" && node.children) {
      const hit = findNode(node.children, path);
      if (hit) return hit;
    }
  }
  return null;
}

function ancestorPaths(path: string): string[] {
  const parts = path.split("/");
  const out: string[] = [];
  for (let i = 1; i < parts.length; i += 1) out.push(parts.slice(0, i).join("/"));
  return out;
}

export type FileListReply = { entries?: RawFileEntry[]; truncated?: boolean };

export type FilesSearchState = {
  active: boolean;
  query: string;
  rows: SearchEntry[];
  total: number;
  truncated: boolean;
  error: string | null;
};

export type FilesViewProps = {
  nodes: FileNode[] | null;
  selected: number;
  error: string | null;
  truncated: boolean;
  noSession: boolean;
  width: number;
  search?: FilesSearchState;
};

export const SEARCH_HINT = "type filter · enter jump · esc back";

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function SearchRow({ row, width, selected }: { row: SearchEntry; width: number; selected: boolean }) {
  const { palette } = useTheme();
  const parts = formatSearchRowParts(row, width, selected);
  const letterColor = statusLetterColor(row.gitStatus, palette) ?? palette.textMuted;
  const parent = parentPath(row.path);
  const suffix = parent ? ` ${parent}/` : "";
  const at = suffix ? parts.body.lastIndexOf(suffix) : -1;
  return (
    <box flexDirection="row">
      {at === -1
        ? <text style={{ fg: palette.textMuted }}>{parts.body}</text>
        : (
          <>
            <text style={{ fg: palette.text }}>{parts.body.slice(0, at)}</text>
            <text style={{ fg: palette.textMuted }}>{parts.body.slice(at)}</text>
          </>
        )}
      {parts.letter ? <text style={{ fg: letterColor }}>{parts.letter}</text> : null}
    </box>
  );
}

export function FilesView(props: FilesViewProps) {
  const { nodes, selected, error, truncated, noSession, width, search } = props;
  const { palette } = useTheme();
  const scrollRef = useRef<ScrollBoxRenderable>(null);
  const searching = search?.active === true;
  const treeRows = searching ? [] : flattenVisible(nodes ?? []);
  const rowCount = searching ? (search?.rows.length ?? 0) : treeRows.length;

  useLayoutEffect(() => {
    const target = scrollRef.current;
    if (!target) return;
    try {
      target.scrollTop = Math.max(0, selected - 2);
    } catch {}
  }, [selected, rowCount, searching]);

  if (noSession) return <text style={{ fg: palette.textMuted }}>select a session to browse files</text>;
  if (!nodes && !error && !searching) return <text style={{ fg: palette.textMuted }}>loading…</text>;
  if (searching && search) {
    const partial = search.truncated ? " · partial" : "";
    const remaining = search.total - search.rows.length;
    const more = remaining > 0 ? ` · ${remaining} more` : "";
    const hint = `${SEARCH_HINT}${partial}${more}`;
    return (
      <box flexDirection="column" flexGrow={1}>
        <text style={{ fg: palette.textMuted }}>{hint}</text>
        {search.error ? <text style={{ fg: palette.error }}>{`error: ${search.error}`}</text> : null}
        <scrollbox ref={scrollRef} flexGrow={1} style={{ flexGrow: 1 }} contentOptions={{ minHeight: 0 }}>
          {search.rows.map((row, i) => (
            <SearchRow key={row.path} row={row} width={width} selected={i === selected} />
          ))}
        </scrollbox>
      </box>
    );
  }
  const rows = treeRows;
  return (
    <box flexDirection="column" flexGrow={1}>
      {error ? <text style={{ fg: palette.error }}>{`error: ${error}`}</text> : null}
      {nodes !== null && nodes.length === 0 ? <text style={{ fg: palette.textMuted }}>(empty)</text> : null}
      <scrollbox ref={scrollRef} flexGrow={1} style={{ flexGrow: 1 }} contentOptions={{ minHeight: 0 }}>
        {rows.map((row, i) => {
          if (row.note) {
            const color = row.note.kind === "error" ? palette.error : palette.textMuted;
            const indent = "  ".repeat(Math.min(row.depth, MAX_INDENT_DEPTH) + 1);
            return <text key={`${row.node.path}#note`} style={{ fg: color }}>{`${indent}${row.note.text}`}</text>;
          }
          const parts = formatFileRowParts(row, width, i === selected);
          const nameColor = row.node.kind === "dir" ? palette.text : palette.textMuted;
          const letterColor = statusLetterColor(row.node.gitStatus, palette) ?? nameColor;
          return (
            <box key={row.node.path} flexDirection="row">
              <text style={{ fg: nameColor }}>{parts.body}</text>
              {parts.letter ? <text style={{ fg: letterColor }}>{parts.letter}</text> : null}
              {row.node.loading ? <text style={{ fg: palette.textMuted }}> …</text> : null}
            </box>
          );
        })}
        {truncated ? <text style={{ fg: palette.textMuted }}>… (truncated)</text> : null}
      </scrollbox>
    </box>
  );
}

export function useFileTree(
  client: RpcClient | null,
  sessionId: string | null,
  projectKey: string = DEFAULT_PROJECT_KEY,
) {
  const [nodes, setNodes] = useState<FileNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);
  const generation = useRef(0);
  const nodesRef = useRef<FileNode[] | null>(null);

  const setTree = useCallback((next: FileNode[] | null): void => {
    nodesRef.current = next;
    setNodes(next);
  }, []);

  const mutate = useCallback((path: string, patch: (node: FileNode) => FileNode): void => {
    setTree(updateNode(nodesRef.current ?? [], path, patch));
  }, [setTree]);

  const request = useCallback(async (path: string | null): Promise<FileListReply> => {
    if (!client || !sessionId) throw new Error("no active session");
    return (await client.request(METHODS.fileList, {
      session_id: sessionId, path, depth: 1,
    })) as FileListReply;
  }, [client, sessionId]);

  const loadDir = useCallback(async (path: string): Promise<void> => {
    const started = generation.current;
    try {
      const reply = await request(path);
      if (generation.current !== started) return;
      mutate(path, (n) => ({
        ...n, loading: false, expanded: true, error: null,
        truncated: Boolean(reply.truncated),
        children: (reply.entries ?? []).map(buildFileNode),
      }));
    } catch (e) {
      if (generation.current !== started) return;
      mutate(path, (n) => ({ ...n, loading: false, error: messageOf(e) }));
    }
  }, [request, mutate]);

  const loadRoot = useCallback(async (): Promise<void> => {
    const started = generation.current;
    setTree(null);
    setError(null);
    setTruncated(false);
    try {
      const reply = await request(null);
      if (generation.current !== started) return;
      setTree((reply.entries ?? []).map(buildFileNode));
      setTruncated(Boolean(reply.truncated));
    } catch (e) {
      if (generation.current !== started) return;
      setError(messageOf(e));
      setTree([]);
    }
  }, [request, setTree]);

  // Saves the live expanded set for this root. Old project-id keys are stale
  // (non-path) and dropped on save; root keys persist for per-root isolation.
  const persist = useCallback((tree: FileNode[] | null): void => {
    const paths: string[] = [];
    const walk = (list: FileNode[] | null): void => {
      for (const node of list ?? []) {
        if (node.kind !== "dir" || !node.expanded) continue;
        paths.push(node.path);
        if (node.children) walk(node.children);
      }
    };
    walk(tree);
    paths.sort((a, b) => pathDepth(a) - pathDepth(b)); // depth order: the 100-path cap drops the deepest
    const stored = loadFileTreePrefs().expanded;
    const liveIds = Object.keys(stored).filter(
      (key) => key === DEFAULT_PROJECT_KEY || key.startsWith("/"),
    );
    if (!liveIds.includes(projectKey)) liveIds.push(projectKey);
    saveFileTreePrefs({ expanded: { ...stored, [projectKey]: paths } }, liveIds);
  }, [projectKey]);

  const ensureLoaded = useCallback(async (path: string, gen: number): Promise<boolean> => {
    const found = findNode(nodesRef.current, path);
    if (!found || found.kind !== "dir") return false;
    if (!found.children) {
      mutate(path, (n) => ({ ...n, loading: true, expanded: true }));
      await loadDir(path);
      if (generation.current !== gen) return false;
      return Boolean(findNode(nodesRef.current, path)?.children);
    }
    if (!found.expanded) mutate(path, (n) => ({ ...n, expanded: true }));
    return true;
  }, [loadDir, mutate]);

  // Restores persisted dirs depth-ascending; paths that no longer exist are skipped silently.
  const restore = useCallback(async (gen: number): Promise<void> => {
    const saved = loadFileTreePrefs().expanded[projectKey] ?? [];
    const paths = [...new Set(saved)].sort((a, b) => pathDepth(a) - pathDepth(b));
    for (const path of paths.slice(0, MAX_RESTORE_PATHS)) {
      if (generation.current !== gen) return;
      let reachable = true;
      for (const ancestor of ancestorPaths(path)) {
        if (generation.current !== gen) return;
        reachable = await ensureLoaded(ancestor, gen);
        if (!reachable) break;
      }
      if (reachable) await ensureLoaded(path, gen);
    }
  }, [ensureLoaded, projectKey]);

  const toggle = useCallback((node: FileNode): void => {
    if (node.kind !== "dir" || node.expanded) return;
    const current = findNode(nodesRef.current, node.path);
    if (!current) return;
    if (current.children) {
      mutate(node.path, (n) => ({ ...n, expanded: true }));
    } else {
      mutate(node.path, (n) => ({ ...n, loading: true, expanded: true }));
      void loadDir(node.path);
    }
    persist(nodesRef.current);
  }, [loadDir, mutate, persist]);

  const collapse = useCallback((path: string): void => {
    mutate(path, (n) => ({ ...n, expanded: false }));
    persist(nodesRef.current);
  }, [mutate, persist]);

  const refresh = useCallback(async (): Promise<void> => {
    const expanded: string[] = [];
    const walk = (list: FileNode[] | null): void => {
      for (const n of list ?? []) {
        if (n.kind === "dir" && n.expanded && n.children) { expanded.push(n.path); walk(n.children); }
      }
    };
    walk(nodesRef.current);
    const started = generation.current;
    await loadRoot();
    for (const path of expanded) {
      if (generation.current !== started) return;
      await loadDir(path);
    }
  }, [loadRoot, loadDir]);

  useEffect(() => {
    generation.current += 1;
    const gen = generation.current;
    if (!sessionId) {
      setTree(null);
      setError(null);
      setTruncated(false);
      return;
    }
    void (async () => {
      await loadRoot();
      if (generation.current !== gen) return;
      await restore(gen);
    })();
  }, [sessionId, projectKey, loadRoot, restore, setTree]);

  return { nodes, error, truncated, toggle, collapse, refresh, loadRoot };
}
