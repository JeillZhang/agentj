import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { App, getBackoffDelay } from "./app.tsx";
import type { TransportHandle } from "./app.tsx";
import { METHODS, ResponseRouter } from "@agentj/sdk";
import type { TransportStatus } from "@agentj/sdk";
import { MODEL_CONFIG, providersFromStatus, renderModelList } from "./models.tsx";

function getInput(setup: TestRendererSetup) {
  const input = setup.mockInput as unknown as {
    press?: (k: string) => void;
    type?: (s: string) => Promise<void>;
  };
  if (!input.press) {
    input.press = (k: string) => {
      if (k === "down") setup.mockInput.pressArrow("down");
      else if (k === "up") setup.mockInput.pressArrow("up");
      else if (k === "enter" || k === "return") setup.mockInput.pressEnter();
      else if (k === "esc" || k === "escape") setup.mockInput.pressEscape();
      else setup.mockInput.pressKey(k);
    };
  }
  if (!input.type) {
    input.type = async (s: string) => {
      await setup.mockInput.typeText(s);
    };
  }
  return input as { press: (k: string) => void; type: (s: string) => Promise<void> };
}

function createUnreachable(): TransportHandle {
  const handle: TransportHandle = {
    close() {},
  };
  queueMicrotask(() => act(() => handle.onStatusChange?.("closed")));
  return handle;
}

function createWithAuthError(): TransportHandle {
  return {
    lastError: "403 Forbidden: NotAuthorized",
    close() {},
  };
}

function createStayConnecting(): TransportHandle {
  return {
    close() {},
  };
}
describe("App connection screen", () => {
  test("unreachable shows kernel unreachable", async () => {
    const setup = await testRender(<App createTransport={createUnreachable} backoffBaseMs={10} />, {});
    await act(async () => {
      await setup.waitFor(() => {
        const frame = setup.captureCharFrame();
        return frame.toLowerCase().includes("kernel unreachable");
      });
    });
    const frame = setup.captureCharFrame();
    expect(frame.toLowerCase()).toContain("kernel unreachable");
    act(() => setup.renderer.destroy());
  });

  test("403 stops reconnect with auth error", async () => {
    let calls = 0;
    const createAuth = (): TransportHandle => {
      calls += 1;
      return createWithAuthError();
    };
    const setup = await testRender(<App createTransport={createAuth} backoffBaseMs={10} />, {});
    await act(async () => {
      await setup.waitFor(() => {
        const frame = setup.captureCharFrame().toLowerCase();
        return frame.includes("auth error") || frame.includes("403");
      });
    });
    const frame = setup.captureCharFrame().toLowerCase();
    expect(frame).toContain("auth");
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      setTimeout(resolve, 50);
      await promise;
    }
    expect(calls).toBe(1);
    expect(frame).not.toContain("kernel unreachable");
    act(() => setup.renderer.destroy());
  });

  test("snapshot contains status line", async () => {
    const setup = await testRender(<App createTransport={createStayConnecting} backoffBaseMs={10000} />, {});
    await act(async () => {
      await setup.flush();
    });
    const frame = setup.captureCharFrame();
    expect(frame.toLowerCase()).toContain("connecting");
    expect(frame.length).toBeGreaterThan(0);
    act(() => setup.renderer.destroy());
  });

  test("exponential backoff capped at 30s", () => {
    expect(getBackoffDelay(0)).toBe(1000);
    expect(getBackoffDelay(1)).toBe(2000);
    expect(getBackoffDelay(4)).toBe(16000);
    expect(getBackoffDelay(5)).toBe(30000);
    expect(getBackoffDelay(10)).toBe(30000);
    expect(getBackoffDelay(6, 500)).toBe(30000);
  });

  test("active session streams standard-frame rows into the transcript", async () => {
    const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
    let router: ResponseRouter | null = null;
    type StreamingHandle = TransportHandle & {
      send: (l: string) => Promise<string>;
      push: (method: string, params: unknown) => void;
    };
    const handle: StreamingHandle = {
      close() {},
      async send(line: string): Promise<string> {
        const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
        requests.push({ method: req.method, params: req.params ?? {} });
        const result = req.method === METHODS.sessionNew ? { session_id: "s1" } : {};
        return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
      },
      push(method: string, params: unknown) {
        router?.feed(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
      },
    };
    const createStreaming = (url: string, r: ResponseRouter): TransportHandle => {
      router = r;
      queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
      return handle;
    };
    const setup = await testRender(<App createTransport={createStreaming} backoffBaseMs={60000} />, {});
    const input = getInput(setup);
    await act(async () => { await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected"));
    });
    await act(async () => {
      input.press("down");
      await setup.flush();
    });
    await act(async () => {
      input.press("enter");
      await setup.flush();
    });
    await act(async () => { await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("s1"));
    });
    await act(async () => {
      await input.type("hi");
      await setup.flush();
    });
    await act(async () => {
      input.press("enter");
      await setup.flush();
    });
    await act(async () => {
      await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
    });
    expect(requests.find((r) => r.method === METHODS.sessionRun)?.params).toEqual({
      session_id: "s1",
      content: "hi",
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("hi"));
    });
    act(() => {
      handle.push(METHODS.progress, {
        session_id: "s1",
        token: 1,
        value: { kind: "text", delta: "hello" },
      });
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("hello"));
    });
    act(() => {
      handle.push(METHODS.progress, {
        session_id: "s1",
        token: 3,
        value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } },
      });
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("in=10 out=5"));
    });
    act(() => setup.renderer.destroy());
  });

  test("kernel provider objects map to ids; absent providers keep the static fallback", () => {
    expect(
      providersFromStatus({
        default_model: "deepseek-chat",
        providers: [
          { id: "deepseek-chat", models: [] },
          { id: "opencode-go", models: [] },
        ],
      }),
    ).toEqual(["deepseek-chat", "opencode-go"]);
    expect(providersFromStatus({ default_model: "deepseek-chat" })).toEqual(
      MODEL_CONFIG.providers,
    );
    expect(providersFromStatus({})).toEqual(MODEL_CONFIG.providers);
    const rendered = renderModelList({
      default_model: "deepseek-chat",
      providers: providersFromStatus({
        providers: [{ id: "opencode-go", models: [] }],
      }),
    });
    expect(rendered).toContain("  opencode-go");
    expect(rendered).not.toContain("[object Object]");
  });
});
