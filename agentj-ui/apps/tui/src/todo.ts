import type { ProgressFrame, TodoItemFrame } from "@agentj/sdk";

export type TodoState = {
  revision: number;
  items: TodoItemFrame[];
  runId?: string | null;
  byRun?: Map<string | null, { revision: number; items: TodoItemFrame[] }>;
  currentRunId?: string | null;
};

export const EMPTY_TODO_STATE: TodoState = { revision: -1, items: [] };

const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

function isValidRunId(s: string): boolean {
  return s.length > 0 && s.length <= 128 && UUID_RE.test(s);
}

function effectiveRunId(frame: ProgressFrame): string | null {
  if (frame.kind !== "todo") return null;
  const candidate = frame.runId;
  return typeof candidate === "string" && isValidRunId(candidate) ? candidate : null;
}

export function applyTodoFrame(state: TodoState, frame: ProgressFrame): TodoState {
  if (frame.kind !== "todo") return state;
  const effRunId = effectiveRunId(frame);

  const seeded = state.byRun ? new Map(state.byRun) : new Map<string | null, { revision: number; items: TodoItemFrame[] }>();

  if (!state.byRun && state.revision >= 0) {
    const seedKey = state.runId ?? state.currentRunId ?? null;
    if (!seeded.has(seedKey)) seeded.set(seedKey, { revision: state.revision, items: state.items });
  }

  let curRev = -1;
  const entry = seeded.get(effRunId);
  if (entry) curRev = entry.revision;
  else if (state.runId === effRunId || state.currentRunId === effRunId) curRev = state.revision;
  else if (effRunId === null && state.runId == null && state.currentRunId == null) curRev = state.revision;

  if (frame.revision <= curRev) return state;

  const items = frame.items;
  seeded.set(effRunId, { revision: frame.revision, items });
  return {
    revision: frame.revision,
    items,
    runId: effRunId,
    currentRunId: effRunId,
    byRun: seeded,
  };
}
