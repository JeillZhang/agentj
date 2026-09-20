// agentj-ui/apps/tui/src/files.tsx (new)
import { useCallback, useEffect, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { useTheme } from "./theme.tsx";
import {
  buildFileNode, flattenVisible, formatFileRowParts, MAX_INDENT_DEPTH, statusLetterColor, updateNode,
} from "./files.ts";
import type { FileNode, RawFileEntry } from "./files.ts";

export type FileListReply = { entries?: RawFileEntry[]; truncated?: boolean };

export type FilesViewProps = {
  nodes: FileNode[] | null;
  selected: number;
  error: string | null;
  truncated: boolean;
  noSession: boolean;
  width: number;
};

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function FilesView({ nodes, selected, error, truncated, noSession, width }: FilesViewProps) {
  const { palette } = useTheme();
  if (noSession) return <text style={{ fg: palette.textMuted }}>select a session to browse files</text>;
  if (!nodes && !error) return <text style={{ fg: palette.textMuted }}>loading…</text>;
  const rows = flattenVisible(nodes ?? []);
  return (
    <box flexDirection="column" flexGrow={1}>
      {error ? <text style={{ fg: palette.error }}>{`error: ${error}`}</text> : null}
      {nodes !== null && nodes.length === 0 ? <text style={{ fg: palette.textMuted }}>(empty)</text> : null}
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
    </box>
  );
}

export function useFileTree(client: RpcClient | null, sessionId: string | null) {
  const [nodes, setNodes] = useState<FileNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);

  const request = useCallback(async (path: string | null): Promise<FileListReply> => {
    if (!client || !sessionId) throw new Error("no active session");
    return (await client.request(METHODS.fileList, {
      session_id: sessionId, path, depth: 1,
    })) as FileListReply;
  }, [client, sessionId]);

  const loadDir = useCallback(async (path: string): Promise<void> => {
    try {
      const reply = await request(path);
      setNodes((prev) => updateNode(prev ?? [], path, (n) => ({
        ...n, loading: false, expanded: true, error: null,
        truncated: Boolean(reply.truncated),
        children: (reply.entries ?? []).map(buildFileNode),
      })));
    } catch (e) {
      setNodes((prev) => updateNode(prev ?? [], path, (n) => ({ ...n, loading: false, error: messageOf(e) })));
    }
  }, [request]);

  const loadRoot = useCallback(async (): Promise<void> => {
    setNodes(null);
    setError(null);
    setTruncated(false);
    try {
      const reply = await request(null);
      setNodes((reply.entries ?? []).map(buildFileNode));
      setTruncated(Boolean(reply.truncated));
    } catch (e) {
      setError(messageOf(e));
      setNodes([]);
    }
  }, [request]);

  const toggle = useCallback((node: FileNode): void => {
    if (node.kind !== "dir" || node.expanded) return;
    if (node.children) {
      setNodes((prev) => updateNode(prev ?? [], node.path, (n) => ({ ...n, expanded: true })));
      return;
    }
    setNodes((prev) => updateNode(prev ?? [], node.path, (n) => ({ ...n, loading: true, expanded: true })));
    void loadDir(node.path);
  }, [loadDir]);

  const collapse = useCallback((path: string): void => {
    setNodes((prev) => updateNode(prev ?? [], path, (n) => ({ ...n, expanded: false })));
  }, []);

  const refresh = useCallback(async (): Promise<void> => {
    const expanded: string[] = [];
    const walk = (list: FileNode[] | null): void => {
      for (const n of list ?? []) {
        if (n.kind === "dir" && n.expanded && n.children) { expanded.push(n.path); walk(n.children); }
      }
    };
    walk(nodes);
    await loadRoot();
    for (const path of expanded) await loadDir(path);
  }, [nodes, loadRoot, loadDir]);

  useEffect(() => {
    if (!sessionId) {
      setNodes(null);
      setError(null);
      setTruncated(false);
      return;
    }
    void loadRoot();
  }, [sessionId, loadRoot]);

  return { nodes, error, truncated, toggle, collapse, refresh, loadRoot };
}
