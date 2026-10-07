import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { App, getBackoffDelay } from "./app.tsx";
import type { TransportHandle } from "./app.tsx";
import { METHODS, ResponseRouter } from "@agentj/sdk";
import type { TransportStatus } from "@agentj/sdk";
import { MODEL_CONFIG, providersFromStatus, renderModelList } from "./models.tsx";
import { loadFileTreePrefs, saveFileTreePrefs } from "./filetreePrefs.ts";
import { saveSessionHistory } from "./session.tsx";
import { saveProjects } from "./sidebar.tsx";
import type { RawFileEntry } from "./files.ts";

// Every app render restores/persists TUI state under XDG_CONFIG_HOME; isolate
// each test so consecutive runs stay idempotent (see filesView.test.tsx).
let configDir: string;
let prevXdg: string | undefined;

beforeEach(() => {
  prevXdg = process.env.XDG_CONFIG_HOME;
  configDir = mkdtempSync(join(tmpdir(), "aj-app-"));
  process.env.XDG_CONFIG_HOME = configDir;
});

afterEach(() => {
  if (prevXdg === undefined) delete process.env.XDG_CONFIG_HOME;
  else process.env.XDG_CONFIG_HOME = prevXdg;
  rmSync(configDir, { recursive: true, force: true });
});

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
      else if (k === "backspace") setup.mockInput.pressBackspace();
      else if (k === "tab") setup.mockInput.pressTab();
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

type KernelCall = { method: string } & Record<string, unknown>;

const KERNEL_TREE: Record<string, RawFileEntry[]> = {
  "": [
    { name: "src", path: "src", kind: "dir", git_status: "M" },
    { name: "README.md", path: "README.md", kind: "file", git_status: null },
  ],
  src: [
    { name: "components", path: "src/components", kind: "dir", git_status: null },
    { name: "main.rs", path: "src/main.rs", kind: "file", git_status: "D" },
  ],
  "src/components": [
    { name: "Button.tsx", path: "src/components/Button.tsx", kind: "file", git_status: "A" },
  ],
};

type FakeKernel = {
  calls: KernelCall[];
  drop: (path: string) => void;
  failDepthEight: (times: number) => void;
  deferDepthEight: () => () => void;
  handle: TransportHandle & { send: (line: string) => Promise<string> };
};

// Server mode answers a string query with ranked matches + total; legacy mode
// (and the default) ignores the query and returns the depth-8 listing as v0.21.
function rankEntry(entry: RawFileEntry, query: string): number {
  const name = entry.name.toLowerCase();
  if (name.startsWith(query)) return 0;
  if (name.includes(query)) return 1;
  return 2;
}

function fakeKernel(options: { search?: "server" | "legacy"; sessionRoot?: string } = {}): FakeKernel {
  const calls: KernelCall[] = [];
  const dropped = new Set<string>();
  let depthEightFailures = 0;
  const depthEightGates: Array<PromiseWithResolvers<void>> = [];
  const live = (path: string): RawFileEntry[] =>
    (KERNEL_TREE[path] ?? []).filter((e) => !dropped.has(e.path));
  const flat = (): RawFileEntry[] => ["", "src", "src/components"].flatMap((p) => live(p));
  const handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params?: Record<string, unknown> };
      const params = req.params ?? {};
      calls.push({ method: req.method, ...params });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) {
        result = options.sessionRoot
          ? { session_id: "s1", project_root: options.sessionRoot }
          : { session_id: "s1" };
      }
      if (req.method === METHODS.sessionStatus && options.sessionRoot) {
        result = { status: "active", project_root: options.sessionRoot };
      }
      if (req.method === METHODS.fileList) {
        const depth = typeof params.depth === "number" ? params.depth : 1;
        if (depth >= 8) {
          const gate = depthEightGates.shift();
          if (gate) await gate.promise;
          if (depthEightFailures > 0) {
            depthEightFailures -= 1;
            throw new Error("index unavailable");
          }
          if (options.search === "server") {
            const query = typeof params.query === "string" ? params.query.trim().toLowerCase() : "";
            const matches = flat().filter(
              (e) => query.length > 0 && (e.name.toLowerCase().includes(query) ||
                e.path.toLowerCase().includes(query)),
            );
            matches.sort(
              (a, b) => rankEntry(a, query) - rankEntry(b, query) || a.path.localeCompare(b.path),
            );
            result = { entries: matches, total: matches.length, truncated: false };
          } else {
            result = { entries: flat(), truncated: false };
          }
        } else {
          result = { entries: live(params.path === null ? "" : String(params.path)), truncated: false };
        }
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  return {
    calls,
    drop: (path) => dropped.add(path),
    failDepthEight: (times) => { depthEightFailures = times; },
    deferDepthEight: () => {
      const gate = Promise.withResolvers<void>();
      depthEightGates.push(gate);
      return () => { gate.resolve(); };
    },
    handle,
  };
}

async function renderApp(
  kernel: FakeKernel,
  options: { width?: number; height?: number } = {},
) {
  const createTransport = (_url: string, _router: ResponseRouter): TransportHandle => {
    queueMicrotask(() => act(() => kernel.handle.onStatusChange?.("open")));
    return kernel.handle;
  };
  const setup = await testRender(
    <App createTransport={createTransport} backoffBaseMs={60000} />,
    options,
  );
  const input = getInput(setup);
  await act(async () => {
    await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected"));
  });
  return { setup, input };
}

type AppInput = ReturnType<typeof getInput>;

// The terminal parser defers a lone ESC byte until the next input arrives, so
// pressing Esc and typing immediately would deliver Alt+<char> instead; give
// the parser a real tick (same pattern as tui-flow.test.tsx).
async function pressEscape(setup: TestRendererSetup, input: AppInput): Promise<void> {
  await act(async () => {
    input.press("esc");
    await setup.flush();
    await new Promise((resolve) => setTimeout(resolve, 30));
    await setup.flush();
  });
}

// Search input debounces 150ms before issuing file/list; wait past it and flush.
async function settleDebounce(setup: TestRendererSetup): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 200));
    await setup.flush();
  });
}

// opentui's incremental diff keeps stale glyphs when a text cell shrinks to a
// space, so a same-size resize is used to force a clean repaint before reading
// hint text that changes with the sidebar view.
async function repaint(setup: TestRendererSetup): Promise<void> {
  setup.resize(setup.renderer.width, setup.renderer.height);
  await act(async () => { await setup.flush(); });
}

async function openFilesSidebar(setup: TestRendererSetup, input: AppInput): Promise<void> {
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  await act(async () => { input.press("tab"); await setup.flush(); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("README.md")); });
}

const depthEightCalls = (kernel: FakeKernel): KernelCall[] =>
  kernel.calls.filter((c) => c.method === METHODS.fileList && c.depth === 8);

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

  test("kernel provider models map to ids; absent models keep the static fallback", () => {
    expect(
      providersFromStatus({
        default_model: "opencode-go/a",
        providers: [{ id: "opencode-go", models: ["opencode-go/a", "opencode-go/b"] }],
      }),
    ).toEqual([
      { name: "opencode-go/a", provider: "opencode-go" },
      { name: "opencode-go/b", provider: "opencode-go" },
    ]);
    expect(
      providersFromStatus({ providers: [{ id: "opencode-go", models: [] }] }),
    ).toEqual(MODEL_CONFIG.providers.map(({ name }) => ({ name })));
    expect(providersFromStatus({ default_model: "deepseek-chat" })).toEqual(
      MODEL_CONFIG.providers.map(({ name }) => ({ name })),
    );
    expect(providersFromStatus({})).toEqual(
      MODEL_CONFIG.providers.map(({ name }) => ({ name })),
    );
    const rendered = renderModelList({
      default_model: "opencode-go/a",
      providers: providersFromStatus({
        providers: [{ id: "opencode-go", models: ["opencode-go/a"] }],
      }),
    });
    expect(rendered).toContain("▶ opencode-go/a");
    expect(rendered).not.toContain("[object Object]");
  });
});

// T14: the picked entry must travel into the next session/run; the kernel/status
// fixture tags each model with its provider id (design D7 — never list position).
describe("App model selection", () => {
  const MODEL_STATUS = {
    default_model: "deepseek-chat",
    providers: [
      { id: "deepseek", models: ["deepseek-chat"] },
      { id: "opencode-go", models: ["opencode-go/kimi-k3"] },
    ],
  };

  async function renderModelApp() {
    const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
    type Handle = TransportHandle & { send: (l: string) => Promise<string> };
    const handle: Handle = {
      close() {},
      async send(line: string): Promise<string> {
        const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
        requests.push({ method: req.method, params: req.params ?? {} });
        let result: unknown = {};
        if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
        if (req.method === METHODS.kernelStatus) result = MODEL_STATUS;
        return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
      },
    };
    const createHandle = (_url: string, _router: ResponseRouter): TransportHandle => {
      queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
      return handle;
    };
    const setup = await testRender(<App createTransport={createHandle} backoffBaseMs={60000} />, {});
    const input = getInput(setup);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected"));
    });
    await act(async () => { input.press("down"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
    return { setup, input, requests };
  }

  // "/models" opens the picker anchored on default_model; j moves to the tagged
  // opencode-go entry and Enter picks it.
  async function pickOpenCodeGo(setup: TestRendererSetup, input: AppInput): Promise<void> {
    await act(async () => { input.press("/"); await setup.flush(); });
    await act(async () => { await input.type("models"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { input.press("j"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
  }

  test("sends the picked model on the next run", async () => {
    const { setup, input, requests } = await renderModelApp();
    await pickOpenCodeGo(setup, input);
    await act(async () => { await input.type("x"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
    });
    const run = requests.filter((r) => r.method === METHODS.sessionRun).at(-1);
    expect(run?.params).toEqual({
      session_id: "s1",
      content: "x",
      model: "opencode-go/kimi-k3",
      provider: "opencode-go",
    });
    act(() => setup.renderer.destroy());
  });

  test("sends nothing extra when no model was picked", async () => {
    const { setup, input, requests } = await renderModelApp();
    await act(async () => { await input.type("hi"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
    });
    const run = requests.filter((r) => r.method === METHODS.sessionRun).at(-1);
    expect(run?.params).toEqual({ session_id: "s1", content: "hi" });
    act(() => setup.renderer.destroy());
  });
});

test("files view lazy-loads the tree and expands directories", async () => {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  let router: ResponseRouter | null = null;
  type Handle = TransportHandle & { send: (l: string) => Promise<string> };
  const handle: Handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
      requests.push({ method: req.method, params: req.params ?? {} });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
      if (req.method === METHODS.fileList) {
        result = req.params?.path
          ? { entries: [{ name: "main.rs", path: "src/main.rs", kind: "file", git_status: "D" }], truncated: false }
          : { entries: [
              { name: "src", path: "src", kind: "dir", git_status: "M" },
              { name: "README.md", path: "README.md", kind: "file", git_status: null },
            ], truncated: false };
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  const createHandle = (_url: string, r: ResponseRouter): TransportHandle => {
    router = r;
    queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
    return handle;
  };
  const setup = await testRender(<App createTransport={createHandle} backoffBaseMs={60000} />, {});
  const input = getInput(setup);
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected")); });
  // Tab enters sidebar focus (composer mode keeps letters in the composer);
  // with no active session yet, f opens Files with a placeholder and fires no request
  await act(async () => { input.press("tab"); await setup.flush(); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("select a session")); });
  expect(requests.some((r) => r.method === METHODS.fileList)).toBe(false);
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });   // creates s1
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.fileList));
  });
  expect(requests.find((r) => r.method === METHODS.fileList)?.params).toEqual({
    session_id: "s1", path: null, depth: 1,
  });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("src")); });
  await act(async () => { input.press("enter"); await setup.flush(); });   // expand src
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.fileList && r.params.path === "src"));
  });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
  // j selects the child row; h jumps back up to the parent dir row
  await act(async () => { input.press("j"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => /●\s+main\.rs/.test(setup.captureCharFrame())); });
  await act(async () => { input.press("h"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => /●\s*▼\s*src/.test(setup.captureCharFrame())); });
  // h on the expanded dir collapses it; l re-expands from the cached children (no new request)
  const beforeCollapse = requests.filter((r) => r.method === METHODS.fileList).length;
  await act(async () => { input.press("h"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => !setup.captureCharFrame().includes("main.rs")); });
  await act(async () => { input.press("l"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
  expect(requests.filter((r) => r.method === METHODS.fileList).length).toBe(beforeCollapse);
  // r refreshes the root and re-fetches expanded dirs
  const beforeRefresh = requests.filter((r) => r.method === METHODS.fileList).length;
  await act(async () => { input.press("r"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() =>
      requests.filter((r) => r.method === METHODS.fileList).length >= beforeRefresh + 2);
  });
  const refreshed = requests.filter((r) => r.method === METHODS.fileList).slice(beforeRefresh);
  expect(refreshed.some((r) => r.params.path === null)).toBe(true);
  expect(refreshed.some((r) => r.params.path === "src")).toBe(true);
  await act(async () => { input.press("f"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("Threads")); });
  act(() => setup.renderer.destroy());
  void router;
});

test("BUG-009: typing in the composer never triggers sidebar/file shortcuts", async () => {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  let router: ResponseRouter | null = null;
  type Handle = TransportHandle & { send: (l: string) => Promise<string> };
  const handle: Handle = {
    close() {},
    async send(line: string): Promise<string> {
      const req = JSON.parse(line) as { id: number; method: string; params: Record<string, unknown> };
      requests.push({ method: req.method, params: req.params ?? {} });
      let result: unknown = {};
      if (req.method === METHODS.sessionNew) result = { session_id: "s1" };
      if (req.method === METHODS.fileList) {
        result = { entries: [{ name: "src", path: "src", kind: "dir", git_status: "M" }], truncated: false };
      }
      return JSON.stringify({ jsonrpc: "2.0", id: req.id, result });
    },
  };
  const createHandle = (_url: string, r: ResponseRouter): TransportHandle => {
    router = r;
    queueMicrotask(() => act(() => handle.onStatusChange?.("open")));
    return handle;
  };
  const setup = await testRender(<App createTransport={createHandle} backoffBaseMs={60000} />, {});
  const input = getInput(setup);
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().toLowerCase().includes("connected")); });
  // create an active session so the composer is visible (SessionList: down + enter)
  await act(async () => { input.press("down"); await setup.flush(); });
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
  expect(setup.captureCharFrame()).toContain("Threads");
  // typing "of" (contains f) must not toggle the sidebar to Files
  await act(async () => { await input.type("of"); await setup.flush(); });
  let frame = setup.captureCharFrame();
  expect(frame).toContain("Threads");
  expect(frame).not.toContain("Files");
  expect(frame).toContain("of");
  expect(requests.filter((r) => r.method === METHODS.fileList)).toHaveLength(0);
  // typing " for" (f would toggle, r would refresh) must not switch views or fetch files
  await act(async () => { await input.type(" for"); await setup.flush(); });
  frame = setup.captureCharFrame();
  expect(frame).toContain("Threads");
  expect(frame).not.toContain("Files");
  expect(frame).toContain("of for");
  expect(requests.filter((r) => r.method === METHODS.fileList)).toHaveLength(0);
  // typing "need" (contains n) must not create another session
  await act(async () => { await input.type(" need"); await setup.flush(); });
  expect(requests.filter((r) => r.method === METHODS.sessionNew)).toHaveLength(1);
  // Enter submits the full composer text unchanged
  await act(async () => { input.press("enter"); await setup.flush(); });
  await act(async () => {
    await setup.waitFor(() => requests.some((r) => r.method === METHODS.sessionRun));
  });
  expect(requests.find((r) => r.method === METHODS.sessionRun)?.params).toEqual({
    session_id: "s1",
    content: "of for need",
  });
  act(() => setup.renderer.destroy());
  void router;
});

describe("files search mode", () => {
  test("s enters search, printables filter, r rebuilds, enter jumps", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    expect(depthEightCalls(kernel)).toHaveLength(0); // no upfront index fetch
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await input.type("fnc"); await setup.flush(); });
    expect(depthEightCalls(kernel)).toHaveLength(0); // f/n/c stay query text, debounce pending
    expect(kernel.calls.filter((c) => c.method === METHODS.sessionNew)).toHaveLength(1);
    await settleDebounce(setup);
    expect(depthEightCalls(kernel)).toHaveLength(1);
    expect(depthEightCalls(kernel)[0]).toMatchObject({ path: null, depth: 8, query: "fnc" });
    expect(setup.captureCharFrame()).toContain("Files");
    expect(setup.captureCharFrame()).not.toContain("Threads");
    await pressEscape(setup, input); // clears the query
    await act(async () => {
      await input.type("main");
      input.press("r"); // rebuilds the index before the debounce fires
      await setup.flush();
    });
    expect(depthEightCalls(kernel)).toHaveLength(2);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    const callsBeforeJump = kernel.calls.filter((c) => c.method === METHODS.fileList).length;
    await act(async () => { input.press("enter"); await setup.flush(); }); // jump
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await setup.waitFor(() => /●\s+main\.rs/.test(setup.captureCharFrame())); });
    expect(kernel.calls).toContainEqual(
      expect.objectContaining({ method: METHODS.fileList, path: "src", depth: 1 }),
    );
    expect(kernel.calls.filter((c) => c.method === METHODS.fileList).length).toBeGreaterThan(callsBeforeJump);
    expect(setup.captureCharFrame()).not.toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("enter on a directory result expands and selects it", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("components"); await setup.flush(); });
    await settleDebounce(setup);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("components"));
    });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { await setup.waitFor(() => /●\s*▼\s*components/.test(setup.captureCharFrame())); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("Button.tsx")); });
    expect(kernel.calls).toContainEqual(
      expect.objectContaining({ method: METHODS.fileList, path: "src/components", depth: 1 }),
    );
    act(() => setup.renderer.destroy());
  });

  test("esc clears a non-empty query before it exits search", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("ab"); await setup.flush(); });
    const before = depthEightCalls(kernel).length;
    await pressEscape(setup, input);
    // f and r would toggle the view / refresh the tree unless search stayed active
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => depthEightCalls(kernel).length > before);
    });
    expect(setup.captureCharFrame()).not.toContain("Threads");
    await pressEscape(setup, input); // clears "f"
    await pressEscape(setup, input); // now exits
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("Threads"));
    });
    act(() => setup.renderer.destroy());
  });

  test("r retries the index after a failed search request", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    kernel.failDepthEight(1);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("main"); await setup.flush(); });
    await settleDebounce(setup); // the debounced query request fails
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("index unavailable"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(1);
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("index unavailable"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(2);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    act(() => setup.renderer.destroy());
  });

  test("keeps the query typed while the index request is in flight", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    const release = kernel.deferDepthEight();
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("main"); await setup.flush(); });
    await settleDebounce(setup);
    expect(depthEightCalls(kernel)).toHaveLength(1); // the debounced reply is gated
    // the depth-8 reply is unresolved: every keystroke must stay in the query
    await act(async () => { await input.type("x"); await setup.flush(); });
    await act(async () => { release(); await setup.flush(); });
    await settleDebounce(setup); // the follow-up debounce must query the typed "mainx"
    const queried = (query: string): number =>
      kernel.calls.filter((c) => c.method === METHODS.fileList && c.query === query).length;
    expect(queried("mainx")).toBe(1);
    expect(queried("main")).toBe(1);
    // the stale reply ("main") must not overwrite rows for the typed "mainx"
    await repaint(setup);
    expect(setup.captureCharFrame()).not.toContain("main.rs");
    expect(setup.captureCharFrame()).toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("keeps the failed-index error while typing and clears it only after r", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    kernel.failDepthEight(1);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("main"); await setup.flush(); });
    await settleDebounce(setup); // the debounced query request fails
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("index unavailable"));
    });
    // C2: while the error is up, keystrokes filter locally and must not hide it
    await act(async () => { await input.type("x"); await setup.flush(); });
    await settleDebounce(setup);
    await repaint(setup);
    expect(setup.captureCharFrame()).toContain("index unavailable");
    expect(depthEightCalls(kernel)).toHaveLength(1); // errored: typing issues no request
    await act(async () => { input.press("backspace"); await setup.flush(); });
    await act(async () => { input.press("r"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("index unavailable"));
    });
    expect(depthEightCalls(kernel)).toHaveLength(2);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    act(() => setup.renderer.destroy());
  });

  test("jump loads ancestors depth-first and selects the nested target", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("Button"); await setup.flush(); });
    await settleDebounce(setup);
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("Button"));
    });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => /●\s+Button\.tsx/.test(setup.captureCharFrame()));
    });
    const treeCalls = kernel.calls
      .filter((c) => c.method === METHODS.fileList && c.depth === 1)
      .map((c) => c.path);
    expect(treeCalls).toEqual([null, "src", "src/components"]);
    expect(setup.captureCharFrame()).not.toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("a stale search target exits, refreshes the root and reports once", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); }); // search starts with no index
    await act(async () => { await input.type("main"); await setup.flush(); });
    await settleDebounce(setup); // the reply's snapshot has main.rs
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    kernel.drop("src/main.rs"); // deleted after indexing: reveal must notice
    const rootCalls = () =>
      kernel.calls.filter((c) => c.method === METHODS.fileList && c.depth === 1 && c.path === null).length;
    const before = rootCalls();
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => rootCalls() > before); });
    await act(async () => {
      await setup.waitFor(() => !setup.captureCharFrame().includes("type filter"));
    });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("path no longer exists"));
    });
    expect(setup.captureCharFrame()).toContain("path no longer exists");
    act(() => setup.renderer.destroy());
  });

  test("enter on an empty result set keeps search mode open", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("zzz"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    expect(setup.captureCharFrame()).toContain("type filter");
    act(() => setup.renderer.destroy());
  });

  test("sidebar advertises the search hint while searching and restores it after", async () => {
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await repaint(setup);
    // Files view with sidebar focus advertises the normal shortcut hint
    expect(setup.captureCharFrame()).toContain("s search");
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("esc back"));
    });
    await repaint(setup);
    // search mode must replace the shortcut hint in the sidebar header
    expect(setup.captureCharFrame()).toContain("esc back");
    expect(setup.captureCharFrame()).not.toContain("s search");
    await pressEscape(setup, input); // empty query exits search
    await repaint(setup);
    expect(setup.captureCharFrame()).toContain("s search");
    expect(setup.captureCharFrame()).not.toContain("esc back");
    act(() => setup.renderer.destroy());
  });

  test("persists expansion under the __default__ fallback, not the local project id", async () => {
    saveProjects([{ id: "p1", name: "Foo" }]);
    const kernel = fakeKernel();
    const { setup, input } = await renderApp(kernel);
    await act(async () => { input.press("down"); await setup.flush(); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("s1")); });
    await act(async () => { input.press("tab"); await setup.flush(); });
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("README.md")); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("main.rs")); });
    expect(loadFileTreePrefs().expanded.__default__).toEqual(["src"]);
    expect(loadFileTreePrefs().expanded.p1).toBeUndefined();
    act(() => setup.renderer.destroy());
  });

  test("keys filetree prefs by the session project_root", async () => {
    const kernel = fakeKernel({ sessionRoot: "/tmp/proj" });
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);                                  // creates the session
    await act(async () => { input.press("enter"); await setup.flush(); }); // expand src
    expect(loadFileTreePrefs().expanded["/tmp/proj"]).toEqual(["src"]);
    act(() => setup.renderer.destroy());
  });

  test("falls back to __default__ when the kernel omits project_root", async () => {
    const kernel = fakeKernel(); // no project_root in session/new
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("enter"); await setup.flush(); });
    expect(loadFileTreePrefs().expanded.__default__).toEqual(["src"]);
    act(() => setup.renderer.destroy());
  });

  test("prunes stale project-id keys on save while keeping root keys", async () => {
    saveFileTreePrefs({ expanded: { p1: ["src"], "/old/root": ["x"] } }, ["p1", "/old/root"]);
    const kernel = fakeKernel({ sessionRoot: "/tmp/proj" });
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("enter"); await setup.flush(); });
    const expanded = loadFileTreePrefs().expanded;
    expect(expanded.p1).toBeUndefined();
    expect(expanded["/old/root"]).toEqual(["x"]);
    expect(expanded["/tmp/proj"]).toEqual(["src"]);
    act(() => setup.renderer.destroy());
  });

  test("continue captures project_root from session/status for the prefs key", async () => {
    saveSessionHistory(["s9"]);
    const kernel = fakeKernel({ sessionRoot: "/tmp/proj2" });
    const { setup, input } = await renderApp(kernel);
    await act(async () => { input.press("enter"); await setup.flush(); }); // select s9
    await act(async () => { input.press("tab"); await setup.flush(); });
    await act(async () => { input.press("f"); await setup.flush(); });
    await act(async () => { await setup.waitFor(() => setup.captureCharFrame().includes("README.md")); });
    await act(async () => { input.press("enter"); await setup.flush(); });
    const expanded = loadFileTreePrefs().expanded;
    expect(kernel.calls).toContainEqual(
      expect.objectContaining({ method: METHODS.sessionStatus, session_id: "s9" }),
    );
    expect(kernel.calls.some((c) => c.method === METHODS.sessionNew)).toBe(false);
    expect(expanded["/tmp/proj2"]).toEqual(["src"]);
    expect(expanded.__default__).toBeUndefined();
    act(() => setup.renderer.destroy());
  });

  test("server search debounces, renders ranked rows and shows total", async () => {
    const kernel = fakeKernel({ search: "server" });
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("mai"); await setup.flush(); });
    await settleDebounce(setup);
    const queryCalls = kernel.calls.filter(
      (c) => c.method === METHODS.fileList && c.query === "mai",
    );
    expect(queryCalls).toHaveLength(1); // debounced to a single request
    expect(queryCalls[0]).toMatchObject({ path: null, depth: 8 });
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    // the server-ranked reply drops non-matches (total itself rides in state)
    expect(setup.captureCharFrame()).not.toContain("README.md");
    act(() => setup.renderer.destroy());
  });

  test("falls back to the local index when the kernel omits total", async () => {
    const kernel = fakeKernel({ search: "legacy" });
    const { setup, input } = await renderApp(kernel);
    await openFilesSidebar(setup, input);
    await act(async () => { input.press("s"); await setup.flush(); });
    await act(async () => { await input.type("main"); await setup.flush(); });
    await settleDebounce(setup);
    const withQuery = (): number =>
      kernel.calls.filter(
        (c) => c.method === METHODS.fileList && typeof c.query === "string",
      ).length;
    expect(withQuery()).toBe(1); // first reply (no total) becomes the local index
    await act(async () => {
      await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
    });
    await act(async () => { await input.type("."); await setup.flush(); });
    await settleDebounce(setup);
    expect(withQuery()).toBe(1); // local filtering issues no further requests
    act(() => setup.renderer.destroy());
  });
});
