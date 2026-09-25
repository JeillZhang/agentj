import { expect, test } from "vitest";
import type { ProgressFrame } from "@agentj/sdk";
import {
  EMPTY_CHANGES,
  applyFileChange,
  changesFromHistory,
  sortBySeq,
  type ChangesState,
  type FileChangeRow,
} from "./changes";
import type { RawEvent } from "./Trajectory";

type Extra = {
  diff?: string | null;
  tool_call_id?: string | null;
  seq?: number;
  source?: string;
  total_bytes?: number;
  diff_truncated?: boolean;
  run_id?: string;
};

const change = (path: string, op: string, additions: number, deletions: number, extra: Extra = {}) => ({
  path,
  op,
  additions,
  deletions,
  tool_call_id: extra.tool_call_id ?? null,
  diff: extra.diff ?? null,
  ...(extra.seq === undefined ? {} : { seq: extra.seq }),
  ...(extra.source === undefined ? {} : { source: extra.source }),
  ...(extra.total_bytes === undefined ? {} : { total_bytes: extra.total_bytes }),
  ...(extra.diff_truncated === undefined ? {} : { diff_truncated: extra.diff_truncated }),
  ...(extra.run_id === undefined ? {} : { run_id: extra.run_id }),
});

const frame = (path: string, op: string, additions: number, deletions: number, extra: Extra = {}): ProgressFrame => ({
  kind: "file_change",
  session_id: "s1",
  change: change(path, op, additions, deletions, extra),
});

const event = (seq: number, value: Record<string, unknown>): RawEvent => ({ session_id: "s1", seq, value });

const UUID_A = "11111111-1111-4111-8111-111111111111";
const UUID_B = "22222222-2222-4222-8222-222222222222";

test("upserts by (run, path) in the summary layer", () => {
  let state: ChangesState = applyFileChange(EMPTY_CHANGES, frame("a.ts", "add", 2, 0));
  expect(state.summary).toEqual([
    { path: "a.ts", op: "add", additions: 2, deletions: 0, diff: null, toolCallId: null },
  ]);
  state = applyFileChange(state, frame("a.ts", "modify", 3, 1, { diff: "+x", tool_call_id: "c1" }));
  expect(state.summary.length).toBe(1);
  expect(state.summary[0].op).toBe("modify");
  expect(state.summary[0].additions).toBe(3);
  expect(state.summary[0].deletions).toBe(1);
  expect(state.summary[0].diff).toBe("+x");
  expect(state.summary[0].toolCallId).toBe("c1");
  // history keeps both frames (append-only, no de-dup)
  expect(state.history.length).toBe(2);
});

test("non-file_change and malformed frames keep the same state reference", () => {
  const state = applyFileChange(EMPTY_CHANGES, frame("a.ts", "add", 1, 0));
  const write: ProgressFrame = {
    kind: "tool_call",
    id: "c1",
    name: "write",
    arguments: { path: "a.ts", content: "x" },
  };
  expect(applyFileChange(state, write)).toBe(state);
  const text: ProgressFrame = { kind: "text", text: "hi" };
  expect(applyFileChange(state, text)).toBe(state);
  const malformed = {
    kind: "file_change",
    session_id: "s1",
    change: { path: 42 },
  } as unknown as ProgressFrame;
  expect(applyFileChange(state, malformed)).toBe(state);
});

test("summary isolates runs for the same path; history keeps both", () => {
  let state = applyFileChange(EMPTY_CHANGES, frame("a.txt", "add", 1, 0, { run_id: UUID_A }));
  state = applyFileChange(state, frame("a.txt", "modify", 2, 1, { run_id: UUID_B }));
  expect(state.summary.length).toBe(2);
  expect(state.summary.map((r) => r.runId)).toEqual([UUID_A, UUID_B]);
  expect(state.history.length).toBe(2);
});

test("history is append-only and ordered by seq; summary keeps the latest", () => {
  let state = applyFileChange(EMPTY_CHANGES, frame("a.txt", "add", 1, 0, { run_id: UUID_A, seq: 1 }));
  state = applyFileChange(state, frame("a.txt", "modify", 2, 1, { run_id: UUID_A, seq: 2 }));
  expect(state.summary.length).toBe(1);
  expect(state.summary[0].additions).toBe(2);
  expect(state.history.length).toBe(2);
  expect(sortBySeq(state.history).map((r) => r.seq)).toEqual([1, 2]);
});

test("rebuilds summary and history from history events", () => {
  const events: RawEvent[] = [
    event(1, { kind: "file_change", change: change("a.ts", "add", 2, 0, { tool_call_id: "c1", diff: "+a" }) }),
    event(2, { kind: "text", delta: "ignored" }),
    event(3, { kind: "file_change", change: change("b.ts", "modify", 5, 3) }),
  ];
  const { summary, history } = changesFromHistory(events);
  expect(summary.map((row) => row.path)).toEqual(["a.ts", "b.ts"]);
  expect(summary[0]).toEqual({ path: "a.ts", op: "add", additions: 2, deletions: 0, diff: "+a", toolCallId: "c1" });
  expect(summary[1]).toEqual({ path: "b.ts", op: "modify", additions: 5, deletions: 3, diff: null, toolCallId: null });
  expect(history.length).toBe(2);
});

test("history upserts by path and skips malformed or unknown events", () => {
  const events: RawEvent[] = [
    event(1, { kind: "file_change", change: change("a.ts", "add", 1, 0) }),
    event(2, { kind: "file_change", change: change("a.ts", "modify", 4, 2) }),
    event(3, { kind: "file_change", change: { path: 42 } }),
    event(4, { kind: "tool_result", tool_call_id: "c1", content: "ok" }),
  ];
  const { summary, history } = changesFromHistory(events);
  expect(summary.length).toBe(1);
  expect(summary[0]).toEqual({ path: "a.ts", op: "modify", additions: 4, deletions: 2, diff: null, toolCallId: null });
  expect(history.length).toBe(2);
});

test("legacy replay without per-run seq falls back to eventSeq ordering", () => {
  const events: RawEvent[] = [
    event(5, { kind: "file_change", change: change("a.ts", "modify", 1, 0, { run_id: UUID_A }) }),
    event(3, { kind: "file_change", change: change("b.ts", "modify", 2, 0, { run_id: UUID_A }) }),
  ];
  const { summary, history } = changesFromHistory(events);
  expect(summary.map((r) => r.path)).toEqual(["a.ts", "b.ts"]);
  expect(history.map((r) => r.path)).toEqual(["b.ts", "a.ts"]);
  expect(history[0].eventSeq).toBe(3);
});

test("unknown source is preserved and truncation metadata is retained", () => {
  const state = applyFileChange(
    EMPTY_CHANGES,
    frame("a.ts", "modify", 1, 0, {
      run_id: UUID_A,
      seq: 1,
      source: "edit",
      diff: "@@",
      total_bytes: 70000,
      diff_truncated: true,
    }),
  );
  expect(state.summary[0].source).toBe("edit");
  expect(state.summary[0].totalBytes).toBe(70000);
  expect(state.summary[0].diffTruncated).toBe(true);
});

test("inner run_id wins over outer in applyFileChange", () => {
  const frameWithConflict: ProgressFrame = {
    kind: "file_change",
    session_id: "s1",
    runId: UUID_A,
    change: {
      path: "a.ts",
      op: "modify",
      additions: 1,
      deletions: 0,
      tool_call_id: null,
      diff: null,
      run_id: UUID_B,
      seq: 1,
      source: "write",
    },
  };
  const state = applyFileChange(EMPTY_CHANGES, frameWithConflict);
  expect(state.summary.length).toBe(1);
  expect(state.summary[0].runId).toBe(UUID_B);
});

test("inner run_id wins over outer in changesFromHistory", () => {
  const events: RawEvent[] = [
    event(1, {
      kind: "file_change",
      run_id: UUID_A,
      change: { path: "a.ts", op: "modify", additions: 1, deletions: 0, run_id: UUID_B, seq: 1, source: "write" },
    }),
  ];
  const { summary } = changesFromHistory(events);
  expect(summary.length).toBe(1);
  expect(summary[0].runId).toBe(UUID_B);
});

test("changesFromHistory filters by effective run id", () => {
  const events: RawEvent[] = [
    event(1, { kind: "file_change", change: change("a.ts", "add", 1, 0, { run_id: UUID_A, seq: 1 }) }),
    event(2, { kind: "file_change", change: change("b.ts", "add", 2, 0, { run_id: UUID_B, seq: 1 }) }),
    event(3, { kind: "file_change", change: change("c.ts", "add", 3, 0) }),
  ];
  const filtered = changesFromHistory(events, UUID_A);
  expect(filtered.summary.map((r) => r.path)).toEqual(["a.ts"]);
  expect(filtered.history.length).toBe(1);
  const legacy = changesFromHistory(events, null);
  expect(legacy.summary.map((r) => r.path)).toEqual(["c.ts"]);
});

test("types stay compatible with the frozen FileChangeRow shape", () => {
  const row: FileChangeRow = { path: "a.ts", op: "add", additions: 1, deletions: 0, diff: null, toolCallId: null };
  expect(row.path).toBe("a.ts");
});
