// fileTreeSearch.test.ts — the reply guard, the reply reducer and the header line that keep
// stale search results off screen.
import { expect, test } from "vitest";
import {
  IDLE_SEARCH,
  SEARCH_UNSUPPORTED_HINT,
  applySearchReply,
  replyIsCurrent,
  searchHead,
  toSearchEntry,
} from "./fileTreeSearch";

test("accepts a reply from the latest request of the current session", () => {
  expect(replyIsCurrent({ seq: 7, session: "s1" }, { seq: 7, session: "s1" })).toBe(true);
});

test("rejects a reply whose request was superseded", () => {
  expect(replyIsCurrent({ seq: 6, session: "s1" }, { seq: 7, session: "s1" })).toBe(false);
});

test("rejects a reply from another session even when seq and query still match", () => {
  // Same seq happens on a clean session switch: nothing else distinguishes the two requests.
  expect(replyIsCurrent({ seq: 7, session: "s1" }, { seq: 7, session: "s2" })).toBe(false);
  // A pending session (no owner yet) is not the same as a real one either.
  expect(replyIsCurrent({ seq: 7, session: null }, { seq: 7, session: "s1" })).toBe(false);
});

test("applySearchReply reports the unsupported hint and clears rows when total is missing", () => {
  const next = applySearchReply(
    { ...IDLE_SEARCH, active: true, query: "ma" },
    { entries: [{ path: "src/main.rs" }] },
  );
  expect(next.rows).toEqual([]);
  expect(next.total).toBe(0);
  expect(next.notice).toBe(SEARCH_UNSUPPORTED_HINT);
});

test("applySearchReply keeps server order and the truncated flag", () => {
  const next = applySearchReply(
    { ...IDLE_SEARCH, active: true, query: "main", loading: true },
    {
      entries: [
        { name: "domain.rs", path: "crates/domain.rs", kind: "file", git_status: "D" },
        { name: "main.ts", path: "src/main.ts", kind: "file", git_status: "M" },
      ],
      total: 500,
      truncated: true,
    },
  );
  expect(next.rows.map((row) => row.path)).toEqual(["crates/domain.rs", "src/main.ts"]);
  expect(next.rows[1].gitStatus).toBe("M");
  expect(next.total).toBe(500);
  expect(next.truncated).toBe(true);
  expect(next.loading).toBe(false);
  expect(next.notice).toBeNull();
});

test("toSearchEntry narrows git_status through the shared normalizer", () => {
  expect(toSearchEntry({ name: "a", path: "a", kind: "dir", git_status: "X" })).toEqual({
    name: "a",
    path: "a",
    kind: "dir",
    git_status: "X",
  });
  expect(toSearchEntry({})).toEqual({ name: "", path: "", kind: "file", git_status: null });
});

test("searchHead summarises a successful reply", () => {
  const row = { name: "a", path: "a", kind: "file", gitStatus: null } as const;
  expect(searchHead({ ...IDLE_SEARCH, active: true, query: "a", rows: [row], total: 12 })).toBe(
    "搜索到 1 / 12 项",
  );
  expect(
    searchHead({ ...IDLE_SEARCH, active: true, query: "a", rows: [row], total: 12, truncated: true }),
  ).toBe("搜索到 1 / 12 项 · 部分结果");
  expect(searchHead({ ...IDLE_SEARCH, active: true, query: "a", loading: true })).toBe("搜索中…");
  expect(searchHead({ ...IDLE_SEARCH, active: true, query: "  " })).toBe("输入关键字搜索文件名");
  expect(searchHead({ ...IDLE_SEARCH, active: true, notice: "x" })).toBe("x");
});
