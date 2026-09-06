import { describe, expect, test, mock } from "bun:test";
import { ConnectionManager } from "./connection.ts";
import type { TransportHandle } from "./connection.ts";
import type { ResponseRouter } from "@agentj/sdk";

describe("B-005 TUI ConnectionManager 403 auth error should not reconnect", () => {
  test("403 still reconnects: closed with 403 should NOT scheduleReconnect", async () => {
    let createCalls = 0;
    const create = mock((_: string, __: ResponseRouter) => {
      createCalls += 1;
      const h = {
        close() {},
        lastError: "403 Forbidden: NotAuthorized",
      } as unknown as TransportHandle;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    const origSetTimeout = globalThis.setTimeout;
    let timeoutCalls = 0;
    const fakeSetTimeout = ((fn: TimerHandler, ms?: number) => {
      timeoutCalls += 1;
      return origSetTimeout(fn, ms);
    }) as unknown as typeof setTimeout;
    globalThis.setTimeout = fakeSetTimeout;
    mgr.connect();
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      origSetTimeout(resolve, 15);
      await promise;
    }
    const mgrWithPrivate = mgr as unknown as { attempt: number; timer: unknown };
    const attempt = mgrWithPrivate.attempt;
    const timer = mgrWithPrivate.timer;
    globalThis.setTimeout = origSetTimeout;
    expect(timeoutCalls).toBe(0);
    expect(attempt).toBe(0);
    expect(timer).toBeNull();
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      origSetTimeout(resolve, 30);
      await promise;
    }
    expect(createCalls).toBe(1);
    mgr.destroy();
    globalThis.setTimeout = origSetTimeout;
  });

  test("closed with -32806 should NOT scheduleReconnect", async () => {
    let createCalls = 0;
    const create = mock((_: string, __: ResponseRouter) => {
      createCalls += 1;
      const h = {
        close() {},
        lastError: "NotAuthorized -32806 token invalid",
      } as unknown as TransportHandle;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    const origSetTimeout = globalThis.setTimeout;
    let timeoutCalls = 0;
    const fakeSetTimeout = ((fn: TimerHandler, ms?: number) => {
      timeoutCalls += 1;
      return origSetTimeout(fn, ms);
    }) as unknown as typeof setTimeout;
    globalThis.setTimeout = fakeSetTimeout;
    mgr.connect();
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      origSetTimeout(resolve, 15);
      await promise;
    }
    const mgrWithPrivate = mgr as unknown as { attempt: number };
    const attempt = mgrWithPrivate.attempt;
    globalThis.setTimeout = origSetTimeout;
    expect(timeoutCalls).toBe(0);
    expect(attempt).toBe(0);
    expect(createCalls).toBe(1);
    mgr.destroy();
  });

  test("closed without auth error SHOULD scheduleReconnect", async () => {
    let current: TransportHandle | null = null;
    const create = mock((_: string, __: ResponseRouter) => {
      const h = {
        close() {},
      } as unknown as TransportHandle;
      current = h;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    mgr.connect();
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      setTimeout(resolve, 5);
      await promise;
    }
    const t = current as unknown as TransportHandle;
    const origSetTimeout = globalThis.setTimeout;
    let timeoutCalls = 0;
    const fakeSetTimeout = ((fn: TimerHandler, ms?: number) => {
      timeoutCalls += 1;
      return origSetTimeout(fn, ms);
    }) as unknown as typeof setTimeout;
    globalThis.setTimeout = fakeSetTimeout;
    t.onStatusChange?.("closed");
    {
      const { promise, resolve } = Promise.withResolvers<void>();
      origSetTimeout(resolve, 5);
      await promise;
    }
    globalThis.setTimeout = origSetTimeout;
    const mgrWithPrivate = mgr as unknown as { attempt: number };
    const attempt = mgrWithPrivate.attempt;
    expect(timeoutCalls).toBe(1);
    expect(attempt).toBe(1);
    mgr.destroy();
  });
});
