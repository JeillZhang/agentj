import { describe, expect, test } from "bun:test";
import { startMockKernel } from "./mock-kernel.ts";

type MessageHandler = (data: never) => void;

type Ws = {
  send: (d: string) => void;
  close: () => void;
  on: (e: string, f: MessageHandler) => void;
  off: (e: string, f: MessageHandler) => void;
};

async function rpc(ws: Ws, id: number, method: string, params?: unknown): Promise<unknown> {
  return new Promise((resolve) => {
    const onMessage = (data: Buffer): void => {
      const msg = JSON.parse(data.toString()) as { id?: number; result?: unknown };
      if (msg.id !== id) return;
      ws.off("message", onMessage as never);
      resolve(msg.result);
    };
    ws.on("message", onMessage as never);
    ws.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
  });
}

describe("mock kernel file/list", () => {
  test("answers root and per-directory listings with git statuses", async () => {
    const k = await startMockKernel();
    const { WebSocket } = await import("ws");
    const ws = new WebSocket(k.url) as unknown as Ws;
    await new Promise<void>((resolve, reject) => {
      (ws as unknown as { on: (e: string, f: () => void) => void }).on("open", () => resolve());
      (ws as unknown as { on: (e: string, f: (err: Error) => void) => void }).on("error", (err) =>
        reject(err),
      );
    });
    await rpc(ws, 1, "initialize", { protocol_version: "0.13.0" });
    await rpc(ws, 2, "session/new", {});
    const root = (await rpc(ws, 3, "file/list", { session_id: "sess-1", depth: 1 })) as {
      entries: Array<{ name: string; kind: string; git_status: string | null }>;
      truncated: boolean;
    };
    expect(root.entries[0]).toMatchObject({ name: "src", kind: "dir", git_status: "M" });
    expect(root.entries[1]).toMatchObject({ name: "README.md", kind: "file", git_status: null });
    expect(root.truncated).toBe(false);
    const sub = (await rpc(ws, 4, "file/list", { session_id: "sess-1", path: "src", depth: 1 })) as {
      entries: Array<{ name: string; kind: string; git_status: string | null }>;
      truncated: boolean;
    };
    expect(sub.entries[0]).toMatchObject({ name: "main.rs", kind: "file", git_status: "M" });
    expect(sub.truncated).toBe(false);
    ws.close();
    await k.close();
  }, 10000);
});
