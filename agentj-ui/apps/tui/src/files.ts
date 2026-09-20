// agentj-ui/apps/tui/src/files.ts (new; pure helpers only)
export type GitLetter = "M" | "A" | "U" | "D";

export type RawFileEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git_status?: string | null;
};

export type FileNode = {
  name: string;
  path: string;
  kind: "file" | "dir";
  gitStatus: GitLetter | null;
  expanded: boolean;
  loading: boolean;
  error: string | null;
  truncated: boolean;
  children: FileNode[] | null; // null = not loaded yet
};

export type VisibleRowNote = { text: string; kind: "muted" | "error" };
export type VisibleRow = { node: FileNode; depth: number; note?: VisibleRowNote };
export type RowParts = { body: string; letter: GitLetter | null };

export const MAX_INDENT_DEPTH = 6;

export function toGitLetter(raw: string | null | undefined): GitLetter | null {
  return raw === "M" || raw === "A" || raw === "U" || raw === "D" ? raw : null;
}

export function buildFileNode(raw: RawFileEntry): FileNode {
  return {
    name: raw.name,
    path: raw.path,
    kind: raw.kind,
    gitStatus: toGitLetter(raw.git_status),
    expanded: false,
    loading: false,
    error: null,
    truncated: false,
    children: null,
  };
}

export function flattenVisible(nodes: FileNode[], depth = 0): VisibleRow[] {
  const out: VisibleRow[] = [];
  for (const node of nodes) {
    out.push({ node, depth });
    if (node.kind === "dir" && node.expanded) {
      if (node.children) {
        out.push(...flattenVisible(node.children, depth + 1));
        if (node.error) {
          out.push({ node, depth: depth + 1, note: { text: `error: ${node.error}`, kind: "error" } });
        } else if (node.truncated) {
          out.push({ node, depth: depth + 1, note: { text: "… (truncated)", kind: "muted" } });
        } else if (isEmptyExpandedDir(node)) {
          out.push({ node, depth: depth + 1, note: { text: "(empty)", kind: "muted" } });
        }
      } else if (node.error) {
        // First load failed (children never arrived): surface the error inline.
        out.push({ node, depth: depth + 1, note: { text: `error: ${node.error}`, kind: "error" } });
      }
    }
  }
  return out;
}

export function isEmptyExpandedDir(node: FileNode): boolean {
  return node.kind === "dir" && node.expanded && node.children !== null && node.children.length === 0;
}

export function clampSelection(index: number, count: number): number {
  if (count <= 0) return 0;
  return Math.min(Math.max(0, index), count - 1);
}

export function moveSelection(index: number, delta: number, count: number): number {
  return clampSelection(index + delta, count);
}

export function updateNode(nodes: FileNode[], path: string, patch: (n: FileNode) => FileNode): FileNode[] {
  return nodes.map((n) => {
    if (n.path === path) return patch(n);
    if (!n.children) return n;
    const children = updateNode(n.children, path, patch);
    return children === n.children ? n : { ...n, children };
  });
}

export function parentPath(path: string): string | null {
  const idx = path.lastIndexOf("/");
  return idx <= 0 ? null : path.slice(0, idx);
}

export function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, Math.max(1, max - 1)) + "…";
}

export function formatFileRowParts(row: VisibleRow, width: number, selected: boolean): RowParts {
  const letter = row.node.gitStatus;
  const bodyWidth = Math.max(4, width - (letter ? 2 : 0));
  const indent = "  ".repeat(Math.min(row.depth, MAX_INDENT_DEPTH));
  const chevron = row.node.kind === "dir" ? (row.node.expanded ? "▼ " : "▶ ") : "";
  const marker = selected ? "● " : "  ";
  const budget = Math.max(1, bodyWidth - marker.length - indent.length - chevron.length);
  const body = `${marker}${indent}${chevron}${truncate(row.node.name, budget)}`;
  return { body: body.padEnd(bodyWidth), letter };
}

export function statusLetterColor(
  letter: GitLetter | null,
  palette: { warning: string; success: string; textCaption: string; error: string },
): string | null {
  switch (letter) {
    case "M": return palette.warning;
    case "A": return palette.success;
    case "U": return palette.textCaption;
    case "D": return palette.error;
    default: return null;
  }
}
