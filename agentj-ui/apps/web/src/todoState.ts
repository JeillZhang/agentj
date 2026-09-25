// Protocol-driven TODO state for the right-panel TodoPanel.
// Live source: `$/progress` kind:"todo" full snapshots, monotonic per run
// (revision gate per run_id). Replay source: session/history
// `TrajectoryValue::Todo` events — the highest revision wins.

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
  runId?: string | null;
  byRun?: Map<string | null, { revision: number; items: TodoView[] }>;
  currentRunId?: string | null;
};

/** No frame seen yet: revision -1 so every real snapshot (revision >= 0) wins. */
export const EMPTY_TODO_STATE: TodoState = { revision: -1, items: [] };

const KNOWN_STATUSES: readonly TodoViewStatus[] = [
  "pending",
  "in_progress",
  "completed",
  "cancelled",
];

const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

function normalizeStatus(status: string): TodoViewStatus {
  return (KNOWN_STATUSES as readonly string[]).includes(status)
    ? (status as TodoViewStatus)
    : "pending";
}

function toView(item: TodoItemFrame): TodoView {
  return {
    id: item.id,
    content: item.content,
    status: normalizeStatus(item.status),
  };
}

/** Live `$/progress` reducer: non-todo or stale (revision <= current) frames keep the same reference. */
export function applyTodoFrame(state: TodoState, frame: ProgressFrame): TodoState {
  if (frame.kind !== "todo") return state;
  const rawRunId = frame.runId;
  const effRunId =
    typeof rawRunId === "string" && rawRunId.length <= 128 && UUID_RE.test(rawRunId) ? rawRunId : null;

  const byRun = state.byRun ? new Map(state.byRun) : new Map<string | null, { revision: number; items: TodoView[] }>();

  if (!state.byRun && state.revision >= 0) {
    const seedKey = state.runId ?? state.currentRunId ?? null;
    if (!byRun.has(seedKey)) byRun.set(seedKey, { revision: state.revision, items: state.items });
  }

  const entry = byRun.get(effRunId);
  let curRev = -1;
  if (entry) curRev = entry.revision;
  else if (state.runId === effRunId || state.currentRunId === effRunId) curRev = state.revision;
  else if (effRunId === null && state.runId == null && state.currentRunId == null) curRev = state.revision;

  if (frame.revision <= curRev) return state;

  const items = frame.items.map(toView);
  byRun.set(effRunId, { revision: frame.revision, items });
  const out: TodoState = { revision: frame.revision, items };
  Object.defineProperty(out, "runId", {
    value: effRunId,
    enumerable: false,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(out, "currentRunId", {
    value: effRunId,
    enumerable: false,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(out, "byRun", {
    value: byRun,
    enumerable: false,
    writable: true,
    configurable: true,
  });
  return out;
}

/** Parse a replay snapshot's `items`; malformed entries invalidate the whole snapshot (skip it). */
function parseHistoryItems(raw: unknown): TodoView[] | null {
  if (!Array.isArray(raw)) return null;
  const items: TodoView[] = [];
  for (const entry of raw as unknown[]) {
    if (!entry || typeof entry !== "object") return null;
    if (!("id" in entry) || !("content" in entry) || !("status" in entry)) return null;
    const id = (entry as { id: unknown }).id;
    const content = (entry as { content: unknown }).content;
    const status = (entry as { status: unknown }).status;
    if (typeof id !== "string" || typeof content !== "string" || typeof status !== "string") return null;
    items.push({ id, content, status: normalizeStatus(status) });
  }
  return items;
}

/** Replay rebuild: the highest-revision todo snapshot in the session history wins. */
export function todosFromHistory(events: RawEvent[], runIdFilter?: string | null): TodoState {
  const hasFilter = arguments.length >= 2;
  const normFilter =
    hasFilter && typeof runIdFilter === "string" && runIdFilter.length <= 128 && UUID_RE.test(runIdFilter)
      ? runIdFilter
      : hasFilter
        ? null
        : undefined;

  let best: TodoState = EMPTY_TODO_STATE;
  let bestRunId: string | null | undefined = undefined;
  const perRun = new Map<string | null, { revision: number; items: TodoView[]; runId: string | null }>();

  for (const event of events) {
    const v = event.value as Record<string, unknown> | null;
    if (!v || typeof v !== "object" || v.kind !== "todo") continue;
    const rev = v.revision;
    if (typeof rev !== "number" || !Number.isInteger(rev)) continue;
    const rawRunId = (v as Record<string, unknown>).run_id;
    const effRunId =
      typeof rawRunId === "string" && rawRunId.length <= 128 && UUID_RE.test(rawRunId) ? rawRunId : null;
    if (hasFilter && effRunId !== normFilter) continue;
    const items = parseHistoryItems(v.items);
    if (items === null) continue;
    const existing = perRun.get(effRunId);
    if (!existing || rev > existing.revision) perRun.set(effRunId, { revision: rev, items, runId: effRunId });
    if (rev > best.revision) {
      // create new object to keep EMPTY_TODO_STATE pristine
      best = { revision: rev, items };
      bestRunId = effRunId;
    }
  }

  if (best === EMPTY_TODO_STATE) return best;

  const withMeta = best as TodoState;
  if (bestRunId !== undefined) {
    Object.defineProperty(withMeta, "runId", {
      value: bestRunId,
      enumerable: false,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(withMeta, "currentRunId", {
      value: bestRunId,
      enumerable: false,
      writable: true,
      configurable: true,
    });
  }
  if (perRun.size > 0) {
    Object.defineProperty(withMeta, "byRun", {
      value: perRun,
      enumerable: false,
      writable: true,
      configurable: true,
    });
  }
  return withMeta;
}
