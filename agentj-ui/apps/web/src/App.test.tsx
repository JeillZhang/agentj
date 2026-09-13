import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, act, within } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const shared = vi.hoisted(() => ({
  lastWsUrl: { value: "" },
  initVersion: { value: "0.12.1" },
  shouldFail: { value: false },
  newSessionId: { value: "s9" },
  statusShouldFail: { value: false },
  statusError: { value: "archived" },
  subscribeShouldFail: { value: false },
  requests: { value: [] as string[] },
  notificationHandler: { value: null as ((m: string, p: unknown) => void) | null },
  lastRunParams: { value: null as Record<string, unknown> | null },
  lastPermissionParams: { value: null as Record<string, unknown> | null },
  permShouldFail: { value: false },
  permError: { value: "stale" },
  sessionListResult: { value: { sessions: [] as Array<Record<string, unknown>> } },
  historyResult: { value: { events: [] as Array<Record<string, unknown>>, next_cursor: null as number | null } },
  lastHistoryParams: { value: null as Record<string, unknown> | null },
}));

vi.mock("@agentj/sdk", () => {
  class ResponseRouter {
    setNotificationHandler(h: (m: string, p: unknown) => void) {
      shared.notificationHandler.value = h;
    }
  }
  class WebSocketTransport {
    onStatusChange?: (s: string) => void;
    constructor(url: string, _router: unknown) {
      shared.lastWsUrl.value = url;
      queueMicrotask(() => this.onStatusChange?.("open"));
    }
    close() {}
  }
  class RpcClient {
    constructor(_t: unknown, _r: unknown) {}
    setNotificationHandler(h: (m: string, p: unknown) => void) {
      shared.notificationHandler.value = h;
    }
    async request(method: string, params?: Record<string, unknown>) {
      shared.requests.value.push(method);
      if (method === "session/new") {
        if (shared.shouldFail.value) throw new Error("unreachable");
        return { session_id: shared.newSessionId.value };
      }
      if (method === "session/status") {
        if (shared.statusShouldFail.value) throw new Error(shared.statusError.value);
        if (shared.shouldFail.value) throw new Error("unreachable");
        void params;
        return { status: "active" };
      }
      if (method === "session/subscribe") {
        if (shared.subscribeShouldFail.value) throw new Error("subscribe failed");
        return null;
      }
      if (method === "session/run") {
        shared.lastRunParams.value = params ?? null;
        if (shared.shouldFail.value) throw new Error("unreachable");
        await new Promise((r) => setTimeout(r, 20));
        return { content: "done", usage: { prompt_tokens: 10, completion_tokens: 5 } };
      }
      if (method === "permission/respond") {
        shared.lastPermissionParams.value = params ?? null;
        if (shared.permShouldFail.value) throw new Error(shared.permError.value);
        return {};
      }
      if (shared.shouldFail.value) throw new Error("unreachable");
      if (method === "session/list") return shared.sessionListResult.value;
      if (method === "session/history") {
        shared.lastHistoryParams.value = params as Record<string, unknown> | null;
        return shared.historyResult.value;
      }
      if (method === "kernel/status") {
        return { modules: [{ id: "test-mod", state: "running" }], default_model: "deepseek-chat" };
      }
      if (method === "initialize") {
        return { protocol_version: shared.initVersion.value };
      }
      return null;
    }
    async initialize(_v?: string) {
      if (shared.shouldFail.value) throw new Error("unreachable");
      return { protocol_version: shared.initVersion.value };
    }
  }
  return {
    RpcClient,
    ResponseRouter,
    WebSocketTransport,
    resolveWsUrl: vi.fn(
      (o?: {
        envUrl?: string;
        host?: string;
        port?: number;
        token?: string;
      }) => {
        if (o?.envUrl) return o.envUrl;
        const host = o?.host ?? "127.0.0.1";
        const port = o?.port ?? 4096;
        const base = `ws://${host}:${port}/ws`;
        return o?.token ? `${base}?token=${o.token}` : base;
      },
    ),
    // T14: real parser coverage lives in @agentj/sdk progress.test.ts (T12); this
    // double mirrors T12's translation table for the standard frame kinds so the
    // web flows exercise the same shapes. DRIFT WARNING: keep in step with T12.
    parseProgress: (params: unknown) => {
      if (typeof (params as { text?: unknown })?.text === "string") {
        return { kind: "text", text: (params as { text: string }).text };
      }
      const v = (params as { value?: unknown })?.value as
        | Record<string, unknown>
        | undefined;
      if (!v || typeof v["kind"] !== "string") return { kind: "unknown", raw: params };
      const kind = v["kind"] as string;
      if (kind === "text") {
        return typeof v["delta"] === "string"
          ? { kind: "text", text: v["delta"] }
          : { kind: "unknown", raw: params };
      }
      if (kind === "tool_call") {
        const tc = v["tool_call"] as Record<string, unknown> | undefined;
        return tc && typeof tc["id"] === "string" && typeof tc["name"] === "string"
          ? { kind: "tool_call", id: tc["id"], name: tc["name"], arguments: tc["arguments"] ?? null }
          : { kind: "unknown", raw: params };
      }
      if (kind === "tool_result") {
        return typeof v["content"] === "string" && typeof v["tool_call_id"] === "string"
          ? { kind: "tool_result", call_id: v["tool_call_id"], content: v["content"] }
          : { kind: "unknown", raw: params };
      }
      if (kind === "usage") {
        const u = v["usage"] as { prompt_tokens?: unknown; completion_tokens?: unknown } | undefined;
        return u && typeof u.prompt_tokens === "number" && typeof u.completion_tokens === "number"
          ? { kind: "usage", usage: { prompt_tokens: u.prompt_tokens, completion_tokens: u.completion_tokens } }
          : { kind: "unknown", raw: params };
      }
      if (kind === "permission") {
        return typeof v["request_id"] === "string" && typeof v["tool"] === "string"
          && typeof v["decision"] === "string"
          ? { kind: "permission", request_id: v["request_id"] as string, tool: v["tool"] as string,
              decision: v["decision"] as string }
          : { kind: "unknown", raw: params };
      }
      return { kind: "unknown", raw: params };
    },
    METHODS: {
      kernelStatus: "kernel/status",
      initialize: "initialize",
      sessionNew: "session/new",
      sessionStatus: "session/status",
      sessionSubscribe: "session/subscribe",
      sessionRun: "session/run",
      sessionList: "session/list",
      sessionHistory: "session/history",
      progress: "$/progress",
      permissionRequest: "$/permissionRequest",
      permissionRespond: "permission/respond",
    },
    PROTOCOL_VERSION: "0.12.1",
  };
});

import App, { connectionLight } from "./App";
import { historyToEntries, historyToConversation, filterNodes } from "./Trajectory";
import { Trajectory } from "./Trajectory";

const WIRE_CORPUS = [
  { session_id: "s1", seq: 1, ts: 100, value: { kind: "user", content: "hi" } },
  { session_id: "s1", seq: 2, ts: 101, value: { kind: "thinking", content: "hello world" } },
  { session_id: "s1", seq: 3, ts: 102, value: { kind: "tool_call", tool_call: { id: "c1", name: "bash", arguments: { cmd: "ls" } } } },
  { session_id: "s1", seq: 4, ts: 103, value: { kind: "tool_result", tool_call_id: "c1", content: "auth.rs\nmain.rs" } },
  { session_id: "s1", seq: 5, ts: 104, value: { kind: "permission", request_id: "r1", tool: "bash", decision: "allow" } },
  { session_id: "s1", seq: 6, ts: 105, value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } } },
  { session_id: "s1", seq: 7, ts: 106, value: { kind: "error", message: "boom" } },
] as Array<Record<string, unknown>>;

describe("App with mocked SDK transport", () => {
  beforeEach(() => {
    shared.lastWsUrl.value = "";
    shared.initVersion.value = "0.12.1";
    shared.shouldFail.value = false;
    shared.newSessionId.value = "s9";
    shared.statusShouldFail.value = false;
    shared.statusError.value = "archived";
    shared.subscribeShouldFail.value = false;
    shared.requests.value = [];
    shared.notificationHandler.value = null;
    shared.lastRunParams.value = null;
    shared.lastPermissionParams.value = null;
    shared.permShouldFail.value = false;
    shared.permError.value = "stale";
    shared.sessionListResult.value = { sessions: [] as Array<Record<string, unknown>> };
    shared.historyResult.value = { events: [] as Array<Record<string, unknown>>, next_cursor: null as number | null };
    shared.lastHistoryParams.value = null;
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  it("renders modules from kernel/status via SDK client", async () => {
    render(<App />);
    fireEvent.click(screen.getByLabelText(/more/i));
    await waitFor(() =>
      expect(screen.getByText("test-mod: running")).toBeInTheDocument(),
    );
  });

  it("shows connected status after successful kernel/status", async () => {
    render(<App />);
    await waitFor(() =>
      expect(screen.getByText("connected")).toBeInTheDocument(),
    );
  });

  it("composer status popover reflects real modules and connection state from kernel/status", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() => expect(screen.getByText(/active: s1/i)).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText("connected")).toBeInTheDocument());
    const composerStatus = screen.getByRole("button", { name: /^status$/i });
    fireEvent.click(composerStatus);
    const dialog = screen.getByRole("dialog", { name: "status" });
    expect(dialog).toBeInTheDocument();
    expect(await within(dialog).findByText("test-mod: running")).toBeInTheDocument();
    expect(within(dialog).queryByText("no modules")).not.toBeInTheDocument();
    expect(dialog.querySelector(".aj-status-light")?.getAttribute("data-state")).toBe("connected");
  });

  it("applies token to wsUrl via connection bar", async () => {
    render(<App />);
    const hostInput = screen.getByLabelText(/^host$/i);
    fireEvent.change(hostInput, { target: { value: "127.0.0.1" } });
    const portInput = screen.getByLabelText(/^port$/i);
    fireEvent.change(portInput, { target: { value: "4096" } });
    const tokenInput = screen.getByLabelText(/token/i);
    fireEvent.change(tokenInput, { target: { value: "abc" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    await waitFor(() =>
      expect(shared.lastWsUrl.value).toContain("?token=abc"),
    );
  });

  it("shows version mismatch banner expected 0.12.1 got 0.9.0", async () => {
    shared.initVersion.value = "0.9.0";
    render(<App />);
    expect(
      await screen.findByText(/expected 0\.12\.1, got 0\.9\.0/i),
    ).toBeInTheDocument();
  });

  it("shows unreachable hint with dashboard start command", async () => {
    shared.shouldFail.value = true;
    render(<App />);
    expect(
      await screen.findByText(/agentj dashboard --port/i),
    ).toBeInTheDocument();
  });

  it("new session becomes active and favorited", async () => {
    shared.newSessionId.value = "s9";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    expect(await screen.findByText("s9")).toBeInTheDocument();
    expect(await screen.findByText(/active: s9/i)).toBeInTheDocument();
    expect(
      JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]"),
    ).toContain("s9");
  });

  it("pasted id continues via status+subscribe", async () => {
    render(<App />);
    const input = screen.getByLabelText(/session id/i);
    fireEvent.change(input, { target: { value: "abc123" } });
    fireEvent.click(screen.getByRole("button", { name: /^continue$/i }));
    await waitFor(() =>
      expect(screen.getByText(/active: abc123/i)).toBeInTheDocument(),
    );
    expect(shared.requests.value).toContain("session/status");
    expect(shared.requests.value).toContain("session/subscribe");
    expect(
      JSON.parse(localStorage.getItem("agentj.favorites") ?? "[]"),
    ).toContain("abc123");
  });

  it("stale favorite shows error with New button and marks stale", async () => {
    localStorage.setItem("agentj.favorites", JSON.stringify(["archived-id"]));
    shared.statusShouldFail.value = true;
    shared.statusError.value = "archived";
    render(<App />);
    const input = screen.getByLabelText(/session id/i);
    fireEvent.change(input, { target: { value: "archived-id" } });
    fireEvent.click(screen.getByRole("button", { name: /^continue$/i }));
    expect(await screen.findByText(/archived/i)).toBeInTheDocument();
    const alert = screen.getByRole("alert");
    expect(alert).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^new$/i }),
    ).toBeInTheDocument();
    await waitFor(() => {
      const staleRaw = localStorage.getItem("agentj.favorites.stale");
      const stale = staleRaw ? JSON.parse(staleRaw) : [];
      const rendered = screen.getByText(/archived-id.*stale/i);
      expect(stale.includes("archived-id") || rendered).toBeTruthy();
    });
  });

  it("composer submits and streams progress rows", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() =>
      expect(screen.getByText(/active: s1/i)).toBeInTheDocument(),
    );
    const prompt = screen.getByLabelText(/prompt/i);
    fireEvent.change(prompt, { target: { value: "hi" } });
    const send = screen.getByRole("button", { name: /send/i });
    fireEvent.click(send);
    await waitFor(() => expect(prompt).toHaveValue(""));
    await waitFor(() => expect(shared.notificationHandler.value).not.toBeNull());
    const handler = shared.notificationHandler.value;
    expect(handler).not.toBeNull();
    act(() => {
      handler!("$/progress", { token: "t", value: { kind: "text", delta: "hello" } });
    });
    expect(await screen.findByText("hello")).toBeInTheDocument();
    const params = shared.lastRunParams.value;
    expect(params).not.toBeNull();
    expect(params?.session_id).toBe("s1");
    expect(params?.content).toBe("hi");
    expect(shared.requests.value).toContain("session/run");
    expect(await screen.findByText(/in=10 out=5/i)).toBeInTheDocument();
    await waitFor(() => expect(send).not.toBeDisabled());
  });

  it("approval dialog shows modal with tool+args+session_id and Allow/Deny/AllowAll", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() => expect(screen.getByText(/active: s1/i)).toBeInTheDocument());
    await waitFor(() => expect(shared.notificationHandler.value).not.toBeNull());
    const handler = shared.notificationHandler.value!;
    act(() => {
      handler("$/permissionRequest", {
        request_id: "r1",
        tool: "bash",
        args: { cmd: "ls" },
        session_id: "s1",
        reason: "ask",
      });
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(/bash/)).toBeInTheDocument();
    expect(screen.getByText(/ls/)).toBeInTheDocument();
    expect(screen.getByText(/session: s1/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^allow$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^deny$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /allow all/i })).toBeInTheDocument();
  });

  it("filters permission request not for active session", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() => expect(screen.getByText(/active: s1/i)).toBeInTheDocument());
    await waitFor(() => expect(shared.notificationHandler.value).not.toBeNull());
    const handler = shared.notificationHandler.value!;
    act(() => {
      handler("$/permissionRequest", {
        request_id: "r2",
        tool: "bash",
        args: { cmd: "rm" },
        session_id: "s-other",
        reason: "ask",
      });
    });
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("AllowAll sends permission/respond with decision allow_always and closes", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() => expect(screen.getByText(/active: s1/i)).toBeInTheDocument());
    await waitFor(() => expect(shared.notificationHandler.value).not.toBeNull());
    const handler = shared.notificationHandler.value!;
    act(() => {
      handler("$/permissionRequest", {
        request_id: "r1",
        tool: "bash",
        args: { cmd: "ls -la" },
        session_id: "s1",
        reason: "ask",
      });
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /allow all/i }));
    await waitFor(() => expect(shared.requests.value).toContain("permission/respond"));
    expect(shared.lastPermissionParams.value).toEqual({
      request_id: "r1",
      decision: "allow_always",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("rejected respond shows inline error and keeps dialog open", async () => {
    shared.newSessionId.value = "s1";
    shared.permShouldFail.value = true;
    shared.permError.value = "stale request";
    render(<App />);
    fireEvent.click(screen.getAllByRole("button", { name: /new session/i })[0]);
    await waitFor(() => expect(screen.getByText(/active: s1/i)).toBeInTheDocument());
    await new Promise((r) => setTimeout(r, 10));
    await waitFor(() => expect(shared.notificationHandler.value).not.toBeNull());
    const handler = shared.notificationHandler.value!;
    act(() => {
      handler("$/permissionRequest", {
        request_id: "r9",
        tool: "bash",
        args: { cmd: "evil" },
        session_id: "s1",
        reason: "ask",
      });
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByLabelText(/progress feed/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^allow$/i }));
    expect(await screen.findByText(/stale request/i)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /close/i })).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByLabelText(/progress feed/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /close/i }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });
});

describe("Trajectory mappers", () => {
  it("historyToEntries collapses the tool pair and keeps wire seqs", () => {
    const nodes = historyToEntries(WIRE_CORPUS as never);
    expect(nodes.map((n) => n.kind)).toEqual(["user", "thinking", "tool", "approval", "usage", "error"]);
    const toolNode = nodes[2] as { seq: number; state: string; output?: string };
    expect(toolNode.seq).toBe(3);
    expect(toolNode.state).toBe("ok");
    expect(toolNode.output).toContain("auth.rs");
  });
  it("historyToConversation skips allow-approval and error rows", () => {
    const entries = historyToConversation(WIRE_CORPUS as never);
    expect(entries.map((e) => e.kind)).toEqual(["user", "ai", "tool", "usage"]);
    expect(entries[1]).toEqual({ kind: "ai", text: "hello world" });
    expect(screen.queryByText("boom")).not.toBeInTheDocument();
  });
  it("a deny permission row marks the newest running tool denied", () => {
    const corpus = WIRE_CORPUS.map((e) => (e.seq === 5 ? { ...e, value: { ...(e.value as Record<string, unknown>), decision: "deny" } } : e));
    const entries = historyToConversation(corpus as never);
    const toolEntry = entries[2] as { state: string };
    expect(toolEntry.state).toBe("denied");
  });
  it("filterNodes filters by text and by kind chips", () => {
    const nodes = historyToEntries(WIRE_CORPUS as never);
    expect(filterNodes(nodes, "auth", []).map((n) => n.kind)).toEqual(["tool"]);
    expect(filterNodes(nodes, "", ["error"]).map((n) => n.kind)).toEqual(["error"]);
  });
});

describe("Trajectory replay", () => {
  it("highlights nodes in seq order and pauses at the end", () => {
    vi.useFakeTimers();
    try {
      const nodes = historyToEntries(WIRE_CORPUS as never);
      void nodes;
      render(<Trajectory events={WIRE_CORPUS as never} />);
      fireEvent.click(screen.getByRole("button", { name: /play/i }));
      expect(screen.getByLabelText(/step 1 of 6/i)).toBeInTheDocument();
      act(() => { vi.advanceTimersByTime(700); });
      expect(screen.getByLabelText(/step 2 of 6/i)).toBeInTheDocument();
      act(() => { vi.advanceTimersByTime(700 * 6); });
      expect(screen.getByLabelText(/step 6 of 6/i)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /play/i })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("App shell + history rebuild", () => {
  beforeEach(() => {
    shared.initVersion.value = "0.12.1";
    shared.shouldFail.value = false;
    shared.historyResult.value = { events: [] as Array<Record<string, unknown>>, next_cursor: null as number | null };
    shared.sessionListResult.value = { sessions: [] as Array<Record<string, unknown>> };
    shared.lastHistoryParams.value = null;
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });
  it("rebuilds the conversation from session/history when continuing s1", async () => {
    shared.historyResult.value = { events: WIRE_CORPUS, next_cursor: null };
    shared.sessionListResult.value = { sessions: [{ session_id: "s1", state: "active", preview: "hi" }] };
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    const list = await screen.findByRole("list", { name: "session list" });
    fireEvent.click(await within(list).findByText(/s1/));
    expect(await screen.findByText("hello world")).toBeInTheDocument();
    expect(screen.getByText("in=10 out=5")).toBeInTheDocument();
    expect(screen.queryByText("boom")).not.toBeInTheDocument();
  });
  it("shows error nodes in the Trajectory tab only", async () => {
    shared.historyResult.value = { events: WIRE_CORPUS, next_cursor: null };
    shared.sessionListResult.value = { sessions: [{ session_id: "s1", state: "active", preview: "hi" }] };
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    const list = await screen.findByRole("list", { name: "session list" });
    fireEvent.click(await within(list).findByText(/s1/));
    await screen.findByText("hello world");
    fireEvent.click(screen.getByRole("tab", { name: /trajectory/i }));
    expect(await screen.findByText("boom")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /play/i })).toBeInTheDocument();
  });
  it("theme toggle cycles dark → light → system and persists agentj.theme-mode", () => {
    localStorage.removeItem("agentj.theme-mode");
    document.documentElement.removeAttribute("data-theme");
    render(<App />);
    expect(document.documentElement.dataset.theme).toBeUndefined();
    fireEvent.click(screen.getByRole("button", { name: /toggle theme/i }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("agentj.theme-mode")).toBe("dark");
    fireEvent.click(screen.getByRole("button", { name: /toggle theme/i }));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("agentj.theme-mode")).toBe("light");
    fireEvent.click(screen.getByRole("button", { name: /toggle theme/i }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
    expect(localStorage.getItem("agentj.theme-mode")).toBe("system");
  });
  it("shows the kernel/status model chip and feeds the meta row 模型 segment", async () => {
    shared.historyResult.value = { events: WIRE_CORPUS, next_cursor: null };
    shared.sessionListResult.value = { sessions: [{ session_id: "s1", state: "active", preview: "hi" }] };
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    const header = document.querySelector(".aj-topbar") as HTMLElement;
    expect(await within(header).findByText("deepseek-chat")).toBeInTheDocument();
    expect(within(header).getByLabelText("model")).toBeInTheDocument();
    const list = await screen.findByRole("list", { name: "session list" });
    fireEvent.click(await within(list).findByText(/s1/));
    await screen.findByText("hello world");
    expect(screen.getAllByText("deepseek-chat").length).toBeGreaterThanOrEqual(2);
  });
  it("maps the connection state onto the status light (绿/橙 semantics)", async () => {
    expect(connectionLight("connected", false)).toBe("connected");
    expect(connectionLight("kernel unreachable on ws://x", true)).toBe("unreachable");
    expect(connectionLight("connecting to kernel on ws://x…", false)).toBe("connecting");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    await screen.findByText("connected");
    const light = () => document.querySelector<HTMLElement>(".aj-status-light")!;
    expect(light()).not.toBeNull();
    expect(light().dataset.state).toBe("connected");
  });
  it("lights unreachable orange while the hint text stays untouched", async () => {
    shared.shouldFail.value = true;
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    await screen.findByText(/agentj dashboard --port/i);
    expect(document.querySelector<HTMLElement>(".aj-status-light")?.dataset.state).toBe("unreachable");
  });
  it("renders the version-mismatch banner with its semantic state", async () => {
    shared.initVersion.value = "0.9.0";
    render(<App />);
    const banner = await waitFor(() => screen.getByRole("alert"), { timeout: 3000 });
    expect(banner.textContent).toMatch(/expected 0\.12\.1, got 0\.9\.0/i);
    expect(banner.className).toContain("aj-banner");
    expect(banner.getAttribute("data-state")).toBe("mismatch");
  });
  it("drawer toggle controls the rail and App.css declares the responsive rules", async () => {
    const css = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "App.css"), "utf8",
    );
    expect(css).toContain(".aj-drawer-toggle");
    expect(css).toContain("@media (max-width: 899px)");
    expect(css).toContain("@media (max-width: 479px)");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /connect/i }));
    const toggle = screen.getByRole("button", { name: /toggle sessions/i });
    const app = () => document.querySelector<HTMLElement>(".aj-app")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(app().dataset.drawer).toBe("closed");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(app().dataset.drawer).toBe("open");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(app().dataset.drawer).toBe("closed");
  });
});

describe("T12 workspace shell + right panel", () => {
  beforeEach(() => {
    shared.initVersion.value = "0.12.1";
    shared.shouldFail.value = false;
    shared.historyResult.value = { events: [] as never[], next_cursor: null };
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });
  afterEach(() => localStorage.clear());

  it("shell + right panel render", () => {
    render(<App />);
    expect(document.querySelector(".aj-workspace")).not.toBeNull();
    expect(document.querySelector(".aj-sidebar")).not.toBeNull();
    expect(document.querySelector(".aj-shell__center")).not.toBeNull();
    expect(document.querySelector(".aj-right")).not.toBeNull();
    expect(screen.getByText("运行详情")).toBeInTheDocument();
    expect(screen.getByText("文件变更")).toBeInTheDocument();
    expect(screen.getAllByText("待协议支持").length).toBeGreaterThanOrEqual(1);
  });

  it("demo fixtures light up the right panel", () => {
    localStorage.setItem("agentj.demo", "1");
    render(<App />);
    expect(screen.getByText("任务")).toBeInTheDocument();
    expect(screen.getByText("2 / 4")).toBeInTheDocument();
    expect(screen.getByText("+86")).toBeInTheDocument();
  });
});

describe("T2 App Shell grid + popover + scrim", () => {
  beforeEach(() => {
    shared.initVersion.value = "0.12.1";
    shared.shouldFail.value = false;
    shared.historyResult.value = { events: [] as never[], next_cursor: null };
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });
  it("shell uses grid and centered main with popover", () => {
    render(<App />);
    const topbar = document.querySelector(".aj-topbar");
    expect(topbar).not.toBeNull();
    expect(topbar).toHaveClass("aj-topbar");
    expect(document.querySelector(".aj-main")).not.toBeNull();
    expect(screen.getByLabelText(/more/i)).toBeInTheDocument();
  });
  it("layout.css has grid and clamp", () => {
    const p = join(dirname(fileURLToPath(import.meta.url)), "components/layout.css");
    const css = readFileSync(p, "utf8");
    expect(css).toContain("display: grid");
    expect(css).toContain("backdrop-filter");
    expect(css).toContain("clamp");
    expect(css).toContain("--aj-content-max");
    expect(css).toContain("rgba(0,0,0,.24)");
    expect(css).toContain("280px");
    expect(css).toContain("var(--aj-space-4)");
  });
});
