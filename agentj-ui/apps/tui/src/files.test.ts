// agentj-ui/apps/tui/src/files.test.ts
import { describe, expect, test } from "bun:test";
import {
  buildFileNode, clampSelection, flattenVisible, formatFileRowParts, isEmptyExpandedDir,
  moveSelection, parentPath, statusLetterColor, toGitLetter, updateNode,
} from "./files.ts";
import type { FileNode } from "./files.ts";

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
