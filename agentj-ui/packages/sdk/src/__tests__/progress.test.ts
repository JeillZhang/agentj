import { describe, expect, it } from "vitest";
import { parseProgress, type ProgressFrame } from "../progress.ts";
import { METHODS } from "../methods.ts";

describe("parseProgress standard wire frames", () => {
  it("parses text deltas", () => {
    const f = parseProgress({ token: 7, value: { kind: "text", delta: "hello" } });
    expect(f).toEqual({ kind: "text", text: "hello" });
  });
  it("parses tool_call with the nested tool object flattened", () => {
    const f = parseProgress({
      token: 7,
      value: { kind: "tool_call", tool_call: { id: "c1", name: "bash", arguments: { cmd: "ls" } } },
    });
    expect(f).toEqual({ kind: "tool_call", id: "c1", name: "bash", arguments: { cmd: "ls" } });
  });
  it("parses tool_result", () => {
    const f = parseProgress({ token: 7, value: { kind: "tool_result", tool_call_id: "c1", content: "auth.rs\n" } });
    expect(f).toEqual({ kind: "tool_result", call_id: "c1", content: "auth.rs\n" });
  });
  it("parses usage settlement", () => {
    const f = parseProgress({ token: 7, value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } } });
    expect(f).toEqual({ kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } });
  });
  it("parses permission settlement with optional session_id", () => {
    const f = parseProgress({
      token: null,
      value: { kind: "permission", request_id: "r1", tool: "bash", decision: "allow", session_id: "s-1" },
    });
    expect(f).toEqual({ kind: "permission", request_id: "r1", tool: "bash", decision: "allow", session_id: "s-1" });
  });
  it("tolerates a missing session_id on permission", () => {
    const f = parseProgress({ token: null, value: { kind: "permission", request_id: "r1", tool: "bash", decision: "deny" } });
    expect(f?.kind).toBe("permission");
  });
  it("returns an unknown frame for unknown kind, non-object params, and missing value (never null)", () => {
    const a = parseProgress({ token: 1, value: { kind: "telemetry", x: 1 } });
    expect(a).toEqual({ kind: "unknown", raw: { token: 1, value: { kind: "telemetry", x: 1 } } });
    expect(parseProgress("junk")).toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 1 })).toMatchObject({ kind: "unknown" });
    expect(parseProgress(null)).toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 1, value: { kind: "text" } })).toMatchObject({ kind: "unknown" });
  });
  it("parses flat legacy frames as text frames (Concretization 11 tolerance)", () => {
    expect(parseProgress({ session_id: "s1", text: "hello" })).toEqual({ kind: "text", text: "hello" });
    expect(parseProgress({ text: "hello" })).toEqual({ kind: "text", text: "hello" });
  });
});

describe("parseProgress todo / file_change frames", () => {
  it("parses a todo frame", () => {
    const f = parseProgress({
      token: 7,
      value: {
        kind: "todo",
        session_id: "s1",
        revision: 2,
        items: [
          { id: "t1", content: "a", status: "in_progress" },
          { id: "t2", content: "b", status: "completed" },
        ],
      },
    });
    expect(f).toEqual({
      kind: "todo",
      session_id: "s1",
      revision: 2,
      items: [
        { id: "t1", content: "a", status: "in_progress" },
        { id: "t2", content: "b", status: "completed" },
      ],
    });
  });
  it("tolerates an unknown todo status and an empty snapshot", () => {
    const f = parseProgress({
      token: 7,
      value: { kind: "todo", session_id: "s1", revision: 1, items: [{ id: "t1", content: "a", status: "blocked" }] },
    });
    expect(f).toEqual({
      kind: "todo",
      session_id: "s1",
      revision: 1,
      items: [{ id: "t1", content: "a", status: "blocked" }],
    });
    expect(parseProgress({ token: 7, value: { kind: "todo", session_id: "s1", revision: 3, items: [] } }))
      .toEqual({ kind: "todo", session_id: "s1", revision: 3, items: [] });
  });
  it("degrades malformed todo to unknown without throwing", () => {
    expect(parseProgress({ token: 7, value: { kind: "todo", session_id: "s1", revision: 2, items: 3 } }))
      .toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 7, value: { kind: "todo", session_id: "s1", items: [] } }))
      .toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 7, value: { kind: "todo", revision: 2, items: [] } }))
      .toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 7, value: { kind: "todo", session_id: "s1", revision: 1, items: [{ id: "t1" }] } }))
      .toMatchObject({ kind: "unknown" });
    const badItem = { id: 1, content: "a", status: "pending" };
    expect(parseProgress({ token: 7, value: { kind: "todo", session_id: "s1", revision: 1, items: [badItem] } }))
      .toMatchObject({ kind: "unknown" });
  });
  it("parses a file_change frame with nullable fields passed through", () => {
    const f = parseProgress({
      token: 7,
      value: {
        kind: "file_change",
        session_id: "s1",
        change: {
          path: "src/a.ts",
          op: "modify",
          additions: 3,
          deletions: 1,
          tool_call_id: "c1",
          diff: "@@ -1 +1 @@",
          diff_truncated: true,
        },
      },
    });
    expect(f).toEqual({
      kind: "file_change",
      session_id: "s1",
      change: {
        path: "src/a.ts",
        op: "modify",
        additions: 3,
        deletions: 1,
        tool_call_id: "c1",
        diff: "@@ -1 +1 @@",
        diff_truncated: true,
      },
    });
  });
  it("defaults omitted nullable file_change fields to null", () => {
    const f = parseProgress({
      token: null,
      value: {
        kind: "file_change",
        session_id: "s1",
        change: { path: "new.txt", op: "add", additions: 1, deletions: 0 },
      },
    });
    expect(f).toEqual({
      kind: "file_change",
      session_id: "s1",
      change: { path: "new.txt", op: "add", additions: 1, deletions: 0, tool_call_id: null, diff: null },
    });
  });
  it("degrades malformed file_change to unknown", () => {
    const partialChange = { path: "a", op: "add", additions: 1 };
    expect(parseProgress({ token: null, value: { kind: "file_change", session_id: "s1", change: partialChange } }))
      .toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: null, value: { kind: "file_change", session_id: "s1" } }))
      .toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: null, value: { kind: "file_change", change: {} } }))
      .toMatchObject({ kind: "unknown" });
  });
  it("keeps future unknown kinds ignored", () => {
    expect(parseProgress({ token: 1, value: { kind: "future_kind", x: 1 } })).toMatchObject({ kind: "unknown" });
  });
});

describe("regenerated method constants", () => {
  it("exposes the two new methods", () => {
    expect(METHODS.sessionList).toBe("session/list");
    expect(METHODS.sessionHistory).toBe("session/history");
  });
});
