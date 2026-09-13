// Derived file-change rows for the right-panel ChangeList.
// Protocol gap: write tool_call frames carry {path, content} only —
// deletions and a unified diff are not derivable, so they stay null.

export type FileChangeRow = {
  path: string;
  op: "add" | "modify" | "delete";
  additions: number;
  deletions: number | null;
  diff: string | null;
  toolCallId: string | null;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Upsert a row for a write tool_call frame; non-matching frames return `prev` unchanged. */
export function applyProgressChanges(prev: FileChangeRow[], frame: unknown): FileChangeRow[] {
  if (!isRecord(frame) || frame.kind !== "tool_call" || frame.name !== "write") return prev;
  if (!isRecord(frame.arguments)) return prev;
  const { path, content } = frame.arguments;
  if (typeof path !== "string" || typeof content !== "string") return prev;

  const toolCallId = typeof frame.id === "string" ? frame.id : null;
  const additions = content.split("\n").length;
  const index = prev.findIndex((row) => row.path === path);
  if (index === -1) {
    return [...prev, { path, op: "add", additions, deletions: null, diff: null, toolCallId }];
  }
  const next = prev.slice();
  next[index] = { ...next[index], op: "modify", additions, toolCallId };
  return next;
}
