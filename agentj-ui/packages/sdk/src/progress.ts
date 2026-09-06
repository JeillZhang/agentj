export type UsageInfo = { prompt_tokens: number; completion_tokens: number };

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
  | { kind: "unknown"; raw: unknown };

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object";
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
    default:
      return { kind: "unknown", raw: params };
  }
}
