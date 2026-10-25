// fileTreeModel.ts — the pure model behind the web file tree: wire normalization, the
// demo-fixture flattening and the footer stats. No React, no requests.
import { toGitLetter } from "@agentj/sdk";
import type { GitLetter } from "@agentj/sdk";
import type { FileNode } from "../demoData";

/** The map key of the tree root: `file/list` without `path` lists it. */
export const ROOT = "";

/** One rendered row; `git` is the shared `toGitLetter` normalization of `git_status`. */
export type ViewEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git: GitLetter | null;
};

export type RawFileEntry = {
  name?: unknown;
  path?: unknown;
  kind?: unknown;
  git_status?: unknown;
};

export type FileListReply = {
  entries?: RawFileEntry[] | null;
  /** Server-side search only: total hit count; absent on kernels without `query`. */
  total?: unknown;
  truncated?: unknown;
} | null;

export type FileTreeStats = {
  files: number;
  changes: number;
};

/** One wire entry with the shared string/kind fallbacks applied; `git` is still raw. */
export type NormalizedEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git: string | null;
};

/**
 * Single normalizer for wire entries: the tree rows and the search rows differ only in how
 * they narrow `git_status` (`toGitLetter` vs. the SDK's own re-narrowing), never in parsing.
 */
export function normalizeEntry(entry: RawFileEntry): NormalizedEntry {
  return {
    name: String(entry.name ?? ""),
    path: String(entry.path ?? ""),
    kind: entry.kind === "dir" ? "dir" : "file",
    git: typeof entry.git_status === "string" ? entry.git_status : null,
  };
}

export function toView(entry: RawFileEntry): ViewEntry {
  const { git, ...rest } = normalizeEntry(entry);
  return { ...rest, git: toGitLetter(git) };
}

/** Flattens the nested demo fixture into the same parent-path map the fetched tree uses. */
export function flattenDemo(nodes: FileNode[]): Map<string, ViewEntry[]> {
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
export function collectStats(entries: Map<string, ViewEntry[]>): FileTreeStats {
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
