import { describe, expect, test } from "bun:test";
import { startMockKernel } from "./mock-kernel.ts";
import { SEARCH_RANK_CASES } from "../searchRankCases";

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

async function openWs(url: string): Promise<Ws> {
  const { WebSocket } = await import("ws");
  const ws = new WebSocket(url) as unknown as Ws;
  await new Promise<void>((resolve, reject) => {
    (ws as unknown as { on: (e: string, f: () => void) => void }).on("open", () => resolve());
    (ws as unknown as { on: (e: string, f: (err: Error) => void) => void }).on("error", (err) =>
      reject(err),
    );
  });
  return ws;
}

describe("mock kernel file/list", () => {
  test("answers root and per-directory listings with git statuses", async () => {
    const k = await startMockKernel();
    const ws = await openWs(k.url);
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

  test("depth 8 returns the flat search fixture", async () => {
    const k = await startMockKernel();
    const ws = await openWs(k.url);
    await rpc(ws, 1, "initialize", { protocol_version: "0.13.0" });
    const res = (await rpc(ws, 2, "file/list", { session_id: "s1", depth: 8 })) as {
      entries: Array<{ path: string; kind: string; git_status: string | null }>;
      truncated: boolean;
    };
    const paths = res.entries.map((e) => e.path);
    expect(paths).toContain("src/main.rs");
    expect(paths).toContain("docs/readme.md");
    const listCall = k.calls.find((c) => c.method === "file/list");
    expect(listCall).toMatchObject({ depth: 8, session_id: "s1" });
    expect(res.entries.some((e) => e.kind === "dir")).toBe(true);
    expect(res.truncated).toBe(false);
    ws.close();
    await k.close();
  }, 10000);

  test("answers query searches with ranked matches and total", async () => {
    const k = await startMockKernel();
    const ws = await openWs(k.url);
    try {
      await rpc(ws, 1, "initialize", { protocol_version: "0.14.0" });
      await rpc(ws, 2, "session/new", {});
      const reply = (await rpc(ws, 3, "file/list", {
        session_id: "sess-1",
        path: null,
        depth: 8,
        query: "main",
      })) as { entries: Array<{ path: string }>; total?: number; truncated: boolean };
      expect(reply.total).toBeGreaterThan(0);
      expect(reply.entries[0].path).toContain("main");
      expect(reply.truncated).toBe(false);
    } finally {
      ws.close();
      await k.close();
    }
  }, 10000);

  test("legacy mode ignores query and omits total", async () => {
    const k = await startMockKernel({ legacySearch: true });
    const ws = await openWs(k.url);
    try {
      await rpc(ws, 1, "initialize", { protocol_version: "0.14.0" });
      await rpc(ws, 2, "session/new", {});
      const reply = (await rpc(ws, 3, "file/list", {
        session_id: "sess-1",
        path: null,
        depth: 8,
        query: "main",
      })) as { entries: Array<{ path: string }>; total?: number; truncated: boolean };
      expect(reply.total).toBeUndefined();
      const paths = reply.entries.map((e) => e.path);
      expect(paths).toContain("src/main.rs");
      expect(paths).toContain("docs/readme.md");
      expect(reply.truncated).toBe(false);
    } finally {
      ws.close();
      await k.close();
    }
  }, 10000);

  test("replays the shared conformance fixture instead of its own ranking rule", async () => {
    const kernel = await startMockKernel();
    const ws = await openWs(kernel.url);
    try {
      let id = 1;
      for (const searchCase of SEARCH_RANK_CASES.searches) {
        const reply = (await rpc(ws, id++, "file/list", {
          session_id: "s1",
          path: null,
          depth: 8,
          query: searchCase.query,
        })) as { entries?: Array<{ path: string }>; total?: number };
        expect(reply.entries?.map((e) => e.path)).toEqual(searchCase.expected_ranked_paths);
        expect(reply.total).toBe(searchCase.expected_total);
      }
    } finally {
      ws.close();
      await kernel.close();
    }
  });

  test("answers a query absent from the fixture with an empty result and total 0", async () => {
    const kernel = await startMockKernel();
    const ws = await openWs(kernel.url);
    try {
      const reply = (await rpc(ws, 1, "file/list", {
        session_id: "s1",
        path: null,
        depth: 8,
        query: "zzz-absent-from-fixture",
      })) as { entries?: unknown[]; total?: number };
      expect(reply.entries).toEqual([]);
      expect(reply.total).toBe(0);
    } finally {
      ws.close();
      await kernel.close();
    }
  });
});
