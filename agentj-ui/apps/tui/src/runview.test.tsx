import { describe, expect, test } from "bun:test";
import { act } from "react";
import { testRender } from "@opentui/react/test-utils";
import {
  rowsFromFrame,
  nextKey,
  rowGlyph,
  RunTranscript,
  RunView,
  applyTodoFrame,
  EMPTY_TODO_STATE,
  EMPTY_TODOS,
} from "./runview.tsx";
import { ThemeProvider } from "./theme.tsx";
import { parseProgress } from "@agentj/sdk";
import type { ProgressFrame } from "@agentj/sdk";

const RUN_A = "11111111-1111-4111-8111-111111111111";
const RUN_B = "22222222-2222-4222-8222-222222222222";

describe("RunView", () => {
  // Blank frames used to occupy a row each, so interleaving them is what makes the
  // difference visible: with the filter the visible rows collapse to the text lines.
  const visibleRows = (frame: string): string[] => {
    const lines = frame.split("\n").map((line) => line.trimEnd());
    while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
    return lines;
  };

  test("drops blank and zero-width frames", async () => {
    const setup = await testRender(
      <ThemeProvider>
        <RunView frames={["hello", "\u200b", "world", "  ", "", "\ufeff"]} />
      </ThemeProvider>,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    expect(visibleRows(setup.captureCharFrame()).map((line) => line.trim())).toEqual([
      "",
      "hello",
      "world",
    ]);
    act(() => setup.renderer.destroy());
  });
});

describe("rowsFromFrame", () => {
  test("appends a text row per parsed text frame with sequential keys", () => {
    const rows = rowsFromFrame([], { kind: "text", text: "hello" });
    expect(rows).toEqual([{ key: 1, kind: "text", text: "hello" }]);
    expect(nextKey(rows)).toBe(2);
  });
  test("blank text frames never create a row (keep-alive deltas)", () => {
    let rows = rowsFromFrame([], { kind: "text", text: "hi" });
    for (const blank of ["", "   ", "\n", "\u200b", " \u200b \ufeff "]) {
      rows = rowsFromFrame(rows, { kind: "text", text: blank });
    }
    expect(rows).toEqual([{ key: 1, kind: "text", text: "hi" }]);
  });
  test("pairs tool_call with its tool_result into one ok row", () => {
    let rows = rowsFromFrame([], {
      kind: "tool_call",
      id: "call-1",
      name: "bash",
      arguments: { command: "ls" },
    });
    expect(rows).toHaveLength(1);
    expect(rowGlyph(rows[0] as never)).toBe("│");
    rows = rowsFromFrame(rows, { kind: "tool_result", call_id: "call-1", content: "done ok" });
    expect(rows).toHaveLength(1);
    expect(rowGlyph(rows[0] as never)).toBe("✓");
  });
  test("Error:-prefixed content marks the tool row error", () => {
    let rows = rowsFromFrame([], { kind: "tool_call", id: "c2", name: "bash", arguments: {} });
    rows = rowsFromFrame(rows, { kind: "tool_result", call_id: "c2", content: "Error: boom" });
    expect(rowGlyph(rows[0] as never)).toBe("✗");
  });
  test("usage frame appends the formatted usage row", () => {
    const rows = rowsFromFrame([], { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } });
    expect(rows[0]).toMatchObject({ kind: "usage", text: "in=10 out=5" });
  });
  test("permission decision appends an approval row", () => {
    const rows = rowsFromFrame([], { kind: "permission", request_id: "req-1", tool: "bash", decision: "allow" });
    expect(rows[0]).toMatchObject({ kind: "approval", text: "allow bash" });
    expect(rowGlyph(rows[0] as never)).toBe("△");
  });
  test("unknown frames degrade to a text row instead of throwing", () => {
    const rows = rowsFromFrame([], { kind: "unknown", raw: { weird: true } });
    expect(rows[0]).toMatchObject({ kind: "text" });
    expect((rows[0] as { text: string }).text).toContain("weird");
  });
  test("ignores a file_change frame without adding rows", () => {
    const before = rowsFromFrame([], { kind: "text", text: "hello" });
    const fileChange: ProgressFrame = {
      kind: "file_change",
      session_id: "s1",
      runId: RUN_A,
      change: {
        path: "a",
        op: "modify",
        additions: 1,
        deletions: 0,
        tool_call_id: null,
        diff: null,
        source: "bash",
      },
    };
    let after: typeof before = [];
    expect(() => {
      after = rowsFromFrame(before, fileChange);
    }).not.toThrow();
    expect(after).toHaveLength(1);
    expect(after.some((row) => (row as { kind: string }).kind === "file_change")).toBe(false);
    expect(after[0]).toEqual(before[0]);
  });
});

describe("RunTranscript", () => {
  test("renders tool rows and the usage line", async () => {
    let rows = rowsFromFrame([], {
      kind: "tool_call",
      id: "c1",
      name: "bash",
      arguments: { command: "ls" },
    });
    rows = rowsFromFrame(rows, { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } });
    const setup = await testRender(
      <ThemeProvider>
        <RunTranscript rows={rows} />
      </ThemeProvider>,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame).toContain("bash");
    expect(frame).toContain("in=10 out=5");
    act(() => setup.renderer.destroy());
  });
});

describe("TodoBlock", () => {
  test("renders todo items from a frame", async () => {
    const frame = parseProgress({
      session_id: "s1",
      token: 1,
      value: {
        kind: "todo",
        session_id: "s1",
        revision: 1,
        items: [
          { id: "t1", content: "write tests", status: "completed" },
          { id: "t2", content: "implement block", status: "in_progress" },
          { id: "t3", content: "verify", status: "pending" },
        ],
      },
    });
    expect(frame.kind).toBe("todo");
    const state = frame.kind === "todo" ? applyTodoFrame(EMPTY_TODOS, frame) : EMPTY_TODOS;
    const setup = await testRender(
      <ThemeProvider>
        <RunTranscript rows={[]} todos={state.items} />
      </ThemeProvider>,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    const out = setup.captureCharFrame();
    expect(out).toContain("[x] write tests");
    expect(out).toContain("[~] implement block");
    expect(out).toContain("[ ] verify");
    act(() => setup.renderer.destroy());
  });

  test("ignores stale revisions", () => {
    const rev2 = parseProgress({
      token: 2,
      value: {
        kind: "todo",
        session_id: "s1",
        revision: 2,
        items: [{ id: "t1", content: "second", status: "pending" }],
      },
    });
    const rev1 = parseProgress({
      token: 1,
      value: {
        kind: "todo",
        session_id: "s1",
        revision: 1,
        items: [{ id: "t0", content: "first", status: "completed" }],
      },
    });
    if (rev2.kind !== "todo" || rev1.kind !== "todo") throw new Error("expected todo frames");
    const applied = applyTodoFrame(EMPTY_TODOS, rev2);
    const afterStale = applyTodoFrame(applied, rev1);
    expect(afterStale).toBe(applied);
    expect(afterStale.items).toEqual(rev2.items);
    expect(afterStale.revision).toBe(2);
  });

  test("tracks todo run_id per run and ignores stale same-run revisions", () => {
    const applied = applyTodoFrame(EMPTY_TODO_STATE, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_A,
      revision: 1,
      items: [{ id: "a1", content: "run a first", status: "pending" }],
    });
    expect(applied.items).toHaveLength(1);
    expect(applied.runId).toBe(RUN_A);

    const advanced = applyTodoFrame(applied, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_A,
      revision: 5,
      items: [{ id: "a2", content: "run a fifth", status: "completed" }],
    });
    expect(advanced.revision).toBe(5);

    const stale = applyTodoFrame(advanced, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_A,
      revision: 4,
      items: [{ id: "a0", content: "run a stale", status: "pending" }],
    });
    expect(stale).toBe(advanced);
    expect(stale.items).toEqual(advanced.items);
  });

  test("applies a different run with revision 1 after the first run advanced", () => {
    let state = applyTodoFrame(EMPTY_TODO_STATE, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_A,
      revision: 1,
      items: [{ id: "a1", content: "run a first", status: "pending" }],
    });
    state = applyTodoFrame(state, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_A,
      revision: 9,
      items: [{ id: "a9", content: "run a ninth", status: "completed" }],
    });
    const runB = applyTodoFrame(state, {
      kind: "todo",
      session_id: "s1",
      runId: RUN_B,
      revision: 1,
      items: [{ id: "b1", content: "run b first", status: "in_progress" }],
    });
    expect(runB).not.toBe(state);
    expect(runB.runId).toBe(RUN_B);
    expect(runB.revision).toBe(1);
    expect(runB.items).toEqual([{ id: "b1", content: "run b first", status: "in_progress" }]);
  });

  test("treats a non-UUID runId as the legacy null bucket and still renders", () => {
    const state = applyTodoFrame(EMPTY_TODO_STATE, {
      kind: "todo",
      session_id: "s1",
      runId: "not-a-uuid",
      revision: 1,
      items: [{ id: "l1", content: "legacy bucket", status: "in_progress" }],
    });
    expect(state.items).toHaveLength(1);
    expect(state.runId).toBeNull();
  });
});
