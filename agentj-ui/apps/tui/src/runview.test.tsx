import { describe, expect, test } from "bun:test";
import { act } from "react";
import { testRender } from "@opentui/react/test-utils";
import { rowsFromFrame, nextKey, rowGlyph, RunTranscript, applyTodoFrame, EMPTY_TODOS } from "./runview.tsx";
import { ThemeProvider } from "./theme.tsx";
import { parseProgress } from "@agentj/sdk";
import type { ProgressFrame } from "@agentj/sdk";

describe("rowsFromFrame", () => {
  test("appends a text row per parsed text frame with sequential keys", () => {
    const rows = rowsFromFrame([], { kind: "text", text: "hello" });
    expect(rows).toEqual([{ key: 1, kind: "text", text: "hello" }]);
    expect(nextKey(rows)).toBe(2);
  });
  test("pairs tool_call with its tool_result into one ok row", () => {
    let rows = rowsFromFrame([], {
      kind: "tool_call",
      id: "call-1",
      name: "bash",
      arguments: { cmd: "ls" },
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
});

describe("RunTranscript", () => {
  test("renders tool rows and the usage line", async () => {
    let rows = rowsFromFrame([], {
      kind: "tool_call",
      id: "c1",
      name: "bash",
      arguments: { cmd: "ls" },
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
});
