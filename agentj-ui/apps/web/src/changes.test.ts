import { expect, test } from "vitest";
import type { ProgressFrame } from "@agentj/sdk";
import { applyFileChange, changesFromHistory, type FileChangeRow } from "./changes";
import type { RawEvent } from "./Trajectory";

type Extra = { diff?: string | null; tool_call_id?: string | null };

const change = (path: string, op: string, additions: number, deletions: number, extra: Extra = {}) => ({
  path,
  op,
  additions,
  deletions,
  tool_call_id: extra.tool_call_id ?? null,
  diff: extra.diff ?? null,
});

const frame = (path: string, op: string, additions: number, deletions: number, extra: Extra = {}): ProgressFrame => ({
  kind: "file_change",
  session_id: "s1",
  change: change(path, op, additions, deletions, extra),
});

const event = (seq: number, value: Record<string, unknown>): RawEvent => ({ session_id: "s1", seq, value });

test("upserts by path", () => {
  let rows: FileChangeRow[] = [];
  rows = applyFileChange(rows, frame("a.ts", "add", 2, 0));
  expect(rows).toEqual([{ path: "a.ts", op: "add", additions: 2, deletions: 0, diff: null, toolCallId: null }]);
  rows = applyFileChange(rows, frame("a.ts", "modify", 3, 1, { diff: "+x", tool_call_id: "c1" }));
  expect(rows.length).toBe(1);
  expect(rows[0].op).toBe("modify");
  expect(rows[0].additions).toBe(3);
  expect(rows[0].deletions).toBe(1);
  expect(rows[0].diff).toBe("+x");
  expect(rows[0].toolCallId).toBe("c1");
});

test("ignores tool_call frames", () => {
  const rows = applyFileChange([], frame("a.ts", "add", 1, 0));
  const write: ProgressFrame = {
    kind: "tool_call",
    id: "c1",
    name: "write",
    arguments: { path: "a.ts", content: "x" },
  };
  expect(applyFileChange(rows, write)).toBe(rows);
  const text: ProgressFrame = { kind: "text", text: "hi" };
  expect(applyFileChange(rows, text)).toBe(rows);
});

test("rebuilds from history", () => {
  const events: RawEvent[] = [
    event(1, { kind: "file_change", change: change("a.ts", "add", 2, 0, { tool_call_id: "c1", diff: "+a" }) }),
    event(2, { kind: "text", delta: "ignored" }),
    event(3, { kind: "file_change", change: change("b.ts", "modify", 5, 3) }),
  ];
  const rows = changesFromHistory(events);
  expect(rows.map((row) => row.path)).toEqual(["a.ts", "b.ts"]);
  expect(rows[0]).toEqual({ path: "a.ts", op: "add", additions: 2, deletions: 0, diff: "+a", toolCallId: "c1" });
  expect(rows[1]).toEqual({ path: "b.ts", op: "modify", additions: 5, deletions: 3, diff: null, toolCallId: null });
});

test("history upserts by path and skips malformed or unknown events", () => {
  const events: RawEvent[] = [
    event(1, { kind: "file_change", change: change("a.ts", "add", 1, 0) }),
    event(2, { kind: "file_change", change: change("a.ts", "modify", 4, 2) }),
    event(3, { kind: "file_change", change: { path: 42 } }),
    event(4, { kind: "tool_result", tool_call_id: "c1", content: "ok" }),
  ];
  const rows = changesFromHistory(events);
  expect(rows.length).toBe(1);
  expect(rows[0]).toEqual({ path: "a.ts", op: "modify", additions: 4, deletions: 2, diff: null, toolCallId: null });
});
