export type UsageInfo = { prompt_tokens: number; completion_tokens: number };

export type TodoItemFrame = { id: string; content: string; status: string };

export type TodoFrame = { kind: "todo"; session_id: string; revision: number; items: TodoItemFrame[] };

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
};

export type ProgressFrame =
  | { kind: "text"; text: string }
  | { kind: "tool_call"; id: string; name: string; arguments: unknown }
  | { kind: "tool_result"; call_id: string; content: string }
  | { kind: "usage"; usage: UsageInfo }
  | {
      kind: "permission";
      request_id: string;
      tool: string;
      decision: string;
      session_id?: string;
    }
  | TodoFrame
  | { kind: "file_change"; session_id: string; change: FileChangeFrame }
  | { kind: "unknown"; raw: unknown };

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object";
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
  return change;
}

// Deliberately LOOSE (Concretization 11): never throws, never returns null.
// Unknown / malformed input degrades to an "unknown" frame that consumers
// ignore after a console.debug. Flat legacy frames ({session_id, text} or
// {text}) parse to a text frame so pre-0.12 servers keep working.
export function parseProgress(params: unknown): ProgressFrame {
  if (!isRecord(params)) return { kind: "unknown", raw: params };
  const value = params["value"];
  if (typeof params["text"] === "string") {
    return { kind: "text", text: params["text"] as string };
  }
  if (!isRecord(value)) return { kind: "unknown", raw: params };
  const kind = value["kind"];
  if (typeof kind !== "string") return { kind: "unknown", raw: params };
  switch (kind) {
    case "text": {
      const delta = value["delta"];
      return typeof delta === "string" ? { kind: "text", text: delta } : { kind: "unknown", raw: params };
    }
    case "tool_call": {
      const tc = value["tool_call"];
      if (!isRecord(tc)) return { kind: "unknown", raw: params };
      const { id, name } = tc;
      return typeof id === "string" && typeof name === "string"
        ? { kind: "tool_call", id, name, arguments: tc["arguments"] ?? null }
        : { kind: "unknown", raw: params };
    }
    case "tool_result": {
      const { tool_call_id, content } = value;
      return typeof tool_call_id === "string" && typeof content === "string"
        ? { kind: "tool_result", call_id: tool_call_id, content }
        : { kind: "unknown", raw: params };
    }
    case "usage": {
      const u = value["usage"];
      if (!isRecord(u)) return { kind: "unknown", raw: params };
      const pt = u["prompt_tokens"];
      const ct = u["completion_tokens"];
      return typeof pt === "number" && typeof ct === "number"
        ? { kind: "usage", usage: { prompt_tokens: pt, completion_tokens: ct } }
        : { kind: "unknown", raw: params };
    }
    case "permission": {
      const { request_id, tool, decision } = value;
      const sid = value["session_id"];
      if (typeof request_id !== "string" || typeof tool !== "string" || typeof decision !== "string") {
        return { kind: "unknown", raw: params };
      }
      return typeof sid === "string"
        ? { kind: "permission", request_id, tool, decision, session_id: sid }
        : { kind: "permission", request_id, tool, decision };
    }
    case "todo": {
      const { session_id, revision } = value;
      if (typeof session_id !== "string" || typeof revision !== "number" || !Number.isInteger(revision)) {
        return { kind: "unknown", raw: params };
      }
      const rawItems = value["items"];
      if (!Array.isArray(rawItems)) return { kind: "unknown", raw: params };
      const items: TodoItemFrame[] = [];
      for (const rawItem of rawItems as unknown[]) {
        const item = parseTodoItem(rawItem);
        if (item === null) return { kind: "unknown", raw: params };
        items.push(item);
      }
      return { kind: "todo", session_id, revision, items };
    }
    case "file_change": {
      const { session_id } = value;
      if (typeof session_id !== "string") return { kind: "unknown", raw: params };
      const change = parseFileChange(value["change"]);
      return change ? { kind: "file_change", session_id, change } : { kind: "unknown", raw: params };
    }
    default:
      return { kind: "unknown", raw: params };
  }
}
