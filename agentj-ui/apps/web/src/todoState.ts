// Protocol-driven TODO state for the right-panel TodoPanel.
// Live source: `$/progress` kind:"todo" full snapshots, monotonic per session
// (revision gate). Replay source: session/history `TrajectoryValue::Todo`
// events — the highest revision wins (design 决策 #10).

import type { ProgressFrame, TodoItemFrame } from "@agentj/sdk";
import type { RawEvent } from "./Trajectory";

/** View-model status: the four known protocol statuses; unknown wire values normalize to pending. */
export type TodoViewStatus = "pending" | "in_progress" | "completed" | "cancelled";

/** Todo row shape; structurally identical to the legacy TodoPanel/demoData `TodoItem` (T33 decouples). */
export type TodoView = {
  id: string;
  content: string;
  status: TodoViewStatus;
};

export type TodoState = {
  revision: number;
  items: TodoView[];
};

/** No frame seen yet: revision -1 so every real snapshot (revision >= 0) wins. */
export const EMPTY_TODO_STATE: TodoState = { revision: -1, items: [] };

const KNOWN_STATUSES: readonly TodoViewStatus[] = ["pending", "in_progress", "completed", "cancelled"];

function normalizeStatus(status: string): TodoViewStatus {
  return (KNOWN_STATUSES as readonly string[]).includes(status) ? (status as TodoViewStatus) : "pending";
}

function toView(item: TodoItemFrame): TodoView {
  return { id: item.id, content: item.content, status: normalizeStatus(item.status) };
}

/** Live `$/progress` reducer: non-todo or stale (revision <= current) frames keep the same reference. */
export function applyTodoFrame(state: TodoState, frame: ProgressFrame): TodoState {
  if (frame.kind !== "todo" || frame.revision <= state.revision) return state;
  return { revision: frame.revision, items: frame.items.map(toView) };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Parse a replay snapshot's `items`; malformed entries invalidate the whole snapshot (skip it). */
function parseHistoryItems(raw: unknown): TodoView[] | null {
  if (!Array.isArray(raw)) return null;
  const items: TodoView[] = [];
  for (const entry of raw as unknown[]) {
    if (!isRecord(entry)) return null;
    const { id, content, status } = entry;
    if (typeof id !== "string" || typeof content !== "string" || typeof status !== "string") return null;
    items.push({ id, content, status: normalizeStatus(status) });
  }
  return items;
}

/** Replay rebuild: the highest-revision todo snapshot in the session history wins. */
export function todosFromHistory(events: RawEvent[]): TodoState {
  let best = EMPTY_TODO_STATE;
  for (const event of events) {
    const value = event.value;
    if (!isRecord(value) || value.kind !== "todo") continue;
    const { revision } = value;
    if (typeof revision !== "number" || !Number.isInteger(revision) || revision <= best.revision) continue;
    const items = parseHistoryItems(value.items);
    if (items === null) continue;
    best = { revision, items };
  }
  return best;
}
