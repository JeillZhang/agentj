// Protocol-driven file-change rows for the right-panel ChangeList.
// Live source: `$/progress` kind:"file_change" frames (upsert by (run_id, path)).
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
  runId?: string | null;
  seq?: number;
  source?: string;
  totalBytes?: number;
  diffTruncated?: boolean;
  eventSeq?: number;
};

const KNOWN_OPS: readonly FileChangeRow["op"][] = ["add", "modify", "delete"];

const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

function normalizeOp(op: string): FileChangeRow["op"] {
  return (KNOWN_OPS as readonly string[]).includes(op)
    ? (op as FileChangeRow["op"])
    : "modify";
}

function normalizeRunId(v: unknown): string | null {
  return typeof v === "string" && v.length <= 128 && UUID_RE.test(v) ? v : null;
}

function defineHidden<T extends object, K extends string>(
  obj: T,
  key: K,
  value: unknown,
): void {
  Object.defineProperty(obj, key, {
    value,
    enumerable: false,
    writable: true,
    configurable: true,
  });
}

/** Validate a wire `change` payload (`diff`/`tool_call_id` null-normalize); malformed shapes return null. */
function toRow(change: unknown): FileChangeRow | null {
  if (!change || typeof change !== "object") return null;
  const c = change as Record<string, unknown>;
  const path = c.path;
  const op = c.op;
  const additions = c.additions;
  const deletions = c.deletions;
  if (typeof path !== "string" || typeof op !== "string") return null;
  if (typeof additions !== "number" || typeof deletions !== "number") return null;
  const diff = c.diff;
  const toolCallId = c.tool_call_id;
  const row: FileChangeRow = {
    path,
    op: normalizeOp(op),
    additions,
    deletions,
    diff: typeof diff === "string" ? diff : null,
    toolCallId: typeof toolCallId === "string" ? toolCallId : null,
  };
  const seq = c.seq;
  if (typeof seq === "number" && Number.isInteger(seq) && seq >= 0) defineHidden(row, "seq", seq);
  const source = c.source;
  if (typeof source === "string" && source.length > 0) defineHidden(row, "source", source);
  const totalBytes = c.total_bytes;
  if (typeof totalBytes === "number" && Number.isInteger(totalBytes) && totalBytes >= 0) {
    defineHidden(row, "totalBytes", totalBytes);
  }
  const trunc = c.diff_truncated;
  if (typeof trunc === "boolean" && trunc) defineHidden(row, "diffTruncated", true);
  const runIdInner = c.run_id;
  const norm = normalizeRunId(runIdInner);
  if (norm) defineHidden(row, "runId", norm);
  return row;
}

/** Composite key: (runId, path). Legacy bucket is "__legacy__". */
function keyOf(row: FileChangeRow): string {
  return `${row.runId ?? "__legacy__"}\0${row.path}`;
}

/** Append a new key or replace it in place (a later change for same (run,path) wins). */
function upsert(prev: FileChangeRow[], row: FileChangeRow): FileChangeRow[] {
  const key = keyOf(row);
  const index = prev.findIndex((existing) => keyOf(existing) === key);
  if (index === -1) return [...prev, row];
  const next = prev.slice();
  next[index] = row;
  return next;
}

/** Sort helper for drawer entries: seq asc, fallback to eventSeq, fallback to stable index. */
export function sortBySeq(rows: FileChangeRow[]): FileChangeRow[] {
  return [...rows].sort((a, b) => {
    const sa = a.seq ?? a.eventSeq ?? Number.MAX_SAFE_INTEGER;
    const sb = b.seq ?? b.eventSeq ?? Number.MAX_SAFE_INTEGER;
    if (sa !== sb) return sa - sb;
    const ea = a.eventSeq ?? 0;
    const eb = b.eventSeq ?? 0;
    return ea - eb;
  });
}

/** Live `$/progress` reducer; non-file_change frames (including write tool_call) keep the same reference. */
export function applyFileChange(prev: FileChangeRow[], frame: ProgressFrame): FileChangeRow[] {
  if (frame.kind !== "file_change") return prev;
  const row = toRow(frame.change);
  if (row === null) return prev;
  const outer = (frame as { runId?: string }).runId;
  const outerNorm = normalizeRunId(outer);
  const eff = (row.runId ?? outerNorm) ?? null;
  if (eff !== null) defineHidden(row, "runId", eff);
  else if (row.runId != null) delete (row as Record<string, unknown>).runId;
  return upsert(prev, row);
}

/** Replay rebuild: merge history `file_change` events in order (upsert by (run_id, path)). */
export function changesFromHistory(events: RawEvent[], runIdFilter?: string | null): FileChangeRow[] {
  const hasFilter = arguments.length >= 2;
  const normFilter = hasFilter ? normalizeRunId(runIdFilter) : undefined;
  let rows: FileChangeRow[] = [];
  for (const event of events) {
    const value = event.value as Record<string, unknown> | null;
    if (!value || typeof value !== "object" || value.kind !== "file_change") continue;
    const row = toRow(value.change);
    if (row === null) continue;
    const outerRunRaw = value.run_id;
    const outerNorm = normalizeRunId(outerRunRaw);
    const eff = (row.runId ?? outerNorm) ?? null;
    if (eff !== null) defineHidden(row, "runId", eff);
    defineHidden(row, "eventSeq", typeof event.seq === "number" ? event.seq : undefined);
    if (hasFilter && eff !== normFilter) continue;
    rows = upsert(rows, row);
  }
  const anySeq = rows.some((r) => r.seq !== undefined);
  if (hasFilter && anySeq) rows = sortBySeq(rows);
  return rows;
}
