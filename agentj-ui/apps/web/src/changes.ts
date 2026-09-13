// Protocol-driven file-change rows for the right-panel ChangeList.
// Live source: `$/progress` kind:"file_change" frames (upsert by path).
// Replay source: session/history `TrajectoryValue::FileChange` events, merged in order
// (design 决策 #9/#10). The legacy write tool_call half-derive is retired: tool_call
// progress frames no longer produce rows.

import type { ProgressFrame } from "@agentj/sdk";
import type { RawEvent } from "./Trajectory";

export type FileChangeRow = {
  path: string;
  op: "add" | "modify" | "delete";
  additions: number;
  deletions: number | null;
  diff: string | null;
  toolCallId: string | null;
};

const KNOWN_OPS: readonly FileChangeRow["op"][] = ["add", "modify", "delete"];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Unknown forward `op` values degrade to `modify` so the row stays renderable. */
function normalizeOp(op: string): FileChangeRow["op"] {
  return (KNOWN_OPS as readonly string[]).includes(op) ? (op as FileChangeRow["op"]) : "modify";
}

/** Validate a wire `change` payload (`diff`/`tool_call_id` null-normalize); malformed shapes return null. */
function toRow(change: unknown): FileChangeRow | null {
  if (!isRecord(change)) return null;
  const { path, op, additions, deletions } = change;
  if (typeof path !== "string" || typeof op !== "string") return null;
  if (typeof additions !== "number" || typeof deletions !== "number") return null;
  const diff = change["diff"];
  const toolCallId = change["tool_call_id"];
  return {
    path,
    op: normalizeOp(op),
    additions,
    deletions,
    diff: typeof diff === "string" ? diff : null,
    toolCallId: typeof toolCallId === "string" ? toolCallId : null,
  };
}

/** Append a new path or replace it in place (a later modify of the same path wins). */
function upsert(prev: FileChangeRow[], row: FileChangeRow): FileChangeRow[] {
  const index = prev.findIndex((existing) => existing.path === row.path);
  if (index === -1) return [...prev, row];
  const next = prev.slice();
  next[index] = row;
  return next;
}

/** Live `$/progress` reducer; non-file_change frames (including write tool_call) keep the same reference. */
export function applyFileChange(prev: FileChangeRow[], frame: ProgressFrame): FileChangeRow[] {
  if (frame.kind !== "file_change") return prev;
  const row = toRow(frame.change);
  return row === null ? prev : upsert(prev, row);
}

/** Replay rebuild: merge history `file_change` events in order (upsert by path). */
export function changesFromHistory(events: RawEvent[]): FileChangeRow[] {
  let rows: FileChangeRow[] = [];
  for (const event of events) {
    const value = event.value;
    if (!isRecord(value) || value.kind !== "file_change") continue;
    const row = toRow(value.change);
    if (row !== null) rows = upsert(rows, row);
  }
  return rows;
}
