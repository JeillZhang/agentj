// agentj-ui/apps/tui/src/files.test.ts
import { describe, expect, test } from "bun:test";
import {
  buildFileNode, buildSearchIndex, clampSelection, filterEntries, flattenVisible,
  formatFileRowParts, formatSearchRowParts, isEmptyExpandedDir, moveSelection, parentPath,
  statusLetterColor, toGitLetter, updateNode,
} from "./files.ts";
import type { FileNode, RawFileEntry } from "./files.ts";

function node(
  partial: Partial<FileNode> & { name: string; path: string; kind: "file" | "dir" },
): FileNode {
  return {
    gitStatus: null, expanded: false, loading: false, error: null,
    truncated: false, children: null, ...partial,
  };
}

describe("files helpers", () => {
  test("flattenVisible walks expanded dirs depth-first", () => {
    const tree = [node({ name: "src", path: "src", kind: "dir", expanded: true, children: [
      node({ name: "main.rs", path: "src/main.rs", kind: "file" }),
      node({ name: "util.rs", path: "src/util.rs", kind: "file" }),
    ] })];
    const rows = flattenVisible(tree);
    expect(rows.map((r) => [r.node.path, r.depth])).toEqual([
      ["src", 0], ["src/main.rs", 1], ["src/util.rs", 1],
    ]);
  });

  test("collapsed dirs hide loaded children", () => {
    const tree = [node({ name: "src", path: "src", kind: "dir", expanded: false, children: [
      node({ name: "main.rs", path: "src/main.rs", kind: "file" }),
    ] })];
    expect(flattenVisible(tree)).toHaveLength(1);
  });

  test("formatFileRowParts caps indent, right-aligns the letter and keeps it intact", () => {
    const deep = node({ name: "averyveryverylongfilename.ts", path: "d/e/f/averyveryverylongfilename.ts",
      kind: "file", gitStatus: "M" });
    const parts = formatFileRowParts({ node: deep, depth: 8 }, 28, true);
    expect(parts.letter).toBe("M");
    expect(parts.body).toHaveLength(26);
    expect(parts.body.startsWith("● ")).toBe(true);
    expect(parts.body).toContain("…");
    expect(parts.body.slice(2, 14)).toBe(" ".repeat(12)); // 6 levels max
  });

  test("moveSelection clamps and empty lists stay at 0", () => {
    expect(moveSelection(0, -1, 3)).toBe(0);
    expect(moveSelection(2, 1, 3)).toBe(2);
    expect(moveSelection(1, 1, 3)).toBe(2);
    expect(clampSelection(5, 0)).toBe(0);
  });

  test("updateNode patches nested nodes and parentPath resolves", () => {
    const tree = [node({ name: "src", path: "src", kind: "dir", children: [
      node({ name: "main.rs", path: "src/main.rs", kind: "file" }),
    ] })];
    const next = updateNode(tree, "src/main.rs", (n) => ({ ...n, gitStatus: "D" }));
    expect(next[0].children?.[0].gitStatus).toBe("D");
    expect(parentPath("src/main.rs")).toBe("src");
    expect(parentPath("README.md")).toBeNull();
  });

  test("status letters map to palette colors", () => {
    const palette = { warning: "w", success: "s", textCaption: "c", error: "e" };
    expect(statusLetterColor("M", palette)).toBe("w");
    expect(statusLetterColor("A", palette)).toBe("s");
    expect(statusLetterColor("U", palette)).toBe("c");
    expect(statusLetterColor("D", palette)).toBe("e");
    expect(statusLetterColor(null, palette)).toBeNull();
    expect(toGitLetter("X")).toBeNull();
    expect(toGitLetter(undefined)).toBeNull();
  });

  test("buildFileNode normalizes raw entries", () => {
    const n = buildFileNode({ name: "src", path: "src", kind: "dir", git_status: "M" });
    expect(n.gitStatus).toBe("M");
    expect(n.children).toBeNull();
    expect(n.expanded).toBe(false);
    expect(n.truncated).toBe(false);
  });

  test("note rows cover empty, truncated and errored expanded dirs", () => {
    const truncated = node({ name: "src", path: "src", kind: "dir", expanded: true, truncated: true,
      children: [node({ name: "main.rs", path: "src/main.rs", kind: "file" })] });
    const rows = flattenVisible([truncated]);
    expect(rows.map((r) => r.note?.text ?? r.node.name)).toEqual([
      "src", "main.rs", "… (truncated)",
    ]);
    expect(rows[2].note?.kind).toBe("muted");
    expect(rows[2].depth).toBe(1);

    const errored = node({ name: "bad", path: "bad", kind: "dir", expanded: true,
      error: "boom", children: [] });
    const errorRows = flattenVisible([errored]);
    expect(errorRows[1].note).toEqual({ text: "error: boom", kind: "error" });

    const firstLoadFailed = node({ name: "gone", path: "gone", kind: "dir", expanded: true,
      error: "PathNotAllowed", children: null });
    expect(flattenVisible([firstLoadFailed])[1].note)
      .toEqual({ text: "error: PathNotAllowed", kind: "error" });

    const empty = node({ name: "empty", path: "empty", kind: "dir", expanded: true, children: [] });
    expect(flattenVisible([empty])[1].note).toEqual({ text: "(empty)", kind: "muted" });

    expect(isEmptyExpandedDir(empty)).toBe(true);
    expect(
      isEmptyExpandedDir(node({ name: "loading", path: "loading", kind: "dir", expanded: true, children: null })),
    ).toBe(false);
    expect(isEmptyExpandedDir(node({ name: "collapsed", path: "collapsed", kind: "dir", children: [] }))).toBe(false);
  });
});

describe("file search helpers", () => {
  const raw = (name: string, path: string, gitStatus?: string | null): RawFileEntry =>
    ({ name, path, kind: "file", git_status: gitStatus });

  test("buildSearchIndex keeps wire order and normalizes git status", () => {
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

  test("filters case-insensitively with name matches ranked first", () => {
    const index = buildSearchIndex([
      raw("main.md", "docs/main.md"),
      raw("main.rs", "src/main.rs"),
      raw("lib.rs", "src/lib.rs"),
    ]);
    const out = filterEntries(index, "MAIN", 10);
    expect(out.rows.map((r) => r.path)).toEqual(["docs/main.md", "src/main.rs"]);
    expect(out.total).toBe(2);
    expect(filterEntries(index, "", 10)).toEqual({ rows: [], total: 0 });
  });

  test("ranks name prefix > name substring > path substring, ties broken by path asc", () => {
    const index = buildSearchIndex([
      raw("index.ts", "src/auth/index.ts"),
      raw("my-auth-helper.ts", "src/helpers/my-auth-helper.ts"),
      raw("auth.ts", "src/auth.ts"),
      raw("auth.test.ts", "tests/auth.test.ts"),
      raw("other.ts", "src/other.ts"),
    ]);
    const out = filterEntries(index, "AUTH", 10);
    expect(out.rows.map((r) => r.path)).toEqual([
      "src/auth.ts",
      "tests/auth.test.ts",
      "src/helpers/my-auth-helper.ts",
      "src/auth/index.ts",
    ]);
    expect(out.total).toBe(4);
  });

  test("caps rows at the limit while total keeps the full match count", () => {
    const index = buildSearchIndex([
      raw("file-1.ts", "src/file-1.ts"),
      raw("file-2.ts", "src/file-2.ts"),
      raw("file-3.ts", "src/file-3.ts"),
      raw("file-4.ts", "src/file-4.ts"),
      raw("file-5.ts", "src/file-5.ts"),
    ]);
    const out = filterEntries(index, "file", 2);
    expect(out.rows.map((r) => r.path)).toEqual(["src/file-1.ts", "src/file-2.ts"]);
    expect(out.total).toBe(5);
    expect(filterEntries(index, "file", 0).rows).toEqual([]);
    expect(filterEntries(index, "file", 0).total).toBe(5);
  });

  test("formatSearchRowParts fits the budget, truncates the name and keeps the letter", () => {
    const [long] = buildSearchIndex([
      { name: "averyveryverylongfilename.ts", path: "src/deep/averyveryverylongfilename.ts",
        kind: "file", git_status: "M" },
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
});
