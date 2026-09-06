import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, act } from "@testing-library/react";

const shared = vi.hoisted(() => ({
  lastWsUrl: { value: "" },
  initVersion: { value: "0.12.0" },
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
      if (method === "kernel/status") {
        return { modules: [{ id: "test-mod", state: "running" }] };
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
      progress: "$/progress",
      permissionRequest: "$/permissionRequest",
      permissionRespond: "permission/respond",
    },
    PROTOCOL_VERSION: "0.12.0",
  };
});

import App from "./App";

describe("App with mocked SDK transport", () => {
  beforeEach(() => {
    shared.lastWsUrl.value = "";
    shared.initVersion.value = "0.12.0";
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
    localStorage.clear();
  });

  it("renders modules from kernel/status via SDK client", async () => {
    render(<App />);
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

  it("applies token to wsUrl via connection bar", async () => {
    render(<App />);
    const tokenInput = screen.getByLabelText(/token/i);
    fireEvent.change(tokenInput, { target: { value: "abc" } });
    fireEvent.click(screen.getByRole("button", { name: /connect/i }));
    await waitFor(() =>
      expect(shared.lastWsUrl.value).toContain("?token=abc"),
    );
  });

  it("shows version mismatch banner expected 0.12.0 got 0.9.0", async () => {
    shared.initVersion.value = "0.9.0";
    render(<App />);
    expect(
      await screen.findByText(/expected 0\.12\.0, got 0\.9\.0/i),
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
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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

  it("AllowAll sends permission/respond with allow_always true and closes", async () => {
    shared.newSessionId.value = "s1";
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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
      decision: "allow",
      allow_always: true,
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("rejected respond shows inline error and keeps dialog open", async () => {
    shared.newSessionId.value = "s1";
    shared.permShouldFail.value = true;
    shared.permError.value = "stale request";
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /new session/i }));
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
