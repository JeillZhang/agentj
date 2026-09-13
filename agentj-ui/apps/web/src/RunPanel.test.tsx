import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import RunPanel, { ConversationFeed, failRun, fmtDuration, reduceRunFeed, type RunEntry } from "./RunPanel.tsx";
import UnifiedSidebar from "./components/UnifiedSidebar";
import type { ProgressFrame } from "@agentj/sdk";

beforeEach(() => localStorage.clear());

const noopClient = {
  setNotificationHandler: () => {},
  removeNotificationHandler: () => {},
} as unknown as Parameters<typeof RunPanel>[0]["client"];

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
  it("allow_always permission keeps the tool running (returns unchanged, like allow)", () => {
    const feed = [user("go"), tool("c1", "bash")];
    const out = reduceRunFeed(feed, {
      kind: "permission",
      request_id: "r1",
      tool: "bash",
      decision: "allow_always",
    } as ProgressFrame);
    expect(out).toEqual(feed);
    expect((out[1] as { state: string }).state).toBe("running");
  });
  it("a tool_result after allow_always still settles the tool to ok (not swallowed)", () => {
    const approved = reduceRunFeed([user("go"), tool("c1", "bash")], {
      kind: "permission",
      request_id: "r1",
      tool: "bash",
      decision: "allow_always",
    } as ProgressFrame);
    const out = reduceRunFeed(approved, {
      kind: "tool_result",
      call_id: "c1",
      content: "auth.rs\n",
    } as ProgressFrame);
    const t = out[1] as { state: string; output: string };
    expect(t.state).toBe("ok");
    expect(t.output).toBe("auth.rs\n");
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
  it("marks the pending tool row with lucide icons while its dialog is open", () => {
    const entries = [
      user("go"),
      tool("c1", "bash"),
      tool("c2", "write"),
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} pendingTool="write" />);
    const [bashRow, writeRow] = container.querySelectorAll<HTMLElement>(".aj-tool-status");
    expect(bashRow.dataset.state).toBe("running");
    expect(bashRow.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    expect(writeRow.dataset.state).toBe("pending");
    expect(writeRow.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    const dots = container.querySelectorAll<HTMLElement>(".aj-tool-dot");
    expect(dots.length).toBe(2);
    expect(dots[0].dataset.state).toBe("running");
    expect(dots[1].dataset.state).toBe("pending");
  });
  it("renders a failed tool card with the lucide X and its error output", () => {
    const entries = [
      user("go"),
      { kind: "tool", id: "c1", name: "bash", args: null, state: "failed", output: "Error: boom" },
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} />);
    const status = container.querySelector<HTMLElement>(".aj-tool-status")!;
    expect(status.dataset.state).toBe("failed");
    expect(status.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    const dot = container.querySelector<HTMLElement>(".aj-tool-dot")!;
    expect(dot.dataset.state).toBe("failed");
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

describe("RunPanel composer toolbar", () => {
  it("toolbar hosts selectors and send, no status popover", () => {
    render(<RunPanel client={noopClient} activeSessionId="s1" model="deepseek-chat" />);
    const toolbar = document.querySelector(".comp-tools") as HTMLElement;
    expect(toolbar.textContent).toContain("deepseek-chat");
    expect(toolbar.textContent).toContain("Build");
    expect(toolbar.textContent).toContain("均衡");
    expect(within(toolbar).getByRole("button", { name: /发送|send/i })).toBeInTheDocument();
    expect(screen.queryByLabelText("status")).toBeNull();
  });
});

describe("RunPanel demo ask card → QA record", () => {
  it("demo ask card submit appends QA record", () => {
    localStorage.setItem("agentj.demo", "1");
    render(<RunPanel client={noopClient} activeSessionId="s1" model="deepseek-chat" />);
    fireEvent.click(screen.getByText("三栏 + 协议扩展"));
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    expect(document.querySelector(".ask")).toBeNull(); // composer card removed
    const records = document.querySelectorAll(".qa");
    expect(records.length).toBe(2); // seeded DEMO_QA_RECORD + the new record
    const record = records[1] as HTMLElement;
    expect(record.textContent).toContain("模型提问");
    expect(record.textContent).toContain("三栏 + 协议扩展");
    localStorage.clear();
  });
  it("skip removes card without record", () => {
    localStorage.setItem("agentj.demo", "1");
    render(<RunPanel client={noopClient} activeSessionId="s1" />);
    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(document.querySelector(".ask")).toBeNull(); // card removed
    expect(document.querySelectorAll(".qa").length).toBe(1); // only the seeded record remains
    localStorage.clear();
  });
});

describe("RunPanel project picker → UnifiedSidebar sync", () => {
  it("shows a project created via the composer picker in the already-mounted sidebar immediately", () => {
    render(
      <>
        <UnifiedSidebar wsUrl="ws://x" client={null} activeSessionId="s1" onActiveChange={() => {}} />
        <RunPanel client={noopClient} activeSessionId="s1" />
      </>,
    );
    expect(screen.getByRole("button", { name: /Inbox folder/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /New Project folder/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /project picker/i }));
    fireEvent.click(screen.getByText("New Project"));
    expect(screen.getByRole("button", { name: /New Project folder/ })).toBeInTheDocument();
  });
});
