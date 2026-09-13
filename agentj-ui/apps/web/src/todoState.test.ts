import { describe, expect, it } from "vitest";
import type { ProgressFrame } from "@agentj/sdk";
import {
  EMPTY_TODO_STATE,
  applyTodoFrame,
  todosFromHistory,
  type TodoState,
} from "./todoState";
import type { RawEvent } from "./Trajectory";

const todoFrame = (revision: number, items: { id: string; content: string; status: string }[]): ProgressFrame =>
  ({ kind: "todo", session_id: "s1", revision, items });

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
