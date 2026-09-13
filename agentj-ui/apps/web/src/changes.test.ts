import { expect, test } from "vitest";
import { applyProgressChanges, type FileChangeRow } from "./changes";
const write = (path: string, content: string) =>
  ({ kind: "tool_call", id: "c1", name: "write", arguments: { path, content } });
test("add then modify", () => {
  let rows: FileChangeRow[] = [];
  rows = applyProgressChanges(rows, write("a.ts", "x\ny"));
  expect(rows).toEqual([{ path: "a.ts", op: "add", additions: 2, deletions: null, diff: null, toolCallId: "c1" }]);
  rows = applyProgressChanges(rows, write("a.ts", "x\ny\nz"));
  expect(rows.length).toBe(1);
  expect(rows[0].op).toBe("modify");
  expect(rows[0].additions).toBe(3);
});
test("ignores non-write, malformed and non-tool frames", () => {
  const rows = applyProgressChanges([], write("a.ts", "1"));
  expect(
    applyProgressChanges(rows, { kind: "tool_call", id: "c2", name: "read", arguments: { path: "a.ts" } }),
  ).toBe(rows);
  expect(applyProgressChanges(rows, { kind: "tool_call", id: "c3", name: "write", arguments: {} })).toBe(rows);
  expect(applyProgressChanges(rows, { kind: "text", text: "hi" })).toBe(rows);
});
