import { createServer } from "node:http";
import { WebSocketServer, WebSocket } from "ws";
import { SEARCH_RANK_CASES, type FixtureEntry } from "../searchRankCases";

export type MockCall = { method: string; [key: string]: unknown };

export type MockKernel = {
  url: string;
  close: () => Promise<void>;
  calls: MockCall[];
};

export type MockKernelOptions = {
  /** Force the v0.21 reply shape: `query` is ignored and no `total` is returned. */
  legacySearch?: boolean;
};

// Flat depth=8 index served for search mode: nested paths as plain entries.
const SEARCH_FIXTURE = [
  { name: "src", path: "src", kind: "dir", git_status: "M" },
  { name: "main.rs", path: "src/main.rs", kind: "file", git_status: "M" },
  { name: "README.md", path: "README.md", kind: "file", git_status: null },
  { name: "docs", path: "docs", kind: "dir", git_status: null },
  { name: "readme.md", path: "docs/readme.md", kind: "file", git_status: "A" },
];

type ListingEntry = (typeof SEARCH_FIXTURE)[number];

// Search replies replay the shared Rust-pinned conformance fixture
// (protocol/search-rank-cases.json) instead of re-deriving the rank rule.
type RankedEntry = { name: string; path: string; kind: "file" | "directory" };
type WireEntry = { name: string; path: string; kind: "file" | "dir" };

// The fixture speaks its own vocabulary (`"file" | "directory"`); the protocol
// wire enum spells directories `"dir"`. Translate before replying.
export function fixtureKindToWire(kind: RankedEntry["kind"]): WireEntry["kind"] {
  return kind === "directory" ? "dir" : "file";
}

// The fixture's `searches[]` is the single source of the ranking rule.
function searchFixture(query: string): { entries: WireEntry[]; total: number } {
  const contract = SEARCH_RANK_CASES.searches.find((c) => c.query === query);
  if (!contract) return { entries: [], total: 0 };
  const byPath = new Map<string, FixtureEntry>(contract.entries.map((e) => [e.path, e]));
  const entries = contract.expected_ranked_paths
    .map((path) => byPath.get(path))
    .filter((e): e is FixtureEntry => e !== undefined)
    .map((e) => ({ name: e.name, path: e.path, kind: fixtureKindToWire(e.kind) }));
  return { entries, total: contract.expected_total };
}

type Pending = {
  id: number;
  method: string;
};

export async function startMockKernel(options: MockKernelOptions = {}): Promise<MockKernel> {
  const legacySearch = options.legacySearch ?? false;
  const server = createServer();
  const wss = new WebSocketServer({ noServer: true });
  const calls: MockCall[] = [];

  server.on("upgrade", (req, socket, head) => {
    const url = req.url ?? "";
    if (url.startsWith("/ws")) {
      wss.handleUpgrade(req, socket, head, (ws) => wss.emit("connection", ws, req));
    } else {
      socket.destroy();
    }
  });

  wss.on("connection", (ws: WebSocket) => {
    ws.on("message", (data: Buffer) => {
      let msg: { id?: number; method?: string; params?: Record<string, unknown> };
      try {
        msg = JSON.parse(data.toString()) as typeof msg;
      } catch {
        return;
      }
      calls.push({ method: msg.method ?? "", ...(msg.params ?? {}) });
      handleMessage(ws, msg, legacySearch);
    });
  });

  await new Promise<void>((resolve) => server.listen(0, resolve));
  const addr = server.address() as { port: number };
  const url = `ws://127.0.0.1:${addr.port}/ws`;

  const close = async (): Promise<void> => {
    wss.close();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  };

  return { url, close, calls };
}

function reply(ws: WebSocket, id: number | undefined, result: unknown): void {
  ws.send(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function handleInitialize(ws: WebSocket, id: number | undefined): void {
  reply(ws, id, {
    protocol_version: "0.12.0",
    connection_id: "c1",
    capabilities: { multi_client: true },
    server_info: { name: "mock", version: "0.12.0" },
  });
}

function handleSessionRun(ws: WebSocket, msg: { id?: number; params?: Record<string, unknown> }): void {
  const sidRaw = msg.params?.["session_id"] as unknown as string | undefined;
  const sid = sidRaw ?? "sess-1";
  reply(ws, msg.id, { status: "ok" });
  scheduleRunFlow(ws, sid);
}

function handlePermissionRespond(ws: WebSocket, id: number | undefined): void {
  reply(ws, id, { ok: true });
  setTimeout(() => {
    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        method: "$/progress",
        params: { session_id: "sess-1", token: 2, value: { kind: "text", delta: "final chunk" } },
      }),
    );
    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        method: "$/progress",
        params: {
          session_id: "sess-1",
          token: 3,
          value: { kind: "usage", usage: { prompt_tokens: 10, completion_tokens: 5 } },
        },
      }),
    );
  }, 20);
}

function handleFileList(
  ws: WebSocket,
  msg: { id?: number; params?: Record<string, unknown> },
  legacySearch: boolean,
): void {
  const rawQuery = msg.params?.["query"];
  if (typeof rawQuery === "string" && !legacySearch) {
    const query = rawQuery.trim().toLowerCase();
    if (query) {
      const { entries, total } = searchFixture(query);
      reply(ws, msg.id, { entries, total, truncated: false });
      return;
    }
  }
  const depthRaw = msg.params?.["depth"];
  const depth = typeof depthRaw === "number" ? depthRaw : 1;
  if (depth === 8) {
    reply(ws, msg.id, { entries: SEARCH_FIXTURE, truncated: false });
    return;
  }
  const path = (msg.params?.["path"] as string | null | undefined) ?? null;
  const entries = path
    ? [{ name: "main.rs", path: "src/main.rs", kind: "file", git_status: "M" }]
    : [
        { name: "src", path: "src", kind: "dir", git_status: "M" },
        { name: "README.md", path: "README.md", kind: "file", git_status: null },
      ];
  reply(ws, msg.id, { entries, truncated: false });
}

function handleMessage(
  ws: WebSocket,
  msg: { id?: number; method?: string; params?: Record<string, unknown> },
  legacySearch: boolean,
): void {
  if (msg.method === "initialize") {
    handleInitialize(ws, msg.id);
    return;
  }
  if (msg.method === "session/new") {
    reply(ws, msg.id, { session_id: "sess-1" });
    return;
  }
  if (msg.method === "session/subscribe") {
    reply(ws, msg.id, { ok: true });
    return;
  }
  if (msg.method === "session/status") {
    reply(ws, msg.id, { session_id: "sess-1" });
    return;
  }
  if (msg.method === "session/run") {
    handleSessionRun(ws, msg);
    return;
  }
  if (msg.method === "permission/respond") {
    handlePermissionRespond(ws, msg.id);
    return;
  }
  if (msg.method === "file/list") {
    handleFileList(ws, msg, legacySearch);
    return;
  }
  if (msg.id !== undefined) reply(ws, msg.id, {});
}

function scheduleRunFlow(ws: WebSocket, sid: string): void {
  setTimeout(() => {
    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        method: "$/progress",
        params: { session_id: sid, token: 1, value: { kind: "text", delta: "hello progress" } },
      }),
    );
  }, 15);
  setTimeout(() => {
    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        method: "$/permissionRequest",
        params: {
          request_id: "req-1",
          tool: "bash",
          args: { command: "ls" },
          session_id: sid,
        },
      }),
    );
  }, 30);
}

async function openWs(url: string): Promise<WebSocket> {
  const ws = new WebSocket(url);
  await new Promise<void>((resolve, reject) => {
    ws.on("open", resolve);
    ws.on("error", reject);
  });
  return ws;
}

type PendingMap = Map<number, (msg: unknown) => void>;

export interface DoneHandle {
  promise: Promise<void>;
  resolve: () => void;
  reject: (reason?: unknown) => void;
}

function attachHandlers(
  ws: WebSocket,
  pending: PendingMap,
  events: string[],
  done: DoneHandle,
  opts: { approve?: string; onFrame?: (params: Record<string, unknown>) => void },
  getNextId: () => number,
): void {
  ws.on("message", (data: Buffer) => {
    let msg: { id?: number | null; method?: string; params?: Record<string, unknown> };
    try {
      msg = JSON.parse(data.toString()) as typeof msg;
    } catch {
      return;
    }
    if (msg.id !== undefined && msg.id !== null && pending.has(msg.id as number)) {
      const cb = pending.get(msg.id as number);
      pending.delete(msg.id as number);
      cb?.(msg);
      return;
    }
    if (msg.method === "$/progress") {
      if (!events.includes("progress")) events.push("progress");
      const p = (msg.params ?? {}) as { value?: { kind?: string } };
      opts.onFrame?.(msg.params as Record<string, unknown>);
      if (p.value?.kind === "usage") {
        if (!events.includes("done")) events.push("done");
        done.resolve();
      }
      return;
    }
    if (msg.method === "$/permissionRequest") {
      events.push("permission");
      const rid = getNextId();
      const rawParams = msg.params as unknown as Record<string, unknown>;
      const requestId = rawParams["request_id"];
      const payload = JSON.stringify({
        jsonrpc: "2.0",
        id: rid,
        method: "permission/respond",
        params: {
          request_id: requestId,
          decision: opts.approve ?? "allow",
        },
      });
      ws.send(payload);
      events.push("respond");
      pending.set(rid, () => {});
    }
  });
}

function createRequester(ws: WebSocket, pending: PendingMap, nextRef: { id: number }) {
  return (method: string, params: Record<string, unknown>): Promise<unknown> => {
    const id = nextRef.id++;
    const line = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    const { promise, resolve } = Promise.withResolvers<unknown>();
    pending.set(id, resolve as (msg: unknown) => void);
    ws.send(line);
    return promise;
  };
}

export async function driveRun(
  url: string,
  prompt: string,
  opts: { approve?: string; onFrame?: (params: Record<string, unknown>) => void } = {},
): Promise<string[]> {
  const events: string[] = [];
  const ws = await openWs(url);
  const pending: PendingMap = new Map();
  const done = Promise.withResolvers<void>();
  const nextRef = { id: 1 };
  attachHandlers(ws, pending, events, done, opts, () => nextRef.id++);
  const request = createRequester(ws, pending, nextRef);
  await request("initialize", { protocol_version: "0.12.0" });
  const newRes = (await request("session/new", {})) as {
    result?: { session_id?: string };
  };
  const sidRaw = (newRes as unknown as { session_id?: string }).session_id ?? "sess-1";
  const sid = typeof sidRaw === "string" ? sidRaw : "sess-1";
  events.push("run");
  const runRes = request("session/run", { session_id: sid, content: prompt });
  const timeout = setTimeout(() => done.resolve(), 3000);
  await done.promise;
  clearTimeout(timeout);
  await runRes;
  ws.close();
  if (!events.includes("done")) events.push("done");
  return events;
}
