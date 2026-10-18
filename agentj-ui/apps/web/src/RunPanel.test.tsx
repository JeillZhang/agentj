import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, within, act, waitFor } from "@testing-library/react";
import RunPanel, { ConversationFeed, failRun, fmtDuration, reduceRunFeed, toolSummary, type RunEntry } from "./RunPanel.tsx";
import { createProject, setCurrentProject } from "./projectStore";
import { setDemoEnabled } from "./demoData";
import { formatUsageTitle } from "./tokenUsage";
import { METHODS, type ProgressFrame } from "@agentj/sdk";
import type { ModelEntry } from "./components/ComposerOptions";

beforeEach(() => localStorage.clear());

const noopClient = {
  setNotificationHandler: () => {},
  removeNotificationHandler: () => {},
} as unknown as Parameters<typeof RunPanel>[0]["client"];

const user = (text: string): RunEntry => ({ kind: "user", text });
const ai = (text: string): RunEntry => ({ kind: "ai", text });
const tool = (id: string, name: string): RunEntry =>
  ({ kind: "tool", id, name, args: null, state: "running" }) as RunEntry;

const propsWithError = (text: string) => ({
  client: noopClient,
  activeSessionId: "s1",
  initialEntries: [{ kind: "error", text }] as RunEntry[],
});

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
  it("shows the bash command beside the tool name", () => {
    const entries = [
      user("go"),
      { kind: "tool", id: "c1", name: "bash", args: { command: "git status --short" }, state: "ok", output: "" },
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} />);
    const summary = container.querySelector<HTMLElement>(".aj-tool-summary")!;
    expect(summary.textContent).toBe("git status --short");
    expect(summary.getAttribute("title")).toBe("git status --short");
  });
  it("shows the target path for read and write tools", () => {
    const entries = [
      user("go"),
      { kind: "tool", id: "c1", name: "read", args: { path: "src/App.tsx" }, state: "ok", output: "" },
      { kind: "tool", id: "c2", name: "write", args: { path: "src/RunPanel.tsx" }, state: "ok", output: "" },
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} />);
    const summaries = [...container.querySelectorAll<HTMLElement>(".aj-tool-summary")];
    expect(summaries.map((s) => s.textContent)).toEqual(["src/App.tsx", "src/RunPanel.tsx"]);
  });
  it("summarizes grep as pattern + include and glob as pattern", () => {
    expect(toolSummary({ kind: "tool", id: "c", name: "grep", args: { pattern: "TODO", include: "*.ts" }, state: "running" })).toBe("TODO · *.ts");
    expect(toolSummary({ kind: "tool", id: "c", name: "grep", args: { pattern: "TODO" }, state: "running" })).toBe("TODO");
    expect(toolSummary({ kind: "tool", id: "c", name: "grep", args: { pattern: "TODO", include: "*" }, state: "running" })).toBe("TODO");
    expect(toolSummary({ kind: "tool", id: "c", name: "glob", args: { pattern: "src/**/*.tsx" }, state: "running" })).toBe("src/**/*.tsx");
    expect(toolSummary({ kind: "tool", id: "c", name: "todo", args: { items: [{}, {}] }, state: "running" })).toBe("2 项");
  });
  it("omits the summary when args are absent or the tool is unknown", () => {
    expect(toolSummary({ kind: "tool", id: "c", name: "bash", args: null, state: "running" })).toBe("");
    expect(toolSummary({ kind: "tool", id: "c", name: "bash", args: [1, 2], state: "running" })).toBe("");
    expect(toolSummary({ kind: "tool", id: "c", name: "bash", args: 42, state: "running" })).toBe("");
    expect(toolSummary({ kind: "tool", id: "c", name: "mystery", args: { x: 1 }, state: "running" })).toBe("");
    const { container } = render(<ConversationFeed entries={[user("go"), tool("c1", "bash")] as RunEntry[]} />);
    expect(container.querySelector(".aj-tool-summary")).toBeNull();
  });

  // BUG-017: gateways stream a lone ZWSP (U+200B) between tool calls; each one
  // used to render an empty `.aj-ai` bubble (blue rule, no text).
  it("renders no assistant bubble for zero-width-only text", () => {
    const entries = [
      user("go"),
      ai("\u200b"),
      ai("\u200b\u200b\u200b"),
      ai(" \n\t"),
      tool("c1", "bash"),
    ] as RunEntry[];
    const { container } = render(<ConversationFeed entries={entries} />);
    expect(container.querySelectorAll(".aj-ai")).toHaveLength(0);
  });

  it("still renders an assistant bubble for zero-width-prefixed real text", () => {
    const { container } = render(<ConversationFeed entries={[ai("\u200b分析结果")] as RunEntry[]} />);
    const bubbles = container.querySelectorAll(".aj-ai");
    expect(bubbles).toHaveLength(1);
    expect(bubbles[0].textContent).toContain("分析结果");
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

describe("RunPanel run overrides", () => {
  function captureClient() {
    const calls: { method: string; params: Record<string, unknown> }[] = [];
    const client = {
      setNotificationHandler: () => {},
      removeNotificationHandler: () => {},
      request: async (method: string, params: Record<string, unknown>) => {
        calls.push({ method, params });
        return null;
      },
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
    return { client, calls };
  }

  async function submitRun(selected: ModelEntry | null): Promise<Record<string, unknown>> {
    const { client, calls } = captureClient();
    render(<RunPanel client={client} activeSessionId="s1" selected={selected} />);
    fireEvent.change(screen.getByLabelText(/prompt/i), { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(calls.length).toBe(1));
    expect(calls[0].method).toBe(METHODS.sessionRun);
    return calls[0].params;
  }

  it("submits provider and model when a model is selected", async () => {
    const params = await submitRun({ name: "opencode-go/kimi-k3", provider: "opencode-go" });
    expect(params).toMatchObject({
      content: "hi",
      model: "opencode-go/kimi-k3",
      provider: "opencode-go",
    });
  });

  it("submits model only when the entry has no provider", async () => {
    const params = await submitRun({ name: "bare-model" });
    expect(params).toMatchObject({ content: "hi", model: "bare-model" });
    expect(params).not.toHaveProperty("provider");
  });

  it("submits nothing extra when no model is selected", async () => {
    const params = await submitRun(null);
    expect(Object.keys(params).sort()).toEqual(["content", "session_id"]);
  });

  // The chip used to name the configured default forever, whatever the picker sent.
  // @since 0.29.8
  it("shows the picked model in the header chip instead of the configured default", () => {
    const { container } = render(
      <RunPanel
        client={noopClient}
        activeSessionId="s1"
        model="DeepSeek-Flash-V4.1"
        selected={{ name: "opencode-go/kimi-k3", provider: "opencode-go" }}
      />,
    );
    const chip = container.querySelector(".aj-chip--model") as HTMLElement;
    expect(chip).toHaveTextContent("opencode-go/kimi-k3");
    expect(chip.textContent).not.toContain("DeepSeek-Flash-V4.1");
  });

  it("falls back to the configured default when nothing is picked", () => {
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" model="DeepSeek-Flash-V4.1" />,
    );
    expect(container.querySelector(".aj-chip--model")).toHaveTextContent("DeepSeek-Flash-V4.1");
  });
});

// BUG-018: the sidebar keeps its own session list, so it has to be told when a
// turn settles (preview text / updated_at changed server-side).
describe("RunPanel run completion announcement", () => {
  it("dispatches agentj:sessions-updated after a turn settles", async () => {
    const seen = vi.fn();
    window.addEventListener("agentj:sessions-updated", seen);
    const rpc = {
      setNotificationHandler: () => {},
      removeNotificationHandler: () => {},
      request: async (method: string) =>
        method === METHODS.sessionRun
          ? { usage: { prompt_tokens: 1, completion_tokens: 2 } }
          : null,
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
    render(<RunPanel client={rpc} activeSessionId="s1" />);
    fireEvent.change(screen.getByLabelText(/prompt/i), { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(seen).toHaveBeenCalled());
    window.removeEventListener("agentj:sessions-updated", seen);
  });

  it("announces even when the turn fails", async () => {
    const seen = vi.fn();
    window.addEventListener("agentj:sessions-updated", seen);
    const rpc = {
      setNotificationHandler: () => {},
      removeNotificationHandler: () => {},
      request: async () => {
        throw new Error("run:boom");
      },
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
    render(<RunPanel client={rpc} activeSessionId="s1" />);
    fireEvent.change(screen.getByLabelText(/prompt/i), { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(seen).toHaveBeenCalled());
    window.removeEventListener("agentj:sessions-updated", seen);
  });
});

describe("RunPanel usage double-count guard", () => {
  it("counts a turn once when a usage frame precedes the run result", async () => {
    let handler: ((method: string, params: unknown) => void) | null = null;
    const client = {
      setNotificationHandler: (h: (method: string, params: unknown) => void) => { handler = h; },
      removeNotificationHandler: () => { handler = null; },
      request: async (method: string) => {
        if (method === METHODS.sessionRun) {
          return { usage: { prompt_tokens: 300, completion_tokens: 200 } };
        }
        return null;
      },
    } as unknown as Parameters<typeof RunPanel>[0]["client"];

    const { container } = render(<RunPanel client={client} activeSessionId="s1" />);
    fireEvent.change(screen.getByLabelText(/prompt/i), { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    act(() => {
      handler?.(METHODS.progress, {
        token: 1,
        value: { kind: "usage", usage: { prompt_tokens: 300, completion_tokens: 200 } },
      });
    });
    await waitFor(() => expect(container.querySelectorAll(".aj-composer__usage").length).toBe(1));
    const readout = container.querySelector(".aj-composer__usage") as HTMLElement;
    expect(readout).toHaveTextContent("Σ 500 · 输入 300 · 输出 200");
    expect(container.querySelectorAll(".aj-composer__usage").length).toBe(1);
  });
});

describe("RunPanel usage readout placement", () => {
  it("renders the readout inside the composer header, after the model chip", () => {
    const entries: RunEntry[] = [
      user("hi"),
      ai("hello"),
      { kind: "usage", prompt_tokens: 300, completion_tokens: 200 },
    ];
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" model="deepseek-chat" initialEntries={entries} />,
    );
    const header = container.querySelector(".aj-composer__header") as HTMLElement;
    const chip = header.querySelector(".aj-chip--model") as HTMLElement;
    const readout = header.querySelector(".aj-composer__usage") as HTMLElement;
    expect(chip).not.toBeNull();
    expect(readout).not.toBeNull();
    expect(readout).toHaveTextContent("Σ 500 · 输入 300 · 输出 200");
    expect(readout).toHaveAttribute("aria-label", formatUsageTitle({ prompt: 300, completion: 200 }));
    const ordered = header.querySelectorAll(".aj-chip--model, .aj-composer__usage");
    expect(ordered.length).toBe(2);
    expect(ordered[0]).toBe(chip);
    expect(ordered[1]).toBe(readout);
    expect(container.querySelector(".comp-tools .aj-composer__usage")).toBeNull();
  });

  it("omits the readout when there are no usage entries", () => {
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" model="deepseek-chat" />,
    );
    expect(container.querySelector(".aj-composer__usage")).toBeNull();
  });

  it("omits the readout for a zero-total usage entry", () => {
    const entries: RunEntry[] = [{ kind: "usage", prompt_tokens: 0, completion_tokens: 0 }];
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" model="deepseek-chat" initialEntries={entries} />,
    );
    expect(container.querySelector(".aj-composer__usage")).toBeNull();
  });

  it("renders the readout alone in the header when no model is provided", () => {
    const entries: RunEntry[] = [{ kind: "usage", prompt_tokens: 300, completion_tokens: 200 }];
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" initialEntries={entries} />,
    );
    const header = container.querySelector(".aj-composer__header") as HTMLElement;
    expect(header.querySelector(".aj-composer__usage")).not.toBeNull();
    expect(header.querySelector(".aj-chip--model")).toBeNull();
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
  it("seeds the demo QA record alongside non-empty initialEntries exactly once", () => {
    localStorage.setItem("agentj.demo", "1");
    const history: RunEntry[] = [user("历史消息"), ai("收到")];
    const { container } = render(
      <RunPanel client={noopClient} activeSessionId="s1" initialEntries={history} />,
    );
    const records = container.querySelectorAll(".qa");
    expect(records.length).toBe(1);
    expect(records[0].textContent).toContain("是否同时更新 TUI？");
    expect(container.querySelectorAll(".qa-q").length).toBe(1);
    localStorage.clear();
  });
  it("hides the ask card when demo mode turns off", () => {
    localStorage.setItem("agentj.demo", "1");
    render(<RunPanel client={noopClient} activeSessionId="s1" />);
    expect(document.querySelector(".ask")).not.toBeNull();
    act(() => setDemoEnabled(false));
    expect(document.querySelector(".ask")).toBeNull();
    localStorage.clear();
  });
});

type AskNotify = (method: string, params: unknown) => void;

function makeAskClient(result: unknown = { accepted: true }) {
  let handler: AskNotify | null = null;
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  const client = {
    setNotificationHandler: (h: AskNotify) => { handler = h; },
    removeNotificationHandler: () => { handler = null; },
    request: async (method: string, params: Record<string, unknown>) => {
      calls.push({ method, params });
      return result;
    },
  } as unknown as Parameters<typeof RunPanel>[0]["client"];
  return { client, calls, notify: (method: string, params: unknown) => handler?.(method, params) };
}

function realAskParams() {
  return {
    question_id: "q1",
    session_id: "s1",
    question: "请选择本次重构的范围？",
    header: "确认范围",
    options: [
      { id: "ui", label: "仅前端三栏布局" },
      { id: "full", label: "三栏 + 协议扩展", hint: "推荐" },
    ],
    allow_multiple: false,
    allow_other: true,
  };
}

describe("RunPanel real $/askQuestion → question/respond", () => {
  it("renders a real question and responds", async () => {
    const { client, calls, notify } = makeAskClient();
    render(<RunPanel client={client} activeSessionId="s1" />);
    act(() => notify(METHODS.askQuestion, realAskParams()));
    expect(screen.getByText("请选择本次重构的范围？")).toBeInTheDocument();
    fireEvent.click(screen.getByText("仅前端三栏布局"));
    fireEvent.change(screen.getByLabelText("其他回答"), { target: { value: "补充说明" } });
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toEqual({
      method: METHODS.questionRespond,
      params: {
        question_id: "q1",
        answers: ["ui", "other:补充说明"],
        skipped: false,
      },
    });
  });

  it("ignores other-session questions", () => {
    const { client, notify } = makeAskClient();
    render(<RunPanel client={client} activeSessionId="s1" />);
    act(() => notify(METHODS.askQuestion, { ...realAskParams(), session_id: "s2" }));
    expect(screen.queryByText("请选择本次重构的范围？")).toBeNull();
  });

  it("appends a QA record after submit", async () => {
    const { client, notify } = makeAskClient();
    const { container } = render(<RunPanel client={client} activeSessionId="s1" />);
    act(() => notify(METHODS.askQuestion, realAskParams()));
    fireEvent.click(screen.getByText("三栏 + 协议扩展"));
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    await waitFor(() => expect(container.querySelectorAll(".qa").length).toBe(1));
    const record = container.querySelector<HTMLElement>(".qa")!;
    expect(record.textContent).toContain("请选择本次重构的范围？");
    expect(record.textContent).toContain("三栏 + 协议扩展");
    expect(container.querySelector(".ask")).toBeNull();
  });

  it("skip sends skipped:true and records 已跳过", async () => {
    const { client, calls, notify } = makeAskClient();
    const { container } = render(<RunPanel client={client} activeSessionId="s1" />);
    act(() => notify(METHODS.askQuestion, realAskParams()));
    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0].params).toEqual({ question_id: "q1", answers: [], skipped: true });
    await waitFor(() => expect(container.querySelector(".qa")?.textContent).toContain("已跳过"));
    expect(container.querySelector(".ask")).toBeNull();
  });
});

describe("RunPanel project picker label follows project changes", () => {
  it("updates the composer picker label when the current project changes elsewhere", () => {
    render(<RunPanel client={noopClient} activeSessionId="s1" />);
    const picker = screen.getByRole("button", { name: /project picker/i });
    expect(picker).toHaveTextContent("Inbox");

    const pick = createProject("MenuPick");
    act(() => setCurrentProject(pick.id));

    expect(screen.getByRole("button", { name: /project picker/i })).toHaveTextContent("MenuPick");
  });
});

describe("ConversationFeed code fences", () => {
  it("renders fenced code as a CodeBlockCard", () => {
    const entries = [{ kind: "ai" as const, text: "```rust\nfn main() {}\n```" }];
    const { container } = render(<ConversationFeed entries={entries} />);
    expect(container.querySelector(".aj-code")).not.toBeNull();
    expect(container.querySelector(".aj-code__lang")?.textContent).toBe("rust");
    expect(screen.getByRole("button", { name: "复制代码" })).toBeTruthy();
    expect(container.querySelector(".hljs-keyword")).not.toBeNull();
  });
  it("keeps inline code inline", () => {
    const entries = [{ kind: "ai" as const, text: "use `cargo test` now" }];
    const { container } = render(<ConversationFeed entries={entries} />);
    expect(container.querySelector(".aj-code")).toBeNull();
    expect(container.querySelector(".aj-ai code")).not.toBeNull();
  });

  it("renders an unclosed fence as a code card (streaming)", () => {
    const entries = [{ kind: "ai" as const, text: "```rust\nfn main() {" }];
    const { container } = render(<ConversationFeed entries={entries} />);
    expect(container.querySelector(".aj-code")).not.toBeNull();
  });
});

describe("RunPanel project picker edit", () => {
  it("edits a project from the picker without switching", async () => {
    localStorage.clear();
    createProject("Alpha");
    const beta = createProject("Beta");
    setCurrentProject(beta.id);
    render(<RunPanel client={null} activeSessionId={null} initialEntries={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "project picker" }));
    fireEvent.click(screen.getByRole("button", { name: "编辑项目 Alpha" }));
    expect(screen.getByRole("dialog", { name: "编辑项目" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "project picker" }).textContent).toContain("Beta");
  });
});

describe("RunPanel composer height CSS variable", () => {
  it("publishes the composer full box height as --aj-composer-h", () => {
    const captured: { cb: ResizeObserverCallback | null } = { cb: null };
    const observe = vi.fn();
    const disconnect = vi.fn();
    class FakeResizeObserver {
      constructor(cb: ResizeObserverCallback) {
        captured.cb = cb;
      }
      observe = observe;
      unobserve = () => {};
      disconnect = disconnect;
    }
    vi.stubGlobal("ResizeObserver", FakeResizeObserver);
    try {
      const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
      const composer = container.querySelector(".aj-composer-card") as HTMLElement;
      const card = container.querySelector(".aj-card") as HTMLElement;
      expect(observe).toHaveBeenCalledWith(composer);
      act(() => {
        const entry = {
          target: composer,
          borderBoxSize: [{ blockSize: 180, inlineSize: 100 }],
        } as unknown as ResizeObserverEntry;
        captured.cb?.([entry], {} as ResizeObserver);
      });
      expect(card.style.getPropertyValue("--aj-composer-h")).toBe("180px");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("renders without crashing when ResizeObserver is undefined", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    try {
      render(<RunPanel client={noopClient} activeSessionId="s1" />);
      expect(document.querySelector(".aj-composer-card")).not.toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("ConversationFeed collapse reporting", () => {
  function setScroll(
    el: HTMLElement,
    { top, height, client }: { top: number; height: number; client: number },
  ) {
    Object.defineProperty(el, "scrollHeight", { configurable: true, value: height });
    Object.defineProperty(el, "clientHeight", { configurable: true, value: client });
    el.scrollTop = top;
  }

  it("reports leaving the bottom on a user upward scroll past 48px", () => {
    const onLeaveBottom = vi.fn();
    const onReachBottom = vi.fn();
    const { container } = render(
      <ConversationFeed
        entries={[user("hi")]}
        onLeaveBottom={onLeaveBottom}
        onReachBottom={onReachBottom}
      />,
    );
    const conv = container.querySelector(".aj-conv") as HTMLElement;
    setScroll(conv, { top: 600, height: 1000, client: 400 });
    fireEvent.scroll(conv); // arriving at the bottom (downward) reports reach-bottom
    onReachBottom.mockClear(); // isolate the upward-scroll behavior below
    fireEvent.wheel(conv, { deltaY: -120 });
    setScroll(conv, { top: 100, height: 1000, client: 400 });
    fireEvent.scroll(conv);
    expect(onLeaveBottom).toHaveBeenCalledTimes(1);
    expect(onReachBottom).not.toHaveBeenCalled();
  });

  it("reports reaching the bottom on a user downward scroll inside 40px", () => {
    const onReachBottom = vi.fn();
    const { container } = render(
      <ConversationFeed entries={[user("hi")]} onReachBottom={onReachBottom} />,
    );
    const conv = container.querySelector(".aj-conv") as HTMLElement;
    setScroll(conv, { top: 100, height: 1000, client: 400 });
    fireEvent.scroll(conv);
    fireEvent.wheel(conv, { deltaY: 120 });
    setScroll(conv, { top: 580, height: 1000, client: 400 });
    fireEvent.scroll(conv);
    expect(onReachBottom).toHaveBeenCalledTimes(1);
  });

  it("ignores the programmatic auto-follow scroll", () => {
    const onLeaveBottom = vi.fn();
    const scrollIntoView = vi.fn();
    (Element.prototype as unknown as { scrollIntoView?: unknown }).scrollIntoView = scrollIntoView;
    try {
      const { container, rerender } = render(
        <ConversationFeed entries={[user("hi")]} onLeaveBottom={onLeaveBottom} />,
      );
      const conv = container.querySelector(".aj-conv") as HTMLElement;
      setScroll(conv, { top: 100, height: 1000, client: 400 });
      rerender(
        <ConversationFeed entries={[user("hi"), user("go")]} onLeaveBottom={onLeaveBottom} />,
      );
      expect(scrollIntoView).toHaveBeenCalled();
      fireEvent.scroll(conv); // consumed as the programmatic follow
      fireEvent.wheel(conv, { deltaY: -120 });
      setScroll(conv, { top: 50, height: 1000, client: 400 });
      fireEvent.scroll(conv);
      expect(onLeaveBottom).toHaveBeenCalledTimes(1);
    } finally {
      delete (Element.prototype as unknown as { scrollIntoView?: unknown }).scrollIntoView;
    }
  });

  it("classifies a fresh wheel gesture as intent when the scroll delta is zero", () => {
    const onLeaveBottom = vi.fn();
    const { container } = render(
      <ConversationFeed entries={[user("hi")]} onLeaveBottom={onLeaveBottom} />,
    );
    const conv = container.querySelector(".aj-conv") as HTMLElement;
    setScroll(conv, { top: 100, height: 1000, client: 400 });
    fireEvent.scroll(conv); // baseline (distance 500 -> no event)
    expect(onLeaveBottom).not.toHaveBeenCalled();
    fireEvent.wheel(conv, { deltaY: -120 });
    fireEvent.scroll(conv); // zero delta -> the fresh gesture decides
    expect(onLeaveBottom).toHaveBeenCalledTimes(1);
  });

  it("pauses scroll-derived decisions for 260ms after a collapse toggle", () => {
    vi.useFakeTimers();
    try {
      const onLeaveBottom = vi.fn();
      const { container, rerender } = render(
        <ConversationFeed entries={[user("hi")]} collapsed={false} onLeaveBottom={onLeaveBottom} />,
      );
      const conv = container.querySelector(".aj-conv") as HTMLElement;
      rerender(
        <ConversationFeed entries={[user("hi")]} collapsed={true} onLeaveBottom={onLeaveBottom} />,
      );
      setScroll(conv, { top: 600, height: 1000, client: 400 });
      fireEvent.scroll(conv);
      fireEvent.wheel(conv, { deltaY: -120 });
      setScroll(conv, { top: 100, height: 1000, client: 400 });
      fireEvent.scroll(conv);
      expect(onLeaveBottom).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(300);
      });
      fireEvent.wheel(conv, { deltaY: -120 });
      setScroll(conv, { top: 80, height: 1000, client: 400 });
      fireEvent.scroll(conv);
      expect(onLeaveBottom).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

      it("does not swallow a real upward scroll while the programmatic mark is pending", () => {
        const onLeaveBottom = vi.fn();
        const scrollIntoView = vi.fn();
        (Element.prototype as unknown as { scrollIntoView?: unknown }).scrollIntoView = scrollIntoView;
        try {
          const { container, rerender } = render(
            <ConversationFeed entries={[user("hi")]} onLeaveBottom={onLeaveBottom} />,
          );
          const conv = container.querySelector(".aj-conv") as HTMLElement;
          setScroll(conv, { top: 100, height: 1000, client: 400 });
          rerender(
            <ConversationFeed entries={[user("hi"), user("go")]} onLeaveBottom={onLeaveBottom} />,
          );
          expect(scrollIntoView).toHaveBeenCalled(); // programmatic mark pending
          fireEvent.wheel(conv, { deltaY: -120 });
          setScroll(conv, { top: 50, height: 1000, client: 400 });
          fireEvent.scroll(conv); // delta < 0 -> real user intent, must classify
          expect(onLeaveBottom).toHaveBeenCalledTimes(1);
        } finally {
          delete (Element.prototype as unknown as { scrollIntoView?: unknown }).scrollIntoView;
        }
      });
});

describe("RunPanel composer scroll collapse", () => {
  function setScroll(
    el: HTMLElement,
    { top, height, client }: { top: number; height: number; client: number },
  ) {
    Object.defineProperty(el, "scrollHeight", { configurable: true, value: height });
    Object.defineProperty(el, "clientHeight", { configurable: true, value: client });
    el.scrollTop = top;
  }
  function collapseViaScroll(container: HTMLElement) {
    const conv = container.querySelector(".aj-conv") as HTMLElement;
    setScroll(conv, { top: 600, height: 1000, client: 400 });
    fireEvent.scroll(conv);
    fireEvent.wheel(conv, { deltaY: -120 });
    setScroll(conv, { top: 100, height: 1000, client: 400 });
    fireEvent.scroll(conv);
  }
  function capturingClient() {
    let handler: ((method: string, params: unknown) => void) | null = null;
    const client = {
      setNotificationHandler: (h: (method: string, params: unknown) => void) => {
        handler = h;
      },
      removeNotificationHandler: () => {},
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
    return {
      client,
      emit: (method: string, params: unknown) =>
        act(() => {
          handler?.(method, params);
        }),
    };
  }

  it("collapses on scroll away and swaps the button into expand mode", () => {
    const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
    expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
    collapseViaScroll(container);
    const card = container.querySelector(".aj-composer-card") as HTMLElement;
    expect(card).toHaveAttribute("data-collapsed", "true");
    const button = screen.getByRole("button", { name: "展开输入框" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button.querySelector("svg")).not.toBeNull();
  });

  it("expands on click, keeps the draft and focuses the textarea", async () => {
    const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
    const textarea = container.querySelector("#aj-prompt") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "keep me" } });
    collapseViaScroll(container);
    fireEvent.click(screen.getByRole("button", { name: "展开输入框" }));
    await waitFor(() =>
      expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed"),
    );
    expect(textarea.value).toBe("keep me");
    await waitFor(() => expect(document.activeElement).toBe(textarea));
    expect(screen.getByRole("button", { name: "发送" })).toBeTruthy();
  });

  it("keeps the composer expanded while a question is pending", () => {
    const { client, emit } = capturingClient();
    const { container } = render(<RunPanel client={client} activeSessionId="s1" />);
    emit(METHODS.askQuestion, {
      question_id: "q1",
      session_id: "s1",
      question: "Continue?",
      options: [{ id: "yes", label: "Yes" }],
    });
    collapseViaScroll(container);
    expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
  });

  it("force-expands when a question arrives while collapsed", () => {
    const { client, emit } = capturingClient();
    const { container } = render(<RunPanel client={client} activeSessionId="s1" />);
    collapseViaScroll(container);
    expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
    emit(METHODS.askQuestion, {
      question_id: "q1",
      session_id: "s1",
      question: "Continue?",
      options: [{ id: "yes", label: "Yes" }],
    });
    expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
  });

  it("collapses in demo mode (demo ask visibility is not a guard)", () => {
    setDemoEnabled(true);
    try {
      const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
      collapseViaScroll(container);
      expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
    } finally {
      setDemoEnabled(false);
    }
  });

      it("freezes --aj-composer-h while collapsed and resumes after expand", () => {
        vi.useFakeTimers();
        const captured: { cb: ResizeObserverCallback | null } = { cb: null };
        class FakeResizeObserver {
          constructor(cb: ResizeObserverCallback) {
            captured.cb = cb;
          }
          observe = vi.fn();
          unobserve = () => {};
          disconnect = vi.fn();
        }
        vi.stubGlobal("ResizeObserver", FakeResizeObserver);
        try {
          const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
          const composer = container.querySelector(".aj-composer-card") as HTMLElement;
          const card = container.querySelector(".aj-card") as HTMLElement;
          const report = (size: number) =>
            act(() => {
              captured.cb?.(
                [{ target: composer, borderBoxSize: [{ blockSize: size, inlineSize: 100 }] } as unknown as ResizeObserverEntry],
                {} as ResizeObserver,
              );
            });
          report(180);
          expect(card.style.getPropertyValue("--aj-composer-h")).toBe("180px");
          collapseViaScroll(container);
          report(90);
          expect(card.style.getPropertyValue("--aj-composer-h")).toBe("180px");
          fireEvent.click(screen.getByRole("button", { name: "展开输入框" }));
          report(200);
          expect(card.style.getPropertyValue("--aj-composer-h")).toBe("180px"); // held while expanding
          act(() => {
            vi.advanceTimersByTime(300);
          });
          report(200);
          expect(card.style.getPropertyValue("--aj-composer-h")).toBe("200px");
        } finally {
          vi.unstubAllGlobals();
          vi.useRealTimers();
        }
      });

  it("closes composer popovers when collapsing", () => {
    const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
    const trigger = container.querySelector("#composer-model-trigger") as HTMLButtonElement;
    fireEvent.click(trigger);
    expect(container.querySelector(".pop")).not.toBeNull();
    collapseViaScroll(container);
    expect(container.querySelector(".pop")).toBeNull();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps the collapsed action button enabled without an active session", () => {
    const { container } = render(<RunPanel client={noopClient} activeSessionId={null} />);
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    collapseViaScroll(container);
    expect(screen.getByRole("button", { name: "展开输入框" })).not.toBeDisabled();
  });

  it("keeps the collapsed action button enabled while a run is busy", async () => {
    const busyClient = {
      request: () => new Promise(() => {}),
      setNotificationHandler: () => {},
      removeNotificationHandler: () => {},
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
    const { container } = render(<RunPanel client={busyClient} activeSessionId="s1" />);
    fireEvent.change(container.querySelector("#aj-prompt") as HTMLTextAreaElement, {
      target: { value: "go" },
    });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeDisabled());
    collapseViaScroll(container);
    expect(screen.getByRole("button", { name: "展开输入框" })).not.toBeDisabled();
  });

  it("starts expanded after a session remount (key change)", () => {
    const { container, rerender } = render(
      <RunPanel key="s1" client={noopClient} activeSessionId="s1" />,
    );
    collapseViaScroll(container);
    expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
    rerender(<RunPanel key="s2" client={noopClient} activeSessionId="s2" />);
    expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
  });

      it("expands when the user parks at the bottom inside the animation lock", () => {
        vi.useFakeTimers();
        try {
          const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
          const conv = container.querySelector(".aj-conv") as HTMLElement;
          collapseViaScroll(container);
          expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
          setScroll(conv, { top: 600, height: 1000, client: 400 });
          fireEvent.scroll(conv); // locked out: pinned updates, no reach-bottom
          expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
          act(() => {
            vi.advanceTimersByTime(300);
          });
          expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
        } finally {
          vi.useRealTimers();
        }
      });

      it("re-collapses when the user scrolls up again after a click-expand", () => {
        vi.useFakeTimers();
        try {
          const { container } = render(<RunPanel client={noopClient} activeSessionId="s1" />);
          collapseViaScroll(container);
          fireEvent.click(screen.getByRole("button", { name: "展开输入框" }));
          expect(container.querySelector(".aj-composer-card")).not.toHaveAttribute("data-collapsed");
          act(() => {
            vi.advanceTimersByTime(300);
          });
          const conv = container.querySelector(".aj-conv") as HTMLElement;
          fireEvent.wheel(conv, { deltaY: -120 });
          setScroll(conv, { top: 50, height: 1000, client: 400 });
          fireEvent.scroll(conv);
          expect(container.querySelector(".aj-composer-card")).toHaveAttribute("data-collapsed", "true");
        } finally {
          vi.useRealTimers();
        }
      });
});

describe("RunPanel quota hint", () => {
  it("hints at the subscription panel on a GoUsageLimitError", async () => {
    render(<RunPanel {...propsWithError("API error (429): GoUsageLimitError: limit reached")} />);
    expect(await screen.findByText(/设置 · 订阅/)).toBeInTheDocument();
  });

  it("does not hint on a plain rate-limit error", async () => {
    render(<RunPanel {...propsWithError("API error (429): RateLimitError: slow down")} />);
    expect(screen.queryByText(/设置 · 订阅/)).not.toBeInTheDocument();
  });

  it("shows the hint at most once per session", async () => {
    const { rerender } = render(<RunPanel {...propsWithError("GoUsageLimitError: a")} />);
    expect(await screen.findByText(/设置 · 订阅/)).toBeInTheDocument();
    rerender(<RunPanel {...propsWithError("GoUsageLimitError: b")} />);
    expect(screen.getAllByText(/设置 · 订阅/)).toHaveLength(1);
  });
});

// BUG-004: a run keeps going after a client disconnect (spec 8.11 异常断开), so a
// reconnected client can legitimately find the session busy. The user must get an
// explanation, not the raw `-32805: SessionBusy` code.
describe("RunPanel session busy hint", () => {
  function failingClient(message: string) {
    return {
      setNotificationHandler: () => {},
      removeNotificationHandler: () => {},
      request: async (method: string) => {
        if (method === METHODS.sessionRun) throw new Error(message);
        return null;
      },
    } as unknown as Parameters<typeof RunPanel>[0]["client"];
  }

  async function submit(client: Parameters<typeof RunPanel>[0]["client"]) {
    render(<RunPanel client={client} activeSessionId="s1" />);
    fireEvent.change(screen.getByLabelText(/prompt/i), { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
  }

  it("explains -32805 SessionBusy instead of showing the raw code", async () => {
    await submit(failingClient("-32805: SessionBusy"));
    expect(await screen.findByText(/该会话已有任务在运行/)).toBeInTheDocument();
    expect(screen.queryByText(/^-32805/)).not.toBeInTheDocument();
  });

  it("passes other failures through unchanged", async () => {
    await submit(failingClient("API error (500): upstream broke"));
    expect(await screen.findByText(/API error \(500\): upstream broke/)).toBeInTheDocument();
  });
});
