import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { File } from "lucide-react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { DEMO_FILE_TREE } from "../demoData";
import type { FileNode } from "../demoData";
import "./filetree.css";

export type FileTreeProps = {
  client: RpcClient | null;
  sessionId: string | null;
  demo: boolean;
};

const ROOT = "";

const GIT_LETTERS = ["M", "A", "U", "D"] as const;
type GitLetter = (typeof GIT_LETTERS)[number];

/** One rendered row; `git` normalizes the wire `git_status` to M/A/U/D or null. */
type ViewEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git: GitLetter | null;
};

type RawFileEntry = {
  name?: unknown;
  path?: unknown;
  kind?: unknown;
  git_status?: unknown;
};

type FileListReply = { entries?: RawFileEntry[] | null } | null;

type FileTreeStats = {
  files: number;
  changes: number;
};

function toGit(raw: unknown): GitLetter | null {
  return typeof raw === "string" && (GIT_LETTERS as readonly string[]).includes(raw)
    ? (raw as GitLetter)
    : null;
}

function toView(entry: RawFileEntry): ViewEntry {
  return {
    name: String(entry.name ?? ""),
    path: String(entry.path ?? ""),
    kind: entry.kind === "dir" ? "dir" : "file",
    git: toGit(entry.git_status),
  };
}

/** Flattens the nested demo fixture into the same parent-path map the fetched tree uses. */
function flattenDemo(nodes: FileNode[]): Map<string, ViewEntry[]> {
  const map = new Map<string, ViewEntry[]>();
  const walk = (parent: string, list: FileNode[]): void => {
    map.set(
      parent,
      list.map((n) => ({ name: n.name, path: n.path, kind: n.kind, git: n.git ?? null })),
    );
    for (const n of list) {
      if (n.kind === "dir" && n.children) walk(n.path, n.children);
    }
  };
  walk(ROOT, nodes);
  return map;
}

/** Counts files and tracked changes over the loaded map; U stays out of the change count. */
function collectStats(entries: Map<string, ViewEntry[]>): FileTreeStats {
  const stats: FileTreeStats = { files: 0, changes: 0 };
  const walk = (parent: string): void => {
    for (const entry of entries.get(parent) ?? []) {
      if (entry.kind === "dir") {
        walk(entry.path);
        continue;
      }
      stats.files += 1;
      if (entry.git && entry.git !== "U") stats.changes += 1;
    }
  };
  walk(ROOT);
  return stats;
}

function childGroupId(path: string): string {
  return `filetree-${path}-children`;
}

type NodeListProps = {
  entries: Map<string, ViewEntry[]>;
  parent: string;
  depth: number;
  expanded: Set<string>;
  loading: Set<string>;
  errors: Map<string, string>;
  onToggle: (path: string) => void;
  onRetry: (path: string) => void;
};

function StatusCell({ git }: { git: GitLetter }) {
  return (
    <>
      <span className={`status-txt status-${git}`}>{git}</span>
      <span className={`dot dot-${git}`} aria-hidden="true" />
    </>
  );
}

/** Renders one loaded level; dirs recurse only while expanded and drive their own load state. */
function NodeList({
  entries,
  parent,
  depth,
  expanded,
  loading,
  errors,
  onToggle,
  onRetry,
}: NodeListProps) {
  const indent = depth > 0 ? ` indent-${depth}` : "";
  return (
    <>
      {(entries.get(parent) ?? []).map((node) => {
        if (node.kind === "dir") {
          const open = expanded.has(node.path);
          const groupId = childGroupId(node.path);
          const children = entries.get(node.path);
          let body: ReactNode = null;
          if (open) {
            if (loading.has(node.path)) body = <p className="aj-empty">加载中…</p>;
            else if (errors.has(node.path)) {
              body = (
                <div className="aj-empty">
                  <span>加载失败</span>
                  <button type="button" className="aj-button--ghost" onClick={() => onRetry(node.path)}>
                    重试
                  </button>
                </div>
              );
            } else if (children && children.length === 0) body = <p className="aj-empty">空目录</p>;
            else if (children) {
              body = (
                <NodeList
                  entries={entries}
                  parent={node.path}
                  depth={depth + 1}
                  expanded={expanded}
                  loading={loading}
                  errors={errors}
                  onToggle={onToggle}
                  onRetry={onRetry}
                />
              );
            }
          }
          return (
            <div className="trow-group" key={node.path}>
              <button
                type="button"
                className={`trow${indent} trow-dir`}
                aria-expanded={open}
                aria-controls={groupId}
                onClick={() => onToggle(node.path)}
              >
                <span className="chev" aria-hidden="true">{open ? "▾" : "▸"}</span>
                <span className="name">{node.name}</span>
              </button>
              <div id={groupId}>{body}</div>
            </div>
          );
        }
        return (
          <div className={`trow${indent}`} key={node.path}>
            <span className="chev" aria-hidden="true" />
            <File className="file-icon" size={12} aria-hidden="true" />
            <span className="name">{node.name}</span>
            {node.git ? <StatusCell git={node.git} /> : null}
          </div>
        );
      })}
    </>
  );
}

/**
 * Left-panel 文件 tab body: lazily loads directories via `file/list`, caches children,
 * renders git markers and footer stats. Demo mode uses fixtures with zero requests.
 */
export default function FileTree({ client, sessionId, demo }: FileTreeProps) {
  const [fetched, setFetched] = useState<Map<string, ViewEntry[]>>(new Map());
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Map<string, string>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const demoEntries = useMemo(() => flattenDemo(DEMO_FILE_TREE), []);
  const entries = demo ? demoEntries : fetched;

  const loadDir = useCallback(
    async (path: string): Promise<void> => {
      if (!client || !sessionId) return;
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
        const list = (reply?.entries ?? []).map(toView);
        setFetched((prev) => new Map(prev).set(path, list));
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setErrors((prev) => new Map(prev).set(path, message));
      } finally {
        setLoading((prev) => {
          const next = new Set(prev);
          next.delete(path);
          return next;
        });
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
    if (demo || !client || !sessionId) return;
    void loadDir(ROOT);
  }, [demo, client, sessionId, loadDir]);

  const toggle = (path: string): void => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (!demo && !fetched.has(path) && !loading.has(path)) void loadDir(path);
  };

  const stats = collectStats(entries);
  const rootEntries = entries.get(ROOT);

  let body: ReactNode;
  if (demo) {
    body =
      rootEntries && rootEntries.length > 0 ? (
        <div className="tree">
          <NodeList
            entries={entries}
            parent={ROOT}
            depth={0}
            expanded={expanded}
            loading={loading}
            errors={errors}
            onToggle={toggle}
            onRetry={(p) => void loadDir(p)}
          />
        </div>
      ) : (
        <p className="aj-empty">空目录</p>
      );
  } else if (!sessionId) {
    body = <p className="aj-empty">未选择会话</p>;
  } else if (loading.has(ROOT)) {
    body = <p className="aj-empty">加载中…</p>;
  } else if (errors.has(ROOT)) {
    body = (
      <div className="aj-empty">
        <span>加载失败</span>
        <button type="button" className="aj-button--ghost" onClick={() => void loadDir(ROOT)}>
          重试
        </button>
      </div>
    );
  } else if (rootEntries && rootEntries.length > 0) {
    body = (
      <div className="tree">
        <NodeList
          entries={entries}
          parent={ROOT}
          depth={0}
          expanded={expanded}
          loading={loading}
          errors={errors}
          onToggle={toggle}
          onRetry={(p) => void loadDir(p)}
        />
      </div>
    );
  } else if (rootEntries) {
    body = <p className="aj-empty">空目录</p>;
  } else {
    body = <p className="aj-empty">加载中…</p>;
  }

  return (
    <div className="filetree">
      {body}
      <div className="left-foot">
        <span>{stats.files} 个文件 · {stats.changes} 处变更</span>
        <span>⤢ 展开全部</span>
      </div>
    </div>
  );
}
