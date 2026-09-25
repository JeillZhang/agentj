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

describe("parseProgress run_id / file_change source enrichment", () => {
  const RUN_A = "3f2a1c4e-5b6d-4e8f-9a0b-1c2d3e4f5a6b";
  const RUN_B = "7c8d9e0f-1a2b-4c3d-8e4f-5a6b7c8d9e0f";

  it("parses a todo frame with a UUID run_id and omits runId when absent", () => {
    const withRun = parseProgress({
      token: 7,
      value: {
        kind: "todo",
        session_id: "s1",
        run_id: RUN_A,
        revision: 3,
        items: [{ id: "a", content: "t", status: "pending" }],
      },
    });
    expect(withRun.kind).toBe("todo");
    if (withRun.kind === "todo") {
      expect(withRun.runId).toBe(RUN_A);
      expect(withRun.revision).toBe(3);
    }
    const withoutRun = parseProgress({
      token: 7,
      value: { kind: "todo", session_id: "s1", revision: 3, items: [] },
    });
    expect(withoutRun.kind).toBe("todo");
    if (withoutRun.kind === "todo") expect(withoutRun.runId).toBeUndefined();
  });

  it("parses a file_change frame with source, seq and total_bytes but no diff", () => {
    const frame = parseProgress({
      token: 7,
      value: {
        kind: "file_change",
        session_id: "s1",
        run_id: RUN_A,
        change: {
          path: "a.txt",
          op: "modify",
          additions: 1,
          deletions: 0,
          source: "bash",
          seq: 5,
          total_bytes: 70000,
        },
      },
    });
    expect(frame.kind).toBe("file_change");
    if (frame.kind === "file_change") {
      expect(frame.runId).toBe(RUN_A);
      expect(frame.change.source).toBe("bash");
      expect(frame.change.seq).toBe(5);
      expect(frame.change.total_bytes).toBe(70000);
      expect(frame.change.diff).toBeNull();
    }
  });

  it("preserves an unknown file_change source verbatim without throwing", () => {
    const build = () =>
      parseProgress({
        token: 7,
        value: {
          kind: "file_change",
          session_id: "s1",
          change: { path: "a", op: "modify", additions: 0, deletions: 0, source: "edit" },
        },
      });
    expect(build).not.toThrow();
    const frame = build();
    expect(frame.kind).toBe("file_change");
    if (frame.kind === "file_change") expect(frame.change.source).toBe("edit");
  });

  it("keeps legacy file_change without source: diff preserved or null", () => {
    const withDiff = parseProgress({
      token: 7,
      value: {
        kind: "file_change",
        session_id: "s1",
        change: { path: "a", op: "modify", additions: 1, deletions: 0, diff: "@@ -1 +1 @@" },
      },
    });
    expect(withDiff.kind).toBe("file_change");
    if (withDiff.kind === "file_change") {
      expect(withDiff.change.diff).toBe("@@ -1 +1 @@");
      expect(withDiff.change.source).toBeUndefined();
    }
    const noDiff = parseProgress({
      token: 7,
      value: {
        kind: "file_change",
        session_id: "s1",
        change: { path: "a", op: "modify", additions: 1, deletions: 0 },
      },
    });
    expect(noDiff.kind).toBe("file_change");
    if (noDiff.kind === "file_change") expect(noDiff.change.diff).toBeNull();
  });

  it("drops an illegal run_id (non-UUID or >128 chars) without throwing", () => {
    const longId = "x".repeat(129);
    for (const badId of ["not-a-uuid", longId]) {
      const todo = parseProgress({
        token: 7,
        value: { kind: "todo", session_id: "s1", run_id: badId, revision: 1, items: [] },
      });
      expect(todo.kind).toBe("todo");
      if (todo.kind === "todo") expect(todo.runId).toBeUndefined();

      const fileChange = parseProgress({
        token: 7,
        value: {
          kind: "file_change",
          session_id: "s1",
          run_id: badId,
          change: { path: "a", op: "modify", additions: 1, deletions: 0, run_id: badId },
        },
      });
      expect(fileChange.kind).toBe("file_change");
      if (fileChange.kind === "file_change") {
        expect(fileChange.runId).toBeUndefined();
        expect(fileChange.change.run_id).toBeUndefined();
      }
    }
  });

  it("prefers the inner change.run_id over the outer run_id", () => {
    const frame = parseProgress({
      token: 7,
      value: {
        kind: "file_change",
        session_id: "s1",
        run_id: RUN_A,
        change: { path: "a", op: "modify", additions: 1, deletions: 0, run_id: RUN_B },
      },
    });
    expect(frame.kind).toBe("file_change");
    if (frame.kind === "file_change") {
      expect(frame.change.run_id).toBe(RUN_B);
      expect(frame.runId).toBe(RUN_B);
    }
  });

  it("returns the tolerant fallback for non-object or unknown-kind input", () => {
    expect(parseProgress("junk")).toMatchObject({ kind: "unknown" });
    expect(parseProgress(null)).toMatchObject({ kind: "unknown" });
    expect(parseProgress({ token: 1, value: { kind: "future_kind" } })).toMatchObject({ kind: "unknown" });
  });
});
