// agentj-ui/packages/sdk/src/__tests__/files.test.ts
// Ported from agentj-ui/apps/tui/src/files.test.ts — the shared module is the single
// source for both the TUI and the Web file tree.
import { describe, expect, it } from "vitest";
import {
  MAX_INDENT_DEPTH,
  buildFileNode,
  buildSearchIndex,
  clampSelection,
  flattenVisible,
  formatFileRowParts,
  formatSearchRowParts,
  isEmptyExpandedDir,
  moveSelection,
  parentPath,
  statusLetterColor,
  toGitLetter,
  truncate,
  updateNode,
} from "../files.ts";
import type { FileNode, RawFileEntry } from "../files.ts";

const n = (name: string): FileNode => ({
  name,
  path: name,
  kind: "file",
  gitStatus: null,
  expanded: false,
  loading: false,
  error: null,
  truncated: false,
  children: null,
});

function node(
  partial: Partial<FileNode> & { name: string; path: string; kind: "file" | "dir" },
): FileNode {
  return {
    gitStatus: null,
    expanded: false,
    loading: false,
    error: null,
    truncated: false,
    children: null,
    ...partial,
  };
}

describe("files", () => {
  it("parentPath stops at the root boundary", () => {
    expect(parentPath("a")).toBeNull();
    expect(parentPath("a/b")).toBe("a");
  });

  it("updateNode is immutable and keeps identity for untouched nodes", () => {
    const nodes = [n("a"), n("b")];
    const next = updateNode(nodes, "a", (x) => ({ ...x, expanded: true }));
    expect(next).not.toBe(nodes);
    expect(next[1]).toBe(nodes[1]);
    expect(nodes[0].expanded).toBe(false);
  });

  it("drops the dim parent before truncating the name", () => {
    // width 12 -> available 10 -> nameBudget = 10 - " deep/dir/".length = 0 < 2, so the parent is dropped
    const row = {
      name: "averylongfilename.ts",
      path: "deep/dir/averylongfilename.ts",
      kind: "file" as const,
      gitStatus: null,
    };
    const parts = formatSearchRowParts(row, 12, false);
    expect(parts.body).not.toContain("deep/dir/");
    expect(parts.body).toContain("…");
  });

  it("keeps the TUI selection semantics (count<=0 -> 0)", () => {
    expect(clampSelection(3, 0)).toBe(0);
    expect(moveSelection(1, 1, 2)).toBe(1);
    expect(moveSelection(0, -1, 3)).toBe(0);
    expect(moveSelection(2, 1, 3)).toBe(2);
    // Non-boundary moves land exactly where the delta says (clamped only at the ends).
    expect(moveSelection(1, 1, 3)).toBe(2);
    expect(moveSelection(1, -1, 3)).toBe(0);
    expect(moveSelection(1, 0, 3)).toBe(1);
    expect(clampSelection(5, 0)).toBe(0);
  });

  it("truncate only shortens what overflows", () => {
    expect(truncate("abc", 5)).toBe("abc");
    expect(truncate("abcdef", 4)).toBe("abc…");
    expect(truncate("abcdef", 0)).toBe("a…"); // slice keeps at least one char
  });
});

describe("flattenVisible", () => {
  it("walks expanded dirs depth-first", () => {
    const tree = [
      node({
        name: "src",
        path: "src",
        kind: "dir",
        expanded: true,
        children: [
          node({ name: "main.rs", path: "src/main.rs", kind: "file" }),
          node({ name: "util.rs", path: "src/util.rs", kind: "file" }),
        ],
      }),
    ];
    const rows = flattenVisible(tree);
    expect(rows.map((r) => [r.node.path, r.depth])).toEqual([
      ["src", 0],
      ["src/main.rs", 1],
      ["src/util.rs", 1],
    ]);
  });

  it("collapsed dirs hide loaded children", () => {
    const tree = [
      node({
        name: "src",
        path: "src",
        kind: "dir",
        expanded: false,
        children: [node({ name: "main.rs", path: "src/main.rs", kind: "file" })],
      }),
    ];
    expect(flattenVisible(tree)).toHaveLength(1);
  });

  it("note rows cover empty, truncated and errored expanded dirs", () => {
    const truncated = node({
      name: "src",
      path: "src",
      kind: "dir",
      expanded: true,
      truncated: true,
      children: [node({ name: "main.rs", path: "src/main.rs", kind: "file" })],
    });
    const rows = flattenVisible([truncated]);
    expect(rows.map((r) => r.note?.text ?? r.node.name)).toEqual(["src", "main.rs", "… (truncated)"]);
    expect(rows[2].note?.kind).toBe("muted");
    expect(rows[2].depth).toBe(1);

    const errored = node({ name: "bad", path: "bad", kind: "dir", expanded: true, error: "boom", children: [] });
    expect(flattenVisible([errored])[1].note).toEqual({ text: "error: boom", kind: "error" });

    const firstLoadFailed = node({
      name: "gone",
      path: "gone",
      kind: "dir",
      expanded: true,
      error: "PathNotAllowed",
      children: null,
    });
    expect(flattenVisible([firstLoadFailed])[1].note).toEqual({
      text: "error: PathNotAllowed",
      kind: "error",
    });

    const empty = node({ name: "empty", path: "empty", kind: "dir", expanded: true, children: [] });
    expect(flattenVisible([empty])[1].note).toEqual({ text: "(empty)", kind: "muted" });

    expect(isEmptyExpandedDir(empty)).toBe(true);
    expect(
      isEmptyExpandedDir(node({ name: "loading", path: "loading", kind: "dir", expanded: true, children: null })),
    ).toBe(false);
    expect(isEmptyExpandedDir(node({ name: "collapsed", path: "collapsed", kind: "dir", children: [] }))).toBe(false);
  });
});

describe("row formatting", () => {
  it("caps indent, right-aligns the letter and keeps it intact", () => {
    const deep = node({
      name: "averyveryverylongfilename.ts",
      path: "d/e/f/averyveryverylongfilename.ts",
      kind: "file",
      gitStatus: "M",
    });
    const parts = formatFileRowParts({ node: deep, depth: 8 }, 28, true);
    expect(parts.letter).toBe("M");
    expect(parts.body).toHaveLength(26);
    expect(parts.body.startsWith("● ")).toBe(true);
    expect(parts.body).toContain("…");
    expect(parts.body.slice(2, 14)).toBe(" ".repeat(12)); // 6 levels max
  });

  it("buildFileNode normalizes raw entries", () => {
    const built = buildFileNode({ name: "src", path: "src", kind: "dir", git_status: "M" });
    expect(built.gitStatus).toBe("M");
    expect(built.children).toBeNull();
    expect(built.expanded).toBe(false);
    expect(built.truncated).toBe(false);
  });

  it("maps status letters to palette colors", () => {
    const palette = { warning: "w", success: "s", textCaption: "c", error: "e" };
    expect(statusLetterColor("M", palette)).toBe("w");
    expect(statusLetterColor("A", palette)).toBe("s");
    expect(statusLetterColor("U", palette)).toBe("c");
    expect(statusLetterColor("D", palette)).toBe("e");
    expect(statusLetterColor(null, palette)).toBeNull();
    expect(toGitLetter("X")).toBeNull();
    expect(toGitLetter(undefined)).toBeNull();
  });
});

describe("search helpers", () => {
  const raw = (name: string, path: string, gitStatus?: string | null): RawFileEntry => ({
    name,
    path,
    kind: "file",
    git_status: gitStatus,
  });

  it("buildSearchIndex keeps wire order and normalizes git status", () => {
    const index = buildSearchIndex([
      { name: "src", path: "src", kind: "dir", git_status: "M" },
      raw("main.rs", "src/main.rs"),
      raw("lib.rs", "src/lib.rs", "X"),
      raw("gone.rs", "src/gone.rs", "D"),
    ]);
    expect(index.map((e) => e.path)).toEqual(["src", "src/main.rs", "src/lib.rs", "src/gone.rs"]);
    expect(index[0]).toEqual({ name: "src", path: "src", kind: "dir", gitStatus: "M" });
    expect(index[1].gitStatus).toBeNull();
    expect(index[2].gitStatus).toBeNull();
    expect(index[3].gitStatus).toBe("D");
  });

  it("formatSearchRowParts fits the budget, truncates the name and keeps the letter", () => {
    const [long] = buildSearchIndex([
      {
        name: "averyveryverylongfilename.ts",
        path: "src/deep/averyveryverylongfilename.ts",
        kind: "file",
        git_status: "M",
      },
    ]);
    const parts = formatSearchRowParts(long, 28, true);
    const budget = parts.letter ? 26 : 28; // 2 columns reserved for the " M" letter slot
    expect(parts.letter).toBe("M");
    expect(parts.body.length).toBeLessThanOrEqual(budget);
    expect(parts.body.startsWith("● ")).toBe(true);
    expect(parts.body).toContain("…");
    expect(parts.body).not.toContain("longfilename");
    expect(parts.body).toContain("src/deep/");

    const narrow = formatSearchRowParts(long, 12, false);
    expect(narrow.letter).toBe("M");
    expect(narrow.body.length).toBeLessThanOrEqual(10);
    expect(narrow.body.startsWith("  ")).toBe(true);
    expect(narrow.body).toContain("…");

    const [short] = buildSearchIndex([raw("main.md", "docs/main.md")]);
    const plain = formatSearchRowParts(short, 28, true);
    expect(plain.letter).toBeNull();
    expect(plain.body).toBe("● main.md docs/".padEnd(28));
    expect(plain.body.length).toBeLessThanOrEqual(28);
  });

  it("pins MAX_INDENT_DEPTH", () => {
    expect(MAX_INDENT_DEPTH).toBe(6);
  });

  it("patches a nested node without touching siblings", () => {
    const tree = [buildFileNode({ name: "src", path: "src", kind: "dir" }), n("README.md")];
    tree[0] = { ...tree[0], children: [buildFileNode({ name: "main.rs", path: "src/main.rs", kind: "file" })] };
    const next = updateNode(tree, "src/main.rs", (x) => ({ ...x, gitStatus: "M" }));
    expect(next[0].children?.[0].gitStatus).toBe("M");
    expect(next[1]).toBe(tree[1]);
    expect(tree[0].children?.[0].gitStatus).toBeNull();
  });
});
