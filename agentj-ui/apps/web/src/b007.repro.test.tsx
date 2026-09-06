import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

const shared = vi.hoisted(() => ({
  initVersion: { value: "0.12.0" },
  shouldFail: { value: false },
}));

vi.mock("@agentj/sdk", () => {
  class ResponseRouter {
    setNotificationHandler() {}
  }
  class WebSocketTransport {
    onStatusChange?: (s: string) => void;
    constructor(_url: string, _router: unknown) {
      queueMicrotask(() => this.onStatusChange?.("open"));
    }
    close() {}
  }
  class RpcClient {
    constructor(_t: unknown, _r: unknown) {}
    setNotificationHandler() {}
    async request(method: string) {
      if (shared.shouldFail.value) throw new Error("unreachable");
      if (method === "kernel/status") return { modules: [] };
      if (method === "initialize") return { protocol_version: shared.initVersion.value };
      return null;
    }
    async initialize() {
      if (shared.shouldFail.value) throw new Error("unreachable");
      return { protocol_version: shared.initVersion.value };
    }
  }
  return {
    RpcClient,
    ResponseRouter,
    WebSocketTransport,
    resolveWsUrl: vi.fn((o?: { envUrl?: string; host?: string; port?: number; token?: string }) => {
      if (o?.envUrl) return o.envUrl;
      const host = o?.host ?? "127.0.0.1";
      const port = o?.port ?? 4096;
      const base = `ws://${host}:${port}/ws`;
      return o?.token ? `${base}?token=${o.token}` : base;
    }),
    METHODS: {
      kernelStatus: "kernel/status",
      initialize: "initialize",
      sessionNew: "session/new",
      sessionStatus: "session/status",
      sessionSubscribe: "session/subscribe",
      sessionRun: "session/run",
      progress: "$/progress",
      permissionRequest: "$/permissionRequest",
      permissionRespond: "permission/respond",
    },
    PROTOCOL_VERSION: "0.12.0",
  };
});

describe("B-007 repro: hint port reflects VITE_AGENTJ_WS_URL", () => {
  beforeEach(() => {
    shared.shouldFail.value = false;
    shared.initVersion.value = "0.12.0";
    localStorage.clear();
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it("should show 9999 when VITE_AGENTJ_WS_URL=ws://host:9999/ws", async () => {
    vi.stubEnv("VITE_AGENTJ_WS_URL", "ws://host:9999/ws");
    // dynamic import needed to re-evaluate App.tsx after stubEnv — static import cannot work
    const { default: App } = await import("./App.tsx");
    shared.shouldFail.value = true;
    render(<App />);
    const hint = await screen.findByText(/agentj dashboard --port/i);
    // This assertion should fail before fix (shows 4096) and pass after fix (shows 9999)
    expect(hint.textContent).toContain("9999");
    // Ensure not 4096 when env is 9999
    expect(hint.textContent).not.toContain("4096");
  });

  // fallback to 4096 when envUrl has no port is covered by `|| new URL(resolveWsUrl({})).port` in App.tsx
});
