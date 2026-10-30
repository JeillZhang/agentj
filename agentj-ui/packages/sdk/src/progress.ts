export type UsageInfo = { prompt_tokens: number; completion_tokens: number };

export type TodoItemFrame = { id: string; content: string; status: string };

export type TodoFrame = { kind: "todo"; session_id: string; runId?: string; revision: number; items: TodoItemFrame[] };

/** Payload of a `file_change` frame. `tool_call_id` / `diff` normalize to null when omitted
 * (Rust `skip_serializing_if = "Option::is_none"`); `diff_truncated` is only present when true. */
export type FileChangeFrame = {
  path: string;
  op: string;
  additions: number;
  deletions: number;
  tool_call_id: string | null;
  diff: string | null;
  diff_truncated?: boolean;
  run_id?: string;
  seq?: number;
  source?: string;
  total_bytes?: number;
};

export type ProgressFrame =
  | { kind: "text"; text: string }
  | { kind: "tool_call"; id: string; name: string; arguments: unknown }
  | { kind: "tool_result"; call_id: string; content: string; truncated?: number }
  | { kind: "usage"; usage: UsageInfo }
  | {
      kind: "permission";
      request_id: string;
      tool: string;
      decision: string;
      session_id?: string;
    }
  | TodoFrame
  | { kind: "file_change"; session_id: string; runId?: string; change: FileChangeFrame }
  | { kind: "unknown"; raw: unknown };

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object";
}

function isValidRunId(s: string): boolean {
  return s.length > 0 && s.length <= 128 && /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(s);
}

function parseTodoItem(v: unknown): TodoItemFrame | null {
  if (!isRecord(v)) return null;
  const { id, content, status } = v;
  return typeof id === "string" && typeof content === "string" && typeof status === "string"
    ? { id, content, status }
    : null;
}

function parseFileChange(v: unknown): FileChangeFrame | null {
  if (!isRecord(v)) return null;
  const { path, op, additions, deletions } = v;
  if (typeof path !== "string" || typeof op !== "string") return null;
  if (typeof additions !== "number" || typeof deletions !== "number") return null;
  const toolCallId = v["tool_call_id"];
  const diff = v["diff"];
  const change: FileChangeFrame = {
    path,
    op,
    additions,
    deletions,
    tool_call_id: typeof toolCallId === "string" ? toolCallId : null,
    diff: typeof diff === "string" ? diff : null,
  };
  const truncated = v["diff_truncated"];
  if (typeof truncated === "boolean") change.diff_truncated = truncated;
  const runIdInner = v["run_id"];
  if (typeof runIdInner === "string" && isValidRunId(runIdInner)) change.run_id = runIdInner;
  const seq = v["seq"];
  if (typeof seq === "number" && Number.isInteger(seq) && seq >= 0) change.seq = seq;
  const source = v["source"];
  if (typeof source === "string" && source.length > 0) change.source = source;
  const totalBytes = v["total_bytes"];
  if (typeof totalBytes === "number" && Number.isInteger(totalBytes) && totalBytes >= 0) change.total_bytes = totalBytes;
  return change;
}

// Deliberately LOOSE (Concretization 11): never throws, never returns null.
// Unknown / malformed input degrades to an "unknown" frame that consumers
// ignore after a console.debug. Flat legacy frames ({session_id, text} or
// {text}) parse to a text frame so pre-0.12 servers keep working.
export function parseProgress(params: unknown): ProgressFrame {
  if (!isRecord(params)) return { kind: "unknown", raw: params };
  // Support both envelope {token,value} and direct value {kind,...}
  const valueRaw = isRecord(params["value"]) ? params["value"] : params;
  // Legacy flat text shims
  if (typeof (params as Record<string, unknown>)["text"] === "string" && !isRecord(valueRaw)) {
    return { kind: "text", text: (params as Record<string, unknown>)["text"] as string };
  }
  if (!isRecord(valueRaw)) return { kind: "unknown", raw: params };
  const raw = valueRaw;
  if (isRecord(raw["text"]) === false && typeof raw["text"] === "string" && !raw["kind"]) {
    return { kind: "text", text: raw["text"] as string };
  }
  const kind = raw["kind"];
  if (typeof kind !== "string") return { kind: "unknown", raw: params };
  switch (kind) {
    case "text": {
      const delta = raw["delta"];
      return typeof delta === "string" ? { kind: "text", text: delta } : { kind: "unknown", raw: params };
    }
    case "tool_call": {
      const tc = raw["tool_call"];
      if (!isRecord(tc)) return { kind: "unknown", raw: params };
      const { id, name } = tc;
      return typeof id === "string" && typeof name === "string"
        ? { kind: "tool_call", id, name, arguments: tc["arguments"] ?? null }
        : { kind: "unknown", raw: params };
    }
    case "tool_result": {
      const { tool_call_id, content } = raw;
      if (typeof tool_call_id !== "string" || typeof content !== "string") {
        return { kind: "unknown", raw: params };
      }
      // Absent means "nothing was dropped" (0.20.0 wire); a malformed count is ignored
      // rather than hiding the result.
      const dropped = raw["truncated_chars"];
      return typeof dropped === "number" && dropped > 0
        ? { kind: "tool_result", call_id: tool_call_id, content, truncated: dropped }
        : { kind: "tool_result", call_id: tool_call_id, content };
    }
    case "usage": {
      const u = raw["usage"];
      if (!isRecord(u)) return { kind: "unknown", raw: params };
      const pt = u["prompt_tokens"];
      const ct = u["completion_tokens"];
      return typeof pt === "number" && typeof ct === "number"
        ? { kind: "usage", usage: { prompt_tokens: pt, completion_tokens: ct } }
        : { kind: "unknown", raw: params };
    }
    case "permission": {
      const { request_id, tool, decision } = raw;
      const sid = raw["session_id"];
      if (typeof request_id !== "string" || typeof tool !== "string" || typeof decision !== "string") {
        return { kind: "unknown", raw: params };
      }
      return typeof sid === "string"
        ? { kind: "permission", request_id, tool, decision, session_id: sid }
        : { kind: "permission", request_id, tool, decision };
    }
    case "todo": {
      const { session_id, revision } = raw;
      if (typeof session_id !== "string" || typeof revision !== "number" || !Number.isInteger(revision)) {
        return { kind: "unknown", raw: params };
      }
      const rawItems = raw["items"];
      if (!Array.isArray(rawItems)) return { kind: "unknown", raw: params };
      const items: TodoItemFrame[] = [];
      for (const rawItem of rawItems as unknown[]) {
        const item = parseTodoItem(rawItem);
        if (item === null) return { kind: "unknown", raw: params };
        items.push(item);
      }
      const runIdOuter = raw["run_id"];
      const runId = typeof runIdOuter === "string" && isValidRunId(runIdOuter) ? runIdOuter : undefined;
      return runId !== undefined ? { kind: "todo", session_id, runId, revision, items } : { kind: "todo", session_id, revision, items };
    }
    case "file_change": {
      const { session_id } = raw;
      if (typeof session_id !== "string") return { kind: "unknown", raw: params };
      const change = parseFileChange(raw["change"]);
      if (!change) return { kind: "unknown", raw: params };
      const outerRunIdRaw = raw["run_id"];
      const outerRunId = typeof outerRunIdRaw === "string" && isValidRunId(outerRunIdRaw) ? outerRunIdRaw : undefined;
      // change.run_id wins when both present; otherwise fall back to outer
      if (change.run_id === undefined && outerRunId !== undefined) change.run_id = outerRunId;
      const effectiveRunId = change.run_id ?? outerRunId;
      return effectiveRunId !== undefined ? { kind: "file_change", session_id, runId: effectiveRunId, change } : { kind: "file_change", session_id, change };
    }
    default:
      return { kind: "unknown", raw: params };
  }
}
