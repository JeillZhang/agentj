import { describe, expect, test } from "bun:test";
import { act } from "react";
import { testRender } from "@opentui/react/test-utils";
import { rowsFromFrame, nextKey, rowGlyph, RunTranscript } from "./runview.tsx";
import { ThemeProvider } from "./theme.tsx";
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
