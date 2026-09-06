import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import { App, getBackoffDelay } from "./app.tsx";
import type { TransportHandle } from "./app.tsx";

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
});
