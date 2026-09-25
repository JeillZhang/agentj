import { describe, expect, it } from "vitest";
import type { ProgressFrame } from "@agentj/sdk";
import {
  EMPTY_TODO_STATE,
  applyTodoFrame,
  focusRun,
  todosFromHistory,
  type TodoState,
} from "./todoState";
import type { RawEvent } from "./Trajectory";

const todoFrame = (revision: number, items: { id: string; content: string; status: string }[]): ProgressFrame =>
  ({ kind: "todo", session_id: "s1", revision, items });

const runFrame = (
  runId: string | undefined,
  revision: number,
  items: { id: string; content: string; status: string }[] = [],
): ProgressFrame =>
  ({ kind: "todo", session_id: "s1", runId, revision, items }) as unknown as ProgressFrame;

const UUID_A = "11111111-1111-4111-8111-111111111111";
const UUID_B = "22222222-2222-4222-8222-222222222222";

describe("applyTodoFrame", () => {
  it("drops stale revisions", () => {
    const state: TodoState = {
      revision: 2,
      items: [{ id: "a", content: "keep", status: "pending" }],
    };
    const next = applyTodoFrame(state, todoFrame(2, [{ id: "b", content: "old", status: "completed" }]));
    expect(next).toBe(state);
    expect(applyTodoFrame(state, todoFrame(1, []))).toBe(state);
  });

  it("replaces items wholesale on a newer revision", () => {
    const state: TodoState = { revision: 1, items: [{ id: "a", content: "old", status: "pending" }] };
    const next = applyTodoFrame(state, todoFrame(3, [{ id: "b", content: "new", status: "completed" }]));
    expect(next).toEqual({ revision: 3, items: [{ id: "b", content: "new", status: "completed" }] });
  });

  it("ignores non-todo frames", () => {
    const state: TodoState = { revision: 5, items: [] };
    expect(applyTodoFrame(state, { kind: "text", text: "hi" })).toBe(state);
    expect(applyTodoFrame(state, { kind: "unknown", raw: null })).toBe(state);
  });

  it("normalizes unknown statuses to pending", () => {
    const next = applyTodoFrame(EMPTY_TODO_STATE, todoFrame(1, [
      { id: "a", content: "blocked task", status: "blocked" },
      { id: "b", content: "known task", status: "in_progress" },
    ]));
    expect(next.items).toEqual([
      { id: "a", content: "blocked task", status: "pending" },
      { id: "b", content: "known task", status: "in_progress" },
    ]);
  });

  it("gates revisions per run and drops stale frames", () => {
    const base = applyTodoFrame(EMPTY_TODO_STATE, runFrame(UUID_A, 2, [{ id: "a", content: "t", status: "pending" }]));
    const stale = applyTodoFrame(base, runFrame(UUID_A, 1, []));
    expect(stale).toBe(base);
    const fresh = applyTodoFrame(base, runFrame(UUID_A, 3, []));
    expect(fresh.revision).toBe(3);
    const otherRun = applyTodoFrame(base, runFrame(UUID_B, 1, []));
    expect(otherRun.revision).toBe(1);
  });

  it("falls back to the legacy revision gate when run_id is absent", () => {
    const state: TodoState = { revision: 5, items: [] };
    expect(applyTodoFrame(state, todoFrame(4, []))).toBe(state);
    expect(applyTodoFrame(state, todoFrame(6, [])).revision).toBe(6);
  });

  it("buckets an illegal run_id under null instead of throwing", () => {
    const illegal = applyTodoFrame(EMPTY_TODO_STATE, runFrame("not-a-uuid", 1, []));
    expect(illegal.revision).toBe(1);
    const byRun = (illegal as unknown as { byRun: Map<string | null, unknown> }).byRun;
    expect(byRun.has(null)).toBe(true);
    expect(applyTodoFrame(illegal, runFrame(undefined, 0, []))).toBe(illegal);
    expect(applyTodoFrame(illegal, runFrame(undefined, 2, [])).revision).toBe(2);
  });
});

describe("todosFromHistory", () => {
  const event = (revision: number, items: { id: string; content: string; status: string }[]): RawEvent => ({
    session_id: "s1",
    seq: revision,
    value: { kind: "todo", revision, items },
  });

  it("rebuilds from history with max revision", () => {
    const events: RawEvent[] = [
      event(1, [{ id: "a", content: "first", status: "pending" }]),
      { session_id: "s1", seq: 3, value: { kind: "user", content: "hi" } },
      event(4, [{ id: "b", content: "last", status: "completed" }]),
    ];
    expect(todosFromHistory(events)).toEqual({
      revision: 4,
      items: [{ id: "b", content: "last", status: "completed" }],
    });
  });

  it("returns empty state when history has no todo frames", () => {
    expect(todosFromHistory([])).toBe(EMPTY_TODO_STATE);
    expect(todosFromHistory([{ session_id: "s1", seq: 1, value: { kind: "text", content: "x" } }])).toBe(
      EMPTY_TODO_STATE,
    );
  });
});

describe("focusRun", () => {
  it("switches to the selected bucket and preserves the per-run map metadata", () => {
    let state = applyTodoFrame(
      EMPTY_TODO_STATE,
      runFrame(UUID_A, 2, [{ id: "a", content: "A", status: "pending" }]),
    );
    state = applyTodoFrame(state, runFrame(UUID_B, 5, [{ id: "b", content: "B", status: "completed" }]));
    const mapBefore = (state as unknown as { byRun: Map<string | null, unknown> }).byRun;

    const focused = focusRun(state, UUID_A);
    expect(focused).not.toBe(state);
    expect(focused.items).toEqual([{ id: "a", content: "A", status: "pending" }]);
    expect(focused.revision).toBe(2);

    const meta = focused as unknown as {
      byRun: Map<string | null, unknown>;
      runId: string;
      currentRunId: string;
    };
    expect(meta.byRun).toBe(mapBefore);
    expect(meta.byRun.get(UUID_B)).toEqual({
      revision: 5,
      items: [{ id: "b", content: "B", status: "completed" }],
    });
    expect(meta.runId).toBe(UUID_A);
    expect(meta.currentRunId).toBe(UUID_A);
    expect(Object.keys(focused).sort()).toEqual(["items", "revision"]);
  });

  it("returns the same reference for unknown runs and states without a map", () => {
    const state = applyTodoFrame(
      EMPTY_TODO_STATE,
      runFrame(UUID_A, 1, [{ id: "a", content: "A", status: "pending" }]),
    );
    expect(focusRun(state, UUID_B)).toBe(state);
    expect(focusRun(EMPTY_TODO_STATE, UUID_A)).toBe(EMPTY_TODO_STATE);
  });
});
