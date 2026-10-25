// fileTreeModel.test.ts — the pure model helpers behind the web file tree.
import { describe, expect, it } from "vitest";
import type { FileNode } from "../demoData";
import { ROOT, collectStats, flattenDemo, normalizeEntry, toView } from "./fileTreeModel";
import { visiblePaths } from "./fileTreeNav";

const dir = (path: string, children: FileNode[]): FileNode => ({ name: path, path, kind: "dir", children });
const file = (path: string, git: FileNode["git"] = null): FileNode => ({ name: path, path, kind: "file", git });

const demoTree = (): FileNode[] => [
  dir("src", [file("src/main.rs", "M")]),
  file("README.md"),
];

describe("flattenDemo", () => {
  it("keys every level by parent path", () => {
    const map = flattenDemo(demoTree());
    expect(map.get(ROOT)?.map((v) => v.path)).toEqual(["src", "README.md"]);
    expect(map.get("src")?.map((v) => v.path)).toEqual(["src/main.rs"]);
    expect(map.get("src/main.rs")).toBeUndefined();
  });

  it("hides collapsed subtree children and counts rows", () => {
    const map = flattenDemo(demoTree());
    expect(visiblePaths(map, new Set())).toEqual(["src", "README.md"]);
    expect(visiblePaths(map, new Set(["src"]))).toEqual(["src", "src/main.rs", "README.md"]);
    // The row count the footer reports equals what the flattened map holds.
    expect(collectStats(map)).toEqual({ files: 2, changes: 1 });
  });
});

describe("collectStats", () => {
  it("keeps U out of the change count", () => {
    const map = flattenDemo([file("a.ts", "U"), file("b.ts", "A"), file("c.ts")]);
    expect(collectStats(map)).toEqual({ files: 3, changes: 1 });
  });
});

describe("normalizeEntry", () => {
  it("applies the string and kind fallbacks", () => {
    expect(normalizeEntry({})).toEqual({ name: "", path: "", kind: "file", git: null });
    expect(normalizeEntry({ name: "a", path: "a", kind: "dir", git_status: 7 })).toEqual({
      name: "a",
      path: "a",
      kind: "dir",
      git: null,
    });
  });
});

describe("toView", () => {
  it("narrows git_status to the shared letters", () => {
    expect(toView({ name: "a", path: "a", kind: "file", git_status: "M" }).git).toBe("M");
    expect(toView({ name: "a", path: "a", kind: "file", git_status: "X" }).git).toBeNull();
  });
});
