import "@testing-library/jest-dom/vitest";
import { describe, expect, it } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ConversationFeed, failRun, fmtDuration, reduceRunFeed, type RunEntry } from "./RunPanel.tsx";
import type { ProgressFrame } from "@agentj/sdk";

const user = (text: string): RunEntry => ({ kind: "user", text });
const ai = (text: string): RunEntry => ({ kind: "ai", text });
const tool = (id: string, name: string): RunEntry =>
  ({ kind: "tool", id, name, args: null, state: "running" }) as RunEntry;

describe("reduceRunFeed", () => {
  it("appends text then usage in order after a user entry", () => {
    const feed = [user("hi")];
    const out = reduceRunFeed(
      reduceRunFeed(feed, { kind: "text", text: "hello" } as ProgressFrame),
      { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } } as ProgressFrame,
    );
    expect(out.map((e) => e.kind)).toEqual(["user", "ai", "usage"]);
    expect((out[1] as { text: string }).text).toBe("hello");
  });
  it("settles a running tool by call id", () => {
    const out = reduceRunFeed([user("go"), tool("c1", "bash")], {
      kind: "tool_result",
      call_id: "c1",
      content: "auth.rs\n",
    } as ProgressFrame);
    const t = out[1] as { state: string; output: string };
    expect(t.state).toBe("ok");
    expect(t.output).toBe("auth.rs\n");
  });
  it("unmatched tool_result settles the newest running entry without crashing", () => {
    const out = reduceRunFeed([user("go"), tool("c1", "bash")], {
      kind: "tool_result",
      call_id: "zz",
      content: "x",
    } as ProgressFrame);
    expect((out[1] as { state: string }).state).toBe("ok");
    expect(out).toHaveLength(2);
  });
  it("deny permission settles the newest running entry of that tool", () => {
    const out = reduceRunFeed([user("go"), tool("c1", "bash")], {
      kind: "permission",
      request_id: "r1",
      tool: "bash",
      decision: "deny",
    } as ProgressFrame);
    expect((out[1] as { state: string }).state).toBe("denied");
  });
  it("an error-prefixed tool_result settles the card as failed, not ok", () => {
    const out = reduceRunFeed([user("go"), tool("c1", "bash")], {
      kind: "tool_result",
      call_id: "c1",
      content: "Error: Permission denied",
    } as ProgressFrame);
    expect((out[1] as { state: string }).state).toBe("failed");
  });
  it("a late tool_result for a denied card is ignored (no double settle)", () => {
    const denied = reduceRunFeed(
      [user("go"), tool("c1", "bash"), tool("c2", "write")],
      { kind: "permission", request_id: "r1", tool: "bash", decision: "deny" } as ProgressFrame,
    );
    expect((denied[1] as { state: string }).state).toBe("denied");
    const out = reduceRunFeed(denied, {
      kind: "tool_result",
      call_id: "c1",
      content: "Error: denied by user",
    } as ProgressFrame);
    expect((out[1] as { state: string }).state).toBe("denied");
    expect((out[2] as { state: string }).state).toBe("running");
    expect(out).toHaveLength(3);
  });
  it("failRun settles running cards to failed and appends a red error entry", () => {
    const out = failRun([user("go"), tool("c1", "bash"), tool("c2", "write")], "run:boom");
    expect((out[1] as { state: string }).state).toBe("failed");
    expect((out[2] as { state: string }).state).toBe("failed");
    expect(out).toHaveLength(4);
    expect(out[3]).toEqual({ kind: "error", text: "run:boom" });
  });
  it("failRun leaves already settled cards untouched", () => {
    const done = { ...tool("c1", "bash"), state: "ok" } as RunEntry;
    const out = failRun([user("go"), done], "x");
    expect(out).toHaveLength(3);
    expect((out[1] as { state: string }).state).toBe("ok");
  });
  it("closes the ai entry before a tool call", () => {
    const withTool = reduceRunFeed([user("go"), ai("thinking")], {
      kind: "tool_call",
      id: "c1",
      name: "bash",
      arguments: { cmd: "ls" },
    } as ProgressFrame);
    expect(withTool.map((e) => e.kind)).toEqual(["user", "ai", "tool"]);
    const closed = reduceRunFeed(withTool, { kind: "text", text: "late" } as ProgressFrame);
    expect(closed).toHaveLength(4);
  });
  it("ignores unknown frames without touching the list", () => {
    const out = reduceRunFeed([user("go")], { kind: "unknown", raw: { x: 1 } } as ProgressFrame);
    expect(out).toEqual([user("go")]);
  });
});

describe("ConversationFeed rendering", () => {
  it("shows usage meta row from a usage entry", () => {
    const entries: RunEntry[] = [
      user("hi"),
      ai("hello"),
      { kind: "usage", prompt_tokens: 10, completion_tokens: 5 },
    ];
    render(<ConversationFeed entries={entries} />);
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(screen.getByText("in=10 out=5")).toBeInTheDocument();
  });
  it("meta row shows model and elapsed before the tokens when provided", () => {
    const entries: RunEntry[] = [
      user("hi"),
      ai("hello"),
      { kind: "usage", prompt_tokens: 10, completion_tokens: 5 },
    ];
    render(<ConversationFeed entries={entries} model="deepseek-chat" elapsedMs={1234} />);
    expect(screen.getByText("deepseek-chat")).toBeInTheDocument();
    expect(screen.getByText("1.23s")).toBeInTheDocument();
    expect(screen.getByText("in=10 out=5")).toBeInTheDocument();
  });
  it("fmtDuration formats sub-second and second-range spans deterministically", () => {
    expect(fmtDuration(412)).toBe("412ms");
    expect(fmtDuration(1234)).toBe("1.23s");
    expect(fmtDuration(65_000)).toBe("65.00s");
  });
  it("renders gfm markdown in the ai entry", () => {
    const entries = [
      user("hi"),
      { kind: "ai", text: "**bold**\n\n| a | b |\n|---|---|\n| 1 | 2 |" },
    ] as RunEntry[];
    render(<ConversationFeed entries={entries} />);
    expect(document.querySelector("strong")?.textContent).toBe("bold");
    expect(document.querySelector("table")).not.toBeNull();
  });
  it("marks the pending tool row with △ while its dialog is open", () => {
    const entries = [
      user("go"),
      tool("c1", "bash"),
      tool("c2", "write"),
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} pendingTool="write" />);
    const [bashRow, writeRow] = container.querySelectorAll<HTMLElement>(".aj-tool-status");
    expect(bashRow.textContent).toBe("│");
    expect(bashRow.dataset.state).toBe("running");
    expect(writeRow.textContent).toBe("△");
    expect(writeRow.dataset.state).toBe("pending");
  });
  it("renders a failed tool card with the ✗ glyph and its error output", () => {
    const entries = [
      user("go"),
      { kind: "tool", id: "c1", name: "bash", args: null, state: "failed", output: "Error: boom" },
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} />);
    const status = container.querySelector<HTMLElement>(".aj-tool-status")!;
    expect(status.textContent).toBe("✗");
    expect(status.dataset.state).toBe("failed");
    const card = screen.getByRole("region", { name: /bash/ });
    expect(card.textContent).toContain("Error: boom");
    expect(screen.getByRole("button", { name: /expand/i })).toBeInTheDocument();
  });
  it("renders a run error entry with role alert", () => {
    const entries = [
      user("go"),
      ai("partial"),
      { kind: "error", text: "run:boom" },
    ] as RunEntry[];
    render(<ConversationFeed entries={entries} />);
    expect(screen.getByRole("alert")).toHaveTextContent("run:boom");
  });
  it("expands a folded tool output on click", () => {
    const long = Array.from({ length: 8 }, (_, i) => `line ${i}`).join("\n");
    const entries = [
      user("go"),
      { kind: "tool", id: "c1", name: "bash", args: { cmd: "ls" }, state: "ok", output: long },
    ] as RunEntry[];
    render(<ConversationFeed entries={entries} />);
    const card = screen.getByRole("region", { name: /bash/ });
    expect(card.textContent).toContain("line 0");
    expect(card.textContent).not.toContain("line 7");
    fireEvent.click(screen.getByRole("button", { name: /expand/i }));
    expect(card.textContent).toContain("line 7");
  });
});
